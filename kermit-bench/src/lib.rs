//! Benchmark infrastructure for Kermit.
//!
//! Provides YAML-based benchmark definitions, discovery from a `benchmarks/`
//! directory, and download/cache management for relation files hosted on
//! ZivaHub.
//!
//! See the [workspace `benchmarks/README.md`][] for the YAML schema.
//!
//! [workspace `benchmarks/README.md`]: https://github.com/aidan-bailey/kermit/blob/master/benchmarks/README.md
#![deny(missing_docs)]

pub mod cache;
pub mod definition;
pub mod discovery;
pub mod error;

/// Name of the `benchmark.yml` manifest inside a generator cache subdir.
pub(crate) const CACHE_MANIFEST: &str = "benchmark.yml";

/// Marker file proving a cache subdir was produced by a kermit generator.
/// Its presence (alongside [`CACHE_MANIFEST`]) is the load-bearing signal that
/// distinguishes a generator-produced benchmark from an unrelated directory.
pub(crate) const CACHE_MARKER: &str = "meta.json";

/// Name of the `benchmarks` directory — both the workspace source directory of
/// static benchmark YAML files and the cache namespace under `kermit/`.
pub(crate) const BENCHMARKS_DIR: &str = "benchmarks";

pub use {
    definition::{
        BenchmarkDefinition, GeneratorSpec, QueryDefinition, RelationSource, WatdivStressSpec,
        DEFAULT_LUBM_ONTOLOGY,
    },
    error::BenchError,
};
