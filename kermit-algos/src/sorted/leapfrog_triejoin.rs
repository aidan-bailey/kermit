//! Leapfrog Triejoin — worst-case-optimal multi-way join over sorted tries.
//!
//! The triejoin coordinates one [`TrieIterator`] per body predicate, descending
//! the relations in lockstep variable by variable. At each depth it delegates
//! the inner intersection to [`LeapfrogJoinIter`]; iterators are swapped in and
//! out of the inner join as the depth changes so that only the relations
//! mentioning the current variable participate.
//!
//! See the original paper: [Leapfrog Triejoin: a worst-case optimal join
//! algorithm](https://arxiv.org/abs/1210.0481).

use {
    crate::{
        analysis::analyse,
        join_algo::JoinAlgo,
        optimiser::QueryPlan,
        sorted::leapfrog_join::{LeapfrogJoinIter, LeapfrogJoinIterator},
    },
    kermit_iters::{LinearIterator, TrieIterable, TrieIterator, TrieIteratorWrapper},
    kermit_parser::JoinQuery,
    std::collections::HashMap,
};

/// Extension of [`LeapfrogJoinIterator`] with trie navigation for the
/// [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481).
pub(crate) trait LeapfrogTriejoinIterator: LeapfrogJoinIterator {
    /// Descends one level in the trie, opening child iterators at the current
    /// key and initializing the leapfrog join at the new depth.
    fn triejoin_open(&mut self) -> bool;

    /// Ascends one level, restoring the leapfrog join state at the parent
    /// depth.
    fn triejoin_up(&mut self) -> bool;
}

/// An iterator that performs the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481).
///
/// Coordinates multiple trie iterators (one per body predicate) to compute a
/// multi-way join. At each variable/depth, only the iterators that mention
/// that variable participate in the inner [`LeapfrogJoinIter`]; iterators
/// are swapped in and out as the depth changes.
///
/// # State machine
///
/// At any moment, every body-predicate iterator is in **exactly one** of two
/// places:
///
/// - in `idle_iterators[i]` as `Some(iter)` — *idle*; not currently joining;
/// - in `leapfrog.iterators` — *active*; participating in the inner leapfrog at
///   the current depth.
///
/// `active_iter_indices` is the list of pool slots currently lent to the
/// leapfrog (parallel to `leapfrog.iterators`, in pop order). The only
/// function that moves iterators between the two places is
/// [`update_iters`](Self::update_iters), called by every depth change
/// ([`triejoin_open`](Self::triejoin_open) /
/// [`triejoin_up`](Self::triejoin_up)). All other code reads — never moves —
/// iterators across this boundary.
///
/// ## Why this deviates from the paper
///
/// Veldhuizen's pseudocode keeps one persistent leapfrog per level over
/// freely-aliased iterator arrays; a depth change just switches which array
/// is consulted. Safe Rust cannot alias owned iterators across levels, so
/// this implementation instead encodes "which iterators participate at the
/// current depth" by *moving* them between the idle pool and the inner
/// [`LeapfrogJoinIter`] (which owns its `Vec` of iterators). The observable
/// join semantics are identical; the cost is a drain/refill and a fresh
/// inner leapfrog per depth change, accepted to keep the implementation in
/// safe, ownership-idiomatic Rust.
pub(crate) struct LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    /// Number of variables in the join (i.e. the maximum depth).
    arity: usize,
    /// Idle trie iterators, indexed by body-predicate position. The slot is
    /// `Some` while the iterator is idle and `None` while it is borrowed by
    /// `leapfrog`.
    idle_iterators: Vec<Option<IT>>,
    /// Indices into `idle_iterators` currently lent to `leapfrog`, parallel
    /// to `leapfrog.iterators` in pop order.
    active_iter_indices: Vec<usize>,
    /// For each depth (`0..arity`), the `idle_iterators` indices of every
    /// iterator that must participate in the leapfrog at that depth.
    variable_to_iter_map: Vec<Vec<usize>>,
    /// Current depth in the join: `0` = uninitialised, `1..=arity` = active.
    depth: usize,
    /// The inner leapfrog join operating at the current depth (empty at
    /// depth 0).
    leapfrog: LeapfrogJoinIter<IT>,
}

