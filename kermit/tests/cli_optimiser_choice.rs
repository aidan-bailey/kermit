//! CLI smoke tests: `--optimiser` selection lands in the bench-report
//! `optimiser` axis, defaulting to `lexicographic`.

use std::{fs, path::PathBuf, process::Command};

fn kermit_bin() -> &'static str { env!("CARGO_BIN_EXE_kermit") }

fn fixtures_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures") }

fn edge_fixture() -> PathBuf { fixtures_dir().join("edge.csv") }

/// Runs `bench join` against the edge fixture with `extra_args` appended,
/// returning the parsed report array. `tag` keeps concurrent tests' temp
/// files apart.
fn run_bench_join(tag: &str, extra_args: &[&str]) -> Vec<serde_json::Value> {
    let pid = std::process::id();
    let report_path = std::env::temp_dir().join(format!("kermit-optimiser-{tag}-{pid}.json"));

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
        report_path.to_str().unwrap(),
        "join",
        "--relations",
        edge_fixture().to_str().unwrap(),
        "--query",
        fixtures_dir().join("path_query.dl").to_str().unwrap(),
        "--algorithm",
        "leapfrog-triejoin",
        "--indexstructure",
        "tree-trie",
    ]);
    cmd.args(extra_args);
    let output = cmd.output().expect("failed to run kermit binary");
    assert!(
        output.status.success(),
        "kermit bench join failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let reports: Vec<serde_json::Value> =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    let _ = fs::remove_file(&report_path);
    assert!(!reports.is_empty(), "report array should not be empty");
    reports
}

#[test]
fn cli_bench_join_with_cardinality_optimiser_records_axis() {
    let reports = run_bench_join("cardinality", &["--optimiser", "cardinality"]);
    assert_eq!(reports[0]["axes"]["optimiser"], "cardinality");
}

#[test]
fn cli_bench_join_default_optimiser_is_lexicographic() {
    let reports = run_bench_join("default", &[]);
    assert_eq!(reports[0]["axes"]["optimiser"], "lexicographic");
}
