//! Temp-dir staging for one LUBM-UBA invocation.
//!
//! The LUBM-UBA jar is self-contained: it writes its output under whatever
//! `-o <dir>` it's given. We hand it a fresh `tempfile::TempDir` and let it
//! emit `Universities.nt.gz` (with `--consolidate Maximal --compress`).
//! `TempStagingDir` is RAII — on `Drop` the inner `TempDir` removes the
//! whole tree, including the produced `.nt` files.
//!
//! This is intentionally simpler than `crate::driver::sandbox::TempStagingDir`
//! because the LUBM jar has no symlink/file-layout requirements (vs the
//! vendored watdiv binary which expects `<cwd>/../../files/firstnames.txt`).

use {
    crate::error::RdfError,
    std::path::{Path, PathBuf},
};

/// Owns a staging temp dir for the lifetime of one generation.
pub struct LubmStagingDir {
    inner: tempfile::TempDir,
}

impl LubmStagingDir {
    /// Creates a fresh staging dir under the system temp root.
    pub fn create() -> Result<Self, RdfError> {
        let inner = tempfile::Builder::new().prefix("kermit-lubm-").tempdir()?;
        Ok(Self {
            inner,
        })
    }

    /// Returns the staging root.
    pub fn root(&self) -> &Path { self.inner.path() }

    /// Path the LUBM-UBA jar is expected to write.
    pub fn compressed_output_path(&self) -> PathBuf { self.root().join("Universities.nt.gz") }

    /// Path the driver writes after gunzipping.
    pub fn ntriples_output_path(&self) -> PathBuf { self.root().join("Universities.nt") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_root_exists_and_is_within_tmp() {
        let stage = LubmStagingDir::create().unwrap();
        assert!(stage.root().exists());
        assert!(stage.root().is_dir());
        let name = stage
            .root()
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        assert!(
            name.starts_with("kermit-lubm-"),
            "stage dir name should start with kermit-lubm-, got {name:?}"
        );
    }

    #[test]
    fn output_paths_are_within_root() {
        let stage = LubmStagingDir::create().unwrap();
        assert!(stage.compressed_output_path().starts_with(stage.root()));
        assert!(stage.ntriples_output_path().starts_with(stage.root()));
    }

    #[test]
    fn drop_removes_staging_dir() {
        let root_path: PathBuf;
        {
            let stage = LubmStagingDir::create().unwrap();
            root_path = stage.root().to_path_buf();
            assert!(root_path.exists());
        }
        assert!(
            !root_path.exists(),
            "staging dir should be cleaned up after Drop"
        );
    }
}
