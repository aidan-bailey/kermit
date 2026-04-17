"""Tests for the .desc → expected.json harvester."""
from __future__ import annotations

import json
from pathlib import Path

import pytest

from watdiv_preprocess.expected import collect_expected, parse_desc, write_expected


def test_parse_desc_skips_blank_lines(tmp_path: Path):
    desc = tmp_path / "a.desc"
    desc.write_text("1\n\n2\n3\n", encoding="utf-8")
    assert parse_desc(desc) == [1, 2, 3]


def test_parse_desc_raises_on_garbage(tmp_path: Path):
    desc = tmp_path / "bad.desc"
    desc.write_text("1\nnot-an-int\n", encoding="utf-8")
    with pytest.raises(ValueError, match="not an integer"):
        parse_desc(desc)


def test_collect_expected_keys_by_yaml_and_query(tmp_path: Path):
    stress = tmp_path / "watdiv-stress-100"
    stress.mkdir()
    (stress / "test.1.sparql").write_text("\n", encoding="utf-8")
    (stress / "test.1.desc").write_text("10\n20\n30\n", encoding="utf-8")

    expected = collect_expected(tmp_path)
    assert expected == {
        "watdiv-stress-100-test-1::q0000": 10,
        "watdiv-stress-100-test-1::q0001": 20,
        "watdiv-stress-100-test-1::q0002": 30,
    }


def test_collect_expected_skips_orphan_sparql(tmp_path: Path):
    stress = tmp_path / "watdiv-stress-100"
    stress.mkdir()
    (stress / "lonely.sparql").write_text("SELECT * WHERE { ?x ?y ?z }\n", encoding="utf-8")
    assert collect_expected(tmp_path) == {}


def test_write_expected_round_trip(tmp_path: Path):
    stress = tmp_path / "watdiv-stress-1000"
    stress.mkdir()
    (stress / "q.sparql").write_text("\n", encoding="utf-8")
    (stress / "q.desc").write_text("5\n7\n", encoding="utf-8")

    out = tmp_path / "expected.json"
    count = write_expected(tmp_path, out)

    assert count == 2
    data = json.loads(out.read_text(encoding="utf-8"))
    assert data == {
        "watdiv-stress-1000-q::q0000": 5,
        "watdiv-stress-1000-q::q0001": 7,
    }