impl<IT> LeapfrogJoinIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    fn leapfrog_next(&mut self) -> Option<usize> { self.leapfrog.leapfrog_next() }

    fn key(&self) -> Option<usize> {
        if self.depth == 0 {
            None
        } else {
            self.leapfrog.key()
        }
    }

    fn leapfrog_init(&mut self) -> bool { self.leapfrog.leapfrog_init() }

    fn leapfrog_search(&mut self) -> bool { self.leapfrog.leapfrog_search() }

    fn at_end(&self) -> bool {
        if self.depth == 0 {
            return true;
        }
        self.leapfrog.at_end()
    }

    fn leapfrog_seek(&mut self, seek_key: usize) -> bool { self.leapfrog.leapfrog_seek(seek_key) }
}

impl<IT> LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    /// Constructs a new `LeapfrogTriejoinIter`.
    ///
    /// # Example
    ///
    /// For the join `Q(a, b, c) :- R(a, b), S(b, c), T(a, c)` with variables
    /// numbered `a=0, b=1, c=2`:
    ///
    /// - `variable_ordering = [0, 1, 2]` — iterate `a` at depth 1, `b` at depth
    ///   2, `c` at depth 3.
    /// - `predicate_variables = [[0, 1], [1, 2], [0, 2]]` — `R` carries
    ///   variables `a, b`; `S` carries `b, c`; `T` carries `a, c`.
    /// - `iters` is one trie iterator per body predicate, in the same order as
    ///   `predicate_variables`.
    ///
    /// # Arguments
    ///
    /// * `variable_ordering` — The variable IDs the join descends through, in
    ///   order. Position `d` in this list is depth `d + 1` in the triejoin. The
    ///   arity of the result equals `variable_ordering.len()`.
    /// * `predicate_variables` — One entry per body predicate (in the same
    ///   order as `iters`); each entry lists the variable IDs that predicate
    ///   carries.
    /// * `iters` — Trie iterators, one per body predicate.
    pub(crate) fn new(
        variable_ordering: Vec<usize>, predicate_variables: Vec<Vec<usize>>, iters: Vec<IT>,
    ) -> Self {
        // Build the variable-to-iterator lookup table. For each depth (position
        // in `variable_ordering`), collect the indices of every body predicate
        // that mentions that variable. The triejoin uses this on every depth
        // change to pick which iterators belong in the active leapfrog.
        let mut variable_to_iter_map: Vec<Vec<usize>> = Vec::new();
        for v in &variable_ordering {
            let mut iters_at_this_depth: Vec<usize> = Vec::new();
            for (predicate_i, predicate_vars) in predicate_variables.iter().enumerate() {
                if predicate_vars.contains(v) {
                    iters_at_this_depth.push(predicate_i);
                }
            }
            variable_to_iter_map.push(iters_at_this_depth);
        }

        let idle_iterators = iters.into_iter().map(Some).collect();

        LeapfrogTriejoinIter {
            idle_iterators,
            active_iter_indices: Vec::new(),
            variable_to_iter_map,
            arity: variable_ordering.len(),
            depth: 0,
            leapfrog: LeapfrogJoinIter::new(vec![]),
        }
    }

    /// Restores the [state-machine](Self#state-machine) invariant after a
    /// depth change.
    ///
    /// Called by [`triejoin_open`](Self::triejoin_open) and
    /// [`triejoin_up`](Self::triejoin_up). Two phases:
    ///
    /// 1. **Drain** the existing leapfrog: every active iterator returns to its
    ///    idle slot via `active_iter_indices`.
    /// 2. **Refill** for the new depth: `variable_to_iter_map[depth - 1]` names
    ///    the idle slots whose iterators belong in the new leapfrog; each one
    ///    is taken out of `idle_iterators` and pushed into a fresh
    ///    [`LeapfrogJoinIter`].
    ///
    /// At depth 0 the second phase is skipped — the leapfrog stays empty.
    fn update_iters(&mut self) {
        while let Some(i) = self.active_iter_indices.pop() {
            let iter = self
                .leapfrog
                .iterators
                .pop()
                .expect("There should always be an iterator here");
            self.idle_iterators[i] = Some(iter);
        }

        if self.depth == 0 {
            return;
        }

        let mut next_iters =
            Vec::<IT>::with_capacity(self.variable_to_iter_map[self.depth - 1].len());
        for i in &self.variable_to_iter_map[self.depth - 1] {
            let iter = self.idle_iterators[*i]
                .take()
                .expect("There is an iterator here");
            next_iters.push(iter);
            self.active_iter_indices.push(*i);
        }
        self.leapfrog = LeapfrogJoinIter::new(next_iters);
    }

    /// Undoes a descent that could not be completed, restoring the parent
    /// depth exactly as [`triejoin_up`](Self::triejoin_up) would.
    ///
    /// Only the first `opened` iterators actually descended:
    /// [`TrieIterator::open`] leaves the position unchanged when it returns
    /// `false`, and the iterators after the refusing one were never asked.
    /// Ascending the others would drift them a level above the join.
    fn abandon_descent(&mut self, opened: usize) {
        for iter in self.leapfrog.iterators.iter_mut().take(opened) {
            assert!(
                iter.up(),
                "iterator that descended must be able to move back up (LFTJ invariant)"
            );
        }
        self.depth -= 1;
        self.update_iters();
    }
}

impl<IT> LeapfrogTriejoinIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    /// Descends one variable: takes the iterators that participate at the new
    /// depth, opens each at the current key, and seeds the inner leapfrog.
    ///
    /// Returns `false` if the join is already at maximum depth, if a
    /// participating iterator has no children at this key (that subtree is
    /// exhausted), or if the leapfrog finds no common key at the new depth.
    ///
    /// A `false` return leaves the triejoin exactly where it started: the
    /// depth, the participating iterators and the inner leapfrog are all
    /// restored to the parent level, so the caller may advance or ascend
    /// there without backing out first. That honours [`TrieIterator::open`]'s
    /// contract that a failed open leaves the position unchanged, which
    /// [`TrieIteratorWrapper`] depends on — it pushes nothing when `open`
    /// fails, and would otherwise run one level out of step with the join,
    /// overwriting the wrong key on its tuple stack.
    fn triejoin_open(&mut self) -> bool {
        if self.depth == self.arity {
            return false;
        }
        // Deviation from the paper's `triejoin-open` (open iterators first,
        // then increment depth): here the iterators participating at the new
        // depth are *selected* by `update_iters`, which is keyed on the new
        // depth — so the increment and swap must come before the opens can
        // happen at all. See the state-machine note on the struct for why
        // participation is per-depth rather than the paper's all-iterators.
        self.depth += 1;
        self.update_iters();
        let mut opened = 0;
        for iter in &mut self.leapfrog.iterators {
            if !iter.open() {
                break;
            }
            opened += 1;
        }
        if opened == self.leapfrog.iterators.len() && self.leapfrog_init() {
            return true;
        }
        self.abandon_descent(opened);
        false
    }

    /// Ascends one variable, returning all participating iterators to the
    /// pool and rebuilding the leapfrog at the parent depth.
    ///
    /// Returns `false` (no-op) when already at the root.
    ///
    /// # Panics
    ///
    /// By the LFTJ invariant, every iterator that successfully opened at the
    /// current depth can move back up. A panic here means a participating
    /// trie iterator violated this contract — either the triejoin's depth
    /// tracking and the iterator's state have drifted, or the iterator's
    /// `up` is buggy. Always a programming error, never a recoverable
    /// runtime condition.
    fn triejoin_up(&mut self) -> bool {
        if self.depth == 0 {
            return false;
        }
        for iter in &mut self.leapfrog.iterators {
            assert!(
                iter.up(),
                "iterator must be able to move up from non-root depth (LFTJ invariant)"
            );
        }
        self.depth -= 1;
        self.update_iters();
        true
    }
}

impl<IT> TrieIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    fn open(&mut self) -> bool { self.triejoin_open() }

    fn up(&mut self) -> bool { self.triejoin_up() }
}

impl<IT> LinearIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    fn key(&self) -> Option<usize> { LeapfrogJoinIterator::key(self) }

    fn next(&mut self) -> Option<usize> { self.leapfrog_next() }

    fn seek(&mut self, seek_key: usize) -> bool { self.leapfrog_seek(seek_key) }

    fn at_end(&self) -> bool { LeapfrogJoinIterator::at_end(self) }
}

