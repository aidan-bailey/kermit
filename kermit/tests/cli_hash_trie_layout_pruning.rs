//! CLI smoke test: `--ds-layout-pruning on` records `ds_layout_pruning: "on"`,
//! the absent flag records `"off"`, and the flag is rejected on a structure
//! without the dimension. Mirrors `cli_hash_trie_hasher_choice.rs`.

mod common;

use common::cli::{axes_of, bench_ds, bench_run, reports_of};

#[test]
fn cli_bench_ds_with_pruning_on_records_axis() {
    let (output, report) = bench_ds("hash-trie", &["--ds-layout-pruning", "on"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let axes = axes_of(&report);
    assert_eq!(axes["ds_layout_pruning"], "on", "{axes}");
    assert_eq!(axes["ds_layout_hasher"], "sip", "{axes}");
}

#[test]
fn cli_bench_ds_default_pruning_is_off() {
    let (output, report) = bench_ds("hash-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_layout_pruning"], "off");
}

#[test]
fn cli_bench_ds_rejects_pruning_flag_on_tree_trie() {
    let (output, _) = bench_ds("tree-trie", &["--ds-layout-pruning", "on"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-layout-pruning"), "{stderr}");
}

#[test]
fn cli_bench_run_sweep_carries_pruning_only_to_hash_trie_cells() {
    // `-i all -a all --ds-layout-pruning on` runs the three valid cells; the
    // sorted cells have no pruning axis and the hash cell reports "on".
    let (output, report) = bench_run(
        "triangle",
        &[
            "-i",
            "all",
            "-a",
            "all",
            "-m",
            "space",
            "--ds-layout-pruning",
            "on",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        if axes["data_structure"] == "HashTrie" {
            assert_eq!(axes["ds_layout_pruning"], "on", "{axes}");
        } else {
            assert!(axes.get("ds_layout_pruning").is_none(), "{axes}");
        }
    }
}
