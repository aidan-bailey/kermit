//! CLI smoke test for the sorted tries' seek-strategy Layout (#80).
//! `--ds-layout-seek` reaches both sorted cells and is recorded as
//! `ds_layout_seek`, and the default records `"binary"`. The flag is
//! rejected where it cannot act: on `hash-trie`, and on `bench ds`, none
//! of whose metrics seeks. Mirrors `cli_hash_trie_layout_pruning.rs`.

mod common;

use {
    common::cli::{axes_of, bench_ds, bench_join, bench_run, kermit_bin, reports_of},
    std::{
        path::PathBuf,
        process::{Command, Output},
    },
};

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_rejected(output: &Output, hint: &str) {
    assert!(
        !output.status.success(),
        "accepted: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-layout-seek"), "{stderr}");
    assert!(stderr.contains(hint), "{stderr}");
}

/// Runs `kermit join` over `first.csv` / `second.csv` with
/// `intersect_query.dl`, appending `extra_args`.
fn kermit_join(indexstructure: &str, algorithm: &str, extra_args: &[&str]) -> Output {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    Command::new(kermit_bin())
        .arg("join")
        .arg("--relations")
        .arg(fixtures.join("first.csv"))
        .arg(fixtures.join("second.csv"))
        .arg("--query")
        .arg(fixtures.join("intersect_query.dl"))
        .arg("--indexstructure")
        .arg(indexstructure)
        .arg("--algorithm")
        .arg(algorithm)
        .args(extra_args)
        .output()
        .expect("failed to execute kermit binary")
}

#[test]
fn cli_bench_run_default_seek_is_binary_on_both_sorted_tries() {
    for ds in ["tree-trie", "column-trie"] {
        let (output, report) = bench_run("triangle", &[
            "-i",
            ds,
            "-a",
            "leapfrog-triejoin",
            "-m",
            "space",
        ]);
        assert_success(&output);
        assert_eq!(axes_of(&report)["ds_layout_seek"], "binary", "{ds}");
    }
}

#[test]
fn cli_bench_run_with_galloping_records_axis() {
    for ds in ["tree-trie", "column-trie"] {
        let (output, report) = bench_run("triangle", &[
            "-i",
            ds,
            "-a",
            "leapfrog-triejoin",
            "-m",
            "space",
            "--ds-layout-seek",
            "galloping",
        ]);
        assert_success(&output);
        assert_eq!(axes_of(&report)["ds_layout_seek"], "galloping", "{ds}");
    }
}

#[test]
fn cli_bench_run_sweep_carries_seek_only_to_sorted_trie_cells() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-layout-seek",
        "linear",
    ]);
    assert_success(&output);
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        if axes["data_structure"] == "HashTrie" {
            assert!(axes.get("ds_layout_seek").is_none(), "{axes}");
        } else {
            assert_eq!(axes["ds_layout_seek"], "linear", "{axes}");
        }
    }
}

#[test]
fn cli_rejects_seek_on_hash_trie() {
    let (output, _) = bench_run("triangle", &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "-m",
        "space",
        "--ds-layout-seek",
        "linear",
    ]);
    assert_rejected(&output, "tree-trie or column-trie");
    let (output, _) = bench_join("hash-trie", "hash-triejoin", &[
        "-m",
        "space",
        "--ds-layout-seek",
        "linear",
    ]);
    assert_rejected(&output, "tree-trie or column-trie");
    let output = kermit_join("hash-trie", "hash-triejoin", &[
        "--ds-layout-seek",
        "linear",
    ]);
    assert_rejected(&output, "tree-trie or column-trie");
}

/// None of `bench ds`'s metrics calls `seek`, so a seek "ablation" there
/// would time identical code under two labels.
#[test]
fn cli_bench_ds_rejects_seek_on_every_structure() {
    for ds in ["tree-trie", "column-trie", "hash-trie", "all"] {
        let (output, _) = bench_ds(ds, &["--ds-layout-seek", "galloping"]);
        assert_rejected(&output, "no effect on bench ds");
    }
}

/// Both sorted cells of a `bench ds` sweep report the default, and the
/// hash cell reports no seek axis.
#[test]
fn cli_bench_ds_reports_the_default_seek() {
    let (output, report) = bench_ds("all", &[]);
    assert_success(&output);
    let reports = reports_of(&report);
    let mut structures: Vec<&str> = reports
        .iter()
        .map(|r| {
            r["axes"]["data_structure"]
                .as_str()
                .expect("data_structure")
        })
        .collect();
    structures.sort_unstable();
    assert_eq!(structures, ["ColumnTrie", "HashTrie", "TreeTrie"]);
    for r in &reports {
        let axes = &r["axes"];
        if axes["data_structure"] == "HashTrie" {
            assert!(axes.get("ds_layout_seek").is_none(), "{axes}");
        } else {
            assert_eq!(axes["ds_layout_seek"], "binary", "{axes}");
        }
    }
}

#[test]
fn cli_bench_join_records_the_seek_strategy() {
    let (output, report) = bench_join("column-trie", "leapfrog-triejoin", &[
        "-m",
        "space",
        "--ds-layout-seek",
        "linear",
    ]);
    assert_success(&output);
    assert_eq!(axes_of(&report)["ds_layout_seek"], "linear");
}

#[test]
fn cli_kermit_join_answers_are_identical_under_every_strategy() {
    for ds in ["tree-trie", "column-trie"] {
        let outputs: Vec<String> = ["linear", "binary", "galloping"]
            .iter()
            .map(|seek| {
                let output = kermit_join(ds, "leapfrog-triejoin", &["--ds-layout-seek", seek]);
                assert_success(&output);
                String::from_utf8(output.stdout).expect("utf-8 CSV")
            })
            .collect();
        assert!(
            outputs[0].lines().count() > 1,
            "{ds}: expected a header and rows, got {:?}",
            outputs[0]
        );
        assert_eq!(outputs[0], outputs[1], "{ds}: linear vs binary");
        assert_eq!(outputs[1], outputs[2], "{ds}: binary vs galloping");
    }
}
