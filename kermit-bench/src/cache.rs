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
//! remove cached files. [`download_verified`] downloads any other file pinned
//! by its digest, such as a generator's raw input, through the same streaming
//! core: a body is staged in a `.part` file while it is hashed and renamed
//! onto its destination only once it passes its checks.

use {
    crate::{
        definition::{BenchmarkDefinition, RelationSource},
        error::BenchError,
    },
    std::{
        fs, io,
        path::{Path, PathBuf},
        time::{Duration, Instant},
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
///   file cannot be written, read back or renamed into place, or a declared
///   local path does not exist.
/// - [`BenchError::Download`] — an HTTP error occurred while fetching a
///   relation file, or the connection stalled for 30 s before or during the
///   body.
/// - [`BenchError::UnusableDownload`] — the server answered without error but
///   did not send a Parquet file (non-200 status, empty body, missing magic).
/// - [`BenchError::Integrity`] — a downloaded file does not match the `sha256`
///   its relation declares.
/// - [`BenchError::CorruptCache`] — a file already at a relation's cache path
///   cannot be a Parquet file: shorter than two `PAR1` markers, or missing one
///   at either end. It is reported, never deleted or re-downloaded.
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
        if path.exists() {
            // Downloads are atomic and checked since #70, but a file written
            // before that, or truncated or replaced since, would otherwise
            // be trusted here and fail only when decoded.
            if let Some(reason) = cached_file_problem(&path)? {
                return Err(BenchError::CorruptCache {
                    benchmark: benchmark.name.clone(),
                    relation: rel.name.clone(),
                    path: path.display().to_string(),
                    reason,
                });
            }
        } else {
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
                subject: format!("relation '{}'", rel.name),
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

/// Explains why the file at `path` cannot be a Parquet file, or returns
/// `None` if it may be one: `PAR1` at both ends, read from the file's first
/// and last four bytes so a large relation is not read whole. Applied to a
/// cached relation and, by [`check_relation_part`], to every relation
/// download before it reaches its cache path.
///
/// # Errors
///
/// [`BenchError::Io`] if the file cannot be opened or read.
fn cached_file_problem(path: &Path) -> Result<Option<String>, BenchError> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    let magic = PARQUET_MAGIC.len() as u64;
    if len < 2 * magic {
        return Ok(Some(format!(
            "it is {len} bytes, shorter than a Parquet file's two `PAR1` markers"
        )));
    }
    let mut head = [0u8; 4];
    file.read_exact(&mut head)?;
    let mut tail = [0u8; 4];
    file.seek(SeekFrom::End(-(magic as i64)))?;
    file.read_exact(&mut tail)?;
    let missing = match (head == PARQUET_MAGIC, tail == PARQUET_MAGIC) {
        | (true, true) => return Ok(None),
        | (true, false) => "its end (it was cut off before its footer)",
        | (false, true) => "its start",
        | (false, false) => "either end",
    };
    Ok(Some(format!(
        "the {len}-byte file lacks the Parquet `PAR1` magic at {missing}"
    )))
}

/// How long a download may wait: to connect, for the response (the request
/// and its headers count as one window), and for each read of the body.
/// reqwest's blocking client applies its `timeout` per operation, not to the
/// whole body, so a download that keeps arriving has no total limit and one
/// that stalls this long fails. `Response::bytes()` would wait on the whole
/// body under one timeout, which is why bodies are streamed through `Read`
/// instead.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// Bytes read from a response body per call.
const DOWNLOAD_CHUNK_BYTES: usize = 64 * 1024;

/// The client every download uses (see [`DOWNLOAD_TIMEOUT`]).
fn http_client(url: &str) -> Result<reqwest::blocking::Client, BenchError> {
    reqwest::blocking::Client::builder()
        .connect_timeout(DOWNLOAD_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|source| BenchError::Download {
            url: url.to_string(),
            source,
        })
}

/// Where a download is staged until it is checked: `dest` with `.part`
/// appended to its file name (`edge.parquet` becomes `edge.parquet.part`).
fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Streams `url` into `<dest>.part`, hands the finished part file and its
/// SHA-256 to `check`, and renames it onto `dest` only if `check` passes.
/// Every failure removes the part file and leaves `dest` as it was, so `dest`
/// only ever receives a checked file (#70).
fn fetch_checked(
    url: &str, dest: &Path, check: impl FnOnce(&Path, String) -> Result<(), BenchError>,
) -> Result<(), BenchError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let part = part_path(dest);
    let fetched = http_client(url)
        .and_then(|client| stream_to_part(&client, url, &part))
        .and_then(|sha256| check(&part, sha256))
        .and_then(|()| fs::rename(&part, dest).map_err(BenchError::from));
    if fetched.is_err() {
        // Absent when the request failed before the body; nothing to report.
        let _ = fs::remove_file(&part);
    }
    fetched
}

/// Sends a GET for `url` and streams a `200 OK` body into `part` while
/// hashing it, returning the body's SHA-256 as 64 lowercase hex characters.
/// A 4xx or 5xx response is a [`BenchError::Download`], and any other status
/// but `200 OK` is rejected by [`unusable_status`], both before the body is
/// read; an empty body is rejected after it. A large body reports its
/// progress on stderr ([`Progress`]). The caller removes `part` on failure.
fn stream_to_part(
    client: &reqwest::blocking::Client, url: &str, part: &Path,
) -> Result<String, BenchError> {
    use {
        io::{Read as _, Write as _},
        sha2::{Digest, Sha256},
    };

    let mut response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|source| BenchError::Download {
            url: url.to_string(),
            source,
        })?;
    let waf_action = response
        .headers()
        .get("x-amzn-waf-action")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if let Some(reason) = unusable_status(response.status(), waf_action.as_deref()) {
        return Err(BenchError::UnusableDownload {
            url: url.to_string(),
            reason,
        });
    }

    let mut progress = Progress::new(url, response.content_length());
    let mut file = fs::File::create(part)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; DOWNLOAD_CHUNK_BYTES];
    let mut received: u64 = 0;
    loop {
        let n = match response.read(&mut buf) {
            | Ok(n) => n,
            | Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            | Err(e) => return Err(body_read_error(url, e)),
        };
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        received += n as u64;
        progress.update(received);
    }
    if received == 0 {
        return Err(BenchError::UnusableDownload {
            url: url.to_string(),
            reason: "the response body is empty".to_string(),
        });
    }
    Ok(crate::definition::hex_digest(&hasher.finalize()))
}

