//! Every query shape from issue #78's table, through the real binary, on
//! all ten cells: TreeTrie and ColumnTrie under LFTJ, and HashTrie under
//! Hash Triejoin with each hasher × pruning × expansion Layout.
//!
//! A query that cannot run must exit 1 — never 101, the panic exit — with
//! a message naming the problem, byte-identical on every cell, and write no
//! CSV. A placeholder query must return the same rows on every cell. The
//! child runs with `RUST_BACKTRACE=0`: CI sets it to 1, which makes anyhow
//! append a backtrace whose frames differ per monomorphisation.

use {
    std::{
        fs,
        path::{Path, PathBuf},
        process::{Command, Output},
    },
    tempfile::TempDir,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

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

/// The triangle benchmark's edges (sources 1–4; vertex 1 has four
/// out-edges) and the ternary `r` from issue #73.
fn write_relations(dir: &Path) -> [PathBuf; 2] {
    let edge = dir.join("edge.csv");
    fs::write(&edge, "src,dst\n1,2\n1,3\n1,4\n1,5\n2,3\n2,4\n3,5\n4,5\n").unwrap();
    let r = dir.join("r.csv");
    fs::write(&r, "a,b,c\n1,2,3\n1,4,5\n").unwrap();
    [edge, r]
}

/// Runs `kermit join` on `query` once per cell.
fn join_on_every_cell(query: &str) -> Vec<Output> {
    let dir = TempDir::new().unwrap();
    let relations = write_relations(dir.path());
    let query_path = dir.path().join("q.dl");
    fs::write(&query_path, query).unwrap();
    CELLS
        .iter()
        .map(|cell| {
            Command::new(kermit_bin())
                .env("RUST_BACKTRACE", "0")
                .env("RUST_LIB_BACKTRACE", "0")
                .arg("join")
                .arg("-r")
                .args(&relations)
                .arg("-q")
                .arg(&query_path)
                .args(*cell)
                .output()
                .unwrap()
        })
        .collect()
}

/// Asserts that every cell rejects `query` with exit 1, no stdout, and one
/// identical stderr containing `fragment`.
fn assert_rejected_everywhere(query: &str, fragment: &str) {
    let outputs = join_on_every_cell(query);
    let first = String::from_utf8_lossy(&outputs[0].stderr).into_owned();
    for (cell, output) in CELLS.iter().zip(&outputs) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{query} on {cell:?}: expected exit 1; stderr: {stderr}"
        );
        assert!(
            output.stdout.is_empty(),
            "{query} on {cell:?}: a rejected query must write no CSV"
        );
        assert_eq!(
            stderr, first,
            "{query}: {cell:?} reports differently from {:?}",
            CELLS[0]
        );
    }
    assert!(
        first.starts_with("Error: "),
        "{query}: not an anyhow error: {first}"
    );
    assert!(
        first.contains(fragment),
        "{query}: expected {fragment:?} in {first}"
    );
}

