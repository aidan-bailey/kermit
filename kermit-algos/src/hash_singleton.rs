//! Singleton hash-trie iterator: a unary "relation" of one value, used by
//! the const-view rewrite (see [`crate::const_rewrite`]).
//!
//! Analogous to [`crate::singleton::SingletonTrieIter`]. Exposes the
//! [`HashTrieIterator`] interface and hashes its single value using
//! [`kermit_iters::hash_attribute`] so the join algorithm can intersect
//! against `HashTrie` data without divergence.

use kermit_iters::{hash_attribute, HashTrieIterable, HashTrieIterator, JoinIterable};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Root,
    AtValue,
    Exhausted,
}

/// A singleton "hash trie" containing one value.
///
/// Implements [`HashTrieIterator`] over a single-value relation. Used by
/// the const-view rewrite to expose a `Const_c<id> = {c<id>}` predicate as
/// if it were a real hash-trie-backed relation.
#[derive(Debug, Clone)]
pub struct SingletonHashTrieIter {
    value: usize,
    hash: u64,
    /// Cached one-tuple chain returned by [`HashTrieIterator::leaf_tuples`]
    /// once the iterator has been [`open`](HashTrieIterator::open)ed.
    /// Pre-materialized in [`new`] so the trait method can hand out a
    /// `&[Vec<usize>]` without allocating or holding interior mutability.
    chain: Vec<Vec<usize>>,
    state: State,
}

impl SingletonHashTrieIter {
    /// Construct a new singleton positioned at its root (pre-`open`).
    pub fn new(value: usize) -> Self {
        Self {
            value,
            hash: hash_attribute(0, value),
            chain: vec![vec![value]],
            state: State::Root,
        }
    }
}

impl HashTrieIterator for SingletonHashTrieIter {
    fn key(&self) -> Option<u64> {
        match self.state {
            | State::AtValue => Some(self.hash),
            | State::Root | State::Exhausted => None,
        }
    }

    fn next(&mut self) -> Option<u64> {
        if self.state == State::AtValue {
            self.state = State::Exhausted;
        }
        None
    }

    fn lookup(&mut self, hash: u64) -> bool {
        if self.state == State::AtValue && hash == self.hash {
            true
        } else if self.state == State::AtValue {
            self.state = State::Exhausted;
            false
        } else {
            false
        }
    }

    fn size(&self) -> usize {
        if self.state == State::AtValue || self.state == State::Root {
            1
        } else {
            0
        }
    }

    fn at_end(&self) -> bool { self.state == State::Exhausted }

    fn open(&mut self) -> bool {
        match self.state {
            | State::Root => {
                self.state = State::AtValue;
                true
            },
            | State::AtValue | State::Exhausted => false,
        }
    }

    fn up(&mut self) -> bool {
        match self.state {
            | State::AtValue | State::Exhausted => {
                self.state = State::Root;
                true
            },
            | State::Root => false,
        }
    }

    fn leaf_tuples(&self) -> Option<&[Vec<usize>]> {
        if self.state == State::AtValue {
            Some(self.chain.as_slice())
        } else {
            None
        }
    }
}

impl JoinIterable for SingletonHashTrieIter {}

impl HashTrieIterable for SingletonHashTrieIter {
    fn hash_trie_iter(&self) -> impl HashTrieIterator { self.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_at_root() {
        let it = SingletonHashTrieIter::new(42);
        assert_eq!(it.state, State::Root);
        assert!(it.key().is_none());
    }

    #[test]
    fn open_advances_to_value() {
        let mut it = SingletonHashTrieIter::new(42);
        assert!(it.open());
        assert_eq!(it.key(), Some(hash_attribute(0, 42)));
    }

    #[test]
    fn lookup_matching_hash_succeeds() {
        let mut it = SingletonHashTrieIter::new(42);
        it.open();
        assert!(it.lookup(hash_attribute(0, 42)));
    }

    #[test]
    fn lookup_other_hash_fails_and_exhausts() {
        let mut it = SingletonHashTrieIter::new(42);
        it.open();
        assert!(!it.lookup(hash_attribute(0, 99)));
        assert!(it.at_end());
    }

    #[test]
    fn next_after_open_exhausts() {
        let mut it = SingletonHashTrieIter::new(42);
        it.open();
        assert!(it.next().is_none());
        assert!(it.at_end());
    }

    #[test]
    fn leaf_tuples_returns_singleton_chain() {
        let mut it = SingletonHashTrieIter::new(42);
        it.open();
        let chain = it.leaf_tuples().expect("singleton's leaf chain after open");
        assert_eq!(chain, &[vec![42]]);
    }

    #[test]
    fn leaf_tuples_returns_none_before_open() {
        let it = SingletonHashTrieIter::new(42);
        assert!(it.leaf_tuples().is_none());
    }
}
