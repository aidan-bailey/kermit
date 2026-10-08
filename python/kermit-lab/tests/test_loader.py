"""Loader parses v2 reports, refuses unknown majors, surfaces axes verbatim."""
from __future__ import annotations

import json
from pathlib import Path

import pytest

from kermit_lab.loader import SchemaError, load_reports


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


def test_phase_of_recognises_end_to_end() -> None:
    from kermit_lab.loader import phase_of

    # `bench run` writes the bare token; `bench ds` prefixes the DS name.
    assert phase_of("end_to_end") == "end_to_end"
    assert phase_of("TreeTrie/end_to_end") == "end_to_end"
    # K is an axis, never a function-id segment — a trailing k-token must
    # NOT parse as a phase.
    assert phase_of("end_to_end/k4") is None


def test_phase_of_existing_phases_unchanged() -> None:
    from kermit_lab.loader import phase_of

    assert phase_of("insertion") == "insertion"
    assert phase_of("TreeTrie/iteration") == "iteration"
    assert phase_of("space/edge") is None


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


def _versioned_report(path: Path, version: int) -> Path:
    path.write_text(
        json.dumps(
            [
                {
                    "schema_version": version,
                    "kind": "run",
                    "metadata": [],
                    "axes": {},
                    "criterion_groups": [],
                }
            ]
        )
    )
    return path


def test_refuses_to_mix_reports_across_the_streamed_join_boundary(tmp_path: Path) -> None:
    old = _versioned_report(tmp_path / "old.json", 2)
    new = _versioned_report(tmp_path / "new.json", 3)
    with pytest.raises(SchemaError, match="refusing to mix schema_version 2"):
        load_reports([old, new])
    with pytest.raises(SchemaError, match="old.json"):
        load_reports([new, old])


def test_allow_mixed_schema_loads_both_sides(tmp_path: Path) -> None:
    old = _versioned_report(tmp_path / "old.json", 2)
    new = _versioned_report(tmp_path / "new.json", 3)
    reports = load_reports([old, new], allow_mixed_schema=True)
    assert [r.schema_version for r in reports] == [2, 3]


def test_single_sided_loads_are_unaffected(tmp_path: Path) -> None:
    for version in (2, 3, 4):
        same = [_versioned_report(tmp_path / f"v{version}_{i}.json", version) for i in range(2)]
        assert len(load_reports(same)) == 2


def test_refuses_to_mix_reports_across_the_flat_tuples_boundary(tmp_path: Path) -> None:
    old = _versioned_report(tmp_path / "v3.json", 3)
    new = _versioned_report(tmp_path / "v4.json", 4)
    with pytest.raises(SchemaError, match="refusing to mix schema_version 3"):
        load_reports([old, new])
    with pytest.raises(SchemaError, match="flat tuple buffer"):
        load_reports([new, old])
    reports = load_reports([old, new], allow_mixed_schema=True)
    assert [r.schema_version for r in reports] == [3, 4]


def test_a_load_straddling_both_boundaries_names_the_first(tmp_path: Path) -> None:
    v2 = _versioned_report(tmp_path / "v2.json", 2)
    v4 = _versioned_report(tmp_path / "v4.json", 4)
    with pytest.raises(SchemaError, match="streamed, counted join"):
        load_reports([v2, v4])


def test_a_load_straddling_both_boundaries_offers_only_what_both_keep(tmp_path: Path) -> None:
    v2 = _versioned_report(tmp_path / "v2.json", 2)
    v4 = _versioned_report(tmp_path / "v4.json", 4)
    with pytest.raises(SchemaError) as exc:
        load_reports([v2, v4])
    message = str(exc.value)
    assert "also straddles v4, where every structure builds from one flat tuple buffer" in message
    # HashTrie's space changed at v4 and iteration at v3: neither may be offered.
    assert "to compare TreeTrie space and ColumnTrie space only" in message
    assert "HashTrie space" not in message
    assert "iteration only" not in message


def test_each_boundary_names_what_stays_comparable_across_it(tmp_path: Path) -> None:
    v2, v3, v4 = (_versioned_report(tmp_path / f"v{v}.json", v) for v in (2, 3, 4))
    with pytest.raises(SchemaError, match="to compare TreeTrie space, ColumnTrie space and HashTrie space only"):
        load_reports([v2, v3])
    with pytest.raises(
        SchemaError,
        match="to compare TreeTrie space, ColumnTrie space, bench run iteration and bench join iteration only",
    ):
        load_reports([v3, v4])


def test_phase_of_recognises_copies() -> None:
    from kermit_lab.loader import TIME_PHASES, phase_of

    # `bench run` under `--column-orders any` times a query's reordered
    # copies as one function beside `insertion` (#93).
    assert "copies" in TIME_PHASES
    assert phase_of("copies") == "copies"
    assert phase_of("space/Index_1_0_edge") is None
