"""Tests for the SPARQL → Datalog translator."""
from __future__ import annotations

import pytest

from watdiv_preprocess.sparql_translator import TranslationError, translate_query


def test_simple_bgp_one_triple():
    uri_to_id = {
        "<http://example/p>": 10,
        "<http://example/c>": 42,
    }
    predicate_map = {"<http://example/p>": "p"}
    sparql = "SELECT ?x WHERE { ?x <http://example/p> <http://example/c> . }"
    out = translate_query(sparql, uri_to_id, predicate_map, "Q0")
    assert out == "Q0(V_x) :- p(V_x, c42)."


def test_select_star_projects_all_bound_vars_in_source_order():
    uri_to_id = {
        "<http://example/p>": 10,
        "<http://example/q>": 20,
    }
    predicate_map = {"<http://example/p>": "p", "<http://example/q>": "q"}
    sparql = (
        "SELECT * WHERE { ?x <http://example/p> ?y . ?y <http://example/q> ?z . }"
    )
    out = translate_query(sparql, uri_to_id, predicate_map, "Q1")
    assert out == "Q1(V_x, V_y, V_z) :- p(V_x, V_y), q(V_y, V_z)."


def test_watdiv_style_select_star_with_constant_object():
    uri_to_id = {
        "<http://xmlns.com/foaf/homepage>": 1,
        "<http://db.uwaterloo.ca/~galuc/wsdbm/Website2948>": 2948,
        "<http://ogp.me/ns#title>": 3,
    }
    predicate_map = {
        "<http://xmlns.com/foaf/homepage>": "homepage",
        "<http://ogp.me/ns#title>": "title",
    }
    sparql = (
        "SELECT * WHERE {  "
        "?v0 <http://xmlns.com/foaf/homepage> "
        "<http://db.uwaterloo.ca/~galuc/wsdbm/Website2948> .  "
        "?v0 <http://ogp.me/ns#title> ?v2 .  }"
    )
    out = translate_query(sparql, uri_to_id, predicate_map, "Q_test1_q0000")
    assert out == (
        "Q_test1_q0000(V_v0, V_v2) :- homepage(V_v0, c2948), title(V_v0, V_v2)."
    )


def test_predicate_map_disambiguates_sanitize_collisions():
    """Two URIs that sanitize to 'title' must resolve to distinct Datalog names."""
    uri_to_id = {
        "<http://ogp.me/ns#title>": 10,
        "<http://purl.org/stuff/rev#title>": 11,
        "<http://example/o1>": 100,
        "<http://example/o2>": 101,
    }
    # The partitioner resolved the second occurrence by appending _11.
    predicate_map = {
        "<http://ogp.me/ns#title>": "title",
        "<http://purl.org/stuff/rev#title>": "title_11",
    }
    sparql = (
        "SELECT * WHERE { "
        "?x <http://ogp.me/ns#title> <http://example/o1> . "
        "?x <http://purl.org/stuff/rev#title> <http://example/o2> . "
        "}"
    )
    out = translate_query(sparql, uri_to_id, predicate_map, "Q_collision")
    assert "title(V_x, c100)" in out
    assert "title_11(V_x, c101)" in out


def test_case_distinct_variables_stay_distinct():
    """``?x`` and ``?X`` are different variables (issue #75)."""
    uri_to_id = {"<http://example/p>": 10}
    predicate_map = {"<http://example/p>": "p"}
    sparql = "SELECT ?x WHERE { ?x <http://example/p> ?X . }"
    out = translate_query(sparql, uri_to_id, predicate_map, "Q")
    assert out == "Q(V_x) :- p(V_x, V_X)."


def test_variable_names_match_the_rust_translator():
    """Same mapping as ``kermit_rdf::sparql::bindings::var_name``."""
    from watdiv_preprocess.sparql_translator import _var_name

    assert _var_name("?x") == "V_x"
    assert _var_name("?X") == "V_X"
    assert _var_name("?_x") == "V___x"
    assert _var_name("?a_b") == "V_a__b"
    assert _var_name("?1a") == "V_1a"
    assert _var_name("?straße") == "V_stra_xdf_e"


def test_missing_predicate_in_map_errors():
    uri_to_id = {"<http://example/p>": 10}
    predicate_map: dict[str, str] = {}
    sparql = "SELECT ?x WHERE { ?x <http://example/p> ?y . }"
    with pytest.raises(TranslationError, match="not in partition map"):
        translate_query(sparql, uri_to_id, predicate_map, "Q_missing")


def test_filter_rejected():
    uri_to_id = {"<http://example/p>": 10}
    predicate_map = {"<http://example/p>": "p"}
    sparql = (
        "SELECT ?x WHERE { ?x <http://example/p> ?y . FILTER(?y = <http://example/y>) }"
    )
    with pytest.raises(TranslationError):
        translate_query(sparql, uri_to_id, predicate_map, "Q2")


def test_optional_rejected():
    uri_to_id = {"<http://example/p>": 10, "<http://example/q>": 20}
    predicate_map = {"<http://example/p>": "p", "<http://example/q>": "q"}
    sparql = (
        "SELECT ?x WHERE { ?x <http://example/p> ?y . "
        "OPTIONAL { ?y <http://example/q> ?z } }"
    )
    with pytest.raises(TranslationError):
        translate_query(sparql, uri_to_id, predicate_map, "Q3")


def test_unknown_uri_added_to_dict():
    uri_to_id = {"<http://example/p>": 10}
    predicate_map = {"<http://example/p>": "p"}
    sparql = "SELECT ?x WHERE { ?x <http://example/p> <http://example/unseen> . }"
    rule = translate_query(sparql, uri_to_id, predicate_map, "Q4")
    assigned = uri_to_id["<http://example/unseen>"]
    assert assigned == 1, "fresh ID should be the next index in dict order"
    assert f"c{assigned}" in rule, rule


def test_literal_object_errors():
    uri_to_id = {"<http://example/p>": 10}
    predicate_map = {"<http://example/p>": "p"}
    sparql = 'SELECT ?x WHERE { ?x <http://example/p> "literal" . }'
    with pytest.raises(TranslationError, match="Literal"):
        translate_query(sparql, uri_to_id, predicate_map, "Q5")