/// Explains why a response that passed `error_for_status` is still not the
/// file, or returns `None` for a `200 OK`.
///
/// `error_for_status` only rejects 4xx/5xx, so a `202 Accepted` with an empty
/// body (what an AWS WAF bot challenge looks like to a non-browser client)
/// would otherwise be streamed into the cache as a zero-byte file.
fn unusable_status(status: reqwest::StatusCode, waf_action: Option<&str>) -> Option<String> {
    if status == reqwest::StatusCode::OK {
        return None;
    }
    Some(match waf_action {
        | Some(action) => format!(
            "HTTP {status} with `x-amzn-waf-action: {action}`: the host answered with a \
             bot-protection challenge instead of the file"
        ),
        | None => format!("expected HTTP 200 OK, got {status}"),
    })
}

/// The error for a failed read of a response body. `Response::read` wraps
/// reqwest's own error (a stalled or dropped connection) in an `io::Error`;
/// that becomes a [`BenchError::Download`], as the same failure before the
/// body would, and any other error stays a [`BenchError::Io`].
fn body_read_error(url: &str, err: io::Error) -> BenchError {
    match err.downcast::<reqwest::Error>() {
        | Ok(source) => BenchError::Download {
            url: url.to_string(),
            source,
        },
        | Err(err) => BenchError::Io(err),
    }
}

