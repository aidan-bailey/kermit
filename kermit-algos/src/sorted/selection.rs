//! Equality-selection view over a sorted trie iterator, used by the
//! selection rewrite (see [`crate::selection_rewrite`]).
//!
//! Presents `σ_{col_i = col_j}(r)` as a trie of the same arity as `r`
//! without materialising anything: every level delegates to the inner
//! iterator, except that a *constrained* level `j` admits exactly one key
//! — the key this iterator itself passed at its source level `i`. Because
//! `i < j`, that key is on the path by the time level `j` is opened.
//!
//! The sorted counterpart of [`crate::hash::EqualitySelectionHashTrieIter`].

use {
    crate::selection_rewrite::ColumnEquality,
    kermit_iters::{LinearIterator, TrieIterator, TrieIteratorWrapper},
};

/// Bookkeeping for one open level of the inner iterator.
#[derive(Debug, Clone, Copy)]
struct Frame {
    /// The inner key at this level, captured when a child level was
    /// opened. Read back when a later constrained level names this one as
    /// its source.
    frozen_key: Option<usize>,
    /// `Some(k)` iff this level is constrained; `k` is the only key it
    /// admits.
    admitted: Option<usize>,
    /// A constrained level has been advanced past its one key. Only
    /// meaningful when `admitted` is `Some`.
    exhausted: bool,
}

/// [`TrieIterator`] adapter enforcing column equalities on a trie.
///
/// Constructed by `kermit::db::run_join` for each `Select_<n>_<base>`
/// predicate the rewrite introduced. At a constrained level the view is a
/// one-key sibling list `{k}`: `open` succeeds only if the inner trie has a
/// child `k` there, `next` exhausts it, and `seek(t)` stays put for
/// `t <= k` and exhausts otherwise — exactly the contract of
/// [`crate::sorted::SingletonTrieIter`], but nested inside a real trie.
#[derive(Debug)]
pub struct EqualitySelectionTrieIter<IT: TrieIterator> {
    inner: IT,
    /// `source_of[j] == Some(i)` iff `σ_{col_i = col_j}` is enforced.
    source_of: Vec<Option<usize>>,
    /// One frame per open level; `frames.len()` is the current depth.
    frames: Vec<Frame>,
}

impl<IT: TrieIterator> EqualitySelectionTrieIter<IT> {
    /// Wraps `inner`, positioned at its root, with the given equalities.
    pub fn new(inner: IT, equalities: &[ColumnEquality]) -> Self {
        let width = equalities.iter().map(|e| e.repeat + 1).max().unwrap_or(0);
        let mut source_of = vec![None; width];
        for e in equalities {
            debug_assert!(e.source < e.repeat, "source column must precede the repeat");
            source_of[e.repeat] = Some(e.source);
        }
        Self {
            inner,
            source_of,
            frames: Vec::new(),
        }
    }

    /// One past the widest constrained column. A prefix that dies at a
    /// constrained level is always shorter than this, so tuple collectors
    /// use it as the minimum length of a complete path (see `into_iter`).
    pub fn constrained_width(&self) -> usize { self.source_of.len() }

    /// The constrained top frame, if the current level is constrained.
    fn constrained_top(&self) -> Option<(usize, bool)> {
        let top = self.frames.last()?;
        top.admitted.map(|k| (k, top.exhausted))
    }

    /// Descends the inner iterator into level `j` and positions it on the
    /// admitted key `k`, or restores the parent position and reports a
    /// miss. A trie iterator's `seek` may only move forward, so a first
    /// child above `k` is a miss without seeking.
    fn open_constrained(&mut self, k: usize) -> bool {
        let hit = match self.inner.key() {
            | Some(first) if first == k => true,
            | Some(first) if first > k => false,
            | _ => self.inner.seek(k) && self.inner.key() == Some(k),
        };
        if !hit {
            assert!(
                self.inner.up(),
                "an iterator that just descended must be able to move back up"
            );
        }
        hit
    }
}

impl<IT: TrieIterator> LinearIterator for EqualitySelectionTrieIter<IT> {
    fn key(&self) -> Option<usize> {
        match self.constrained_top() {
            | Some((_, true)) => None,
            | _ => self.inner.key(),
        }
    }

    fn next(&mut self) -> Option<usize> {
        if let Some(top) = self.frames.last_mut() {
            if top.admitted.is_some() {
                top.exhausted = true;
                return None;
            }
        }
        self.inner.next()
    }

