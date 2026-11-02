//! Hash Trie Join — worst-case-optimal multi-way join over hash tries.
//!
//! Implements the probe phase, Algorithm 3 of §3.2.3, of the SIGMOD 2020
//! paper "Combining Worst-Case Optimal and Traditional Binary Join
//! Processing". One [`HashTrieIterator`] per body atom descends variable by
//! variable. At each variable the participant with the smallest hash table
//! (`argmin size`) scans its hashes, and the others are probed with
//! [`HashTrieIterator::lookup`]. Once every variable is bound, every
//! iterator's leaf chain is cross-producted and each candidate is checked
//! against the actual join condition, since a hash match at any level may be
//! a collision.
//!
//! A relation participates at a variable only where its trie has a level
//! for it (`HashTrieIterable::levels`, #116): under `--ds-layout-levels keys`
//! a payload column is no level, so its variable is skipped on the way down
//! and bound from the leaf rows, by the same leaf product (Algorithm 3,
//! lines 16–19).

use {
    crate::{
        executed::ExecutedQuery,
        join_algo::{assert_index_count, JoinAlgo},
    },
    kermit_iters::{HashTrieIterable, HashTrieIterator, LeafCursor, LeafRows},
};

/// Build `variable_to_iter_map[i]`: the predicates whose trie has a level
/// for the `i`-th variable of `variable_ordering`, i.e. that carry it among
/// their first `levels[p]` columns (#116). Under full levels that is every
/// predicate carrying it, LFTJ's inline construction; under join-key levels
/// a payload column is no level, and its variable is bound from the leaf
/// rows instead.
fn build_variable_to_iter_map(
    variable_ordering: &[usize], predicate_variables: &[Vec<usize>], levels: &[usize],
) -> Vec<Vec<usize>> {
    variable_ordering
        .iter()
        .map(|v| {
            predicate_variables
                .iter()
                .zip(levels)
                .enumerate()
                .filter_map(|(p, (vars, &depth))| vars[..depth].contains(v).then_some(p))
                .collect()
        })
        .collect()
}

/// Algorithm 3 line 18: checks one candidate's join condition, writing its
/// output tuple into `row`, indexed by canonical (head-first) variable.
/// Returns `false` if a variable two atoms carry takes two values in the
/// candidate (a hash collision); `row` then holds a partial write and must
/// not be emitted.
///
/// `candidate` yields one tuple per relation, the k-th from the k-th
/// relation's leaf chain, whose columns carry `predicate_variables[k]`.
/// `row` and `bound` are per-join scratch with one slot per variable;
/// `bound` is reset on entry, so a rejected candidate leaves nothing behind
/// for the next one.
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
    // Every variable is carried by some relation (an [`ExecutedQuery`] holds
    // no other), so no slot of `row` is stale.
    debug_assert!(bound.iter().all(|&b| b), "unbound variable at the leaf");
    true
}

/// Per-join scratch for the leaf cross product (Algorithm 3 lines 16–19),
/// allocated once per join (`Probe::new`) so that emitting a result tuple
/// allocates nothing (issue #65).
struct LeafScratch {
    /// The mixed-base odometer: `cursor[k]` is a position in the k-th
    /// iterator's current leaf chain.
    cursor: Vec<LeafCursor>,
    /// The candidate's output tuple, in canonical variable order.
    row: Vec<usize>,
    /// `bound[v]` records whether `row[v]` was written for this candidate.
    bound: Vec<bool>,
}

impl LeafScratch {
    /// Scratch for `relations` body predicates over `arity` distinct
    /// variables. A cursor has no default position, so `cursor` starts empty
    /// with room for every relation's, and `emit_leaf` fills it from each
    /// chain's `first` without allocating.
    fn new(relations: usize, arity: usize) -> Self {
        Self {
            cursor: Vec::with_capacity(relations),
            row: vec![0; arity],
            bound: vec![false; arity],
        }
    }
}

