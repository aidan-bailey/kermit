//! On-disk cache for benchmark relation files.
//!
//! Relation files referenced by a [`BenchmarkDefinition`] via `url` are
//! downloaded lazily into the platform cache directory under
//! `<cache_dir>/kermit/benchmarks/<benchmark>/<relation>.parquet`. On Linux
//! this resolves to `~/.cache/kermit/benchmarks/…`. Relations referenced via
//! `path` are committed in the workspace and bypass the cache entirely. A
//! relation that declares a `sha256` has its downloaded bytes checked before
//! they reach the cache path; [`verify_integrity`] re-checks declared digests
//! on demand.
//!
//! [`ensure_cached`] is the entry point; [`clean_benchmark`] and [`clean_all`]
//! remove cached files.

use {
    crate::{definition::BenchmarkDefinition, error::BenchError},
    std::{
        fs, io,
        path::{Path, PathBuf},
    },
};

/// Returns the base cache directory for kermit benchmarks
/// (`$XDG_CACHE_HOME/kermit/benchmarks` on Linux).
///
/// # Errors
///
/// Returns [`BenchError::NoCacheDir`] if the platform cache directory cannot
/// be determined.
pub fn base_cache_dir() -> Result<PathBuf, BenchError> {
    let cache = dirs::cache_dir().ok_or(BenchError::NoCacheDir)?;
    Ok(cache.join("kermit").join(crate::BENCHMARKS_DIR))
}

/// Returns the cache directory for a specific benchmark.
///
/// # Errors
///
/// Returns [`BenchError::NoCacheDir`] if the platform cache directory cannot
/// be determined.
pub fn cache_dir(benchmark_name: &str) -> Result<PathBuf, BenchError> {
    Ok(base_cache_dir()?.join(benchmark_name))
}

/// Returns the expected path for a cached relation file.
///
/// # Errors
///
/// Returns [`BenchError::NoCacheDir`] if the platform cache directory cannot
/// be determined.
pub fn relation_cache_path(
    benchmark_name: &str, relation_name: &str,
) -> Result<PathBuf, BenchError> {
    Ok(cache_dir(benchmark_name)?.join(format!("{relation_name}.parquet")))
}

/// Returns true if all relation files for the benchmark are cached.
///
/// # Errors
///
/// Returns [`BenchError::NoCacheDir`] if the platform cache directory cannot
/// be determined.
/// Relations committed in the repository are always "cached" — they are never
/// fetched and occupy no cache entry — so a benchmark built entirely from
/// local paths reports cached without touching the cache directory.
pub fn is_cached(benchmark: &BenchmarkDefinition) -> Result<bool, BenchError> {
    for rel in &benchmark.relations {
        if rel.is_local() {
            continue;
        }
        let path = relation_cache_path(&benchmark.name, &rel.name)?;
        if !path.exists() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Resolves every relation of a benchmark to a readable file, fetching the
/// ones that declare a `url` and are not yet cached.
///
/// Returns paths in the same order as the benchmark's relations list. A
/// relation declaring `path` resolves to `workspace_root/<path>` and is never
/// downloaded, copied, or cached — the committed file is read in place, so a
/// benchmark built from local paths runs offline and on a cold cache.
///
/// # Errors
///
/// Returns a [`BenchError`] if any of the following occur:
/// - [`BenchError::NoCacheDir`] — the platform cache directory is not
///   available.
/// - [`BenchError::Io`] — the cache directory cannot be created, a downloaded
///   file cannot be written, or a declared local path does not exist.
/// - [`BenchError::Download`] — an HTTP error occurred while fetching a
///   relation file.
/// - [`BenchError::Integrity`] — a downloaded file does not match the `sha256`
///   its relation declares.
pub fn ensure_cached(
    benchmark: &BenchmarkDefinition, workspace_root: &Path,
) -> Result<Vec<PathBuf>, BenchError> {
    let mut paths = Vec::with_capacity(benchmark.relations.len());

    for rel in &benchmark.relations {
        if let Some(local) = &rel.path {
            let path = workspace_root.join(local);
            if !path.exists() {
                return Err(BenchError::Io(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "benchmark '{}' declares relation '{}' at '{}', which does not exist",
                        benchmark.name,
                        rel.name,
                        path.display()
                    ),
                )));
            }
            paths.push(path);
            continue;
        }

        let path = relation_cache_path(&benchmark.name, &rel.name)?;
        if !path.exists() {
            // `validate` guarantees the XOR, so a non-local relation has a url.
            let url = rel.url.as_deref().unwrap_or_default();
            eprintln!("  downloading {} from {url}...", rel.name);
            // A declared digest is checked before the file reaches its cache
            // path.
            download_file(url, &path, &rel.name, rel.sha256.as_deref())?;
        }
        paths.push(path);
    }

    Ok(paths)
}

