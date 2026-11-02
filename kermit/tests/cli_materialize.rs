//! The opt-in `materialize` metric on kermit's own cells: the join with every
//! full assignment kept, sorted and counted distinct. `bench run` and
//! `bench join` take `--metrics materialize`; `bench probe` takes
//! `--materialize`; `bench ds` has no join and refuses it. The memory guard
//! (`--materialize-max-bytes`) skips it with a note, `--verify` checks its
//! count against `expected`, and a run that does not ask for it reports as
//! before.
//!
//! The workspace's benchmark `probe` reads `e` (0 → 0 .. 0 → 399, so the
//! query `star` has 400 × 400 answers of 3 variables, 3,840,000 bytes) and
//! `dup` (the row 1 → 2 twice, which a bag counts twice and the distinct
//! count once).

mod common;

use {
    common::cli::{bench_join, edge_fixture, kermit_bin, reports_of},
    std::{
        fs,
        process::{Command, Output},
    },
    tempfile::TempDir,
};

const FAN: usize = 400;

/// The workspace and an empty cache directory.
fn workspace() -> (TempDir, TempDir) {
    let workspace = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let data = workspace.path().join("data");
    fs::create_dir_all(&data).unwrap();
    let mut e = String::from("src,dst\n");
    for i in 0..FAN {
        e.push_str(&format!("0,{i}\n"));
    }
    fs::write(data.join("e.csv"), e).unwrap();
    fs::write(data.join("dup.csv"), "src,dst\n1,2\n1,2\n").unwrap();
    let benchmarks = workspace.path().join("benchmarks");
    fs::create_dir_all(&benchmarks).unwrap();
    fs::write(
        benchmarks.join("probe.yml"),
        format!(
            r#"name: probe
description: "the materialize metric"
relations:
  - name: e
    path: "data/e.csv"
  - name: dup
    path: "data/dup.csv"
queries:
  - name: small
    description: "400 answers of 2 variables"
    query: "Q(A, B) :- e(A, B)."
    expected: {FAN}
  - name: star
    description: "160000 answers of 3 variables"
    query: "Q(A, B, C) :- e(A, B), e(A, C)."
    expected: {}
  - name: twice
    description: "a duplicated row: a bag answers it twice"
    query: "Q(X, Y) :- dup(X, Y)."
    expected: 2
"#,
            FAN * FAN
        ),
    )
    .unwrap();
    (workspace, cache)
}

/// `kermit bench <Criterion settings> --report-json <report> <args>` in
/// `workspace`. Returns the output and the parsed report array (empty when
/// none was written).
fn kermit(workspace: &TempDir, cache: &TempDir, args: &[&str]) -> (Output, Vec<serde_json::Value>) {
    let report = workspace.path().join("report.json");
    let output = Command::new(kermit_bin())
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LIB_BACKTRACE", "0")
        .env("KERMIT_WORKSPACE", workspace.path())
        .env("XDG_CACHE_HOME", cache.path())
        .env("CRITERION_HOME", workspace.path().join("criterion"))
        .current_dir(workspace.path())
        .args([
            "bench",
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
            "--report-json",
        ])
        .arg(&report)
        .args(args)
        .output()
        .unwrap();
    let reports = fs::read_to_string(&report)
        .ok()
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap_or_default();
    (output, reports)
}

fn stderr(output: &Output) -> String { String::from_utf8_lossy(&output.stderr).into_owned() }

/// The Criterion function ids of `report`, in order.
fn functions(report: &serde_json::Value) -> Vec<String> {
    report["criterion_groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| group["function"].as_str().unwrap().to_string())
        .collect()
}

/// The `values` entry of `report` named `function`, if any.
fn value<'r>(report: &'r serde_json::Value, function: &str) -> Option<&'r serde_json::Value> {
    report["values"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["function"] == function)
}

/// `bench run probe -q <query> -i <ds> -a <algo>` plus `extra`.
fn run(
    workspace: &TempDir, cache: &TempDir, query: &str, ds: &str, algo: &str, extra: &[&str],
) -> (Output, Vec<serde_json::Value>) {
    let mut args = vec!["run", "probe", "-q", query, "-i", ds, "-a", algo];
    args.extend_from_slice(extra);
    kermit(workspace, cache, &args)
}

#[test]
fn materialize_is_recorded_under_its_function_name() {
    let (workspace, cache) = workspace();
    for (ds, algo) in [
        ("tree-trie", "leapfrog-triejoin"),
        ("hash-trie", "hash-triejoin"),
    ] {
        let (output, reports) = run(&workspace, &cache, "small", ds, algo, &[
            "-m",
            "materialize",
        ]);
        assert!(output.status.success(), "{ds}: {}", stderr(&output));
        assert_eq!(reports.len(), 1);
        assert_eq!(functions(&reports[0]), ["materialize"], "{ds}");
        let group = &reports[0]["criterion_groups"][0];
        assert_eq!(group["metric"], "time");
        assert!(
            group["group"].as_str().unwrap().contains("/small/"),
            "{group}"
        );
        // The guard needed the row count, so the describe step counted.
        assert_eq!(reports[0]["axes"]["answer_rows"], FAN);
    }
}

