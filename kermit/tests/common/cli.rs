//! Shared helpers for the CLI smoke tests (`cli_hash_trie_*.rs`). Each of
//! those files exercises a slice of the optimization-standard CLI surface
//! end-to-end: CLI parser -> `LayoutChoices`/`ConfigChoices` -> `run_ds_bench*`
//! dispatch -> `HasOptimizationAxes::optimization_axes()` -> `BenchReport.axes`
//! -> JSON on disk. This module centralises the process-spawning and
//! report-parsing boilerplate they all shared.

use {
    std::{
        path::PathBuf,
        process::{Command, Output},
    },
    tempfile::NamedTempFile,
};

/// Path to the `kermit` binary under test, built by Cargo for this test run.
pub fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

/// Path to the small `edge.csv` relation fixture used by the `bench ds`
/// smoke tests.
pub fn edge_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/edge.csv")
}

/// Runs `bench ds` against `edge.csv` with the space metric (cheapest of the
/// three to keep wall-clock minimal) and the given index structure,
/// appending `extra_args`. Returns the raw process output and the report
/// file so callers can assert on either success or failure.
pub fn bench_ds(indexstructure: &str, extra_args: &[&str]) -> (Output, NamedTempFile) {
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
        .arg(indexstructure)
        .arg("-m")
        .arg("space");
    for arg in extra_args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to execute kermit binary");
    (output, report)
}

/// Runs `bench run` against the given named benchmark, appending
/// `extra_args`. Returns the raw process output and the report file so
/// callers can assert on either success or failure. Callers pass `-m space`
/// themselves via `extra_args` when they want the cheap metric.
pub fn bench_run(benchmark: &str, extra_args: &[&str]) -> (Output, NamedTempFile) {
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
        .arg("run")
        .arg(benchmark);
    for arg in extra_args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to execute kermit binary");
    (output, report)
}

/// Parses a report file into the full `Vec<BenchReport>` JSON array.
pub fn reports_of(report: &NamedTempFile) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(report.path()).expect("report file should exist");
    serde_json::from_str(&text).expect("report should be valid JSON array")
}

/// Parses a report file and returns the `axes` object of its first report.
pub fn axes_of(report: &NamedTempFile) -> serde_json::Value {
    reports_of(report)[0]["axes"].clone()
}

/// Runs `bench join` over the `first.csv` / `second.csv` fixtures with
/// `intersect_query.dl`, appending `extra_args` after the structure and
/// algorithm. Returns the raw process output and the report file.
pub fn bench_join(
    indexstructure: &str, algorithm: &str, extra_args: &[&str],
) -> (Output, NamedTempFile) {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
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
        .arg("join")
        .arg("--relations")
        .arg(fixtures.join("first.csv"))
        .arg(fixtures.join("second.csv"))
        .arg("--query")
        .arg(fixtures.join("intersect_query.dl"))
        .arg("--indexstructure")
        .arg(indexstructure)
        .arg("--algorithm")
        .arg(algorithm);
    for arg in extra_args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to execute kermit binary");
    (output, report)
}