impl<IT> IntoIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    type IntoIter = TrieIteratorWrapper<Self>;
    type Item = Vec<usize>;

    fn into_iter(self) -> Self::IntoIter {
        let arity = self.arity;
        TrieIteratorWrapper::with_arity(self, arity)
    }
}

/// Entry point for the Leapfrog Triejoin algorithm, implementing
/// [`JoinAlgo`](crate::JoinAlgo) for any [`TrieIterable`] data structure.
pub struct LeapfrogTriejoin {}

impl<DS> JoinAlgo<DS> for LeapfrogTriejoin
where
    DS: TrieIterable,
{
    fn join_iter(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>> {
        let analysis = analyse(&query);
        if let Err(e) = plan.validate(&analysis) {
            panic!("LeapfrogTriejoin::join_iter: invalid query plan: {e}");
        }
        let variable_ordering = plan.variable_ordering.clone();
        let predicate_variables = analysis.predicate_variables;

        let trie_iters: Vec<_> = query
            .body
            .iter()
            .map(|pred| {
                let ds = datastructures
                    .get(&pred.name)
                    .expect("Missing datastructure for predicate name");
                ds.trie_iter()
            })
            .collect();

        // The triejoin descends in `variable_ordering` (a valid descent order)
        // and yields tuples in *descent* order. Map descent position back to
        // canonical variable index so output columns stay in head-first order
        // regardless of the descent order chosen.
        let arity = variable_ordering.len();
        let mut descent_pos_of_var = vec![0usize; arity];
        for (pos, &v) in variable_ordering.iter().enumerate() {
            descent_pos_of_var[v] = pos;
        }

        LeapfrogTriejoinIter::new(variable_ordering, predicate_variables, trie_iters)
            .into_iter()
            .map(move |descent_tuple| {
                (0..arity)
                    .map(|v| descent_tuple[descent_pos_of_var[v]])
                    .collect()
            })
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::sorted::{
            leapfrog_join::LeapfrogJoinIterator,
            leapfrog_triejoin::{LeapfrogTriejoinIter, LeapfrogTriejoinIterator},
        },
        kermit_ds::{Relation, TreeTrie},
        kermit_iters::TrieIterable,
    };

    /// Collect triejoin results end-to-end via `into_iter().collect()`.
    fn triejoin_collect(
        variable_ordering: Vec<usize>, predicate_variables: Vec<Vec<usize>>,
        relations: Vec<&TreeTrie>,
    ) -> Vec<Vec<usize>> {
        let iters: Vec<_> = relations.iter().map(|r| r.trie_iter()).collect();
        LeapfrogTriejoinIter::new(variable_ordering, predicate_variables, iters)
            .into_iter()
            .collect()
    }

    // -- Original manual-stepping tests --

    #[test]
    fn test_classic() {
        let t1 = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let t2 = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let t1_iter = t1.trie_iter();
        let t2_iter = t2.trie_iter();
        let mut triejoin_iter =
            LeapfrogTriejoinIter::new(vec![0], vec![vec![0], vec![0]], vec![t1_iter, t2_iter]);
        triejoin_iter.triejoin_open();
        assert_eq!(triejoin_iter.key(), Some(1));
        assert_eq!(triejoin_iter.leapfrog_next(), Some(2));
        assert_eq!(triejoin_iter.leapfrog_next(), Some(3));
        triejoin_iter.leapfrog_next();
        assert!(triejoin_iter.at_end());
        triejoin_iter.triejoin_up();
        assert!(triejoin_iter.at_end());
        let res = triejoin_iter.into_iter().collect::<Vec<_>>();
        assert_eq!(res, vec![vec![1], vec![2], vec![3]]);
    }

    #[test]
    fn more_complicated() {
        let r = TreeTrie::from_tuples(2.into(), vec![vec![7, 4]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![4, 1], vec![4, 4], vec![4, 5], vec![
            4, 9,
        ]]);
        let t = TreeTrie::from_tuples(2.into(), vec![vec![7, 2], vec![7, 3], vec![7, 5]]);
        let r_iter = r.trie_iter();
        let s_iter = s.trie_iter();
        let t_iter = t.trie_iter();
        let mut triejoin_iter = LeapfrogTriejoinIter::new(
            vec![0, 1, 2],
            vec![vec![0, 1], vec![1, 2], vec![0, 2]],
            vec![r_iter, s_iter, t_iter],
        );
        triejoin_iter.triejoin_open();
        assert_eq!(triejoin_iter.key().unwrap().clone(), 7);
        triejoin_iter.leapfrog_next();
        assert!(triejoin_iter.at_end());
        triejoin_iter.triejoin_open();
        assert_eq!(triejoin_iter.key().unwrap().clone(), 4);
        triejoin_iter.leapfrog_next();
        assert!(triejoin_iter.at_end());
        triejoin_iter.triejoin_open();
        assert_eq!(triejoin_iter.key().unwrap().clone(), 5);
    }

    #[test]
    fn chain() {
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![2, 4], vec![3, 5]]);
        let t = TreeTrie::from_tuples(2.into(), vec![vec![4, 6], vec![5, 7]]);
        let r_iter = r.trie_iter();
        let s_iter = s.trie_iter();
        let t_iter = t.trie_iter();
        let mut triejoin_iter = LeapfrogTriejoinIter::new(
            vec![0, 1, 2, 3],
            vec![vec![0, 1], vec![1, 2], vec![2, 3]],
            vec![r_iter, s_iter, t_iter],
        );
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(1));
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(2));
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(4));
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(6));

        assert!(triejoin_iter.triejoin_up());
        assert!(triejoin_iter.triejoin_up());
        assert!(triejoin_iter.triejoin_up());

        assert_eq!(triejoin_iter.leapfrog_next(), Some(2));
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(3));
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(5));
        assert!(triejoin_iter.triejoin_open());
        assert_eq!(triejoin_iter.key(), Some(7));
    }

    // -- End-to-end collect tests --

    #[test]
    fn unary_intersection() {
        let r = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let s = TreeTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![4]]);
        assert_eq!(
            triejoin_collect(vec![0], vec![vec![0], vec![0]], vec![&r, &s]),
            vec![vec![2], vec![3]],
        );
    }

    #[test]
    fn unary_no_match() {
        let r = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2]]);
        let s = TreeTrie::from_tuples(1.into(), vec![vec![3], vec![4]]);
        assert_eq!(
            triejoin_collect(vec![0], vec![vec![0], vec![0]], vec![&r, &s]),
            Vec::<Vec<usize>>::new(),
        );
    }

    #[test]
    fn unary_empty_relation() {
        let r = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let s = TreeTrie::from_tuples(1.into(), vec![]);
        assert_eq!(
            triejoin_collect(vec![0], vec![vec![0], vec![0]], vec![&r, &s]),
            Vec::<Vec<usize>>::new(),
        );
    }

    #[test]
    fn unary_single_match() {
        let r = TreeTrie::from_tuples(1.into(), vec![vec![5]]);
        let s = TreeTrie::from_tuples(1.into(), vec![vec![5]]);
        assert_eq!(
            triejoin_collect(vec![0], vec![vec![0], vec![0]], vec![&r, &s]),
            vec![vec![5]],
        );
    }

    #[test]
    fn three_way_unary() {
        let r = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3], vec![4]]);
        let s = TreeTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![5]]);
        let t = TreeTrie::from_tuples(1.into(), vec![vec![3], vec![4], vec![5]]);
        assert_eq!(
            triejoin_collect(vec![0], vec![vec![0], vec![0], vec![0]], vec![&r, &s, &t]),
            vec![vec![3]],
        );
    }

    #[test]
    fn binary_natural_join() {
        // R(a,b) ⋈ S(b,c)
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![2, 4], vec![3, 5]]);
        assert_eq!(
            triejoin_collect(vec![0, 1, 2], vec![vec![0, 1], vec![1, 2]], vec![&r, &s]),
            vec![vec![1, 2, 4], vec![1, 3, 5]],
        );
    }

    #[test]
    fn no_match_at_shared_variable() {
        // R(a,b) ⋈ S(a,c) where a values are disjoint — mismatch at the
        // depth where both relations participate, producing correct empty
        // result
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![3, 4]]);
        assert_eq!(
            triejoin_collect(vec![0, 1, 2], vec![vec![0, 1], vec![0, 2]], vec![&r, &s]),
            Vec::<Vec<usize>>::new(),
        );
    }

    #[test]
    fn triangle_join_collect() {
        // Q(a,b,c) :- R(a,b), S(b,c), T(a,c)
        let r = TreeTrie::from_tuples(2.into(), vec![vec![7, 4]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![4, 1], vec![4, 4], vec![4, 5], vec![
            4, 9,
        ]]);
        let t = TreeTrie::from_tuples(2.into(), vec![vec![7, 2], vec![7, 3], vec![7, 5]]);
        assert_eq!(
            triejoin_collect(
                vec![0, 1, 2],
                vec![vec![0, 1], vec![1, 2], vec![0, 2]],
                vec![&r, &s, &t],
            ),
            vec![vec![7, 4, 5]],
        );
    }

    #[test]
    fn star_join() {
        // Q(a,b,c) :- R(a,b), S(a,c) — star pattern on variable a
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![1, 5], vec![1, 6], vec![2, 7]]);
        assert_eq!(
            triejoin_collect(vec![0, 1, 2], vec![vec![0, 1], vec![0, 2]], vec![&r, &s]),
            vec![
                vec![1, 2, 5],
                vec![1, 2, 6],
                vec![1, 3, 5],
                vec![1, 3, 6],
                vec![2, 4, 7],
            ],
        );
    }

    #[test]
    fn self_join() {
        // R(a,b) ⋈ R(b,c) — self-join where every a value has a matching b
        // chain
        let r1 = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 1]]);
        let r2 = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 1]]);
        assert_eq!(
            triejoin_collect(vec![0, 1, 2], vec![vec![0, 1], vec![1, 2]], vec![&r1, &r2]),
            vec![vec![1, 2, 1], vec![2, 1, 2]],
        );
    }

    #[test]
    fn four_way_chain() {
        // Q(a,b,c,d,e) :- R(a,b), S(b,c), T(c,d), U(d,e)
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![2, 3]]);
        let t = TreeTrie::from_tuples(2.into(), vec![vec![3, 4]]);
        let u = TreeTrie::from_tuples(2.into(), vec![vec![4, 5]]);
        assert_eq!(
            triejoin_collect(
                vec![0, 1, 2, 3, 4],
                vec![vec![0, 1], vec![1, 2], vec![2, 3], vec![3, 4]],
                vec![&r, &s, &t, &u],
            ),
            vec![vec![1, 2, 3, 4, 5]],
        );
    }

    #[test]
    fn single_relation_passthrough() {
        // Join with just one relation returns all its tuples
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(
            triejoin_collect(vec![0, 1], vec![vec![0, 1]], vec![&r]),
            vec![vec![1, 2], vec![3, 4]],
        );
    }

    #[test]
    fn column_trie_binary_join() {
        use kermit_ds::ColumnTrie;
        let r = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3]]);
        let s = ColumnTrie::from_tuples(2.into(), vec![vec![2, 4], vec![3, 5]]);
        let r_iter = r.trie_iter();
        let s_iter = s.trie_iter();
        let result: Vec<Vec<usize>> =
            LeapfrogTriejoinIter::new(vec![0, 1, 2], vec![vec![0, 1], vec![1, 2]], vec![
                r_iter, s_iter,
            ])
            .into_iter()
            .collect();
        assert_eq!(result, vec![vec![1, 2, 4], vec![1, 3, 5]]);
    }

    #[test]
    fn binary_no_match_regression() {
        // Regression: R(a,b) ⋈ S(b,c) where b values are disjoint.
        // Previously emitted partial tuple [1] because triejoin_open
        // incremented depth before validating the leapfrog at depth 2
        // (variable b), and TrieIteratorWrapper returned the incomplete
        // stack.
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![3, 4]]);
        assert_eq!(
            triejoin_collect(vec![0, 1, 2], vec![vec![0, 1], vec![1, 2]], vec![&r, &s]),
            Vec::<Vec<usize>>::new(),
        );
    }

    #[test]
    fn self_join_with_dead_ends() {
        // Regression: R(a,b) ⋈ R(b,c) where some a values don't chain.
        // R = {(1,2),(2,3),(3,4)} — only a=1→b=2→c=3 and a=2→b=3→c=4 produce
        // full 3-tuples. a=3→b=4 has no continuation in R(b=4,...).
        // Previously emitted spurious partial tuples like [3].
        let r1 = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3], vec![3, 4]]);
        let r2 = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3], vec![3, 4]]);
        assert_eq!(
            triejoin_collect(vec![0, 1, 2], vec![vec![0, 1], vec![1, 2]], vec![&r1, &r2]),
            vec![vec![1, 2, 3], vec![2, 3, 4]],
        );
    }

    #[test]
    fn chain_join_collect() {
        // Q(a,b,c,d) :- R(a,b), S(b,c), T(c,d) — end-to-end collect
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![2, 4], vec![3, 5]]);
        let t = TreeTrie::from_tuples(2.into(), vec![vec![4, 6], vec![5, 7]]);
        assert_eq!(
            triejoin_collect(
                vec![0, 1, 2, 3],
                vec![vec![0, 1], vec![1, 2], vec![2, 3]],
                vec![&r, &s, &t],
            ),
            vec![vec![1, 2, 4, 6], vec![2, 3, 5, 7]],
        );
    }

    /// Like `triejoin_collect`, but maps each tuple from descent order back
    /// to canonical variable order (as `join_iter` does) and sorts, so
    /// results from different variable orderings are directly comparable.
    fn triejoin_canonical(
        variable_ordering: Vec<usize>, predicate_variables: Vec<Vec<usize>>,
        relations: Vec<&TreeTrie>,
    ) -> Vec<Vec<usize>> {
        let arity = variable_ordering.len();
        let mut descent_pos_of_var = vec![0usize; arity];
        for (pos, &v) in variable_ordering.iter().enumerate() {
            descent_pos_of_var[v] = pos;
        }
        let mut tuples: Vec<Vec<usize>> =
            triejoin_collect(variable_ordering, predicate_variables, relations)
                .into_iter()
                .map(|t| (0..arity).map(|v| t[descent_pos_of_var[v]]).collect())
                .collect();
        tuples.sort();
        tuples
    }

    #[test]
    fn valid_orderings_agree_when_a_deep_open_fails() {
        // Regression (LUBM q7 shape, minimised to 9 tuples). A descent that
        // fails at depth >= 3 used to leave `depth` incremented, desyncing
        // the triejoin from the tuple stack in `TrieIteratorWrapper` and
        // silently dropping every answer under some valid orderings.
        //
        // Q(X, Y) :- type(X, K0), type(Y, K1), takescourse(X, Y),
        //            teacherof(K2, Y), c21(K0), c141(K1), c1688(K2).
        // Variables: X=0, Y=1, K0=2, K1=3, K2=4.
        let type_rel = TreeTrie::from_tuples(2.into(), vec![
            vec![8433, 21],
            vec![8433, 83],
            vec![8435, 21],
            vec![8514, 141],
        ]);
        let takescourse =
            TreeTrie::from_tuples(2.into(), vec![vec![8433, 5687], vec![8433, 13069], vec![
                8435, 8514,
            ]]);
        let teacherof = TreeTrie::from_tuples(2.into(), vec![vec![1688, 8009], vec![1688, 8514]]);
        let c21 = TreeTrie::from_tuples(1.into(), vec![vec![21]]);
        let c141 = TreeTrie::from_tuples(1.into(), vec![vec![141]]);
        let c1688 = TreeTrie::from_tuples(1.into(), vec![vec![1688]]);

        let predicate_variables = vec![
            vec![0, 2],
            vec![1, 3],
            vec![0, 1],
            vec![4, 1],
            vec![2],
            vec![3],
            vec![4],
        ];
        let relations = vec![
            &type_rel,
            &type_rel,
            &takescourse,
            &teacherof,
            &c21,
            &c141,
            &c1688,
        ];

        // The only answer: X=8435 takes course Y=8514, taught by 1688.
        let expected = vec![vec![8435, 8514, 21, 141, 1688]];

        // Lexicographic order: its first failed descent lands at depth 2.
        assert_eq!(
            triejoin_canonical(
                vec![0, 2, 4, 1, 3],
                predicate_variables.clone(),
                relations.clone(),
            ),
            expected,
            "lexicographic ordering"
        );
        // Cardinality order: binds the constant first, so the first failed
        // descent lands at depth 3 — the case that used to return nothing.
        assert_eq!(
            triejoin_canonical(vec![4, 0, 2, 1, 3], predicate_variables, relations),
            expected,
            "cardinality ordering"
        );
    }
}
