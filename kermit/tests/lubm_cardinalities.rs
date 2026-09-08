//! Cardinality regression test for the LUBM pipeline — the load-bearing
//! correctness check for the Univ-Bench TBox entailment rule set.
//!
//! Generates LUBM(1, 0) end-to-end (drive the vendored jar → entail →
//! partition → translate → emit), then runs all 14 queries through kermit's
//! join engine and asserts each result count matches the published LUBM
//! paper Table 3 reference cardinality (carried by
//! [`kermit_rdf::lubm::queries::lubm_query_specs`]).
//!
//! This lives in the `kermit` crate (not `kermit-rdf`) because only the
//! binary crate depends on *both* the LUBM pipeline (`kermit-rdf`) and the
//! join engine (`kermit-algos` + [`DatabaseEngine`]). The WatDiv equivalent,
//! `watdiv_correctness.rs`, sits here for the same reason.
//!
//! Gated on `java` being on PATH and a present vendored jar — CI runners
//! without a JDK skip the test (mirrors `e2e_lubm`). Reference cardinalities
//! only hold for LUBM(1, 0), so the scale is pinned to 1.

use {
    kermit::db::{DatabaseEngine, DB},
    kermit_algos::{
        CardinalityOptimiser, JoinQuery, LeapfrogTriejoin, LexicographicOptimiser, QueryOptimiser,
    },
    kermit_bench::BenchmarkDefinition,
    kermit_ds::TreeTrie,
    kermit_rdf::lubm::{
        driver::{LubmDriverInputs, DEFAULT_ONTOLOGY_IRI},
        pipeline::{run_lubm_pipeline, LubmPipelineInputs},
        queries::lubm_query_specs,
    },
    std::{
        collections::HashMap,
        path::{Path, PathBuf},
        process::Command,
    },
};

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The vendored jar lives in the sibling `kermit-rdf` crate; this test runs
/// from the `kermit` crate's manifest dir, so reach across the workspace.
fn vendored_jar() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../kermit-rdf/vendor/lubm-uba/lubm-uba.jar")
}

/// Loads the generated relations into a fresh engine planned by `optimiser`,
/// runs every query, and returns one line per query whose result count
/// differs from the paper's reference cardinality.
fn cardinality_mismatches(
    bench: &BenchmarkDefinition, dir: &Path, optimiser_name: &str,
    optimiser: Box<dyn QueryOptimiser>, expected: &HashMap<String, u64>,
) -> Vec<String> {
    let mut db: DatabaseEngine<TreeTrie, LeapfrogTriejoin> =
        DatabaseEngine::with_optimiser(bench.name.clone(), optimiser);
    for rel in &bench.relations {
        let path = dir.join(format!("{}.parquet", rel.name));
        db.add_file(&path)
            .unwrap_or_else(|e| panic!("failed to load relation {path:?}: {e}"));
    }

    // Collect every divergence so one run surfaces the complete picture
    // rather than failing on the first mismatch.
    let mut mismatches = Vec::new();
    for q in &bench.queries {
        let want = *expected
            .get(&q.name)
            .unwrap_or_else(|| panic!("no reference cardinality for query {}", q.name));

        let parsed: JoinQuery = q.query.parse().expect("datalog parse failure");
        let got = db.join(parsed).len() as u64;

        if got != want {
            mismatches.push(format!(
                "  [{optimiser_name}] {}: got {got}, expected {want}\n    query: {}",
                q.name, q.query
            ));
        }
    }
    mismatches
}

#[test]
#[cfg_attr(miri, ignore = "miri does not support spawning java")]
fn lubm_one_university_query_cardinalities_match_paper() {
    if !java_available() {
        eprintln!("skipping: `java` not on PATH");
        return;
    }
    let jar = vendored_jar();
    if !jar.exists() {
        eprintln!("skipping: vendored jar missing at {jar:?}");
        return;
    }

    // The 14 queries carry their LUBM(1, 0) reference cardinalities; this is
    // both the workload we generate and the oracle we assert against.
    let specs = lubm_query_specs(true);
    let expected: HashMap<String, u64> = specs
        .iter()
        .filter_map(|s| s.expected_cardinality.map(|c| (s.name.clone(), c)))
        .collect();
    assert_eq!(
        expected.len(),
        14,
        "all 14 queries must carry a reference cardinality"
    );

    // Generate LUBM(1, 0) end-to-end into a temp dir.
    let out = tempfile::tempdir().expect("create temp dir");
    let inputs = LubmPipelineInputs {
        driver: LubmDriverInputs {
            jar_path: &jar,
            scale: 1,
            seed: 0,
            start_index: 0,
            threads: 1,
            ontology_iri: DEFAULT_ONTOLOGY_IRI,
        },
        out_dir: out.path(),
        bench_name: "lubm-1",
        tag: "cardinality-test",
        queries: &specs,
        spec_hash: None,
    };
    run_lubm_pipeline(&inputs).expect("LUBM(1, 0) pipeline must succeed");

    // Load the emitted benchmark definition and its relations.
    let yaml = std::fs::read_to_string(out.path().join("benchmark.yml"))
        .expect("emitted benchmark.yml missing");
    let bench: BenchmarkDefinition = serde_yaml::from_str(&yaml).expect("benchmark.yml malformed");

    // Every optimiser must reproduce the reference cardinalities: the plan
    // it picks changes the join's descent order, never its answer. This is
    // the only test with real, deeply-pruning data, so it is where executor
    // bugs that just one variable ordering exposes actually surface — a
    // failed descent at depth 3 or deeper once silently dropped every answer
    // to q7 under `cardinality` while `lexicographic` stayed correct. Add a
    // row here whenever an optimiser is added.
    let optimisers: Vec<(&str, Box<dyn QueryOptimiser>)> = vec![
        ("lexicographic", Box::new(LexicographicOptimiser)),
        ("cardinality", Box::new(CardinalityOptimiser)),
    ];
    let optimiser_count = optimisers.len();

    let mut mismatches: Vec<String> = Vec::new();
    for (name, optimiser) in optimisers {
        mismatches.extend(cardinality_mismatches(
            &bench,
            out.path(),
            name,
            optimiser,
            &expected,
        ));
    }

    assert!(
        mismatches.is_empty(),
        "LUBM(1, 0) cardinality mismatches ({} across {} queries x {} optimisers):\n{}",
        mismatches.len(),
        bench.queries.len(),
        optimiser_count,
        mismatches.join("\n"),
    );
}