/// One run of Algorithm 3 over a join's iterators, holding what the
/// recursion reads, so that [`enumerate`](Self::enumerate) takes the paper's
/// one argument, the position `i`.
///
/// **Where the descent happens.** The paper's iterators start in their root
/// nodes and step `down` as soon as a hash matches (lines 10–11): into the
/// node of their next variable or, after their last, into the tuple chain.
/// A [`HashTrieIterator`] has no position inside a chain: `open` fails at
/// the last level, and [`leaf_tuples`](HashTrieIterator::leaf_tuples) reads
/// the chain from the last level's bucket. So here every iterator starts at
/// its pre-root, and `enumerate(i)` opens its participants into their nodes
/// for `v_i` on entry and ascends again before it returns: each `down` is
/// deferred until the variable it serves is reached. The scans, probes and
/// leaf product are the paper's.
struct Probe<'q, IT, S> {
    /// The paper's `I`: one iterator per body atom.
    iters: Vec<IT>,
    /// `predicate_variables[k]`: the variables of the k-th atom (the paper's
    /// `E_k`), in column order.
    predicate_variables: &'q [Vec<usize>],
    /// `variable_to_iter_map[i]`: the atoms whose trie has a level for the
    /// `i`-th variable of the plan, the paper's `I_join` at `i`.
    variable_to_iter_map: &'q [Vec<usize>],
    scratch: LeafScratch,
    /// The sink for every verified result row, in canonical variable order.
    emit: S,
}

impl<'q, IT: HashTrieIterator, S: FnMut(&[usize])> Probe<'q, IT, S> {
    fn new(
        iters: Vec<IT>, predicate_variables: &'q [Vec<usize>],
        variable_to_iter_map: &'q [Vec<usize>], emit: S,
    ) -> Self {
        let scratch = LeafScratch::new(iters.len(), variable_to_iter_map.len());
        Self {
            iters,
            predicate_variables,
            variable_to_iter_map,
            scratch,
            emit,
        }
    }

    /// Algorithm 3: binds the plan's variables from position `i` on, and
    /// emits every verified result below the current prefix.
    ///
    /// **Contract.** Each iterator in `variable_to_iter_map[i]` stands on
    /// the matched bucket of its level before `v_i`, or at its pre-root if
    /// `v_i` is its first level. `enumerate(i)` opens it, recurses for every
    /// hash the participants share, and ascends it again, so every iterator
    /// is back where it was on return. A position no iterator has a
    /// level for is skipped (#116). After the last position every iterator
    /// stands on its last level's bucket, or at the pre-root of a trie with
    /// no level, so `emit_leaf` reads every chain.
    fn enumerate(&mut self, i: usize) {
        // Lines 2 and 16: past the last variable, the leaf product.
        let variable_to_iter_map = self.variable_to_iter_map;
        let Some(participating) = variable_to_iter_map.get(i) else {
            return self.emit_leaf();
        };
        if participating.is_empty() {
            // No trie has a level for `v_i`: it is payload, bound from the
            // leaf rows (lines 16–19). Under full levels every variable is
            // some trie's level, so this never runs.
            return self.enumerate(i + 1);
        }

        // Lines 10–11, deferred to here (see `Probe`): open each participant
        // into its node for `v_i`, stopping at the first that fails. Under
        // the contract only an empty root can fail, and then no row passes
        // this position, so nothing is scanned. A failed open moves nothing
        // (`HashTrieIterator::open`), so only the ones that opened go back up.
        let opened = participating
            .iter()
            .take_while(|&&j| self.iters[j].open())
            .count();
        if opened == participating.len() {
            // Line 5: scan the smallest table (lines 6 and 15), probing the
            // others for each of its hashes (lines 7–9).
            let scan = *participating
                .iter()
                .min_by_key(|&&j| self.iters[j].size())
                .expect("participating is non-empty");
            while let Some(hash) = self.iters[scan].key() {
                if participating
                    .iter()
                    .all(|&j| j == scan || self.iters[j].lookup(hash))
                {
                    self.enumerate(i + 1); // line 12
                }
                self.iters[scan].next();
            }
        }
        // Lines 13–14, for the participants that opened.
        for &j in &participating[..opened] {
            self.iters[j].up();
        }
    }

