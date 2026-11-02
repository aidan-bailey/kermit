//! Binary Hash Join: the left-deep, pipelined hash join of Free Join §2.2.
//!
//! The baseline that worst-case optimal joins are measured against. A binary
//! plan joins one atom at a time. Written as a left-deep plan `[R_1, …, R_m]`
//! it is executed by pipelining (Wang et al., "Free Join: Unifying
//! Worst-Case Optimal and Traditional Joins", PACMMOD 2023, §2.2, Figure 2a):
//! the engine iterates over each tuple of the first atom, probes the hash
//! table of the second atom on the variables the first binds, and for each
//! match probes the third, and so on. Nothing intermediate is stored. For
//! the triangle `Q(x, y, z) :- R(x, y), S(y, z), T(x, z)`:
//!
//! ```text
//! for (x, y) in R:
//!     for (y, z) in S[y]:        // a failed lookup continues the loop above
//!         for (x, z) in T[x, z]:
//!             output(x, y, z)
//! ```
//!
//! The work is proportional to the sum of the intermediate results (on the
//! triangle, `|R ⋈ S|` probes into `T`), which can exceed the AGM bound that
//! worst-case optimal joins meet. That gap is what this algorithm is in
//! kermit to measure.
//!
//! **Where the join order comes from.** The executed query's body is already
//! in join order, and each atom's index is keyed on the variables the atoms
//! before it bind: `kermit_algos::plan_indexes` under
//! [`TrieLevels::ProbeKeys`](kermit_iters::TrieLevels::ProbeKeys) derives both
//! from the optimiser's variable order (the `join_order` rule of
//! `docs/algorithms/binary-hash-join.md`). This module derives nothing: step
//! `j` is body atom `j`.

use {
    crate::{analysis::analyse, join_algo::JoinAlgo, optimiser::QueryPlan},
    kermit_iters::KeyedIterable,
    kermit_parser::JoinQuery,
    std::collections::HashMap,
};

/// One level of the loop nest: a body atom, probed on the variables the
/// atoms before it bind.
struct Step<'a, DS> {
    /// The atom's index. Its first `key_len` columns are the probe key.
    index: &'a DS,
    /// The atom's variables (canonical indices), one per column, in column
    /// order (the paper's `x_j`).
    variables: &'a [usize],
    /// How many leading columns form the key: `index.key_len()`.
    key_len: usize,
}

impl<'a, DS: KeyedIterable> Step<'a, DS> {
    /// The step for the atom `name`, the `position`-th of the body, with
    /// `bound[v]` recording the variables the steps before it bind. Marks the
    /// variables this step binds.
    ///
    /// # Panics
    ///
    /// If the atom's index is not keyed on exactly the variables bound
    /// before it: the invariant "probe keys lead the columns". A key column
    /// that no earlier atom binds would probe with a stale value, and a
    /// column past the key that an earlier atom or column binds already would
    /// overwrite a bound variable instead of joining on it.
    fn new(
        position: usize, name: &str, index: &'a DS, variables: &'a [usize], bound: &mut [bool],
    ) -> Self {
        let key_len = index.key_len();
        assert!(
            key_len <= variables.len() && variables[..key_len].iter().all(|&v| bound[v]),
            "probe keys lead the columns: atom {position} ({name}) is keyed on its first \
             {key_len} of {} columns, but the atoms before it do not bind them all",
            variables.len()
        );
        for &variable in &variables[key_len..] {
            assert!(
                !bound[variable],
                "probe keys lead the columns: atom {position} ({name}) carries variable \
                 {variable} past its key, but an earlier atom or column already binds it"
            );
            bound[variable] = true;
        }
        Self {
            index,
            variables,
            key_len,
        }
    }
}

