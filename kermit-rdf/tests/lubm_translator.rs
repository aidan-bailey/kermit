//! Golden test: each of the 14 LUBM queries must translate to a valid
//! Datalog rule against the predicate map produced by partitioning the
//! entailed LUBM(1, 0) ABox.
//!
//! This catches:
//!  - Predicate IRIs in queries that never appear in entailed data (entailment
//!    rules incomplete vs. query expectations)
//!  - SPARQL syntax errors in committed queries
//!  - BGP-only constraint violations (FILTER, OPTIONAL, UNION)
//!
//! Gated on `which java` (the entailed predicate map is sourced from a
//! real LUBM(1, 0) generation).

use {
    kermit_rdf::{
        lubm::{
            driver::{drive, LubmDriverInputs, DEFAULT_ONTOLOGY_IRI},
            entailment::entail,
            queries::lubm_query_specs,
        },
        partition,
        sparql::translator::translate_query,
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
fn all_fourteen_queries_translate_against_entailed_lubm_one_university() {
    if !java_available() {
        eprintln!("skipping: `java` not on PATH");
        return;
    }
    let jar = vendored_jar();
    if !jar.exists() {
        eprintln!("skipping: vendored jar missing at {jar:?}");
        return;
    }

    // Set up: generate LUBM(1, 0), entail, partition. Reuse for all 14
    // translations.
    let raw = drive(&LubmDriverInputs {
        jar_path: &jar,
        scale: 1,
        seed: 0,
        start_index: 0,
        threads: 1,
        ontology_iri: DEFAULT_ONTOLOGY_IRI,
    })
    .expect("drive");
    let work = tempfile::tempdir().unwrap();
    let entailed = work.path().join("data.entailed.nt");
    entail(&raw.data_nt, &entailed).expect("entail");
    let part = partition::partition(&entailed).expect("partition");
    let mut dict = part.dict;

    let specs = lubm_query_specs(false);
    let mut failures: Vec<String> = Vec::new();
    for spec in &specs {
        let head = format!("Q_{}", spec.name);
        match translate_query(&spec.sparql, &mut dict, &part.predicate_map, &head) {
            | Ok(dl) => {
                // Sanity: the produced rule must contain the head and a `:-`
                // separator.
                if !dl.contains(":-") || !dl.contains(&head) {
                    failures.push(format!(
                        "{}: translated rule looks malformed: {dl}",
                        spec.name
                    ));
                }
            },
            | Err(e) => {
                failures.push(format!("{}: {e}", spec.name));
            },
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} of {} LUBM queries failed to translate:\n  {}",
            failures.len(),
            specs.len(),
            failures.join("\n  ")
        );
    }
}
