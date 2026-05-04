//! Port of `scripts/watdiv-preprocess/tests/test_translator.py`.

use {
    kermit_rdf::{
        dict::Dictionary, error::RdfError, sparql::translator::translate_query, value::RdfValue,
    },
    std::collections::HashMap,
};

fn build_dict(uris: &[&str]) -> Dictionary {
    let mut d = Dictionary::new();
    for u in uris {
        d.intern(RdfValue::Iri(u.to_string()));
    }
    d
}

#[test]
fn simple_bgp_one_triple() {
    let mut dict = build_dict(&["http://example/p", "http://example/c"]);
    let mut pm = HashMap::new();
    pm.insert("http://example/p".to_string(), "p".to_string());
    let out = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> <http://example/c> . }",
        &mut dict,
        &pm,
        "Q0",
    )
    .unwrap();
    assert_eq!(out, "Q0(X) :- p(X, c1).");
}

#[test]
fn select_star_projects_all_bound_in_source_order() {
    let mut dict = build_dict(&["http://example/p", "http://example/q"]);
    let mut pm = HashMap::new();
    pm.insert("http://example/p".to_string(), "p".to_string());
    pm.insert("http://example/q".to_string(), "q".to_string());
    let out = translate_query(
        "SELECT * WHERE { ?x <http://example/p> ?y . ?y <http://example/q> ?z . }",
        &mut dict,
        &pm,
        "Q1",
    )
    .unwrap();
    assert_eq!(out, "Q1(X, Y, Z) :- p(X, Y), q(Y, Z).");
}

#[test]
fn watdiv_style_select_star_with_constant_object() {
    let mut dict = build_dict(&[
        "http://xmlns.com/foaf/homepage",
        "http://db.uwaterloo.ca/~galuc/wsdbm/Website2948",
        "http://ogp.me/ns#title",
    ]);
    let mut pm = HashMap::new();
    pm.insert(
        "http://xmlns.com/foaf/homepage".to_string(),
        "homepage".to_string(),
    );
    pm.insert("http://ogp.me/ns#title".to_string(), "title".to_string());
    let out = translate_query(
        "SELECT * WHERE { \
         ?v0 <http://xmlns.com/foaf/homepage> <http://db.uwaterloo.ca/~galuc/wsdbm/Website2948> . \
         ?v0 <http://ogp.me/ns#title> ?v2 . }",
        &mut dict,
        &pm,
        "Q_test1_q0000",
    )
    .unwrap();
    assert_eq!(
        out,
        "Q_test1_q0000(V0, V2) :- homepage(V0, c1), title(V0, V2)."
    );
}

#[test]
fn predicate_map_disambiguates_sanitize_collisions() {
    let mut dict = build_dict(&[
        "http://ogp.me/ns#title",
        "http://purl.org/stuff/rev#title",
        "http://example/o1",
        "http://example/o2",
    ]);
    let mut pm = HashMap::new();
    pm.insert("http://ogp.me/ns#title".to_string(), "title".to_string());
    pm.insert(
        "http://purl.org/stuff/rev#title".to_string(),
        "title_1".to_string(),
    );
    let sparql = "SELECT * WHERE { \
         ?x <http://ogp.me/ns#title> <http://example/o1> . \
         ?x <http://purl.org/stuff/rev#title> <http://example/o2> . \
         }";
    let out = translate_query(sparql, &mut dict, &pm, "Q_collision").unwrap();
    assert!(out.contains("title(X, c2)"), "got: {out}");
    assert!(out.contains("title_1(X, c3)"), "got: {out}");
}

#[test]
fn missing_predicate_in_map_errors() {
    let mut dict = build_dict(&["http://example/p"]);
    let pm = HashMap::new();
    let err = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> ?y . }",
        &mut dict,
        &pm,
        "Q",
    )
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("partition map"), "msg: {msg}");
}

#[test]
fn filter_rejected() {
    let mut dict = build_dict(&["http://example/p"]);
    let mut pm = HashMap::new();
    pm.insert("http://example/p".to_string(), "p".to_string());
    let err = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> ?y . FILTER(?y = <http://example/y>) }",
        &mut dict,
        &pm,
        "Q",
    )
    .unwrap_err();
    assert!(matches!(err, RdfError::UnsupportedSparql(_)));
}

#[test]
fn optional_rejected() {
    let mut dict = build_dict(&["http://example/p", "http://example/q"]);
    let mut pm = HashMap::new();
    pm.insert("http://example/p".to_string(), "p".to_string());
    pm.insert("http://example/q".to_string(), "q".to_string());
    let err = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> ?y . OPTIONAL { ?y <http://example/q> ?z } }",
        &mut dict,
        &pm,
        "Q",
    )
    .unwrap_err();
    assert!(matches!(err, RdfError::UnsupportedSparql(_)));
}

#[test]
fn unknown_uri_added_to_dict() {
    let mut dict = build_dict(&["http://example/p"]);
    let mut pm = HashMap::new();
    pm.insert("http://example/p".to_string(), "p".to_string());
    let rule = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> <http://example/unseen> . }",
        &mut dict,
        &pm,
        "Q4",
    )
    .unwrap();
    let assigned = dict
        .lookup(&RdfValue::Iri("http://example/unseen".into()))
        .unwrap();
    assert_eq!(assigned, 1);
    assert!(rule.contains(&format!("c{assigned}")), "rule: {rule}");
}

#[test]
fn literal_object_errors() {
    let mut dict = build_dict(&["http://example/p"]);
    let mut pm = HashMap::new();
    pm.insert("http://example/p".to_string(), "p".to_string());
    let err = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> \"literal\" . }",
        &mut dict,
        &pm,
        "Q",
    )
    .unwrap_err();
    assert!(matches!(err, RdfError::UnsupportedSparql(_)));
}
