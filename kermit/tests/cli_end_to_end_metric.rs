//! CLI smoke tests for the `end-to-end` metric: `--metrics end-to-end`
//! times build + K joins in one Criterion body, with K set by
//! `--queries-per-build` (default 1) and recorded in the report's
//! `queries_per_build` axis.
//!
//! `bench run` is exercised for both trait families through the one
//! generic `run_benchmark<F: ExecutionFamily>` — (tree-trie,
//! leapfrog-triejoin) as `TrieLftj<R>` and (hash-trie, hash-triejoin) as
//! `HashHtj<H, P>` — against a fake benchmark cache built from the
//! committed watdiv-mini fixture, so no network access is needed. `bench
//! ds` is exercised directly against the `edge.csv` fixture.

use {
    std::{fs, path::PathBuf, process::Command},
    tempfile::{NamedTempFile, TempDir},
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

fn fixtures_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures") }

/// Benchmark name of the committed watdiv-mini fixture; doubles as the fake
/// cache subdir name (`ensure_cached` resolves relations under
/// `<cache>/<benchmark.name>/`).
const MINI_BENCH: &str = "watdiv-stress-mini-tiny";

/// The `bench run` tests redirect the benchmark cache via `XDG_CACHE_HOME`,
/// which `dirs::cache_dir()` honours only on Linux.
fn skip_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) {
        eprintln!("skipping cli_end_to_end_metric bench-run test: XDG_CACHE_HOME needs Linux");
        return true;
    }
    false
}

/// Builds a fake benchmark cache under `<tmp>/kermit/benchmarks/<MINI_BENCH>/`
/// from the committed watdiv-mini fixture: `benchmark.yml`, the `meta.json`
/// cache marker, and the three relation parquets. With every relation file
/// already present, `ensure_cached` never touches the network (the fixture's
/// `file:///` URLs would fail if it tried).
fn make_fake_cache() -> TempDir {
    let tmp = tempfile::tempdir().expect("failed to create temp cache dir");
    let bench_dir = tmp.path().join("kermit/benchmarks").join(MINI_BENCH);
    fs::create_dir_all(&bench_dir).expect("failed to create fake cache subdir");
    let artifacts = fixtures_dir().join("watdiv-mini/artifacts");
    fs::copy(
        artifacts.join(format!("{MINI_BENCH}.yml")),
        bench_dir.join("benchmark.yml"),
    )
    .expect("failed to copy fixture YAML");
    fs::write(bench_dir.join("meta.json"), "{}").expect("failed to write meta.json marker");
    for rel in ["eligibleregion", "includes", "parentcountry"] {
        fs::copy(
            artifacts.join(format!("{rel}.parquet")),
            bench_dir.join(format!("{rel}.parquet")),
        )
        .expect("failed to copy fixture parquet");
    }
    tmp
}

