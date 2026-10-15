//! Where `bench gen <kind>` writes one benchmark.

use std::path::{Path, PathBuf};

/// Where `bench gen <kind>` writes one benchmark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDir {
    /// `<parent>/<bench_name>`.
    pub path: PathBuf,
    /// Whether the parent is the cache root that `bench list/fetch/run`
    /// scan, so that the benchmark will be discovered there.
    pub discovered: bool,
}

/// Resolves `<parent or the cache root>/<bench_name>`, the rule
/// `bench gen lubm` applies to its `--output-dir`.
///
/// `parent` is `--output-dir`; `None` means the cache root,
/// [`kermit_bench::cache::base_cache_dir`]. The result is
/// [`discovered`](OutputDir::discovered) when the parent is that root.
/// The comparison is lexical: no symlink or `..` is resolved. The function
/// is pure: it creates nothing, prints nothing and does not look at the
/// workspace.
///
/// # Errors
///
/// Fails only when `parent` is `None` and the platform has no cache
/// directory. With an explicit `parent` it always succeeds; the result is
/// then undiscovered if there is no cache directory to compare against.
pub fn output_dir(bench_name: &str, parent: Option<&Path>) -> anyhow::Result<OutputDir> {
    let cache_root = kermit_bench::cache::base_cache_dir();
    let (parent, discovered) = match parent {
        | Some(dir) => (dir.to_path_buf(), cache_root.is_ok_and(|root| root == dir)),
        | None => (
            cache_root.map_err(|e| anyhow::anyhow!("no cache directory available: {e}"))?,
            true,
        ),
    };
    Ok(OutputDir {
        path: parent.join(bench_name),
        discovered,
    })
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_bench::cache::base_cache_dir};

    #[test]
    fn explicit_parent_is_not_discovered() {
        let parent = tempfile::tempdir().unwrap();
        let out = output_dir("h2o-test", Some(parent.path())).unwrap();
        assert_eq!(out, OutputDir {
            path: parent.path().join("h2o-test"),
            discovered: false,
        });
        assert!(
            !out.path.exists(),
            "output_dir must not create the directory"
        );
    }

    #[test]
    fn default_parent_is_the_cache_root() {
        let root = base_cache_dir().unwrap();
        let out = output_dir("h2o-test", None).unwrap();
        assert_eq!(out, OutputDir {
            path: root.join("h2o-test"),
            discovered: true,
        });
    }

    #[test]
    fn explicit_cache_root_is_discovered() {
        let root = base_cache_dir().unwrap();
        let out = output_dir("h2o-test", Some(root.as_path())).unwrap();
        assert_eq!(out, OutputDir {
            path: root.join("h2o-test"),
            discovered: true,
        });
    }
}
