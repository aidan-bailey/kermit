//! Sandbox + temp-dir staging for the watdiv binary.
//!
//! The watdiv binary expects the layout `<cwd>/../../files/firstnames.txt`
//! relative to `bin/Release/watdiv`. We satisfy that by staging a fresh
//! temp dir per generation:
//! ```text
//! <stage>/bin/Release/watdiv  -> <resolved binary path>  (symlink)
//! <stage>/files/firstnames.txt
//! <stage>/files/lastnames.txt
//! <stage>/files/words
//! ```
//! `TempStagingDir` is RAII: removes itself on Drop.

use {
    crate::error::RdfError,
    std::{
        fs,
        path::{Path, PathBuf},
    },
};

/// Owns a staging temp dir for the lifetime of one generation.
///
/// Holds a [`tempfile::TempDir`] directly, so cleanup is guaranteed via the
/// inner TempDir's `Drop` even if `create` returns `Err` partway through.
pub struct TempStagingDir {
    inner: tempfile::TempDir,
}

impl TempStagingDir {
    /// Creates the staging layout for an existing `binary_path`.
    /// `vendor_files` must be a directory containing `firstnames.txt`,
    /// `lastnames.txt`, and `words`.
    pub fn create(binary_path: &Path, vendor_files: &Path) -> Result<Self, RdfError> {
        let inner = tempfile::Builder::new()
            .prefix("kermit-watdiv-")
            .tempdir()?;
        let root = inner.path();

        let bin_release = root.join("bin").join("Release");
        fs::create_dir_all(&bin_release)?;
        let stage_bin = bin_release.join("watdiv");
        std::os::unix::fs::symlink(binary_path, &stage_bin).map_err(|e| {
            RdfError::Sandbox(format!("symlink {binary_path:?} -> {stage_bin:?}: {e}"))
        })?;

        let files_dir = root.join("files");
        fs::create_dir_all(&files_dir)?;
        for name in ["firstnames.txt", "lastnames.txt", "words"] {
            let src = vendor_files.join(name);
            let dst = files_dir.join(name);
            fs::copy(&src, &dst)
                .map_err(|e| RdfError::Sandbox(format!("copy {src:?} -> {dst:?}: {e}")))?;
        }

        Ok(Self {
            inner,
        })
    }

    /// Returns the staged binary path (the symlink under `bin/Release/`).
    pub fn binary_path(&self) -> PathBuf {
        self.inner.path().join("bin").join("Release").join("watdiv")
    }

    /// Returns the staging root.
    pub fn root(&self) -> &Path { self.inner.path() }

    /// Returns the staged words file, used for bind-mounting to
    /// `/usr/share/dict/words`.
    pub fn words_path(&self) -> PathBuf { self.inner.path().join("files").join("words") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_vendor_files(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        for name in ["firstnames.txt", "lastnames.txt", "words"] {
            fs::write(dir.join(name), b"sample\n").unwrap();
        }
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri does not support fchmod via std::fs::copy")]
    fn staging_creates_expected_layout() {
        let workdir = tempfile::tempdir().unwrap();
        let bin = workdir.path().join("real_watdiv");
        fs::write(&bin, b"#!/bin/sh\n").unwrap();
        let vendor = workdir.path().join("vendor");
        make_vendor_files(&vendor);

        let stage = TempStagingDir::create(&bin, &vendor).unwrap();
        let staged = stage.binary_path();
        assert!(staged.exists() || staged.symlink_metadata().is_ok());
        assert!(stage.root().join("files/firstnames.txt").exists());
        assert!(stage.root().join("files/words").exists());
    }

    #[test]
    #[cfg_attr(miri, ignore = "miri does not support fchmod via std::fs::copy")]
    fn drop_removes_root() {
        let workdir = tempfile::tempdir().unwrap();
        let bin = workdir.path().join("real_watdiv");
        fs::write(&bin, b"#!/bin/sh\n").unwrap();
        let vendor = workdir.path().join("vendor");
        make_vendor_files(&vendor);

        let root_path: PathBuf;
        {
            let stage = TempStagingDir::create(&bin, &vendor).unwrap();
            root_path = stage.root().to_path_buf();
            assert!(root_path.exists());
        }
        assert!(!root_path.exists(), "stage dir should be cleaned up");
    }
}
