//! Smoke-tests the entailment module against a real LUBM(1, 0) ABox
//! produced by the vendored jar. Verifies that the closure expands the
//! triple count (it must, given the rule set) and that all original
//! triples survive.
//!
//! Gated on `which java` succeeding so CI runners without a JDK skip.

use {
    kermit_rdf::{
        lubm::{
            driver::{drive, LubmDriverInputs, DEFAULT_ONTOLOGY_IRI},
            entailment::entail,
        },
        ntriples,
        value::RdfValue,
    },
    std::{collections::HashSet, path::PathBuf, process::Command},
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
fn entail_on_real_lubm_one_university() {
    if !java_available() {
        eprintln!("skipping: `java` not on PATH");
        return;
    }
    let jar = vendored_jar();
    if !jar.exists() {
        eprintln!("skipping: vendored jar missing at {jar:?}");
        return;
    }

    let inputs = LubmDriverInputs {
        jar_path: &jar,
        scale: 1,
        seed: 0,
        start_index: 0,
        threads: 1,
        ontology_iri: DEFAULT_ONTOLOGY_IRI,
    };
    let raw = drive(&inputs).expect("drive must succeed");

    let out_dir = tempfile::tempdir().expect("tempdir");
    let entailed = out_dir.path().join("data.entailed.nt");
    let stats = entail(&raw.data_nt, &entailed).expect("entailment must succeed");

    assert!(
        stats.input_triples > 95_000,
        "expected ≥95k input triples for LUBM(1,0), got {}",
        stats.input_triples
    );
    assert!(
        stats.derived_triples > 0,
        "entailment derived no new triples — rule set must be broken"
    );
    assert!(
        stats.output_triples >= stats.input_triples,
        "output must be a superset of input"
    );
    assert!(
        stats.iterations > 0 && stats.iterations < 64,
        "iterations should be in (0, 64), got {}",
        stats.iterations
    );

    // Every input subject-predicate-object must survive in the output
    // (we only ever add, never delete).
    let input_set: HashSet<(String, String, RdfValue)> = ntriples::iter_path(&raw.data_nt)
        .unwrap()
        .map(|t| t.unwrap())
        .collect();
    let output_set: HashSet<(String, String, RdfValue)> = ntriples::iter_path(&entailed)
        .unwrap()
        .map(|t| t.unwrap())
        .collect();
    let missing: Vec<_> = input_set.difference(&output_set).take(3).cloned().collect();
    assert!(
        missing.is_empty(),
        "input triples not preserved: e.g. {missing:?}"
    );

    eprintln!(
        "[smoke] LUBM(1, 0) entailment: input={}, output={}, derived={}, iters={}",
        stats.input_triples, stats.output_triples, stats.derived_triples, stats.iterations
    );
}
