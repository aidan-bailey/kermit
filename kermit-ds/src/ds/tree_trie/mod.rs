//! This module provides a [trie](https://en.wikipedia.org/wiki/Trie)-based implementation of a relation.

mod build_mode;
mod implementation;
mod tree_trie_iter;

#[cfg(test)]
mod tests;

#[cfg(feature = "test-hooks")]
pub(crate) use implementation::take_parallel_builds;
pub use {
    build_mode::{ParseTreeTrieBuildModeError, TreeTrieBuildMode},
    implementation::TreeTrie,
};
