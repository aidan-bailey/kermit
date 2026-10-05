//! CLI smoke test for `TreeTrie`'s `ds_build_mode` axis (issue #94):
//! `--ds-build serial | parallel:N` selects the build, every `TreeTrie` report
//! records it, and a value is rejected on a structure without it.

mod common;

use common::cli::{axes_of, bench_ds, bench_join, bench_run, reports_of};

#[test]
fn cli_bench_ds_tree_trie_records_a_parallel_build() {
    let (output, report) = bench_ds("tree-trie", &["--ds-build", "parallel:2"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "parallel:2");
}

#[test]
fn cli_bench_join_tree_trie_records_a_parallel_build() {
    let (output, report) = bench_join("tree-trie", "leapfrog-triejoin", &[
        "-m",
        "space",
        "--ds-build",
        "parallel:3",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "parallel:3");
}

/// `--verify` checks the answer counts of the parallel-built tries against
/// the benchmark's expected values.
#[test]
fn cli_bench_run_verifies_answers_from_parallel_built_tries() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "-m",
        "iteration",
        "--verify",
        "--ds-build",
        "parallel:2",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let axes = axes_of(&report);
    assert_eq!(axes["ds_build_mode"], "parallel:2", "{axes}");
    assert_eq!(axes["verified"], true, "{axes}");
}

#[test]
fn cli_bench_run_sweep_carries_parallel_only_to_tree_trie_cells() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-build",
        "parallel:2",
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
            | Some("TreeTrie") => assert_eq!(axes["ds_build_mode"], "parallel:2", "{axes}"),
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | Some("HashTrie") => assert!(axes.get("ds_build_mode").is_none(), "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}

/// Until #94's second plan only `TreeTrie` has `parallel:N`.
#[test]
fn cli_bench_ds_rejects_parallel_off_tree_trie() {
    for ds in ["column-trie", "hash-trie"] {
        let (output, _) = bench_ds(ds, &["--ds-build", "parallel:2"]);
        assert!(!output.status.success(), "{ds} accepted parallel:2");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--ds-build parallel:2"), "{ds}: {stderr}");
        assert!(stderr.contains("tree-trie"), "{ds}: {stderr}");
    }
}

#[test]
fn cli_rejects_malformed_parallel_values() {
    for bad in ["parallel", "parallel:0", "parallel:1025", "parallel:x"] {
        let (output, _) = bench_ds("tree-trie", &["--ds-build", bad]);
        assert!(!output.status.success(), "accepted {bad}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("parallel:N"), "{bad}: {stderr}");
    }
}
