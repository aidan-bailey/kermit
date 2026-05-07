//! This modules defines traits, structs and iters used to compute a Leapfrog
//! Join.

use kermit_iters::LinearIterator;

/// The `LeapfrogJoinIterator` trait defines the interface for a leapfrog join
/// iterator.
pub trait LeapfrogJoinIterator {
    /// Returns a reference to the key at the iterator's current position,
    /// otherwise `None` if `leapfrog_init` has not yet been called, or the
    /// iterator is positioned at the end.
    fn key(&self) -> Option<usize>;

    /// Initialises the iterator and finds the first common key.
    ///
    /// Returns `true` if a common key is found, otherwise `false`.
    fn leapfrog_init(&mut self) -> bool;

    /// Searches for a common key in the iterators.
    ///
    /// Returns `true` if a common key is found, otherwise `false`.
    fn leapfrog_search(&mut self) -> bool;

    /// Seeks the iterator to the least upper bound for the `seek_key`,
    /// i.e., the smallest key ≥ `seek_key`.
    ///
    /// Returns `true` if the key exists, otherwise `false`.
    fn leapfrog_seek(&mut self, seek_key: usize) -> bool;

    /// Moves the iterator to the next common key and returns a reference
    /// to it, or `None` if there are no more common keys.
    fn leapfrog_next(&mut self) -> Option<usize>;

    /// Returns `true` if the iterator is positioned at the end.
    fn at_end(&self) -> bool;
}

/// Concrete implementation of the leapfrog join over sorted linear iterators.
///
/// Coordinates `k` sorted iterators to find their common keys. The iterators
/// are arranged in a logical ring whose order is determined by their current
/// keys: `sorted_iter_perm` is a permutation of `0..k` sorted by each
/// iterator's initial key (computed in
/// [`leapfrog_init`](LeapfrogJoinIterator::leapfrog_init)), and `p` walks
/// this permutation cyclically.
pub struct LeapfrogJoinIter<IT>
where
    IT: LinearIterator,
{
    /// The underlying sorted iterators being joined.
    pub(crate) iterators: Vec<IT>,
    /// Permutation of `0..k` ordering the iterators by their initial key.
    /// The leapfrog walks this permutation cyclically (the "ring") rather
    /// than `iterators` directly.
    pub sorted_iter_perm: Vec<usize>,
    /// Index into `sorted_iter_perm` for the current iterator in the ring.
    p: usize,
}

impl<IT> LeapfrogJoinIter<IT>
where
    IT: LinearIterator,
{
    /// Creates a new leapfrog join over the given sorted iterators.
    pub fn new(iterators: Vec<IT>) -> Self {
        LeapfrogJoinIter {
            sorted_iter_perm: (0..iterators.len()).collect(),
            iterators,
            p: 0,
        }
    }

    /// Returns the number of iterators being joined.
    pub fn k(&self) -> usize { self.iterators.len() }

    /// Resolves a ring position `i` (an index into `sorted_iter_perm`) to the
    /// corresponding underlying iterator. Centralised so the cyclic-walk call
    /// sites read as `self.mut_iter(self.p)` rather than the double
    /// indirection `&mut self.iterators[self.sorted_iter_perm[self.p]]`.
    fn mut_iter(&mut self, i: usize) -> &mut IT { &mut self.iterators[self.sorted_iter_perm[i]] }
}

impl<IT> LeapfrogJoinIterator for LeapfrogJoinIter<IT>
where
    IT: LinearIterator,
{
    fn key(&self) -> Option<usize> { self.iterators[self.p].key() }

    fn leapfrog_init(&mut self) -> bool {
        for iter in &mut self.iterators {
            if iter.key().is_none() {
                iter.next(); // Move to the first key if not already set.
            }
            if iter.at_end() {
                return false; // If iterator is at the end, no common key can be
                              // found.
            }
        }

        self.sorted_iter_perm.sort_unstable_by(|a, b| {
            self.iterators[*a]
                .key()
                .unwrap()
                .cmp(&self.iterators[*b].key().unwrap())
        });

        self.p = 0;
        self.leapfrog_search()
    }

    fn leapfrog_search(&mut self) -> bool {
        // Loop invariant: `target_key` is the largest key any iterator
        // currently holds. The loop terminates because each `seek` either
        // advances the current iterator's key strictly past `target_key`
        // (raising the bar for the next ring step) or finds equality (a
        // common key, returned immediately) or runs off the end (no common
        // key exists). Because keys are monotone non-decreasing, the maximum
        // can only rise finitely many times before some iterator hits its
        // end.
        //
        // The predecessor in the ring (wraps to k-1 when p == 0). Its key is
        // the largest one any iterator currently holds.
        let prev_idx = if self.p == 0 {
            self.k() - 1
        } else {
            self.p - 1
        };
        let mut target_key = self.mut_iter(prev_idx).key().unwrap();
        loop {
            let current_key = self.mut_iter(self.p).key().unwrap();
            if current_key == target_key {
                return true;
            }
            self.mut_iter(self.p).seek(target_key);
            if self.mut_iter(self.p).at_end() {
                return false;
            }
            // Advancing the current iterator may have raised its key past the
            // old target; that key becomes the new target for the next ring
            // step.
            target_key = self.mut_iter(self.p).key().unwrap();
            self.p = (self.p + 1) % self.k();
        }
    }

    fn leapfrog_next(&mut self) -> Option<usize> {
        self.mut_iter(self.p).next();
        if self.mut_iter(self.p).at_end() {
            None
        } else {
            self.p = (self.p + 1) % self.k();
            self.leapfrog_search();
            self.key()
        }
    }

    fn at_end(&self) -> bool {
        for iter in &self.iterators {
            if iter.at_end() {
                return true;
            }
        }
        false
    }

    fn leapfrog_seek(&mut self, seek_key: usize) -> bool {
        self.mut_iter(self.p).seek(seek_key);
        if self.mut_iter(self.p).at_end() {
            false
        } else {
            self.p = (self.p + 1) % self.k();
            self.leapfrog_search()
        }
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
