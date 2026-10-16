//! CLI end-to-end for SNAP (#96): `bench gen snap --url … --sha256 …` and the
//! declarative `kind: snap` YAML, against a loopback HTTP server that serves
//! the gzipped mini fixture. No network is used.
//!
//! `XDG_CACHE_HOME` and `KERMIT_WORKSPACE` point at temp dirs, so the shared
//! cache and the real `benchmarks/` are never touched. Linux only, as the
//! other CLI tests that set `XDG_CACHE_HOME`.
//!
//! The server counts the connections it accepts, one per download, so the
//! tests can tell a cache hit (no download) from a regeneration (one more).

mod common;

use {
    common::snap::{fixture_text, gzip, MINI_COUNTS},
    kermit_bench::{cache::sha256_hex, BenchmarkDefinition},
    std::{
        collections::BTreeSet,
        fs,
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        path::{Path, PathBuf},
        process::{Command, Output},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    },
    tempfile::{NamedTempFile, TempDir},
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

fn skip_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) {
        eprintln!("skipping cli_snap_gen test: XDG_CACHE_HOME needs Linux");
        return true;
    }
    false
}

/// A loopback HTTP server that answers every connection with the same body.
struct Server {
    url: String,
    connections: Arc<AtomicUsize>,
}

impl Server {
    /// Binds a loopback port and serves `body` to any number of connections
    /// from detached threads that end with the test process.
    fn start(body: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/mini.ungraph.txt.gz",
            listener.local_addr().unwrap()
        );
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&connections);
        let body: Arc<[u8]> = body.into();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                counter.fetch_add(1, Ordering::SeqCst);
                let body = Arc::clone(&body);
                std::thread::spawn(move || respond(stream, &body));
            }
        });
        Self {
            url,
            connections,
        }
    }

    /// Connections accepted so far: the number of downloads.
    fn connections(&self) -> usize { self.connections.load(Ordering::SeqCst) }
}

/// Reads one request head, then answers `200 OK` with `body` and closes.
fn respond(mut stream: TcpStream, body: &[u8]) {
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buf) {
            | Ok(0) | Err(_) => return,
            | Ok(n) => request.extend_from_slice(&buf[..n]),
        }
    }
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

/// Three temp dirs: the cache root, the workspace and scratch space.
struct Sandbox {
    cache: TempDir,
    workspace: TempDir,
    work: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let sandbox = Self {
            cache: tempfile::tempdir().unwrap(),
            workspace: tempfile::tempdir().unwrap(),
            work: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(sandbox.workspace.path().join("benchmarks")).unwrap();
        sandbox
    }

    /// `<cache>/kermit/benchmarks/<name>`, where the binary keeps a benchmark.
    fn cache_subdir(&self, name: &str) -> PathBuf {
        self.cache.path().join("kermit/benchmarks").join(name)
    }

    /// A `kermit` command inside the sandbox.
    fn kermit(&self) -> Command {
        let mut command = Command::new(kermit_bin());
        command
            .env("XDG_CACHE_HOME", self.cache.path())
            .env("KERMIT_WORKSPACE", self.workspace.path())
            // Reach the loopback server directly, whatever proxy the host sets.
            .env("NO_PROXY", "127.0.0.1")
            .env("no_proxy", "127.0.0.1");
        command
    }

    /// Serves the gzipped fixture and returns the server with the digest of
    /// the bytes it serves.
    fn serve_mini(&self) -> (Server, String) {
        let gz = self.work.path().join("mini.ungraph.txt.gz");
        fs::write(&gz, gzip(&fixture_text())).unwrap();
        let sha256 = sha256_hex(&gz).unwrap();
        (Server::start(fs::read(&gz).unwrap()), sha256)
    }

    /// `bench gen snap --url <url> --sha256 <sha256> --tag t`.
    fn gen_snap(&self, url: &str, sha256: &str) -> Output {
        self.kermit()
            .args(["bench", "gen", "snap", "--url", url, "--sha256", sha256])
            .args(["--tag", "t"])
            .output()
            .unwrap()
    }

    /// `bench run <name> <cell…> --metrics iteration <extra…>` with the
    /// fastest Criterion settings, returning the output and the JSON report
    /// (empty when the run wrote none).
    fn bench_run(
        &self, name: &str, cell: &[&str], extra: &[&str],
    ) -> (Output, Vec<serde_json::Value>) {
        let report = NamedTempFile::new_in(self.work.path()).unwrap();
        let output = self
            .kermit()
            .args([
                "bench",
                "--sample-size",
                "10",
                "--measurement-time",
                "1",
                "--warm-up-time",
                "1",
            ])
            .arg("--report-json")
            .arg(report.path())
            .args(["run", name])
            .args(cell)
            .args(["--metrics", "iteration"])
            .args(extra)
            .output()
            .unwrap();
        let text = fs::read_to_string(report.path()).unwrap();
        let reports = if text.trim().is_empty() {
            vec![]
        } else {
            serde_json::from_str(&text).unwrap()
        };
        (output, reports)
    }

