# SNAP Phase 1: `download_verified` (#96) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** kermit-bench gains `download_verified(url, dest, sha256)`, a streaming, digest-checked download for any file pinned by its SHA-256 (a SNAP graph's `.gz`, #96; JOB's IMDb dump and query files, #97). The relation downloads of `ensure_cached` move onto the same streaming core, with their behaviour and tests unchanged.

**Architecture:**
- **Error messages first (T1).** `BenchError::Integrity`'s `relation` field becomes a `subject` noun phrase (`relation 'edge'`, `file 'x.gz'`), with relation messages byte-identical. `BenchError::UnusableDownload` stops claiming every download is Parquet.
- **One streaming core (T2).** `fetch_checked` stages a body in `<dest>.part` through `stream_to_part` (GET, status check, streamed `Read` with hashing, empty-body check), runs a caller's check on the finished part file, then renames it. Every failure removes `.part`. The relation path (`download_file`) becomes `fetch_checked` plus `check_relation_part` (the Parquet magic, then the optional digest). reqwest's blocking `timeout` bounds each read, not the whole body, so streaming makes 30 s a stall limit. A loopback test pins that.
- **The public function (T3).** `download_verified` is `fetch_checked` with a required digest, plus progress lines on stderr for bodies of 64 MiB or more.
- **Docs (T4)** and **the gate (T5).**
- **Untouched:** everything outside `kermit-bench` except one `CLAUDE.md` gotcha. No `GeneratorSpec` change and nothing in the binary (spec § Sequencing: phase 1 must build on its own).

**Tech Stack:** Rust nightly (workspace `rust-toolchain.toml`, edition 2021, so no let-chains), the nix dev shell for every cargo command and for rustfmt, reqwest 0.13 blocking with rustls, sha2 0.11, tempfile (dev).

**Spec:** `docs/specs/2026-10-15-snap-benchmark-design.md`, § The download, § Testing → Phase 1, § Sequencing (committed at 02bcd0c). Read § The download first. Its "Client and timeouts" bullet records the reqwest semantics this plan relies on, read from `reqwest-0.13.5/src/blocking/{client,response}.rs`.

---

## Ground rules for the executor

- **Paths.** `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/snap_18dea46d17ed9e48`.
  - Every command runs from `$WT`.
  - `SCRATCH` is the executing session's scratchpad.
  - `BASE=02bcd0c`, the spec commit this plan starts from. The scope check diffs against it.
- **Branch:** work on the loom branch `aidanb/snap`. Never switch branches, never amend, never push. Landing on master is a separate decision for the user.
- **Before Task 1:** this plan is new and uncommitted. Unless the user has already committed it, commit it first, by name:
  - `git add docs/superpowers/plans/2026-10-15-snap-phase-1-download.md`;
  - message `docs: plan #96 phase 1, download_verified`, with the two trailer lines below.
- **Host (two checks, both required):**
  1. **No cargo command until the benchmark co-ordinator has sent `FULL_EXIT`.** The #101 B1+B2 replicate chain (`kermit-bench-runs/hash-trie-chain-links-2026-10-14`) is timing on this host until about 16:00–17:30 on 2026-10-15. Edits and commits are fine before then, but no `cargo build`, `test`, `clippy`, `doc` or `fmt`.
  2. Before every compile or test run, check that no timing run is active:
     ```bash
     find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
     ```
     It must print `0`. If not, wait: a compile under a timing run contaminates it.

  If the co-ordinator hasn't sent `FULL_EXIT` yet, write each task's code and commit it with the message "(untested)" in the body. Then run every task's test steps, in order, once it arrives. Fix any failure in a new commit; never amend.
- **Builds:** every cargo command runs inside the dev shell with two jobs, in the foreground:
  - `nix develop --command env CARGO_BUILD_JOBS=2 cargo …`;
  - set `RUSTFLAGS` / `RUSTDOCFLAGS` inside the `env`, never outside `nix develop`.
