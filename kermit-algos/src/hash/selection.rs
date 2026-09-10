//! Equality-selection view over a hash trie iterator, used by the
//! selection rewrite (see [`crate::selection_rewrite`]).
//!
//! Mirror of [`crate::sorted::EqualitySelectionTrieIter`] for the hash
//! family. Presents `σ_{col_i = col_j}(r)` without materialising it: every
//! level delegates to the inner iterator, except that a *constrained*
//! level `j` admits exactly one bucket — the one whose hash this iterator
//! itself passed at its source level `i`.
//!
//! Hash equality is not value equality, so the constrained level can admit
//! a colliding bucket. The view therefore also filters the leaf chain by
//! value: "hash collision rejection is at the leaves" (the
//! [`HashTriejoin`](crate::HashTriejoin) invariant) applies inside the view
//! as well as across relations.

use {
    crate::selection_rewrite::ColumnEquality,
    kermit_iters::HashTrieIterator,
};

/// Bookkeeping for one open level of the inner iterator.
#[derive(Debug, Clone, Copy)]
struct Frame {
    /// The inner hash at this level, captured when a child level was
    /// opened. Read back when a later constrained level names this one as
    /// its source.
    frozen_hash: Option<u64>,
    /// `Some(h)` iff this level is constrained; `h` is the only hash it
    /// admits.
    admitted: Option<u64>,
    /// A constrained level has been advanced past its one bucket. Only
    /// meaningful when `admitted` is `Some`.
    exhausted: bool,
}

/// [`HashTrieIterator`] adapter enforcing column equalities on a hash
/// trie.
///
/// Constructed by `kermit::db::run_join` for each `Select_<n>_<base>`
/// predicate the rewrite introduced. At a constrained level the view is a
/// one-bucket table: `size` is 1, `open` succeeds only if the inner node
/// has the source hash, `next` exhausts it, and `lookup(h)` answers
/// `h == admitted` without moving — a probe iterator may be asked about
/// several hashes at one level, so unlike
/// [`SingletonHashTrieIter`](crate::hash::SingletonHashTrieIter) a miss
/// must not exhaust it.
#[derive(Debug)]
pub struct EqualitySelectionHashTrieIter<IT: HashTrieIterator> {
    inner: IT,
    equalities: Vec<ColumnEquality>,
    /// `source_of[j] == Some(i)` iff `σ_{col_i = col_j}` is enforced.
    source_of: Vec<Option<usize>>,
    /// One frame per open level; `frames.len()` is the current depth.
    frames: Vec<Frame>,
    /// Value-filtered copy of the inner leaf chain, `Some` iff the inner
    /// sits on an occupied leaf bucket. Owned because
    /// [`HashTrieIterator::leaf_tuples`] hands out a borrowed slice; the
    /// copy is paid only by selected atoms, once per leaf bucket visited.
    leaf: Option<Vec<Vec<usize>>>,
}

impl<IT: HashTrieIterator> EqualitySelectionHashTrieIter<IT> {
    /// Wraps `inner`, positioned at its root, with the given equalities.
    pub fn new(inner: IT, equalities: &[ColumnEquality]) -> Self {
        let width = equalities
            .iter()
            .map(|e| e.repeat + 1)
            .max()
            .unwrap_or(0);
        let mut source_of = vec![None; width];
        for e in equalities {
            debug_assert!(e.source < e.repeat, "source column must precede the repeat");
            source_of[e.repeat] = Some(e.source);
        }
        Self {
            inner,
            equalities: equalities.to_vec(),
            source_of,
            frames: Vec::new(),
            leaf: None,
        }
    }

    /// The constrained top frame, if the current level is constrained.
    fn constrained_top(&self) -> Option<(u64, bool)> {
        let top = self.frames.last()?;
        top.admitted.map(|h| (h, top.exhausted))
    }

    /// Recomputes the filtered leaf chain for the inner's current bucket.
    fn refresh_leaf(&mut self) {
        let equalities = &self.equalities;
        self.leaf = self.inner.leaf_tuples().map(|chain| {
            chain
                .iter()
                .filter(|t| equalities.iter().all(|e| t[e.source] == t[e.repeat]))
                .cloned()
                .collect()
        });
    }
}

impl<IT: HashTrieIterator> HashTrieIterator for EqualitySelectionHashTrieIter<IT> {
    fn key(&self) -> Option<u64> {
        match self.constrained_top() {
            | Some((_, true)) => None,
            | _ => self.inner.key(),
        }
    }

