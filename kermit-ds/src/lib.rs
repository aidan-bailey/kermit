//! Data structures for Kermit's relational algebra engine.
//!
//! Provides two trie-based relation implementations:
//!
//! - [`TreeTrie`]: A pointer-based trie where each node owns its children.
//!   Simple and cache-friendly for small relations.
//! - [`ColumnTrie`]: A column-oriented (flattened) trie that stores each level
//!   in parallel `data`/`interval` arrays. More compact for large relations.
//!
//! Both implement the [`Relation`] and
//! [`TrieIterable`](kermit_iters::TrieIterable) traits, making them
//! interchangeable in join algorithms.
#![deny(missing_docs)]

mod ds;
mod heap_size;
mod relation;
mod shared;

// Re-export IndexStructure for external crates (CLI) to reference directly
pub use {
    ds::{ColumnTrie, IndexStructure, TreeTrie},
    heap_size::HeapSize,
    relation::{ModelType, Projectable, Relation, RelationError, RelationFileExt, RelationHeader},
};