- **Formatting:** `nix develop --command cargo fmt --all` only. Stable rustfmt rewrites ~30 files. `rustfmt.toml` sets `match_arm_leading_pipes = "Always"` (every arm starts with `| `), `imports_granularity = "One"` (one `use { … }` block per module), `max_width = 100` and `wrap_comments` at 80 columns. The code below follows them; accept whatever `cargo fmt` re-wraps.
- **Doc comments:** backtick every code identifier and code-like word (`PAR1`, `.part`, `MiB`, `x-amzn-waf-action`, `Read`). CLAUDE.md requires it for clippy's `doc_markdown`.
- **No `unsafe`.**
- **Commits.** Conventional commits referencing `(#96)`. Stage files by name. Every message ends with these two lines:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01GVMQgHfjGHYeJpbR8BZsQ9
  ```
- **Test commands** name the test by its path under the lib target:
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench --lib <module>::tests::<name> -- --exact
  ```
- **Mutation checks.** Each runs in the task that can first see it fail, *after* that task's commit, so `git checkout -- <file>` restores the committed fix. Assert the mutant applied (the `grep -c` must print the stated count) before running the test:

  | # | Mutant | Must fail | Run in |
  | --- | --- | --- | --- |
  | 1 | `body_read_error` always returns `BenchError::Io(err)` | `cache::tests::a_body_that_stalls_fails_as_a_download_error` | T2 |
  | 2 | `fetch_checked` never removes the part file | `cache::tests::download_file_reports_a_digest_mismatch_as_its_relation` | T2 |
  | 3 | `check_relation_part` skips the Parquet check | `cache::tests::download_rejects_a_200_body_that_is_not_parquet` | T2 |
  | 4 | `download_verified` accepts any digest | `cache::tests::download_verified_rejects_a_digest_mismatch_and_leaves_nothing` | T3 |
  | 5 | `reports_progress` ignores a known total | `cache::tests::progress_is_reported_only_for_large_bodies` | T3 |

---

## File structure

| File | Change | Responsibility after the change |
| --- | --- | --- |
| `kermit-bench/src/error.rs` | Modify | `Integrity { subject, … }`; a general `UnusableDownload` message; a new `#[cfg(test)]` module pinning both messages |
| `kermit-bench/src/cache.rs` | Modify | The streaming core (`http_client`, `part_path`, `fetch_checked`, `stream_to_part`, `unusable_status`, `body_read_error`), the relation path (`check_relation_part`, `download_file`), `download_verified` and its progress (`reports_progress`, `progress_line`, `Progress`), and their tests |
| `CLAUDE.md` | Modify | One Gotchas bullet on downloads |

`cache.rs` stays one file, because the download code is ~200 lines beside the cache-path code it serves and the existing loopback test helpers. Splitting it would be a restructure the spec doesn't ask for.

---

### Task 1: The error messages

**Files:**
- Modify: `kermit-bench/src/error.rs` (the `UnusableDownload` and `Integrity` variants; append a test module)
- Modify: `kermit-bench/src/cache.rs` (`verify_integrity`'s and `download_file`'s `Integrity` constructions; the test `verify_integrity_reports_a_mismatch`)

- [ ] **Step 1: Write the failing tests**

Append to `kermit-bench/src/error.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::BenchError;

    /// A relation's message is byte-identical to the one before `subject`
    /// replaced `relation`, which `cli_bench_fetch_integrity.rs` asserts.
    #[test]
    fn integrity_names_its_subject_verbatim() {
        let err = BenchError::Integrity {
            subject: "relation 'edge'".to_string(),
            location: "/x".to_string(),
            expected: "aa".to_string(),
            actual: "bb".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "integrity check failed for relation 'edge' from /x: expected sha256 aa, got bb"
        );
    }

    #[test]
    fn unusable_download_does_not_presume_parquet() {
        let err = BenchError::UnusableDownload {
            url: "http://h/x.gz".to_string(),
            reason: "the response body is empty".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "download from http://h/x.gz is unusable: the response body is empty"
        );
    }
}
```

