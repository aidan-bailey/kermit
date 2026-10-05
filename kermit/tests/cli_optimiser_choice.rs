//! CLI smoke tests: `--optimiser` selection lands in the bench-report
//! `optimiser` axis, defaulting to `lexicographic`.

use {
    std::{fs, path::PathBuf, process::Command},
    tempfile::NamedTempFile,
};

fn kermit_bin() -> &'static str { env!("CARGO_BIN_EXE_kermit") }

fn fixtures_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures") }

fn edge_fixture() -> PathBuf { fixtures_dir().join("edge.csv") }

/// Runs `bench join` against the edge fixture with `extra_args` appended,
/// returning the parsed report array.
fn run_bench_join(extra_args: &[&str]) -> Vec<serde_json::Value> {
    let report = NamedTempFile::new().expect("failed to create temp report file");

    let mut cmd = Command::new(kermit_bin());
    cmd.args([
        "bench",
        "--sample-size",
        "10",
        "--measurement-time",
        "1",
        "--warm-up-time",
        "1",
        "--report-json",
        report.path().to_str().unwrap(),
        "join",
        "--relations",
        edge_fixture().to_str().unwrap(),
        "--query",
        fixtures_dir().join("path_query.dl").to_str().unwrap(),
        "--algorithm",
        "leapfrog-triejoin",
        "--indexstructure",
        "tree-trie",
    ]);
    cmd.args(extra_args);
    let output = cmd.output().expect("failed to run kermit binary");
    assert!(
        output.status.success(),
        "kermit bench join failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let reports: Vec<serde_json::Value> =
        serde_json::from_str(&fs::read_to_string(report.path()).unwrap()).unwrap();
    assert!(!reports.is_empty(), "report array should not be empty");
    reports
}

#[test]
fn cli_bench_join_with_cardinality_optimiser_records_axis() {
    let reports = run_bench_join(&["--optimiser", "cardinality"]);
    assert_eq!(reports[0]["axes"]["optimiser"], "cardinality");
    // Sibling axis guard: the optimiser entry must extend the axes map,
    // not displace existing keys.
    assert_eq!(reports[0]["axes"]["algorithm"], "LeapfrogTriejoin");
}

#[test]
fn cli_bench_join_default_optimiser_is_lexicographic() {
    let reports = run_bench_join(&[]);
    assert_eq!(reports[0]["axes"]["optimiser"], "lexicographic");
    // Sibling axis guard: the optimiser entry must extend the axes map,
    // not displace existing keys.
    assert_eq!(reports[0]["axes"]["data_structure"], "TreeTrie");
}

#[test]
fn cli_bench_join_with_cost_based_optimiser_records_axis() {
    let reports = run_bench_join(&["--optimiser", "cost-based"]);
    assert_eq!(reports[0]["axes"]["optimiser"], "cost-based");
    // Sibling axis guard: the optimiser entry must extend the axes map,
    // not displace existing keys.
    assert_eq!(reports[0]["axes"]["algorithm"], "LeapfrogTriejoin");
}

/// `kermit join` writes the same rows under every optimiser, in every
/// cell: the plan changes the descent order, never the answer.
#[test]
fn cli_join_answers_identically_under_every_optimiser() {
    let rows = |optimiser: &str, structure: &str, algorithm: &str| -> Vec<String> {
        let output = Command::new(kermit_bin())
            .args([
                "join",
                "--relations",
                edge_fixture().to_str().unwrap(),
                "--query",
                fixtures_dir().join("path_query.dl").to_str().unwrap(),
                "--algorithm",
                algorithm,
                "--indexstructure",
                structure,
                "--optimiser",
                optimiser,
            ])
            .output()
            .expect("failed to run kermit binary");
        assert!(
            output.status.success(),
            "kermit join --optimiser {optimiser} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut lines: Vec<String> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        lines.sort();
        lines
    };
    for (structure, algorithm) in [
        ("tree-trie", "leapfrog-triejoin"),
        ("column-trie", "leapfrog-triejoin"),
        ("hash-trie", "hash-triejoin"),
    ] {
        let expected = rows("lexicographic", structure, algorithm);
        assert!(expected.len() > 1, "the fixture query must return rows");
        for optimiser in ["cardinality", "cost-based"] {
            assert_eq!(
                rows(optimiser, structure, algorithm),
                expected,
                "{optimiser} on {structure}"
            );
        }
    }
}
