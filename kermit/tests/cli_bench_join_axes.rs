//! `bench join` goes through the same generic runner as `bench run`, so an
//! ad-hoc join reports the full metric set and the structure's
//! optimization axes, under `bench run`'s Criterion naming with the
//! `adhoc/<query-stem>` identity. The runner reads each relation once,
//! builds the indexes each query's plan reads, and reports every one of
//! them.

mod common;

use {
    common::cli::{bench_join, kermit_bin, reports_of},
    std::{
        fs,
        process::{Command, Output},
    },
    tempfile::TempDir,
};

#[test]
fn cli_bench_join_hash_trie_reports_axes_and_all_default_metrics() {
    let (output, report) = bench_join("hash-trie", "hash-triejoin", &[
        "--ds-layout-hasher",
        "fxhash",
        "--ds-config",
        "load-factor=0.5",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 1, "one query, one report");
    let r = &reports[0];
    assert_eq!(r["kind"], "join");
    let axes = &r["axes"];
    assert_eq!(axes["benchmark"], "adhoc");
    assert_eq!(axes["query"], "intersect_query");
    assert_eq!(axes["data_structure"], "HashTrie");
    assert_eq!(axes["algorithm"], "HashTriejoin");
    assert_eq!(axes["optimiser"], "lexicographic");
    assert!(axes.get("column_orders").is_none(), "{axes}");
    // The binary under test is built with this package's features (#112).
    let allocator = if cfg!(feature = "jemalloc") {
        "jemalloc"
    } else {
        "system"
    };
    assert_eq!(axes["allocator"], allocator);
    assert_eq!(axes["ds_layout_hasher"], "fxhash");
    assert_eq!(axes["ds_layout_pruning"], "off");
    assert_eq!(axes["ds_config_load_factor"], 0.5);
    // `first` and `second` hold three tuples each, and each atom builds its
    // own unfiltered index over its relation.
    assert_eq!(axes["tuples"], 6);
    assert_eq!(axes["index_rows"], 6);
    assert!(
        axes.get("relations").is_none(),
        "the old count axis is gone"
    );

    let groups = r["criterion_groups"].as_array().unwrap();
    let group = "join/adhoc/intersect_query/HashTrie/HashTriejoin";
    assert!(groups.iter().all(|g| g["group"] == group), "{groups:?}");
    let mut functions: Vec<&str> = groups
        .iter()
        .map(|g| g["function"].as_str().unwrap())
        .collect();
    functions.sort_unstable();
    assert_eq!(functions, ["build", "iteration", "space"]);
}

#[test]
fn cli_bench_join_name_is_a_prefix() {
    let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let report = tempfile::NamedTempFile::new().unwrap();
    let output = std::process::Command::new(kermit_bin())
        .args([
            "bench",
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
        ])
        .arg("--name")
        .arg("mine")
        .arg("--report-json")
        .arg(report.path())
        .arg("join")
        .arg("--relations")
        .arg(fixtures.join("first.csv"))
        .arg(fixtures.join("second.csv"))
        .arg("--query")
        .arg(fixtures.join("intersect_query.dl"))
        .args(["-i", "tree-trie", "-a", "leapfrog-triejoin", "-m", "space"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let groups = reports_of(&report)[0]["criterion_groups"].clone();
    assert_eq!(
        groups[0]["group"],
        "mine/adhoc/intersect_query/TreeTrie/LeapfrogTriejoin"
    );
}

/// A TreeTrie built from a view whose filter keeps no row holds no heap, so
/// its `space` has nothing to measure: the run succeeds, notes the skip on
/// stderr and reports no `space` function, instead of tripping
/// `add_space_bench`'s zero-cost guard.
#[test]
fn cli_bench_join_skips_space_for_indexes_that_hold_no_bytes() {
    let dir = TempDir::new().unwrap();
    let query = dir.path().join("no_match.dl");
    fs::write(&query, "Q(X) :- edge(X, c999).").unwrap();
    let report = tempfile::NamedTempFile::new().unwrap();
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
        .arg("join")
        .arg("--relations")
        .arg(common::cli::edge_fixture())
        .arg("--query")
        .arg(&query)
        .args(["-i", "tree-trie", "-a", "leapfrog-triejoin", "-m", "space"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert!(
        stderr.contains("skipping the space metric"),
        "stderr: {stderr}"
    );
    let reports = reports_of(&report);
    assert_eq!(reports[0]["axes"]["index_rows"], 0);
    assert!(
        reports[0]["criterion_groups"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{}",
        reports[0]["criterion_groups"]
    );
}

#[test]
fn cli_bench_join_rejects_ds_config_on_tree_trie() {
    let (output, _) = bench_join("tree-trie", "leapfrog-triejoin", &[
        "--ds-config",
        "load-factor=0.5",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-config"), "{stderr}");
}

/// One benchmark, `per-query`, over `data/edge.csv` (1 -> 2, 2 -> 1,
/// 2 -> 3). A second declared relation, `data/unused.csv`, has a header that
/// parses and a row that does not, so any run that reads its tuples fails.
/// Returns the workspace and an empty cache directory.
fn per_query_workspace() -> (TempDir, TempDir) {
    let workspace = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let data = workspace.path().join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("edge.csv"), "src,dst\n1,2\n2,1\n2,3\n").unwrap();
    fs::write(data.join("unused.csv"), "a,b\nnot,numbers\n").unwrap();
    let benchmarks = workspace.path().join("benchmarks");
    fs::create_dir_all(&benchmarks).unwrap();
    fs::write(
        benchmarks.join("per-query.yml"),
        r#"name: per-query
description: "queries over one relation, beside a relation no query names"
relations:
  - name: edge
    path: "data/edge.csv"
  - name: unused
    path: "data/unused.csv"
queries:
  - name: mutual
    description: "edges present in both directions: two indexes over one relation"
    query: "Q(X, Y) :- edge(X, Y), edge(Y, X)."
    expected: 2
  - name: pinned
    description: "the sources of edges into 2: a constant filter"
    query: "Q(X) :- edge(X, c2)."
    expected: 1
"#,
    )
    .unwrap();
    (workspace, cache)
}

/// Where `bench_run_per_query` sends Criterion's output.
fn criterion_home(workspace: &TempDir) -> std::path::PathBuf { workspace.path().join("criterion") }

/// The mean point estimate of the Criterion function `group`/`function`, found
/// by the `group_id` and `function_id` in each `new/benchmark.json` under
/// `home`, never by computing the directory name.
fn mean_estimate(home: &std::path::Path, group: &str, function: &str) -> f64 {
    fn benchmark_jsons(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                benchmark_jsons(&path, found);
            } else if path
                .file_name()
                .is_some_and(|name| name == "benchmark.json")
                && path
                    .parent()
                    .and_then(|p| p.file_name())
                    .is_some_and(|name| name == "new")
            {
                found.push(path);
            }
        }
    }
    let mut jsons = Vec::new();
    benchmark_jsons(home, &mut jsons);
    let matching: Vec<_> = jsons
        .into_iter()
        .filter(|path| {
            let id: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
            id["group_id"] == group && id["function_id"] == function
        })
        .collect();
    assert_eq!(matching.len(), 1, "{group}/{function}: {matching:?}");
    let estimates = matching[0].with_file_name("estimates.json");
    let estimates: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(estimates).unwrap()).unwrap();
    estimates["mean"]["point_estimate"].as_f64().unwrap()
}

/// `bench run per-query` in `workspace` with `args`, returning the process
/// output and the parsed report array (empty if no report was written).
fn bench_run_per_query(
    workspace: &TempDir, cache: &TempDir, args: &[&str],
) -> (Output, Vec<serde_json::Value>) {
    let report = workspace.path().join("report.json");
    let output = Command::new(kermit_bin())
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LIB_BACKTRACE", "0")
        .env("KERMIT_WORKSPACE", workspace.path())
        .env("XDG_CACHE_HOME", cache.path())
        .env("CRITERION_HOME", criterion_home(workspace))
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
        .args(["run", "per-query"])
        .args(args)
        .output()
        .unwrap();
    let reports = fs::read_to_string(&report)
        .ok()
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap_or_default();
    (output, reports)
}

/// The report's Criterion function ids, sorted.
fn functions_of(report: &serde_json::Value) -> Vec<String> {
    let mut functions: Vec<String> = report["criterion_groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["function"].as_str().unwrap().to_string())
        .collect();
    functions.sort_unstable();
    functions
}

/// The values of the report's `index` metadata lines, in order.
fn index_lines(report: &serde_json::Value) -> Vec<String> {
    report["metadata"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["label"] == "index")
        .map(|m| m["value"].as_str().unwrap().to_string())
        .collect()
}

/// `edge(X, Y), edge(Y, X)` was rejected under `--column-orders stored`
/// (contradictory orders). Every query is now planned freely and each atom
/// builds the index its plan reads: here the relation twice, in two column
/// orders. `unused.csv` is declared but never read.
#[test]
fn bench_run_builds_one_index_per_atom_and_reports_each() {
    let (workspace, cache) = per_query_workspace();
    let (output, reports) = bench_run_per_query(&workspace, &cache, &[
        "-q",
        "mutual",
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "--verify",
        "-m",
        "build",
        "iteration",
        "space",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(reports.len(), 1, "{reports:?}");
    let report = &reports[0];
    assert_eq!(functions_of(report), ["build", "iteration", "space"]);
    let axes = &report["axes"];
    assert!(axes.get("column_orders").is_none(), "{axes}");
    assert_eq!(axes["verified"], true);
    assert_eq!(
        axes["tuples"], 3,
        "the relation's rows, once, however many atoms read it"
    );
    assert_eq!(
        axes["index_rows"], 6,
        "two unfiltered indexes of three rows"
    );
    let lines = index_lines(report);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[0].starts_with("#0 edge filter - columns [0, 1]: 3 rows, "),
        "{lines:?}"
    );
    assert!(
        lines[1].starts_with("#1 edge filter - columns [1, 0]: 3 rows, "),
        "{lines:?}"
    );
    assert!(
        lines.iter().all(|line| line.ends_with(" bytes")),
        "{lines:?}"
    );
    for line in &lines {
        assert!(
            stderr.contains(line.as_str()),
            "{line} missing from {stderr}"
        );
    }

    // `space` is one function summing every index the query reads, so its
    // per-iteration measurement is the sum of the `index` lines' bytes.
    let indexed_bytes: usize = lines
        .iter()
        .map(|line| {
            let bytes = line.strip_suffix(" bytes").unwrap();
            bytes[bytes.rfind(", ").unwrap() + 2..]
                .parse::<usize>()
                .unwrap()
        })
        .sum();
    let space = report["criterion_groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["function"] == "space")
        .unwrap();
    let measured = mean_estimate(
        &criterion_home(&workspace),
        space["group"].as_str().unwrap(),
        "space",
    );
    assert!(
        (measured - indexed_bytes as f64).abs() < 0.5,
        "space measured {measured} bytes, the index lines sum to {indexed_bytes}"
    );
}

/// A lazy `HashTrie`'s joins expand it, so `--verify` and `iteration` run on
/// fresh builds (#92), `end_to_end` builds its own, and `space` must find the
/// as-built indexes untouched.
#[test]
fn bench_run_on_a_lazy_cell_runs_every_metric_over_fresh_builds() {
    let (workspace, cache) = per_query_workspace();
    let (output, reports) = bench_run_per_query(&workspace, &cache, &[
        "-q",
        "mutual",
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "--ds-layout-expansion",
        "lazy",
        "--verify",
        "-m",
        "build",
        "iteration",
        "end-to-end",
        "space",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(functions_of(&reports[0]), [
        "build",
        "end_to_end",
        "iteration",
        "space"
    ]);
    assert_eq!(reports[0]["axes"]["verified"], true);
    assert_eq!(reports[0]["axes"]["ds_layout_expansion"], "lazy");
}

/// A constant is applied while the index is built: the index holds only the
/// rows that match, and its line names the filter.
#[test]
fn bench_run_pushes_a_constant_into_the_index_it_builds() {
    let (workspace, cache) = per_query_workspace();
    let (output, reports) = bench_run_per_query(&workspace, &cache, &[
        "-q",
        "pinned",
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "--verify",
        "-m",
        "space",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let report = &reports[0];
    assert_eq!(report["axes"]["verified"], true);
    assert_eq!(
        report["axes"]["tuples"], 3,
        "tuples counts the catalog's rows, not the filtered ones"
    );
    assert_eq!(report["axes"]["index_rows"], 1);
    let lines = index_lines(report);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("#0 edge filter col1=2 columns [0]: 1 rows, "),
        "{lines:?}"
    );
}

/// `unused.csv` cannot be read, and the run succeeds: no query names it, so
/// it is neither read nor listed among the query's relations.
#[test]
fn bench_run_reads_only_the_relations_its_queries_name() {
    let (workspace, cache) = per_query_workspace();
    let (output, _) = bench_run_per_query(&workspace, &cache, &[
        "-q",
        "pinned",
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "-m",
        "space",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("\"edge\" (arity 2)"), "{stderr}");
    assert!(!stderr.contains("\"unused\""), "{stderr}");
}