/// Runs `bench run <MINI_BENCH> -q q0002 --metrics end-to-end` for the given
/// (structure, algorithm) pair with `extra_args` appended, returning the
/// parsed report array.
fn run_bench_run_end_to_end(ds: &str, algo: &str, extra_args: &[&str]) -> Vec<serde_json::Value> {
    let cache = make_fake_cache();
    let report = NamedTempFile::new().expect("failed to create temp report file");
    let mut cmd = Command::new(kermit_bin());
    cmd.env("XDG_CACHE_HOME", cache.path());
    cmd.args([
        "bench",
        "--sample-size",
        "10",
        "--measurement-time",
        "1",
        "--warm-up-time",
        "1",
        "--report-json",
    ])
    .arg(report.path())
    .args([
        "run",
        MINI_BENCH,
        "-q",
        "q0002",
        "--indexstructure",
        ds,
        "--algorithm",
        algo,
        "--metrics",
        "end-to-end",
    ])
    .args(extra_args);
    let output = cmd.output().expect("failed to execute kermit binary");
    assert!(
        output.status.success(),
        "bench run failed for ({ds}, {algo}); stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let text = fs::read_to_string(report.path()).expect("report file should exist");
    let reports: Vec<serde_json::Value> =
        serde_json::from_str(&text).expect("report should be valid JSON array");
    assert!(!reports.is_empty(), "report array should not be empty");
    reports
}

/// Asserts the single-report shape shared by both families: exactly one
/// time-metric `end_to_end` criterion function and the expected
/// `queries_per_build` axis value.
fn assert_end_to_end_report(report: &serde_json::Value, expected_k: u64) {
    assert_eq!(
        report["axes"]["queries_per_build"],
        serde_json::json!(expected_k),
        "queries_per_build axis missing or wrong: {}",
        report["axes"]
    );
    let groups = report["criterion_groups"]
        .as_array()
        .expect("criterion_groups should be an array");
    assert_eq!(
        groups.len(),
        1,
        "--metrics end-to-end should run exactly one function: {groups:?}"
    );
    assert_eq!(
        groups[0]["function"], "end_to_end",
        "function id must be the bare phase token (K lives in the axes): {groups:?}"
    );
    assert_eq!(groups[0]["metric"], "time");
}

#[test]
fn cli_bench_run_end_to_end_sorted_family() {
    if skip_unsupported() {
        return;
    }
    let reports = run_bench_run_end_to_end("tree-trie", "leapfrog-triejoin", &[
        "--queries-per-build",
        "4",
    ]);
    assert_end_to_end_report(&reports[0], 4);
    assert_eq!(reports[0]["axes"]["data_structure"], "TreeTrie");
    assert_eq!(reports[0]["axes"]["algorithm"], "LeapfrogTriejoin");
}

#[test]
fn cli_bench_run_end_to_end_hash_family() {
    if skip_unsupported() {
        return;
    }
    // No --queries-per-build: the default must be K=1.
    let reports = run_bench_run_end_to_end("hash-trie", "hash-triejoin", &[]);
    assert_end_to_end_report(&reports[0], 1);
    assert_eq!(reports[0]["axes"]["data_structure"], "HashTrie");
    assert_eq!(reports[0]["axes"]["algorithm"], "HashTriejoin");
}

/// Runs `bench ds` against `edge.csv` with `--metrics end-to-end` for the
/// given structure, returning the parsed report array. No cache redirection
/// needed — `bench ds` reads the relation path directly.
fn run_bench_ds_end_to_end(ds: &str, extra_args: &[&str]) -> Vec<serde_json::Value> {
    let report = NamedTempFile::new().expect("failed to create temp report file");
    let mut cmd = Command::new(kermit_bin());
    cmd.args([
        "bench",
        "--sample-size",
        "10",
        "--measurement-time",
        "1",
        "--warm-up-time",
        "1",
        "--report-json",
    ])
    .arg(report.path())
    .args(["ds", "--relation"])
    .arg(fixtures_dir().join("edge.csv"))
    .args(["--indexstructure", ds, "-m", "end-to-end"])
    .args(extra_args);
    let output = cmd.output().expect("failed to execute kermit binary");
    assert!(
        output.status.success(),
        "bench ds failed for {ds}; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let text = fs::read_to_string(report.path()).expect("report file should exist");
    let reports: Vec<serde_json::Value> =
        serde_json::from_str(&text).expect("report should be valid JSON array");
    assert!(!reports.is_empty(), "report array should not be empty");
    reports
}

#[test]
fn cli_bench_ds_end_to_end_sorted_family() {
    let reports = run_bench_ds_end_to_end("tree-trie", &["--queries-per-build", "3"]);
    assert_eq!(
        reports[0]["axes"]["queries_per_build"],
        serde_json::json!(3),
        "queries_per_build axis missing or wrong: {}",
        reports[0]["axes"]
    );
    let groups = reports[0]["criterion_groups"].as_array().unwrap();
    assert_eq!(
        groups.len(),
        1,
        "expected only the end_to_end function: {groups:?}"
    );
    assert_eq!(
        groups[0]["function"], "TreeTrie/end_to_end",
        "bench ds prefixes the DS name; the id must still END with the phase token"
    );
    assert_eq!(groups[0]["metric"], "time");
}

#[test]
fn cli_bench_ds_end_to_end_hash_family() {
    let reports = run_bench_ds_end_to_end("hash-trie", &[]);
    assert_eq!(
        reports[0]["axes"]["queries_per_build"],
        serde_json::json!(1),
        "default queries_per_build should be 1: {}",
        reports[0]["axes"]
    );
    let groups = reports[0]["criterion_groups"].as_array().unwrap();
    assert_eq!(
        groups.len(),
        1,
        "expected only the end_to_end function: {groups:?}"
    );
    assert_eq!(groups[0]["function"], "HashTrie/end_to_end");
    assert_eq!(groups[0]["metric"], "time");
}
