//! End-to-end integration test for `kermit_rdf::lubm::pipeline`.
//!
//! Runs the full driver → entail → partition → translate → emit pipeline
//! on a real LUBM(1, 0) dataset using a minimal placeholder SPARQL query
//! (the 14 LUBM queries are committed in Phase 4 and exercised by
//! `lubm_cardinalities` in Phase 5). Verifies the on-disk output layout,
//! `meta.json` shape, and benchmark.yml validity.
//!
//! Gated on `which java` succeeding.

use {
    kermit_rdf::lubm::{
        driver::{LubmDriverInputs, DEFAULT_ONTOLOGY_IRI},
        pipeline::{run_lubm_pipeline, LubmPipelineInputs, LubmQuerySpec},
    },
    std::{path::PathBuf, process::Command},
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

fn vendored_jar() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("vendor")
        .join("lubm-uba")
        .join("lubm-uba.jar")
}

#[test]
#[cfg_attr(miri, ignore = "miri does not support spawning java")]
fn full_pipeline_lubm_one_university_with_placeholder_query() {
    if !java_available() {
        eprintln!("skipping: `java` not on PATH");
        return;
    }
    let jar = vendored_jar();
    if !jar.exists() {
        eprintln!("skipping: vendored jar missing at {jar:?}");
        return;
    }

    let out = tempfile::tempdir().expect("tempdir");
    let queries = vec![LubmQuerySpec {
        name: "smoke".to_string(),
        sparql: r#"
            PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
            PREFIX ub: <http://www.lehigh.edu/~zhp2/2004/0401/univ-bench.owl#>
            SELECT ?x ?y WHERE {
                ?x ub:subOrganizationOf ?y .
            }
        "#
        .to_string(),
        expected_cardinality: None,
    }];

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
        bench_name: "lubm-1-pipeline-test",
        tag: "pipeline-test",
        queries: &queries,
        spec_hash: None,
    };

    let meta = run_lubm_pipeline(&inputs).expect("pipeline must succeed");

    // Output files exist.
    assert!(out.path().join("meta.json").exists(), "meta.json missing");
    assert!(
        out.path().join("benchmark.yml").exists(),
        "benchmark.yml missing"
    );
    assert!(
        out.path().join("dict.parquet").exists(),
        "dict.parquet missing"
    );
    assert!(
        out.path().join("raw/data.nt").exists(),
        "raw/data.nt missing"
    );
    assert!(
        out.path().join("raw/data.entailed.nt").exists(),
        "raw/data.entailed.nt missing"
    );
    assert!(
        out.path().join("raw/queries/smoke.sparql").exists(),
        "raw query missing"
    );

    // At least one predicate parquet — subOrganizationOf is in the data.
    let pred_files: Vec<_> = std::fs::read_dir(out.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|s| s.ends_with(".parquet") && !matches!(s, "dict.parquet"))
                .unwrap_or(false)
        })
        .collect();
    assert!(!pred_files.is_empty(), "no <pred>.parquet files written");

    // meta.json fields populated.
    assert_eq!(meta.kind, "lubm-onthefly");
    assert_eq!(meta.scale, 1);
    assert_eq!(meta.seed, 0);
    assert_eq!(meta.tag, "pipeline-test");
    assert_eq!(meta.query_count, 1);
    assert!(
        meta.relation_count > 5,
        "expected > 5 predicates after entailment, got {}",
        meta.relation_count
    );
    assert!(
        meta.triple_count_post_entailment > meta.triple_count_pre_entailment,
        "entailment must add triples"
    );
    assert_eq!(
        meta.lubm_jar_sha256.len(),
        64,
        "SHA-256 must be 64 hex chars"
    );

    // benchmark.yml is valid YAML and references the predicate used.
    let yaml = std::fs::read_to_string(out.path().join("benchmark.yml")).unwrap();
    assert!(yaml.contains("name: lubm-1-pipeline-test"));
    assert!(
        yaml.contains("suborganizationof"),
        "predicate not in YAML: {yaml}"
    );
    assert!(yaml.contains("Q_smoke"), "query not in YAML: {yaml}");
}
