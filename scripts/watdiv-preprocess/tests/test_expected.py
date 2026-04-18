"""Tests for the .desc → expected.json harvester.

The generic API takes an explicit iterable of SPARQL files so any
workload can reuse it; `watdiv_collect_expected` is a WatDiv-specific
compatibility shim that runs the ``watdiv-stress-*`` glob and delegates.
"""
from __future__ import annotations

import json
from pathlib import Path

import pytest

from watdiv_preprocess.expected import (
    collect_expected,
    parse_desc,
    watdiv_collect_expected,
    write_expected,
    write_watdiv_expected,
)


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
    sparql = stress / "test.1.sparql"
    sparql.write_text("\n", encoding="utf-8")
    (stress / "test.1.desc").write_text("10\n20\n30\n", encoding="utf-8")

    expected = collect_expected([sparql])
    assert expected == {
        "watdiv-stress-100-test-1::q0000": 10,
        "watdiv-stress-100-test-1::q0001": 20,
        "watdiv-stress-100-test-1::q0002": 30,
    }


def test_collect_expected_skips_orphan_sparql(tmp_path: Path):
    stress = tmp_path / "watdiv-stress-100"
    stress.mkdir()
    lonely = stress / "lonely.sparql"
    lonely.write_text("SELECT * WHERE { ?x ?y ?z }\n", encoding="utf-8")
    assert collect_expected([lonely]) == {}


def test_collect_expected_accepts_arbitrary_parent_dir(tmp_path: Path):
    # Any parent name works — yaml_name derives from parent.name + stem,
    # mirroring `yaml_emitter.emit_yaml`.
    workload = tmp_path / "my-workload"
    workload.mkdir()
    sparql = workload / "q.sparql"
    sparql.write_text("\n", encoding="utf-8")
    (workload / "q.desc").write_text("1\n2\n", encoding="utf-8")

    assert collect_expected([sparql]) == {
        "my-workload-q::q0000": 1,
        "my-workload-q::q0001": 2,
    }


def test_write_expected_round_trip(tmp_path: Path):
    stress = tmp_path / "watdiv-stress-1000"
    stress.mkdir()
    sparql = stress / "q.sparql"
    sparql.write_text("\n", encoding="utf-8")
    (stress / "q.desc").write_text("5\n7\n", encoding="utf-8")

    out = tmp_path / "expected.json"
    count = write_expected([sparql], out)

    assert count == 2
    data = json.loads(out.read_text(encoding="utf-8"))
    assert data == {
        "watdiv-stress-1000-q::q0000": 5,
        "watdiv-stress-1000-q::q0001": 7,
    }


def test_watdiv_collect_expected_uses_stress_glob(tmp_path: Path):
    stress_100 = tmp_path / "watdiv-stress-100"
    stress_100.mkdir()
    (stress_100 / "a.sparql").write_text("\n", encoding="utf-8")
    (stress_100 / "a.desc").write_text("11\n", encoding="utf-8")

    stress_1000 = tmp_path / "watdiv-stress-1000"
    stress_1000.mkdir()
    (stress_1000 / "b.sparql").write_text("\n", encoding="utf-8")
    (stress_1000 / "b.desc").write_text("22\n", encoding="utf-8")

    # Sibling dirs that don't match the glob must be ignored — the shim
    # is the only place the watdiv-stress-* convention is hard-coded.
    decoy = tmp_path / "not-watdiv"
    decoy.mkdir()
    (decoy / "c.sparql").write_text("\n", encoding="utf-8")
    (decoy / "c.desc").write_text("99\n", encoding="utf-8")

    assert watdiv_collect_expected(tmp_path) == {
        "watdiv-stress-100-a::q0000": 11,
        "watdiv-stress-1000-b::q0000": 22,
    }


def test_write_watdiv_expected_delegates_to_shim(tmp_path: Path):
    stress = tmp_path / "watdiv-stress-100"
    stress.mkdir()
    (stress / "x.sparql").write_text("\n", encoding="utf-8")
    (stress / "x.desc").write_text("3\n", encoding="utf-8")

    out = tmp_path / "expected.json"
    count = write_watdiv_expected(tmp_path, out)

    assert count == 1
    assert json.loads(out.read_text(encoding="utf-8")) == {
        "watdiv-stress-100-x::q0000": 3,
    }
