use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn fixtures_dir() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures") }

fn kermit_bin() -> PathBuf { Path::new(env!("CARGO_BIN_EXE_kermit")).to_path_buf() }

fn run_subcommand(
    subcommand: &str, relations: &[&str], query: &str, algorithm: &str, indexstructure: &str,
    extra_args: &[&str],
) -> std::process::Output {
    let fixtures = fixtures_dir();
    let mut cmd = Command::new(kermit_bin());
    cmd.arg(subcommand);
    for rel in relations {
        cmd.arg("--relations").arg(fixtures.join(rel));
    }
    cmd.arg("--query").arg(fixtures.join(query));
    cmd.arg("--algorithm").arg(algorithm);
    cmd.arg("--indexstructure").arg(indexstructure);
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.output().expect("failed to execute kermit binary")
}

fn run_join(
    relations: &[&str], query: &str, algorithm: &str, indexstructure: &str,
) -> std::process::Output {
    run_subcommand("join", relations, query, algorithm, indexstructure, &[])
}

fn parse_output(output: &std::process::Output) -> Vec<Vec<usize>> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Skip the CSV header row (first non-empty line, which holds the head's
    // variable names) and parse the integer rows that follow.
    let mut tuples: Vec<Vec<usize>> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .skip(1)
        .map(|line| {
            line.split(',')
                .map(|v| v.parse::<usize>().unwrap())
                .collect()
        })
        .collect();
    tuples.sort();
    tuples
}

