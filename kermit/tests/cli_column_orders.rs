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
