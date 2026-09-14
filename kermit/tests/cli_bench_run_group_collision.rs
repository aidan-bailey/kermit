//! CLI regression test for issue #69: Criterion 0.8.2 truncates each on-disk
//! directory name to 64 bytes, and `bench run` builds a fresh `Criterion`
//! per group, so two groups of one sweep that share their first 64 bytes
//! write into the same `target/criterion/<dir>/` — each cell silently
//! overwriting the last, with Criterion reporting the difference as a
//! "regression". `bench run` must refuse such a sweep before timing anything.
//!
//! Uses the committed `triangle` benchmark (a `path:` relation), so no
//! network or cache fixture is needed.

use {
    std::{path::PathBuf, process::Command},
    tempfile::TempDir,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

#[test]
fn sweep_whose_groups_share_a_criterion_directory_is_refused_before_timing() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let criterion_home = tmp.path().join("criterion");
    let report = tmp.path().join("report.json");
    // `{prefix}/triangle/triangle/{ds}/{algo}`: with this prefix the structure
    // segment starts past byte 64, so all three cells truncate identically.
    let prefix = "run-with-a-prefix-long-enough-to-push-the-structure-name-past-byte-64";

    let output = Command::new(kermit_bin())
        .env("CRITERION_HOME", &criterion_home)
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .args([
            "bench",
            "--name",
            prefix,
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
            "--report-json",
        ])
        .arg(&report)
        .args([
            "run",
            "triangle",
            "-i",
            "all",
            "-a",
            "all",
            "-m",
            "iteration",
        ])
        .output()
        .expect("failed to spawn kermit");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "bench run should refuse the sweep; stderr:\n{stderr}"
    );
    // Which pair is reported first depends on sweep order; either way the
    // error must name two distinct groups of this sweep.
    let named: std::collections::BTreeSet<&str> = ["TreeTrie", "ColumnTrie", "HashTrie"]
        .into_iter()
        .filter(|ds| stderr.contains(&format!("{prefix}/triangle/triangle/{ds}/")))
        .collect();
    assert_eq!(
        named.len(),
        2,
        "error should name both colliding groups; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("64"),
        "error should explain the truncation; stderr:\n{stderr}"
    );
    assert!(
        !criterion_home.exists(),
        "nothing should have been timed before the refusal"
    );
    assert!(
        !report.exists(),
        "no report should be written for a refused sweep"
    );
}