#[test]
fn cli_join_intersection_tree_trie() {
    let output = run_join(
        &["first.csv", "second.csv"],
        "intersect_query.dl",
        "leapfrog-triejoin",
        "tree-trie",
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tuples = parse_output(&output);
    assert_eq!(tuples, vec![vec![2], vec![3]]);
}

#[test]
fn cli_join_intersection_column_trie() {
    let output = run_join(
        &["first.csv", "second.csv"],
        "intersect_query.dl",
        "leapfrog-triejoin",
        "column-trie",
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tuples = parse_output(&output);
    assert_eq!(tuples, vec![vec![2], vec![3]]);
}

#[test]
fn cli_join_path_query() {
    // edge: (1,2), (2,3), (3,4), (1,3)
    // path(X, Y, Z) :- edge(X, Y), edge(Y, Z).
    // All variables in head so variable ordering matches trie column order.
    // Expected (X,Y,Z) triples:
    //   (1,2,3): edge(1,2) ∧ edge(2,3)
    //   (1,3,4): edge(1,3) ∧ edge(3,4)
    //   (2,3,4): edge(2,3) ∧ edge(3,4)
    let output = run_join(
        &["edge.csv"],
        "path_query.dl",
        "leapfrog-triejoin",
        "tree-trie",
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tuples = parse_output(&output);
    assert_eq!(tuples, vec![vec![1, 2, 3], vec![1, 3, 4], vec![2, 3, 4]]);
}

#[test]
fn cli_join_missing_query_file() {
    let output = run_join(
        &["first.csv"],
        "nonexistent.dl",
        "leapfrog-triejoin",
        "tree-trie",
    );
    assert!(!output.status.success());
}

#[test]
fn cli_join_output_to_file() {
    let fixtures = fixtures_dir();
    let tmp_output = std::env::temp_dir().join("kermit_test_output.csv");

    let mut cmd = Command::new(kermit_bin());
    cmd.arg("join")
        .arg("--relations")
        .arg(fixtures.join("first.csv"))
        .arg("--relations")
        .arg(fixtures.join("second.csv"))
        .arg("--query")
        .arg(fixtures.join("intersect_query.dl"))
        .arg("--algorithm")
        .arg("leapfrog-triejoin")
        .arg("--indexstructure")
        .arg("tree-trie")
        .arg("--output")
        .arg(&tmp_output);

    let output = cmd.output().expect("failed to execute kermit binary");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let contents = std::fs::read_to_string(&tmp_output).unwrap();
    // First non-empty line is the header (head variable names); rows follow.
    let mut tuples: Vec<Vec<usize>> = contents
        .lines()
        .filter(|l| !l.is_empty())
        .skip(1)
        .map(|line| {
            line.split(',')
                .map(|v| v.parse::<usize>().unwrap())
                .collect()
        })
        .collect();
    tuples.sort();
    assert_eq!(tuples, vec![vec![2], vec![3]]);

    let _ = std::fs::remove_file(&tmp_output);
}

#[test]
fn cli_join_no_bench_stderr_silent() {
    let output = run_join(
        &["first.csv", "second.csv"],
        "intersect_query.dl",
        "leapfrog-triejoin",
        "tree-trie",
    );
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("--- bench metadata ---"),
        "stderr should be empty for join subcommand: {stderr}"
    );
}

fn run_bench_join(
    relations: &[&str], query: &str, algorithm: &str, indexstructure: &str, bench_args: &[&str],
) -> std::process::Output {
    let fixtures = fixtures_dir();
    let mut cmd = Command::new(kermit_bin());
    cmd.arg("bench");
    for arg in bench_args {
        cmd.arg(arg);
    }
    cmd.arg("join");
    for rel in relations {
        cmd.arg("--relations").arg(fixtures.join(rel));
    }
    cmd.arg("--query").arg(fixtures.join(query));
    cmd.arg("--algorithm").arg(algorithm);
    cmd.arg("--indexstructure").arg(indexstructure);
    cmd.output().expect("failed to execute kermit binary")
}

#[test]
fn cli_bench_runs_criterion() {
    let output = run_bench_join(
        &["first.csv", "second.csv"],
        "intersect_query.dl",
        "leapfrog-triejoin",
        "tree-trie",
        &[
            "--name",
            "test-intersect",
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // stderr contains metadata
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--- bench metadata ---"),
        "stderr missing metadata header: {stderr}"
    );
    assert!(stderr.contains("data structure:"));
    assert!(stderr.contains("algorithm:"));

    // stdout contains Criterion benchmark output with the given name
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("test-intersect"),
        "stdout should contain benchmark name: {stdout}"
    );
    assert!(
        stdout.contains("time:"),
        "stdout should contain Criterion timing output: {stdout}"
    );
}

#[test]
fn cli_bench_default_name() {
    let output = run_bench_join(
        &["first.csv", "second.csv"],
        "intersect_query.dl",
        "leapfrog-triejoin",
        "tree-trie",
        &["--sample-size", "10", "--measurement-time", "1"],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("join/"),
        "stdout should use default 'join' group name: {stdout}"
    );
}

fn run_bench_ds(
    relation: &str, indexstructure: &str, bench_args: &[&str], ds_args: &[&str],
) -> std::process::Output {
    let fixtures = fixtures_dir();
    let mut cmd = Command::new(kermit_bin());
    cmd.arg("bench");
    for arg in bench_args {
        cmd.arg(arg);
    }
    cmd.arg("ds");
    cmd.arg("--relation").arg(fixtures.join(relation));
    cmd.arg("--indexstructure").arg(indexstructure);
    for arg in ds_args {
        cmd.arg(arg);
    }
    cmd.output().expect("failed to execute kermit binary")
}

#[test]
fn cli_bench_ds_all_metrics() {
    let output = run_bench_ds(
        "first.csv",
        "tree-trie",
        &[
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
        ],
        &[],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--- bench ds metadata ---"),
        "stderr missing ds metadata header: {stderr}"
    );
    assert!(
        stderr.contains("data structure:"),
        "missing data structure in metadata"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("TreeTrie/insertion"),
        "stdout should contain insertion benchmark: {stdout}"
    );
    assert!(
        stdout.contains("TreeTrie/iteration"),
        "stdout should contain iteration benchmark: {stdout}"
    );
    assert!(
        stdout.contains("TreeTrie/space"),
        "stdout should contain space benchmark: {stdout}"
    );
}

#[test]
fn cli_bench_ds_writes_json_report() {
    let fixtures = fixtures_dir();
    let tmp_report = std::env::temp_dir().join("kermit_test_report.json");

    let mut cmd = Command::new(kermit_bin());
    cmd.arg("bench")
        .arg("--sample-size")
        .arg("10")
        .arg("--measurement-time")
        .arg("1")
        .arg("--warm-up-time")
        .arg("1")
        .arg("--report-json")
        .arg(&tmp_report)
        .arg("ds")
        .arg("--relation")
        .arg(fixtures.join("first.csv"))
        .arg("--indexstructure")
        .arg("tree-trie")
        .arg("-m")
        .arg("space");

    let output = cmd.output().expect("failed to execute kermit binary");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let contents = std::fs::read_to_string(&tmp_report).expect("report file should exist");
    let json: serde_json::Value =
        serde_json::from_str(&contents).expect("report should be valid JSON");

    assert!(json.is_array(), "top-level shape is always a JSON array");
    let report = &json[0];
    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["kind"], "ds");

    let metadata = report["metadata"]
        .as_array()
        .expect("metadata should be an array");
    assert!(metadata
        .iter()
        .any(|f| f["label"] == "data structure" && f["value"] == "TreeTrie"));
    assert!(metadata
        .iter()
        .any(|f| f["label"] == "relation size" && f["value"].as_str().unwrap().ends_with(" B")));

    let axes = &report["axes"];
    assert!(axes.is_object(), "axes should be a JSON object");
    assert_eq!(axes["data_structure"], "TreeTrie");
    assert!(
        axes["relation_bytes"].is_number(),
        "relation_bytes should be numeric, not stringified"
    );
    assert!(axes["tuples"].is_number(), "tuples should be numeric");
    assert!(axes["arity"].is_number(), "arity should be numeric");

    let groups = report["criterion_groups"]
        .as_array()
        .expect("criterion_groups should be an array");
    assert_eq!(groups.len(), 1, "space-only should yield exactly one group");
    assert_eq!(groups[0]["function"], "TreeTrie/space");
    assert_eq!(groups[0]["metric"], "space");

    let _ = std::fs::remove_file(&tmp_report);
}

#[test]
fn cli_bench_ds_space_only() {
    let output = run_bench_ds("first.csv", "column-trie", &["--sample-size", "10"], &[
        "-m", "space",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ColumnTrie/space"),
        "stdout should contain space benchmark: {stdout}"
    );
    assert!(
        !stdout.contains("ColumnTrie/insertion"),
        "space-only should not have insertion benchmark: {stdout}"
    );
    assert!(
        !stdout.contains("ColumnTrie/iteration"),
        "space-only should not have iteration benchmark: {stdout}"
    );
}
