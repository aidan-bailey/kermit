//! CLI smoke test for the `hash-index` structure and the binary hash join:
//! `--ds-layout-hasher` reaches the structure and its report axis, a report
//! names `HashIndex` / `BinaryHashJoin` and carries no other `ds_*` axis, the
//! cell verifies the triangle benchmark's answer, and every other `--ds-*`
//! flag, and every other (structure, algorithm) pair with the cell's two
//! halves, is a usage error.

mod common;

use {
    common::cli::{axes_of, bench_ds, bench_join, bench_run, kermit_bin},
    std::{path::PathBuf, process::Command},
};

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: &std::process::Output) {
    assert!(output.status.success(), "{}", stderr_of(output));
}

/// Every `ds_*` key of `axes`, sorted.
fn ds_axes(axes: &serde_json::Value) -> Vec<&str> {
    let mut keys: Vec<&str> = axes
        .as_object()
        .expect("axes is an object")
        .keys()
        .map(String::as_str)
        .filter(|key| key.starts_with("ds_"))
        .collect();
    keys.sort_unstable();
    keys
}

#[test]
fn cli_bench_ds_hash_index_default_hasher_is_sip() {
    let (output, report) = bench_ds("hash-index", &[]);
    assert_success(&output);
    let axes = axes_of(&report);
    assert_eq!(axes["data_structure"], "HashIndex", "{axes}");
    assert_eq!(axes["ds_layout_hasher"], "sip", "{axes}");
    assert_eq!(ds_axes(&axes), ["ds_layout_hasher"], "{axes}");
}

#[test]
fn cli_bench_ds_hash_index_with_fxhash_records_axis() {
    let (output, report) = bench_ds("hash-index", &["--ds-layout-hasher", "fxhash"]);
    assert_success(&output);
    let axes = axes_of(&report);
    assert_eq!(axes["ds_layout_hasher"], "fxhash", "{axes}");
    assert_eq!(ds_axes(&axes), ["ds_layout_hasher"], "{axes}");
}

/// `bench run` on the triangle benchmark, verified against its `expected`
/// count: the hash index and the binary hash join answer it.
#[test]
fn cli_bench_run_hash_index_verifies_the_triangle() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "hash-index",
        "-a",
        "binary-hash-join",
        "-m",
        "space",
        "--verify",
        "--ds-layout-hasher",
        "fxhash",
    ]);
    assert_success(&output);
    let axes = axes_of(&report);
    assert_eq!(axes["data_structure"], "HashIndex", "{axes}");
    assert_eq!(axes["algorithm"], "BinaryHashJoin", "{axes}");
    assert_eq!(axes["ds_layout_hasher"], "fxhash", "{axes}");
    assert_eq!(axes["verified"], true, "{axes}");
    assert_eq!(ds_axes(&axes), ["ds_layout_hasher"], "{axes}");
}

#[test]
fn cli_bench_join_hash_index_reports_the_cell() {
    let (output, report) = bench_join("hash-index", "binary-hash-join", &["-m", "space"]);
    assert_success(&output);
    let axes = axes_of(&report);
    assert_eq!(axes["benchmark"], "adhoc", "{axes}");
    assert_eq!(axes["data_structure"], "HashIndex", "{axes}");
    assert_eq!(axes["algorithm"], "BinaryHashJoin", "{axes}");
    assert_eq!(axes["ds_layout_hasher"], "sip", "{axes}");
}

/// `kermit join` writes the same rows on the hash index as on the hash trie.
#[test]
fn cli_join_hash_index_writes_the_rows_the_hash_trie_writes() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let rows = |structure: &str, algorithm: &str| {
        let output = Command::new(kermit_bin())
            .arg("join")
            .arg("-r")
            .arg(fixtures.join("first.csv"))
            .arg(fixtures.join("second.csv"))
            .arg("-q")
            .arg(fixtures.join("intersect_query.dl"))
            .args(["-i", structure, "-a", algorithm])
            .output()
            .expect("failed to execute kermit binary");
        assert_success(&output);
        let mut lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_owned)
            .collect();
        lines.sort();
        lines
    };
    let hash_index = rows("hash-index", "binary-hash-join");
    assert_eq!(hash_index, ["2", "3", "X"]);
    assert_eq!(hash_index, rows("hash-trie", "hash-triejoin"));
}

/// Every `--ds-*` flag but `--ds-layout-hasher` is refused on `hash-index`,
/// with the usage error naming the structures that have its axis.
#[test]
fn cli_hash_index_refuses_every_other_ds_flag() {
    let refused: [(&[&str], &str); 7] = [
        (
            &["--ds-layout-pruning", "on"],
            "--ds-layout-pruning is only valid with --indexstructure hash-trie",
        ),
        (
            &["--ds-layout-expansion", "lazy"],
            "--ds-layout-expansion is only valid with --indexstructure hash-trie",
        ),
        (
            &["--ds-layout-levels", "keys"],
            "--ds-layout-levels is only valid with --indexstructure hash-trie",
        ),
        (
            &["--ds-layout-seek", "galloping"],
            "--ds-layout-seek is only valid with --indexstructure tree-trie or column-trie",
        ),
        (
            &["--ds-config", "load-factor=0.5"],
            "--ds-config is only valid with --indexstructure hash-trie",
        ),
        (
            &["--ds-build", "hash-trie=radix:4"],
            "--ds-build hash-trie is only valid with --indexstructure hash-trie",
        ),
        (
            &["--ds-build", "hash-index=bulk"],
            "--ds-build: hash-index has a single build process",
        ),
    ];
    for (args, fragment) in refused {
        let (output, _) = bench_join("hash-index", "binary-hash-join", args);
        let stderr = stderr_of(&output);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {stderr}");
        assert!(
            stderr.contains(fragment),
            "{args:?}: expected {fragment:?} in {stderr}"
        );
    }
}

/// The hash index pairs only with the binary hash join, and the binary hash
/// join only with the hash index.
#[test]
fn cli_hash_index_incompatible_pairs_are_usage_errors() {
    for (structure, algorithm, fragment) in [
        (
            "hash-index",
            "hash-triejoin",
            "incompatible selection: HashIndex cannot run under HashTriejoin",
        ),
        (
            "hash-index",
            "leapfrog-triejoin",
            "incompatible selection: HashIndex cannot run under LeapfrogTriejoin",
        ),
        (
            "tree-trie",
            "binary-hash-join",
            "incompatible selection: TreeTrie cannot run under BinaryHashJoin",
        ),
        (
            "hash-trie",
            "binary-hash-join",
            "incompatible selection: HashTrie cannot run under BinaryHashJoin",
        ),
    ] {
        let (output, _) = bench_join(structure, algorithm, &[]);
        let stderr = stderr_of(&output);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{structure} {algorithm}: {stderr}"
        );
        assert!(
            stderr.contains(fragment),
            "{structure} {algorithm}: expected {fragment:?} in {stderr}"
        );
    }
}
