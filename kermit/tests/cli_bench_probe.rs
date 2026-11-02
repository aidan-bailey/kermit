//! `bench probe` screens a benchmark without Criterion. Per query and cell it
//! builds once and runs once, timing both, verifies every query that declares
//! `expected`, and reports the exact `space`, the answer count and the plan.

use {
    std::{
        fs,
        path::PathBuf,
        process::{Command, Output},
    },
    tempfile::TempDir,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

/// A workspace whose one benchmark, `probe`, reads `edge.csv` (1 -> 2,
/// 2 -> 1, 2 -> 3) with two queries: `mutual` (2 answers, declared), then
/// `pinned` (1 answer) declared as `pinned_expected`. Returns the workspace
/// and an empty cache directory.
fn workspace(pinned_expected: u64) -> (TempDir, TempDir) {
    let workspace = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let data = workspace.path().join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("edge.csv"), "src,dst\n1,2\n2,1\n2,3\n").unwrap();
    let benchmarks = workspace.path().join("benchmarks");
    fs::create_dir_all(&benchmarks).unwrap();
    fs::write(
        benchmarks.join("probe.yml"),
        format!(
            r#"name: probe
description: "two queries over one relation"
relations:
  - name: edge
    path: "data/edge.csv"
queries:
  - name: mutual
    description: "edges present in both directions"
    query: "Q(X, Y) :- edge(X, Y), edge(Y, X)."
    expected: 2
  - name: pinned
    description: "the sources of edges into 2"
    query: "Q(X) :- edge(X, c2)."
    expected: {pinned_expected}
"#
        ),
    )
    .unwrap();
    (workspace, cache)
}

/// `kermit bench <bench_args> probe probe <args>` in `workspace`, with
/// Criterion pointed at `workspace/criterion` so a test can check that
/// nothing is written there. Returns the output and the parsed report.
fn probe(
    workspace: &TempDir, cache: &TempDir, bench_args: &[&str], args: &[&str],
) -> (Output, Vec<serde_json::Value>) {
    let report = workspace.path().join("report.json");
    let output = Command::new(kermit_bin())
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LIB_BACKTRACE", "0")
        .env("KERMIT_WORKSPACE", workspace.path())
        .env("XDG_CACHE_HOME", cache.path())
        .env("CRITERION_HOME", workspace.path().join("criterion"))
        .current_dir(workspace.path())
        .arg("bench")
        .args(bench_args)
        .arg("--report-json")
        .arg(&report)
        .args(["probe", "probe"])
        .args(args)
        .output()
        .unwrap();
    let reports = fs::read_to_string(&report)
        .ok()
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap_or_default();
    (output, reports)
}

/// The `values` entry of `report` named `function`.
fn value<'r>(report: &'r serde_json::Value, function: &str) -> &'r serde_json::Value {
    report["values"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["function"] == function)
        .unwrap_or_else(|| panic!("no {function} value in {report}"))
}

#[test]
fn a_probe_verifies_each_query_and_times_one_build_and_one_run() {
    let (workspace, cache) = workspace(1);
    let (output, reports) = probe(&workspace, &cache, &[], &["-i", "all", "-a", "all"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(reports.len(), 6, "two queries on three cells");
    for report in &reports {
        assert_eq!(report["kind"], "probe");
        assert_eq!(report["schema_version"], 6);
        assert!(report["criterion_groups"].as_array().unwrap().is_empty());
        assert_eq!(report["axes"]["verified"], true);
        let space = value(report, "space");
        assert_eq!(space["metric"], "space");
        assert_eq!(space["estimator"], "exact");
        for phase in ["build", "iteration"] {
            let single = value(report, phase);
            assert_eq!(single["metric"], "time", "{phase}");
            assert_eq!(single["estimator"], "single", "{phase}");
            assert!(single["value"].is_u64(), "{phase}: {single}");
        }
    }
    for report in reports.iter().filter(|r| r["axes"]["query"] == "mutual") {
        assert_eq!(report["axes"]["answer_rows"], 2);
        assert_eq!(report["axes"]["plan"], "X, Y");
    }
    assert!(stderr.contains("--- bench probe metadata ---"), "{stderr}");
    assert!(
        stderr.contains("bench probe: skipping incompatible pair"),
        "{stderr}"
    );
    assert!(stderr.contains("single run:"), "{stderr}");
    assert!(
        !workspace.path().join("criterion").exists(),
        "a probe writes no Criterion output"
    );
}

#[test]
fn a_wrong_expected_aborts_the_probe_and_keeps_the_finished_queries() {
    let (workspace, cache) = workspace(5);
    let (output, reports) = probe(&workspace, &cache, &[], &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "verification failed: benchmark 'probe' query 'pinned' on TreeTrie/LeapfrogTriejoin \
             returned 1 tuples, expected 5"
        ),
        "{stderr}"
    );
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0]["axes"]["query"], "mutual");
}

#[test]
fn a_probe_ignores_benchs_criterion_settings() {
    let (workspace, cache) = workspace(1);
    let (output, reports) = probe(
        &workspace,
        &cache,
        &[
            "--name",
            "ignored",
            "--sample-size",
            "50",
            "--warm-up-time",
            "9",
        ],
        &["-q", "pinned", "-i", "hash-trie", "-a", "hash-triejoin"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(reports.len(), 1);
}

#[test]
fn a_probe_writes_its_default_report_under_its_own_kind() {
    let (workspace, cache) = workspace(1);
    let output = Command::new(kermit_bin())
        .env("KERMIT_WORKSPACE", workspace.path())
        .env("XDG_CACHE_HOME", cache.path())
        .current_dir(workspace.path())
        .args(["bench", "probe", "probe", "-q", "pinned"])
        .args(["-i", "tree-trie", "-a", "leapfrog-triejoin"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let names: Vec<String> = fs::read_dir(workspace.path().join("bench-runs"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(
        names[0].starts_with("probe-") && names[0].ends_with(".json"),
        "{names:?}"
    );
}