/// Streaming SHA-256 of a file, as 64 lowercase hex characters.
///
/// # Errors
///
/// Returns [`BenchError::Io`] if the file cannot be read.
pub fn sha256_hex(path: &Path) -> Result<String, BenchError> {
    use {
        io::Read as _,
        sha2::{Digest, Sha256},
    };
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    // Hand-rolled instead of `io::copy` because sha2 0.11 does not expose
    // the `std::io::Write` impl on `Sha256`.
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(crate::definition::hex_digest(&hasher.finalize()))
}

/// Compares `bytes` against a declared digest. `Err` carries the actual
/// digest so the caller can report both.
fn check_digest(bytes: &[u8], expected: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let actual = crate::definition::hex_digest(&Sha256::digest(bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(actual)
    }
}

/// Re-hashes every relation of `benchmark` that declares a `sha256`
/// (cached `url:` files and committed `path:` files alike) and returns how
/// many were checked. Callers run [`ensure_cached`] first.
///
/// # Errors
///
/// [`BenchError::Integrity`] on the first mismatch; [`BenchError::Io`] if a
/// file is missing or unreadable; [`BenchError::NoCacheDir`] if the cache
/// directory cannot be determined.
pub fn verify_integrity(
    benchmark: &BenchmarkDefinition, workspace_root: &Path,
) -> Result<usize, BenchError> {
    let mut checked = 0;
    for rel in &benchmark.relations {
        let Some(expected) = &rel.sha256 else {
            continue;
        };
        let path = match &rel.path {
            | Some(local) => workspace_root.join(local),
            | None => relation_cache_path(&benchmark.name, &rel.name)?,
        };
        let actual = sha256_hex(&path)?;
        if &actual != expected {
            return Err(BenchError::Integrity {
                relation: rel.name.clone(),
                location: path.display().to_string(),
                expected: expected.clone(),
                actual,
            });
        }
        checked += 1;
    }
    Ok(checked)
}

/// Downloads a file from a URL to the given destination path, checking it
/// against `expected` (a declared `sha256`) before anything is written.
fn download_file(
    url: &str, dest: &Path, relation: &str, expected: Option<&str>,
) -> Result<(), BenchError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    // Atomic-download staging: write bytes to a `.part` sibling, then rename it
    // onto `dest` only on success, so a crashed download never leaves a
    // truncated file at the real cache path.
    let part_path = dest.with_extension("parquet.part");

    let response = reqwest::blocking::get(url)
        .and_then(|r| r.error_for_status())
        .map_err(|source| BenchError::Download {
            url: url.to_string(),
            source,
        })?;

    let bytes = response.bytes().map_err(|source| BenchError::Download {
        url: url.to_string(),
        source,
    })?;

    if let Some(expected) = expected {
        if let Err(actual) = check_digest(&bytes, expected) {
            return Err(BenchError::Integrity {
                relation: relation.to_string(),
                location: url.to_string(),
                expected: expected.to_string(),
                actual,
            });
        }
    }

    fs::write(&part_path, &bytes)?;
    fs::rename(&part_path, dest)?;

    Ok(())
}

