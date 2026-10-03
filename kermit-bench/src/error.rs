//! Error type used across the `kermit-bench` crate.

use std::path::PathBuf;

/// Errors that can occur during benchmark operations.
#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    /// A YAML file failed to parse.
    #[error("YAML parse error for {path}: {source}")]
    Yaml {
        /// The file being parsed.
        path: PathBuf,
        /// The underlying serde-yaml error.
        source: serde_yaml::Error,
    },

    /// An underlying I/O error (filesystem access, reading cache files, etc.).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Downloading a relation file failed.
    #[error("download failed for {url}: {source}")]
    Download {
        /// URL that was being fetched.
        url: String,
        /// Underlying reqwest error.
        source: reqwest::Error,
    },

    /// A download succeeded at the HTTP level but did not deliver a relation
    /// file: a status other than `200 OK`, an empty body, or bytes without the
    /// Parquet magic. Nothing is written to the cache.
    #[error("download from {url} did not return a Parquet file: {reason}")]
    UnusableDownload {
        /// URL that was being fetched.
        url: String,
        /// What was wrong with the response.
        reason: String,
    },

    /// A benchmark with the requested name does not exist.
    #[error("benchmark not found: {0}")]
    NotFound(String),

    /// The benchmark definition is structurally invalid (see
    /// [`BenchmarkDefinition::validate`](crate::BenchmarkDefinition::validate)).
    #[error("invalid benchmark definition {name}: {reason}")]
    Invalid {
        /// The benchmark name as declared in the YAML.
        name: String,
        /// Human-readable description of the invariant that was violated.
        reason: String,
    },

    /// The platform cache directory could not be determined.
    #[error("cache directory not available")]
    NoCacheDir,

    /// A generator-spec YAML's parameters disagree with the cached
    /// `meta.json`'s `spec_hash`. Returned by the materialization layer to
    /// prevent silently re-running an expensive pipeline.
    #[error(
        "spec drift for benchmark '{name}': cached spec_hash={actual_hash}, current \
         spec_hash={expected_hash}; {hint}"
    )]
    SpecDrift {
        /// Benchmark name.
        name: String,
        /// Spec hash computed from the workspace YAML at this invocation.
        expected_hash: String,
        /// Spec hash recorded in the cached `meta.json`.
        actual_hash: String,
        /// Resolution hint shown to the user.
        hint: String,
    },

    /// A generator cache matches its spec but was written by an older
    /// pipeline whose output for that spec differs from today's. Returned
    /// by the materialization layer, like [`BenchError::SpecDrift`], so a
    /// stale encoding is never silently reused and never silently rebuilt.
    #[error("cache for benchmark '{name}' is outdated: {reason}; {hint}")]
    OutdatedCache {
        /// Benchmark name.
        name: String,
        /// Why the cached encoding differs from what the pipeline now writes.
        reason: String,
        /// Resolution hint shown to the user.
        hint: String,
    },

    /// A relation file already at its cache path cannot be a Parquet file:
    /// it is shorter than the format's two `PAR1` markers or lacks one, as
    /// a zero-byte or truncated file does. Reported instead of reusing it,
    /// so the problem is named here rather than as a Parquet decode error
    /// once the relation is loaded.
    #[error(
        "cached relation '{relation}' of benchmark '{benchmark}' at {path} is not a usable \
         Parquet file ({reason}); delete it with `kermit bench clean {benchmark}`, then fetch or \
         regenerate the benchmark"
    )]
    CorruptCache {
        /// Benchmark name.
        benchmark: String,
        /// Relation name as declared in the YAML.
        relation: String,
        /// The cached file.
        path: String,
        /// What is wrong with it.
        reason: String,
    },

    /// A relation file's contents do not match the `sha256` its benchmark
    /// declares.
    #[error(
        "integrity check failed for relation '{relation}' from {location}: expected sha256 \
         {expected}, got {actual}"
    )]
    Integrity {
        /// Relation name as declared in the YAML.
        relation: String,
        /// The URL it was downloaded from, or the local path that was hashed.
        location: String,
        /// Digest declared in the YAML.
        expected: String,
        /// Digest computed from the bytes.
        actual: String,
    },
}
