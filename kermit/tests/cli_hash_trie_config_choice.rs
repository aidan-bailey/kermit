//! CLI smoke test: `bench ds` with `--ds-config singleton-pruning=true`
//! records the Config axis, and the flag is rejected where it cannot
//! apply. Mirrors `cli_hash_trie_hasher_choice.rs` for the Config category
//! of the optimization standard: CLI parser -> `ConfigChoices` ->
//! `run_ds_bench_hash::<H>` -> `HashTrie::from_tuples_with_config` ->
//! `HasOptimizationAxes::optimization_axes()` -> `BenchReport.axes` -> JSON.

use {
    std::{
        path::PathBuf,
        process::{Command, Output},
    },
    tempfile::NamedTempFile,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

fn edge_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/edge.csv")
}

/// Runs `bench ds` on `edge.csv` with the space metric (cheapest) and the
/// given index structure, appending `extra_args`. Returns the raw output
/// and the report path so callers can assert on either.
fn run_bench_ds(indexstructure: &str, extra_args: &[&str]) -> (Output, NamedTempFile) {
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

fn axes_of(report: &NamedTempFile) -> serde_json::Value {
    let text = std::fs::read_to_string(report.path()).expect("report file should exist");
    let reports: Vec<serde_json::Value> =
        serde_json::from_str(&text).expect("report should be valid JSON array");
    reports[0]["axes"].clone()
}

#[test]
fn cli_bench_ds_with_singleton_pruning_records_axis_true() {
    let (output, report) = run_bench_ds("hash-trie", &["--ds-config", "singleton-pruning=true"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let axes = axes_of(&report);
    assert_eq!(axes["ds_config_singleton_pruning"], true, "{axes}");
    assert_eq!(axes["ds_layout_hasher"], "sip", "{axes}");
}

#[test]
fn cli_bench_ds_default_config_records_axis_false() {
    let (output, report) = run_bench_ds("hash-trie", &[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(axes_of(&report)["ds_config_singleton_pruning"], false);
}

#[test]
fn cli_bench_ds_rejects_ds_config_on_tree_trie() {
    let (output, _) = run_bench_ds("tree-trie", &["--ds-config", "singleton-pruning=true"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-config"), "{stderr}");
}

#[test]
fn cli_bench_ds_rejects_unknown_config_key() {
    let (output, _) = run_bench_ds("hash-trie", &["--ds-config", "lazy-expansion=true"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("lazy-expansion"), "{stderr}");
}