    fn seek(&mut self, seek_key: usize) -> bool {
        if let Some(top) = self.frames.last_mut() {
            if let Some(k) = top.admitted {
                if top.exhausted {
                    return false;
                }
                if seek_key > k {
                    top.exhausted = true;
                    return false;
                }
                return true;
            }
        }
        self.inner.seek(seek_key)
    }

    fn at_end(&self) -> bool {
        match self.constrained_top() {
            | Some((_, exhausted)) => exhausted,
            | None => self.inner.at_end(),
        }
    }
}

impl<IT: TrieIterator> TrieIterator for EqualitySelectionTrieIter<IT> {
    fn open(&mut self) -> bool {
        if let Some(top) = self.frames.last_mut() {
            // An exhausted constrained level still has the inner parked on
            // its one key, so freezing here matches the trait's "proceed as
            // if on the last sibling" rule for `open` after `at_end`.
            top.frozen_key = self.inner.key();
        }
        if !self.inner.open() {
            return false;
        }
        let level = self.frames.len();
        let admitted = match self.source_of.get(level).copied().flatten() {
            | Some(source) => {
                let Some(k) = self.frames[source].frozen_key else {
                    // The source level was never positioned; nothing can
                    // match. Restore the parent position like any miss.
                    assert!(self.inner.up());
                    return false;
                };
                if !self.open_constrained(k) {
                    return false;
                }
                Some(k)
            },
            | None => None,
        };
        self.frames.push(Frame {
            frozen_key: None,
            admitted,
            exhausted: false,
        });
        true
    }

    fn up(&mut self) -> bool {
        if !self.inner.up() {
            return false;
        }
        self.frames.pop();
        true
    }
}

impl<IT: TrieIterator> IntoIterator for EqualitySelectionTrieIter<IT> {
    type IntoIter = TrieIteratorWrapper<Self>;
    type Item = Vec<usize>;

    /// A prefix that dies at a constrained level `j` is a path of length
    /// `j`, which the wrapper would otherwise emit as if it were a leaf.
    /// Every such path is shorter than the widest constrained column, so
    /// skipping tuples below that width drops exactly the dead prefixes.
    fn into_iter(self) -> Self::IntoIter {
        let width = self.constrained_width();
        TrieIteratorWrapper::with_arity(self, width)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_ds::{Relation, TreeTrie},
        kermit_iters::TrieIterable,
    };

    fn eq(source: usize, repeat: usize) -> ColumnEquality {
        ColumnEquality {
            source,
            repeat,
        }
    }

    fn trie(arity: usize, tuples: Vec<Vec<usize>>) -> TreeTrie {
        TreeTrie::from_tuples(arity.into(), tuples)
    }

    /// `r = {(1,1),(1,2),(2,3),(3,3),(4,5)}`, the PROBLEMS.md repro.
    fn diagonal_fixture() -> TreeTrie {
        trie(2, vec![
            vec![1, 1],
            vec![1, 2],
            vec![2, 3],
            vec![3, 3],
            vec![4, 5],
        ])
    }

