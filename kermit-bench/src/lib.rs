//! Benchmark infrastructure for Kermit.
//!
//! Provides YAML-based benchmark definitions, discovery from a `benchmarks/`
//! directory, and download/cache management for relation files hosted on
//! ZivaHub.
//!
//! See the [workspace `benchmarks/README.md`][] for the YAML schema.
//!
//! [workspace `benchmarks/README.md`]: https://github.com/AlexStrickland/kermit/blob/master/benchmarks/README.md
#![deny(missing_docs)]

pub mod cache;
pub mod definition;
pub mod discovery;
pub mod error;

pub use {
    definition::{BenchmarkDefinition, QueryDefinition, RelationSource},
    error::BenchError,
};
