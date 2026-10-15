//! `kermit bench gen job` and `bench run job-N`, through the real binary.
//!
//! The first two tests fail before any generation starts and need no DuckDB.
//! The others build the fixture raw cache
//! (`kermit-generate/tests/fixtures/job/`) at `<XDG_CACHE_HOME>/kermit/imdb`,
//! where `raw::default_raw_dir()` looks, with a provenance equal to the pins,
//! and skip with a stderr line without DuckDB (CI installs none; under `nix
//! develop` they run).
//!
//! Every test redirects the platform cache with `XDG_CACHE_HOME`, which `dirs`
//! honours only on Linux, and passes `--output-dir` or `KERMIT_WORKSPACE`, so
//! nothing is written under `~/.cache/kermit`.

mod common;

use {
    common::{
        cli::kermit_bin,
        job::{build_raw_cache, csv_dir, fixture_dir, locate_or_skip, pinned_provenance},
    },
    kermit_bench::BenchmarkDefinition,
    kermit_generate::{
        job::{duckdb::Duckdb, JobExtras},
        GeneratedMeta,
    },
    std::{
        fs,
        path::Path,
        process::{Command, Output},
    },
};

/// The `--tag` every `bench gen` here passes.
const TAG: &str = "citest";

/// The tests that start generation redirect the cache with `XDG_CACHE_HOME`,
/// which only Linux honours.
fn skip_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) {
        eprintln!("skipping cli_job_gen test: XDG_CACHE_HOME needs Linux");
        return true;
    }
    false
}

/// The fixture raw cache at `<cache>/kermit/imdb`, with a provenance equal to
/// the pins so `raw::ensure` takes it as current.
fn fixture_raw_cache(duckdb: &Duckdb, cache: &Path) {
    let csv = csv_dir(cache, false);
    build_raw_cache(
        duckdb,
        &cache.join("kermit").join("imdb"),
        &csv,
        &fixture_dir().join("queries"),
        pinned_provenance(duckdb),
    );
}

/// `kermit bench gen job --template <template> --tag citest --output-dir <out>`
/// with the platform cache at `cache`; `duckdb_bin` overrides
/// `KERMIT_DUCKDB_BIN`.
fn gen_job(cache: &Path, out: &Path, template: &str, duckdb_bin: Option<&str>) -> Output {
    let mut command = Command::new(kermit_bin());
    command
        .env("XDG_CACHE_HOME", cache)
        .args(["bench", "gen", "job", "--template", template, "--tag", TAG])
        .arg("--output-dir")
        .arg(out);
    if let Some(bin) = duckdb_bin {
        command.env("KERMIT_DUCKDB_BIN", bin);
    }
    command.output().expect("failed to execute kermit binary")
}

#[test]
fn template_outside_1_to_33_is_rejected_before_anything_is_written() {
    for template in ["0", "34"] {
        let cache = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let output = gen_job(cache.path(), out.path(), template, None);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "--template {template} must fail; stderr: {stderr}"
        );
        assert!(
            stderr.contains("--template") && stderr.contains("1..=33"),
            "the error must name the flag and the range: {stderr}"
        );
        assert_eq!(
            fs::read_dir(out.path()).unwrap().count(),
            0,
            "nothing in --output-dir"
        );
        assert_eq!(
            fs::read_dir(cache.path()).unwrap().count(),
            0,
            "nothing in the cache"
        );
    }
}

/// A host without DuckDB fails with an error naming the variable that
/// overrides it, and has created nothing: DuckDB is located before the output
/// directory is made.
#[test]
fn a_missing_duckdb_is_reported_before_anything_is_written() {
    let cache = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let output = gen_job(
        cache.path(),
        out.path(),
        "1",
        Some("/nonexistent/kermit-test/duckdb"),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("KERMIT_DUCKDB_BIN"), "{stderr}");
    assert_eq!(
        fs::read_dir(out.path()).unwrap().count(),
        0,
        "nothing in --output-dir"
    );
    assert_eq!(
        fs::read_dir(cache.path()).unwrap().count(),
        0,
        "nothing in the cache"
    );
}

#[test]
fn gen_job_writes_a_benchmark_from_the_raw_cache() {
    if skip_unsupported() {
        return;
    }
    let Some(duckdb) = locate_or_skip("gen_job_writes_a_benchmark_from_the_raw_cache") else {
        return;
    };
    let cache = tempfile::tempdir().unwrap();
    fixture_raw_cache(&duckdb, cache.path());
    let out = tempfile::tempdir().unwrap();

    let output = gen_job(cache.path(), out.path(), "1", None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "bench gen job failed; stderr: {stderr}"
    );
    assert!(stderr.contains("[gen job] wrote"), "{stderr}");

    let dir = out.path().join(format!("job-1-{TAG}"));
    let text = fs::read_to_string(dir.join("meta.json")).unwrap();
    let meta: GeneratedMeta<JobExtras> = serde_json::from_str(&text).unwrap();
    assert_eq!(meta.kind, "job");
    assert_eq!(meta.tag, TAG);
    assert_eq!(
        meta.spec_hash, None,
        "bench gen records no spec hash, like bench gen lubm"
    );
    assert_eq!(
        (meta.relation_count, meta.query_count, meta.extras.template),
        (9, 2, 1)
    );

    let yaml = fs::read_to_string(dir.join("benchmark.yml")).unwrap();
    let bench: BenchmarkDefinition = serde_yaml::from_str(&yaml).unwrap();
    bench.validate().unwrap();
    assert_eq!(bench.name, format!("job-1-{TAG}"));
    let counts: Vec<(&str, Option<u64>)> = bench
        .queries
        .iter()
        .map(|query| (query.name.as_str(), query.expected))
        .collect();
    assert_eq!(counts, [("1a", Some(5)), ("1b", Some(7))]);

    // The output went to --output-dir; the cache holds only the raw cache the
    // test built.
    assert!(cache.path().join("kermit/imdb/provenance.json").is_file());
    assert!(!cache.path().join("kermit/benchmarks").exists());
}