In `kermit-bench/src/cache.rs`, in `verify_integrity_reports_a_mismatch`, replace

```rust
            | BenchError::Integrity {
                relation,
                expected,
                actual,
                ..
            } => {
                assert_eq!(relation, "edge");
```

with

```rust
            | BenchError::Integrity {
                subject,
                expected,
                actual,
                ..
            } => {
                assert_eq!(subject, "relation 'edge'");
```

- [ ] **Step 2: Run the tests to verify they fail** (after `FULL_EXIT`; see Ground rules)

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench --lib`
Expected: a compile error, `variant BenchError::Integrity does not have a field named subject` (E0026 / E0559).

- [ ] **Step 3: Implement**

In `kermit-bench/src/error.rs`, replace the `UnusableDownload` doc comment and attribute:

```rust
    /// A download succeeded at the HTTP level but did not deliver a relation
    /// file: a status other than `200 OK`, an empty body, or bytes without the
    /// Parquet magic. Nothing is written to the cache.
    #[error("download from {url} did not return a Parquet file: {reason}")]
    UnusableDownload {
```

with

```rust
    /// A download succeeded at the HTTP level but did not deliver the file: a
    /// status other than `200 OK`, an empty body, or, for a relation, bytes
    /// without the Parquet magic. Nothing is left at the destination.
    #[error("download from {url} is unusable: {reason}")]
    UnusableDownload {
```

and replace the whole `Integrity` variant (its doc comment, attribute and fields):

```rust
    /// A relation file's contents do not match the `sha256` its benchmark
    /// declares.
    #[error(
        "integrity check failed for relation '{relation}' from {location}: expected sha256 \
         {expected}, got {actual}"
    )]
    Integrity {
        /// Relation name as declared in the YAML.
        relation: String,
        /// The URL it was downloaded from, or the local path that was hashed.
        location: String,
        /// Digest declared in the YAML.
        expected: String,
        /// Digest computed from the bytes.
        actual: String,
    },
```

with

```rust
    /// A file's contents do not match the `sha256` declared for it: a
    /// benchmark relation's, or a raw file a generator pins by its digest.
    #[error(
        "integrity check failed for {subject} from {location}: expected sha256 {expected}, got \
         {actual}"
    )]
    Integrity {
        /// What was checked, as a noun phrase: `relation 'edge'` for a
        /// benchmark relation, `file 'com-amazon.ungraph.txt.gz'` for a raw
        /// download.
        subject: String,
        /// The URL it was downloaded from, or the local path that was hashed.
        location: String,
        /// Digest declared for it.
        expected: String,
        /// Digest computed from the bytes.
        actual: String,
    },
```

In `kermit-bench/src/cache.rs`, `verify_integrity` constructs the error with `relation: rel.name.clone(),`. Replace that line with:

```rust
                subject: format!("relation '{}'", rel.name),
```

In `download_file` (rewritten in Task 2; this keeps T1 compiling), replace `relation: rel.name.clone(),` with the same line.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench --lib`
Expected: PASS, including `error::tests::integrity_names_its_subject_verbatim`, `error::tests::unusable_download_does_not_presume_parquet` and `cache::tests::verify_integrity_reports_a_mismatch`.

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_bench_fetch_integrity`
Expected: PASS. It asserts `integrity check failed for relation 'parentcountry'`, which must be unchanged.

- [ ] **Step 5: Check nothing else names the old field**

Run: `grep -rn "Integrity {" --include='*.rs' . | grep -v '^./target'`
Expected: only `kermit-bench/src/error.rs`, `kermit-bench/src/cache.rs` (constructions and tests). If any other file appears, it destructures `relation`. Update it to `subject` the same way, and list it in the commit body.

- [ ] **Step 6: Commit**

```bash
git add kermit-bench/src/error.rs kermit-bench/src/cache.rs
git commit -m "refactor(kermit-bench): Integrity names its subject; UnusableDownload drops Parquet (#96)" -m "Integrity's relation field becomes a subject noun phrase, so a raw download can name its file; relation messages are byte-identical. UnusableDownload's message no longer claims every download is Parquet." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01GVMQgHfjGHYeJpbR8BZsQ9"
```

---

### Task 2: The streaming core, and the relation path on it

**Files:**
- Modify: `kermit-bench/src/cache.rs`:
  - the top `use` block;
  - delete `check_digest` and its test `check_digest_accepts_match_and_reports_mismatch`;
  - replace everything from the doc comment `/// Explains why a response that passed `error_for_status`` through the end of `fn download_file`;
  - `cached_file_problem`'s doc comment;
  - tests: refactor `serve_once` into `serve_with`, and add five tests.

