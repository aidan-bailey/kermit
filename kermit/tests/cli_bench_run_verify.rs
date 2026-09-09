//! `bench run --verify`: each query with an `expected` cardinality is run
//! once before timing and its tuple count compared; a mismatch aborts.
//!
//! Runs against fake caches built from the committed watdiv-mini fixture
//! (`q0002` has 6 answers), so no network is needed.

use {
    std::{fs, path::PathBuf, process::Command},
    tempfile::{NamedTempFile, TempDir},
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

fn fixtures_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures") }

const MINI_BENCH: &str = "watdiv-stress-mini-tiny";
const Q0002_LINE: &str = "  query: Q_tiny_q0002(V0, V1) :- parentcountry(V0, V1).";

fn skip_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) {
        eprintln!("skipping cli_bench_run_verify test: XDG_CACHE_HOME needs Linux");
        return true;
    }
    false
}

/// Copies the mini fixture into `<cache>/kermit/benchmarks/<name>/`,
/// renaming the YAML's `name:` and, when `expected` is given, adding
/// `expected: <n>` to query `q0002`.
fn add_benchmark(cache: &TempDir, name: &str, expected: Option<u64>) {
    let dir = cache.path().join("kermit/benchmarks").join(name);
    fs::create_dir_all(&dir).unwrap();
    let artifacts = fixtures_dir().join("watdiv-mini/artifacts");
    let mut yaml = fs::read_to_string(artifacts.join(format!("{MINI_BENCH}.yml"))).unwrap();
    yaml = yaml.replacen(&format!("name: {MINI_BENCH}"), &format!("name: {name}"), 1);
    assert!(yaml.contains(Q0002_LINE), "fixture drifted: {yaml}");
    if let Some(n) = expected {
        yaml = yaml.replacen(Q0002_LINE, &format!("{Q0002_LINE}\n  expected: {n}"), 1);
    }
    fs::write(dir.join("benchmark.yml"), yaml).unwrap();
    fs::write(dir.join("meta.json"), "{}").unwrap();
    for rel in ["eligibleregion", "includes", "parentcountry"] {
        fs::copy(
            artifacts.join(format!("{rel}.parquet")),
            dir.join(format!("{rel}.parquet")),
        )
        .unwrap();
    }
}

/// `bench run --all -q q0002 -i tree-trie -a leapfrog-triejoin -m iteration`
/// with the given extra args, against `cache`, from an empty workspace.
fn run(cache: &TempDir, extra: &[&str]) -> (std::process::Output, Vec<serde_json::Value>) {
    let workspace = tempfile::tempdir().unwrap();
    let report = NamedTempFile::new().unwrap();
    let output = Command::new(kermit_bin())
        .env("XDG_CACHE_HOME", cache.path())
        .env("KERMIT_WORKSPACE", workspace.path())
        .args([
            "bench",
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
        ])
        .arg("--report-json")
        .arg(report.path())
        .args([
            "run",
            "--all",
            "-q",
            "q0002",
            "-i",
            "tree-trie",
            "-a",
            "leapfrog-triejoin",
        ])
        .args(["--metrics", "iteration"])
        .args(extra)
        .output()
        .unwrap();
    let text = fs::read_to_string(report.path()).unwrap();
    let reports = if text.trim().is_empty() {
        vec![]
    } else {
        serde_json::from_str(&text).unwrap()
    };
    (output, reports)
}

#[test]
fn verify_passes_and_stamps_the_axis_when_the_count_matches() {
    if skip_unsupported() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    add_benchmark(&cache, "a-ok", Some(6));
    let (output, reports) = run(&cache, &["--verify"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["axes"]["verified"], true);
    assert!(
        stderr.contains("verified:"),
        "metadata block should show verified; {stderr}"
    );
}

#[test]
fn verify_aborts_on_a_wrong_count_and_keeps_earlier_reports() {
    if skip_unsupported() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    add_benchmark(&cache, "a-ok", Some(6));
    add_benchmark(&cache, "z-bad", Some(5));
    let (output, reports) = run(&cache, &["--verify"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "z-bad must fail; stderr: {stderr}"
    );
    assert!(stderr.contains("verification failed"), "{stderr}");
    assert!(stderr.contains("returned 6 tuples, expected 5"), "{stderr}");
    assert!(stderr.contains("partial report retained at"), "{stderr}");
    assert_eq!(reports.len(), 1, "a-ok's report survives: {reports:?}");
    assert_eq!(reports[0]["axes"]["benchmark"], "a-ok");
}

#[test]
fn verify_notes_queries_without_an_expected_count() {
    if skip_unsupported() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    add_benchmark(&cache, "a-ok", None);
    let (output, reports) = run(&cache, &["--verify"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("not verified"), "{stderr}");
    assert!(reports[0]["axes"].get("verified").is_none());
}

#[test]
fn without_the_flag_nothing_is_verified() {
    if skip_unsupported() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    add_benchmark(&cache, "a-ok", Some(5)); // wrong on purpose: must not matter
    let (output, reports) = run(&cache, &[]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(reports[0]["axes"].get("verified").is_none());
}
