//! CLI smoke test: `--ds-layout-expansion lazy` records
//! `ds_layout_expansion: "lazy"`, the absent flag records `"eager"`, and
//! the flag is rejected on a structure without the dimension. Mirrors
//! `cli_hash_trie_layout_pruning.rs`.
//!
//! It also pins the probe rule of issue #92: a lazy family's loaded engine
//! is never probed. `run_benchmark` checks that its relations still have
//! their as-built footprint before measuring `space`, and fails the run if
//! `--verify` or `iteration` reached them.

mod common;

use common::cli::{axes_of, bench_ds, bench_run, reports_of};

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_bench_ds_with_lazy_expansion_records_axis() {
    let (output, report) = bench_ds("hash-trie", &["--ds-layout-expansion", "lazy"]);
    assert_success(&output);
    let axes = axes_of(&report);
    assert_eq!(axes["ds_layout_expansion"], "lazy", "{axes}");
    assert_eq!(axes["ds_layout_pruning"], "off", "{axes}");
}

#[test]
fn cli_bench_ds_default_expansion_is_eager() {
    let (output, report) = bench_ds("hash-trie", &[]);
    assert_success(&output);
    assert_eq!(axes_of(&report)["ds_layout_expansion"], "eager");
}

#[test]
fn cli_bench_ds_rejects_expansion_flag_on_tree_trie() {
    let (output, _) = bench_ds("tree-trie", &["--ds-layout-expansion", "lazy"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-layout-expansion"), "{stderr}");
}

#[test]
fn cli_bench_run_sweep_carries_expansion_only_to_hash_trie_cells() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-layout-expansion",
        "lazy",
    ]);
    assert_success(&output);
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        if axes["data_structure"] == "HashTrie" {
            assert_eq!(axes["ds_layout_expansion"], "lazy", "{axes}");
        } else {
            assert!(axes.get("ds_layout_expansion").is_none(), "{axes}");
        }
    }
}

/// `--verify` and `iteration` run before `space` here. Both would expand
/// the loaded engine if they probed it, and `run_benchmark`'s as-built
/// check would then fail the run. Triangle's vertex 1 has four out-edges,
/// so its child is a multi-tuple `Unexpanded` list under either pruning
/// setting.
#[test]
fn cli_bench_run_lazy_probes_leave_the_engine_as_built() {
    for pruning in ["off", "on"] {
        let (output, report) = bench_run("triangle", &[
            "-i",
            "hash-trie",
            "-a",
            "hash-triejoin",
            "--ds-layout-expansion",
            "lazy",
            "--ds-layout-pruning",
            pruning,
            "-m",
            "iteration",
            "space",
            "--verify",
        ]);
        assert_success(&output);
        let axes = axes_of(&report);
        assert_eq!(axes["verified"], true, "{axes}");
        assert_eq!(axes["ds_layout_expansion"], "lazy", "{axes}");
    }
}