    /// Algorithm 3 lines 16–19: cross-products every iterator's leaf chain
    /// and emits each verified candidate.
    ///
    /// The product is a mixed-base odometer of [`LeafCursor`]s, one per
    /// relation, the first relation's turning fastest, so each chain is read
    /// in chain order and no row is reached by index: a linked chain's next
    /// row is one link away. It allocates nothing: each chain is re-borrowed
    /// through `leaf_tuples` as a [`LeafRows`] view of its relation's tuple
    /// buffer, a candidate's tuples are slices of those buffers, and the
    /// cursors and output row live in `scratch`, sized once per join.
    fn emit_leaf(&mut self) {
        let Self {
            iters,
            predicate_variables,
            scratch,
            emit,
            ..
        } = self;
        // Every cursor at its chain's first row; an empty chain leaves no
        // candidate. `cursor` has room for every relation, so `push` never
        // allocates.
        scratch.cursor.clear();
        for it in iters.iter() {
            let chain = leaf_chain(it);
            if chain.is_empty() {
                return;
            }
            scratch.cursor.push(chain.first());
        }
        loop {
            let candidate = iters
                .iter()
                .zip(&scratch.cursor)
                .map(|(it, &at)| leaf_chain(it).row_at(at));
            if verify_and_construct(
                candidate,
                predicate_variables,
                &mut scratch.row,
                &mut scratch.bound,
            ) {
                emit(&scratch.row);
            }
            if !advance_cursor(iters, &mut scratch.cursor) {
                break;
            }
        }
    }
}

/// Advances the odometer by one candidate: the first relation's cursor steps
/// along its chain, and a cursor that steps past its chain's last row
/// restarts at the chain's first and carries into the next relation's.
/// Returns `false` once every cursor has carried, after the last candidate.
fn advance_cursor<IT: HashTrieIterator>(iters: &[IT], cursor: &mut [LeafCursor]) -> bool {
    for (it, at) in iters.iter().zip(cursor) {
        let chain = leaf_chain(it);
        if chain.advance(at) {
            return true;
        }
        *at = chain.first();
    }
    false
}