/// The relation checks on a downloaded part file: the Parquet magic at both
/// ends ([`cached_file_problem`]), then the relation's declared digest, if it
/// declares one.
fn check_relation_part(
    url: &str, part: &Path, rel: &RelationSource, actual: String,
) -> Result<(), BenchError> {
    if let Some(reason) = cached_file_problem(part)? {
        return Err(BenchError::UnusableDownload {
            url: url.to_string(),
            reason,
        });
    }
    match rel.sha256.as_deref() {
        | Some(expected) if expected != actual => Err(BenchError::Integrity {
            subject: format!("relation '{}'", rel.name),
            location: url.to_string(),
            expected: expected.to_string(),
            actual,
        }),
        | _ => Ok(()),
    }
}

/// Downloads a relation file from `url` to `dest`: [`fetch_checked`] with the
/// relation checks ([`check_relation_part`]).
fn download_file(url: &str, dest: &Path, rel: &RelationSource) -> Result<(), BenchError> {
    fetch_checked(url, dest, |part, actual| {
        check_relation_part(url, part, rel, actual)
    })
}

/// A body of at least this many bytes reports its progress on stderr.
const PROGRESS_MIN_BYTES: u64 = 64 << 20;

/// The least time between two progress lines.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);

/// Downloads `url` to `dest`, checked against `sha256` (64 lowercase hex
/// characters). For any file pinned by its digest, Parquet or not: a SNAP
/// graph's `.gz` (#96), JOB's IMDb dump and query files (#97).
///
/// The body is streamed into `<dest>.part` while it is hashed, and renamed
/// onto `dest` (replacing any file there) only once its digest equals
/// `sha256`. A failure leaves `dest` as it was and removes the part file, so
/// `dest` never holds a partial or unverified download. It waits up to
/// 30 s to connect, for the response headers and for each read of the body,
/// with no limit on the whole download. A body of 64 `MiB` or more reports
/// its progress on stderr every 10 s.
///
/// # Errors
///
/// - [`BenchError::Download`] — the request failed, the server answered with a
///   4xx or 5xx status, or the connection stalled or dropped before or during
///   the body.
/// - [`BenchError::UnusableDownload`] — the server answered with a success
///   status other than `200 OK` (naming a WAF challenge) or with an empty body.
/// - [`BenchError::Integrity`] — the body's digest differs from `sha256`; its
///   `subject` is `file '<name>'`, where `<name>` is `dest`'s file name.
/// - [`BenchError::Io`] — `dest`'s directory or its part file cannot be created
///   or written, or the part file cannot be renamed onto `dest`.
pub fn download_verified(url: &str, dest: &Path, sha256: &str) -> Result<(), BenchError> {
    fetch_checked(url, dest, |_part, actual| {
        if actual == sha256 {
            Ok(())
        } else {
            Err(BenchError::Integrity {
                subject: format!("file '{}'", file_name(dest)),
                location: url.to_string(),
                expected: sha256.to_string(),
                actual,
            })
        }
    })
}

/// `path`'s file name for a message, or the whole path if it has none.
fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Whether a download that has received `received` bytes reports its
/// progress: a body whose length `total` is known to be at least
/// [`PROGRESS_MIN_BYTES`], or one of unknown length once that many bytes have
/// arrived.
fn reports_progress(total: Option<u64>, received: u64) -> bool {
    total.unwrap_or(received) >= PROGRESS_MIN_BYTES
}

/// One progress line, in whole `MiB`.
fn progress_line(url: &str, received: u64, total: Option<u64>) -> String {
    let mib = received >> 20;
    match total {
        | Some(total) => format!("  downloading {url}: {mib} of {} MiB", total >> 20),
        | None => format!("  downloading {url}: {mib} MiB so far"),
    }
}

/// Prints a large download's progress on stderr, at most once per
/// [`PROGRESS_INTERVAL`].
struct Progress<'a> {
    /// The URL being fetched.
    url: &'a str,
    /// The body's length, when the response gives one.
    total: Option<u64>,
    /// When the last line was printed, or the download started.
    last: Instant,
}

impl<'a> Progress<'a> {
    fn new(url: &'a str, total: Option<u64>) -> Self {
        Self {
            url,
            total,
            last: Instant::now(),
        }
    }

