//! Error type for the kermit-rdf crate.

use std::path::PathBuf;

/// Errors that can occur during RDF preprocessing.
#[derive(Debug, thiserror::Error)]
pub enum RdfError {
    /// The watdiv binary could not be found at the resolved path.
    #[error("watdiv binary not found at {path:?} (set --watdiv-bin or KERMIT_WATDIV_BIN)")]
    BinaryNotFound {
        /// The path that was searched.
        path: PathBuf,
    },

    /// The watdiv binary exited with a non-zero status.
    #[error("watdiv exited with status {status}: {stderr}")]
    BinaryFailed {
        /// Exit status as a string ("exit code 1", "killed by signal SIGSEGV", etc.).
        status: String,
        /// Captured stderr.
        stderr: String,
    },

    /// Sandbox setup (bwrap detection, mount construction) failed.
    #[error("sandbox setup failed: {0}")]
    Sandbox(String),

    /// Parsing N-Triples failed.
    #[error("N-Triples parse error at line {line}: {message}")]
    NTriplesParse {
        /// 1-indexed line number where the error occurred.
        line: usize,
        /// Human-readable message.
        message: String,
    },

    /// Parsing a SPARQL query failed.
    #[error("SPARQL parse error: {0}")]
    SparqlParse(String),

    /// SPARQL feature not expressible as a Datalog rule (FILTER, OPTIONAL, etc.).
    #[error("unsupported SPARQL feature: {0}")]
    UnsupportedSparql(String),

    /// Computing expected results failed.
    #[error("expected-results computation failed: {0}")]
    Expected(String),

    /// Underlying I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Underlying Arrow error.
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// Underlying Parquet error.
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_not_found_message_includes_path() {
        let err = RdfError::BinaryNotFound {
            path: PathBuf::from("/no/such/path"),
        };
        let msg = format!("{err}");
        assert!(msg.contains("/no/such/path"));
        assert!(msg.contains("watdiv binary not found"));
    }

    #[test]
    fn io_error_wraps_transparently() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "boom");
        let err: RdfError = io_err.into();
        assert!(matches!(err, RdfError::Io(_)));
    }
}
