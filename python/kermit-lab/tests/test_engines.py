"""kermit-lab reads engine rows: the v8 axes, the `materialize` phase, the `system`
back-fill, the cell label, and the `kl.engines()` preset (external engines, Story 11)."""
from __future__ import annotations

from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
import pandas as pd
import pytest
from matplotlib.figure import Figure

import kermit_lab as kl
from kermit_lab import presets
from kermit_lab.drivers.render_all import render_all
from kermit_lab.analysis import cell_labels, compare, speedup_table, summary
from kermit_lab.defaults import BINARY_AXIS_DEFAULTS, apply_axis_defaults
from kermit_lab.frame import load, load_samples
from kermit_lab.loader import TIME_PHASES, load_reports, phase_of
from kermit_lab.plots_errors import InsufficientAxesError

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
    report, a v8 DuckDB report (no `space`; a `materialize` function that DuckDB does not
    report in reality, so that no like-for-like pair may draw it) and a v8 Nemo report (no
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
            {"build": 400.0, "iteration": 150.0, "end_to_end": 800.0, "materialize": 1234.0},
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


def test_cell_labels_name_engines_by_system_and_kermit_cells_by_structure_and_algorithm() -> None:
    df = pd.DataFrame(
        {
            "system": ["kermit", "kermit", "duckdb", "nemo", pd.NA],
            "data_structure": ["TreeTrie", "HashTrie", pd.NA, pd.NA, "ColumnTrie"],
            "algorithm": ["LeapfrogTriejoin", "HashTriejoin", pd.NA, pd.NA, pd.NA],
            "engine_threads": pd.array([pd.NA, pd.NA, 4, pd.NA, pd.NA], dtype="Int64"),
        },
        index=[10, 11, 12, 13, 14],
    )
    labels = cell_labels(df)
    assert labels.tolist() == [
        "TreeTrie/LeapfrogTriejoin", "HashTrie/HashTriejoin", "duckdb-t4", "nemo", "ColumnTrie",
    ]
    # Aligned with the frame, so `df.assign(cell=cell_labels(df))` is safe.
    assert labels.index.tolist() == [10, 11, 12, 13, 14]


def test_cell_labels_of_a_frame_without_the_v8_columns() -> None:
    df = pd.DataFrame({"data_structure": ["TreeTrie"], "algorithm": ["LeapfrogTriejoin"]})
    assert cell_labels(df).tolist() == ["TreeTrie/LeapfrogTriejoin"]
    assert cell_labels(pd.DataFrame({"metric": ["time"]})).tolist() == ["kermit"]


def test_speedup_table_ignores_engine_rows() -> None:
    """An engine row has no build mode, so it cannot be a baseline or an arm: a frame that
    mixes engines in gives the table it gives without them."""
    kermit = []
    for mode, mean in (("serial", 100.0), ("parallel:2", 50.0)):
        kermit.append({
            "kind": "ds", "metric": "time", "phase": "build", "estimator": "criterion",
            "system": "kermit", "data_structure": "TreeTrie", "relation_path": "r.parquet",
            "ds_build_mode": mode, "threads": 2 if mode == "parallel:2" else pd.NA,
            "mean_ns": mean, "mean_lo": mean * 0.99, "mean_hi": mean * 1.01,
            "source_path": f"{mode}.json", "criterion_group": mode,
            "criterion_function": "TreeTrie/build",
        })
    engine = {
        "kind": "run", "metric": "time", "phase": "build", "estimator": "criterion",
        "system": "nemo", "data_structure": pd.NA, "relation_path": pd.NA,
        "ds_build_mode": pd.NA, "threads": pd.NA,
        "mean_ns": 10.0, "mean_lo": 9.0, "mean_hi": 11.0,
        "source_path": "nemo.json", "criterion_group": "nemo",
        "criterion_function": "build",
    }
    plain = pd.DataFrame(kermit)
    plain["threads"] = plain["threads"].astype("Int64")
    mixed = pd.DataFrame([*kermit, engine])
    mixed["threads"] = mixed["threads"].astype("Int64")

    expected = speedup_table(plain)
    got = speedup_table(mixed)
    assert got["speedup"].tolist() == pytest.approx([2.0])
    assert got["speedup"].tolist() == pytest.approx(expected["speedup"].tolist())
    assert got["threads"].tolist() == expected["threads"].tolist()


def _panels(fig: Figure) -> dict[str, list[float]]:
    """Each visible panel's title -> its sorted bar heights."""
    return {
        ax.get_title(): sorted(p.get_height() for p in ax.patches)
        for ax in fig.axes
        if ax.get_visible()
    }


def test_engines_draws_only_the_like_for_like_pairs(engine_tree) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    fig = kl.engines(df)
    assert isinstance(fig, Figure)
    panels = _panels(fig)
    # DuckDB against kermit on end_to_end only: its iteration (150) is not drawn.
    assert panels["duckdb vs kermit: end_to_end"] == [700.0, 800.0, 900.0]
    # Nemo against kermit on build, materialize and space. The v7 TreeTrie report has no
    # materialize function, so that panel holds the HashTrie cell and Nemo.
    assert panels["nemo vs kermit: build"] == [300.0, 500.0, 600.0]
    assert panels["nemo vs kermit: materialize"] == [1500.0, 2000.0]
    assert panels["nemo vs kermit: space"] == [3000.0, 6400.0, 9000.0]
    assert len(panels) == 4
    # DuckDB is paired on end_to_end only: neither its materialize (1234) nor a DuckDB
    # materialize panel is drawn.
    assert "duckdb vs kermit: materialize" not in panels
    assert not any(1234.0 in heights for heights in panels.values())
    plt.close("all")


def test_engines_labels_cells_by_system_and_by_structure_and_algorithm(engine_tree) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    fig = kl.engines(df)
    ax = next(a for a in fig.axes if a.get_title() == "nemo vs kermit: build")
    labels = set(ax.get_legend_handles_labels()[1])
    assert labels == {"HashTrie/HashTriejoin", "TreeTrie/LeapfrogTriejoin", "nemo"}
    ax = next(a for a in fig.axes if a.get_title() == "duckdb vs kermit: end_to_end")
    assert "duckdb-t1" in set(ax.get_legend_handles_labels()[1])
    plt.close("all")


def test_engines_skips_the_pairs_of_an_engine_that_is_absent(engine_tree) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    only_duckdb = df[~df["system"].isin(["nemo"])]
    assert list(_panels(kl.engines(only_duckdb))) == ["duckdb vs kermit: end_to_end"]
    plt.close("all")


def test_engines_needs_an_engine_row(fixture_tree) -> None:
    plt.close("all")
    kermit_only = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    with pytest.raises(InsufficientAxesError, match="no engine rows"):
        kl.engines(kermit_only)
    with pytest.raises(InsufficientAxesError, match="'system' column"):
        kl.engines(pd.DataFrame({"metric": ["time"]}))
    plt.close("all")


def test_engines_saves_to_out(engine_tree, tmp_path: Path) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    out = tmp_path / "engines.pdf"
    kl.engines(df, out=out)
    assert out.is_file() and out.stat().st_size > 0
    plt.close("all")


def test_engines_is_public() -> None:
    assert kl.engines is presets.engines
    assert "engines" in kl.__all__


def test_summary_pivots_engine_rows_by_system(engine_tree) -> None:
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    table = summary(df[df["metric"] == "time"], rows="system", cols="phase")
    assert list(table.index) == ["duckdb", "kermit", "nemo"]
    assert table.loc["duckdb", "iteration"] == 150.0
    assert table.loc["nemo", "materialize"] == 1500.0
    assert table.loc["kermit", "materialize"] == 2000.0


def test_compare_pairs_kermit_cells_and_ignores_engine_rows() -> None:
    df = pd.DataFrame(
        {
            "system": ["kermit", "kermit", "duckdb"],
            "data_structure": ["TreeTrie", "HashTrie", pd.NA],
            "query": ["triangle", "triangle", "triangle"],
            "mean_ns": [300.0, 150.0, 100.0],
            "mean_lo": [290.0, 140.0, 90.0],
            "mean_hi": [310.0, 160.0, 110.0],
        }
    )
    out = compare(df, baseline="TreeTrie", target="HashTrie")
    assert len(out) == 1
    assert out["speedup"].iloc[0] == pytest.approx(2.0)
    assert out["system"].tolist() == ["kermit"]


def _bar_heights(fig: Figure) -> list[float]:
    return sorted(p.get_height() for ax in fig.axes if ax.get_visible() for p in ax.patches)


def test_the_structure_figures_draw_kermit_rows_only(engine_tree) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    # Nemo's 3000 bytes are not drawn: only the TreeTrie (v7, no `system`) and HashTrie rows are.
    assert _bar_heights(presets.bar_space(df)) == [6400.0, 9000.0]
    # DuckDB's end_to_end (800) is not drawn either.
    fig = presets.bar_time(df, query="triangle", phase="end_to_end")
    assert _bar_heights(fig) == [700.0, 900.0]
    plt.close("all")


def test_render_all_takes_a_frame_holding_engine_rows(engine_tree, tmp_path: Path) -> None:
    plt.close("all")
    reports = load_reports(engine_tree["paths"])
    render_all(reports, tmp_path, engine_tree["criterion_root"], "pdf", phase="end_to_end")
    names = {p.name for p in tmp_path.iterdir()}
    assert {"bar-space.pdf", "bar-time-triangle.pdf"} <= names
    assert plt.get_fignums() == []


def test_engines_draws_kermit_rows_only_of_the_queries_an_engine_ran(engine_tree) -> None:
    """A kermit row of a query no engine ran, and a `bench ds` row (no query), are not drawn."""
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    expected = _panels(kl.engines(df))
    kermit = df[df["system"] == "kermit"]
    other_query = kermit.assign(query="chain", mean_ns=kermit["mean_ns"] + 11.0)
    ds_rows = kermit.assign(query=pd.NA, kind="ds", mean_ns=kermit["mean_ns"] + 22.0)
    fig = kl.engines(pd.concat([df, other_query, ds_rows], ignore_index=True))
    assert _panels(fig) == expected
    plt.close("all")


def test_engines_keys_on_the_query_alone_without_a_benchmark_column(engine_tree) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    expected = _panels(kl.engines(df))
    kermit = df[df["system"] == "kermit"]
    other_query = kermit.assign(query="chain", mean_ns=kermit["mean_ns"] + 11.0)
    mixed = pd.concat([df, other_query], ignore_index=True).drop(columns=["benchmark"])
    assert _panels(kl.engines(mixed)) == expected
    plt.close("all")


def test_engines_refuses_two_rows_of_one_cell_and_query(engine_tree) -> None:
    """Both optimisers' rows of one cell would be drawn as overlaid bars."""
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    hash_trie = df[df["data_structure"] == "HashTrie"]
    both = pd.concat(
        [df, hash_trie.assign(optimiser="cardinality", mean_ns=hash_trie["mean_ns"] + 5.0)],
        ignore_index=True,
    )
    with pytest.raises(InsufficientAxesError, match=r"HashTrie/HashTriejoin.*triangle.*filter"):
        kl.engines(both)
    # Filtering by optimiser, as the message says, draws it.
    kl.engines(both[both["optimiser"] == "lexicographic"])
    plt.close("all")


def test_engines_draws_a_kermit_row_without_system_as_kermit(engine_tree) -> None:
    """Loaded without defaults, the v7 report has no `system`: it is still kermit's."""
    plt.close("all")
    raw = load(engine_tree["paths"], engine_tree["criterion_root"], apply_defaults=False)
    assert raw.loc[raw["data_structure"] == "TreeTrie", "system"].isna().all()
    panels = _panels(kl.engines(raw))
    assert panels["nemo vs kermit: build"] == [300.0, 500.0, 600.0]
    assert panels["duckdb vs kermit: end_to_end"] == [700.0, 800.0, 900.0]
    plt.close("all")


def _legend_labels(fig: Figure) -> set[str]:
    return {
        label for ax in fig.axes if ax.get_visible() for label in ax.get_legend_handles_labels()[1]
    }


def test_every_structure_preset_draws_kermit_rows_only(engine_tree) -> None:
    """`scaling`, `tradeoff`, `dist` and `bar_queries` (like `bar_space` and `bar_time`) drop
    the engines' rows. The presets are called directly: `render_all` swallows their errors."""
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    samples = load_samples(engine_tree["paths"], engine_tree["criterion_root"])
    kermit_cells = {"HashTrie / HashTriejoin", "TreeTrie / LeapfrogTriejoin"}

    fig = presets.scaling(df, phase="build")
    assert _legend_labels(fig) == kermit_cells
    fig = presets.bar_queries(df, phase="build")
    assert _legend_labels(fig) == kermit_cells
    assert _bar_heights(fig) == [300.0, 500.0]
    fig = presets.tradeoff(df, phase="build")
    assert _legend_labels(fig) == kermit_cells
    points = sorted(
        tuple(xy) for ax in fig.axes for c in ax.collections for xy in c.get_offsets().tolist()
    )
    assert points == [(6400.0, 300.0), (9000.0, 500.0)]

    only_kermit = df[df["system"] == "kermit"]
    fig = presets.dist(df, samples=samples, phase="build")
    reference = presets.dist(only_kermit, samples=samples, phase="build")
    assert [len(ax.collections) for ax in fig.axes] == [len(ax.collections) for ax in reference.axes]
    assert [t.get_text() for ax in fig.axes for t in ax.get_xticklabels()] == [
        t.get_text() for ax in reference.axes for t in ax.get_xticklabels()
    ]
    plt.close("all")


def _bar_colour_plot(df: pd.DataFrame, **kwargs) -> Figure:
    return kl.plot(df, kind="bar", x="query", y="time", phase="end_to_end", **kwargs)


def test_plot_colours_a_frame_holding_engine_rows_by_cell(engine_tree) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    fig = _bar_colour_plot(df)
    assert _legend_labels(fig) == {"TreeTrie/LeapfrogTriejoin", "HashTrie/HashTriejoin", "duckdb-t1"}
    fig = kl.plot(df, kind="bar", x="query", y="time", phase="build")
    assert _legend_labels(fig) == {
        "TreeTrie/LeapfrogTriejoin", "HashTrie/HashTriejoin", "duckdb-t1", "nemo",
    }
    # The tradeoff scatter takes the default too: a point per cell with time and space.
    fig = kl.plot(df, kind="scatter", x="space", y="time", phase="build")
    assert _legend_labels(fig) == {"TreeTrie/LeapfrogTriejoin", "HashTrie/HashTriejoin", "nemo"}
    plt.close("all")


def test_plot_leaves_a_kermit_only_frame_uncoloured_and_unmutated(engine_tree) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    kermit = df[df["data_structure"].notna()].copy()
    columns = list(kermit.columns)
    fig = _bar_colour_plot(kermit)
    # One series, labelled by the metric as before, and no `cell` column added to the caller's.
    assert _legend_labels(fig) == {"time"}
    assert list(kermit.columns) == columns
    # A frame whose `system` is all kermit, or has no `system` at all, is the same case.
    kermit["system"] = "kermit"
    assert _legend_labels(_bar_colour_plot(kermit)) == {"time"}
    assert _legend_labels(_bar_colour_plot(kermit.drop(columns=["system"]))) == {"time"}
    # The engine frame itself is not mutated either.
    columns = list(df.columns)
    _bar_colour_plot(df)
    assert list(df.columns) == columns and "cell" not in df.columns
    plt.close("all")


def test_plot_explicit_colour_beats_the_cell_default(engine_tree) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    fig = _bar_colour_plot(df, colour="system")
    assert _legend_labels(fig) == {"kermit", "duckdb"}
    plt.close("all")
