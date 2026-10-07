//! CLI smoke test for `HashTrie`'s `ds_build_mode` axis (issues #91, #94).
//! Every `HashTrie` report says which build made its relations: `bulk` by
//! default (`incremental` is the per-tuple build), `radix:<bits>`,
//! `parallel:<threads>` or `presized:<threads>` under
//! `--ds-build hash-trie=…`. Every mode builds an equivalent trie (the
//! identical one, except `presized:N`, whose root keys may sit in other
//! buckets), so only the axis (and build time) shows which ran.

mod common;

use common::cli::{axes_of, bench_ds, bench_join, bench_run, reports_of};

#[test]
fn cli_bench_ds_hash_trie_records_bulk_build_mode() {
    let (output, report) = bench_ds("hash-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "bulk");
}

#[test]
fn cli_bench_ds_with_radix_build_records_axis() {
    let (output, report) = bench_ds("hash-trie", &["--ds-build", "hash-trie=radix:4"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "radix:4");
}

#[test]
fn cli_bench_join_with_radix_build_records_axis() {
    let (output, report) = bench_join("hash-trie", "hash-triejoin", &[
        "-m",
        "space",
        "--ds-build",
        "hash-trie=radix:4",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "radix:4");
}

#[test]
fn cli_bench_run_sweep_carries_each_build_mode_to_its_cell() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-build",
        "hash-trie=radix:4,tree-trie=parallel:2",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        match axes["data_structure"].as_str() {
            | Some("HashTrie") => assert_eq!(axes["ds_build_mode"], "radix:4", "{axes}"),
            | Some("TreeTrie") => assert_eq!(axes["ds_build_mode"], "parallel:2", "{axes}"),
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}

#[test]
fn cli_bench_ds_with_parallel_build_records_axis() {
    let (output, report) = bench_ds("hash-trie", &["--ds-build", "hash-trie=parallel:2"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "parallel:2");
}

/// `bench join --output` builds through its own family
/// (`load_query_runner`), so this pins that route's mode, which the
/// `bench ds` and `bench run` tests do not reach.
#[test]
fn cli_bench_join_with_output_records_a_parallel_build() {
    let csv = tempfile::NamedTempFile::new().expect("temp csv");
    let csv_path = csv.path().to_str().expect("utf-8 temp path");
    let (output, report) = bench_join("hash-trie", "hash-triejoin", &[
        "-m",
        "space",
        "--ds-build",
        "hash-trie=parallel:3",
        "--output",
        csv_path,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "parallel:3");
}

/// `--verify` checks the answer counts of parallel-built tries against the
/// benchmark's expected values, eager and lazy.
#[test]
fn cli_bench_run_verifies_answers_from_parallel_built_tries() {
    for expansion in ["eager", "lazy"] {
        let (output, report) = bench_run("triangle", &[
            "-i",
            "hash-trie",
            "-a",
            "hash-triejoin",
            "-m",
            "iteration",
            "--verify",
            "--ds-layout-expansion",
            expansion,
            "--ds-build",
            "hash-trie=parallel:2",
        ]);
        assert!(
            output.status.success(),
            "{expansion}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let axes = axes_of(&report);
        assert_eq!(axes["ds_build_mode"], "parallel:2", "{expansion}: {axes}");
        assert_eq!(axes["verified"], true, "{expansion}: {axes}");
    }
}

/// The same mode name reaches each trie with that trie's own thread count.
#[test]
fn cli_bench_run_sweep_carries_each_structures_parallel_mode() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-build",
        "hash-trie=parallel:3,tree-trie=parallel:2",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        match axes["data_structure"].as_str() {
            | Some("HashTrie") => assert_eq!(axes["ds_build_mode"], "parallel:3", "{axes}"),
            | Some("TreeTrie") => assert_eq!(axes["ds_build_mode"], "parallel:2", "{axes}"),
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}

#[test]
fn cli_bench_ds_rejects_malformed_hash_trie_modes() {
    for mode in [
        "radix",
        "radix:0",
        "radix:17",
        "serial",
        "parallel",
        "parallel:0",
        "parallel:1025",
        "parallel:x",
        "presized",
        "presized:0",
    ] {
        let pair = format!("hash-trie={mode}");
        let (output, _) = bench_ds("hash-trie", &["--ds-build", &pair]);
        assert!(!output.status.success(), "{pair} was accepted");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--ds-build hash-trie"), "{pair}: {stderr}");
        assert!(
            stderr.contains(
                "expected bulk, incremental, radix:<bits>, parallel:<threads> or \
                 presized:<threads>"
            ),
            "{pair}: {stderr}"
        );
    }
}

/// `serial` named the per-tuple build until #107; the CLI rejects it and
/// names what replaced it.
#[test]
fn cli_rejects_the_retired_serial_build() {
    let (output, _) = bench_ds("hash-trie", &["--ds-build", "hash-trie=serial"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("serial was renamed incremental in #107"),
        "{stderr}"
    );
    assert!(
        stderr.contains("the default is now bulk (Algorithm 2)"),
        "{stderr}"
    );
}

#[test]
fn cli_bench_ds_records_the_incremental_build() {
    let (output, report) = bench_ds("hash-trie", &["--ds-build", "hash-trie=incremental"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "incremental");
}

/// `incremental` cannot size a child it creates on its first tuple.
#[test]
fn cli_rejects_incremental_with_sized_children() {
    let (output, _) = bench_ds("hash-trie", &[
        "--ds-config",
        "child-capacity=tuples",
        "--ds-build",
        "hash-trie=incremental",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "--ds-build hash-trie=incremental requires --ds-config child-capacity=grow; got \
             child-capacity=tuples"
        ),
        "{stderr}"
    );
}

/// `presized:N` is the paper's presized build; it requires
/// `root-capacity=tuples`. One report records both.
#[test]
fn cli_bench_ds_records_a_presized_parallel_build() {
    let (output, report) = bench_ds("hash-trie", &[
        "--ds-config",
        "root-capacity=tuples",
        "--ds-build",
        "hash-trie=presized:2",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let axes = axes_of(&report);
    assert_eq!(axes["ds_build_mode"], "presized:2", "{axes}");
    assert_eq!(axes["ds_config_root_capacity"], "tuples", "{axes}");
}

/// `--verify` checks the answers of presized parallel builds, eager and
/// lazy, at the paper's load factor.
#[test]
fn cli_bench_run_verifies_presized_parallel_builds() {
    for expansion in ["eager", "lazy"] {
        let (output, report) = bench_run("triangle", &[
            "-i",
            "hash-trie",
            "-a",
            "hash-triejoin",
            "-m",
            "iteration",
            "--verify",
            "--ds-layout-expansion",
            expansion,
            "--ds-config",
            "root-capacity=tuples,load-factor=0.8",
            "--ds-build",
            "hash-trie=presized:3",
        ]);
        assert!(
            output.status.success(),
            "{expansion}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let axes = axes_of(&report);
        assert_eq!(axes["ds_build_mode"], "presized:3", "{expansion}: {axes}");
        assert_eq!(axes["verified"], true, "{expansion}: {axes}");
        assert_eq!(
            axes["ds_config_root_capacity"], "tuples",
            "{expansion}: {axes}"
        );
    }
}

/// `presized:N` requires `root-capacity=tuples`: the prerequisite table
/// (`Prerequisite` in `kermit/src/options.rs`) rejects the pair before
/// anything is built, on every command, naming the flag to add.
#[test]
fn cli_rejects_presized_without_a_presized_root() {
    const MESSAGE: &str = "--ds-build hash-trie=presized:2 requires --ds-config \
                           root-capacity=tuples; got root-capacity=grow (the default)";
    let (output, _) = bench_ds("hash-trie", &["--ds-build", "hash-trie=presized:2"]);
    assert!(
        !output.status.success(),
        "bench ds accepted presized:2 under grow"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(MESSAGE), "bench ds: {stderr}");

    let (output, _) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-config",
        "root-capacity=grow",
        "--ds-build",
        "hash-trie=presized:2",
    ]);
    assert!(
        !output.status.success(),
        "bench run accepted presized:2 under grow"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(MESSAGE), "bench run: {stderr}");

    let (output, _) = bench_join("hash-trie", "hash-triejoin", &[
        "-m",
        "space",
        "--ds-build",
        "hash-trie=presized:2",
    ]);
    assert!(
        !output.status.success(),
        "bench join accepted presized:2 under grow"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(MESSAGE), "bench join: {stderr}");
}

/// `--verify` checks the answers of the closest-to-paper configuration,
/// pruned, eager and lazy (#107).
#[test]
fn cli_bench_run_verifies_the_papers_configuration() {
    for expansion in ["eager", "lazy"] {
        let (output, report) = bench_run("triangle", &[
            "-i",
            "hash-trie",
            "-a",
            "hash-triejoin",
            "-m",
            "iteration",
            "--verify",
            "--ds-layout-pruning",
            "on",
            "--ds-layout-expansion",
            expansion,
            "--ds-config",
            "root-capacity=tuples,child-capacity=tuples,load-factor=0.8",
            "--ds-build",
            "hash-trie=presized:3",
        ]);
        assert!(
            output.status.success(),
            "{expansion}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let axes = axes_of(&report);
        assert_eq!(axes["verified"], true, "{expansion}: {axes}");
        assert_eq!(
            axes["ds_config_child_capacity"], "tuples",
            "{expansion}: {axes}"
        );
        assert_eq!(axes["ds_build_mode"], "presized:3", "{expansion}: {axes}");
    }
}
