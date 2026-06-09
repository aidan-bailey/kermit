//! CLI smoke test: `bench ds` with `--ds-layout-hasher fxhash` records the
//! optimization axis correctly. Also verifies that the default (no flag)
//! records `ds_layout_hasher: "sip"`.
//!
//! These tests exercise the end-to-end chain set up by Phases 1-5 of the
//! optimization-standard plan: CLI parser -> `LayoutChoices` ->
//! `run_ds_bench_hash::<H>` dispatch -> `HashTrie<H>` ->
//! `HasOptimizationAxes::optimization_axes()` -> `BenchReport.axes` ->
//! JSON on disk.

use {
    std::{path::PathBuf, process::Command},
    tempfile::NamedTempFile,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

fn edge_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/edge.csv")
}

/// Run `bench ds` against `edge.csv` with the hash-trie + space metric
/// (space is the cheapest of the three to keep wall-clock minimal) and
/// return the parsed JSON report array. `extra_ds_args` is appended after
/// the standard `--relation` / `--indexstructure` so callers can inject
/// `--ds-layout-hasher fxhash` (or omit it for the default).
fn run_bench_ds_hash(extra_ds_args: &[&str]) -> Vec<serde_json::Value> {
    let report = NamedTempFile::new().expect("failed to create temp report file");
    let mut cmd = Command::new(kermit_bin());
    cmd.arg("bench")
        .arg("--sample-size")
        .arg("10")
        .arg("--measurement-time")
        .arg("1")
        .arg("--warm-up-time")
        .arg("1")
        .arg("--report-json")
        .arg(report.path())
        .arg("ds")
        .arg("--relation")
        .arg(edge_fixture())
        .arg("--indexstructure")
        .arg("hash-trie")
        .arg("-m")
        .arg("space");
    for arg in extra_ds_args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to execute kermit binary");
    assert!(
        output.status.success(),
        "bench ds command failed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let text = std::fs::read_to_string(report.path()).expect("report file should exist");
    let reports: Vec<serde_json::Value> =
        serde_json::from_str(&text).expect("report should be valid JSON array");
    assert!(
        !reports.is_empty(),
        "bench-runs JSON was empty (expected at least one report)"
    );
    reports
}

#[test]
fn cli_bench_ds_with_fxhash_layout_records_axis() {
    let reports = run_bench_ds_hash(&["--ds-layout-hasher", "fxhash"]);
    assert_eq!(
        reports[0]["axes"]["ds_layout_hasher"], "fxhash",
        "ds_layout_hasher axis missing or wrong value: {}",
        reports[0]["axes"]
    );
}

#[test]
fn cli_bench_ds_default_hasher_is_sip() {
    let reports = run_bench_ds_hash(&[]);
    assert_eq!(
        reports[0]["axes"]["ds_layout_hasher"], "sip",
        "default ds_layout_hasher should be 'sip': {}",
        reports[0]["axes"]
    );
}