    fn view<'r>(
        r: &'r TreeTrie, eqs: &[ColumnEquality],
    ) -> EqualitySelectionTrieIter<impl TrieIterator + 'r> {
        EqualitySelectionTrieIter::new(r.trie_iter(), eqs)
    }

    #[test]
    fn unconstrained_levels_delegate() {
        let r = diagonal_fixture();
        let mut it = view(&r, &[]);
        assert!(it.open());
        assert_eq!(it.key(), Some(1));
        assert!(it.open());
        assert_eq!(it.key(), Some(1));
        assert_eq!(it.next(), Some(2));
        assert!(it.up());
        assert!(it.seek(3));
        assert_eq!(it.key(), Some(3));
    }

    #[test]
    fn diagonal_walk_admits_only_the_source_key() {
        let r = diagonal_fixture();
        let mut it = view(&r, &[eq(0, 1)]);
        assert!(it.open());
        assert_eq!(it.key(), Some(1));
        // (1, 1) exists.
        assert!(it.open());
        assert_eq!(it.key(), Some(1));
        assert!(!it.at_end());
        assert_eq!(it.next(), None);
        assert!(it.at_end());
        assert_eq!(it.key(), None);
        assert!(it.up());
        // 2 has children {3} only: a miss that leaves the parent in place.
        assert_eq!(it.next(), Some(2));
        assert!(!it.open());
        assert_eq!(it.key(), Some(2));
        // (3, 3) exists.
        assert_eq!(it.next(), Some(3));
        assert!(it.open());
        assert_eq!(it.key(), Some(3));
        assert!(it.up());
        // 4 has children {5} only.
        assert_eq!(it.next(), Some(4));
        assert!(!it.open());
        assert_eq!(it.key(), Some(4));
        assert_eq!(it.next(), None);
        assert!(it.at_end());
    }

    #[test]
    fn seek_at_constrained_level() {
        let r = diagonal_fixture();
        let mut it = view(&r, &[eq(0, 1)]);
        it.open();
        it.seek(3);
        assert!(it.open());
        assert!(it.seek(2), "seek below the admitted key stays");
        assert!(it.seek(3), "seek at the admitted key stays");
        assert_eq!(it.key(), Some(3));
        assert!(!it.seek(4), "seek above the admitted key exhausts");
        assert!(it.at_end());
        assert!(!it.seek(3), "an exhausted level stays exhausted");
    }

    #[test]
    fn admitted_key_below_first_child_is_a_miss_not_a_panic() {
        // TreeTrieIter::seek panics on a backward seek; the adapter must
        // recognise 5 < 7 as a miss without seeking.
        let r = trie(2, vec![vec![5, 7]]);
        let mut it = view(&r, &[eq(0, 1)]);
        assert!(it.open());
        assert!(!it.open());
        assert_eq!(it.key(), Some(5));
    }

    #[test]
    fn admitted_key_beyond_last_child_is_a_miss() {
        let r = trie(2, vec![vec![5, 3]]);
        let mut it = view(&r, &[eq(0, 1)]);
        assert!(it.open());
        assert!(!it.open());
        assert_eq!(it.key(), Some(5));
        assert!(!it.at_end());
    }

    #[test]
    fn open_after_exhausted_constrained_level_descends_from_the_admitted_key() {
        // Three columns, constraint on the middle one: after exhausting
        // level 1 the inner is still parked on k, so `open` proceeds as if
        // on the last sibling (the trait's rule).
        let r = trie(3, vec![vec![1, 1, 9], vec![1, 2, 8]]);
        let mut it = view(&r, &[eq(0, 1)]);
        it.open();
        assert!(it.open());
        assert_eq!(it.next(), None);
        assert!(it.at_end());
        assert!(it.open());
        assert_eq!(it.key(), Some(9));
    }

    #[test]
    fn three_way_repeat() {
        let r = trie(3, vec![vec![1, 1, 1], vec![1, 1, 2], vec![2, 2, 3]]);
        let tuples: Vec<Vec<usize>> = view(&r, &[eq(0, 1), eq(0, 2)]).into_iter().collect();
        assert_eq!(tuples, vec![vec![1, 1, 1]]);
    }

    #[test]
    fn non_adjacent_repeat() {
        let r = trie(3, vec![vec![1, 2, 1], vec![1, 2, 3], vec![2, 5, 2]]);
        let tuples: Vec<Vec<usize>> = view(&r, &[eq(0, 2)]).into_iter().collect();
        assert_eq!(tuples, vec![vec![1, 2, 1], vec![2, 5, 2]]);
    }

    #[test]
    fn interleaved_repeats() {
        let r = trie(4, vec![vec![1, 2, 1, 2], vec![1, 2, 1, 3], vec![
            1, 2, 2, 2,
        ]]);
        let tuples: Vec<Vec<usize>> = view(&r, &[eq(0, 2), eq(1, 3)]).into_iter().collect();
        assert_eq!(tuples, vec![vec![1, 2, 1, 2]]);
    }

    #[test]
    fn wrapper_yields_only_diagonal_tuples() {
        let r = diagonal_fixture();
        let tuples: Vec<Vec<usize>> = view(&r, &[eq(0, 1)]).into_iter().collect();
        assert_eq!(tuples, vec![vec![1, 1], vec![3, 3]]);
    }

    #[test]
    fn empty_relation_has_nothing_to_open() {
        let r = trie(2, vec![]);
        let mut it = view(&r, &[eq(0, 1)]);
        assert!(!it.open());
    }
}
