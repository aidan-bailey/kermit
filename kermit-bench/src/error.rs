use std::path::PathBuf;

/// Errors that can occur during benchmark operations.
#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    #[error("YAML parse error for {path}: {source}")]
    Yaml {
        path: PathBuf,
        source: serde_yaml::Error,
    },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("download failed for {url}: {source}")]
    Download { url: String, source: reqwest::Error },

    #[error("benchmark not found: {0}")]
    NotFound(String),

    #[error("invalid benchmark definition {name}: {reason}")]
    Invalid { name: String, reason: String },

    #[error("cache directory not available")]
    NoCacheDir,
}
