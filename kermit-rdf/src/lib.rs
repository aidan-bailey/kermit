//! RDF/SPARQL preprocessing pipelines for Kermit benchmarks.
//!
//! Drives the upstream WatDiv binary and the LUBM-UBA jar (the latter with
//! Univ-Bench TBox entailment, see [`lubm`]), parses their N-Triples + SPARQL
//! output, and produces a kermit-runnable benchmark artifact set (dict +
//! per-predicate Parquet, BenchmarkDefinition YAML carrying any known expected
//! cardinalities).
#![deny(missing_docs)]

pub mod dict;
pub mod driver;
pub mod error;
pub mod generator;
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
    Ok(hex_digest(&h.finalize()))
}

/// Lowercase, zero-padded hex encoding of a digest.
///
/// sha2 0.11 returns a `hybrid_array::Array` from `finalize`, which no longer
/// implements `LowerHex`. This reproduces the exact string the previous
/// `format!("{:x}", ..)` produced — the digests are persisted (in `meta.json`,
/// and compared by `spec_hash` drift detection), so the encoding must not
/// shift under a dependency bump.
fn hex_digest(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(out, "{b:02x}").expect("writing to a String never fails");
    }
    out
}

#[cfg(test)]
mod hex_digest_tests {
    use super::*;

    /// Pins the encoding against the NIST SHA-256 vector for `"abc"`. The
    /// digests this crate emits are written into `meta.json` and compared on
    /// later runs, so a formatting change here would read as spec drift on
    /// every already-cached benchmark.
    #[test]
    fn hex_digest_matches_the_nist_abc_vector() {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"abc");
        assert_eq!(
            hex_digest(&h.finalize()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
