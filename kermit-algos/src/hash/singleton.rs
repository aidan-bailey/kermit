//! Singleton hash-trie iterator: a unary "relation" of one value, used by
//! the const-view rewrite (see [`crate::const_rewrite`]).
//!
//! Analogous to [`crate::sorted::SingletonTrieIter`]. Exposes the
//! [`HashTrieIterator`] interface around a precomputed `u64` hash of its
//! single value so the join algorithm can intersect against `HashTrie`
//! data without divergence. The caller (`kermit::db::hash_join`) is
//! responsible for computing the hash via the chosen
//! [`kermit_iters::HashStrategy`] before constructing the singleton; this
//! file does not depend on any strategy.

use kermit_iters::{HashTrieIterable, HashTrieIterator, JoinIterable};

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
    // Kept as a sentinel of the constructed value; the join algorithm and
    // `leaf_tuples` consume the cached `chain` instead. Phase 7 may grow
    // direct uses (e.g., diagnostics) — allow until then.
    #[allow(dead_code)]
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
    ///
    /// `hash` is the precomputed [`u64`] hash of `value` under whichever
    /// [`kermit_iters::HashStrategy`] the caller uses for the matching
    /// `HashTrie<H>`. Keeping the strategy out of this struct lets the
    /// singleton remain `Sized` without a phantom parameter and lets a
    /// generic caller (e.g. `hash_join<R, H>`) drive both the trie and
    /// the singleton with the same hash function.
    pub fn new(value: usize, hash: u64) -> Self {
        Self {
            value,
            hash,
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
    use {
        super::*,
        kermit_iters::{HashStrategy, SipHashStrategy},
    };

    /// Pick a concrete strategy for the tests below. The struct itself is
    /// strategy-agnostic — what matters is that we use *the same* hash for
    /// both construction and lookup, mirroring how `hash_join<R, H>` wires
    /// the singleton against its matching `HashTrie<H>`.
    fn h(value: usize) -> u64 { SipHashStrategy::hash(value) }

    #[test]
    fn new_starts_at_root() {
        let it = SingletonHashTrieIter::new(42, h(42));
        assert_eq!(it.state, State::Root);
        assert!(it.key().is_none());
    }

    #[test]
    fn open_advances_to_value() {
        let mut it = SingletonHashTrieIter::new(42, h(42));
        assert!(it.open());
        assert_eq!(it.key(), Some(h(42)));
    }

    #[test]
    fn lookup_matching_hash_succeeds() {
        let mut it = SingletonHashTrieIter::new(42, h(42));
        it.open();
        assert!(it.lookup(h(42)));
    }

    #[test]
    fn lookup_other_hash_fails_and_exhausts() {
        let mut it = SingletonHashTrieIter::new(42, h(42));
        it.open();
        assert!(!it.lookup(h(99)));
        assert!(it.at_end());
    }

    #[test]
    fn next_after_open_exhausts() {
        let mut it = SingletonHashTrieIter::new(42, h(42));
        it.open();
        assert!(it.next().is_none());
        assert!(it.at_end());
    }

    #[test]
    fn leaf_tuples_returns_singleton_chain() {
        let mut it = SingletonHashTrieIter::new(42, h(42));
        it.open();
        let chain = it.leaf_tuples().expect("singleton's leaf chain after open");
        assert_eq!(chain, &[vec![42]]);
    }

    #[test]
    fn leaf_tuples_returns_none_before_open() {
        let it = SingletonHashTrieIter::new(42, h(42));
        assert!(it.leaf_tuples().is_none());
    }
}