    fn next(&mut self) -> Option<u64> {
        if let Some(top) = self.frames.last_mut() {
            if top.admitted.is_some() {
                top.exhausted = true;
                self.leaf = None;
                return None;
            }
        }
        let hash = self.inner.next();
        self.refresh_leaf();
        hash
    }

    fn lookup(&mut self, hash: u64) -> bool {
        if let Some((admitted, exhausted)) = self.constrained_top() {
            return !exhausted && admitted == hash;
        }
        let found = self.inner.lookup(hash);
        self.refresh_leaf();
        found
    }

    fn size(&self) -> usize {
        match self.constrained_top() {
            | Some((_, exhausted)) => usize::from(!exhausted),
            | None => self.inner.size(),
        }
    }

    fn at_end(&self) -> bool {
        match self.constrained_top() {
            | Some((_, exhausted)) => exhausted,
            | None => self.inner.at_end(),
        }
    }

    fn open(&mut self) -> bool {
        if let Some(top) = self.frames.last_mut() {
            top.frozen_hash = self.inner.key();
        }
        if !self.inner.open() {
            return false;
        }
        let level = self.frames.len();
        let admitted = match self.source_of.get(level).copied().flatten() {
            | Some(source) => {
                // A missed `lookup` parks the inner past-end at the child
                // level; `up` discards that level, restoring the parent
                // position as a failed `open` must.
                match self.frames[source].frozen_hash {
                    | Some(h) if self.inner.lookup(h) => Some(h),
                    | _ => {
                        assert!(self.inner.up());
                        return false;
                    },
                }
            },
            | None => None,
        };
        self.frames.push(Frame {
            frozen_hash: None,
            admitted,
            exhausted: false,
        });
        self.refresh_leaf();
        true
    }

    fn up(&mut self) -> bool {
        if !self.inner.up() {
            return false;
        }
        self.frames.pop();
        self.leaf = None;
        true
    }

