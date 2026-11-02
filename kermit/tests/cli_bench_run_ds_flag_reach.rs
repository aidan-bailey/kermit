//! CLI regression tests for issue #86: a `--ds-*` flag must reach a cell
//! that runs. `-i all` passes every flag's selector check, but `-a` can
//! then narrow the sweep to cells without the flag's axis. Such a run used
//! to exit 0 with the default axis values, silently dropping the flag; it
//! is now a usage error that writes no report. `-i all -a all` keeps a cell
//! for every flag and must still apply each one to its cell.

mod common;

use {
    common::cli::{bench_run, reports_of},
    std::{fs, process::Output},
    tempfile::NamedTempFile,
};

/// Asserts that a narrowed sweep refused `flag` before running anything:
/// non-zero exit, an error naming the flag, and an empty report file.
fn assert_rejected_unreached(output: &Output, report: &NamedTempFile, flag: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "{flag} reached no cell but the run succeeded; stderr: {stderr}"
    );
    assert!(
        stderr.contains(flag),
        "the error must name {flag}: {stderr}"
    );
    let text = fs::read_to_string(report.path()).expect("report file should exist");
    assert!(text.trim().is_empty(), "no report may be written: {text}");
}

/// Runs `bench run triangle -i all -a <algorithm> -m space` plus `extra`.
fn narrowed_sweep(algorithm: &str, extra: &[&str]) -> (Output, NamedTempFile) {
    let mut args = vec!["-i", "all", "-a", algorithm, "-m", "space"];
    args.extend_from_slice(extra);
    bench_run("triangle", &args)
}

#[test]
fn ds_config_is_rejected_when_lftj_leaves_no_hash_trie_cell() {
    let (output, report) = narrowed_sweep("leapfrog-triejoin", &["--ds-config", "load-factor=0.5"]);
    assert_rejected_unreached(&output, &report, "--ds-config");
}

#[test]
fn ds_layout_hasher_is_rejected_when_lftj_leaves_no_hash_trie_cell() {
    let (output, report) = narrowed_sweep("leapfrog-triejoin", &["--ds-layout-hasher", "fxhash"]);
    assert_rejected_unreached(&output, &report, "--ds-layout-hasher");
}

#[test]
fn ds_layout_pruning_is_rejected_when_lftj_leaves_no_hash_trie_cell() {
    let (output, report) = narrowed_sweep("leapfrog-triejoin", &["--ds-layout-pruning", "on"]);
    assert_rejected_unreached(&output, &report, "--ds-layout-pruning");
}

#[test]
fn ds_build_is_rejected_when_htj_leaves_no_column_trie_cell() {
    let (output, report) =
        narrowed_sweep("hash-triejoin", &["--ds-build", "column-trie=incremental"]);
    assert_rejected_unreached(&output, &report, "--ds-build column-trie");
}

#[test]
fn ds_build_hash_trie_is_rejected_when_lftj_leaves_no_hash_trie_cell() {
    let (output, report) =
        narrowed_sweep("leapfrog-triejoin", &["--ds-build", "hash-trie=radix:4"]);
    assert_rejected_unreached(&output, &report, "--ds-build hash-trie");
}

#[test]
fn ds_layout_seek_is_rejected_when_htj_leaves_no_sorted_trie_cell() {
    let (output, report) = narrowed_sweep("hash-triejoin", &["--ds-layout-seek", "binary"]);
    assert_rejected_unreached(&output, &report, "--ds-layout-seek");
}

/// The error says which cells did run, so the user can see why the flag
/// had nowhere to go.
#[test]
fn the_rejection_names_the_cells_that_would_have_run() {
    let (output, report) = narrowed_sweep("leapfrog-triejoin", &["--ds-config", "load-factor=0.5"]);
    assert_rejected_unreached(&output, &report, "--ds-config");
    let stderr = String::from_utf8_lossy(&output.stderr);
    for cell in [
        "(ColumnTrie, LeapfrogTriejoin)",
        "(TreeTrie, LeapfrogTriejoin)",
    ] {
        assert!(
            stderr.contains(cell),
            "the error must name {cell}: {stderr}"
        );
    }
}

