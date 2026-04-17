"""Tests for the YAML emitter."""
from __future__ import annotations

from pathlib import Path

import yaml

from watdiv_preprocess.yaml_emitter import collect_predicates, emit_yaml


def test_collect_predicates_ignores_head():
    datalog = "Q_test_q0000(V0, V1) :- foo(V0, V1), bar(V1, V2)."
    assert collect_predicates(datalog) == {"foo", "bar"}


def test_emit_yaml_structure(tmp_path: Path):
    stress_dir = tmp_path / "watdiv-stress-100"
    stress_dir.mkdir()
    sparql = stress_dir / "test.1.sparql"
    sparql.write_text(
        "SELECT * WHERE { ?v0 <http://example/p> <http://example/c> . }\n"
        "SELECT * WHERE { ?v0 <http://example/p> ?v1 . ?v1 <http://example/q> ?v2 . }\n",
        encoding="utf-8",
    )

    uri_to_id = {
        "<http://example/p>": 1,
        "<http://example/q>": 2,
        "<http://example/c>": 99,
    }
    filename_map = {"p": "p", "q": "q"}

    out = emit_yaml(sparql, tmp_path, uri_to_id, "https://host/dl", filename_map)
    assert out.exists()

    doc = yaml.safe_load(out.read_text(encoding="utf-8"))
    assert doc["name"] == "watdiv-stress-100-test-1"
    assert "2 queries" in doc["description"]

    rel_names = sorted(r["name"] for r in doc["relations"])
    assert rel_names == ["p", "q"]
    assert doc["relations"][0]["url"].startswith("https://host/dl/")

    query_names = [q["name"] for q in doc["queries"]]
    assert query_names == ["q0000", "q0001"]
    assert doc["queries"][0]["query"].startswith("Q_test_1_q0000(")
    assert "c99" in doc["queries"][0]["query"]
