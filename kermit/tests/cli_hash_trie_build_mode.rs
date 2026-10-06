//! CLI smoke test for `HashTrie`'s `ds_build_mode` axis (issues #91, #94).
//! Every `HashTrie` report says which build made its relations: `serial` by
//! default, `radix:<bits>` or `parallel:<threads>` under `--ds-build
//! hash-trie=…`. Every mode builds an equivalent trie (the identical one,
//! except `parallel:N` under `root-capacity=tuples`, whose root keys may sit
//! in other buckets), so only the axis (and build time) shows which ran.

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

/// `bench join --output` builds through its own family
/// (`load_query_runner`), so this pins that route's mode, which the
/// `bench ds` and `bench run` tests do not reach.
#[test]
fn cli_bench_join_with_output_records_a_parallel_build() {
    let csv = tempfile::NamedTempFile::new().expect("temp csv");
    let csv_path = csv.path().to_str().expect("utf-8 temp path");
    let (output, report) = bench_join("hash-trie", "hash-triejoin", &[
        "-m",
        "space",
        "--ds-build",
        "hash-trie=parallel:3",
        "--output",
        csv_path,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "parallel:3");
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

/// Under `root-capacity=tuples`, `parallel:N` is the paper's presized
/// build. One report records both the config and the mode.
#[test]
fn cli_bench_ds_records_a_presized_parallel_build() {
    let (output, report) = bench_ds("hash-trie", &[
        "--ds-config",
        "root-capacity=tuples",
        "--ds-build",
        "hash-trie=parallel:2",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let axes = axes_of(&report);
    assert_eq!(axes["ds_build_mode"], "parallel:2", "{axes}");
    assert_eq!(axes["ds_config_root_capacity"], "tuples", "{axes}");
}

/// `--verify` checks the answers of presized parallel builds, eager and
/// lazy, at the paper's load factor.
#[test]
fn cli_bench_run_verifies_presized_parallel_builds() {
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
            "--ds-config",
            "root-capacity=tuples,load-factor=0.8",
            "--ds-build",
            "hash-trie=parallel:3",
        ]);
        assert!(
            output.status.success(),
            "{expansion}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let axes = axes_of(&report);
        assert_eq!(axes["verified"], true, "{expansion}: {axes}");
        assert_eq!(
            axes["ds_config_root_capacity"], "tuples",
            "{expansion}: {axes}"
        );
    }
}
