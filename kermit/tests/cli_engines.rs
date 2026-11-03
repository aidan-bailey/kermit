//! The engine CLI: `--engine` and `--engine-threads` on `bench run`, `bench
//! probe` and `bench join`. The usage errors hold in every build; the report
//! tests run only with both engine features (`--features
//! engine-duckdb,engine-nemo`), on the committed `triangle` benchmark, so they
//! need no network and no cache.

use {
    std::{
        fs,
        path::PathBuf,
        process::{Command, Output},
    },
    tempfile::TempDir,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

/// Runs `kermit bench --sample-size 10 --measurement-time 1 --warm-up-time 1
/// --report-json <tmp>/report.json <subcommand and args>` with Criterion and
/// the cache redirected into `tmp`. Returns the output and the parsed report
/// array (empty when no report was written).
fn bench(tmp: &TempDir, name: Option<&str>, args: &[&str]) -> (Output, Vec<serde_json::Value>) {
    let report = tmp.path().join("report.json");
    let mut command = Command::new(kermit_bin());
    command
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LIB_BACKTRACE", "0")
        .env("CRITERION_HOME", tmp.path().join("criterion"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .arg("bench");
    if let Some(name) = name {
        command.args(["--name", name]);
    }
    let output = command
        .args([
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
            "--report-json",
        ])
        .arg(&report)
        .args(args)
        .output()
        .expect("failed to spawn kermit");
    let reports = fs::read_to_string(&report)
        .ok()
        .map(|text| serde_json::from_str(&text).expect("the report is a JSON array"))
        .unwrap_or_default();
    (output, reports)
}

fn stderr(output: &Output) -> String { String::from_utf8_lossy(&output.stderr).into_owned() }

#[test]
fn without_engine_the_selectors_stay_required() {
    let tmp = TempDir::new().unwrap();
    let (output, reports) = bench(&tmp, None, &["run", "triangle"]);
    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("--indexstructure"), "{stderr}");
    assert!(reports.is_empty());
}

#[test]
fn one_selector_without_the_other_is_refused() {
    let tmp = TempDir::new().unwrap();
    let (output, reports) = bench(&tmp, None, &[
        "run", "triangle", "-i", "all", "--engine", "nemo",
    ]);
    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("--algorithm"), "{stderr}");
    assert!(reports.is_empty());
}

#[test]
fn engine_threads_outside_its_range_is_a_usage_error() {
    let tmp = TempDir::new().unwrap();
    for threads in ["0", "999999"] {
        let (output, reports) = bench(&tmp, None, &[
            "probe",
            "triangle",
            "--engine",
            "duckdb",
            "--engine-threads",
            threads,
        ]);
        assert!(!output.status.success(), "{threads}");
        let stderr = stderr(&output);
        assert!(
            stderr.contains("--engine-threads must be between 1 and "),
            "{threads}: {stderr}"
        );
        assert!(reports.is_empty());
    }
}

/// The flag reaches DuckDB only: without a DuckDB engine it is refused (before
/// its range is checked, so `0` is no range error), as #86 refuses a flag with
/// no cell to reach.
#[test]
fn engine_threads_without_a_duckdb_engine_is_a_usage_error() {
    let tmp = TempDir::new().unwrap();
    for args in [
        &[
            "probe",
            "triangle",
            "--engine",
            "nemo",
            "--engine-threads",
            "0",
        ][..],
        &[
            "probe",
            "triangle",
            "--engine",
            "nemo",
            "--engine-threads",
            "2",
        ][..],
        &[
            "run",
            "triangle",
            "-i",
            "tree-trie",
            "-a",
            "leapfrog-triejoin",
            "--engine-threads",
            "2",
        ][..],
    ] {
        let (output, reports) = bench(&tmp, None, args);
        assert!(!output.status.success(), "{args:?}");
        let stderr = stderr(&output);
        assert!(
            stderr.contains("--engine-threads applies to --engine duckdb only"),
            "{args:?}: {stderr}"
        );
        assert!(!stderr.contains("must be between"), "{args:?}: {stderr}");
        assert!(reports.is_empty());
    }
}

#[cfg(not(feature = "engine-nemo"))]
#[test]
fn an_engine_without_its_feature_is_a_usage_error() {
    let tmp = TempDir::new().unwrap();
    let (output, reports) = bench(&tmp, None, &["probe", "triangle", "--engine", "nemo"]);
    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(
        stderr.contains(
            "--engine nemo needs the engine-nemo feature (build with --features engine-nemo)"
        ),
        "{stderr}"
    );
    assert!(reports.is_empty());
}

#[cfg(feature = "engine-nemo")]
#[test]
fn a_ds_flag_with_no_kermit_cell_is_refused() {
    let tmp = TempDir::new().unwrap();
    let (output, reports) = bench(&tmp, None, &[
        "probe",
        "triangle",
        "--engine",
        "nemo",
        "--ds-layout-seek",
        "galloping",
    ]);
    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(
        stderr.contains("--ds-layout-seek applies only to tree-trie or column-trie cells"),
        "{stderr}"
    );
    assert!(reports.is_empty());
}

#[cfg(all(feature = "engine-duckdb", feature = "engine-nemo"))]
mod with_both_engines {
    use super::*;

    /// The report's `criterion_groups` functions, sorted.
    fn functions(report: &serde_json::Value) -> Vec<String> {
        let mut functions: Vec<String> = report["criterion_groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|group| group["function"].as_str().unwrap().to_string())
            .collect();
        functions.sort();
        functions
    }

    fn value<'r>(report: &'r serde_json::Value, function: &str) -> Option<&'r serde_json::Value> {
        report["values"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["function"] == function)
    }

    fn by_system<'r>(reports: &'r [serde_json::Value], system: &str) -> Vec<&'r serde_json::Value> {
        reports
            .iter()
            .filter(|report| report["axes"]["system"] == system)
            .collect()
    }

    /// Spec Acceptance 2: every kermit cell (four, with the hash index's) and
    /// two engine cells in one v8 report.
    #[test]
    fn a_sweep_beside_the_engines_writes_a_v8_report() {
        let tmp = TempDir::new().unwrap();
        let (output, reports) = bench(&tmp, None, &[
            "run",
            "triangle",
            "-i",
            "all",
            "-a",
            "all",
            "--engine",
            "duckdb,nemo",
            "--metrics",
            "build",
            "iteration",
            "space",
            "materialize",
            "end-to-end",
            "--verify",
        ]);
        let stderr = stderr(&output);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(
            reports.len(),
            6,
            "one query on four kermit cells and two engines"
        );
        for report in &reports {
            assert_eq!(report["schema_version"], 8);
            assert_eq!(report["axes"]["verified"], true, "{report}");
            assert_eq!(report["axes"]["answer_rows"], 4, "{report}");
        }
        assert_eq!(by_system(&reports, "kermit").len(), 4);

        let duckdb = by_system(&reports, "duckdb");
        let nemo = by_system(&reports, "nemo");
        assert_eq!((duckdb.len(), nemo.len()), (1, 1));
        let (duckdb, nemo) = (duckdb[0], nemo[0]);
        for engine in [duckdb, nemo] {
            for absent in ["data_structure", "algorithm", "ds_layout_seek"] {
                assert!(engine["axes"].get(absent).is_none(), "{absent}: {engine}");
            }
            assert!(engine["axes"]["engine_version"].is_string(), "{engine}");
        }
        assert_eq!(duckdb["axes"]["engine_threads"], 1);
        assert!(duckdb["axes"]["engine_version"]
            .as_str()
            .unwrap()
            .starts_with('v'));
        assert!(nemo["axes"].get("engine_threads").is_none());
        assert!(nemo["axes"]["engine_version"]
            .as_str()
            .unwrap()
            .starts_with("nemo-physical"));

        // DuckDB streams and reports no space; Nemo stores its result, so it
        // skips the streamed metrics, and reports its trie's space.
        assert_eq!(functions(duckdb), [
            "build",
            "end_to_end",
            "iteration",
            "materialize"
        ]);
        assert!(value(duckdb, "space").is_none());
        assert_eq!(functions(nemo), ["build", "materialize"]);
        assert_eq!(value(nemo, "space").unwrap()["estimator"], "exact");
        for (report, segment) in [(duckdb, "/duckdb-t1"), (nemo, "/nemo")] {
            for group in report["criterion_groups"].as_array().unwrap() {
                let name = group["group"].as_str().unwrap();
                assert!(name.ends_with(segment), "{name}");
            }
        }
        assert!(
            stderr.contains("note: nemo does not stream, so it skips iteration and end-to-end"),
            "{stderr}"
        );
        assert!(
            stderr.contains("note: duckdb-t1 reports no space"),
            "{stderr}"
        );
        assert!(
            !stderr.contains("note: duckdb-t1 does not stream"),
            "{stderr}"
        );
    }

    #[test]
    fn engines_alone_need_no_selectors_and_a_probe_times_one_build() {
        let tmp = TempDir::new().unwrap();
        let (output, reports) = bench(&tmp, None, &[
            "probe",
            "triangle",
            "--engine",
            "duckdb,nemo",
            "--materialize",
        ]);
        let stderr = stderr(&output);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().all(|report| report["kind"] == "probe"));
        let duckdb = by_system(&reports, "duckdb")[0];
        let nemo = by_system(&reports, "nemo")[0];
        for phase in ["build", "iteration", "materialize"] {
            assert_eq!(
                value(duckdb, phase).unwrap()["estimator"],
                "single",
                "{phase}"
            );
        }
        assert_eq!(value(nemo, "build").unwrap()["estimator"], "single");
        assert_eq!(value(nemo, "materialize").unwrap()["estimator"], "single");
        assert!(
            value(nemo, "iteration").is_none(),
            "Nemo does not stream: {nemo}"
        );
        assert!(
            stderr.contains("note: nemo does not stream, so it skips iteration and end-to-end"),
            "{stderr}"
        );
    }

    #[test]
    fn the_group_clash_check_covers_engine_cells() {
        let tmp = TempDir::new().unwrap();
        // `{prefix}/triangle/triangle/{segment}`: the engine segments start
        // past byte 64.
        let prefix = "run-with-a-prefix-long-enough-to-push-the-engine-segment-past-byte-64";
        let (output, reports) = bench(&tmp, Some(prefix), &[
            "run",
            "triangle",
            "--engine",
            "duckdb,nemo",
            "-m",
            "build",
        ]);
        let stderr = stderr(&output);
        assert!(!output.status.success(), "{stderr}");
        assert!(
            stderr.contains(&format!("{prefix}/triangle/triangle/duckdb-t1")),
            "{stderr}"
        );
        assert!(
            stderr.contains(&format!("{prefix}/triangle/triangle/nemo")),
            "{stderr}"
        );
        assert!(stderr.contains("64"), "{stderr}");
        assert!(
            !tmp.path().join("criterion").exists(),
            "nothing is timed before the refusal"
        );
        assert!(reports.is_empty());
    }

    /// `bench join` names its groups `{prefix}/adhoc/{stem}/{segment}`, and
    /// its kermit cell and its engines are checked together, before anything
    /// runs.
    #[test]
    fn bench_join_refuses_groups_that_share_a_criterion_directory() {
        let tmp = TempDir::new().unwrap();
        let query = tmp.path().join("triangle.dl");
        fs::write(&query, "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).").unwrap();
        let edge =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../benchmarks/data/triangle/edge.csv");
        let prefix = "join-with-a-prefix-long-enough-to-push-the-segment-past-byte-64";
        let (output, reports) = bench(&tmp, Some(prefix), &[
            "join",
            "-r",
            edge.to_str().unwrap(),
            "-q",
            query.to_str().unwrap(),
            "-i",
            "tree-trie",
            "-a",
            "leapfrog-triejoin",
            "--engine",
            "duckdb,nemo",
            "--metrics",
            "build",
        ]);
        let stderr = stderr(&output);
        assert!(!output.status.success(), "{stderr}");
        assert!(
            stderr.contains(&format!(
                "{prefix}/adhoc/triangle/TreeTrie/LeapfrogTriejoin"
            )),
            "{stderr}"
        );
        assert!(
            stderr.contains(&format!("{prefix}/adhoc/triangle/duckdb-t1")),
            "{stderr}"
        );
        assert!(stderr.contains("64"), "{stderr}");
        assert!(
            !tmp.path().join("criterion").exists(),
            "nothing is timed before the refusal"
        );
        assert!(reports.is_empty());
    }

    /// `bench join` takes `-i` / `-a` as before and adds the engines beside
    /// them.
    #[test]
    fn bench_join_adds_engines_beside_its_one_kermit_cell() {
        let tmp = TempDir::new().unwrap();
        let query = tmp.path().join("triangle.dl");
        fs::write(&query, "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).").unwrap();
        let edge =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../benchmarks/data/triangle/edge.csv");
        let (output, reports) = bench(&tmp, None, &[
            "join",
            "-r",
            edge.to_str().unwrap(),
            "-q",
            query.to_str().unwrap(),
            "-i",
            "tree-trie",
            "-a",
            "leapfrog-triejoin",
            "--engine",
            "duckdb,nemo",
            "--metrics",
            "build",
            "space",
        ]);
        let stderr = stderr(&output);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(reports.len(), 3, "{stderr}");
        let systems: Vec<&str> = reports
            .iter()
            .map(|report| report["axes"]["system"].as_str().unwrap())
            .collect();
        assert_eq!(systems, ["kermit", "duckdb", "nemo"]);
        assert!(reports.iter().all(|report| report["kind"] == "join"));
    }
}

