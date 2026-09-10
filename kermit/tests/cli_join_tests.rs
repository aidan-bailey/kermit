use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

fn fixtures_dir() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures") }

fn kermit_bin() -> PathBuf { Path::new(env!("CARGO_BIN_EXE_kermit")).to_path_buf() }

static REPORT_COUNTER: AtomicU64 = AtomicU64::new(0);
static CACHE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_report_path(tag: &str) -> PathBuf {
    let n = REPORT_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "kermit_bench_{tag}_{}_{}.json",
        std::process::id(),
        n
    ))
}

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

/// The hash cell is reachable from `kermit join`: same answer as the
/// sorted cells, via `hash_join` rather than `lftj_join`.
#[test]
fn cli_join_intersection_hash_trie() {
    let output = run_join(
        &["first.csv", "second.csv"],
        "intersect_query.dl",
        "hash-triejoin",
        "hash-trie",
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(parse_output(&output), vec![vec![2], vec![3]]);
}

/// `Q(X) :- diagonal(X, X).` — the PROBLEMS.md repro, through the CLI on
/// every valid cell. Only the head column is asserted: the entry points
/// emit every variable, including the fresh one the selection rewrite
/// introduces (the same leak the const rewrite has), so each row is
/// `X,X`. Head projection is tracked in issue #71.
fn assert_cli_diagonal(algorithm: &str, indexstructure: &str) {
    let output = run_join(
        &["diagonal.csv"],
        "diagonal_query.dl",
        algorithm,
        indexstructure,
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut heads: Vec<usize> = parse_output(&output).iter().map(|row| row[0]).collect();
    heads.sort();
    assert_eq!(heads, vec![1, 3]);
}

#[test]
fn cli_join_diagonal_tree_trie() { assert_cli_diagonal("leapfrog-triejoin", "tree-trie"); }

#[test]
fn cli_join_diagonal_column_trie() { assert_cli_diagonal("leapfrog-triejoin", "column-trie"); }

#[test]
fn cli_join_diagonal_hash_trie() { assert_cli_diagonal("hash-triejoin", "hash-trie"); }

/// `--ds-layout-hasher` threads through to the hash cell on `kermit join`.
#[test]
fn cli_join_hash_trie_fxhash_layout() {
    let output = run_subcommand(
        "join",
        &["first.csv", "second.csv"],
        "intersect_query.dl",
        "hash-triejoin",
        "hash-trie",
        &["--ds-layout-hasher", "fxhash"],
    );
    assert!(output.status.success());
    assert_eq!(parse_output(&output), vec![vec![2], vec![3]]);
}

/// `--ds-layout-pruning` threads through to the hash cell on `kermit join`.
/// Pruning is a Layout — it changes the node shape, not the answers — so
/// the result must match the unpruned run above.
#[test]
fn cli_join_hash_trie_pruning_layout() {
    let output = run_subcommand(
        "join",
        &["first.csv", "second.csv"],
        "intersect_query.dl",
        "hash-triejoin",
        "hash-trie",
        &["--ds-layout-pruning", "on"],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(parse_output(&output), vec![vec![2], vec![3]]);
}

/// Both Layout dimensions at once: `kermit join` reaches the same
/// `with_hash_trie_layout!` product the bench dispatchers do, so the
/// off-diagonal cell (`fxhash` × pruning `on`) is reachable and agrees.
#[test]
fn cli_join_hash_trie_layout_product() {
    let output = run_subcommand(
        "join",
        &["first.csv", "second.csv"],
        "intersect_query.dl",
        "hash-triejoin",
        "hash-trie",
        &["--ds-layout-hasher", "fxhash", "--ds-layout-pruning", "on"],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(parse_output(&output), vec![vec![2], vec![3]]);
}

/// A `--ds-layout-*` flag on a sorted structure is a usage error rather
/// than a silently ignored flag — the same discipline
/// `validate_layout_choices` enforces on the bench subcommands.
#[test]
fn cli_join_layout_flag_rejected_on_sorted_structure() {
    let output = run_subcommand(
        "join",
        &["first.csv", "second.csv"],
        "intersect_query.dl",
        "leapfrog-triejoin",
        "tree-trie",
        &["--ds-layout-pruning", "on"],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--ds-layout-pruning is only valid with --indexstructure hash-trie"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("panicked"), "stderr: {stderr}");
}

/// An incompatible (structure, algorithm) pair is a clean usage error, not
/// a panic.
#[test]
fn cli_join_incompatible_pair_is_usage_error() {
    let output = run_join(
        &["first.csv", "second.csv"],
        "intersect_query.dl",
        "leapfrog-triejoin",
        "hash-trie",
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("incompatible selection"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("panicked"), "stderr: {stderr}");
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
    // PID + nanos suffix so concurrent test processes don't race on the path.
    let tmp_output = std::env::temp_dir().join(format!(
        "kermit_test_output_{}_{}.csv",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));

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
    let report_path = temp_report_path("join");
    let mut cmd = Command::new(kermit_bin());
    cmd.arg("bench");
    for arg in bench_args {
        cmd.arg(arg);
    }
    cmd.arg("--report-json").arg(&report_path);
    cmd.arg("join");
    for rel in relations {
        cmd.arg("--relations").arg(fixtures.join(rel));
    }
    cmd.arg("--query").arg(fixtures.join(query));
    cmd.arg("--algorithm").arg(algorithm);
    cmd.arg("--indexstructure").arg(indexstructure);
    let output = cmd.output().expect("failed to execute kermit binary");
    let _ = std::fs::remove_file(&report_path);
    output
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
            "--warm-up-time",
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
        stderr.contains("--- bench run metadata ---"),
        "stderr missing metadata header: {stderr}"
    );
    assert!(
        stderr.contains("benchmark:") && stderr.contains("adhoc"),
        "{stderr}"
    );
    assert!(stderr.contains("data structure:"));
    assert!(stderr.contains("algorithm:"));

    // stdout contains Criterion benchmark output with the given name as
    // the group prefix
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("test-intersect/adhoc/intersect_query/TreeTrie/LeapfrogTriejoin"),
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
        &[
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("join/adhoc/intersect_query/TreeTrie/LeapfrogTriejoin/"),
        "stdout should use default 'join' group name: {stdout}"
    );
}

fn run_bench_ds(
    relation: &str, indexstructure: &str, bench_args: &[&str], ds_args: &[&str],
) -> std::process::Output {
    let fixtures = fixtures_dir();
    let report_path = temp_report_path("ds");
    let mut cmd = Command::new(kermit_bin());
    cmd.arg("bench");
    for arg in bench_args {
        cmd.arg(arg);
    }
    cmd.arg("--report-json").arg(&report_path);
    cmd.arg("ds");
    cmd.arg("--relation").arg(fixtures.join(relation));
    cmd.arg("--indexstructure").arg(indexstructure);
    for arg in ds_args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to execute kermit binary");
    let _ = std::fs::remove_file(&report_path);
    output
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
fn cli_bench_ds_writes_default_report_when_path_omitted() {
    let fixtures = fixtures_dir();
    let tmp_cwd =
        std::env::temp_dir().join(format!("kermit_default_report_{}", std::process::id(),));
    let _ = std::fs::remove_dir_all(&tmp_cwd);
    std::fs::create_dir_all(&tmp_cwd).expect("create tmp cwd");

    let mut cmd = Command::new(kermit_bin());
    cmd.current_dir(&tmp_cwd)
        .arg("bench")
        .arg("--sample-size")
        .arg("10")
        .arg("--measurement-time")
        .arg("1")
        .arg("--warm-up-time")
        .arg("1")
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

    let bench_runs = tmp_cwd.join("bench-runs");
    let entries: Vec<PathBuf> = std::fs::read_dir(&bench_runs)
        .expect("bench-runs/ should be auto-created")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "expected exactly one default report; got {entries:?}"
    );
    let path = &entries[0];
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    assert!(
        name.starts_with("ds-") && name.ends_with(".json"),
        "default name should be ds-<unix-millis>.json; got {name}"
    );

    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap())
        .expect("default report should be valid JSON");
    assert_eq!(json[0]["schema_version"], 2);
    assert_eq!(json[0]["kind"], "ds");

    let _ = std::fs::remove_dir_all(&tmp_cwd);
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

#[test]
fn cli_bench_run_writes_json_report() {
    // `triangle` declares its edge relation with `path:`, so the committed
    // CSV is read in place and nothing is ever downloaded. The cache is still
    // redirected to a temporary directory to keep the test hermetic against
    // anything else that might write there.
    let n = CACHE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_cache = std::env::temp_dir().join(format!(
        "kermit_bench_run_cache_{}_{}",
        std::process::id(),
        n
    ));
    std::fs::create_dir_all(&tmp_cache).expect("create tmp cache dir");

    let tmp_report = temp_report_path("run");
    let _ = std::fs::remove_file(&tmp_report);

    let mut cmd = Command::new(kermit_bin());
    cmd.env("XDG_CACHE_HOME", &tmp_cache)
        .arg("bench")
        .arg("--sample-size")
        .arg("10")
        .arg("--measurement-time")
        .arg("1")
        .arg("--warm-up-time")
        .arg("1")
        .arg("--report-json")
        .arg(&tmp_report)
        .arg("run")
        .arg("triangle")
        .arg("-i")
        .arg("tree-trie")
        .arg("-a")
        .arg("leapfrog-triejoin")
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
    assert_eq!(report["kind"], "run");

    let metadata = report["metadata"]
        .as_array()
        .expect("metadata should be an array");
    assert!(metadata
        .iter()
        .any(|f| f["label"] == "benchmark" && f["value"] == "triangle"));
    assert!(metadata
        .iter()
        .any(|f| f["label"] == "data structure" && f["value"] == "TreeTrie"));
    assert!(metadata
        .iter()
        .any(|f| f["label"] == "algorithm" && f["value"] == "LeapfrogTriejoin"));

    let axes = &report["axes"];
    assert!(axes.is_object(), "axes should be a JSON object");
    assert_eq!(axes["benchmark"], "triangle");
    assert_eq!(axes["data_structure"], "TreeTrie");
    assert_eq!(axes["algorithm"], "LeapfrogTriejoin");
    assert!(
        axes["query"].is_string(),
        "axes.query should be a string identifier"
    );
    // Tuple count pins the committed fixture: `benchmarks/data/triangle/
    // edge.csv` holds 8 edges. If path resolution silently broke and the
    // relation came from anywhere else, the count would not match.
    assert_eq!(
        axes["tuples"], 8,
        "should reflect the committed 8-edge triangle fixture",
    );

    let groups = report["criterion_groups"]
        .as_array()
        .expect("criterion_groups should be an array");
    assert!(
        !groups.is_empty(),
        "space metric must produce at least one criterion group"
    );
    for g in groups {
        assert_eq!(g["metric"], "space");
        let function = g["function"].as_str().expect("function must be a string");
        assert!(
            function.starts_with("space/"),
            "space functions are named space/<relation>; got {function}"
        );
    }

    let _ = std::fs::remove_file(&tmp_report);
    let _ = std::fs::remove_dir_all(&tmp_cache);
}
