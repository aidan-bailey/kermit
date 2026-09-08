//! CLI smoke test: `bench ds` with `--ds-config load-factor=0.5` records
//! the Config axis, and the flag is rejected where it cannot apply.
//! Mirrors `cli_hash_trie_hasher_choice.rs` for the Config category of the
//! optimization standard: CLI parser -> `ConfigChoices` ->
//! `run_ds_bench_hash::<H, P>` -> `HashTrie::from_tuples_with_config` ->
//! `HasOptimizationAxes::optimization_axes()` -> `BenchReport.axes` -> JSON.

mod common;

use common::cli::{axes_of, bench_ds};

#[test]
fn cli_bench_ds_rejects_ds_config_on_tree_trie() {
    let (output, _) = bench_ds("tree-trie", &["--ds-config", "load-factor=0.5"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-config"), "{stderr}");
}

#[test]
fn cli_bench_ds_rejects_unknown_config_key() {
    // `singleton-pruning` is a Layout flag now, so `--ds-config` must
    // reject it rather than silently accepting a stale key.
    let (output, _) = bench_ds("hash-trie", &["--ds-config", "singleton-pruning=true"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("singleton-pruning"), "{stderr}");
}

#[test]
fn cli_bench_ds_with_load_factor_records_axis() {
    let (output, report) = bench_ds("hash-trie", &["--ds-config", "load-factor=0.5"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_config_load_factor"], 0.5);
}

#[test]
fn cli_bench_ds_default_load_factor_is_seventy_percent() {
    let (output, report) = bench_ds("hash-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_config_load_factor"], 0.7);
}