/// The committed `benchmarks/job-1.yml` as the only YAML of a temp workspace.
fn workspace_with_job_1() -> tempfile::TempDir {
    let workspace = tempfile::tempdir().unwrap();
    let benchmarks = workspace.path().join("benchmarks");
    fs::create_dir(&benchmarks).unwrap();
    let committed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../benchmarks/job-1.yml");
    fs::copy(&committed, benchmarks.join("job-1.yml")).unwrap();
    workspace
}

/// `kermit bench <timing flags> run job-1 -m iteration --verify <extra>` from
/// `workspace` against the platform cache `cache`, writing the report to
/// `report`; `duckdb_bin` overrides `KERMIT_DUCKDB_BIN`. `bench`'s own flags
/// (`--sample-size`, `--measurement-time`, `--warm-up-time`, `--report-json`)
/// belong to `bench`, not `run`, so they come before `run`, as in
/// `cli_bench_run_verify.rs`. `--verify` checks every count before the timing.
fn bench_run(
    cache: &Path, workspace: &Path, report: &Path, duckdb_bin: Option<&str>, extra: &[&str],
) -> Output {
    let mut command = Command::new(kermit_bin());
    command
        .env("XDG_CACHE_HOME", cache)
        .env("KERMIT_WORKSPACE", workspace)
        .args(["bench", "--sample-size", "10", "--measurement-time", "1"])
        .args(["--warm-up-time", "1", "--report-json"])
        .arg(report)
        .args(["run", "job-1", "-m", "iteration", "--verify"])
        .args(extra);
    if let Some(bin) = duckdb_bin {
        command.env("KERMIT_DUCKDB_BIN", bin);
    }
    command.output().expect("failed to execute kermit binary")
}

/// `(query, verified)` of each report in `report`, in file order.
fn verified(report: &Path) -> Vec<(String, bool)> {
    let text = fs::read_to_string(report).expect("the report file");
    let reports: Vec<serde_json::Value> = serde_json::from_str(&text).expect("a JSON array");
    reports
        .iter()
        .map(|r| {
            (
                r["axes"]["query"].as_str().expect("query axis").to_owned(),
                r["axes"]["verified"] == true,
            )
        })
        .collect()
}

/// The whole path a user takes: a committed YAML, a cache miss that generates
/// from the raw cache, `--verify` against DuckDB's counts, then a hit, drift
/// and `--force`, as for LUBM.
#[test]
fn bench_run_materialises_a_committed_yaml_and_verifies_every_count() {
    if skip_unsupported() {
        return;
    }
    let Some(duckdb) = locate_or_skip("bench_run_materialises_a_committed_yaml") else {
        return;
    };
    let cache = tempfile::tempdir().unwrap();
    fixture_raw_cache(&duckdb, cache.path());
    let workspace = workspace_with_job_1();
    let reports = tempfile::tempdir().unwrap();
    let subdir = cache.path().join("kermit/benchmarks/job-1");

    // 1. A miss: job-1 (template 1) is generated, then both queries verify (5
    //    and 7).
    let report = reports.path().join("miss.json");
    let output = bench_run(cache.path(), workspace.path(), &report, None, &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the first run failed; stderr: {stderr}"
    );
    assert!(subdir.join("meta.json").is_file() && subdir.join("benchmark.yml").is_file());
    assert_eq!(verified(&report), [
        ("1a".to_owned(), true),
        ("1b".to_owned(), true)
    ]);

    // 2. A hit: with no usable DuckDB the run succeeds only if nothing is
    //    regenerated.
    let report = reports.path().join("hit.json");
    let output = bench_run(
        cache.path(),
        workspace.path(),
        &report,
        Some("/nonexistent/kermit-test/duckdb"),
        &["-i", "tree-trie", "-a", "leapfrog-triejoin", "-q", "1a"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "a cache hit must not need DuckDB; stderr: {stderr}"
    );
    assert_eq!(verified(&report), [("1a".to_owned(), true)]);

    // 3. Drift: the YAML now names template 2, so the cached spec hash no
    //    longer matches.
    let yaml_path = workspace.path().join("benchmarks/job-1.yml");
    let yaml = fs::read_to_string(&yaml_path).unwrap();
    let drifted = yaml.replace("template: 1", "template: 2");
    assert_ne!(yaml, drifted, "job-1.yml must declare `template: 1`");
    fs::write(&yaml_path, drifted).unwrap();
    let report = reports.path().join("drift.json");
    let output = bench_run(cache.path(), workspace.path(), &report, None, &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "drift must fail without --force; stderr: {stderr}"
    );
    assert!(stderr.contains("spec drift"), "{stderr}");
    assert!(
        subdir.join("meta.json").is_file(),
        "drift must not delete the cache"
    );

    // 4. --force regenerates from the new spec: template 2's queries, including
    //    2b, whose relation `name_n_b` is empty and whose count is 0, on Hash
    //    Triejoin.
    let report = reports.path().join("force.json");
    let output = bench_run(cache.path(), workspace.path(), &report, None, &[
        "-i",
        "hash-trie",
        "-a",
        "hash-triejoin",
        "--force",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "--force failed; stderr: {stderr}");
    assert_eq!(verified(&report), [
        ("2a".to_owned(), true),
        ("2b".to_owned(), true)
    ]);
}
