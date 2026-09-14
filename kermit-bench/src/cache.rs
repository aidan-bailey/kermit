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
    crate::{
        definition::{BenchmarkDefinition, RelationSource},
        error::BenchError,
    },
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
/// - [`BenchError::UnusableDownload`] — the server answered without error but
///   did not send a Parquet file (non-200 status, empty body, missing magic).
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
            download_file(url, &path, rel)?;
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

/// Four bytes that open and close every Parquet file.
const PARQUET_MAGIC: &[u8] = b"PAR1";

/// Explains why a response that passed `error_for_status` is still not a
/// relation file, or returns `None` if it is one.
///
/// `error_for_status` only rejects 4xx/5xx, so a `202 Accepted` with an empty
/// body — what an AWS WAF bot challenge looks like to a non-browser client —
/// would otherwise be cached as a zero-byte `.parquet` and never re-fetched.
fn unusable_download(
    status: reqwest::StatusCode, waf_action: Option<&str>, body: &[u8],
) -> Option<String> {
    if status != reqwest::StatusCode::OK {
        return Some(match waf_action {
            | Some(action) => format!(
                "HTTP {status} with `x-amzn-waf-action: {action}`: the host answered with a \
                 bot-protection challenge instead of the file"
            ),
            | None => format!("expected HTTP 200 OK, got {status}"),
        });
    }
    if body.is_empty() {
        return Some("the response body is empty".to_string());
    }
    let is_parquet = body.len() >= 2 * PARQUET_MAGIC.len()
        && body.starts_with(PARQUET_MAGIC)
        && body.ends_with(PARQUET_MAGIC);
    if !is_parquet {
        return Some(format!(
            "the {}-byte body lacks the Parquet `PAR1` magic at both ends",
            body.len()
        ));
    }
    None
}

/// Downloads a file from a URL to the given destination path, rejecting a
/// response that is not a Parquet file (see [`unusable_download`]) and
/// checking it against `rel`'s declared `sha256` (if any) before anything is
/// written.
fn download_file(url: &str, dest: &Path, rel: &RelationSource) -> Result<(), BenchError> {
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

    let status = response.status();
    let waf_action = response
        .headers()
        .get("x-amzn-waf-action")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let bytes = response.bytes().map_err(|source| BenchError::Download {
        url: url.to_string(),
        source,
    })?;

    if let Some(reason) = unusable_download(status, waf_action.as_deref(), &bytes) {
        return Err(BenchError::UnusableDownload {
            url: url.to_string(),
            reason,
        });
    }

    if let Some(expected) = rel.sha256.as_deref() {
        if let Err(actual) = check_digest(&bytes, expected) {
            return Err(BenchError::Integrity {
                relation: rel.name.clone(),
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

    /// Serves `response` verbatim to the first connection on a loopback port
    /// and returns the URL to fetch it from.
    fn serve_once(response: Vec<u8>) -> String {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/relation.parquet", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            stream.write_all(&response).unwrap();
        });
        url
    }

    fn http_response(status: &str, headers: &[&str], body: &[u8]) -> Vec<u8> {
        let mut head = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for header in headers {
            head.push_str(header);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        let mut response = head.into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn url_relation(url: &str) -> RelationSource {
        RelationSource {
            name: "edge".to_string(),
            url: Some(url.to_string()),
            path: None,
            sha256: None,
        }
    }

    /// Smallest byte string carrying the Parquet magic at both ends.
    const PARQUET_LIKE: &[u8] = b"PAR1\x00\x00\x00\x00PAR1";

    /// Fetches `response` into a fresh temp cache path and returns the error
    /// text, asserting that neither the cache file nor its `.part` staging
    /// sibling was left behind.
    fn rejected_download(response: Vec<u8>) -> String {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("edge.parquet");
        let url = serve_once(response);
        let err = download_file(&url, &dest, &url_relation(&url))
            .expect_err("download should have been rejected");
        assert!(
            !dest.exists(),
            "a rejected download must not reach the cache"
        );
        assert!(!dest.with_extension("parquet.part").exists());
        err.to_string()
    }

    #[test]
    fn download_writes_a_parquet_body_served_with_200() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("edge.parquet");
        let url = serve_once(http_response("200 OK", &[], PARQUET_LIKE));
        download_file(&url, &dest, &url_relation(&url)).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), PARQUET_LIKE);
    }

    #[test]
    fn download_rejects_a_waf_challenge_and_names_it() {
        // ZivaHub's AWS WAF answers non-browser clients this way (issue #70).
        let msg = rejected_download(http_response(
            "202 Accepted",
            &["x-amzn-waf-action: challenge"],
            b"",
        ));
        assert!(msg.contains("challenge"), "{msg}");
    }

    #[test]
    fn download_rejects_a_success_status_other_than_200() {
        let msg = rejected_download(http_response("202 Accepted", &[], PARQUET_LIKE));
        assert!(msg.contains("202"), "{msg}");
    }

    #[test]
    fn download_rejects_an_empty_200_body() {
        let msg = rejected_download(http_response("200 OK", &[], b""));
        assert!(msg.contains("empty"), "{msg}");
    }

    #[test]
    fn download_rejects_a_200_body_that_is_not_parquet() {
        let msg = rejected_download(http_response(
            "200 OK",
            &["Content-Type: text/html"],
            b"<html>please enable JavaScript</html>",
        ));
        assert!(msg.contains("Parquet"), "{msg}");
    }

    #[test]
    fn download_rejects_a_parquet_body_cut_off_before_its_footer() {
        let truncated = &PARQUET_LIKE[..PARQUET_LIKE.len() - 1];
        let msg = rejected_download(http_response("200 OK", &[], truncated));
        assert!(msg.contains("Parquet"), "{msg}");
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
