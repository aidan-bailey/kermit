//! On-disk cache for benchmark relation files.
//!
//! Relation files referenced by a [`BenchmarkDefinition`] are downloaded lazily
//! into the platform cache directory under
//! `<cache_dir>/kermit/benchmarks/<benchmark>/<relation>.parquet`. On Linux
//! this resolves to `~/.cache/kermit/benchmarks/…`.
//!
//! [`ensure_cached`] is the entry point; [`clean_benchmark`] and [`clean_all`]
//! remove cached files.

use {
    crate::{definition::BenchmarkDefinition, error::BenchError},
    std::{
        fs,
        path::{Path, PathBuf},
    },
};

/// Returns the base cache directory for kermit benchmarks.
fn base_cache_dir() -> Result<PathBuf, BenchError> {
    let cache = dirs::cache_dir().ok_or(BenchError::NoCacheDir)?;
    Ok(cache.join("kermit").join("benchmarks"))
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
pub fn is_cached(benchmark: &BenchmarkDefinition) -> Result<bool, BenchError> {
    for rel in &benchmark.relations {
        let path = relation_cache_path(&benchmark.name, &rel.name)?;
        if !path.exists() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Ensures all relations for a benchmark are downloaded and cached.
///
/// Returns paths to the cached files in the same order as the benchmark's
/// relations list.
///
/// # Errors
///
/// Returns a [`BenchError`] if any of the following occur:
/// - [`BenchError::NoCacheDir`] — the platform cache directory is not
///   available.
/// - [`BenchError::Io`] — the cache directory cannot be created, or a
///   downloaded file cannot be written.
/// - [`BenchError::Download`] — an HTTP error occurred while fetching a
///   relation file.
pub fn ensure_cached(benchmark: &BenchmarkDefinition) -> Result<Vec<PathBuf>, BenchError> {
    let mut paths = Vec::with_capacity(benchmark.relations.len());

    for rel in &benchmark.relations {
        let path = relation_cache_path(&benchmark.name, &rel.name)?;
        if !path.exists() {
            eprintln!("  downloading {} from {}...", rel.name, rel.url);
            download_file(&rel.url, &path)?;
        }
        paths.push(path);
    }

    Ok(paths)
}

/// Downloads a file from a URL to the given destination path.
fn download_file(url: &str, dest: &Path) -> Result<(), BenchError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

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
    use super::*;

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
                url: "http://x".to_string(),
            }],
            queries: vec![crate::definition::QueryDefinition {
                name: "q".to_string(),
                description: "test".to_string(),
                query: "Q(X) :- r(X).".to_string(),
            }],
        };
        assert!(!is_cached(&def).unwrap());
    }

    #[test]
    fn clean_nonexistent_is_noop() {
        assert!(clean_benchmark("this_benchmark_does_not_exist_12345").is_ok());
    }
}
