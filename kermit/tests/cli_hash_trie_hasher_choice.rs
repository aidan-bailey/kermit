//! CLI smoke test: `bench ds` with `--ds-layout-hasher fxhash` records the
//! optimization axis correctly. Also verifies that the default (no flag)
//! records `ds_layout_hasher: "sip"`.
//!
//! These tests exercise the end-to-end chain set up by Phases 1-5 of the
//! optimization-standard plan: CLI parser -> `LayoutChoices` ->
//! `Execution::for_structure` -> `run_ds_bench::<HashTrieFamily<H, P>>` ->
//! `HashTrie<H, P>` -> `HasOptimizationAxes::optimization_axes()` ->
//! `BenchReport.axes` -> JSON on disk.

mod common;

use common::cli::bench_ds;

/// Runs `bench ds` against `edge.csv` with the hash-trie + space metric and
/// returns the parsed JSON report array, asserting the command succeeded.
/// `extra_ds_args` is appended after the standard `--relation` /
/// `--indexstructure` so callers can inject `--ds-layout-hasher fxhash` (or
/// omit it for the default).
fn run_bench_ds_hash(extra_ds_args: &[&str]) -> Vec<serde_json::Value> {
    let (output, report) = bench_ds("hash-trie", extra_ds_args);
    assert!(
        output.status.success(),
        "bench ds command failed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let reports = common::cli::reports_of(&report);
    assert!(
        !reports.is_empty(),
        "bench-runs JSON was empty (expected at least one report)"
    );
    reports
}

#[test]
fn cli_bench_ds_with_fxhash_layout_records_axis() {
    let reports = run_bench_ds_hash(&["--ds-layout-hasher", "fxhash"]);
    assert_eq!(
        reports[0]["axes"]["ds_layout_hasher"], "fxhash",
        "ds_layout_hasher axis missing or wrong value: {}",
        reports[0]["axes"]
    );
}

#[test]
fn cli_bench_ds_default_hasher_is_sip() {
    let reports = run_bench_ds_hash(&[]);
    assert_eq!(
        reports[0]["axes"]["ds_layout_hasher"], "sip",
        "default ds_layout_hasher should be 'sip': {}",
        reports[0]["axes"]
    );
}
