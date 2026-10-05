//! CLI smoke test for the sorted tries' `ds_build_mode` axis (issues #84,
//! #94). Every `ColumnTrie` report says which build made its relations, so
//! kermit-lab can read a `ColumnTrie` report *without* the axis as the pre-#84
//! incremental build; every `TreeTrie` report does too (`serial` unless
//! `--ds-build parallel:N`). `HashTrie` has a single build and carries no such
//! axis until #94's second plan. It also covers selecting `ColumnTrie`'s mode
//! with `--ds-build` and rejecting its values on other structures.

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
fn cli_bench_ds_tree_trie_records_serial_and_hash_trie_no_build_mode() {
    let (output, report) = bench_ds("tree-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "serial");

    let (output, report) = bench_ds("hash-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let axes = axes_of(&report);
    assert!(axes.get("ds_build_mode").is_none(), "{axes}");
}

#[test]
fn cli_bench_run_sweep_reports_each_cells_build_mode() {
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
            | Some("HashTrie") => assert!(axes.get("ds_build_mode").is_none(), "{axes}"),
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
    let (output, report) = bench_ds("column-trie", &["--ds-build", "incremental"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "incremental");
}

#[test]
fn cli_bench_ds_rejects_bulk_off_column_trie() {
    for ds in ["tree-trie", "hash-trie"] {
        let (output, _) = bench_ds(ds, &["--ds-build", "bulk"]);
        assert!(!output.status.success(), "{ds} accepted --ds-build");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--ds-build bulk"), "{ds}: {stderr}");
        assert!(stderr.contains("column-trie"), "{ds}: {stderr}");
    }
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
        "incremental",
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
            | Some("HashTrie") => assert!(axes.get("ds_build_mode").is_none(), "{axes}"),
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
        "incremental",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "incremental");
}