/// The memory guard on an engine that does not stream. A 400-row relation `e`
/// gives the `star` query 160000 answers of 3 variables (3840000 bytes) and the
/// `plain` query 400.
#[cfg(feature = "engine-nemo")]
mod nemo_materialize_guard {
    use super::*;

    fn workspace() -> (TempDir, TempDir) {
        let workspace = TempDir::new().unwrap();
        let cache = TempDir::new().unwrap();
        let data = workspace.path().join("data");
        fs::create_dir_all(&data).unwrap();
        let mut rows = String::from("src,dst\n");
        for i in 0..400 {
            rows.push_str(&format!("0,{i}\n"));
        }
        fs::write(data.join("e.csv"), rows).unwrap();
        let benchmarks = workspace.path().join("benchmarks");
        fs::create_dir_all(&benchmarks).unwrap();
        fs::write(
            benchmarks.join("guard.yml"),
            r#"name: guard
description: "the materialize guard on an engine that does not stream"
relations:
  - name: e
    path: "data/e.csv"
queries:
  - name: star
    description: "160000 answers of 3 variables, with expected"
    query: "Q(A, B, C) :- e(A, B), e(A, C)."
    expected: 160000
  - name: plain
    description: "400 answers, no expected"
    query: "Q(A, B) :- e(A, B)."
"#,
        )
        .unwrap();
        (workspace, cache)
    }

