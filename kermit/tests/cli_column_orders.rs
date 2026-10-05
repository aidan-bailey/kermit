//! CLI smoke tests for `--column-orders` (issue #93): the flag lands in the
//! bench-report `column_orders` axis, defaulting to `stored`, and `kermit
//! join --column-orders any` answers a query that `stored` rejects, with
//! identical rows on every cell.

mod common;

use {
    common::cli::{axes_of, bench_join, bench_run, kermit_bin},
    std::{fs, path::Path, process::Command},
    tempfile::TempDir,
};

#[test]
fn bench_join_records_stored_by_default() {
    let (output, report) = bench_join("tree-trie", "leapfrog-triejoin", &["-m", "space"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let axes = axes_of(&report);
    assert_eq!(axes["column_orders"], "stored");
    assert_eq!(axes["optimiser"], "lexicographic");
}

#[test]
fn bench_join_with_any_records_the_axis() {
    let (output, report) = bench_join("hash-trie", "hash-triejoin", &[
        "-m",
        "space",
        "--column-orders",
        "any",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["column_orders"], "any");
}

/// `triangle` binds every atom left to right under `lexicographic` either
/// way, so `any` needs no copy and the run succeeds on the axis alone.
#[test]
fn bench_run_with_any_records_the_axis() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "column-trie",
        "-a",
        "leapfrog-triejoin",
        "-m",
        "space",
        "--column-orders",
        "any",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let axes = axes_of(&report);
    assert_eq!(axes["column_orders"], "any");
    assert_eq!(axes["data_structure"], "ColumnTrie");
}

/// The ten (structure, algorithm, Layout) cells `kermit join` can run.
const CELLS: [&[&str]; 10] = [
    &["-i", "tree-trie", "-a", "leapfrog-triejoin"],
    &["-i", "column-trie", "-a", "leapfrog-triejoin"],
    &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "--ds-layout-hasher",
        "sip",
        "--ds-layout-pruning",
        "off",
    ],
    &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "--ds-layout-hasher",
        "sip",
        "--ds-layout-pruning",
        "on",
    ],
    &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "--ds-layout-hasher",
        "fxhash",
        "--ds-layout-pruning",
        "off",
    ],
    &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "--ds-layout-hasher",
        "fxhash",
        "--ds-layout-pruning",
        "on",
    ],
    &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "--ds-layout-hasher",
        "sip",
        "--ds-layout-pruning",
        "off",
        "--ds-layout-expansion",
        "lazy",
    ],
    &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "--ds-layout-hasher",
        "sip",
        "--ds-layout-pruning",
        "on",
        "--ds-layout-expansion",
        "lazy",
    ],
    &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "--ds-layout-hasher",
        "fxhash",
        "--ds-layout-pruning",
        "off",
        "--ds-layout-expansion",
        "lazy",
    ],
    &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "--ds-layout-hasher",
        "fxhash",
        "--ds-layout-pruning",
        "on",
        "--ds-layout-expansion",
        "lazy",
    ],
];

/// 1 ↔ 2, 2 → 3.
fn write_edges(dir: &Path) -> std::path::PathBuf {
    let edge = dir.join("edge.csv");
    fs::write(&edge, "src,dst\n1,2\n2,1\n2,3\n").unwrap();
    edge
}

