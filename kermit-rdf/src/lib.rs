//! RDF/SPARQL preprocessing pipelines for Kermit benchmarks.
//!
//! Drives the upstream WatDiv binary and the LUBM-UBA jar (the latter with
//! Univ-Bench TBox entailment, see [`lubm`]), parses their N-Triples + SPARQL
//! output, and produces a kermit-runnable benchmark artifact set (dict +
//! per-predicate Parquet, BenchmarkDefinition YAML, expected cardinalities).
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
mod timestamp;
pub mod value;
pub mod yaml_emit;

pub use error::RdfError;
use {
    sha2::{Digest, Sha256},
    std::{io::Read, path::Path},
};

/// Computes the SHA-256 of a file's bytes, returning the lowercase hex digest.
///
/// Streams the file in 8 KiB chunks so the whole content never lives in
/// memory at once — important for the large `.nt`/`.parquet` artifacts the
/// pipeline produces.
pub(crate) fn sha256_file(path: &Path) -> Result<String, RdfError> {
    let mut h = Sha256::new();
    let mut f = std::fs::File::open(path)?;
    let mut buf = [0u8; 8192];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
