//! CLI smoke test: `--ds-layout-levels keys` records `ds_layout_levels:
//! "keys"`, the absent flag records `"all"`, a payload query's `index` lines
//! name their levels only under `keys`, the answers are the same under both,
//! and the flag is refused by `bench ds` and on a structure without the
//! dimension (#116). Mirrors `cli_hash_trie_layout_expansion.rs`.

mod common;

use {
    common::cli::{axes_of, bench_ds, bench_run, kermit_bin, reports_of},
    std::{path::Path, process::Command},
};

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_bench_run_with_keys_records_the_axis() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "-m",
        "space",
        "--ds-layout-levels",
        "keys",
    ]);
    assert_success(&output);
    assert_eq!(axes_of(&report)["ds_layout_levels"], "keys");
}

#[test]
fn cli_bench_run_default_levels_are_all() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "-m",
        "space",
    ]);
    assert_success(&output);
    assert_eq!(axes_of(&report)["ds_layout_levels"], "all");
}

/// Each query is prepared once per policy its cells need: the sorted cells
/// under `all`, the HashTrie cell under `keys`.
#[test]
fn cli_bench_run_sweep_carries_levels_only_to_hash_trie_cells() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-layout-levels",
        "keys",
    ]);
    assert_success(&output);
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 4, "four valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        if axes["data_structure"] == "HashTrie" {
            assert_eq!(axes["ds_layout_levels"], "keys", "{axes}");
        } else {
            assert!(axes.get("ds_layout_levels").is_none(), "{axes}");
        }
    }
}

#[test]
fn cli_bench_ds_refuses_the_levels_flag() {
    let (output, _) = bench_ds("hash-trie", &["--ds-layout-levels", "keys"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--ds-layout-levels has no effect on bench ds"),
        "{stderr}"
    );
}

#[test]
fn cli_bench_run_refuses_the_levels_flag_on_tree_trie() {
    let (output, _) = bench_run("triangle", &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "--ds-layout-levels",
        "keys",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-layout-levels"), "{stderr}");
}

/// `r`, `s` and the query `Q(X, P) :- r(X, P), s(X).`, written into `dir`:
/// `P` is carried by `r` alone, so it is payload.
fn payload_fixture(dir: &Path) -> [std::path::PathBuf; 3] {
    let (r, s, query) = (dir.join("r.csv"), dir.join("s.csv"), dir.join("payload.dl"));
    std::fs::write(&r, "x,p\n1,10\n1,11\n2,20\n3,30\n").unwrap();
    std::fs::write(&s, "x\n1\n2\n").unwrap();
    std::fs::write(&query, "Q(X, P) :- r(X, P), s(X).\n").unwrap();
    [r, s, query]
}

/// Under `keys` the index of `r` has one level of its two columns and its
/// `index` line names it; under `all` no line does; and `kermit join` writes
/// the same rows under both.
#[test]
fn cli_payload_query_names_its_levels_and_answers_alike() {
    let dir = tempfile::tempdir().unwrap();
    let [r, s, query] = payload_fixture(dir.path());
    let index_lines = |levels: &str| -> Vec<String> {
        let report = tempfile::NamedTempFile::new().unwrap();
        let output = Command::new(kermit_bin())
            .arg("bench")
            .arg("--report-json")
            .arg(report.path())
            .args(["join", "--relations"])
            .arg(&r)
            .arg(&s)
            .arg("--query")
            .arg(&query)
            .args(["-i", "hash-trie", "-a", "hash-triejoin", "-m", "space"])
            .args(["--ds-layout-levels", levels])
            .output()
            .unwrap();
        assert_success(&output);
        reports_of(&report)[0]["metadata"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|line| line["label"] == "index")
            .map(|line| line["value"].as_str().unwrap().to_string())
            .collect()
    };
    let keys = index_lines("keys");
    assert!(
        keys.iter()
            .any(|line| line.starts_with("#0 r ") && line.contains(" levels 1:")),
        "{keys:?}"
    );
    let all = index_lines("all");
    assert!(all.iter().all(|line| !line.contains(" levels ")), "{all:?}");

    let rows = |levels: &str| -> Vec<String> {
        let output = Command::new(kermit_bin())
            .args(["join", "--relations"])
            .arg(&r)
            .arg(&s)
            .arg("--query")
            .arg(&query)
            .args(["-i", "hash-trie", "-a", "hash-triejoin"])
            .args(["--ds-layout-levels", levels])
            .output()
            .unwrap();
        assert_success(&output);
        // The first line is the CSV header.
        let mut rows: Vec<String> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .skip(1)
            .map(str::to_string)
            .collect();
        rows.sort();
        rows
    };
    assert_eq!(rows("keys"), ["1,10", "1,11", "2,20"]);
    assert_eq!(rows("all"), rows("keys"));
}