    /// The progress line due at `now`, if any: once the body is large enough
    /// ([`reports_progress`]) and [`PROGRESS_INTERVAL`] has passed since the
    /// last line, or since the start. Returning a line restarts the interval.
    fn tick(&mut self, received: u64, now: Instant) -> Option<String> {
        if !reports_progress(self.total, received)
            || now.saturating_duration_since(self.last) < PROGRESS_INTERVAL
        {
            return None;
        }
        self.last = now;
        Some(progress_line(self.url, received, self.total))
    }

    fn update(&mut self, received: u64) {
        if let Some(line) = self.tick(received, Instant::now()) {
            eprintln!("{line}");
        }
    }
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

    /// Writes `bytes` to a fresh temp file.
    fn temp_file(bytes: &[u8]) -> tempfile::NamedTempFile {
        let f = tempfile::NamedTempFile::new().unwrap();
        fs::write(f.path(), bytes).unwrap();
        f
    }

    #[test]
    fn a_complete_parquet_file_passes_the_cached_file_check() {
        let f = temp_file(b"PAR1 row groups and footer PAR1");
        assert_eq!(cached_file_problem(f.path()).unwrap(), None);
    }

    /// A zero-byte file is what a WAF challenge left at the cache path
    /// before #70; any file shorter than both markers cannot be Parquet.
    #[test]
    fn an_empty_or_tiny_file_fails_the_cached_file_check() {
        for bytes in [&b""[..], b"PAR1PAR"] {
            let reason = cached_file_problem(temp_file(bytes).path()).unwrap();
            assert!(reason.is_some_and(|r| r.contains("bytes")), "{bytes:?}");
        }
    }

    /// A file cut off before its footer still starts with `PAR1` but
    /// does not end with it.
    #[test]
    fn a_truncated_file_fails_the_cached_file_check() {
        let reason = cached_file_problem(temp_file(b"PAR1 row groups, no footer").path()).unwrap();
        assert!(
            reason.as_deref().is_some_and(|r| r.contains("PAR1")),
            "{reason:?}"
        );
    }

    #[test]
    fn a_file_that_is_not_parquet_fails_the_cached_file_check() {
        let reason = cached_file_problem(temp_file(b"<html>challenge</html>").path()).unwrap();
        assert!(
            reason.as_deref().is_some_and(|r| r.contains("PAR1")),
            "{reason:?}"
        );
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
        serve_with(move |stream| {
            use std::io::Write;
            stream.write_all(&response).unwrap();
        })
    }