/// A narrowed sweep that keeps the flag's cell still runs: `-a
/// hash-triejoin` keeps the hash-trie cell, so `--ds-config` reaches it.
#[test]
fn a_narrowed_sweep_keeping_the_flags_cell_still_runs() {
    let (output, report) = narrowed_sweep("hash-triejoin", &["--ds-config", "load-factor=0.5"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 1, "only the hash-trie cell: {reports:?}");
    assert_eq!(reports[0]["axes"]["ds_config_load_factor"], 0.5);
}

/// `-a binary-hash-join` keeps the hash-index cell, which has the hasher
/// axis, so `--ds-layout-hasher` reaches it.
#[test]
fn ds_layout_hasher_reaches_the_hash_index_cell_under_binary_hash_join() {
    let (output, report) = narrowed_sweep("binary-hash-join", &["--ds-layout-hasher", "fxhash"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 1, "only the hash-index cell: {reports:?}");
    assert_eq!(reports[0]["axes"]["data_structure"], "HashIndex");
    assert_eq!(reports[0]["axes"]["ds_layout_hasher"], "fxhash");
}

/// The hash-index cell has no other axis, so under `-a binary-hash-join`
/// every other flag reaches no cell and is a usage error.
#[test]
fn every_other_flag_is_rejected_when_binary_hash_join_leaves_only_the_hash_index_cell() {
    let rejected: [(&str, &[&str]); 7] = [
        ("--ds-layout-pruning", &["--ds-layout-pruning", "on"]),
        ("--ds-layout-expansion", &["--ds-layout-expansion", "lazy"]),
        ("--ds-layout-levels", &["--ds-layout-levels", "keys"]),
        ("--ds-layout-seek", &["--ds-layout-seek", "binary"]),
        ("--ds-config", &["--ds-config", "load-factor=0.5"]),
        ("--ds-build tree-trie", &[
            "--ds-build",
            "tree-trie=parallel:2",
        ]),
        ("--ds-build hash-trie", &["--ds-build", "hash-trie=radix:4"]),
    ];
    for (flag, args) in rejected {
        let (output, report) = narrowed_sweep("binary-hash-join", args);
        assert_rejected_unreached(&output, &report, flag);
    }
}

/// Acceptance criterion 2: under `-i all -a all` every flag has a cell, so
/// all of them at once still run, each reaching exactly its own cells.
#[test]
fn every_flag_reaches_its_cell_under_all_all() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-layout-hasher",
        "fxhash",
        "--ds-layout-pruning",
        "on",
        "--ds-layout-seek",
        "binary",
        "--ds-config",
        "load-factor=0.5",
        "--ds-build",
        "column-trie=incremental,hash-trie=radix:2",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 4, "four valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        match axes["data_structure"].as_str() {
            | Some("HashTrie") => {
                assert_eq!(axes["ds_layout_hasher"], "fxhash", "{axes}");
                assert_eq!(axes["ds_layout_pruning"], "on", "{axes}");
                assert_eq!(axes["ds_config_load_factor"], 0.5, "{axes}");
                assert_eq!(axes["ds_build_mode"], "radix:2", "{axes}");
            },
            | Some("ColumnTrie") => {
                assert_eq!(axes["ds_layout_seek"], "binary", "{axes}");
                assert_eq!(axes["ds_build_mode"], "incremental", "{axes}");
            },
            | Some("TreeTrie") => assert_eq!(axes["ds_layout_seek"], "binary", "{axes}"),
            | Some("HashIndex") => {
                assert_eq!(axes["ds_layout_hasher"], "fxhash", "{axes}");
                for absent in [
                    "ds_layout_pruning",
                    "ds_layout_seek",
                    "ds_config_load_factor",
                    "ds_build_mode",
                ] {
                    assert!(axes.get(absent).is_none(), "{absent} on {axes}");
                }
            },
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}
