//! Hash Trie Join — worst-case-optimal multi-way join over hash tries.
//!
//! Implements the probe phase from §3.2.3 of the SIGMOD 2020 paper
//! "Combining Worst-Case Optimal and Traditional Binary Join Processing".
//! Coordinates one [`HashTrieIterator`] per body predicate, descending in
//! lockstep variable by variable. At each depth it picks the iterator with
//! the smallest hash table (`argmin size`), iterates that table's hashes,
//! and probes the rest via [`HashTrieIterator::lookup`]. At the leaf level
//! it cross-products the participating iterators' tuple chains and
//! verifies the actual join condition (hash collisions can produce false
//! positives at any inner level).

use {
    crate::{analysis::analyse, join_algo::JoinAlgo, optimiser::QueryPlan},
    kermit_iters::{HashTrieIterable, HashTrieIterator},
    kermit_parser::JoinQuery,
    std::collections::HashMap,
};

/// Build `variable_to_iter_map[i] = predicate indices that carry the i-th
/// variable in `variable_ordering`. Identical shape to LFTJ's inline
/// construction.
fn build_variable_to_iter_map(
    variable_ordering: &[usize], predicate_variables: &[Vec<usize>],
) -> Vec<Vec<usize>> {
    variable_ordering
        .iter()
        .map(|v| {
            predicate_variables
                .iter()
                .enumerate()
                .filter_map(|(i, vars)| {
                    if vars.contains(v) {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .collect()
}

/// Verify one candidate result tuple's join condition, writing its output
/// tuple into `row` in canonical (head-first) variable-index order (i.e.
/// indexed by the variable indices in `predicate_variables`). Returns
/// `false` if any variable mentioned by two or more predicates has
/// inconsistent values in the candidate (a hash false positive); `row` then
/// holds a partial write and must not be emitted.
///
/// `candidate` yields one tuple per participating relation, the k-th from
/// the k-th relation's leaf chain. `predicate_variables[k]` lists the
/// variable indices carried by the k-th relation (in attribute order).
/// `row` and `bound` are per-join scratch with one slot per distinct query
/// variable. `bound` is reset on entry, so a rejected candidate leaves
/// nothing behind for the next one.
fn verify_and_construct<'t>(
    candidate: impl IntoIterator<Item = &'t [usize]>, predicate_variables: &[Vec<usize>],
    row: &mut [usize], bound: &mut [bool],
) -> bool {
    bound.fill(false);
    for (tuple, vars) in candidate.into_iter().zip(predicate_variables) {
        for (col, &var_idx) in vars.iter().enumerate() {
            let v = tuple[col];
            if !bound[var_idx] {
                row[var_idx] = v;
                bound[var_idx] = true;
            } else if row[var_idx] != v {
                return false;
            }
        }
    }
    // Every variable is carried by some relation, or `enumerate` would have
    // returned before reaching the leaf, so no slot of `row` is stale.
    debug_assert!(bound.iter().all(|&b| b), "unbound variable at the leaf");
    true
}

/// Per-join scratch for the leaf cross product (Algorithm 3 lines 16–19),
/// allocated once in `join_for_each` so that emitting a result tuple
/// allocates nothing (issue #65).
struct LeafScratch {
    /// Mixed-base cursor: `cursor[k]` indexes the k-th iterator's leaf chain.
    cursor: Vec<usize>,
    /// `chain_lens[k]` is the length of the k-th iterator's current chain.
    chain_lens: Vec<usize>,
    /// The candidate's output tuple, in canonical variable order.
    row: Vec<usize>,
    /// `bound[v]` records whether `row[v]` was written for this candidate.
    bound: Vec<bool>,
}

impl LeafScratch {
    /// Scratch for `relations` body predicates over `arity` distinct
    /// variables.
    fn new(relations: usize, arity: usize) -> Self {
        Self {
            cursor: vec![0; relations],
            chain_lens: vec![0; relations],
            row: vec![0; arity],
            bound: vec![false; arity],
        }
    }
}

/// Algorithm 3 from the paper. Recursively descends through attribute
/// positions; passes every verified result tuple to `emit`.
///
/// **Contract.** `enumerate(i, ...)` is called with each iterator in
/// `variable_to_iter_map[i]` positioned at depth `i - 1` (or pre-root for
/// `i == 0`). The function descends each one level (to depth `i`), scans
/// at depth `i`, recurses on matches, then ascends back. This way the
/// caller's stack is unchanged on return.
fn enumerate<IT: HashTrieIterator, S: FnMut(&[usize])>(
    i: usize, arity: usize, iters: &mut [IT], predicate_variables: &[Vec<usize>],
    variable_to_iter_map: &[Vec<usize>], scratch: &mut LeafScratch, emit: &mut S,
) {
    if i == arity {
        emit_leaf(iters, predicate_variables, scratch, emit);
        return;
    }

    let participating = &variable_to_iter_map[i];
    if participating.is_empty() {
        return; // no relation carries this variable
    }

    // Descend every participating iterator to depth i. Track how many we
    // successfully opened so we can match `up` calls on early return.
    let mut opened = 0;
    let mut descend_ok = true;
    for &idx in participating {
        if iters[idx].open() {
            opened += 1;
        } else {
            descend_ok = false;
            break;
        }
    }

    if descend_ok {
        let scan_idx = *participating
            .iter()
            .min_by_key(|&&idx| iters[idx].size())
            .expect("participating non-empty");

        while !iters[scan_idx].at_end() {
            let scan_hash = iters[scan_idx]
                .key()
                .expect("scan iterator not at end => key Some");

            // Probe the other participating iterators for this hash.
            let mut all_match = true;
            for &idx in participating {
                if idx == scan_idx {
                    continue;
                }
                if !iters[idx].lookup(scan_hash) {
                    all_match = false;
                    break;
                }
            }

            if all_match {
                enumerate(
                    i + 1,
                    arity,
                    iters,
                    predicate_variables,
                    variable_to_iter_map,
                    scratch,
                    emit,
                );
            }

            iters[scan_idx].next();
        }
    }

    // Ascend back to the parent depth, matching the descend count so
    // partial-open failures are symmetric.
    for &idx in &participating[..opened] {
        let _ = iters[idx].up();
    }
}

/// Algorithm 3 lines 16–19. Cross-product the leaf chains of every
/// iterator and emit each verified candidate.
///
/// Allocates nothing: each chain is re-borrowed through `leaf_tuples()` as
/// a `LeafRows` view (row ids over its relation's tuple buffer) rather than
/// collected, a candidate's tuples are slices of those buffers, and the
/// cursor and output row live in `scratch`.
fn emit_leaf<IT: HashTrieIterator, S: FnMut(&[usize])>(
    iters: &[IT], predicate_variables: &[Vec<usize>], scratch: &mut LeafScratch, emit: &mut S,
) {
    let chain = |k: usize| {
        iters[k]
            .leaf_tuples()
            .expect("at leaf level for every participating iter")
    };
    for (k, len) in scratch.chain_lens.iter_mut().enumerate() {
        *len = chain(k).len();
    }
    if scratch.chain_lens.contains(&0) {
        return;
    }

    scratch.cursor.fill(0);
    loop {
        let candidate = scratch
            .cursor
            .iter()
            .enumerate()
            .map(|(k, &i)| chain(k).row(i));
        if verify_and_construct(
            candidate,
            predicate_variables,
            &mut scratch.row,
            &mut scratch.bound,
        ) {
            emit(&scratch.row);
        }
        if !advance_cursor(&mut scratch.cursor, &scratch.chain_lens) {
            break;
        }
    }
}

/// Advance a mixed-base cursor over the chain lengths. Returns `false`
/// once every position has been exhausted (overflow off the high end).
fn advance_cursor(cursor: &mut [usize], chain_lens: &[usize]) -> bool {
    let mut k = 0;
    loop {
        if k >= cursor.len() {
            return false;
        }
        cursor[k] += 1;
        if cursor[k] < chain_lens[k] {
            return true;
        }
        cursor[k] = 0;
        k += 1;
    }
}

/// Entry point for the hash-trie-join algorithm.
///
/// Implements [`JoinAlgo`] for any [`HashTrieIterable`] data structure.
/// See the module docs for the algorithm overview.
pub struct HashTriejoin {}

impl<DS> JoinAlgo<DS> for HashTriejoin
where
    DS: HashTrieIterable,
{
    fn join_for_each<S: FnMut(&[usize])>(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>, mut emit: S,
    ) {
        let analysis = analyse(&query);
        if let Err(e) = plan.validate(&analysis) {
            panic!("HashTriejoin::join_for_each: invalid query plan: {e}");
        }
        let variable_ordering = &plan.variable_ordering;
        let predicate_variables = analysis.predicate_variables;
        let mut iters: Vec<_> = query
            .body
            .iter()
            .map(|pred| {
                datastructures
                    .get(&pred.name)
                    .expect("Missing datastructure for predicate name")
                    .hash_trie_iter()
            })
            .collect();
        let variable_to_iter_map =
            build_variable_to_iter_map(variable_ordering, &predicate_variables);
        let mut scratch = LeafScratch::new(iters.len(), variable_ordering.len());
        enumerate(
            0,
            variable_ordering.len(),
            &mut iters,
            &predicate_variables,
            &variable_to_iter_map,
            &mut scratch,
            &mut emit,
        );
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::optimiser::{CatalogStats, LexicographicOptimiser, QueryOptimiser},
    };

    #[test]
    fn variable_to_iter_map_triangle() {
        let pred_vars = vec![vec![0, 1], vec![1, 2], vec![0, 2]];
        let map = build_variable_to_iter_map(&[0, 1, 2], &pred_vars);
        assert_eq!(map, vec![vec![0, 2], vec![0, 1], vec![1, 2]]);
    }

    #[test]
    fn verify_constructs_when_shared_var_agrees() {
        // R(X, Y), S(Y, Z) with X=1, Y=2, Z=3.
        let pv = vec![vec![0, 1], vec![1, 2]];
        let (mut row, mut bound) = (vec![0; 3], vec![false; 3]);
        assert!(verify_and_construct(
            [&[1, 2][..], &[2, 3][..]],
            &pv,
            &mut row,
            &mut bound
        ));
        assert_eq!(row, vec![1, 2, 3]);
    }

    #[test]
    fn verify_rejects_when_shared_var_disagrees() {
        // R(X, Y), S(Y, Z) but Y differs in R (=2) vs S (=99)
        let pv = vec![vec![0, 1], vec![1, 2]];
        let (mut row, mut bound) = (vec![0; 3], vec![false; 3]);
        assert!(!verify_and_construct(
            [&[1, 2][..], &[99, 3][..]],
            &pv,
            &mut row,
            &mut bound
        ));
    }

    #[test]
    fn verify_passes_no_shared_var() {
        // Two disjoint unary predicates R(X), S(Y).
        let pv = vec![vec![0], vec![1]];
        let (mut row, mut bound) = (vec![0; 2], vec![false; 2]);
        assert!(verify_and_construct(
            [&[1][..], &[2][..]],
            &pv,
            &mut row,
            &mut bound
        ));
        assert_eq!(row, vec![1, 2]);
    }

    /// The scratch row outlives each candidate: a rejected candidate leaves
    /// a partial write, and the next one must not mistake it for a binding.
    #[test]
    fn verify_resets_scratch_between_candidates() {
        let pv = vec![vec![0, 1], vec![1, 2]];
        let (mut row, mut bound) = (vec![0; 3], vec![false; 3]);
        assert!(!verify_and_construct(
            [&[1, 2][..], &[99, 3][..]],
            &pv,
            &mut row,
            &mut bound
        ));
        assert!(verify_and_construct(
            [&[4, 5][..], &[5, 6][..]],
            &pv,
            &mut row,
            &mut bound
        ));
        assert_eq!(row, vec![4, 5, 6]);
    }

    #[test]
    fn enumerate_unary_intersection() {
        use kermit_ds::{HashTrie, Relation};
        // Explicit `HashTrie` annotation pins the default `H = SipHashStrategy`
        // since the local bindings escape into `Vec<_>` iter values that
        // would otherwise leave `H` ambiguous.
        let r: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let s: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![4]]);
        let mut iters = vec![r.hash_trie_iter(), s.hash_trie_iter()];
        // Inline the same setup the JoinAlgo entry point does.
        let predicate_variables = vec![vec![0], vec![0]];
        let variable_to_iter_map = vec![vec![0, 1]];
        let mut scratch = LeafScratch::new(iters.len(), 1);
        let mut output = Vec::new();
        enumerate(
            0,
            1,
            &mut iters,
            &predicate_variables,
            &variable_to_iter_map,
            &mut scratch,
            &mut |row: &[usize]| output.push(row.to_vec()),
        );
        output.sort();
        assert_eq!(output, vec![vec![2], vec![3]]);
    }

    // ------------------------------------------------------------------
    // Real hash collisions.
    //
    // The `verify_*` tests above feed hand-built candidates. These build
    // real `HashTrie`s under a strategy that *guarantees* collisions, so the
    // false positive travels the actual path: bucket index, linear probe,
    // inner-level `lookup`, leaf chain, `emit_leaf` cross product, and only
    // then `verify_and_construct`.
    // ------------------------------------------------------------------

    /// `hash(k) = k mod 10`: keys congruent modulo 10 collide at every
    /// level. A twin lives in `kermit-ds/tests/hash_trie_tests.rs`; neither
    /// crate exposes a colliding strategy publicly.
    #[derive(Copy, Clone, Default, Debug)]
    struct Mod10HashStrategy;

    impl kermit_iters::LayoutOption for Mod10HashStrategy {
        const NAME: &'static str = "mod10";
    }

    impl kermit_iters::HashStrategy for Mod10HashStrategy {
        fn hash(key: usize) -> u64 { (key % 10) as u64 }
    }

    type CollidingHashTrie = kermit_ds::HashTrie<Mod10HashStrategy>;

    #[test]
    fn verify_rejects_real_inner_level_collision() {
        use kermit_ds::Relation;
        // R(X, Y) = {(1, 2)}, S(Y, Z) = {(12, 3)}. h(2) == h(12), so the
        // Y-level probe of S succeeds although 2 != 12.
        let r = CollidingHashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let s = CollidingHashTrie::from_tuples(2.into(), vec![vec![12, 3]]);
        let mut r_it = r.hash_trie_iter();
        let mut s_it = s.hash_trie_iter();

        // Walk the iterators exactly as `enumerate` would for X, Y, Z.
        assert!(r_it.open()); // X level of R
        assert!(r_it.open()); // Y level of R (its leaf)
        assert!(s_it.open()); // Y level of S (its root)
        let y_hash = r_it.key().expect("R positioned on its Y bucket");
        assert!(
            s_it.lookup(y_hash),
            "the inner-level probe must hit: the collision is real"
        );
        assert!(s_it.open()); // Z level of S (its leaf)

        // Both chains hold one tuple; together they form the only candidate.
        let r_chain = r_it.leaf_tuples().expect("R at leaf");
        let s_chain = s_it.leaf_tuples().expect("S at leaf");
        assert_eq!(r_chain.to_vecs(), vec![vec![1, 2]]);
        assert_eq!(s_chain.to_vecs(), vec![vec![12, 3]]);
        let pv = vec![vec![0, 1], vec![1, 2]];
        let (mut row, mut bound) = (vec![0; 3], vec![false; 3]);
        assert!(
            !verify_and_construct([r_chain.row(0), s_chain.row(0)], &pv, &mut row, &mut bound),
            "Y = 2 in R but 12 in S: the leaf-level check must reject it"
        );
    }

    #[test]
    fn join_algo_drops_inner_level_collision_false_positive() {
        use kermit_ds::Relation;
        // R(X, Y) = {(1, 2)}, S(Y, Z) = {(12, 3), (2, 5)}. The Y-level probe
        // matches both S tuples (h(2) == h(12)); only (2, 5) truly joins.
        let r = CollidingHashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let s = CollidingHashTrie::from_tuples(2.into(), vec![vec![12, 3], vec![2, 5]]);
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z).".parse().unwrap();
        let mut ds: HashMap<String, &CollidingHashTrie> = HashMap::new();
        ds.insert("R".to_string(), &r);
        ds.insert("S".to_string(), &s);
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        let mut out: Vec<Vec<usize>> = HashTriejoin::join_iter(&plan, query, ds).collect();
        out.sort();
        assert_eq!(out, vec![vec![1, 2, 5]]);
    }

    #[test]
    fn join_algo_drops_leaf_chain_collision_false_positive() {
        use kermit_ds::Relation;
        // R(X) = {1, 11}, S(X) = {11}. Both R tuples share one leaf chain
        // with S's, so `emit_leaf` cross-products (1, 11) and (11, 11);
        // only the latter survives verification.
        let r = CollidingHashTrie::from_tuples(1.into(), vec![vec![1], vec![11]]);
        let s = CollidingHashTrie::from_tuples(1.into(), vec![vec![11]]);
        let query: JoinQuery = "Q(X) :- R(X), S(X).".parse().unwrap();
        let mut ds: HashMap<String, &CollidingHashTrie> = HashMap::new();
        ds.insert("R".to_string(), &r);
        ds.insert("S".to_string(), &s);
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        let mut out: Vec<Vec<usize>> = HashTriejoin::join_iter(&plan, query, ds).collect();
        out.sort();
        assert_eq!(out, vec![vec![11]]);
    }

    #[test]
    fn join_algo_unary_intersection() {
        use kermit_ds::{HashTrie, Relation};
        let r = HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let s = HashTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![4]]);
        let query: JoinQuery = "Q(X) :- R(X), S(X).".parse().unwrap();
        let mut ds: HashMap<String, &HashTrie> = HashMap::new();
        ds.insert("R".to_string(), &r);
        ds.insert("S".to_string(), &s);
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        let mut out: Vec<Vec<usize>> = HashTriejoin::join_iter(&plan, query, ds).collect();
        out.sort();
        assert_eq!(out, vec![vec![2], vec![3]]);
    }

    #[test]
    fn join_algo_triangle() {
        use kermit_ds::{HashTrie, Relation};
        let r = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3], vec![3, 1]]);
        let s = HashTrie::from_tuples(2.into(), vec![vec![2, 3], vec![3, 1], vec![1, 2]]);
        let t = HashTrie::from_tuples(2.into(), vec![vec![1, 3], vec![2, 1], vec![3, 2]]);
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        let mut ds: HashMap<String, &HashTrie> = HashMap::new();
        ds.insert("R".to_string(), &r);
        ds.insert("S".to_string(), &s);
        ds.insert("T".to_string(), &t);
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        let mut out: Vec<Vec<usize>> = HashTriejoin::join_iter(&plan, query, ds).collect();
        out.sort();
        assert_eq!(out, vec![vec![1, 2, 3], vec![2, 3, 1], vec![3, 1, 2]]);
    }

    #[test]
    fn join_for_each_streams_every_verified_row() {
        use kermit_ds::{HashTrie, Relation};
        let r = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3], vec![3, 1]]);
        let s = HashTrie::from_tuples(2.into(), vec![vec![2, 3], vec![3, 1], vec![1, 2]]);
        let t = HashTrie::from_tuples(2.into(), vec![vec![1, 3], vec![2, 1], vec![3, 2]]);
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        let ds: HashMap<String, &HashTrie> = HashMap::from([
            ("R".to_string(), &r),
            ("S".to_string(), &s),
            ("T".to_string(), &t),
        ]);
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        let mut out = Vec::new();
        HashTriejoin::join_for_each(&plan, query, ds, |tuple| out.push(tuple.to_vec()));
        out.sort();
        assert_eq!(out, vec![vec![1, 2, 3], vec![2, 3, 1], vec![3, 1, 2]]);
    }
}
