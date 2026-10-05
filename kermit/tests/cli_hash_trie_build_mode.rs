//! CLI smoke test for `HashTrie`'s `ds_build_mode` axis (issues #91, #94).
//! Every `HashTrie` report says which build made its relations: `serial` by
//! default, `radix:<bits>` or `parallel:<threads>` under `--ds-build
//! hash-trie=…`. Every mode builds the identical trie, so only the axis (and
//! build time) shows which ran.

mod common;

use common::cli::{axes_of, bench_ds, bench_join, bench_run, reports_of};

#[test]
fn cli_bench_ds_hash_trie_records_serial_build_mode() {
    let (output, report) = bench_ds("hash-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "serial");
}

#[test]
fn cli_bench_ds_with_radix_build_records_axis() {
    let (output, report) = bench_ds("hash-trie", &["--ds-build", "hash-trie=radix:4"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "radix:4");
}

#[test]
fn cli_bench_join_with_radix_build_records_axis() {
    let (output, report) = bench_join("hash-trie", "hash-triejoin", &[
        "-m",
        "space",
        "--ds-build",
        "hash-trie=radix:4",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "radix:4");
}

#[test]
fn cli_bench_run_sweep_carries_each_build_mode_to_its_cell() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-build",
        "hash-trie=radix:4,tree-trie=parallel:2",
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
            | Some("HashTrie") => assert_eq!(axes["ds_build_mode"], "radix:4", "{axes}"),
            | Some("TreeTrie") => assert_eq!(axes["ds_build_mode"], "parallel:2", "{axes}"),
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}

#[test]
fn cli_bench_ds_with_parallel_build_records_axis() {
    let (output, report) = bench_ds("hash-trie", &["--ds-build", "hash-trie=parallel:2"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "parallel:2");
}

/// `--verify` checks the answer counts of parallel-built tries against the
/// benchmark's expected values, eager and lazy.
#[test]
fn cli_bench_run_verifies_answers_from_parallel_built_tries() {
    for expansion in ["eager", "lazy"] {
        let (output, report) = bench_run("triangle", &[
            "-i",
            "hash-trie",
            "-a",
            "hash-triejoin",
            "-m",
            "iteration",
            "--verify",
            "--ds-layout-expansion",
            expansion,
            "--ds-build",
            "hash-trie=parallel:2",
        ]);
        assert!(
            output.status.success(),
            "{expansion}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let axes = axes_of(&report);
        assert_eq!(axes["ds_build_mode"], "parallel:2", "{expansion}: {axes}");
        assert_eq!(axes["verified"], true, "{expansion}: {axes}");
    }
}

/// The same mode name reaches each trie with that trie's own thread count.
#[test]
fn cli_bench_run_sweep_carries_each_structures_parallel_mode() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-build",
        "hash-trie=parallel:3,tree-trie=parallel:2",
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
            | Some("HashTrie") => assert_eq!(axes["ds_build_mode"], "parallel:3", "{axes}"),
            | Some("TreeTrie") => assert_eq!(axes["ds_build_mode"], "parallel:2", "{axes}"),
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}

#[test]
fn cli_bench_ds_rejects_malformed_hash_trie_modes() {
    for mode in [
        "radix",
        "radix:0",
        "radix:17",
        "bulk",
        "parallel",
        "parallel:0",
        "parallel:1025",
        "parallel:x",
    ] {
        let pair = format!("hash-trie={mode}");
        let (output, _) = bench_ds("hash-trie", &["--ds-build", &pair]);
        assert!(!output.status.success(), "{pair} was accepted");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--ds-build hash-trie"), "{pair}: {stderr}");
        assert!(
            stderr.contains("expected serial, radix:<bits> or parallel:<threads>"),
            "{pair}: {stderr}"
        );
    }
}