- [ ] **Step 1: Write the failing tests**

In `kermit-bench/src/cache.rs`'s test module, replace `serve_once` (its doc comment and body) with these two functions:

```rust
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
```

Then append these helpers and tests at the end of the test module:

```rust
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
                if stream.write_all(piece).and_then(|()| stream.flush()).is_err() {
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
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("edge.parquet");
        let url = serve_once(http_response("200 OK", &[], PARQUET_LIKE));
        let mut rel = url_relation(&url);
        rel.sha256 = Some("0".repeat(64));
        match download_file(&url, &dest, &rel).unwrap_err() {
            | BenchError::Integrity {
                subject, expected, ..
            } => {
                assert_eq!(subject, "relation 'edge'");
                assert_eq!(expected, "0".repeat(64));
            },
            | other => panic!("expected Integrity, got {other:?}"),
        }
        assert!(!dest.exists());
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
        let err = stream_to_part(&client_with_timeout(Duration::from_millis(500)), &url, &part)
            .unwrap_err();
        assert!(matches!(err, BenchError::Download { .. }), "{err:?}");
    }

    #[test]
    fn a_body_read_error_not_raised_by_reqwest_stays_io() {
        let err = body_read_error("http://h/x", io::Error::other("disk full"));
        assert!(matches!(err, BenchError::Io(_)), "{err:?}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail** (after `FULL_EXIT`)

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench --lib`
Expected: compile errors, `cannot find function part_path`, `stream_to_part` and `body_read_error` in this scope (E0425), and `cannot find type Duration` (E0412).

- [ ] **Step 3: Implement**

Replace the top `use` block of `kermit-bench/src/cache.rs` with:

```rust
use {
    crate::{
        definition::{BenchmarkDefinition, RelationSource},
        error::BenchError,
    },
    std::{
        fs, io,
        path::{Path, PathBuf},
        time::Duration,
    },
};
```

Delete `check_digest` (its doc comment `/// Compares `bytes` against a declared digest. …` and its body). Delete its test `check_digest_accepts_match_and_reports_mismatch`: the digest comparison it covered moves into `check_relation_part` and is covered by `download_file_reports_a_digest_mismatch_as_its_relation`.

In `cached_file_problem`'s doc comment, replace

```rust
/// Explains why the file at `path` cannot be a Parquet file, or returns
/// `None` if it may be one: the structural check [`unusable_download`]
/// applies to a download, `PAR1` at both ends, read from the file's first
/// and last four bytes so a large relation is not read whole.
```

with

```rust
/// Explains why the file at `path` cannot be a Parquet file, or returns
/// `None` if it may be one: `PAR1` at both ends, read from the file's first
/// and last four bytes so a large relation is not read whole. Applied to a
/// cached relation and, by [`check_relation_part`], to every relation
/// download before it reaches its cache path.
```

Replace everything from the doc comment `/// Explains why a response that passed `error_for_status` is still not a` through the closing brace of `fn download_file` with:

```rust
/// How long a download waits to connect, for the response headers, and for
/// each read of the body. reqwest's blocking client applies its `timeout` to
/// each of those operations, not to the whole body, so a download that keeps
/// arriving has no total limit and one that stalls this long fails.
/// `Response::bytes()` would wait on the whole body under one timeout, which
/// is why bodies are streamed through `Read` instead.
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
/// Every failure removes the part file, so a download leaves either a checked
/// file at `dest` or nothing (#70).
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
/// A response other than `200 OK` is rejected before its body is read
/// ([`unusable_status`]), and an empty body after it. The caller removes
/// `part` on failure.
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
    if !err
        .get_ref()
        .is_some_and(|inner| inner.is::<reqwest::Error>())
    {
        return BenchError::Io(err);
    }
    let source = err
        .into_inner()
        .and_then(|inner| inner.downcast::<reqwest::Error>().ok())
        .expect("checked above that the inner error is a reqwest::Error");
    BenchError::Download {
        url: url.to_string(),
        source: *source,
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
```

`ensure_cached`'s call `download_file(url, &path, rel)?;` is unchanged.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench --lib cache::tests`
Expected: PASS. The six existing download tests pass unchanged (`download_writes_a_parquet_body_served_with_200`, `download_rejects_a_waf_challenge_and_names_it`, `download_rejects_a_success_status_other_than_200`, `download_rejects_an_empty_200_body`, `download_rejects_a_200_body_that_is_not_parquet`, `download_rejects_a_parquet_body_cut_off_before_its_footer`), and so do the five new ones. The two timing tests take about 1.5 s and 0.5 s.

Run: `nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-bench --all-targets`
Expected: no warnings. If clippy flags an unused import or item, it is a leftover from the deleted `check_digest` or `unusable_download`; remove it.

- [ ] **Step 5: Commit**

```bash
git add kermit-bench/src/cache.rs
git commit -m "feat(kermit-bench): stream relation downloads through a checked core (#96)" -m "fetch_checked stages a body in <dest>.part through stream_to_part, which streams it through Read while hashing, runs the caller's check, and renames it only if the check passes; every failure removes .part. Relation downloads keep their Parquet and optional-digest checks through check_relation_part. Streaming makes reqwest's 30 s blocking timeout a stall limit instead of a cap on the whole body, which Response::bytes() imposed; a loopback test pins the per-read semantics. A failed body read stays a BenchError::Download." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01GVMQgHfjGHYeJpbR8BZsQ9"
```

- [ ] **Step 6: Mutation checks 1 to 3** (after the commit; each must make its named test FAIL, then be restored)

Mutant 1: `body_read_error` always returns `Io`. (As built: the review changed `body_read_error` to `match err.downcast::<reqwest::Error>()`; see As-built notes.)
```bash
sed -i 's/    match err.downcast::<reqwest::Error>() {/    match Err::<reqwest::Error, io::Error>(err) {/' kermit-bench/src/cache.rs
grep -c 'match Err::<reqwest::Error, io::Error>(err)' kermit-bench/src/cache.rs   # must print 1
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench --lib cache::tests::a_body_that_stalls_fails_as_a_download_error -- --exact
git checkout -- kermit-bench/src/cache.rs
```
Expected: the test FAILS (`Io(..)` is not `Download`).

Mutant 2: `fetch_checked` never removes the part file.
```bash
sed -i 's/        let _ = fs::remove_file(&part);/        let _ = ();/' kermit-bench/src/cache.rs
grep -c 'let _ = ();' kermit-bench/src/cache.rs   # must print 1
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench --lib cache::tests::download_file_reports_a_digest_mismatch_as_its_relation -- --exact
git checkout -- kermit-bench/src/cache.rs
```
Expected: the test FAILS at `assert!(!part_path(&dest).exists())`.

Mutant 3: `check_relation_part` skips the Parquet check.
```bash
sed -i 's/    if let Some(reason) = cached_file_problem(part)? {/    if let Some(reason) = None::<String> {/' kermit-bench/src/cache.rs
grep -c 'None::<String>' kermit-bench/src/cache.rs   # must print 1
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench --lib cache::tests::download_rejects_a_200_body_that_is_not_parquet -- --exact
git checkout -- kermit-bench/src/cache.rs
```
Expected: the test FAILS (the HTML body is accepted).

