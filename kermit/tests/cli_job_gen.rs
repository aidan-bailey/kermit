//! `kermit bench gen job` and `bench run job-N`, through the real binary.
//!
//! The first three tests fail before any generation starts and need no DuckDB.
//! The others build the fixture raw cache
//! (`kermit-generate/tests/fixtures/job/`) at `<XDG_CACHE_HOME>/kermit/imdb`,
//! where `raw::default_raw_dir()` looks, with a provenance equal to the pins,
//! and skip with a stderr line without DuckDB (CI installs none; under `nix
//! develop` they run).
//!
//! Every test redirects the platform cache with `XDG_CACHE_HOME`, which `dirs`
//! honours only on Linux, and passes `--output-dir` or `KERMIT_WORKSPACE`, so
//! nothing is written under `~/.cache/kermit`. The `bench run` test also sets
//! `CRITERION_HOME`, so its Criterion groups stay out of the workspace's
//! `target/criterion/`, and points the HTTP proxies at a closed port, so no
//! run can download anything.

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

/// The `--tag` every `bench gen` here passes, unless a test is about the tag.
const TAG: &str = "citest";

/// A `KERMIT_DUCKDB_BIN` that names no file, so a run that reaches DuckDB
/// fails with the variable's name instead of finding one on the host.
const MISSING_DUCKDB: &str = "/nonexistent/kermit-test/duckdb";

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
    gen_job_tagged(cache, out, template, TAG, duckdb_bin)
}

/// [`gen_job`] with the `--tag` given.
fn gen_job_tagged(
    cache: &Path, out: &Path, template: &str, tag: &str, duckdb_bin: Option<&str>,
) -> Output {
    let mut command = Command::new(kermit_bin());
    // `raw::ensure` downloads a raw cache it cannot find, so `gen` must never
    // reach the network from a test: the proxy is dead and `NO_PROXY` is
    // cleared, as in `bench_run`, so a missing cache fails at once instead
    // of downloading.
    command
        .env("XDG_CACHE_HOME", cache)
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .args(["bench", "gen", "job", "--template", template, "--tag", tag])
        .arg("--output-dir")
        .arg(out);
    if let Some(bin) = duckdb_bin {
        command.env("KERMIT_DUCKDB_BIN", bin);
    }
    command.output().expect("failed to execute kermit binary")
}

/// A template outside 1..=33 fails naming the flag and the range, before
/// DuckDB is looked for (the missing one would name another variable) and with
/// nothing written.
#[test]
fn template_outside_1_to_33_is_rejected_before_anything_is_written() {
    for template in ["0", "34"] {
        let cache = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let output = gen_job(cache.path(), out.path(), template, Some(MISSING_DUCKDB));
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
    let output = gen_job(cache.path(), out.path(), "1", Some(MISSING_DUCKDB));
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

/// A tag that makes an invalid benchmark name fails naming the name rule, not
/// DuckDB (the missing one would name `KERMIT_DUCKDB_BIN`), and leaves nothing
/// behind: the name is checked before the raw cache and the filters are built.
#[test]
fn a_bad_tag_is_rejected_before_anything_is_written() {
    let cache = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let output = gen_job_tagged(cache.path(), out.path(), "1", "a b", Some(MISSING_DUCKDB));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("name may only contain"), "{stderr}");
    assert!(!stderr.contains("KERMIT_DUCKDB_BIN"), "{stderr}");
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

/// `bench gen job` on the fixture raw cache writes `job-1-<tag>` into
/// `--output-dir`: a valid `meta.json` and `benchmark.yml` with the hand
/// counts, and nothing else in the cache.
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
/// `report` and the Criterion groups to `criterion` beside it; `duckdb_bin`
/// overrides `KERMIT_DUCKDB_BIN`. The proxies name a closed port and
/// `NO_PROXY` is cleared, so a run that tried to download would fail. `bench`'s
/// own flags (`--sample-size`, `--measurement-time`, `--warm-up-time`,
/// `--report-json`) belong to `bench`, not `run`, so they come before `run`, as
/// in `cli_bench_run_verify.rs`. `--verify` checks every count before the
/// timing.
fn bench_run(
    cache: &Path, workspace: &Path, report: &Path, duckdb_bin: Option<&str>, extra: &[&str],
) -> Output {
    let mut command = Command::new(kermit_bin());
    command
        .env("XDG_CACHE_HOME", cache)
        .env("KERMIT_WORKSPACE", workspace)
        .env("CRITERION_HOME", report.parent().unwrap().join("criterion"))
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
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
    let Some(duckdb) =
        locate_or_skip("bench_run_materialises_a_committed_yaml_and_verifies_every_count")
    else {
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
    let meta_text = fs::read_to_string(subdir.join("meta.json")).unwrap();
    let yaml_text = fs::read_to_string(subdir.join("benchmark.yml")).unwrap();

    // 2. A hit: with no usable DuckDB the run succeeds only if nothing is
    //    regenerated.
    let report = reports.path().join("hit.json");
    let output = bench_run(
        cache.path(),
        workspace.path(),
        &report,
        Some(MISSING_DUCKDB),
        &["-i", "tree-trie", "-a", "leapfrog-triejoin", "-q", "1a"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "a cache hit must not need DuckDB; stderr: {stderr}"
    );
    assert_eq!(verified(&report), [("1a".to_owned(), true)]);
    assert_eq!(
        fs::read_to_string(subdir.join("meta.json")).unwrap(),
        meta_text,
        "a hit must not regenerate"
    );
    assert_eq!(
        fs::read_to_string(subdir.join("benchmark.yml")).unwrap(),
        yaml_text,
        "a hit must not regenerate"
    );

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
    assert_eq!(
        fs::read_to_string(subdir.join("meta.json")).unwrap(),
        meta_text,
        "drift must leave the cache as it was"
    );
    assert_eq!(
        fs::read_to_string(subdir.join("benchmark.yml")).unwrap(),
        yaml_text,
        "drift must leave the cache as it was"
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
