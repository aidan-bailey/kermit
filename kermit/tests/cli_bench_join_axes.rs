//! `bench join` goes through the same generic runner as `bench run`, so an
//! ad-hoc join reports the full metric set and the structure's
//! optimization axes, under `bench run`'s Criterion naming with the
//! `adhoc/<query-stem>` identity.

mod common;

use common::cli::{bench_join, kermit_bin, reports_of};

#[test]
fn cli_bench_join_hash_trie_reports_axes_and_all_default_metrics() {
    let (output, report) = bench_join("hash-trie", "hash-triejoin", &[
        "--ds-layout-hasher",
        "fxhash",
        "--ds-config",
        "load-factor=0.5",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 1, "one query, one report");
    let r = &reports[0];
    assert_eq!(r["kind"], "join");
    let axes = &r["axes"];
    assert_eq!(axes["benchmark"], "adhoc");
    assert_eq!(axes["query"], "intersect_query");
    assert_eq!(axes["data_structure"], "HashTrie");
    assert_eq!(axes["algorithm"], "HashTriejoin");
    assert_eq!(axes["optimiser"], "lexicographic");
    assert_eq!(axes["ds_layout_hasher"], "fxhash");
    assert_eq!(axes["ds_layout_pruning"], "off");
    assert_eq!(axes["ds_config_load_factor"], 0.5);
    assert!(axes["tuples"].as_u64().unwrap() > 0);
    assert!(
        axes.get("relations").is_none(),
        "the old count axis is gone"
    );

    let groups = r["criterion_groups"].as_array().unwrap();
    let group = "join/adhoc/intersect_query/HashTrie/HashTriejoin";
    assert!(groups.iter().all(|g| g["group"] == group), "{groups:?}");
    let mut functions: Vec<&str> = groups
        .iter()
        .map(|g| g["function"].as_str().unwrap())
        .collect();
    functions.sort_unstable();
    assert_eq!(functions, [
        "insertion",
        "iteration",
        "space/first",
        "space/second"
    ]);
}

#[test]
fn cli_bench_join_name_is_a_prefix() {
    let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let report = tempfile::NamedTempFile::new().unwrap();
    let output = std::process::Command::new(kermit_bin())
        .args([
            "bench",
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
        ])
        .arg("--name")
        .arg("mine")
        .arg("--report-json")
        .arg(report.path())
        .arg("join")
        .arg("--relations")
        .arg(fixtures.join("first.csv"))
        .arg(fixtures.join("second.csv"))
        .arg("--query")
        .arg(fixtures.join("intersect_query.dl"))
        .args(["-i", "tree-trie", "-a", "leapfrog-triejoin", "-m", "space"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let groups = reports_of(&report)[0]["criterion_groups"].clone();
    assert_eq!(
        groups[0]["group"],
        "mine/adhoc/intersect_query/TreeTrie/LeapfrogTriejoin"
    );
}

#[test]
fn cli_bench_join_rejects_ds_config_on_tree_trie() {
    let (output, _) = bench_join("tree-trie", "leapfrog-triejoin", &[
        "--ds-config",
        "load-factor=0.5",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-config"), "{stderr}");
}
