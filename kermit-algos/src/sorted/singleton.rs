//! Singleton unary trie iterator representing a nonmaterialized view
//! of `Const_a = {a}`, used by the Const-view rewrite (see
//! [`crate::const_rewrite`]).

use kermit_iters::{JoinIterable, LinearIterator, TrieIterable, TrieIterator, TrieIteratorWrapper};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Root,
    AtValue,
    Exhausted,
}

/// Unary trie iterator holding a single `usize` value.
///
/// Equivalent to the paper's "nonmaterialized view" of `Const_a = {a}`
/// (Veldhuizen 2014 §3.4 point 4). Exposes exactly one tuple `[value]`
/// through the [`TrieIterator`] / [`LinearIterator`] interface.
#[derive(Debug, Clone)]
pub struct SingletonTrieIter {
    value: usize,
    state: State,
}

impl SingletonTrieIter {
    /// Constructs a new singleton positioned at its root (pre-`open`).
    pub fn new(value: usize) -> Self {
        Self {
            value,
            state: State::Root,
        }
    }
}

impl LinearIterator for SingletonTrieIter {
    fn key(&self) -> Option<usize> {
        match self.state {
            | State::AtValue => Some(self.value),
            | State::Root | State::Exhausted => None,
        }
    }

    fn next(&mut self) -> Option<usize> {
        if self.state == State::AtValue {
            self.state = State::Exhausted;
        }
        None
    }

    fn seek(&mut self, seek_key: usize) -> bool {
        if self.state != State::AtValue {
            return false;
        }
        if seek_key > self.value {
            self.state = State::Exhausted;
            false
        } else {
            true
        }
    }

    fn at_end(&self) -> bool { self.state == State::Exhausted }
}

impl TrieIterator for SingletonTrieIter {
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
}

impl IntoIterator for SingletonTrieIter {
    type IntoIter = TrieIteratorWrapper<Self>;
    type Item = Vec<usize>;

    fn into_iter(self) -> Self::IntoIter { TrieIteratorWrapper::new(self) }
}

impl JoinIterable for SingletonTrieIter {}

impl TrieIterable for SingletonTrieIter {
    fn trie_iter(&self) -> impl TrieIterator + IntoIterator<Item = Vec<usize>> { self.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_at_root_not_at_end() {
        let it = SingletonTrieIter::new(42);
        assert_eq!(it.state, State::Root);
        assert!(!it.at_end());
        assert_eq!(it.key(), None);
    }

    #[test]
    fn open_descends_to_value() {
        let mut it = SingletonTrieIter::new(42);
        assert!(it.open());
        assert_eq!(it.state, State::AtValue);
        assert_eq!(it.key(), Some(42));
        assert!(!it.at_end());
    }

    #[test]
    fn next_exhausts_the_level() {
        let mut it = SingletonTrieIter::new(42);
        it.open();
        assert_eq!(it.next(), None);
        assert_eq!(it.state, State::Exhausted);
        assert!(it.at_end());
        assert_eq!(it.key(), None);
    }

    #[test]
    fn seek_below_value_stays_at_value() {
        let mut it = SingletonTrieIter::new(42);
        it.open();
        assert!(it.seek(10));
        assert_eq!(it.key(), Some(42));
    }

    #[test]
    fn seek_at_value_stays_at_value() {
        let mut it = SingletonTrieIter::new(42);
        it.open();
        assert!(it.seek(42));
        assert_eq!(it.key(), Some(42));
    }

    #[test]
    fn seek_above_value_exhausts() {
        let mut it = SingletonTrieIter::new(42);
        it.open();
        assert!(!it.seek(43));
        assert!(it.at_end());
    }

    #[test]
    fn up_from_value_returns_to_root() {
        let mut it = SingletonTrieIter::new(42);
        it.open();
        assert!(it.up());
        assert_eq!(it.state, State::Root);
    }

    #[test]
    fn open_then_up_then_reopen_is_idempotent() {
        let mut it = SingletonTrieIter::new(42);
        assert!(it.open());
        assert_eq!(it.key(), Some(42));
        assert!(it.up());
        assert!(it.open());
        assert_eq!(it.key(), Some(42));
    }

    #[test]
    fn wrapper_yields_single_tuple() {
        let tuples: Vec<Vec<usize>> = SingletonTrieIter::new(7).into_iter().collect();
        assert_eq!(tuples, vec![vec![7]]);
    }
}
