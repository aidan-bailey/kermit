//! Every Datalog rule the SPARQL translator emits must parse and pass the
//! join's validation (issues #75, #78).
//!
//! `kermit-rdf` emits rules as text and must not depend on `kermit-parser`
//! (see CLAUDE.md), so a rule it cannot express only failed at `bench run`
//! time — `?_x` became `_X`, which the parser rejects. This crate depends on
//! both, so it closes the loop: translate, parse, validate.

use {
    kermit::db::validate_query,
    kermit_algos::{ColumnOrderPolicy, JoinQuery},
    kermit_ds::RelationHeader,
    kermit_parser::Term,
    kermit_rdf::{
        dict::Dictionary,
        lubm::queries::lubm_query_specs,
        sparql::translator::{bgp_predicate_iris, translate_query},
    },
    std::collections::{HashMap, HashSet},
};

/// A predicate map naming each IRI `p<i>`, as `partition` would after
/// sanitising, plus a binary header per relation for validation.
fn catalogue(iris: &[String]) -> (HashMap<String, String>, Vec<RelationHeader>) {
    let map: HashMap<String, String> = iris
        .iter()
        .enumerate()
        .map(|(i, iri)| (iri.clone(), format!("p{i}")))
        .collect();
    let headers = map
        .values()
        .map(|name| RelationHeader::new_positional(name.clone(), 2))
        .collect();
    (map, headers)
}

/// Translates `sparql`, parses the rule, validates it against binary
/// relations for its predicates, and returns the parsed rule.
fn round_trip(sparql: &str) -> JoinQuery {
    let iris = bgp_predicate_iris(sparql).unwrap();
    let (map, headers) = catalogue(&iris);
    let mut dict = Dictionary::new();
    let rule = translate_query(sparql, &mut dict, &map, "Q").unwrap();
    let query: JoinQuery = rule
        .parse()
        .unwrap_or_else(|e| panic!("emitted rule {rule:?} does not parse: {e}"));
    validate_query(&query, headers.as_slice(), ColumnOrderPolicy::Stored)
        .unwrap_or_else(|e| panic!("emitted rule {rule:?} fails validation: {e}"));
    query
}

fn head_variables(query: &JoinQuery) -> Vec<&str> {
    query
        .head
        .terms
        .iter()
        .map(|t| match t {
            | Term::Var(v) => v.as_str(),
            | other => panic!("non-variable head term {other:?}"),
        })
        .collect()
}

#[test]
fn every_lubm_query_round_trips() {
    for spec in lubm_query_specs(true) {
        round_trip(&spec.sparql);
    }
}

/// Names that uppercasing merged (`?x`/`?X`, `?straße`/`?strasse`) or made
/// unparseable (`?_x`, `?1a`) all become distinct, parseable variables.
#[test]
fn awkward_variable_names_round_trip_as_distinct_variables() {
    let query = round_trip(
        "SELECT ?x ?X ?_x ?1a ?straße ?strasse WHERE { \
         ?x <http://e/p> ?X . ?_x <http://e/p> ?1a . ?straße <http://e/q> ?strasse . }",
    );
    let head = head_variables(&query);
    assert_eq!(head.len(), 6);
    assert_eq!(
        head.iter().collect::<HashSet<_>>().len(),
        6,
        "variables merged: {head:?}"
    );
}

/// `?x <p> ?X` joins two different variables: the rule must not turn into
/// the self-loop `p(V, V)`.
#[test]
fn case_distinct_variables_are_not_joined() {
    let query = round_trip("SELECT ?x WHERE { ?x <http://e/p> ?X . }");
    let atom = &query.body[0];
    assert_ne!(atom.terms[0], atom.terms[1], "{query}");
}
