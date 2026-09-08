//! Data structures for Kermit's relational algebra engine.
//!
//! Provides three trie-based relation implementations:
//!
//! - [`TreeTrie`]: A pointer-based trie where each node owns its children.
//!   Simple; allocates a node per key — preferable for small relations and
//!   pedagogy.
//! - [`ColumnTrie`]: A column-oriented (flattened) trie that stores each level
//!   in parallel `data`/`interval` arrays. More compact for large relations.
//! - [`HashTrie`]: A hash-based trie (nested hash tables, one per attribute),
//!   generic over a `HashStrategy`.
//!
//! All three implement the [`Relation`] trait. `TreeTrie` and `ColumnTrie`
//! additionally implement [`TrieIterable`](kermit_iters::TrieIterable);
//! `HashTrie` implements [`HashTrieIterable`](kermit_iters::HashTrieIterable)
//! instead — algorithms select the matching iterable trait.
#![deny(missing_docs)]

mod cardinality;
mod configured;
mod ds;
mod heap_size;
mod relation;

// Re-export IndexStructure for external crates (CLI) to reference directly
pub use {
    cardinality::Cardinality,
    configured::{ConfigProvider, Configured},
    ds::{ColumnTrie, HashTrie, HashTrieConfig, IndexStructure, TreeTrie},
    heap_size::HeapSize,
    relation::{
        ConfigurableRelation, ModelType, Projectable, Relation, RelationError, RelationFileExt,
        RelationHeader,
    },
};
