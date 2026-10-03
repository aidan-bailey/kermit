//! Issue #77: the LUBM pipeline keeps erroring on a query predicate the
//! data lacks — for the fixed Univ-Bench workload a missing predicate means
//! broken input — but the error names every query and every absent
//! predicate at once, before any query is translated.
//!
//! Runs the post-driver stages over a hand-written ABox, so no jar runs.

use {
    kermit_rdf::{
        error::RdfError,
        lubm::{
            driver::{LubmDriverInputs, LubmRawArtifacts, DEFAULT_ONTOLOGY_IRI},
            pipeline::{process_artifacts, LubmMeta, LubmPipelineInputs, LubmQuerySpec},
            sandbox::LubmStagingDir,
        },
    },
    std::fs,
};

const UB: &str = "http://www.lehigh.edu/~zhp2/2004/0401/univ-bench.owl#";

/// Two students taking one course: only `ub:takesCourse` occurs.
fn abox() -> String {
    format!(
        "<http://x/s1> <{UB}takesCourse> <http://x/c1> .\n\
         <http://x/s2> <{UB}takesCourse> <http://x/c1> .\n"
    )
}

fn spec(name: &str, sparql: String) -> LubmQuerySpec {
    LubmQuerySpec {
        name: name.to_string(),
        sparql,
        expected_cardinality: None,
    }
}

/// Runs the post-driver pipeline over [`abox`] with `specs`; returns the
/// result and the output directory.
fn generate(specs: &[LubmQuerySpec]) -> (Result<LubmMeta, RdfError>, tempfile::TempDir) {
    let stage = LubmStagingDir::create().unwrap();
    let data_nt = stage.ntriples_output_path();
    fs::write(&data_nt, abox()).unwrap();
    let jar_placeholder = stage.root().join("no-jar-was-run");
    fs::write(&jar_placeholder, b"").unwrap();
    let out = tempfile::tempdir().unwrap();
    let inputs = LubmPipelineInputs {
        driver: LubmDriverInputs {
            jar_path: &jar_placeholder,
            scale: 1,
            seed: 0,
            start_index: 0,
            threads: 1,
            ontology_iri: DEFAULT_ONTOLOGY_IRI,
        },
        out_dir: out.path(),
        bench_name: "lubm-missing",
        tag: "missing",
        queries: specs,
        spec_hash: None,
    };
    let raw = LubmRawArtifacts {
        data_nt,
        scale: 1,
        seed: 0,
        start_index: 0,
        ontology_iri: DEFAULT_ONTOLOGY_IRI.to_string(),
        stage,
    };
    (process_artifacts(&inputs, &raw), out)
}

#[test]
#[cfg_attr(
    miri,
    ignore = "miri cannot model `fs::copy` (copy_file_range) in the LUBM stage step"
)]
fn every_absent_predicate_of_every_query_is_reported_at_once() {
    let specs = [
        spec(
            "present",
            format!("SELECT ?s WHERE {{ ?s <{UB}takesCourse> ?c . }}"),
        ),
        spec(
            "one_missing",
            format!("SELECT ?s WHERE {{ ?s <{UB}takesCourse> ?c . ?s <{UB}advisor> ?a . }}"),
        ),
        spec(
            "two_missing",
            format!("SELECT ?p WHERE {{ ?p <{UB}publicationAuthor> ?a . ?a <{UB}memberOf> ?d . }}"),
        ),
    ];
    let (result, out) = generate(&specs);
    let err = result.unwrap_err();
    match &err {
        | RdfError::MissingQueryPredicates {
            by_query,
        } => assert_eq!(by_query, &vec![
            ("one_missing".to_string(), vec![format!("{UB}advisor")]),
            ("two_missing".to_string(), vec![
                format!("{UB}publicationAuthor"),
                format!("{UB}memberOf"),
            ]),
        ]),
        | other => panic!("expected MissingQueryPredicates, got {other:?}"),
    }
    let message = err.to_string();
    for fragment in [
        "one_missing",
        "two_missing",
        "advisor",
        "publicationAuthor",
        "memberOf",
    ] {
        assert!(message.contains(fragment), "{fragment} not in: {message}");
    }
    assert!(
        !message.contains("present"),
        "a query whose predicates all occur is not listed: {message}"
    );
    assert!(
        !out.path().join("benchmark.yml").exists(),
        "no benchmark may be emitted for an incomplete workload"
    );
}

/// A query the translator rejects is named in the error, whether the
/// up-front predicate scan rejects it (a FILTER is not a BGP) or only the
/// translation does (a predicate variable carries no IRI to check).
#[test]
#[cfg_attr(
    miri,
    ignore = "miri cannot model `fs::copy` (copy_file_range) in the LUBM stage step"
)]
fn an_untranslatable_query_is_named() {
    for (name, sparql) in [
        (
            "filtered",
            format!("SELECT ?s WHERE {{ ?s <{UB}takesCourse> ?c . FILTER(?s != ?c) }}"),
        ),
        (
            "variable_predicate",
            "SELECT ?s WHERE { ?s ?p ?o . }".to_string(),
        ),
    ] {
        let (result, _out) = generate(&[spec(name, sparql)]);
        let err = result.unwrap_err();
        assert!(
            matches!(&err, RdfError::QueryTranslation { query, .. } if query == name),
            "{name}: {err:?}"
        );
        assert!(err.to_string().contains(&format!("{name:?}")), "{err}");
    }
}