/// Asserts that every cell accepts `query` and returns `header` and
/// `rows` (as a multiset; the hash family's row order is unspecified).
fn assert_rows_everywhere(query: &str, header: &str, rows: &[&str]) {
    let mut want: Vec<&str> = rows.to_vec();
    want.sort();
    for (cell, output) in CELLS.iter().zip(join_on_every_cell(query)) {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{query} on {cell:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut lines = stdout.lines();
        assert_eq!(lines.next(), Some(header), "{query} on {cell:?}");
        let mut got: Vec<&str> = lines.collect();
        got.sort();
        assert_eq!(got, want, "{query} on {cell:?}");
    }
}

/// Row 1: the sorted family panicked (subtract with overflow), the hash
/// family printed only a header.
#[test]
fn row1_unbound_head_variable() {
    assert_rejected_everywhere(
        "Q(X, Y) :- edge(X, Z).",
        "head variable `Y` does not appear in the body",
    );
}

/// Row 2: printed the header `X,X` over `(X, Y)` rows.
#[test]
fn row2_repeated_head_variable() {
    assert_rejected_everywhere(
        "Q(X, X) :- edge(X, Y).",
        "head variable `X` appears more than once",
    );
}

/// Row 3: returned zero rows, indistinguishable from an empty result.
#[test]
fn row3_atom_wider_than_its_relation() {
    assert_rejected_everywhere(
        "Q(X, Y, Z) :- edge(X, Y, Z).",
        "atom `edge(X, Y, Z)` has 3 term(s), but relation \"edge\" has arity 2",
    );
}

/// Row 4: the sorted family returned the sources, the hash family
/// panicked.
#[test]
fn row4_atom_narrower_than_its_relation() {
    assert_rejected_everywhere(
        "Q(X) :- edge(X).",
        "atom `edge(X)` has 1 term(s), but relation \"edge\" has arity 2",
    );
}

/// Row 5: `_` is a fresh unused variable (bag semantics, decided for
/// #78), so each source repeats once per out-edge, on every cell. The
/// sorted family used to return each source once; the hash family
/// panicked.
#[test]
fn row5_trailing_placeholder_returns_a_row_per_tuple() {
    assert_rows_everywhere("Q(X) :- edge(X, _).", "X", &[
        "1", "1", "1", "1", "2", "2", "3", "4",
    ]);
}

/// #73's middle placeholder: `Y` names the third column. The sorted family
/// returned the second; the hash family panicked.
#[test]
fn middle_placeholder_keeps_later_columns_in_place() {
    assert_rows_everywhere("Q(X, Y) :- r(X, _, Y).", "X,Y", &["1,3", "1,5"]);
}

/// #71: a body-only variable no longer leaks as an unlabelled column.
#[test]
fn rows_carry_only_the_head_columns() {
    assert_rows_everywhere("Q(Y) :- edge(X, Y).", "Y", &[
        "2", "3", "3", "4", "4", "5", "5", "5",
    ]);
}

/// Row 6: panicked with this message; now the same message is an error.
#[test]
fn row6_unknown_relation() {
    assert_rejected_everywhere(
        "Q(X, Y) :- nope(X, Y).",
        r#"query body references unknown relation "nope"; known relations: ["edge", "r"]"#,
    );
}

#[test]
fn malformed_constant() {
    assert_rejected_everywhere(
        "Q(X) :- edge(X, cfoo).",
        "atom \"cfoo\" does not match the expected c<digits> shape",
    );
}

/// A user atom named like a synthetic predicate used to be skipped as
/// synthetic, then panic in the executor ("Missing datastructure").
#[test]
fn reserved_relation_name() {
    assert_rejected_everywhere(
        "Q(X) :- Const_c5(X).",
        "relation name \"Const_c5\" is reserved",
    );
}

/// Head placeholders and constants printed a header over the wrong column.
#[test]
fn non_variable_head_terms() {
    assert_rejected_everywhere("Q(_) :- edge(X, Y).", "head term `_` is not a variable");
    assert_rejected_everywhere(
        "Q(c5, X) :- edge(X, Y).",
        "head term `c5` is not a variable",
    );
}

/// A relation with a mutual pair (1 and 2), a one-way edge and a self-loop.
const MUTUAL_EDGES: &str = "src,dst\n1,2\n2,1\n2,3\n3,3\n";

/// Both `edge(X, Y)` and `edge(Y, X)` hold. The query reads `edge` in two
/// column orders, which `--column-orders stored` rejected as a limitation;
/// every query now builds each index in its plan's order, so it just runs.
const MUTUAL_QUERY: &str = "Q(X, Y) :- edge(X, Y), edge(Y, X).";

/// [`MUTUAL_QUERY`]'s answer over [`MUTUAL_EDGES`], sorted.
const MUTUAL_ROWS: [&str; 3] = ["1,2", "2,1", "3,3"];

/// Writes [`MUTUAL_EDGES`] and [`MUTUAL_QUERY`] into `dir`; returns their
/// paths.
fn write_mutual_fixture(dir: &Path) -> (PathBuf, PathBuf) {
    let edge = dir.join("edge.csv");
    fs::write(&edge, MUTUAL_EDGES).unwrap();
    let query = dir.join("mutual.dl");
    fs::write(&query, MUTUAL_QUERY).unwrap();
    (edge, query)
}

/// Splits CSV output into its header line and its data lines, sorted (the
/// hash family's row order is unspecified).
fn header_and_sorted_rows(csv: &str) -> (&str, Vec<&str>) {
    let mut lines = csv.lines();
    let header = lines.next().expect("a header line");
    let mut rows: Vec<&str> = lines.collect();
    rows.sort_unstable();
    (header, rows)
}

/// The mutual-edge query needs `edge` in two column orders. It used to be
/// rejected as a limitation; now it runs, with the same answer, on every cell.
#[test]
fn mutual_edges_run_on_every_cell() {
    let dir = TempDir::new().unwrap();
    let (edge, query) = write_mutual_fixture(dir.path());
    for cell in CELLS {
        let output = Command::new(kermit_bin())
            .env("RUST_BACKTRACE", "0")
            .env("RUST_LIB_BACKTRACE", "0")
            .arg("join")
            .arg("-r")
            .arg(&edge)
            .arg("-q")
            .arg(&query)
            .args(cell)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{cell:?}: {stderr}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let (header, rows) = header_and_sorted_rows(&stdout);
        assert_eq!(header, "X,Y", "{cell:?}");
        assert_eq!(rows, MUTUAL_ROWS, "{cell:?}");
    }
}

/// `--column-orders` is gone from `join` and `bench join` (and `bench run`,
/// which shares `PlannerArgs`): clap rejects it as an unknown argument.
#[test]
fn column_orders_flag_is_gone() {
    let dir = TempDir::new().unwrap();
    let (edge, query) = write_mutual_fixture(dir.path());
    let report = dir.path().join("report.json");
    let edge = edge.to_str().unwrap();
    let query = query.to_str().unwrap();
    let report = report.to_str().unwrap();
    let invocations: [Vec<&str>; 2] = [
        vec![
            "join",
            "-r",
            edge,
            "-q",
            query,
            "-i",
            "tree-trie",
            "-a",
            "leapfrog-triejoin",
            "--column-orders",
            "any",
        ],
        vec![
            "bench",
            "--report-json",
            report,
            "join",
            "-r",
            edge,
            "-q",
            query,
            "-i",
            "tree-trie",
            "-a",
            "leapfrog-triejoin",
            "-m",
            "space",
            "--column-orders",
            "any",
        ],
    ];
    for args in &invocations {
        let output = Command::new(kermit_bin())
            .env("RUST_BACKTRACE", "0")
            .args(args)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {stderr}");
        assert!(stderr.contains("--column-orders"), "{args:?}: {stderr}");
        assert!(output.stdout.is_empty(), "{args:?}: wrote to stdout");
    }
}

/// A relation file the query does not name: its header is valid, its only
/// row is not a number, so loading it would fail.
fn write_unreadable_unnamed_relation(dir: &Path) -> PathBuf {
    let bad = dir.join("bad.csv");
    fs::write(&bad, "a,b\n1,x\n").unwrap();
    bad
}

/// `kermit join` reads only the relations its query names; the others are
/// validated from their headers and left alone.
#[test]
fn join_reads_only_the_relations_the_query_names() {
    let dir = TempDir::new().unwrap();
    let (edge, query) = write_mutual_fixture(dir.path());
    let bad = write_unreadable_unnamed_relation(dir.path());
    for cell in [CELLS[0], CELLS[2]] {
        let output = Command::new(kermit_bin())
            .env("RUST_BACKTRACE", "0")
            .env("RUST_LIB_BACKTRACE", "0")
            .arg("join")
            .arg("-r")
            .arg(&edge)
            .arg(&bad)
            .arg("-q")
            .arg(&query)
            .args(cell)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{cell:?}: {stderr}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let (header, rows) = header_and_sorted_rows(&stdout);
        assert_eq!(header, "X,Y", "{cell:?}");
        assert_eq!(rows, MUTUAL_ROWS, "{cell:?}");
    }
}

/// `bench join --output` writes the CSV and runs the benchmark from one
/// prepared workload: the CSV holds the answer, and the report names the
/// same query and cell. The unnamed unreadable relation is never loaded
/// for either.
#[test]
fn bench_join_output_and_benchmark_share_one_prepared_workload() {
    let dir = TempDir::new().unwrap();
    let (edge, query) = write_mutual_fixture(dir.path());
    let bad = write_unreadable_unnamed_relation(dir.path());
    for (structure, algorithm) in [
        ("tree-trie", "leapfrog-triejoin"),
        ("column-trie", "leapfrog-triejoin"),
        ("hash-trie", "hash-triejoin"),
    ] {
        let csv = dir.path().join(format!("{structure}.csv"));
        let report = dir.path().join(format!("{structure}.json"));
        let output = Command::new(kermit_bin())
            .env("RUST_BACKTRACE", "0")
            .env("RUST_LIB_BACKTRACE", "0")
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
            .arg("join")
            .arg("--relations")
            .arg(&edge)
            .arg(&bad)
            .arg("--query")
            .arg(&query)
            .args(["-i", structure, "-a", algorithm, "-m", "space", "--output"])
            .arg(&csv)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{structure}: {stderr}");

        let text = fs::read_to_string(&csv).unwrap();
        let (header, rows) = header_and_sorted_rows(&text);
        assert_eq!(header, "X,Y", "{structure}");
        assert_eq!(rows, MUTUAL_ROWS, "{structure}");

        let reports: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
        let axes = &reports[0]["axes"];
        assert_eq!(axes["query"], "mutual", "{structure}");
        assert_eq!(axes["benchmark"], "adhoc", "{structure}");
    }
}

/// A rejected query leaves no CSV behind, and says why: the output file is
/// created only after the query has been prepared.
#[test]
fn bench_join_output_rejects_a_bad_query_before_creating_the_file() {
    let dir = TempDir::new().unwrap();
    let (edge, _) = write_mutual_fixture(dir.path());
    let query = dir.path().join("unbound.dl");
    fs::write(&query, "Q(X, Y) :- edge(X, Z).").unwrap();
    let csv = dir.path().join("out.csv");
    let output = Command::new(kermit_bin())
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LIB_BACKTRACE", "0")
        .args(["bench", "--report-json"])
        .arg(dir.path().join("report.json"))
        .arg("join")
        .arg("--relations")
        .arg(&edge)
        .arg("--query")
        .arg(&query)
        .args([
            "-i",
            "tree-trie",
            "-a",
            "leapfrog-triejoin",
            "-m",
            "space",
            "--output",
        ])
        .arg(&csv)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        stderr.contains("head variable `Y` does not appear in the body"),
        "{stderr}"
    );
    assert!(!csv.exists(), "a rejected query must not create the CSV");
}