/// `it`'s current leaf chain: `emit_leaf` runs with every iterator on an
/// occupied bucket of its last level, or at the pre-root of a trie with no
/// level (#116).
fn leaf_chain<IT: HashTrieIterator>(it: &IT) -> LeafRows<'_> {
    it.leaf_tuples()
        .expect("every iterator is at its last level, or at the pre-root of a trie with no level")
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
    fn join_for_each<S: FnMut(&[usize])>(query: &ExecutedQuery, indexes: &[DS], emit: S) {
        assert_index_count("HashTriejoin", query, indexes.len());
        let predicate_variables = &query.analysis().predicate_variables;
        let variable_ordering = &query.plan().variable_ordering;
        let relations: Vec<&DS> = query
            .atom_indexes()
            .iter()
            .map(|&id| &indexes[id])
            .collect();
        // Each relation participates only at its levels' variables (#116).
        let levels: Vec<usize> = relations.iter().map(|relation| relation.levels()).collect();
        let variable_to_iter_map =
            build_variable_to_iter_map(variable_ordering, predicate_variables, &levels);
        let iters = relations
            .iter()
            .map(|relation| relation.hash_trie_iter())
            .collect();
        Probe::new(iters, predicate_variables, &variable_to_iter_map, emit).enumerate(0);
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            optimiser::{CatalogStats, LexicographicOptimiser, QueryOptimiser, QueryPlan},
            ExecutedQuery,
        },
        kermit_iters::Tuples,
        kermit_parser::JoinQuery,
    };

    /// `query` joined by [`HashTriejoin`] over `indexes`, collected as a
    /// caller collects through `join_for_each`: into a flat [`Tuples`] batch
    /// with one column per query variable. Returns the rows sorted.
    fn joined<DS: HashTrieIterable>(query: &ExecutedQuery, indexes: &[DS]) -> Vec<Vec<usize>> {
        let mut rows = Tuples::new(query.analysis().num_vars);
        HashTriejoin::join_for_each(query, indexes, |row| rows.push(row));
        rows.sort();
        rows.to_vecs()
    }

    #[test]
    fn variable_to_iter_map_triangle() {
        let pred_vars = vec![vec![0, 1], vec![1, 2], vec![0, 2]];
        let map = build_variable_to_iter_map(&[0, 1, 2], &pred_vars, &[2, 2, 2]);
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
        // The `HashTrie` annotations pin its defaulted Layout parameters,
        // which the iterators in `iters` would otherwise leave ambiguous.
        let r: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let s: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![4]]);
        let iters = vec![r.hash_trie_iter(), s.hash_trie_iter()];
        // The setup `join_for_each` derives from `Q(X) :- R(X), S(X).`
        let predicate_variables = vec![vec![0], vec![0]];
        let variable_to_iter_map = vec![vec![0, 1]];
        let mut output = Vec::new();
        Probe::new(
            iters,
            &predicate_variables,
            &variable_to_iter_map,
            |row: &[usize]| output.push(row.to_vec()),
        )
        .enumerate(0);
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
            !verify_and_construct(
                [
                    r_chain.row_at(r_chain.first()),
                    s_chain.row_at(s_chain.first())
                ],
                &pv,
                &mut row,
                &mut bound
            ),
            "Y = 2 in R but 12 in S: the leaf-level check must reject it"
        );
    }

    /// `enumerate` hands every iterator back at its pre-root, including one
    /// whose `open` failed on an empty root: a failed open moves nothing, so
    /// ascending only the iterators that opened restores them all.
    #[test]
    fn enumerate_returns_every_iterator_to_its_pre_root() {
        use kermit_ds::{HashTrie, Relation};
        let r: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![1], vec![2]]);
        let s: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![2]]);
        let empty: HashTrie = HashTrie::from_tuples(1.into(), Vec::<Vec<usize>>::new());
        // `Q(X) :- R(X), S(X).` with S, then with an empty relation in its
        // place.
        let predicate_variables = vec![vec![0], vec![0]];
        let variable_to_iter_map = vec![vec![0, 1]];
        for (other, want) in [(&s, 1), (&empty, 0)] {
            let mut rows = 0;
            let mut probe = Probe::new(
                vec![r.hash_trie_iter(), other.hash_trie_iter()],
                &predicate_variables,
                &variable_to_iter_map,
                |_: &[usize]| rows += 1,
            );
            probe.enumerate(0);
            let below_pre_root: Vec<bool> = probe.iters.iter_mut().map(|it| it.up()).collect();
            assert_eq!(
                below_pre_root,
                [false, false],
                "every iterator at its pre-root"
            );
            drop(probe);
            assert_eq!(rows, want);
        }
    }

    #[test]
    fn join_algo_drops_inner_level_collision_false_positive() {
        use kermit_ds::Relation;
        // R(X, Y) = {(1, 2)}, S(Y, Z) = {(12, 3), (2, 5)}. The Y-level probe
        // matches both S tuples (h(2) == h(12)); only (2, 5) truly joins.
        let r = CollidingHashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let s = CollidingHashTrie::from_tuples(2.into(), vec![vec![12, 3], vec![2, 5]]);
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z).".parse().unwrap();
        // The order the tries' columns follow: X, Y, Z. (Every optimiser
        // plans the join key Y first, #116, which these tries are not laid
        // out for.)
        let plan = QueryPlan {
            variable_ordering: vec![0, 1, 2],
        };
        assert_eq!(
            joined(&ExecutedQuery::new(query, plan).unwrap(), &[r, s]),
            vec![vec![1, 2, 5]]
        );
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
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        assert_eq!(
            joined(&ExecutedQuery::new(query, plan).unwrap(), &[r, s]),
            vec![vec![11]]
        );
    }

    #[test]
    fn join_algo_unary_intersection() {
        use kermit_ds::{HashTrie, Relation};
        let r: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let s = HashTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![4]]);
        let query: JoinQuery = "Q(X) :- R(X), S(X).".parse().unwrap();
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        assert_eq!(
            joined(&ExecutedQuery::new(query, plan).unwrap(), &[r, s]),
            vec![vec![2], vec![3]]
        );
    }

    #[test]
    fn join_algo_triangle() {
        use kermit_ds::{HashTrie, Relation};
        let r: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3], vec![3, 1]]);
        let s = HashTrie::from_tuples(2.into(), vec![vec![2, 3], vec![3, 1], vec![1, 2]]);
        let t = HashTrie::from_tuples(2.into(), vec![vec![1, 3], vec![2, 1], vec![3, 2]]);
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        assert_eq!(
            joined(&ExecutedQuery::new(query, plan).unwrap(), &[r, s, t]),
            vec![vec![1, 2, 3], vec![2, 3, 1], vec![3, 1, 2]]
        );
    }

    /// Algorithm 3 lines 16–19 over three leaf chains of lengths 1, 2 and 3:
    /// `R`'s, `S`'s and `T`'s, one for each of `A`, `B` and `C` below the
    /// shared `X`. `Mod10HashStrategy` puts `S`'s two `B` values (3 and 13)
    /// in one leaf bucket and `T`'s three `C` values (4, 14 and 24) in
    /// another, so the chains are that long, and every one of the 6
    /// candidates verifies: `X` is 1 throughout, and `A`, `B` and `C` each
    /// come from one relation. The odometer turns `R`'s cursor fastest.
    /// `R`'s chain has one row, so every advance carries out of it, and `S`'s
    /// every second advance: each carry must restart the cursor it leaves at
    /// its chain's first row. The candidates come out in odometer order,
    /// each exactly once, every chain in input order.
    #[test]
    fn emit_leaf_resets_each_cursor_on_carry() {
        use kermit_ds::Relation;
        let r = CollidingHashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let s = CollidingHashTrie::from_tuples(2.into(), vec![vec![1, 3], vec![1, 13]]);
        let t =
            CollidingHashTrie::from_tuples(2.into(), vec![vec![1, 4], vec![1, 14], vec![1, 24]]);
        let expected = vec![
            vec![1, 2, 3, 4],
            vec![1, 2, 13, 4],
            vec![1, 2, 3, 14],
            vec![1, 2, 13, 14],
            vec![1, 2, 3, 24],
            vec![1, 2, 13, 24],
        ];

        // `emit_leaf` itself, each iterator walked to its leaf as `enumerate`
        // walks it: `X` at the root, then the relation's own variable.
        let mut iters = vec![r.hash_trie_iter(), s.hash_trie_iter(), t.hash_trie_iter()];
        for it in &mut iters {
            assert!(it.open(), "the X level");
            assert!(it.open(), "the relation's own variable: its leaf");
        }
        let lens: Vec<usize> = iters
            .iter()
            .map(|it| it.leaf_tuples().expect("at its leaf").len())
            .collect();
        assert_eq!(lens, [1, 2, 3], "the chains the test needs");
        let predicate_variables = vec![vec![0, 1], vec![0, 2], vec![0, 3]];
        let variable_to_iter_map = vec![vec![0, 1, 2], vec![0], vec![1], vec![2]];
        let mut out = Vec::new();
        Probe::new(
            iters,
            &predicate_variables,
            &variable_to_iter_map,
            |row: &[usize]| out.push(row.to_vec()),
        )
        .emit_leaf();
        assert_eq!(out, expected, "every candidate once, in odometer order");

        // The same product through the join. The lexicographic plan binds
        // `X`, `A`, `B`, `C` in that order, so `R`, `S` and `T` reach their
        // leaves at depths 1, 2 and 3, and `emit_leaf` runs once, at depth 4.
        let query: JoinQuery = "Q(X, A, B, C) :- R(X, A), S(X, B), T(X, C)."
            .parse()
            .unwrap();
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        assert_eq!(plan.variable_ordering, [0, 1, 2, 3]);
        let executed = ExecutedQuery::new(query, plan).unwrap();
        let mut joined = Vec::new();
        HashTriejoin::join_for_each(&executed, &[r, s, t], |row| joined.push(row.to_vec()));
        assert_eq!(joined, expected);
    }

    type Keys = kermit_ds::HashTrie<
        kermit_iters::SipHashStrategy,
        kermit_ds::NoPruning,
        kermit_ds::EagerExpansion,
        kermit_ds::KeyLevels,
    >;

    /// A `keys` trie of `arity` columns, `levels` of them levelled.
    fn keys_trie(arity: usize, levels: usize, rows: Vec<Vec<usize>>) -> Keys {
        <Keys as kermit_ds::Relation>::from_tuples(
            kermit_ds::RelationHeader::new_nameless_positional(arity).with_levels(levels),
            rows,
        )
    }

    /// Only the levelled columns participate (#116): R(X, P) at one level,
    /// S(X, Y) at two and T(Y) at one, under X, P, Y.
    #[test]
    fn variable_to_iter_map_counts_only_levelled_columns() {
        let pred_vars = vec![vec![0, 1], vec![0, 2], vec![2]];
        let map = build_variable_to_iter_map(&[0, 1, 2], &pred_vars, &[1, 2, 1]);
        assert_eq!(map, vec![vec![0, 1], vec![], vec![1, 2]]);
    }

    /// The #116 design's worked example: P is payload, skipped at its
    /// position and bound from R's leaf rows; X and Y are joined by level.
    /// Under `all`, the same rows.
    #[test]
    fn payload_is_skipped_at_its_position_and_bound_from_the_leaf_rows() {
        let r = vec![vec![1, 10], vec![1, 11], vec![2, 20]];
        let s = vec![vec![1, 5], vec![2, 6]];
        let t = vec![vec![5], vec![7]];
        let (kr, ks, kt) = (
            keys_trie(2, 1, r.clone()),
            keys_trie(2, 2, s.clone()),
            keys_trie(1, 1, t.clone()),
        );
        use kermit_ds::{HashTrie, Relation};
        let (ar, as_, at): (HashTrie, HashTrie, HashTrie) = (
            HashTrie::from_tuples(2.into(), r),
            HashTrie::from_tuples(2.into(), s),
            HashTrie::from_tuples(1.into(), t),
        );
        let want = vec![vec![1, 10, 5], vec![1, 11, 5]];
        let keys = [kr, ks, kt];
        let all = [ar, as_, at];
        // A forced order with the payload before a key, then payload last
        // (what every optimiser plans, #116).
        for order in [vec![0, 1, 2], vec![0, 2, 1]] {
            let query: JoinQuery = "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).".parse().unwrap();
            let plan = QueryPlan {
                variable_ordering: order.clone(),
            };
            let executed = ExecutedQuery::new(query, plan).unwrap();
            assert_eq!(joined(&executed, &keys), want, "keys, {order:?}");
            assert_eq!(joined(&executed, &all), want, "all, {order:?}");
        }
    }

    /// Two atoms with no level: every position is skipped and the leaf
    /// product is their cross product; an empty one leaves none.
    #[test]
    fn atoms_with_no_level_cross_their_whole_chains() {
        let r = || keys_trie(1, 0, vec![vec![1], vec![2]]);
        let s = keys_trie(1, 0, vec![vec![7]]);
        let empty = keys_trie(1, 0, Vec::new());
        let plan = QueryPlan {
            variable_ordering: vec![0, 1],
        };
        let query: JoinQuery = "Q(X, Y) :- R(X), S(Y).".parse().unwrap();
        let executed = ExecutedQuery::new(query, plan).unwrap();
        assert_eq!(joined(&executed, &[r(), s]), vec![vec![1, 7], vec![2, 7]]);
        assert!(joined(&executed, &[r(), empty]).is_empty());
    }

    /// The empty conjunction: its one answer is the empty row.
    #[test]
    fn join_for_each_emits_one_empty_row_for_an_empty_body() {
        use kermit_ds::HashTrie;
        let mut query: JoinQuery = "Q(c1) :- R(c1).".parse().unwrap();
        query.head.terms.clear();
        query.body.clear();
        let plan = QueryPlan {
            variable_ordering: vec![],
        };
        let executed = ExecutedQuery::new(query, plan).unwrap();
        let none: [HashTrie; 0] = [];
        let mut rows: Vec<Vec<usize>> = Vec::new();
        HashTriejoin::join_for_each(&executed, &none, |row| rows.push(row.to_vec()));
        assert_eq!(rows, vec![Vec::<usize>::new()]);
    }

    #[test]
    #[should_panic(expected = "HashTriejoin::join_for_each: 1 indexes for a query that reads 2")]
    fn join_for_each_panics_when_an_index_is_missing() {
        use kermit_ds::{HashTrie, Relation};
        let r: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z).".parse().unwrap();
        let plan = QueryPlan {
            variable_ordering: vec![0, 1, 2],
        };
        let executed = ExecutedQuery::new(query, plan).unwrap();
        HashTriejoin::join_for_each(&executed, &[r], |_| {});
    }
}