After the three: `git status --short kermit-bench` prints nothing.

---

### Task 3: `download_verified` and its progress

**Files:**
- Modify: `kermit-bench/src/cache.rs` (the `use` block; new items after `download_file`; one line each in `stream_to_part`; tests)

- [ ] **Step 1: Write the failing tests**

Append at the end of `kermit-bench/src/cache.rs`'s test module:

```rust
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
        assert!(accepted.contains("202"), "{accepted}");
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
```

- [ ] **Step 2: Run the tests to verify they fail** (after `FULL_EXIT`)

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench --lib`
Expected: compile errors, `cannot find function download_verified`, `reports_progress` and `progress_line`, and `cannot find value PROGRESS_MIN_BYTES` (E0425).

- [ ] **Step 3: Implement**

In the top `use` block, replace `time::Duration,` with `time::{Duration, Instant},`.

After `fn download_file`, add:

```rust
/// A body of at least this many bytes reports its progress on stderr.
const PROGRESS_MIN_BYTES: u64 = 64 << 20;

/// The least time between two progress lines.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);

/// Downloads `url` to `dest`, checked against `sha256` (64 lowercase hex
/// characters). For any file pinned by its digest, Parquet or not: a SNAP
/// graph's `.gz` (#96), JOB's IMDb dump and query files (#97).
///
/// The body is streamed into `<dest>.part` while it is hashed, and renamed
/// onto `dest` only once its digest equals `sha256`, so `dest` never holds a
/// partial or unverified file and a failure leaves neither. It waits up to
/// 30 s to connect, for the response headers and for each read of the body,
/// with no limit on the whole download. A body of 64 `MiB` or more reports
/// its progress on stderr every 10 s.
///
/// # Errors
///
/// - [`BenchError::Download`] — the request failed, or the connection stalled
///   or dropped before or during the body.
/// - [`BenchError::UnusableDownload`] — the server answered with a status
///   other than `200 OK` (naming a WAF challenge) or with an empty body.
/// - [`BenchError::Integrity`] — the body's digest differs from `sha256`; its
///   `subject` is `file '<name>'`, where `<name>` is `dest`'s file name.
/// - [`BenchError::Io`] — `dest`'s directory or its part file cannot be
///   created or written.
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
    match total {
        | Some(total) => {
            format!("  downloading {url}: {} of {} MiB", received >> 20, total >> 20)
        },
        | None => format!("  downloading {url}: {} MiB so far", received >> 20),
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

    fn update(&mut self, received: u64) {
        if reports_progress(self.total, received) && self.last.elapsed() >= PROGRESS_INTERVAL {
            self.last = Instant::now();
            eprintln!("{}", progress_line(self.url, received, self.total));
        }
    }
}
```

In `stream_to_part`, after the `unusable_status` check and before `let mut file = …`, add:

```rust
    let mut progress = Progress::new(url, response.content_length());
```

and in its loop, after `received += n as u64;`, add:

```rust
        progress.update(received);
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench --lib`
Expected: PASS, every `cache::tests` and `error::tests` test.

Run: `nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-bench --all-targets`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add kermit-bench/src/cache.rs
git commit -m "feat(kermit-bench): download_verified, a digest-pinned streaming download (#96)" -m "For any file pinned by its SHA-256, Parquet or not: SNAP's .gz (#96), JOB's IMDb dump and query files (#97). It runs the streaming core with a required digest; a mismatch is BenchError::Integrity naming file '<name>', and nothing is left behind. Bodies of 64 MiB or more report progress on stderr every 10 s." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01GVMQgHfjGHYeJpbR8BZsQ9"
```

- [ ] **Step 6: Mutation checks 4 and 5** (after the commit)

