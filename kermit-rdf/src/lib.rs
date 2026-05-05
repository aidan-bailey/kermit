//! RDF/SPARQL preprocessing pipeline for Kermit benchmarks.
//!
//! Drives the upstream WatDiv binary, parses its N-Triples + SPARQL output,
//! produces a kermit-runnable benchmark artifact set (dict + per-predicate
//! Parquet, BenchmarkDefinition YAML, expected cardinalities).
#![deny(missing_docs)]

pub mod dict;
pub mod driver;
pub mod error;
pub mod expected;
pub mod lubm;
pub mod ntriples;
pub mod parquet;
pub mod partition;
pub mod pipeline;
pub mod sparql;
pub mod value;
pub mod yaml_emit;

pub use error::RdfError;