/// Free Join Figure 2a's loop nest from step `steps[0]` on, for the prefix
/// `row` binds so far.
///
/// Probes the first step's index on the key `row` holds for its leading
/// variables, writes each match's remaining columns into `row`, and descends
/// into the later steps. Past the last step every variable is bound and `row`
/// is emitted. `keys` is scratch, `key_len` slots per step, so the recursion
/// allocates nothing: this level's key is the front of `keys`, kept intact
/// while the deeper levels use the rest.
fn descend<DS: KeyedIterable, S: FnMut(&[usize])>(
    steps: &[Step<'_, DS>], row: &mut [usize], keys: &mut [usize], emit: &mut S,
) {
    let Some((step, later)) = steps.split_first() else {
        emit(row);
        return;
    };
    let (key, later_keys) = keys.split_at_mut(step.key_len);
    for (slot, &variable) in key.iter_mut().zip(step.variables) {
        *slot = row[variable];
    }
    let key: &[usize] = key;
    for tuple in step.index.probe(key) {
        for (&variable, &value) in step.variables[step.key_len..]
            .iter()
            .zip(&tuple[step.key_len..])
        {
            row[variable] = value;
        }
        descend(later, row, later_keys, emit);
    }
}

/// Entry point for the binary hash join.
///
/// Implements [`JoinAlgo`] for any [`KeyedIterable`] data structure. See the
/// module docs for the algorithm. The query it runs is the executed query of
/// `kermit::db::prepare` under
/// [`TrieLevels::ProbeKeys`](kermit_iters::TrieLevels::ProbeKeys): body atom
/// `j` is step `j`, and its index is keyed on the variables the atoms before
/// it bind.
///
/// # Panics
///
/// `join_for_each` panics if the plan is invalid, and if an index is not
/// keyed on exactly the variables the atoms before it bind (the panic message
/// starts with `probe keys lead the columns`). Both are caller errors:
/// `kermit::db::build` already refuses an index set planned under another
/// trie-level policy.
pub struct BinaryHashJoin;

impl<DS> JoinAlgo<DS> for BinaryHashJoin
where
    DS: KeyedIterable,
{
    fn join_for_each<S: FnMut(&[usize])>(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>, mut emit: S,
    ) {
        let analysis = analyse(&query);
        if let Err(e) = plan.validate(&analysis) {
            panic!("BinaryHashJoin::join_for_each: invalid query plan: {e}");
        }
        // One step per body atom, in body order, which is the join order.
        // `bound` records which variables the steps so far bind.
        let mut bound = vec![false; analysis.num_vars];
        let mut steps = Vec::with_capacity(query.body.len());
        for (position, (atom, variables)) in query
            .body
            .iter()
            .zip(&analysis.predicate_variables)
            .enumerate()
        {
            let index = *datastructures
                .get(&atom.name)
                .expect("Missing datastructure for predicate name");
            steps.push(Step::new(
                position, &atom.name, index, variables, &mut bound,
            ));
        }
        // A variable no atom carries can take no value, so the query has no
        // answer (validation rules it out for every query from `kermit::db`).
        if !bound.iter().all(|&b| b) {
            return;
        }
        // The only allocations of the join besides the steps: the row, in
        // canonical variable order, and the keys, `key_len` slots per step.
        let mut row = vec![0; analysis.num_vars];
        let mut keys = vec![0; steps.iter().map(|step| step.key_len).sum()];
        descend(&steps, &mut row, &mut keys, &mut emit);
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_ds::{HashIndex, Relation, RelationHeader},
        kermit_iters::Tuples,
    };

    /// `rows` as an index of `arity` columns keyed on its first `levels`.
    fn index(name: &str, arity: usize, levels: usize, rows: &[&[usize]]) -> HashIndex {
        let mut tuples = Tuples::new(arity);
        for row in rows {
            tuples.push(row);
        }
        HashIndex::from_tuples(
            RelationHeader::new_positional(name, arity).with_levels(levels),
            tuples,
        )
    }

    /// `query` joined by [`BinaryHashJoin`] under the plan `order`, over the
    /// named indexes, with the rows sorted.
    fn joined(order: &[usize], query: &str, indexes: &[(&str, &HashIndex)]) -> Vec<Vec<usize>> {
        let plan = QueryPlan {
            variable_ordering: order.to_vec(),
        };
        let datastructures: HashMap<String, &HashIndex> = indexes
            .iter()
            .map(|&(name, index)| (name.to_string(), index))
            .collect();
        let mut rows = Vec::new();
        BinaryHashJoin::join_for_each(&plan, query.parse().unwrap(), datastructures, |row| {
            rows.push(row.to_vec());
        });
        rows.sort();
        rows
    }

    const TRIANGLE: &str = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).";

    /// The triangle as `kermit::db::prepare` plans it under `ProbeKeys` for
    /// the variable order `[X, Y, Z]`: `R` is the driver (no key), `S` is
    /// keyed on `Y`, `T` on `(X, Z)`. `|R ⋈ S|` is 5, the output 3: the
    /// worked micro-example of `docs/algorithms/binary-hash-join.md`.
    fn triangle_indexes() -> (HashIndex, HashIndex, HashIndex) {
        (
            index("R", 2, 0, &[&[1, 2], &[1, 3], &[2, 3]]),
            index("S", 2, 1, &[&[2, 3], &[3, 4], &[3, 5]]),
            index("T", 2, 2, &[&[1, 3], &[1, 4], &[2, 5]]),
        )
    }

    #[test]
    fn the_triangle_probes_each_atom_on_what_the_earlier_ones_bind() {
        let (r, s, t) = triangle_indexes();
        let rows = joined(&[0, 1, 2], TRIANGLE, &[("R", &r), ("S", &s), ("T", &t)]);
        assert_eq!(rows, vec![vec![1, 2, 3], vec![1, 3, 4], vec![2, 3, 5]]);
    }

    /// An atom sharing no variable with the earlier ones has an empty key, and
    /// its probe lends every row: the cross product.
    #[test]
    fn an_empty_key_crosses_every_row() {
        let r = index("R", 2, 0, &[&[1, 2], &[3, 4]]);
        let s = index("S", 2, 0, &[&[5, 6], &[7, 8]]);
        let rows = joined(&[0, 1, 2, 3], "Q(A, B, C, D) :- R(A, B), S(C, D).", &[
            ("R", &r),
            ("S", &s),
        ]);
        assert_eq!(rows, vec![
            vec![1, 2, 5, 6],
            vec![1, 2, 7, 8],
            vec![3, 4, 5, 6],
            vec![3, 4, 7, 8],
        ]);
    }

    /// A probe that finds nothing ends its branch, and an empty atom ends the
    /// whole join.
    #[test]
    fn a_probe_with_no_match_ends_its_branch() {
        let (r, s, t) = triangle_indexes();
        let no_s = index("S", 2, 1, &[]);
        let no_t = index("T", 2, 2, &[&[9, 9]]);
        assert!(joined(&[0, 1, 2], TRIANGLE, &[("R", &r), ("S", &no_s), ("T", &t)]).is_empty());
        assert!(joined(&[0, 1, 2], TRIANGLE, &[("R", &r), ("S", &s), ("T", &no_t)]).is_empty());
    }

    /// Duplicate rows are matched separately: bag semantics, as for
    /// `HashTrie`.
    #[test]
    fn duplicate_rows_multiply() {
        let r = index("R", 1, 0, &[&[1], &[1]]);
        let s = index("S", 2, 1, &[&[1, 5]]);
        let rows = joined(&[0, 1], "Q(X, Y) :- R(X), S(X, Y).", &[
            ("R", &r),
            ("S", &s),
        ]);
        assert_eq!(rows, vec![vec![1, 5], vec![1, 5]]);
    }

    /// Rows are emitted by canonical variable index (head variables first), not
    /// by atom column: `Q(Y, X) :- R(X, Y)` numbers `Y` 0 and `X` 1, so the
    /// plan binds `X` first (`[1, 0]`) and the row is `[y, x]`.
    #[test]
    fn rows_are_emitted_in_canonical_variable_order() {
        let r = index("R", 2, 0, &[&[1, 10], &[2, 20]]);
        let rows = joined(&[1, 0], "Q(Y, X) :- R(X, Y).", &[("R", &r)]);
        assert_eq!(rows, vec![vec![10, 1], vec![20, 2]]);
    }

    /// A variable no atom carries can take no value, so the query has no
    /// answer. Validation rules it out for every query from `kermit::db`.
    #[test]
    fn a_variable_no_atom_carries_has_no_answer() {
        let r = index("R", 1, 0, &[&[1]]);
        assert!(joined(&[0, 1], "Q(X, W) :- R(X).", &[("R", &r)]).is_empty());
    }

    #[test]
    #[should_panic(expected = "invalid query plan")]
    fn a_plan_that_is_not_a_permutation_is_refused() {
        let (r, s, t) = triangle_indexes();
        let _ = joined(&[0, 0, 1], TRIANGLE, &[("R", &r), ("S", &s), ("T", &t)]);
    }

    /// Indexes keyed on every column, as `bench ds` builds them (the header's
    /// `levels()` is the arity), are not planned for a join: the driver's key
    /// is not bound by any atom before it.
    #[test]
    #[should_panic(expected = "probe keys lead the columns")]
    fn an_index_keyed_on_every_column_is_refused() {
        let (_, s, t) = triangle_indexes();
        let r = index("R", 2, 2, &[&[1, 2]]);
        let _ = joined(&[0, 1, 2], TRIANGLE, &[("R", &r), ("S", &s), ("T", &t)]);
    }

    /// `S(Z, Y)` is keyed on `Z`, which `R(X, Y)` does not bind: a query not
    /// planned under `ProbeKeys`.
    #[test]
    #[should_panic(expected = "probe keys lead the columns")]
    fn a_key_no_earlier_atom_binds_is_refused() {
        let r = index("R", 2, 0, &[&[1, 2]]);
        let s = index("S", 2, 1, &[&[3, 2]]);
        let _ = joined(&[0, 2, 1], "Q(X, Y, Z) :- R(X, Y), S(Z, Y).", &[
            ("R", &r),
            ("S", &s),
        ]);
    }

    /// `S(X, Y)` is keyed on `X`, which `R(X, Y)` binds, but carries `Y` past
    /// its key, which `R` already binds too: the probe would overwrite `Y`
    /// instead of joining on it, so the query is not planned under `ProbeKeys`.
    #[test]
    #[should_panic(expected = "probe keys lead the columns")]
    fn a_column_past_the_key_that_an_earlier_atom_binds_is_refused() {
        let r = index("R", 2, 0, &[&[1, 2]]);
        let s = index("S", 2, 1, &[&[1, 2]]);
        let _ = joined(&[0, 1], "Q(X, Y) :- R(X, Y), S(X, Y).", &[
            ("R", &r),
            ("S", &s),
        ]);
    }
}
