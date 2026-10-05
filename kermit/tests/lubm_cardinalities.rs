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
//! join engine (`kermit-algos` + `kermit::db::lftj_join`). The WatDiv
//! equivalent, `watdiv_correctness.rs`, sits here for the same reason.
//!
//! Gated on `java` being on PATH and a present vendored jar — CI runners
//! without a JDK skip the test (mirrors `e2e_lubm`). Reference cardinalities
//! only hold for LUBM(1, 0), so the scale is pinned to 1.

mod common;

use {
    clap::ValueEnum,
    common::utils::{join_under_planner, load_parquet_relations, JoinEntry},
    kermit_algos::{
        CardinalityOptimiser, ColumnOrderPolicy, CostBasedOptimiser, JoinQuery, LeapfrogTriejoin,
        LexicographicOptimiser, Planner, QueryOptimiser,
    },
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{
        BinarySeek, Cardinality, ColumnTrie, GallopingSeek, LinearSeek, Relation, TreeTrie,
    },
    kermit_iters::TrieIterable,
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

/// Builds one optimiser: a planner owns its optimiser, so each policy
/// needs a fresh one.
type NewOptimiser = fn() -> Box<dyn QueryOptimiser>;

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

/// Loads the generated relations as `R` into a fresh engine planned by
/// `planner`, runs every query (building the reordered copies it reads
/// under `--column-orders any`), and returns one line per query whose
/// result count differs from the paper's reference cardinality. `label`
/// names the optimiser and policy in each line.
fn cardinality_mismatches<R: TrieIterable + Relation + Cardinality>(
    bench: &BenchmarkDefinition, dir: &Path, planner: &Planner, label: &str,
    expected: &HashMap<String, u64>,
) -> Vec<String> {
    let (relations, inputs) =
        load_parquet_relations::<R>(dir, bench.relations.iter().map(|r| r.name.as_str()));
    let mut database =
        <LeapfrogTriejoin as JoinEntry<R>>::database(relations, planner.required_statistics());

    // Collect every divergence so one run surfaces the complete picture
    // rather than failing on the first mismatch.
    let mut mismatches = Vec::new();
    for q in &bench.queries {
        let want = *expected
            .get(&q.name)
            .unwrap_or_else(|| panic!("no reference cardinality for query {}", q.name));

        let parsed: JoinQuery = q.query.parse().expect("datalog parse failure");
        let (rows, _) =
            join_under_planner::<R, LeapfrogTriejoin>(&mut database, &inputs, parsed, planner)
                .unwrap_or_else(|e| panic!("query {}: {e}", q.name));
        let got = rows.len() as u64;

        if got != want {
            mismatches.push(format!(
                "  [{label} / {}] {}: got {got}, expected {want}\n    query: {}",
                std::any::type_name::<R>(),
                q.name,
                q.query
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
    // Every column-order policy too: under `any` an atom may read a
    // reordered copy, which must give the same answers (issue #93).
    let optimisers: Vec<(&str, NewOptimiser)> = vec![
        ("lexicographic", || Box::new(LexicographicOptimiser)),
        ("cardinality", || Box::new(CardinalityOptimiser)),
        ("cost-based", || Box::new(CostBasedOptimiser::default())),
    ];
    let optimiser_count = optimisers.len();
    let policies = ColumnOrderPolicy::value_variants();

    let mut mismatches: Vec<String> = Vec::new();
    for &policy in policies {
        for (name, optimiser) in &optimisers {
            let planner = Planner::new(optimiser(), policy);
            let label = format!("{name} / {}", policy.axis_value());
            mismatches.extend(cardinality_mismatches::<TreeTrie<LinearSeek>>(
                &bench,
                out.path(),
                &planner,
                &label,
                &expected,
            ));
            mismatches.extend(cardinality_mismatches::<TreeTrie<BinarySeek>>(
                &bench,
                out.path(),
                &planner,
                &label,
                &expected,
            ));
            mismatches.extend(cardinality_mismatches::<TreeTrie<GallopingSeek>>(
                &bench,
                out.path(),
                &planner,
                &label,
                &expected,
            ));
            mismatches.extend(cardinality_mismatches::<ColumnTrie<LinearSeek>>(
                &bench,
                out.path(),
                &planner,
                &label,
                &expected,
            ));
            mismatches.extend(cardinality_mismatches::<ColumnTrie<BinarySeek>>(
                &bench,
                out.path(),
                &planner,
                &label,
                &expected,
            ));
            mismatches.extend(cardinality_mismatches::<ColumnTrie<GallopingSeek>>(
                &bench,
                out.path(),
                &planner,
                &label,
                &expected,
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "LUBM(1, 0) cardinality mismatches ({} across {} queries x {} optimisers x {} \
         column-order policies x 2 sorted tries x 3 seek strategies):\n{}",
        mismatches.len(),
        bench.queries.len(),
        optimiser_count,
        policies.len(),
        mismatches.join("\n"),
    );
}
