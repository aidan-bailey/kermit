use std::{env, path::PathBuf};

/// Handles downloading benchmark datasets to a temporary directory.
pub struct Downloader;

/// How to fetch a benchmark dataset.
pub enum DownloadMethod {
    /// Clone a git repository.
    CLONE,
}

/// Specification for downloading a benchmark dataset.
pub struct DownloadSpec {
    /// Short identifier used as the directory name.
    pub name: &'static str,
    /// Source URL to download from.
    pub url: &'static str,
    /// The method used to fetch the dataset.
    pub method: DownloadMethod,
}

impl Downloader {
    fn tmp_dir() -> std::path::PathBuf { env::temp_dir().join("kermit") }

    fn ensure_init() {
        let tmp_dir = Self::tmp_dir();
        if !tmp_dir.exists() {
            std::fs::create_dir_all(&tmp_dir).expect("Failed to create temporary directory");
        }
    }

    /// Removes the entire Kermit temp directory and all downloaded datasets.
    pub fn cleanall() {
        let tmp_dir = Self::tmp_dir();
        if tmp_dir.exists() {
            std::fs::remove_dir_all(&tmp_dir).expect("Failed to clean up temporary directory");
        }
    }

    /// Removes the temp directory for a single dataset.
    pub fn clean(spec: &DownloadSpec) {
        let dest = Self::tmp_dir().join(spec.name);
        if dest.exists() {
            std::fs::remove_dir_all(&dest).expect("Failed to clean up temporary directory");
        }
    }

    /// Downloads a dataset according to `spec`, returning the path to the
    /// downloaded directory. No-ops if the dataset is already present.
    pub fn download(spec: &DownloadSpec) -> Result<PathBuf, Box<dyn std::error::Error>> {
        Self::ensure_init();
        let dest = Self::tmp_dir().join(spec.name);
        if dest.exists() {
            return Ok(dest.to_path_buf());
        }
        match spec.method {
            | DownloadMethod::CLONE => {
                let status = std::process::Command::new("git")
                    .args(["clone", spec.url, dest.to_str().unwrap()])
                    .status()?;
                if !status.success() {
                    Self::clean(spec);
                    return Err(format!("Git clone failed with status: {}", status).into());
                }
            },
        }
        Ok(dest.to_path_buf())
    }
}
