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
//!
//! The two sorted tries take their seek algorithm as a Layout type
//! parameter, [`SeekStrategy`]: [`LinearSeek`], [`BinarySeek`] (the
//! default) or [`GallopingSeek`].
#![deny(missing_docs)]

mod built_with;
mod cardinality;
mod configured;
mod ds;
mod heap_size;
mod morsel;
mod relation;
mod seek;
#[cfg(feature = "test-hooks")]
pub mod test_hooks;
#[cfg(test)]
mod test_support;
mod tuple_scan;

// Re-export IndexStructure for external crates (CLI) to reference directly
pub use {
    built_with::{BuildModeProvider, BuiltWith},
    cardinality::Cardinality,
    configured::{ConfigProvider, Configured},
    ds::{
        ChildCapacity, ColumnTrie, ColumnTrieBuildMode, EagerExpansion, ExpansionPolicy, HashTrie,
        HashTrieBuildMode, HashTrieConfig, IndexStructure, InvalidLoadFactor, InvalidRadixBits,
        LazyExpansion, LoadFactor, NoPruning, ParseChildCapacityError, ParseHashTrieBuildModeError,
        ParseRootCapacityError, ParseTreeTrieBuildModeError, PruningPolicy, RadixBits,
        RootCapacity, SingletonPruning, TreeTrie, TreeTrieBuildMode,
    },
    heap_size::HeapSize,
    morsel::Threads,
    relation::{
        read_csv, read_csv_header, read_parquet, read_parquet_header, BuildModeRelation,
        ConfigurableRelation, ConfiguredBuildModeRelation, ModelType, Projectable, Relation,
        RelationError, RelationFileExt, RelationHeader,
    },
    seek::{BinarySeek, GallopingSeek, LinearSeek, SeekStrategy},
    tuple_scan::TupleScan,
};
