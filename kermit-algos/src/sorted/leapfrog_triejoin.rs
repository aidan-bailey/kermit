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
        executed::ExecutedQuery,
        join_algo::{assert_index_count, JoinAlgo},
        sorted::leapfrog_join::LeapfrogJoinIter,
    },
    kermit_iters::{LinearIterator, TrieIterable, TrieIterator, TrieIteratorWrapper},
};

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
/// `active_iter_indices` names the slot each active iterator was lent from
/// (parallel to `leapfrog.iterators`). The only function that moves
/// iterators between the two places is [`update_iters`](Self::update_iters),
/// called by every depth change ([`triejoin_open`](Self::triejoin_open) /
/// [`triejoin_up`](Self::triejoin_up)).
///
/// ## Why this deviates from the paper
///
/// Veldhuizen's pseudocode keeps one persistent leapfrog per level over
/// freely-aliased iterator arrays; a depth change just switches which array
/// is consulted. Safe Rust cannot alias owned iterators across levels, so
/// this implementation instead encodes "which iterators participate at the
/// current depth" by *moving* them between the idle pool and the inner
/// [`LeapfrogJoinIter`] (which owns its `Vec` of iterators). The observable
/// join semantics are identical; the cost is a drain/refill per depth
/// change, accepted to keep the implementation in safe, ownership-idiomatic
/// Rust. The refill reuses the one inner leapfrog's buffers, so a depth
/// change moves iterators but allocates nothing (issue #83).
pub(crate) struct LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    /// Number of variables in the join (i.e. the maximum depth).
    arity: usize,
    /// Idle trie iterators, indexed by body-predicate position. The slot is
    /// `Some` while the iterator is idle and `None` while it is lent to
    /// `leapfrog`.
    idle_iterators: Vec<Option<IT>>,
    /// The `idle_iterators` slot each iterator in `leapfrog.iterators` was
    /// lent from, in the same order.
    active_iter_indices: Vec<usize>,
    /// `participants[d - 1]`: the `idle_iterators` slots of every iterator
    /// whose predicate carries the variable bound at depth `d` (the paper's
    /// per-variable leapfrog joins, §3.5).
    participants: Vec<Vec<usize>>,
    /// Current depth in the join: `0` = the root, `1..=arity` = a variable.
    depth: usize,
    /// The inner leapfrog join operating at the current depth (empty at
    /// depth 0).
    leapfrog: LeapfrogJoinIter<IT>,
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
        variable_ordering: &[usize], predicate_variables: &[Vec<usize>], iters: Vec<IT>,
    ) -> Self {
        let participants = variable_ordering
            .iter()
            .map(|v| {
                (0..predicate_variables.len())
                    .filter(|&i| predicate_variables[i].contains(v))
                    .collect()
            })
            .collect();
        LeapfrogTriejoinIter {
            arity: variable_ordering.len(),
            idle_iterators: iters.into_iter().map(Some).collect(),
            active_iter_indices: Vec::new(),
            participants,
            depth: 0,
            leapfrog: LeapfrogJoinIter::new(vec![]),
        }
    }

    /// Restores the [state-machine](Self#state-machine) invariant after a
    /// depth change: returns every active iterator to its idle slot, then
    /// lends the new depth's participants to the leapfrog through
    /// [`refill`](LeapfrogJoinIter::refill), which reuses its buffers and so
    /// saves two allocations and two frees per depth change (issue #83). At
    /// depth 0 the leapfrog stays empty.
    fn update_iters(&mut self) {
        debug_assert_eq!(self.active_iter_indices.len(), self.leapfrog.k());
        let lent = self.active_iter_indices.drain(..);
        for (slot, iter) in lent.zip(self.leapfrog.iterators.drain(..)) {
            self.idle_iterators[slot] = Some(iter);
        }
        if self.depth == 0 {
            return;
        }
        let participants = &self.participants[self.depth - 1];
        let idle_iterators = &mut self.idle_iterators;
        self.leapfrog.refill(participants.iter().map(|&slot| {
            idle_iterators[slot]
                .take()
                .expect("a participant is idle until it is lent")
        }));
        self.active_iter_indices.extend_from_slice(participants);
    }

    /// Ascends the first `opened` active iterators, the ones that descended
    /// to the current depth, and returns to the parent depth.
    ///
    /// # Panics
    ///
    /// If one of them cannot move back up. Every iterator that opened can,
    /// so a panic here means the triejoin's depth and an iterator's have
    /// drifted apart, or the iterator's `up` is buggy: always a programming
    /// error.
    fn ascend(&mut self, opened: usize) {
        for iter in &mut self.leapfrog.iterators[..opened] {
            assert!(
                iter.up(),
                "an iterator that descended must be able to move back up (LFTJ invariant)"
            );
        }
        self.depth -= 1;
        self.update_iters();
    }

    /// Algorithm 5, `triejoin-open`: descends one variable, opening each
    /// participant of the new depth at its current key and seeding the
    /// leapfrog there.
    ///
    /// Returns `false` if every variable is already bound, if a participant
    /// has no child at its key, or if the participants share no key at the
    /// new depth.
    ///
    /// A `false` return leaves the triejoin exactly where it started: the
    /// depth, the participants and the leapfrog are all restored to the
    /// parent level, so the caller may advance or ascend there without
    /// backing out first. That honours [`TrieIterator::open`]'s contract that
    /// a failed open leaves the position unchanged, which
    /// [`TrieIteratorWrapper`] depends on: it pushes nothing when `open`
    /// fails, and would otherwise run one level out of step with the join,
    /// overwriting the wrong key on its tuple stack.
    fn triejoin_open(&mut self) -> bool {
        if self.depth == self.arity {
            return false;
        }
        // As in the paper, the depth rises before the opens. Here that is
        // also what selects the iterators to open: `update_iters` lends the
        // new depth's participants to the leapfrog.
        self.depth += 1;
        self.update_iters();
        // Open each participant in turn, stopping at the first that refuses
        // (its position is then unchanged, and the later ones are not asked).
        let opened = self
            .leapfrog
            .iterators
            .iter_mut()
            .map(|iter| iter.open())
            .take_while(|&opened| opened)
            .count();
        if opened == self.leapfrog.k() && self.leapfrog.leapfrog_init() {
            return true;
        }
        self.ascend(opened);
        false
    }

    /// Algorithm 6, `triejoin-up`: ascends one variable. Returns `false`
    /// (and does nothing) at the root.
    fn triejoin_up(&mut self) -> bool {
        if self.depth == 0 {
            return false;
        }
        self.ascend(self.leapfrog.k());
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

/// The linear-iterator operations are the current depth's leapfrog join's
/// (§3.5). At the root, as in the paper, only `open` is permitted: `key` is
/// `None` and `at_end` is `true` there, which is all [`TrieIteratorWrapper`]
/// asks before it opens.
impl<IT> LinearIterator for LeapfrogTriejoinIter<IT>
where
    IT: TrieIterator,
{
    fn key(&self) -> Option<usize> {
        if self.depth == 0 {
            None
        } else {
            self.leapfrog.key()
        }
    }

    fn next(&mut self) -> Option<usize> { self.leapfrog.leapfrog_next() }

    fn seek(&mut self, seek_key: usize) -> bool { self.leapfrog.leapfrog_seek(seek_key) }

    fn at_end(&self) -> bool { self.depth == 0 || self.leapfrog.at_end() }
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
/// [`JoinAlgo`] for any [`TrieIterable`] data structure.
pub struct LeapfrogTriejoin {}

impl<DS> JoinAlgo<DS> for LeapfrogTriejoin
where
    DS: TrieIterable,
{
    fn join_for_each<S: FnMut(&[usize])>(query: &ExecutedQuery, indexes: &[DS], mut emit: S) {
        assert_index_count("LeapfrogTriejoin", query, indexes.len());
        if query.atom_indexes().is_empty() {
            // The empty conjunction: one answer, the empty row.
            emit(&[]);
            return;
        }
        let trie_iters: Vec<_> = query
            .atom_indexes()
            .iter()
            .map(|&id| indexes[id].trie_iter())
            .collect();
        let order = &query.plan().variable_ordering;
        let mut descents =
            LeapfrogTriejoinIter::new(order, &query.analysis().predicate_variables, trie_iters)
                .into_iter();

        // The triejoin yields each tuple in descent order: position `pos`
        // binds variable `order[pos]`. Scatter it into one scratch row in
        // canonical (head-first) order, so no result tuple is ever allocated
        // (issue #65).
        let mut tuple = vec![0usize; order.len()];
        while let Some(descent_tuple) = descents.advance() {
            for (&v, &key) in order.iter().zip(descent_tuple) {
                tuple[v] = key;
            }
            emit(&tuple);
        }
    }
}

// These tests stay inline deliberately, despite using `kermit-ds` and being
// roughly half this file. They drive `LeapfrogTriejoinIter` directly — a
// `pub(crate)` type — rather than the public
// `LeapfrogTriejoin::join_for_each`, so moving them to `tests/` would mean
// exporting the iterator purely to serve tests. Keeping
// the iterator internal is the deliberate choice; the test placement follows
// from it.
#[cfg(test)]
mod tests {
    use {
        crate::{
            optimiser::QueryPlan, sorted::leapfrog_triejoin::LeapfrogTriejoinIter, ExecutedQuery,
            JoinAlgo, LeapfrogTriejoin,
        },
        kermit_ds::{Relation, TreeTrie},
        kermit_iters::{LinearIterator, TrieIterable, TrieIterator},
        kermit_parser::JoinQuery,
    };

    /// Collect triejoin results end-to-end via `into_iter().collect()`.
    fn triejoin_collect(
        variable_ordering: Vec<usize>, predicate_variables: Vec<Vec<usize>>,
        relations: Vec<&TreeTrie>,
    ) -> Vec<Vec<usize>> {
        let iters: Vec<_> = relations.iter().map(|r| r.trie_iter()).collect();
        LeapfrogTriejoinIter::new(&variable_ordering, &predicate_variables, iters)
            .into_iter()
            .collect()
    }

    // -- Original manual-stepping tests --

    #[test]
    fn test_classic() {
        let t1: TreeTrie = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let t2 = TreeTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let t1_iter = t1.trie_iter();
        let t2_iter = t2.trie_iter();
        let mut triejoin_iter =
            LeapfrogTriejoinIter::new(&[0], &[vec![0], vec![0]], vec![t1_iter, t2_iter]);
        triejoin_iter.triejoin_open();
        assert_eq!(triejoin_iter.key(), Some(1));
        assert_eq!(triejoin_iter.next(), Some(2));
        assert_eq!(triejoin_iter.next(), Some(3));
        triejoin_iter.next();
        assert!(triejoin_iter.at_end());
        triejoin_iter.triejoin_up();
        assert!(triejoin_iter.at_end());
        let res = triejoin_iter.into_iter().collect::<Vec<_>>();
        assert_eq!(res, vec![vec![1], vec![2], vec![3]]);
    }

    /// The triangle `R(a, b), S(b, c), T(a, c)` stepped by hand, as
    /// `TrieIteratorWrapper` steps it: each `triejoin_open` binds one more
    /// variable, and on the way back each depth is exhausted before it is
    /// left. `T` participates at depths 1 and 3 but not 2.
    #[test]
    fn triangle_stepped_by_hand() {
        let r: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![7, 4]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![4, 1], vec![4, 4], vec![4, 5], vec![
            4, 9,
        ]]);
        let t = TreeTrie::from_tuples(2.into(), vec![vec![7, 2], vec![7, 3], vec![7, 5]]);
        let mut triejoin_iter =
            LeapfrogTriejoinIter::new(&[0, 1, 2], &[vec![0, 1], vec![1, 2], vec![0, 2]], vec![
                r.trie_iter(),
                s.trie_iter(),
                t.trie_iter(),
            ]);
        for key in [7, 4, 5] {
            assert!(triejoin_iter.triejoin_open());
            assert_eq!(triejoin_iter.key(), Some(key));
        }
        assert!(!triejoin_iter.triejoin_open(), "every variable is bound");
        assert_eq!(triejoin_iter.key(), Some(5), "a refused open moves nothing");
        for _ in 0..3 {
            assert_eq!(triejoin_iter.next(), None);
            assert!(triejoin_iter.at_end());
            assert!(triejoin_iter.triejoin_up());
        }
        assert!(!triejoin_iter.triejoin_up(), "already at the root");
    }

    #[test]
    fn chain() {
        let r: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![2, 4], vec![3, 5]]);
        let t = TreeTrie::from_tuples(2.into(), vec![vec![4, 6], vec![5, 7]]);
        let r_iter = r.trie_iter();
        let s_iter = s.trie_iter();
        let t_iter = t.trie_iter();
        let mut triejoin_iter =
            LeapfrogTriejoinIter::new(&[0, 1, 2, 3], &[vec![0, 1], vec![1, 2], vec![2, 3]], vec![
                r_iter, s_iter, t_iter,
            ]);
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

        assert_eq!(triejoin_iter.next(), Some(2));
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
        let r: ColumnTrie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3]]);
        let s = ColumnTrie::from_tuples(2.into(), vec![vec![2, 4], vec![3, 5]]);
        let r_iter = r.trie_iter();
        let s_iter = s.trie_iter();
        let result: Vec<Vec<usize>> =
            LeapfrogTriejoinIter::new(&[0, 1, 2], &[vec![0, 1], vec![1, 2]], vec![r_iter, s_iter])
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
    /// to canonical variable order (as `join_for_each` does) and sorts, so
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

    /// `Q(X, Y) :- R(Y, X).` with the descent order Y, X (R's column
    /// order) against the head order X, Y, so `join_for_each` must reorder
    /// every descent tuple into its scratch row before emitting it.
    #[test]
    fn join_for_each_emits_head_order_under_a_reordering_plan() {
        let r: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![1, 10], vec![2, 20]]);
        let query: JoinQuery = "Q(X, Y) :- R(Y, X).".parse().unwrap();
        // No column order constrains an optimiser any more, so the plan that
        // descends Y first is written out.
        let plan = QueryPlan {
            variable_ordering: vec![1, 0],
        };
        let executed = ExecutedQuery::new(query, plan).unwrap();
        let mut rows = Vec::new();
        LeapfrogTriejoin::join_for_each(&executed, &[r], |tuple| rows.push(tuple.to_vec()));
        assert_eq!(rows, vec![vec![10, 1], vec![20, 2]]);
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
        let none: [TreeTrie; 0] = [];
        let mut rows: Vec<Vec<usize>> = Vec::new();
        LeapfrogTriejoin::join_for_each(&executed, &none, |row| rows.push(row.to_vec()));
        assert_eq!(rows, vec![Vec::<usize>::new()]);
    }

    #[test]
    #[should_panic(
        expected = "LeapfrogTriejoin::join_for_each: 1 indexes for a query that reads 2"
    )]
    fn join_for_each_panics_when_an_index_is_missing() {
        let r: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z).".parse().unwrap();
        let plan = QueryPlan {
            variable_ordering: vec![0, 1, 2],
        };
        let executed = ExecutedQuery::new(query, plan).unwrap();
        LeapfrogTriejoin::join_for_each(&executed, &[r], |_| {});
    }

    /// A trie iterator that panics if it is opened at its end, below its
    /// root (where a trie iterator reports `at_end` before its first open).
    struct NoOpenAtEnd<IT> {
        inner: IT,
        depth: usize,
    }

    impl<IT: TrieIterator> LinearIterator for NoOpenAtEnd<IT> {
        fn key(&self) -> Option<usize> { self.inner.key() }

        fn next(&mut self) -> Option<usize> { self.inner.next() }

        fn seek(&mut self, seek_key: usize) -> bool { self.inner.seek(seek_key) }

        fn at_end(&self) -> bool { self.inner.at_end() }
    }

    impl<IT: TrieIterator> TrieIterator for NoOpenAtEnd<IT> {
        fn open(&mut self) -> bool {
            assert!(
                self.depth == 0 || !self.inner.at_end(),
                "the triejoin opened an exhausted iterator"
            );
            let opened = self.inner.open();
            self.depth += usize::from(opened);
            opened
        }

        fn up(&mut self) -> bool {
            let ascended = self.inner.up();
            self.depth -= usize::from(ascended);
            ascended
        }
    }

    /// Driven by its wrapper, the triejoin descends only from a key: every
    /// participant of the depth it leaves holds the common key, and every
    /// other iterator still holds the key of its own last depth. So an index
    /// structure need not support `open` after `at_end`; it must support
    /// `up` after `at_end`, which every exhausted depth calls. The fixtures
    /// exhaust every depth, fail descents at depths 2 and 3 (the chain's
    /// `a = 3` has no `b`, the triangle's `a = 1` no `c`), and have `T` skip
    /// depth 2.
    #[test]
    fn never_opens_an_exhausted_iterator() {
        let chain: TreeTrie =
            TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3], vec![3, 4]]);
        let r: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![1, 4], vec![7, 4]]);
        let s = TreeTrie::from_tuples(2.into(), vec![vec![4, 1], vec![4, 5], vec![4, 9]]);
        let t = TreeTrie::from_tuples(2.into(), vec![vec![1, 3], vec![7, 2], vec![7, 5]]);
        let count = |atoms: &[Vec<usize>], relations: &[&TreeTrie]| {
            let iters = relations
                .iter()
                .map(|r| NoOpenAtEnd {
                    inner: r.trie_iter(),
                    depth: 0,
                })
                .collect();
            LeapfrogTriejoinIter::new(&[0, 1, 2], atoms, iters)
                .into_iter()
                .count()
        };
        assert_eq!(count(&[vec![0, 1], vec![1, 2]], &[&chain, &chain]), 2);
        assert_eq!(
            count(&[vec![0, 1], vec![1, 2], vec![0, 2]], &[&r, &s, &t]),
            1
        );
    }
}
