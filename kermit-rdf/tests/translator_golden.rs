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
    assert_eq!(out, "Q0(V_x) :- p(V_x, c1).");
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
    assert_eq!(out, "Q1(V_x, V_y, V_z) :- p(V_x, V_y), q(V_y, V_z).");
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
        "Q_test1_q0000(V_v0, V_v2) :- homepage(V_v0, c1), title(V_v0, V_v2)."
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
    assert!(out.contains("title(V_x, c2)"), "got: {out}");
    assert!(out.contains("title_1(V_x, c3)"), "got: {out}");
}

fn p_map() -> HashMap<String, String> {
    HashMap::from([("http://example/p".to_string(), "p".to_string())])
}

/// `?x` and `?X` are different SPARQL variables. Uppercasing merged them,
/// so this rule returned only self-loops instead of every subject (#75).
#[test]
fn case_distinct_variables_stay_distinct() {
    let mut dict = build_dict(&["http://example/p"]);
    let out = translate_query(
        "SELECT ?x WHERE { ?x <http://example/p> ?X . }",
        &mut dict,
        &p_map(),
        "Q",
    )
    .unwrap();
    assert_eq!(out, "Q(V_x) :- p(V_x, V_X).");
}

/// With `?x` and `?X` distinct, projecting `?x` from a body that binds
/// only `?X` is unbound — uppercasing let it through.
#[test]
fn a_projected_variable_differing_only_by_case_is_unbound() {
    let mut dict = build_dict(&["http://example/p"]);
    let err = translate_query(
        "SELECT ?x WHERE { ?X <http://example/p> ?y . }",
        &mut dict,
        &p_map(),
        "Q",
    )
    .unwrap_err();
    assert!(err.to_string().contains("not bound"), "{err}");
}

/// Underscore-leading, digit-leading and non-ASCII names, which
/// uppercasing turned into tokens the Datalog parser rejects or merges.
#[test]
fn awkward_names_map_to_distinct_datalog_variables() {
    let mut dict = build_dict(&["http://example/p"]);
    let out = translate_query(
        "SELECT ?_x ?1a ?straße ?strasse WHERE { ?_x <http://example/p> ?1a . \
         ?straße <http://example/p> ?strasse . }",
        &mut dict,
        &p_map(),
        "Q",
    )
    .unwrap();
    assert_eq!(
        out,
        "Q(V___x, V_1a, V_stra_xdf_e, V_strasse) :- p(V___x, V_1a), p(V_stra_xdf_e, V_strasse)."
    );
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
