//! CLI smoke test: `--ds-layout-pruning on` records `ds_layout_pruning: "on"`,
//! the absent flag records `"off"`, and the flag is rejected on a structure
//! without the dimension. Mirrors `cli_hash_trie_hasher_choice.rs`.

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
fn cli_bench_ds_with_pruning_on_records_axis() {
    let (output, report) = run_bench_ds("hash-trie", &["--ds-layout-pruning", "on"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let axes = axes_of(&report);
    assert_eq!(axes["ds_layout_pruning"], "on", "{axes}");
    assert_eq!(axes["ds_layout_hasher"], "sip", "{axes}");
}

#[test]
fn cli_bench_ds_default_pruning_is_off() {
    let (output, report) = run_bench_ds("hash-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_layout_pruning"], "off");
}

#[test]
fn cli_bench_ds_rejects_pruning_flag_on_tree_trie() {
    let (output, _) = run_bench_ds("tree-trie", &["--ds-layout-pruning", "on"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-layout-pruning"), "{stderr}");
}

#[test]
fn cli_bench_run_sweep_carries_pruning_only_to_hash_trie_cells() {
    // `-i all -a all --ds-layout-pruning on` runs the three valid cells; the
    // sorted cells have no pruning axis and the hash cell reports "on".
    let report = NamedTempFile::new().expect("failed to create temp report file");
    let output = Command::new(kermit_bin())
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
        .args([
            "run",
            "triangle",
            "-i",
            "all",
            "-a",
            "all",
            "-m",
            "space",
            "--ds-layout-pruning",
            "on",
        ])
        .output()
        .expect("failed to execute kermit binary");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(report.path()).expect("report file should exist");
    let reports: Vec<serde_json::Value> =
        serde_json::from_str(&text).expect("report should be valid JSON array");
    assert_eq!(reports.len(), 3, "three valid cells: {text}");
    for r in &reports {
        let axes = &r["axes"];
        if axes["data_structure"] == "HashTrie" {
            assert_eq!(axes["ds_layout_pruning"], "on", "{axes}");
        } else {
            assert!(axes.get("ds_layout_pruning").is_none(), "{axes}");
        }
    }
}