    /// `kermit bench <Criterion settings> --report-json <report> run guard -q
    /// <query> --engine nemo -m build materialize` plus `extra`, in
    /// `workspace`.
    fn run(
        workspace: &TempDir, cache: &TempDir, query: &str, extra: &[&str],
    ) -> (Output, Vec<serde_json::Value>) {
        let report = workspace.path().join("report.json");
        let output = Command::new(kermit_bin())
            .env("RUST_BACKTRACE", "0")
            .env("RUST_LIB_BACKTRACE", "0")
            .env("KERMIT_WORKSPACE", workspace.path())
            .env("XDG_CACHE_HOME", cache.path())
            .env("CRITERION_HOME", workspace.path().join("criterion"))
            .current_dir(workspace.path())
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
            .arg(&report)
            .args([
                "run",
                "guard",
                "-q",
                query,
                "--engine",
                "nemo",
                "-m",
                "build",
                "materialize",
            ])
            .args(extra)
            .output()
            .expect("failed to spawn kermit");
        let reports = fs::read_to_string(&report)
            .ok()
            .map(|text| serde_json::from_str(&text).expect("the report is a JSON array"))
            .unwrap_or_default();
        (output, reports)
    }

    fn functions(report: &serde_json::Value) -> Vec<String> {
        report["criterion_groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|group| group["function"].as_str().unwrap().to_string())
            .collect()
    }

    /// Over the limit, nothing materialises: the note and the metadata line are
    /// printed, the report has no `materialize` function and no counted
    /// `answer_rows`, because neither the preflight nor the describe step
    /// counted (Nemo can count only through `materialize`).
    #[test]
    fn a_result_over_the_limit_is_never_materialised() {
        let (workspace, cache) = workspace();
        let (output, reports) = run(&workspace, &cache, "star", &[
            "--materialize-max-bytes",
            "1048576",
        ]);
        let err = stderr(&output);
        assert!(output.status.success(), "{err}");
        assert!(
            err.contains(
                "note: skipping materialize for guard/star on nemo: 3840000 bytes over \
                 --materialize-max-bytes 1048576"
            ),
            "{err}"
        );
        assert!(err.contains("skipped: 3840000 bytes over 1048576"), "{err}");
        assert_eq!(reports.len(), 1, "{err}");
        assert_eq!(functions(&reports[0]), ["build"]);
        assert!(
            reports[0]["axes"].get("answer_rows").is_none(),
            "{}",
            reports[0]["axes"]
        );
    }

    /// With no `expected` the guard cannot decide: the warning is printed once
    /// and `materialize` runs, its count being the describe step's.
    #[test]
    fn without_expected_the_warning_is_printed_once_and_materialize_runs() {
        let (workspace, cache) = workspace();
        let (output, reports) = run(&workspace, &cache, "plain", &[]);
        let err = stderr(&output);
        assert!(output.status.success(), "{err}");
        let warning = "warning: nemo does not stream and guard/plain has no expected count, so \
                       materialize runs without the --materialize-max-bytes guard";
        assert_eq!(err.matches(warning).count(), 1, "{err}");
        assert!(!err.contains("skipping materialize"), "{err}");
        assert_eq!(reports.len(), 1, "{err}");
        let mut functions = functions(&reports[0]);
        functions.sort();
        assert_eq!(functions, ["build", "materialize"]);
        assert_eq!(reports[0]["axes"]["answer_rows"], 400);
    }
}
