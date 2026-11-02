//! Leapfrog join: the k-way intersection of sorted linear iterators, after
//! Algorithms 1–4 of [Veldhuizen 2014](https://arxiv.org/abs/1210.0481).

use kermit_iters::LinearIterator;

/// The leapfrog join of `k` sorted [`LinearIterator`]s: the keys all `k`
/// hold, in ascending order.
///
/// # The ring
///
/// The iterators form a ring that `p` walks cyclically. As in the paper, the
/// ring stays sorted by the iterators' current keys read from `p`: the
/// iterator at `p` holds the least key and the one before it the greatest.
/// That is why one comparison, least against greatest, proves every
/// iterator is at the same key. The paper sorts its iterator array itself;
/// here the iterators stay where they were given and `ring` holds their
/// indices in ring order, so the paper's `Iter[p]` is
/// [`iter_at(p)`](Self::iter_at).
///
/// Call [`leapfrog_init`](Self::leapfrog_init) before any other operation,
/// and only while `k >= 1`.
pub(crate) struct LeapfrogJoinIter<IT>
where
    IT: LinearIterator,
{
    /// The iterators being intersected, in the order they were given.
    pub(crate) iterators: Vec<IT>,
    /// Indices into `iterators`, in ring order (see [the ring](Self#the-ring)).
    ring: Vec<usize>,
    /// The paper's ring position `p`: an index into `ring`.
    p: usize,
}