/// `kermit join` on `query` under `--column-orders <policy>`, once per
/// cell: the sorted rows of each cell's CSV (after the header line).
fn rows_on_every_cell(query: &str, policy: &str) -> Vec<Vec<String>> {
    let dir = TempDir::new().unwrap();
    let edge = write_edges(dir.path());
    let query_path = dir.path().join("q.dl");
    fs::write(&query_path, query).unwrap();
    CELLS
        .iter()
        .map(|cell| {
            let output = Command::new(kermit_bin())
                .env("RUST_BACKTRACE", "0")
                .env("RUST_LIB_BACKTRACE", "0")
                .args(["join", "-r"])
                .arg(&edge)
                .arg("-q")
                .arg(&query_path)
                .args(*cell)
                .args(["--column-orders", policy])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{query} under {policy} on {cell:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut lines: Vec<String> = stdout.lines().skip(1).map(str::to_string).collect();
            lines.sort();
            lines
        })
        .collect()
}

/// `stored` rejects the mutual-edge query for its column order; `any`
/// answers it over one reordered copy, on every cell.
#[test]
fn join_answers_a_cyclic_query_under_any_on_every_cell() {
    let rows = rows_on_every_cell("Q(X, Y) :- edge(X, Y), edge(Y, X).", "any");
    for (cell, got) in CELLS.iter().zip(&rows) {
        assert_eq!(got, &["1,2".to_string(), "2,1".to_string()], "{cell:?}");
    }
}

/// The head numbers `Z` before `Y`, so under `any` `lexicographic` binds
/// `Z` first and `edge(Y, Z)` reads a reordered copy, while `stored` reads
/// it as stored. Both must give the same rows on every cell.
#[test]
fn join_under_any_answers_an_acyclic_query_like_stored() {
    let query = "Q(X, Z) :- edge(X, Y), edge(Y, Z).";
    let want = ["1,1".to_string(), "1,3".to_string(), "2,2".to_string()];
    let stored = rows_on_every_cell(query, "stored");
    let any = rows_on_every_cell(query, "any");
    for ((cell, stored), any) in CELLS.iter().zip(&stored).zip(&any) {
        assert_eq!(stored, &want, "{cell:?} under stored");
        assert_eq!(any, &want, "{cell:?} under any");
    }
}

/// A workspace with `edge.csv` (1 ↔ 2, 2 → 3) and one benchmark whose
/// query is cyclic under `stored`.
fn mutual_workspace() -> (TempDir, TempDir) {
    let workspace = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let data = workspace.path().join("data");
    fs::create_dir_all(&data).unwrap();
    write_edges(&data);
    let benchmarks = workspace.path().join("benchmarks");
    fs::create_dir_all(&benchmarks).unwrap();
    fs::write(
        benchmarks.join("mutual.yml"),
        r#"name: mutual
description: "mutual edges: cyclic under stored, one copy under any"
relations:
  - name: edge
    path: "data/edge.csv"
queries:
  - name: mutual
    description: "edges present in both directions"
    query: "Q(X, Y) :- edge(X, Y), edge(Y, X)."
    expected: 2
"#,
    )
    .unwrap();
    (workspace, cache)
}

/// `bench run mutual` in `workspace`, returning the process output and
/// the parsed report array (empty if no report was written).
fn bench_run_mutual(
    workspace: &TempDir, cache: &TempDir, args: &[&str],
) -> (std::process::Output, Vec<serde_json::Value>) {
    let report = workspace.path().join("report.json");
    let output = Command::new(kermit_bin())
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LIB_BACKTRACE", "0")
        .env("KERMIT_WORKSPACE", workspace.path())
        .env("XDG_CACHE_HOME", cache.path())
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
        .args(["run", "mutual"])
        .args(args)
        .output()
        .unwrap();
    let reports = fs::read_to_string(&report)
        .ok()
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap_or_default();
    (output, reports)
}

fn functions_of(report: &serde_json::Value) -> Vec<String> {
    report["criterion_groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["function"].as_str().unwrap().to_string())
        .collect()
}

fn has_index_line(report: &serde_json::Value, value: &str) -> bool {
    report["metadata"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["label"] == "index" && m["value"] == value)
}

#[test]
fn bench_run_under_any_times_the_copies_and_measures_their_space() {
    let (workspace, cache) = mutual_workspace();
    let (output, reports) = bench_run_mutual(&workspace, &cache, &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "--column-orders",
        "any",
        "--verify",
        "-m",
        "insertion",
        "iteration",
        "space",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(reports.len(), 1, "{reports:?}");
    let functions = functions_of(&reports[0]);
    for function in [
        "insertion",
        "copies",
        "iteration",
        "space/edge",
        "space/Index_1_0_edge",
    ] {
        assert!(
            functions.contains(&function.to_string()),
            "{function} missing from {functions:?}"
        );
    }
    assert_eq!(reports[0]["axes"]["column_orders"], "any");
    assert_eq!(reports[0]["axes"]["verified"], true);
    assert!(has_index_line(&reports[0], "edge (1, 0)"), "{reports:?}");
    assert!(stderr.contains("edge (1, 0)"), "{stderr}");
}

/// A lazy `HashTrie`'s joins expand it, so `--verify`, `iteration` and
/// `end_to_end` run on fresh engines (#92), which need the query's copies
/// too, and `space` must measure the loaded copy as built.
#[test]
fn bench_run_under_any_on_a_lazy_cell_runs_every_metric_over_copies() {
    let (workspace, cache) = mutual_workspace();
    let (output, reports) = bench_run_mutual(&workspace, &cache, &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "--ds-layout-expansion",
        "lazy",
        "--column-orders",
        "any",
        "--verify",
        "-m",
        "insertion",
        "iteration",
        "end-to-end",
        "space",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let functions = functions_of(&reports[0]);
    for function in [
        "copies",
        "iteration",
        "end_to_end",
        "space/edge",
        "space/Index_1_0_edge",
    ] {
        assert!(
            functions.contains(&function.to_string()),
            "{function} missing from {functions:?}"
        );
    }
    assert_eq!(reports[0]["axes"]["verified"], true);
    assert_eq!(reports[0]["axes"]["ds_layout_expansion"], "lazy");
}

#[test]
fn bench_run_under_stored_still_rejects_the_cyclic_query() {
    let (workspace, cache) = mutual_workspace();
    let (output, reports) = bench_run_mutual(&workspace, &cache, &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "-m",
        "iteration",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("contradictory orders"), "{stderr}");
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn bench_run_under_any_emits_no_copies_when_the_plan_agrees() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "-m",
        "insertion",
        "space",
        "--column-orders",
        "any",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = common::cli::reports_of(&report);
    let functions = functions_of(&reports[0]);
    assert!(!functions.iter().any(|f| f == "copies"), "{functions:?}");
    assert!(
        !functions.iter().any(|f| f.starts_with("space/Index_")),
        "{functions:?}"
    );
    assert!(
        !reports[0]["metadata"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["label"] == "index"),
        "{reports:?}"
    );
    assert_eq!(reports[0]["axes"]["column_orders"], "any");
}
