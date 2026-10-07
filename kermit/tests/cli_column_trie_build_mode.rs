//! CLI smoke test for `ColumnTrie`'s `ds_build_mode` axis (issue #84), and
//! for every structure reporting its own. Every `ColumnTrie` report says which
//! build made its relations, so kermit-lab can read a `ColumnTrie` report
//! *without* the axis as the pre-#84 incremental build. `TreeTrie` and
//! `HashTrie` report theirs (`serial` and `bulk` by default — see
//! `cli_tree_trie_build_mode.rs` and `cli_hash_trie_build_mode.rs`). It also
//! covers selecting the mode with `--ds-build` and rejecting a column-trie
//! pair on another structure.

mod common;

use common::cli::{axes_of, bench_ds, bench_join, bench_run, reports_of};

#[test]
fn cli_bench_ds_column_trie_records_bulk_build_mode() {
    let (output, report) = bench_ds("column-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "bulk");
}

#[test]
fn cli_bench_ds_tree_trie_records_serial_build_mode() {
    let (output, report) = bench_ds("tree-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "serial");
}

#[test]
fn cli_bench_run_sweep_reports_each_structures_build_mode() {
    let (output, report) = bench_run("triangle", &["-i", "all", "-a", "all", "-m", "space"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        match axes["data_structure"].as_str() {
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | Some("TreeTrie") => assert_eq!(axes["ds_build_mode"], "serial", "{axes}"),
            | Some("HashTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}

#[test]
fn cli_bench_join_column_trie_records_bulk_build_mode() {
    let (output, report) = bench_join("column-trie", "leapfrog-triejoin", &["-m", "space"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "bulk");
}

#[test]
fn cli_bench_ds_with_incremental_build_records_axis() {
    let (output, report) = bench_ds("column-trie", &["--ds-build", "column-trie=incremental"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "incremental");
}

#[test]
fn cli_bench_ds_rejects_a_column_trie_pair_off_column_trie() {
    for ds in ["tree-trie", "hash-trie"] {
        let (output, _) = bench_ds(ds, &["--ds-build", "column-trie=bulk"]);
        assert!(!output.status.success(), "{ds} accepted a column-trie pair");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--ds-build column-trie"), "{ds}: {stderr}");
    }
}

/// The pre-#91 bare spelling is a usage error that names the keyed one.
#[test]
fn cli_bench_ds_rejects_the_bare_spelling_with_a_hint() {
    let (output, _) = bench_ds("column-trie", &["--ds-build", "incremental"]);
    assert!(!output.status.success(), "bare --ds-build was accepted");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("column-trie=incremental"), "{stderr}");
}

#[test]
fn cli_bench_run_sweep_carries_incremental_only_to_column_trie_cells() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-build",
        "column-trie=incremental",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        match axes["data_structure"].as_str() {
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "incremental", "{axes}"),
            | Some("TreeTrie") => assert_eq!(axes["ds_build_mode"], "serial", "{axes}"),
            | Some("HashTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}

#[test]
fn cli_bench_join_with_incremental_build_records_axis() {
    let (output, report) = bench_join("column-trie", "leapfrog-triejoin", &[
        "-m",
        "space",
        "--ds-build",
        "column-trie=incremental",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "incremental");
}
