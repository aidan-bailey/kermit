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
}
