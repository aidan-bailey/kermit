//! CLI regression tests for issue #56: `bench run` with `-i all` / `-a all`
//! must run exactly the three valid (structure, algorithm) cells, and no
//! report row may name an algorithm the cell did not execute — the
//! historical bug was `-i all -a leapfrog-triejoin` stamping HashTrie's
//! `hash_join` measurements with `LeapfrogTriejoin`.
//!
//! Runs against a fake benchmark cache built from the committed watdiv-mini
//! fixture, so no network access is needed.

use {
    std::{collections::BTreeSet, fs, path::PathBuf, process::Command},
    tempfile::{NamedTempFile, TempDir},
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

fn fixtures_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures") }

const MINI_BENCH: &str = "watdiv-stress-mini-tiny";

/// The `bench run` tests redirect the benchmark cache via `XDG_CACHE_HOME`,
/// which `dirs::cache_dir()` honours only on Linux.
fn skip_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) {
        eprintln!("skipping cli_bench_run_sweep test: XDG_CACHE_HOME needs Linux");
        return true;
    }
    false
}

fn make_fake_cache() -> TempDir {
    let tmp = tempfile::tempdir().expect("failed to create temp cache dir");
    let bench_dir = tmp.path().join("kermit/benchmarks").join(MINI_BENCH);
    fs::create_dir_all(&bench_dir).expect("failed to create fake cache subdir");
    let artifacts = fixtures_dir().join("watdiv-mini/artifacts");
    fs::copy(
        artifacts.join(format!("{MINI_BENCH}.yml")),
        bench_dir.join("benchmark.yml"),
    )
    .expect("failed to copy fixture YAML");
    fs::write(bench_dir.join("meta.json"), "{}").expect("failed to write meta.json marker");
    for rel in ["eligibleregion", "includes", "parentcountry"] {
        fs::copy(
            artifacts.join(format!("{rel}.parquet")),
            bench_dir.join(format!("{rel}.parquet")),
        )
        .expect("failed to copy fixture parquet");
    }
    tmp
}

/// Runs `bench run <MINI_BENCH> -q q0002 --metrics iteration -i <ds> -a <algo>`
/// and returns the process output plus the parsed report (if one was
/// written).
fn run_sweep(ds: &str, algo: &str) -> (std::process::Output, Option<Vec<serde_json::Value>>) {
    let cache = make_fake_cache();
    let report = NamedTempFile::new().expect("failed to create temp report file");
    let output = Command::new(kermit_bin())
        .env("XDG_CACHE_HOME", cache.path())
        .args([
            "bench",
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
            "--report-json",
        ])
        .arg(report.path())
        .args([
            "run",
            MINI_BENCH,
            "-q",
            "q0002",
            "--indexstructure",
            ds,
            "--algorithm",
            algo,
            "--metrics",
            "iteration",
        ])
        .output()
        .expect("failed to execute kermit binary");
    let text = fs::read_to_string(report.path()).expect("report file should exist");
    let reports = if text.trim().is_empty() {
        None
    } else {
        Some(serde_json::from_str(&text).expect("report should be a valid JSON array"))
    };
    (output, reports)
}

fn cells(reports: &[serde_json::Value]) -> BTreeSet<(String, String)> {
    reports
        .iter()
        .map(|r| {
            (
                r["axes"]["data_structure"]
                    .as_str()
                    .expect("data_structure axis")
                    .to_string(),
                r["axes"]["algorithm"]
                    .as_str()
                    .expect("algorithm axis")
                    .to_string(),
            )
        })
        .collect()
}

fn valid_cells() -> BTreeSet<(String, String)> {
    [
        ("TreeTrie", "LeapfrogTriejoin"),
        ("ColumnTrie", "LeapfrogTriejoin"),
        ("HashTrie", "HashTriejoin"),
    ]
    .into_iter()
    .map(|(d, a)| (d.to_string(), a.to_string()))
    .collect()
}

/// Acceptance criterion: `-i all -a all` runs exactly the 3 valid cells.
#[test]
fn cli_bench_run_all_all_runs_exactly_the_three_valid_cells() {
    if skip_unsupported() {
        return;
    }
    let (output, reports) = run_sweep("all", "all");
    assert!(
        output.status.success(),
        "bench run -i all -a all failed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports.expect("a report must be written");
    assert_eq!(reports.len(), 3, "one report per valid cell: {reports:?}");
    assert_eq!(cells(&reports), valid_cells());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("skipping incompatible pair (HashTrie, LeapfrogTriejoin)"),
        "skipped cells must be announced; stderr: {stderr}"
    );
}

/// Regression for issue #56: `-i all -a leapfrog-triejoin` must not emit a
/// HashTrie row at all (it used to emit one labelled LeapfrogTriejoin).
#[test]
fn cli_bench_run_all_structures_with_lftj_skips_hash_trie() {
    if skip_unsupported() {
        return;
    }
    let (output, reports) = run_sweep("all", "leapfrog-triejoin");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports.expect("a report must be written");
    let ran = cells(&reports);
    assert_eq!(
        ran,
        valid_cells()
            .into_iter()
            .filter(|(_, a)| a == "LeapfrogTriejoin")
            .collect::<BTreeSet<_>>(),
        "{reports:?}"
    );
    assert!(
        !ran.iter().any(|(d, _)| d == "HashTrie"),
        "HashTrie must never be labelled LeapfrogTriejoin: {ran:?}"
    );
}

/// A single explicit incompatible pair is a usage error, not a silent
/// no-op run that writes an empty report.
#[test]
fn cli_bench_run_explicit_incompatible_pair_is_rejected() {
    if skip_unsupported() {
        return;
    }
    let (output, reports) = run_sweep("hash-trie", "leapfrog-triejoin");
    assert!(!output.status.success(), "incompatible pair must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("incompatible CLI selection"),
        "stderr: {stderr}"
    );
    assert!(reports.is_none(), "no report must be written: {reports:?}");
}