/// Removes the cache directory for a specific benchmark.
///
/// A non-existent cache directory is treated as success (idempotent).
///
/// # Errors
///
/// Returns a [`BenchError`] if:
/// - [`BenchError::NoCacheDir`] — the platform cache directory is not
///   available.
/// - [`BenchError::Io`] — the directory exists but cannot be removed.
pub fn clean_benchmark(name: &str) -> Result<(), BenchError> {
    let dir = cache_dir(name)?;
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

/// Removes the entire kermit benchmark cache.
///
/// A non-existent cache directory is treated as success (idempotent).
///
/// # Errors
///
/// Returns a [`BenchError`] if:
/// - [`BenchError::NoCacheDir`] — the platform cache directory is not
///   available.
/// - [`BenchError::Io`] — the directory exists but cannot be removed.
pub fn clean_all() -> Result<(), BenchError> {
    let dir = base_cache_dir()?;
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use {super::*, crate::RelationSource};

    #[test]
    fn cache_dir_structure() {
        let dir = cache_dir("triangle").unwrap();
        assert!(dir.ends_with("kermit/benchmarks/triangle"));
    }

    #[test]
    fn relation_cache_path_structure() {
        let path = relation_cache_path("triangle", "edge").unwrap();
        assert!(path.ends_with("kermit/benchmarks/triangle/edge.parquet"));
    }

    #[test]
    fn is_cached_false_when_missing() {
        let def = BenchmarkDefinition {
            name: "nonexistent_test_benchmark".to_string(),
            description: String::new(),
            relations: vec![crate::definition::RelationSource {
                name: "r".to_string(),
                url: Some("http://x".to_string()),
                path: None,
                sha256: None,
            }],
            queries: vec![crate::definition::QueryDefinition {
                name: "q".to_string(),
                description: "test".to_string(),
                query: "Q(X) :- r(X).".to_string(),
                expected: None,
            }],
            generator: None,
        };
        assert!(!is_cached(&def).unwrap());
    }

    #[test]
    fn clean_nonexistent_is_noop() {
        assert!(clean_benchmark("this_benchmark_does_not_exist_12345").is_ok());
    }

    #[test]
    fn sha256_hex_matches_a_known_vector() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("abc.txt");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_hex(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn check_digest_accepts_match_and_reports_mismatch() {
        let ok = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(check_digest(b"abc", ok), Ok(()));
        let bad = "0".repeat(64);
        assert_eq!(check_digest(b"abc", &bad), Err(ok.to_string()));
    }

    fn local_bench(root: &std::path::Path, sha256: Option<&str>) -> BenchmarkDefinition {
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data/edge.csv"), b"abc").unwrap();
        BenchmarkDefinition {
            name: "local".to_string(),
            description: String::new(),
            relations: vec![RelationSource {
                name: "edge".to_string(),
                url: None,
                path: Some("data/edge.csv".to_string()),
                sha256: sha256.map(str::to_string),
            }],
            queries: vec![],
            generator: None,
        }
    }

    #[test]
    fn verify_integrity_counts_checked_relations() {
        let root = tempfile::tempdir().unwrap();
        let ok = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(
            verify_integrity(&local_bench(root.path(), Some(ok)), root.path()).unwrap(),
            1
        );
        assert_eq!(
            verify_integrity(&local_bench(root.path(), None), root.path()).unwrap(),
            0
        );
    }

    #[test]
    fn verify_integrity_reports_a_mismatch() {
        let root = tempfile::tempdir().unwrap();
        let bad = "0".repeat(64);
        let err = verify_integrity(&local_bench(root.path(), Some(&bad)), root.path()).unwrap_err();
        match err {
            | BenchError::Integrity {
                relation,
                expected,
                actual,
                ..
            } => {
                assert_eq!(relation, "edge");
                assert_eq!(expected, bad);
                assert_eq!(
                    actual,
                    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                );
            },
            | other => panic!("expected Integrity, got {other:?}"),
        }
    }
}
