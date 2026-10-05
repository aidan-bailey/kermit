//! CLI smoke test for `TreeTrie`'s `ds_build_mode` axis (issue #94):
//! `--ds-build tree-trie=serial|parallel:<threads>` selects the build, every
//! `TreeTrie` report records it, and the pair is rejected when `-i` does not
//! select `tree-trie`.

mod common;

use common::cli::{axes_of, bench_ds, bench_join, bench_run, reports_of};

#[test]
fn cli_bench_ds_tree_trie_records_a_parallel_build() {
    let (output, report) = bench_ds("tree-trie", &["--ds-build", "tree-trie=parallel:2"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "parallel:2");
}

#[test]
fn cli_bench_join_tree_trie_records_a_parallel_build() {
    let csv = tempfile::NamedTempFile::new().expect("temp csv");
    let csv_path = csv.path().to_str().expect("utf-8 temp path");
    let (output, report) = bench_join("tree-trie", "leapfrog-triejoin", &[
        "-m",
        "space",
        "--ds-build",
        "tree-trie=parallel:3",
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
        "tree-trie=parallel:2",
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
        "tree-trie=parallel:2",
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
            | Some("HashTrie") => assert_eq!(axes["ds_build_mode"], "serial", "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}

#[test]
fn cli_bench_ds_rejects_a_tree_trie_pair_off_tree_trie() {
    for ds in ["column-trie", "hash-trie"] {
        let (output, _) = bench_ds(ds, &["--ds-build", "tree-trie=parallel:2"]);
        assert!(!output.status.success(), "{ds} accepted a tree-trie pair");
        assert_eq!(output.status.code(), Some(1), "{ds}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--ds-build tree-trie"), "{ds}: {stderr}");
    }
}

#[test]
fn cli_bench_ds_rejects_malformed_tree_trie_modes() {
    for mode in [
        "parallel",
        "parallel:0",
        "parallel:1025",
        "parallel:x",
        "bulk",
    ] {
        let pair = format!("tree-trie={mode}");
        let (output, _) = bench_ds("tree-trie", &["--ds-build", &pair]);
        assert!(!output.status.success(), "{pair} was accepted");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--ds-build tree-trie"), "{pair}: {stderr}");
        assert!(
            stderr.contains("expected serial or parallel:<threads>, threads in 1..=1024"),
            "{pair}: {stderr}"
        );
    }
}

/// #94's first spelling, `--ds-build parallel:2`, predates the keyed flag
/// (#91): it is a usage error that names the keyed spelling.
#[test]
fn cli_bench_ds_rejects_the_bare_spelling_with_a_hint() {
    let (output, _) = bench_ds("tree-trie", &["--ds-build", "parallel:2"]);
    assert!(!output.status.success(), "bare --ds-build was accepted");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("tree-trie=parallel:2"), "{stderr}");
}
