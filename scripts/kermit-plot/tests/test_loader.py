"""Loader parses v2 reports, refuses unknown majors, surfaces axes verbatim."""
from __future__ import annotations

import json
from pathlib import Path

import pytest

from kermit_plot.loader import SchemaError, load_reports


def test_loads_array_with_one_report(tmp_path: Path) -> None:
    p = tmp_path / "r.json"
    p.write_text(
        json.dumps(
            [
                {
                    "schema_version": 2,
                    "kind": "ds",
                    "metadata": [{"label": "tuples", "value": "4"}],
                    "axes": {"tuples": 4, "data_structure": "TreeTrie"},
                    "criterion_groups": [
                        {"group": "ds", "function": "TreeTrie/space", "metric": "space"}
                    ],
                }
            ]
        )
    )
    [r] = load_reports([p])
    assert r.kind == "ds"
    assert r.axis("tuples") == 4
    assert r.axis("data_structure") == "TreeTrie"
    assert r.has_metric("space")
    assert not r.has_metric("time")
    assert r.criterion_groups[0].group == "ds"
    assert r.source_path == p


def test_flattens_array_across_files(tmp_path: Path) -> None:
    a = tmp_path / "a.json"
    b = tmp_path / "b.json"
    a.write_text(
        json.dumps(
            [
                {
                    "schema_version": 2,
                    "kind": "run",
                    "metadata": [],
                    "axes": {"query": "triangle"},
                    "criterion_groups": [],
                }
            ]
        )
    )
    b.write_text(
        json.dumps(
            [
                {
                    "schema_version": 2,
                    "kind": "run",
                    "metadata": [],
                    "axes": {"query": "chain"},
                    "criterion_groups": [],
                },
                {
                    "schema_version": 2,
                    "kind": "run",
                    "metadata": [],
                    "axes": {"query": "star"},
                    "criterion_groups": [],
                },
            ]
        )
    )
    reports = load_reports([a, b])
    assert sorted(r.axis("query") for r in reports) == ["chain", "star", "triangle"]


def test_rejects_unknown_major_version(tmp_path: Path) -> None:
    p = tmp_path / "r.json"
    p.write_text(
        json.dumps(
            [
                {
                    "schema_version": 9999,
                    "kind": "ds",
                    "metadata": [],
                    "axes": {},
                    "criterion_groups": [],
                }
            ]
        )
    )
    with pytest.raises(SchemaError, match="schema_version 9999"):
        load_reports([p])


def test_rejects_non_array_top_level(tmp_path: Path) -> None:
    p = tmp_path / "r.json"
    p.write_text(json.dumps({"schema_version": 2, "kind": "ds"}))
    with pytest.raises(SchemaError, match="JSON array"):
        load_reports([p])


def test_axes_preserve_value_types(tmp_path: Path) -> None:
    p = tmp_path / "r.json"
    p.write_text(
        json.dumps(
            [
                {
                    "schema_version": 2,
                    "kind": "ds",
                    "metadata": [],
                    "axes": {"tuples": 4, "data_structure": "TreeTrie", "is_synthetic": True},
                    "criterion_groups": [],
                }
            ]
        )
    )
    [r] = load_reports([p])
    assert r.axis("tuples") == 4
    assert isinstance(r.axis("tuples"), int)
    assert r.axis("is_synthetic") is True