#[test]
fn a_run_that_does_not_ask_for_materialize_reports_as_before() {
    let (workspace, cache) = workspace();
    let (output, reports) = run(
        &workspace,
        &cache,
        "small",
        "tree-trie",
        "leapfrog-triejoin",
        &[],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(functions(&reports[0]), ["iteration", "build"]);
    assert!(value(&reports[0], "materialize").is_none());
    assert!(
        reports[0]["axes"].get("answer_rows").is_none(),
        "nothing counted: {}",
        reports[0]["axes"]
    );
    assert!(
        !stderr(&output).contains("materialize"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_guard_skips_a_result_over_the_limit_with_a_note_and_a_metadata_line() {
    let (workspace, cache) = workspace();
    for (ds, algo, label) in [
        (
            "tree-trie",
            "leapfrog-triejoin",
            "TreeTrie/LeapfrogTriejoin",
        ),
        ("hash-trie", "hash-triejoin", "HashTrie/HashTriejoin"),
    ] {
        let (output, reports) = run(&workspace, &cache, "star", ds, algo, &[
            "-m",
            "build",
            "materialize",
            "--materialize-max-bytes",
            "1048576",
        ]);
        let err = stderr(&output);
        assert!(output.status.success(), "{err}");
        assert!(
            err.contains(&format!(
                "note: skipping materialize for probe/star on {label}: 3840000 bytes over \
                 --materialize-max-bytes 1048576"
            )),
            "{err}"
        );
        assert!(err.contains("materialize:"), "{err}");
        assert!(err.contains("skipped: 3840000 bytes over 1048576"), "{err}");
        assert_eq!(functions(&reports[0]), ["build"], "{ds}");
    }
}

#[test]
fn a_result_under_the_limit_runs() {
    let (workspace, cache) = workspace();
    let (output, reports) = run(
        &workspace,
        &cache,
        "star",
        "tree-trie",
        "leapfrog-triejoin",
        &["-m", "build", "materialize"],
    );
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(!err.contains("skipping materialize"), "{err}");
    assert_eq!(functions(&reports[0]), ["materialize", "build"]);
}

#[test]
fn verify_checks_the_materialized_count_against_expected() {
    let (workspace, cache) = workspace();
    let (output, reports) = run(
        &workspace,
        &cache,
        "star",
        "tree-trie",
        "leapfrog-triejoin",
        &["--verify", "-m", "materialize"],
    );
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(err.contains("verified materialize:"), "{err}");
    assert_eq!(reports[0]["axes"]["verified"], true);
}

/// The bag counts the duplicated row twice (`expected: 2` holds), the
/// distinct count once: the check fails, naming both numbers.
#[test]
fn verify_aborts_when_the_distinct_count_differs_from_expected() {
    let (workspace, cache) = workspace();
    let (output, _) = run(
        &workspace,
        &cache,
        "twice",
        "hash-trie",
        "hash-triejoin",
        &["--verify", "-m", "materialize"],
    );
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains(
            "verification failed: benchmark 'probe' query 'twice' on HashTrie/HashTriejoin \
             materialized 1 distinct full assignments, expected 2"
        ),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_limit_is_bounded_to_one_mebibyte_through_one_tebibyte() {
    let (workspace, cache) = workspace();
    let message =
        "--materialize-max-bytes must be between 1048576 (1 MiB) and 1099511627776 (1 TiB)";
    for bad in ["1048575", "0", "1099511627777", "abc"] {
        let (output, _) = run(
            &workspace,
            &cache,
            "small",
            "tree-trie",
            "leapfrog-triejoin",
            &["-m", "space", "--materialize-max-bytes", bad],
        );
        assert!(!output.status.success(), "{bad}");
        assert!(
            stderr(&output).contains(message),
            "{bad}: {}",
            stderr(&output)
        );
    }
    for good in ["1048576", "1099511627776"] {
        let (output, _) = run(
            &workspace,
            &cache,
            "small",
            "tree-trie",
            "leapfrog-triejoin",
            &["-m", "space", "--materialize-max-bytes", good],
        );
        assert!(output.status.success(), "{good}: {}", stderr(&output));
    }
}

#[test]
fn a_probe_records_a_single_run_materialize_value_when_asked() {
    let (workspace, cache) = workspace();
    let base = [
        "probe",
        "probe",
        "-q",
        "small",
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
    ];
    let mut asked = base.to_vec();
    asked.push("--materialize");
    let (output, reports) = kermit(&workspace, &cache, &asked);
    assert!(output.status.success(), "{}", stderr(&output));
    let single = value(&reports[0], "materialize").expect("a materialize value");
    assert_eq!(single["metric"], "time");
    assert_eq!(single["estimator"], "single");
    assert!(single["value"].is_u64(), "{single}");
    assert!(reports[0]["criterion_groups"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(
        stderr(&output).contains("materialize"),
        "{}",
        stderr(&output)
    );
    assert!(
        !workspace.path().join("criterion").exists(),
        "a probe writes no Criterion output"
    );

    let (output, reports) = kermit(&workspace, &cache, &base);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(value(&reports[0], "materialize").is_none());
}

#[test]
fn a_probe_skips_materialize_over_the_limit() {
    let (workspace, cache) = workspace();
    let (output, reports) = kermit(&workspace, &cache, &[
        "probe",
        "probe",
        "-q",
        "star",
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "--materialize",
        "--materialize-max-bytes",
        "1048576",
    ]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(
        err.contains("note: skipping materialize for probe/star"),
        "{err}"
    );
    assert!(value(&reports[0], "materialize").is_none());
    assert!(value(&reports[0], "build").is_some());
}

#[test]
fn bench_ds_refuses_materialize() {
    let output = Command::new(kermit_bin())
        .env("RUST_BACKTRACE", "0")
        .args(["bench", "ds", "--relation"])
        .arg(edge_fixture())
        .args(["--indexstructure", "tree-trie", "-m", "materialize"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("bench ds has no join, so it cannot measure materialize"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn bench_join_takes_materialize() {
    let (output, report) = bench_join("tree-trie", "leapfrog-triejoin", &["-m", "materialize"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let reports = reports_of(&report);
    assert_eq!(functions(&reports[0]), ["materialize"]);
}