    fn leaf_tuples(&self) -> Option<&[Vec<usize>]> {
        match self.constrained_top() {
            | Some((_, true)) => None,
            | _ => self.leaf.as_deref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_ds::{HashTrie, Relation},
        kermit_iters::{HashStrategy, HashTrieIterable, SipHashStrategy},
    };

    fn eq(source: usize, repeat: usize) -> ColumnEquality {
        ColumnEquality {
            source,
            repeat,
        }
    }

    fn h(v: usize) -> u64 { SipHashStrategy::hash(v) }

    /// `r = {(1,1),(1,2),(2,3),(3,3),(4,5)}`, the PROBLEMS.md repro.
    fn diagonal_fixture() -> HashTrie {
        HashTrie::from_tuples(2.into(), vec![
            vec![1, 1],
            vec![1, 2],
            vec![2, 3],
            vec![3, 3],
            vec![4, 5],
        ])
    }

    fn view<'r, R: HashTrieIterable>(
        r: &'r R, eqs: &[ColumnEquality],
    ) -> EqualitySelectionHashTrieIter<impl HashTrieIterator + 'r> {
        EqualitySelectionHashTrieIter::new(r.hash_trie_iter(), eqs)
    }

    /// Every full-arity tuple the view admits, by walking it the way
    /// `HashTriejoin::enumerate` does: scan each level, descend, collect
    /// leaf chains.
    fn collect<IT: HashTrieIterator>(it: &mut IT, arity: usize) -> Vec<Vec<usize>> {
        fn go<IT: HashTrieIterator>(it: &mut IT, depth: usize, arity: usize, out: &mut Vec<Vec<usize>>) {
            if !it.open() {
                return;
            }
            while !it.at_end() {
                if depth + 1 == arity {
                    out.extend(it.leaf_tuples().unwrap_or(&[]).iter().cloned());
                } else {
                    go(it, depth + 1, arity, out);
                }
                if it.next().is_none() {
                    break;
                }
            }
            assert!(it.up());
        }
        let mut out = Vec::new();
        go(it, 0, arity, &mut out);
        out.sort();
        out
    }

    #[test]
    fn unconstrained_view_yields_everything() {
        let r = diagonal_fixture();
        let mut it = view(&r, &[]);
        assert_eq!(collect(&mut it, 2).len(), 5);
    }

    #[test]
    fn diagonal_view_yields_only_diagonal_tuples() {
        let r = diagonal_fixture();
        let mut it = view(&r, &[eq(0, 1)]);
        assert_eq!(collect(&mut it, 2), vec![vec![1, 1], vec![3, 3]]);
    }

    #[test]
    fn constrained_level_is_a_one_bucket_table() {
        let r = diagonal_fixture();
        let mut it = view(&r, &[eq(0, 1)]);
        assert!(it.open());
        assert!(it.lookup(h(1)));
        assert!(it.open());
        assert_eq!(it.size(), 1);
        assert_eq!(it.key(), Some(h(1)));
        assert!(!it.at_end());
        assert_eq!(it.leaf_tuples(), Some(&[vec![1, 1]][..]));
        assert_eq!(it.next(), None);
        assert!(it.at_end());
        assert_eq!(it.size(), 0);
        assert_eq!(it.key(), None);
        assert!(it.leaf_tuples().is_none());
    }

    #[test]
    fn lookup_at_constrained_level_answers_without_moving_or_exhausting() {
        let r = diagonal_fixture();
        let mut it = view(&r, &[eq(0, 1)]);
        it.open();
        it.lookup(h(3));
        assert!(it.open());
        assert!(!it.lookup(h(99)), "a foreign hash misses");
        assert!(!it.at_end(), "but does not exhaust the level");
        assert!(it.lookup(h(3)), "and the admitted hash still hits");
        assert_eq!(it.key(), Some(h(3)));
    }

    #[test]
    fn open_false_when_source_hash_absent_and_position_restored() {
        let r = diagonal_fixture();
        let mut it = view(&r, &[eq(0, 1)]);
        it.open();
        assert!(it.lookup(h(2)));
        assert!(!it.open(), "2 has children {{3}} only");
        assert_eq!(it.key(), Some(h(2)));
        assert!(!it.at_end());
        assert!(it.up());
    }

    #[test]
    fn non_adjacent_repeat() {
        let r: HashTrie = HashTrie::from_tuples(3.into(), vec![vec![1, 2, 1], vec![1, 2, 3], vec![2, 5, 2]]);
        let mut it = view(&r, &[eq(0, 2)]);
        assert_eq!(collect(&mut it, 3), vec![vec![1, 2, 1], vec![2, 5, 2]]);
    }

    #[test]
    fn interleaved_repeats() {
        let r: HashTrie = HashTrie::from_tuples(4.into(), vec![
            vec![1, 2, 1, 2],
            vec![1, 2, 1, 3],
            vec![1, 2, 2, 2],
        ]);
        let mut it = view(&r, &[eq(0, 2), eq(1, 3)]);
        assert_eq!(collect(&mut it, 4), vec![vec![1, 2, 1, 2]]);
    }

    /// A deliberately colliding strategy. Local copy of the one in
    /// `hash_triejoin.rs`'s tests: neither crate exposes a colliding
    /// strategy publicly.
    #[derive(Copy, Clone, Default, Debug)]
    struct Mod10HashStrategy;

    impl kermit_iters::LayoutOption for Mod10HashStrategy {
        const NAME: &'static str = "mod10";
    }

    impl HashStrategy for Mod10HashStrategy {
        fn hash(key: usize) -> u64 { (key % 10) as u64 }
    }

    type CollidingHashTrie = HashTrie<Mod10HashStrategy>;

    #[test]
    fn leaf_filter_rejects_hash_collision() {
        // h(11) == h(1): the constrained level admits the bucket, the
        // leaf filter must still reject (1, 11).
        let r = CollidingHashTrie::from_tuples(2.into(), vec![vec![1, 11]]);
        let mut it = view(&r, &[eq(0, 1)]);
        assert!(it.open());
        assert!(it.open(), "the colliding bucket is admitted by hash");
        assert_eq!(it.leaf_tuples(), Some(&[][..]));
        assert_eq!(collect(&mut view(&r, &[eq(0, 1)]), 2), Vec::<Vec<usize>>::new());
    }

    #[test]
    fn leaf_filter_keeps_true_diagonal_among_colliders() {
        let r = CollidingHashTrie::from_tuples(2.into(), vec![vec![1, 11], vec![1, 1], vec![1, 21]]);
        let mut it = view(&r, &[eq(0, 1)]);
        assert_eq!(collect(&mut it, 2), vec![vec![1, 1]]);
    }

    #[test]
    fn empty_relation_has_nothing_to_open() {
        let r: HashTrie = HashTrie::from_tuples(2.into(), vec![]);
        let mut it = view(&r, &[eq(0, 1)]);
        assert!(!it.open());
    }
}