    /// Writes the workspace YAML `benchmarks/snap-mini.yml`: a `kind: snap`
    /// generator over `url` with the given `queries` list body.
    fn write_spec(&self, url: &str, sha256: &str, queries: &str) {
        let yaml = format!(
            "name: snap-mini\ndescription: declarative SNAP smoke test\ngenerator:\n  kind: \
             snap\n  url: {url}\n  sha256: {sha256}\n  queries: [{queries}]\n"
        );
        fs::write(self.workspace.path().join("benchmarks/snap-mini.yml"), yaml).unwrap();
    }
}

/// Everything a process printed.
fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// The queries and `expected` counts the cache subdir's `benchmark.yml` holds.
fn cached_counts(dir: &Path) -> Vec<(String, u64)> {
    let yaml = fs::read_to_string(dir.join("benchmark.yml")).unwrap();
    let bench: BenchmarkDefinition = serde_yaml::from_str(&yaml).unwrap();
    bench
        .queries
        .into_iter()
        .map(|q| (q.name, q.expected.expect("SNAP queries carry expected")))
        .collect()
}

/// The names in `dir`, sorted; empty when it does not exist.
fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn counts_of(pairs: &[(&str, u64)]) -> Vec<(String, u64)> {
    pairs
        .iter()
        .map(|&(name, count)| (name.to_string(), count))
        .collect()
}

/// `bench gen snap --url … --sha256 …` writes `snap-mini-t`, and `bench run`
/// verifies every query on all three cells against the generator's counts.
#[test]
fn bench_gen_snap_from_a_url_then_bench_run_verifies_every_cell() {
    if skip_unsupported() {
        return;
    }
    let sandbox = Sandbox::new();
    let (server, sha256) = sandbox.serve_mini();

    let generated = sandbox.gen_snap(&server.url, &sha256);
    let text = combined(&generated);
    assert!(generated.status.success(), "{text}");
    assert!(text.contains("snap benchmark written to"), "{text}");
    assert!(text.contains("snap-mini-t"), "{text}");
    assert_eq!(server.connections(), 1, "one download");

    let dir = sandbox.cache_subdir("snap-mini-t");
    for file in [
        "benchmark.yml",
        "meta.json",
        "edge.parquet",
        "raw/mini.ungraph.txt.gz",
    ] {
        assert!(
            dir.join(file).is_file(),
            "{file} missing from {}",
            dir.display()
        );
    }
    let meta = read_json(&dir.join("meta.json"));
    assert_eq!(meta["kind"], "snap");
    assert_eq!(meta["tag"], "t");
    assert_eq!(meta["source_url"], server.url.as_str());
    assert_eq!(meta["gz_sha256"], sha256.as_str());
    assert!(
        meta.get("spec_hash").is_none(),
        "an imperative run records no spec hash: {meta}"
    );
    assert_eq!(cached_counts(&dir), counts_of(&MINI_COUNTS));

    let cells = ["-i", "all", "-a", "all"];
    let (run, reports) = sandbox.bench_run("snap-mini-t", &cells, &["--verify"]);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "stderr: {stderr}");
    assert_eq!(
        reports.len(),
        12,
        "4 queries x the 3 valid cells; stderr: {stderr}"
    );
    for report in &reports {
        assert_eq!(report["axes"]["benchmark"], "snap-mini-t");
        assert_eq!(report["axes"]["verified"], true, "{report}");
    }
    let distinct = |key: &str| -> BTreeSet<String> {
        reports.iter().map(|r| r["axes"][key].to_string()).collect()
    };
    let structures = distinct("data_structure");
    assert_eq!(structures.len(), 3, "{structures:?}");
    let queries = distinct("query");
    assert_eq!(queries.len(), 4, "{queries:?}");
    assert_eq!(server.connections(), 1, "`bench run` must not download");
}