    /// Accepts the first connection on a loopback port, reads its request
    /// head, hands the stream to `respond`, and returns the URL to fetch.
    fn serve_with(respond: impl FnOnce(&mut std::net::TcpStream) + Send + 'static) -> String {
        use std::io::Read;

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
            respond(&mut stream);
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
        assert!(!part_path(&dest).exists());
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
        assert!(msg.contains("got 202"), "{msg}");
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
                subject,
                expected,
                actual,
                ..
            } => {
                assert_eq!(subject, "relation 'edge'");
                assert_eq!(expected, bad);
                assert_eq!(
                    actual,
                    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                );
            },
            | other => panic!("expected Integrity, got {other:?}"),
        }
    }

    /// Serves a `200 OK` with `body` to the first connection, writing the
    /// body in `chunks` pieces with `gap` before each, and returns its URL.
    fn serve_trickle(body: Vec<u8>, chunks: usize, gap: Duration) -> String {
        serve_with(move |stream| {
            use std::io::Write;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            for piece in body.chunks(body.len().div_ceil(chunks)) {
                std::thread::sleep(gap);
                if stream
                    .write_all(piece)
                    .and_then(|()| stream.flush())
                    .is_err()
                {
                    return;
                }
            }
        })
    }

    /// A client whose per-operation timeout is short enough to test against.
    fn client_with_timeout(timeout: Duration) -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap()
    }

    #[test]
    fn part_path_appends_to_the_file_name() {
        assert_eq!(
            part_path(Path::new("/c/edge.parquet")),
            Path::new("/c/edge.parquet.part")
        );
        assert_eq!(
            part_path(Path::new("raw/com-amazon.ungraph.txt.gz")),
            Path::new("raw/com-amazon.ungraph.txt.gz.part")
        );
    }

    #[test]
    fn download_file_reports_a_digest_mismatch_as_its_relation() {
        use sha2::{Digest, Sha256};

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("edge.parquet");
        let url = serve_once(http_response("200 OK", &[], PARQUET_LIKE));
        let mut rel = url_relation(&url);
        rel.sha256 = Some("0".repeat(64));
        match download_file(&url, &dest, &rel).unwrap_err() {
            | BenchError::Integrity {
                subject,
                expected,
                actual,
                ..
            } => {
                assert_eq!(subject, "relation 'edge'");
                assert_eq!(expected, "0".repeat(64));
                assert_eq!(
                    actual,
                    crate::definition::hex_digest(&Sha256::digest(PARQUET_LIKE))
                );
            },
            | other => panic!("expected Integrity, got {other:?}"),
        }
        assert!(!dest.exists());
        assert!(!part_path(&dest).exists());
    }

    #[test]
    fn download_file_accepts_a_matching_digest() {
        use sha2::{Digest, Sha256};

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("edge.parquet");
        let url = serve_once(http_response("200 OK", &[], PARQUET_LIKE));
        let mut rel = url_relation(&url);
        rel.sha256 = Some(crate::definition::hex_digest(&Sha256::digest(PARQUET_LIKE)));
        download_file(&url, &dest, &rel).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), PARQUET_LIKE);
        assert!(!part_path(&dest).exists());
    }

    /// reqwest's blocking timeout bounds each read, not the whole body: a body
    /// that keeps arriving in gaps shorter than the timeout succeeds although
    /// it takes longer than the timeout in total. This is the semantics
    /// `DOWNLOAD_TIMEOUT` relies on, pinned against a reqwest upgrade.
    #[test]
    fn a_body_that_keeps_arriving_has_no_total_timeout() {
        use sha2::{Digest, Sha256};

        let body = vec![7u8; 6 * 1024];
        let url = serve_trickle(body.clone(), 6, Duration::from_millis(250));
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("slow.bin.part");
        let started = std::time::Instant::now();
        let sha256 =
            stream_to_part(&client_with_timeout(Duration::from_secs(1)), &url, &part).unwrap();
        assert!(
            started.elapsed() > Duration::from_secs(1),
            "the body must take longer than the timeout"
        );
        assert_eq!(fs::read(&part).unwrap(), body);
        assert_eq!(
            sha256,
            crate::definition::hex_digest(&Sha256::digest(&body))
        );
    }

    /// A connection that stalls mid-body for longer than the timeout fails as
    /// a download error, as the same stall before the body would.
    #[test]
    fn a_body_that_stalls_fails_as_a_download_error() {
        let url = serve_with(|stream| {
            use std::io::Write;
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nPAR1",
            );
            let _ = stream.flush();
            std::thread::sleep(Duration::from_secs(3));
            let _ = stream.write_all(b"PAR1");
        });
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("stalled.part");
        let err =
            stream_to_part(&client_with_timeout(Duration::from_secs(1)), &url, &part).unwrap_err();
        assert!(matches!(err, BenchError::Download { .. }), "{err:?}");
        assert_eq!(
            fs::read(&part).unwrap(),
            b"PAR1",
            "the stall must come mid-body, after the first read"
        );
    }

    #[test]
    fn a_body_read_error_not_raised_by_reqwest_stays_io() {
        let err = body_read_error("http://h/x", io::Error::other("disk full"));
        assert!(matches!(err, BenchError::Io(_)), "{err:?}");
    }

    /// A rename that fails (here `dest` is a non-empty directory) still
    /// removes the part file.
    #[test]
    fn a_failed_rename_still_removes_the_part_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("edge.parquet");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("occupant"), b"x").unwrap();
        let url = serve_once(http_response("200 OK", &[], PARQUET_LIKE));
        let err = download_file(&url, &dest, &url_relation(&url)).unwrap_err();
        assert!(matches!(err, BenchError::Io(_)), "{err:?}");
        assert!(!part_path(&dest).exists());
    }

    /// SHA-256 of `b"abc"`.
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// `b"abc"` is not Parquet, so this also shows `download_verified` makes
    /// no format check.
    #[test]
    fn download_verified_writes_a_file_whose_digest_matches() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("raw/graph.txt.gz");
        let url = serve_once(http_response("200 OK", &[], b"abc"));
        download_verified(&url, &dest, ABC_SHA256).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"abc");
        assert!(!part_path(&dest).exists());
    }

    #[test]
    fn download_verified_rejects_a_digest_mismatch_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("graph.txt.gz");
        let url = serve_once(http_response("200 OK", &[], b"abc"));
        let bad = "0".repeat(64);
        match download_verified(&url, &dest, &bad).unwrap_err() {
            | BenchError::Integrity {
                subject,
                location,
                expected,
                actual,
            } => {
                assert_eq!(subject, "file 'graph.txt.gz'");
                assert_eq!(location, url);
                assert_eq!(expected, bad);
                assert_eq!(actual, ABC_SHA256);
            },
            | other => panic!("expected Integrity, got {other:?}"),
        }
        assert!(!dest.exists());
        assert!(!part_path(&dest).exists());
    }

    /// Fetches `response` through `download_verified` into a fresh temp path
    /// and returns the error text, asserting that neither the file nor its
    /// `.part` was left behind.
    fn rejected_verified(response: Vec<u8>) -> String {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("graph.txt.gz");
        let url = serve_once(response);
        let err = download_verified(&url, &dest, ABC_SHA256)
            .expect_err("download should have been rejected");
        assert!(!dest.exists());
        assert!(!part_path(&dest).exists());
        err.to_string()
    }

    #[test]
    fn download_verified_rejects_a_challenge_a_non_200_and_an_empty_body() {
        let waf = rejected_verified(http_response(
            "202 Accepted",
            &["x-amzn-waf-action: challenge"],
            b"",
        ));
        assert!(waf.contains("challenge"), "{waf}");
        let accepted = rejected_verified(http_response("202 Accepted", &[], b"abc"));
        assert!(accepted.contains("got 202"), "{accepted}");
        let empty = rejected_verified(http_response("200 OK", &[], b""));
        assert!(empty.contains("empty"), "{empty}");
    }

    #[test]
    fn progress_is_reported_only_for_large_bodies() {
        let min = PROGRESS_MIN_BYTES;
        assert!(reports_progress(Some(min), 0));
        assert!(!reports_progress(Some(min - 1), min - 1));
        assert!(!reports_progress(None, min - 1));
        assert!(reports_progress(None, min));
    }

    #[test]
    fn progress_lines_name_the_total_when_known() {
        assert_eq!(
            progress_line("http://h/x.tgz", 3 << 20, Some(10 << 20)),
            "  downloading http://h/x.tgz: 3 of 10 MiB"
        );
        assert_eq!(
            progress_line("http://h/x.tgz", 3 << 20, None),
            "  downloading http://h/x.tgz: 3 MiB so far"
        );
    }

    #[test]
    fn progress_prints_at_most_once_per_interval() {
        let url = "http://h/x.tgz";
        let mut progress = Progress::new(url, Some(PROGRESS_MIN_BYTES));
        let start = progress.last;
        let due = start + PROGRESS_INTERVAL;
        assert_eq!(
            progress.tick(1 << 20, start),
            None,
            "not before an interval"
        );
        assert_eq!(progress.tick(1 << 20, due - Duration::from_nanos(1)), None);
        assert_eq!(
            progress.tick(1 << 20, due).as_deref(),
            Some("  downloading http://h/x.tgz: 1 of 64 MiB")
        );
        assert_eq!(
            progress.tick(2 << 20, due),
            None,
            "a line restarts the interval"
        );
        let mut small = Progress::new(url, Some(PROGRESS_MIN_BYTES - 1));
        let later = small.last + PROGRESS_INTERVAL;
        assert_eq!(small.tick(1, later), None, "a small body stays silent");
    }
}