Mutant 4: `download_verified` accepts any digest.
```bash
sed -i 's/        if actual == sha256 {/        if actual == sha256 || true {/' kermit-bench/src/cache.rs
grep -c 'actual == sha256 || true' kermit-bench/src/cache.rs   # must print 1
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench --lib cache::tests::download_verified_rejects_a_digest_mismatch_and_leaves_nothing -- --exact
git checkout -- kermit-bench/src/cache.rs
```
Expected: the test FAILS (`unwrap_err` on `Ok`).

Mutant 5: `reports_progress` ignores a known total.
```bash
sed -i 's/    total.unwrap_or(received) >= PROGRESS_MIN_BYTES/    received >= PROGRESS_MIN_BYTES/' kermit-bench/src/cache.rs
grep -c '^    received >= PROGRESS_MIN_BYTES' kermit-bench/src/cache.rs   # must print 1
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench --lib cache::tests::progress_is_reported_only_for_large_bodies -- --exact
git checkout -- kermit-bench/src/cache.rs
```
Expected: the test FAILS at `assert!(reports_progress(Some(min), 0))`. (Clippy may warn about the unused `total` parameter under the mutant; this only runs the test.)

After both: `git status --short kermit-bench` prints nothing.

---

### Task 4: Docs

**Files:**
- Modify: `kermit-bench/src/cache.rs` (the module doc; `ensure_cached`'s `# Errors`)
- Modify: `CLAUDE.md` (Gotchas)

- [ ] **Step 1: The module doc**

In `kermit-bench/src/cache.rs`, replace the module doc's last paragraph

```rust
//! [`ensure_cached`] is the entry point; [`clean_benchmark`] and [`clean_all`]
//! remove cached files.
```

with

```rust
//! [`ensure_cached`] is the entry point; [`clean_benchmark`] and [`clean_all`]
//! remove cached files. [`download_verified`] downloads any other file pinned
//! by its digest, such as a generator's raw input, through the same streaming
//! core: a body is staged in a `.part` file while it is hashed and renamed
//! onto its destination only once it passes its checks.
```

- [ ] **Step 2: `ensure_cached`'s errors**

In `ensure_cached`'s `# Errors` list, replace

```rust
/// - [`BenchError::Download`] — an HTTP error occurred while fetching a
///   relation file.
```

with

```rust
/// - [`BenchError::Download`] — an HTTP error occurred while fetching a
///   relation file, or the connection stalled for 30 s before or during the
///   body.
```

- [ ] **Step 3: The CLAUDE.md gotcha**

In `CLAUDE.md`'s `## Gotchas`, after the bullet that starts `- **Relation integrity**:`, insert:

```markdown
- **Downloads stream; a digest-pinned one is `download_verified`**: `kermit_bench::cache::download_verified(url, dest, sha256)` streams a body into `<dest>.part` while hashing it and renames it onto `dest` only once the digest matches, for any file pinned by its digest (SNAP's `.gz`, #96; JOB's IMDb dump, #97). `ensure_cached`'s relation downloads share the same core (`fetch_checked`) and add the Parquet `PAR1` check and their optional digest. Never read a download with `Response::bytes()`: reqwest's blocking `timeout` (30 s here) bounds the header wait and each body read, so a streamed download fails only when it stalls, while `bytes()` waits on the whole body under one 30 s timeout. A failed download leaves neither `dest` nor `.part`; a stalled body is `BenchError::Download`, a digest mismatch `BenchError::Integrity`, whose `subject` is `relation '<name>'` or `file '<name>'`.
```

- [ ] **Step 4: Check the docs build** (after `FULL_EXIT`)

Run: `nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-bench --no-deps`
Expected: no warnings (the new intra-doc link `[`download_verified`]` resolves).

- [ ] **Step 5: Commit**

```bash
git add kermit-bench/src/cache.rs CLAUDE.md
git commit -m "docs: downloads stream, and download_verified pins a raw file (#96)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01GVMQgHfjGHYeJpbR8BZsQ9"
```

---

### Task 5: The gate

Run every step after `FULL_EXIT`, with the host check clear.

- [ ] **Step 1: Scope check**

Run: `git diff --stat 02bcd0c..HEAD -- . ':!docs/superpowers/plans' ':!docs/specs'`
Expected: exactly `CLAUDE.md`, `kermit-bench/src/cache.rs` and `kermit-bench/src/error.rs`. Anything else is out of scope for phase 1 (spec § Sequencing).

- [ ] **Step 2: Format**

Run: `nix develop --command cargo fmt --all -- --check`
Expected: no output. If it lists diffs, run `nix develop --command cargo fmt --all`. Check that only the three files above changed, then commit them as `style: cargo fmt (#96)` with the trailers.

- [ ] **Step 3: Clippy, workspace**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets`
Expected: no warnings.

- [ ] **Step 4: Docs, workspace**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`
Expected: no warnings.

- [ ] **Step 5: Tests**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench`
Expected: PASS.

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_bench_fetch_integrity --test cli_bench_corrupt_cache`
Expected: PASS (both assert the messages of errors this phase touches).

Run the whole workspace detached, because it takes minutes:
```bash
setsid nohup nix develop --command env CARGO_BUILD_JOBS=2 cargo test --workspace > "$SCRATCH/phase1-test.log" 2>&1 & disown
```
Watch it with `kill -0 <pid>` (not `pgrep` over `/proc`), then:
```bash
grep -E '^test result:|FAILED|panicked' "$SCRATCH/phase1-test.log" | sort | uniq -c
```
Expected: every `test result: ok`, no `FAILED`. If `e2e_watdiv` fails, rerun it alone before blaming this work: it is a known intermittent under the parallel run.

- [ ] **Step 6: Report**

Report the gate results, the five mutation outcomes, and the branch tip (`git log --oneline -1`) to the user. Landing on master (merge `origin/master`, re-gate, `git push origin HEAD:master`) waits for the user's go-ahead. After it lands, send the master SHA to the JOB session (`job-18dea468f74c13c3-e6`) and to the benchmark co-ordinator, as promised.

---

## As-built notes

Changes made during execution, each after a review, so that the code differs from this plan's blocks:

- **T1 (0c11e61), code-quality review:** three doc-comment rewordings in `kermit-bench/src/error.rs`. `Download` says "Downloading a file failed.", `Integrity`'s doc reads cleanly, and the message-pinning test states its rule instead of its history.
- **T2, code-quality review (fix commit after 23f14ea):**
  - `serve_trickle`'s write chain is split one call per line (`chain_width = 60`).
  - New test `download_file_accepts_a_matching_digest`. Deleting `check_digest`'s test had left the accept branch untested. The mismatch test also asserts `actual`.
  - `body_read_error` is `match err.downcast::<reqwest::Error>() { | Ok(source) => Download { .. }, | Err(err) => Io(err) }` (`io::Error::downcast`, stable since 1.79), with no `expect`. Mutant 1 targets that `match`.
  - The stall test asserts the part file holds `PAR1`, pinning the failure to mid-body.
  - `rejected_download` checks `part_path(&dest)`.
  - New test `a_failed_rename_still_removes_the_part_file`.
  - `DOWNLOAD_TIMEOUT`'s doc now says the request and its headers count as one window.
  - Not taken: a `sync_all` before the rename (durability across power loss); the old code didn't have one either.
- **T5 gate, toolchain (co-ordinator, 2026-10-15):** `flake.lock` pins rust-overlay at 2026-10-07, while CI floats nightly. Run T5's fmt, clippy and doc checks a second time against CI's current nightly, without writing the lock:
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all -- --check
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
  ```
  Never commit a `flake.lock` refresh from this branch without telling the co-ordinator first. The first of #95 and #96 to land on master commits it as its own `chore(nix): refresh rust-overlay`.