/// `bench run` validates every query of a workload before it loads or
/// times anything: the valid first query never reaches Criterion when the
/// second cannot run.
#[test]
fn bench_run_rejects_a_bad_query_before_timing_anything() {
    let workspace = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let data = workspace.path().join("data");
    fs::create_dir_all(&data).unwrap();
    write_relations(&data);
    let benchmarks = workspace.path().join("benchmarks");
    fs::create_dir_all(&benchmarks).unwrap();
    fs::write(
        benchmarks.join("malformed.yml"),
        r#"name: malformed
description: "one runnable query, then one that cannot run"
relations:
  - name: edge
    path: "data/edge.csv"
queries:
  - name: fine
    description: "valid"
    query: "Q(X, Y) :- edge(X, Y)."
  - name: unbound
    description: "Y is not bound"
    query: "Q(X, Y) :- edge(X, Z)."
"#,
    )
    .unwrap();
    let report = workspace.path().join("report.json");
    let output = Command::new(kermit_bin())
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LIB_BACKTRACE", "0")
        .env("KERMIT_WORKSPACE", workspace.path())
        .env("XDG_CACHE_HOME", cache.path())
        .args(["bench", "--report-json"])
        .arg(&report)
        .args([
            "run",
            "malformed",
            "-i",
            "tree-trie",
            "-a",
            "leapfrog-triejoin",
            "-m",
            "iteration",
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        stderr.contains(
            "benchmark 'malformed' query 'unbound': head variable `Y` does not appear in the body"
        ),
        "{stderr}"
    );
    assert!(
        !stderr.contains("bench run metadata"),
        "the valid query must not have started timing: {stderr}"
    );
}
