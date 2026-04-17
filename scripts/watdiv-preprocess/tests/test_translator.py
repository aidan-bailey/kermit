"""Tests for the SPARQL → Datalog translator."""
from __future__ import annotations

import pytest

from watdiv_preprocess.sparql_translator import TranslationError, translate_query


def test_simple_bgp_one_triple():
    uri_to_id = {
        "<http://example/p>": 10,
        "<http://example/c>": 42,
    }
    sparql = "SELECT ?x WHERE { ?x <http://example/p> <http://example/c> . }"
    out = translate_query(sparql, uri_to_id, "Q0")
    assert out == "Q0(X) :- p(X, c42)."


def test_select_star_projects_all_bound_vars_in_source_order():
    uri_to_id = {
        "<http://example/p>": 10,
        "<http://example/q>": 20,
    }
    sparql = (
        "SELECT * WHERE { ?x <http://example/p> ?y . ?y <http://example/q> ?z . }"
    )
    out = translate_query(sparql, uri_to_id, "Q1")
    assert out == "Q1(X, Y, Z) :- p(X, Y), q(Y, Z)."


def test_watdiv_style_select_star_with_constant_object():
    uri_to_id = {
        "<http://xmlns.com/foaf/homepage>": 1,
        "<http://db.uwaterloo.ca/~galuc/wsdbm/Website2948>": 2948,
        "<http://ogp.me/ns#title>": 3,
    }
    sparql = (
        "SELECT * WHERE {  "
        "?v0 <http://xmlns.com/foaf/homepage> "
        "<http://db.uwaterloo.ca/~galuc/wsdbm/Website2948> .  "
        "?v0 <http://ogp.me/ns#title> ?v2 .  }"
    )
    out = translate_query(sparql, uri_to_id, "Q_test1_q0000")
    assert out == "Q_test1_q0000(V0, V2) :- homepage(V0, c2948), title(V0, V2)."


def test_filter_rejected():
    uri_to_id = {"<http://example/p>": 10}
    sparql = (
        "SELECT ?x WHERE { ?x <http://example/p> ?y . FILTER(?y = <http://example/y>) }"
    )
    with pytest.raises(TranslationError):
        translate_query(sparql, uri_to_id, "Q2")


def test_optional_rejected():
    uri_to_id = {"<http://example/p>": 10, "<http://example/q>": 20}
    sparql = (
        "SELECT ?x WHERE { ?x <http://example/p> ?y . "
        "OPTIONAL { ?y <http://example/q> ?z } }"
    )
    with pytest.raises(TranslationError):
        translate_query(sparql, uri_to_id, "Q3")


def test_unknown_uri_errors():
    uri_to_id = {"<http://example/p>": 10}
    sparql = "SELECT ?x WHERE { ?x <http://example/p> <http://example/unseen> . }"
    with pytest.raises(TranslationError, match="not in dictionary"):
        translate_query(sparql, uri_to_id, "Q4")


def test_literal_object_errors():
    uri_to_id = {"<http://example/p>": 10}
    sparql = 'SELECT ?x WHERE { ?x <http://example/p> "literal" . }'
    with pytest.raises(TranslationError, match="Literal"):
        translate_query(sparql, uri_to_id, "Q5")