/// A wrong `--sha256` exits non-zero and leaves nothing a benchmark could be
/// read from: `prepare` created the directory, but there is no `meta.json`
/// (so no cache and no `bench list` entry), and `raw/` holds no `.gz` and no
/// `.part`.
#[test]
fn a_wrong_sha256_leaves_no_cache() {
    if skip_unsupported() {
        return;
    }
    let sandbox = Sandbox::new();
    let (server, real) = sandbox.serve_mini();
    let wrong = "0".repeat(64);
    assert_ne!(wrong, real);

    let output = sandbox.gen_snap(&server.url, &wrong);
    let text = combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("integrity check failed"), "{text}");
    assert!(
        text.contains(&wrong),
        "the declared digest is named: {text}"
    );
    assert!(
        text.contains(&real),
        "the digest actually served is named: {text}"
    );
    assert_eq!(server.connections(), 1);

    let dir = sandbox.cache_subdir("snap-mini-t");
    assert!(
        !dir.join("meta.json").exists(),
        "a failed run is not a cache"
    );
    assert!(!dir.join("benchmark.yml").exists());
    let raw = file_names(&dir.join("raw"));
    assert!(
        raw.iter()
            .all(|name| !name.ends_with(".gz") && !name.ends_with(".part")),
        "raw/ holds {raw:?}"
    );

    let list = sandbox.kermit().args(["bench", "list"]).output().unwrap();
    let listed = combined(&list);
    assert!(!listed.contains("snap-mini-t"), "{listed}");
    let cell = ["-i", "tree-trie", "-a", "leapfrog-triejoin"];
    let (run, _) = sandbox.bench_run("snap-mini-t", &cell, &[]);
    assert!(!run.status.success());
    assert!(
        combined(&run).contains("benchmark not found"),
        "{}",
        combined(&run)
    );
}

/// A workspace `kind: snap` YAML materialises on `bench run`, is a hit the
/// second time, raises `SpecDrift` after its `queries` are edited, and
/// regenerates under `--force`. #95's tests cover the general mechanism; this
/// pins SNAP's `dispatch` arm.
#[test]
fn a_snap_yaml_materialises_hits_drifts_and_regenerates() {
    if skip_unsupported() {
        return;
    }
    let sandbox = Sandbox::new();
    let (server, sha256) = sandbox.serve_mini();
    let cell = ["-i", "tree-trie", "-a", "leapfrog-triejoin"];
    let cache = sandbox.cache_subdir("snap-mini");
    sandbox.write_spec(&server.url, &sha256, "triangle, path3");

    // Miss: the pipeline runs once, and the cache holds exactly the two
    // queries the spec names, with the independently pinned counts.
    let (first, reports) = sandbox.bench_run("snap-mini", &cell, &["--verify"]);
    let stderr = String::from_utf8_lossy(&first.stderr);
    assert!(first.status.success(), "stderr: {stderr}");
    assert_eq!(reports.len(), 2, "stderr: {stderr}");
    assert!(reports.iter().all(|r| r["axes"]["verified"] == true));
    assert_eq!(server.connections(), 1);
    assert_eq!(
        cached_counts(&cache),
        counts_of(&[("triangle", 36), ("path3", 482)])
    );
    let meta_text = fs::read_to_string(cache.join("meta.json")).unwrap();
    let meta: serde_json::Value = serde_json::from_str(&meta_text).unwrap();
    assert_eq!(meta["kind"], "snap");
    let first_hash = meta["spec_hash"]
        .as_str()
        .expect("a declarative run records its spec hash")
        .to_string();

    // Hit: same spec, no download, `meta.json` untouched.
    let (second, reports) = sandbox.bench_run("snap-mini", &cell, &["--verify"]);
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(second.status.success(), "stderr: {stderr}");
    assert_eq!(reports.len(), 2);
    assert_eq!(server.connections(), 1, "a hit must not download again");
    assert_eq!(
        fs::read_to_string(cache.join("meta.json")).unwrap(),
        meta_text,
        "a hit must not regenerate"
    );

    // Drift: editing `queries` changes the spec hash. Without `--force` the
    // run refuses, downloads nothing and leaves the cache as it was.
    sandbox.write_spec(&server.url, &sha256, "triangle");
    let (drift, _) = sandbox.bench_run("snap-mini", &cell, &["--verify"]);
    let stderr = String::from_utf8_lossy(&drift.stderr);
    assert!(!drift.status.success(), "stderr: {stderr}");
    assert!(
        stderr.contains("spec drift for benchmark 'snap-mini'"),
        "{stderr}"
    );
    assert!(stderr.contains("--force"), "{stderr}");
    assert_eq!(server.connections(), 1);
    assert_eq!(
        cached_counts(&cache),
        counts_of(&[("triangle", 36), ("path3", 482)])
    );

    // `--force` wipes the subdirectory and regenerates with the new spec.
    let (forced, reports) = sandbox.bench_run("snap-mini", &cell, &["--verify", "--force"]);
    let stderr = String::from_utf8_lossy(&forced.stderr);
    assert!(forced.status.success(), "stderr: {stderr}");
    assert_eq!(reports.len(), 1, "stderr: {stderr}");
    assert_eq!(server.connections(), 2, "`--force` downloads again");
    assert_eq!(cached_counts(&cache), counts_of(&[("triangle", 36)]));
    let forced_meta = read_json(&cache.join("meta.json"));
    assert_ne!(
        forced_meta["spec_hash"].as_str().unwrap(),
        first_hash,
        "the regenerated cache records the new spec"
    );
}
