//! Column-oriented (flattened) trie implementation.

mod build_mode;
mod column_trie_iter;
mod implementation;

pub use {build_mode::ColumnTrieBuildMode, implementation::ColumnTrie};