impl<IT> LeapfrogJoinIter<IT>
where
    IT: LinearIterator,
{
    /// Creates a new leapfrog join over the given sorted iterators.
    pub(crate) fn new(iterators: Vec<IT>) -> Self {
        LeapfrogJoinIter {
            ring: (0..iterators.len()).collect(),
            iterators,
            p: 0,
        }
    }

    /// Refills a drained join with a new set of sorted iterators, leaving it
    /// as [`new`](Self::new) would but in the existing buffers: once
    /// `iterators` and `ring` have grown to the widest set they will hold, a
    /// refill allocates nothing. The triejoin refills on every depth change
    /// rather than building a new join (issue #83).
    ///
    /// The caller must already have taken every previous iterator out of
    /// `iterators`.
    pub(crate) fn refill(&mut self, iterators: impl IntoIterator<Item = IT>) {
        debug_assert!(
            self.iterators.is_empty(),
            "refill needs the previous iterators drained first"
        );
        self.iterators.extend(iterators);
        self.ring.clear();
        self.ring.extend(0..self.iterators.len());
        self.p = 0;
    }

    /// Returns the number of iterators being joined.
    pub(crate) fn k(&self) -> usize { self.iterators.len() }

    /// The iterator at ring position `i`: the paper's `Iter[i]`.
    fn iter_at(&self, i: usize) -> &IT { &self.iterators[self.ring[i]] }

    /// The iterator at ring position `i`, mutably.
    fn iter_at_mut(&mut self, i: usize) -> &mut IT { &mut self.iterators[self.ring[i]] }

    /// Returns the current common key, or `None` once the join is exhausted
    /// (the iterator at `p` is then the one that ran off its end).
    pub(crate) fn key(&self) -> Option<usize> { self.iter_at(self.p).key() }

    /// Returns `true` once any iterator has run off its end: no further
    /// common key exists. The paper keeps this as a flag; it is derived here.
    pub(crate) fn at_end(&self) -> bool { self.iterators.iter().any(LinearIterator::at_end) }

    /// Algorithm 1, `leapfrog-init`: sorts the ring by current key and finds
    /// the first common key. Returns whether one exists.
    pub(crate) fn leapfrog_init(&mut self) -> bool {
        for iter in &mut self.iterators {
            // The paper's iterators are always positioned on a key, as a trie
            // iterator is after `open`. A fresh `LinearIterator` sits before
            // its first key instead, so step it there first.
            if iter.key().is_none() {
                iter.next();
            }
            if iter.at_end() {
                return false;
            }
        }
        self.ring.sort_unstable_by_key(|&i| self.iterators[i].key());
        self.p = 0;
        self.leapfrog_search()
    }

    /// Algorithm 2, `leapfrog-search`: from a sorted ring, seeks the
    /// iterator with the least key to the greatest key until all agree.
    /// Returns whether they do; `false` means one ran off its end.
    ///
    /// Each pass that does not return moves `iter_at(p)` strictly forward,
    /// since its least key differs from the greatest, and makes it the new
    /// greatest. So the ring stays sorted from the next `p`, and the loop
    /// ends within as many seeks as the iterators have keys.
    fn leapfrog_search(&mut self) -> bool {
        let k = self.k();
        // No iterator is at its end on entry, so every key is `Some`.
        let mut max_key = self.iter_at((self.p + k - 1) % k).key().unwrap();
        loop {
            let least_key = self.iter_at(self.p).key().unwrap();
            if least_key == max_key {
                return true; // every iterator is at the same key
            }
            let iter = self.iter_at_mut(self.p);
            iter.seek(max_key);
            if iter.at_end() {
                return false;
            }
            max_key = iter.key().unwrap();
            self.p = (self.p + 1) % k;
        }
    }

    /// Algorithm 3, `leapfrog-next`: moves to the next common key and
    /// returns it, or `None` if there is none.
    pub(crate) fn leapfrog_next(&mut self) -> Option<usize> {
        let iter = self.iter_at_mut(self.p);
        iter.next();
        if iter.at_end() {
            return None;
        }
        self.p = (self.p + 1) % self.k();
        if self.leapfrog_search() {
            self.key()
        } else {
            None
        }
    }

    /// Algorithm 4, `leapfrog-seek`: moves to the least common key
    /// `≥ seek_key`. Returns whether one exists.
    pub(crate) fn leapfrog_seek(&mut self, seek_key: usize) -> bool {
        let iter = self.iter_at_mut(self.p);
        iter.seek(seek_key);
        if iter.at_end() {
            return false;
        }
        self.p = (self.p + 1) % self.k();
        self.leapfrog_search()
    }
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_iters::LinearIterable};

    #[test]
    fn test_leapfrog_join_iter() {
        let v1: Vec<usize> = vec![1, 2, 3];
        let v2: Vec<usize> = vec![2, 3, 4];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);

        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(2));
        assert_eq!(join_iter.leapfrog_next(), Some(3));
        assert_eq!(join_iter.leapfrog_next(), None);
    }

    #[test]
    fn test_leapfrog_join_iter_empty() {
        let v1: Vec<usize> = vec![];
        let v2: Vec<usize> = vec![];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);

        assert!(!join_iter.leapfrog_init());
    }

    #[test]
    fn test_leapfrog_join_iter_no_common_elements() {
        let v1: Vec<usize> = vec![1, 3, 5];
        let v2: Vec<usize> = vec![2, 4, 6];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);

        assert!(!join_iter.leapfrog_init());
    }

    #[test]
    fn test_leapfrog_join_iter_single_iterator() {
        let v1: Vec<usize> = vec![1, 2, 3];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter()]);

        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(1));
    }

    #[test]
    fn test_leapfrog_join_iter_multiple_vectors_with_common_elements() {
        let v1: Vec<usize> = vec![1, 2, 3, 5];
        let v2: Vec<usize> = vec![2, 4, 5, 6];
        let v3: Vec<usize> = vec![2, 5, 7];

        let mut join_iter =
            LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter(), v3.linear_iter()]);

        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(2));
        assert_eq!(join_iter.leapfrog_next(), Some(5));
        assert_eq!(join_iter.leapfrog_next(), None);
    }

    #[test]
    fn test_leapfrog_join_iter_no_common_elements_multiple_vectors() {
        let v1: Vec<usize> = vec![1, 3, 5];
        let v2: Vec<usize> = vec![2, 4, 6];
        let v3: Vec<usize> = vec![7, 8, 9];

        let mut join_iter =
            LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter(), v3.linear_iter()]);

        assert!(!join_iter.leapfrog_init());
    }

    #[test]
    fn test_leapfrog_join_iter_empty_and_non_empty() {
        let v1: Vec<usize> = vec![];
        let v2: Vec<usize> = vec![1, 2, 3];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);

        assert!(!join_iter.leapfrog_init());
    }

    #[test]
    fn test_leapfrog_join_iter_multiple_empty_vectors() {
        let v1: Vec<usize> = vec![];
        let v2: Vec<usize> = vec![];
        let v3: Vec<usize> = vec![];

        let mut join_iter =
            LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter(), v3.linear_iter()]);

        assert!(!join_iter.leapfrog_init());
    }

    #[test]
    fn test_leapfrog_join_iter_single_common_element() {
        let v1: Vec<usize> = vec![1, 2, 3];
        let v2: Vec<usize> = vec![2];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);

        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(2));
    }

    #[test]
    fn test_leapfrog_join_iter_multiple_vectors_with_duplicates() {
        let v1: Vec<usize> = vec![1, 2, 3];
        let v2: Vec<usize> = vec![2, 4];
        let v3: Vec<usize> = vec![2, 5, 7];

        let mut join_iter =
            LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter(), v3.linear_iter()]);

        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(2));
    }

    #[test]
    fn test_leapfrog_join_iter_large_vectors() {
        let v1: Vec<usize> = (1..1000).map(|x| x as usize).collect();
        let v2: Vec<usize> = (500..1500).map(|x| x as usize).collect();

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);

        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(500));
        assert_eq!(join_iter.leapfrog_next(), Some(501));
    }

    /// The triejoin steps a refilled join without calling `leapfrog_init`
    /// after `triejoin_up`, so `refill` must reset the ring position as well
    /// as rebuild the ring for the new iterator count.
    #[test]
    fn test_leapfrog_join_iter_refill_behaves_like_new() {
        let v1: Vec<usize> = vec![1, 2, 3, 5];
        let v2: Vec<usize> = vec![2, 4, 5, 6];
        let v3: Vec<usize> = vec![2, 7];

        let mut join_iter = LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter()]);
        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(2));
        assert_ne!(join_iter.p, 0, "the used join should sit mid-ring");
        join_iter.iterators.clear();

        join_iter.refill([v1.linear_iter(), v2.linear_iter(), v3.linear_iter()]);
        assert_eq!(join_iter.ring, vec![0, 1, 2]);
        assert_eq!(join_iter.p, 0);
        // A ring left over two iterators would also report the 5 that only
        // `v1` and `v2` share.
        assert!(join_iter.leapfrog_init());
        assert_eq!(join_iter.key(), Some(2));
        assert_eq!(join_iter.leapfrog_next(), None);
    }

    #[test]
    fn test_leapfrog_join_iter_multiple_vectors_with_one_empty() {
        let v1: Vec<usize> = vec![1, 2, 3];
        let v2: Vec<usize> = vec![];
        let v3: Vec<usize> = vec![2, 4, 6];

        let mut join_iter =
            LeapfrogJoinIter::new(vec![v1.linear_iter(), v2.linear_iter(), v3.linear_iter()]);

        assert!(!join_iter.leapfrog_init());
    }
}
