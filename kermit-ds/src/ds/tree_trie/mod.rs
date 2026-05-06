//! This module provides a [trie](https://en.wikipedia.org/wiki/Trie)-based implementation of a relation.

mod implementation;
mod tree_trie_iter;

#[cfg(test)]
mod tests;

pub use implementation::TreeTrie;
