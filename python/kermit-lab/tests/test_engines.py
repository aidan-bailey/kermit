"""kermit-lab reads engine rows: the v8 axes, the `materialize` phase, the `system`
back-fill, the cell label, and the `kl.engines()` preset (external engines, Story 11)."""
from __future__ import annotations

from pathlib import Path

import pandas as pd
import pytest

from kermit_lab.defaults import BINARY_AXIS_DEFAULTS, apply_axis_defaults
from kermit_lab.frame import load
from kermit_lab.loader import TIME_PHASES, load_reports, phase_of

from .conftest import _add_values, _FunctionSpec, _write_function_dir, _write_report

NEMO_VERSION = "nemo-physical 0.10.2-dev @ 04f0962b91ed"


def _write_time_functions(
    criterion_root: Path, group: str, points: dict[str, float]
) -> list[tuple[str, str, str]]:
    """One Criterion time function per entry of ``points`` (function -> mean), all in ``group``."""
    refs: list[tuple[str, str, str]] = []
    for function, point in points.items():
        samples = [(i + 1, point * (i + 1)) for i in range(10)]
        _write_function_dir(
            criterion_root, _FunctionSpec(group, function, "time", point, samples)
        )
        refs.append((group, function, "time"))
    return refs


@pytest.fixture
def engine_tree(tmp_path: Path) -> dict:
    """Four reports of one query, `triangle`: a v7 TreeTrie report (no `system`), a v8 HashTrie
    report, a v8 DuckDB report (no `space`, no `materialize`) and a v8 Nemo report (no
    `iteration` or `end_to_end`, but a `materialize` and a `space`)."""
    criterion_root = tmp_path / "target" / "criterion"
    reports_dir = tmp_path / "reports"
    criterion_root.mkdir(parents=True)
    reports_dir.mkdir()
    base = {
        "benchmark": "triangle", "query": "triangle", "tuples": 12,
        "optimiser": "lexicographic", "allocator": "jemalloc",
    }

    tree = _write_report(
        reports_dir, "tree-v7", kind="run",
        axes={**base, "data_structure": "TreeTrie", "algorithm": "LeapfrogTriejoin"},
        metadata=[],
        groups=_write_time_functions(
            criterion_root, "run/tri/TreeTrie", {"build": 300.0, "end_to_end": 900.0}
        ),
        schema_version=7,
    )
    _add_values(tree, [{"function": "space", "metric": "space", "estimator": "exact", "value": 6400}])

    hash_trie = _write_report(
        reports_dir, "hash-v8", kind="run",
        axes={
            **base, "system": "kermit", "data_structure": "HashTrie", "algorithm": "HashTriejoin",
        },
        metadata=[],
        groups=_write_time_functions(
            criterion_root, "run/tri/HashTrie",
            {"build": 500.0, "end_to_end": 700.0, "materialize": 2000.0},
        ),
        schema_version=8,
    )
    _add_values(hash_trie, [{"function": "space", "metric": "space", "estimator": "exact", "value": 9000}])

    _write_report(
        reports_dir, "duckdb-v8", kind="run",
        axes={**base, "system": "duckdb", "engine_version": "v1.4.4", "engine_threads": 1},
        metadata=[],
        groups=_write_time_functions(
            criterion_root, "run/tri/duckdb-t1",
            {"build": 400.0, "iteration": 150.0, "end_to_end": 800.0},
        ),
        schema_version=8,
    )

    nemo = _write_report(
        reports_dir, "nemo-v8", kind="run",
        axes={**base, "system": "nemo", "engine_version": NEMO_VERSION},
        metadata=[],
        groups=_write_time_functions(
            criterion_root, "run/tri/nemo", {"build": 600.0, "materialize": 1500.0}
        ),
        schema_version=8,
    )
    _add_values(nemo, [{"function": "space", "metric": "space", "estimator": "exact", "value": 3000}])

    return {
        "criterion_root": criterion_root,
        "reports_dir": reports_dir,
        "paths": sorted(reports_dir.glob("*.json")),
    }


def test_v7_and_v8_reports_load_together(engine_tree) -> None:
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    assert sorted(set(df["schema_version"])) == [7, 8]
    assert len(load_reports(engine_tree["paths"])) == 4


def test_a_report_without_system_is_back_filled_as_kermit(engine_tree) -> None:
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    v7 = df[df["data_structure"].isin(["TreeTrie"])]
    assert not v7.empty
    assert (v7["system"] == "kermit").all()
    assert sorted(set(df["system"])) == ["duckdb", "kermit", "nemo"]

    raw = load(engine_tree["paths"], engine_tree["criterion_root"], apply_defaults=False)
    raw_v7 = raw[raw["data_structure"].isin(["TreeTrie"])]
    assert raw_v7["system"].isna().all()


def test_engine_rows_carry_system_version_and_threads(engine_tree) -> None:
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    duck = df[df["system"] == "duckdb"]
    assert set(duck["engine_version"]) == {"v1.4.4"}
    assert set(duck["engine_threads"]) == {1}
    assert str(df["engine_threads"].dtype) == "Int64"
    nemo = df[df["system"] == "nemo"]
    assert set(nemo["engine_version"]) == {NEMO_VERSION}
    assert nemo["engine_threads"].isna().all()
    # An engine row has no structure, and the structure-scoped back-fills leave it so.
    engines = df[df["system"].isin(["duckdb", "nemo"])]
    assert engines["data_structure"].isna().all()
    assert engines["algorithm"].isna().all()


def test_materialize_is_a_time_phase(engine_tree) -> None:
    assert "materialize" in TIME_PHASES
    assert phase_of("materialize") == "materialize"
    assert phase_of("TreeTrie/materialize") == "materialize"
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    nemo_time = df[(df["system"] == "nemo") & (df["metric"] == "time")]
    assert set(nemo_time["phase"]) == {"build", "materialize"}


def test_system_default_fills_rows_without_it_and_keeps_the_rest() -> None:
    df = pd.DataFrame({
        "system": [pd.NA, "duckdb", pd.NA],
        "data_structure": ["TreeTrie", pd.NA, pd.NA],
    })
    out = apply_axis_defaults(df)
    assert out["system"].tolist() == ["kermit", "duckdb", "kermit"]
    assert BINARY_AXIS_DEFAULTS["system"] == "kermit"
    # The caller's frame is unchanged.
    assert df["system"].isna().tolist() == [True, False, True]
