//! Binary Hash Join: the left-deep, pipelined hash join of Free Join §2.2.
//!
//! The baseline that worst-case optimal joins are measured against. A binary
//! plan joins one atom at a time. Written as a left-deep plan `[R_1, …, R_m]`
//! it is executed by pipelining (Wang et al., "Free Join: Unifying
//! Worst-Case Optimal and Traditional Joins", SIGMOD 2023, §2.2, Figure 2a):
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
    crate::{
        executed::ExecutedQuery,
        join_algo::{assert_index_count, JoinAlgo},
    },
    kermit_iters::KeyedIterable,
};

/// One level of the loop nest: a body atom, probed on the variables the
/// atoms before it bind.
struct Step<'a, DS> {
    /// The atom's index. Its first `key.len()` columns are the probe key.
    index: &'a DS,
    /// The atom's variables (canonical indices), one per column, in column
    /// order (the paper's `x_j`).
    variables: &'a [usize],
    /// The probe key, `index.key_len()` values: what the row holds for the
    /// atom's leading variables, rewritten each time the step is reached.
    key: Vec<usize>,
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
            key: vec![0; key_len],
        }
    }
}

/// Free Join Figure 2a's loop nest from step `steps[0]` on, for the prefix
/// `row` binds so far.
///
/// Probes the first step's index on the key `row` holds for its leading
/// variables, writes each match's remaining columns into `row`, and descends
/// into the later steps. Past the last step every variable is bound and `row`
/// is emitted. Each step owns its key, so the recursion allocates nothing and
/// a level's key stays intact while the deeper levels run.
fn descend<DS: KeyedIterable, S: FnMut(&[usize])>(
    steps: &mut [Step<'_, DS>], row: &mut [usize], emit: &mut S,
) {
    let Some((step, later)) = steps.split_first_mut() else {
        emit(row);
        return;
    };
    for (slot, &variable) in step.key.iter_mut().zip(step.variables) {
        *slot = row[variable];
    }
    let key_len = step.key.len();
    for tuple in step.index.probe(&step.key) {
        for (&variable, &value) in step.variables[key_len..].iter().zip(&tuple[key_len..]) {
            row[variable] = value;
        }
        descend(later, row, emit);
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
/// `join_for_each` panics if it is given the wrong number of indexes (see
/// [`JoinAlgo::join_for_each`]), and if an index is not keyed on exactly the
/// variables the atoms before it bind (the panic message starts with `probe
/// keys lead the columns`). Both are caller errors: `kermit::db::build`
/// already refuses an index set planned under another trie-level policy.
pub struct BinaryHashJoin;

impl<DS> JoinAlgo<DS> for BinaryHashJoin
where
    DS: KeyedIterable,
{
    fn join_for_each<S: FnMut(&[usize])>(query: &ExecutedQuery, indexes: &[DS], mut emit: S) {
        assert_index_count("BinaryHashJoin", query, indexes.len());
        let analysis = query.analysis();
        // One step per body atom, in body order, which is the join order.
        // `bound` records which variables the steps so far bind.
        let mut bound = vec![false; analysis.num_vars];
        let mut steps = Vec::with_capacity(query.atom_indexes().len());
        for (position, ((atom, &id), variables)) in query
            .query()
            .body
            .iter()
            .zip(query.atom_indexes())
            .zip(&analysis.predicate_variables)
            .enumerate()
        {
            steps.push(Step::new(
                position,
                &atom.name,
                &indexes[id],
                variables,
                &mut bound,
            ));
        }
        // Every variable is carried by some atom (an [`ExecutedQuery`] holds
        // no other), so no slot of `row` is left unbound. An empty body has
        // no step, and `descend` emits its one answer, the empty row.
        debug_assert!(bound.iter().all(|&b| b), "unbound variable");
        // The only allocation of the join besides the steps and their keys:
        // the row, in canonical variable order.
        let mut row = vec![0; analysis.num_vars];
        descend(&mut steps, &mut row, &mut emit);
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::optimiser::QueryPlan,
        kermit_ds::{HashIndex, Relation, RelationHeader},
        kermit_iters::Tuples,
        kermit_parser::JoinQuery,
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

    /// `query` as an [`ExecutedQuery`] under the plan `order`.
    fn executed(order: &[usize], query: &str) -> ExecutedQuery {
        let plan = QueryPlan {
            variable_ordering: order.to_vec(),
        };
        ExecutedQuery::new(query.parse().unwrap(), plan).unwrap()
    }

    /// `query` joined by [`BinaryHashJoin`] under the plan `order`, over
    /// `indexes` in id order (the body's distinct atom names by first
    /// appearance), with the rows sorted.
    fn joined(order: &[usize], query: &str, indexes: &[HashIndex]) -> Vec<Vec<usize>> {
        let mut rows = Vec::new();
        BinaryHashJoin::join_for_each(&executed(order, query), indexes, |row| {
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
    fn triangle_indexes() -> [HashIndex; 3] {
        [
            index("R", 2, 0, &[&[1, 2], &[1, 3], &[2, 3]]),
            index("S", 2, 1, &[&[2, 3], &[3, 4], &[3, 5]]),
            index("T", 2, 2, &[&[1, 3], &[1, 4], &[2, 5]]),
        ]
    }

    #[test]
    fn the_triangle_probes_each_atom_on_what_the_earlier_ones_bind() {
        let rows = joined(&[0, 1, 2], TRIANGLE, &triangle_indexes());
        assert_eq!(rows, vec![vec![1, 2, 3], vec![1, 3, 4], vec![2, 3, 5]]);
    }

    /// An atom sharing no variable with the earlier ones has an empty key, and
    /// its probe lends every row: the cross product.
    #[test]
    fn an_empty_key_crosses_every_row() {
        let r = index("R", 2, 0, &[&[1, 2], &[3, 4]]);
        let s = index("S", 2, 0, &[&[5, 6], &[7, 8]]);
        let rows = joined(&[0, 1, 2, 3], "Q(A, B, C, D) :- R(A, B), S(C, D).", &[r, s]);
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
        let [r, s, t] = triangle_indexes();
        let no_s = index("S", 2, 1, &[]);
        let no_t = index("T", 2, 2, &[&[9, 9]]);
        assert!(joined(&[0, 1, 2], TRIANGLE, &[r.clone(), no_s, t]).is_empty());
        assert!(joined(&[0, 1, 2], TRIANGLE, &[r, s, no_t]).is_empty());
    }

    /// Duplicate rows are matched separately: bag semantics, as for
    /// `HashTrie`.
    #[test]
    fn duplicate_rows_multiply() {
        let r = index("R", 1, 0, &[&[1], &[1]]);
        let s = index("S", 2, 1, &[&[1, 5]]);
        let rows = joined(&[0, 1], "Q(X, Y) :- R(X), S(X, Y).", &[r, s]);
        assert_eq!(rows, vec![vec![1, 5], vec![1, 5]]);
    }

    /// Rows are emitted by canonical variable index (head variables first), not
    /// by atom column: `Q(Y, X) :- R(X, Y)` numbers `Y` 0 and `X` 1, so the
    /// plan binds `X` first (`[1, 0]`) and the row is `[y, x]`.
    #[test]
    fn rows_are_emitted_in_canonical_variable_order() {
        let r = index("R", 2, 0, &[&[1, 10], &[2, 20]]);
        let rows = joined(&[1, 0], "Q(Y, X) :- R(X, Y).", &[r]);
        assert_eq!(rows, vec![vec![10, 1], vec![20, 2]]);
    }

    /// A self-join reads one index twice: the two atoms share id 0.
    #[test]
    fn a_self_join_reads_one_index_through_both_atoms() {
        let r = index("R", 2, 1, &[&[1, 2], &[2, 3], &[3, 4]]);
        let driver = index("D", 1, 0, &[&[1], &[2]]);
        let rows = joined(&[0, 1, 2], "Q(X, Y, Z) :- D(X), R(X, Y), R(Y, Z).", &[
            driver, r,
        ]);
        assert_eq!(rows, vec![vec![1, 2, 3], vec![2, 3, 4]]);
    }

    /// The empty conjunction: its one answer is the empty row.
    #[test]
    fn join_for_each_emits_one_empty_row_for_an_empty_body() {
        let mut query: JoinQuery = "Q(c1) :- R(c1).".parse().unwrap();
        query.head.terms.clear();
        query.body.clear();
        let plan = QueryPlan {
            variable_ordering: vec![],
        };
        let executed = ExecutedQuery::new(query, plan).unwrap();
        let none: [HashIndex; 0] = [];
        let mut rows: Vec<Vec<usize>> = Vec::new();
        BinaryHashJoin::join_for_each(&executed, &none, |row| rows.push(row.to_vec()));
        assert_eq!(rows, vec![Vec::<usize>::new()]);
    }

    #[test]
    #[should_panic(expected = "BinaryHashJoin::join_for_each: 1 indexes for a query that reads 2")]
    fn join_for_each_panics_when_an_index_is_missing() {
        let r = index("R", 2, 0, &[&[1, 2]]);
        let _ = joined(&[0, 1, 2], "Q(X, Y, Z) :- R(X, Y), S(Y, Z).", &[r]);
    }

    /// Indexes keyed on every column, as `bench ds` builds them (the header's
    /// `levels()` is the arity), are not planned for a join: the driver's key
    /// is not bound by any atom before it.
    #[test]
    #[should_panic(expected = "probe keys lead the columns")]
    fn an_index_keyed_on_every_column_is_refused() {
        let [_, s, t] = triangle_indexes();
        let r = index("R", 2, 2, &[&[1, 2]]);
        let _ = joined(&[0, 1, 2], TRIANGLE, &[r, s, t]);
    }

    /// `S(Z, Y)` is keyed on `Z`, which `R(X, Y)` does not bind: a query not
    /// planned under `ProbeKeys`.
    #[test]
    #[should_panic(expected = "probe keys lead the columns")]
    fn a_key_no_earlier_atom_binds_is_refused() {
        let r = index("R", 2, 0, &[&[1, 2]]);
        let s = index("S", 2, 1, &[&[3, 2]]);
        let _ = joined(&[0, 2, 1], "Q(X, Y, Z) :- R(X, Y), S(Z, Y).", &[r, s]);
    }

    /// `S(X, Y)` is keyed on `X`, which `R(X, Y)` binds, but carries `Y` past
    /// its key, which `R` already binds too: the probe would overwrite `Y`
    /// instead of joining on it, so the query is not planned under `ProbeKeys`.
    #[test]
    #[should_panic(expected = "probe keys lead the columns")]
    fn a_column_past_the_key_that_an_earlier_atom_binds_is_refused() {
        let r = index("R", 2, 0, &[&[1, 2]]);
        let s = index("S", 2, 1, &[&[1, 2]]);
        let _ = joined(&[0, 1], "Q(X, Y) :- R(X, Y), S(X, Y).", &[r, s]);
    }
}
