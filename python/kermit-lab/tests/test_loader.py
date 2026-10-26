"""Loader parses v2 reports, refuses unknown majors, surfaces axes verbatim."""
from __future__ import annotations

import json
from pathlib import Path

import pytest

from kermit_lab.loader import SchemaError, iter_values, load_reports


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
    for version in (2, 3, 4, 5):
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
    with pytest.raises(SchemaError) as exc:
        load_reports([v3, v4])
    message = str(exc.value)
    assert (
        "to compare TreeTrie space, ColumnTrie space and bench run / bench join iteration "
        "outside lazy-expansion HashTrie cells only" in message
    )


def test_the_flat_tuples_boundary_names_lazy_iteration_as_changed(tmp_path: Path) -> None:
    # A lazy HashTrie's `iteration` runs on a fresh build and times child
    # expansion, which v4 changed (#111): it must not be offered as comparable.
    v3, v4 = (_versioned_report(tmp_path / f"v{v}.json", v) for v in (3, 4))
    with pytest.raises(SchemaError) as exc:
        load_reports([v3, v4])
    message = str(exc.value)
    assert "changes insertion, copies, end_to_end, HashTrie's space and lazy-expansion HashTrie's iteration" in message
    assert "outside lazy-expansion HashTrie cells" in message


def test_phase_of_recognises_copies() -> None:
    from kermit_lab.loader import TIME_PHASES, phase_of

    # `bench run` under `--column-orders any` times a query's reordered
    # copies as one function beside `insertion` (#93).
    assert "copies" in TIME_PHASES
    assert phase_of("copies") == "copies"
    assert phase_of("space/Index_1_0_edge") is None


def test_refuses_to_mix_reports_across_the_per_query_build_boundary(tmp_path: Path) -> None:
    from kermit_lab.loader import PER_QUERY_BUILD_SCHEMA

    assert PER_QUERY_BUILD_SCHEMA == 5
    old = _versioned_report(tmp_path / "v4.json", 4)
    new = _versioned_report(tmp_path / "v5.json", 5)
    with pytest.raises(SchemaError, match="refusing to mix schema_version 4"):
        load_reports([old, new])
    with pytest.raises(SchemaError, match="builds, and times, only the indexes it reads"):
        load_reports([new, old])
    reports = load_reports([old, new], allow_mixed_schema=True)
    assert [r.schema_version for r in reports] == [4, 5]


def test_no_metric_is_comparable_across_the_per_query_build_boundary(tmp_path: Path) -> None:
    """v5 changes `iteration`, `end_to_end` and `space` per query and replaces
    `insertion` and `copies` with `build`, so the message offers nothing to compare."""
    v4 = _versioned_report(tmp_path / "v4.json", 4)
    v5 = _versioned_report(tmp_path / "v5.json", 5)
    with pytest.raises(SchemaError) as exc:
        load_reports([v4, v5])
    assert "no metric is comparable across them all" in str(exc.value)


def test_a_load_straddling_v4_and_v5_names_both_boundaries(tmp_path: Path) -> None:
    v3 = _versioned_report(tmp_path / "v3.json", 3)
    v5 = _versioned_report(tmp_path / "v5.json", 5)
    with pytest.raises(SchemaError) as exc:
        load_reports([v3, v5])
    message = str(exc.value)
    assert "from v4 every structure builds from one flat tuple buffer" in message
    assert "also straddles v5, where each query builds, and times, only the indexes it reads" in message
    assert "no metric is comparable across them all" in message


def test_phase_of_recognises_build() -> None:
    from kermit_lab.loader import TIME_PHASES, phase_of

    # `bench run` writes the bare token; `bench ds` prefixes the DS name (v5).
    assert phase_of("build") == "build"
    assert phase_of("TreeTrie/build") == "build"
    assert phase_of("space") is None
    # Reports before v5 still load: their phases stay recognised.
    assert TIME_PHASES == ("build", "insertion", "copies", "iteration", "end_to_end")
    assert phase_of("TreeTrie/insertion") == "insertion"


def _report_with_values(tmp_path: Path, values: list[dict] | None) -> Path:
    """One schema-5 probe report whose ``values`` list is ``values`` (absent
    when ``None``)."""
    obj = {
        "schema_version": 5,
        "kind": "probe",
        "metadata": [],
        "axes": {"query": "q1", "data_structure": "TreeTrie"},
        "criterion_groups": [],
    }
    if values is not None:
        obj["values"] = values
    p = tmp_path / "values.json"
    p.write_text(json.dumps([obj]))
    return p


def test_values_parse_into_value_entries(tmp_path: Path) -> None:
    p = _report_with_values(tmp_path, [
        {"function": "space", "metric": "space", "estimator": "exact", "value": 65664},
        {"function": "build", "metric": "time", "estimator": "single", "value": 1361000},
    ])
    [r] = load_reports([p])
    assert [(v.function, v.metric, v.estimator, v.value) for v in r.values] == [
        ("space", "space", "exact", 65664.0),
        ("build", "time", "single", 1361000.0),
    ]
    assert r.has_metric("space")
    assert r.has_metric("time")


def test_a_report_without_values_has_none(tmp_path: Path) -> None:
    [r] = load_reports([_report_with_values(tmp_path, None)])
    assert r.values == []
    assert not r.has_metric("space")


def test_iter_values_pairs_each_entry_with_its_report(tmp_path: Path) -> None:
    p = _report_with_values(tmp_path, [
        {"function": "space", "metric": "space", "estimator": "exact", "value": 8},
    ])
    reports = load_reports([p])
    [(report, entry)] = list(iter_values(reports))
    assert report is reports[0]
    assert entry.value == 8.0


def test_v5_and_v6_reports_load_together(tmp_path: Path) -> None:
    paths = []
    for version in (5, 6):
        p = tmp_path / f"v{version}.json"
        p.write_text(json.dumps([{
            "schema_version": version, "kind": "run", "metadata": [], "axes": {},
            "criterion_groups": [], "values": [],
        }]))
        paths.append(p)
    assert [r.schema_version for r in load_reports(paths)] == [5, 6]
