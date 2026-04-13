//! Benchmark infrastructure for Kermit.
//!
//! Provides YAML-based benchmark definitions, discovery from a `benchmarks/`
//! directory, and download/cache management for relation files hosted on
//! ZivaHub.

pub mod cache;
pub mod definition;
pub mod discovery;
pub mod error;

pub use {
    definition::{BenchmarkDefinition, RelationSource},
    error::BenchError,
};
