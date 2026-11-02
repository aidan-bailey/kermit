"""Named presets: each fills an AestheticMap and calls kl.plot.

These are the legacy shapes, re-expressed as configurations of the general
engine. They exist for ergonomics and for the CLI / render-all to call.
"""
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Optional, Sequence

import pandas as pd
from matplotlib.figure import Figure

from .analysis import (
    KERMIT_SYSTEM,
    SPEEDUP_MEASURES,
    baseline_build_mode,
    cell_labels,
    speedup_table,
)
from .encoding import AestheticMap, resolve_scalar, select
from .facet import finish, make_grid
from .frame import build_of
from .geoms import draw_bar
from .loader import TIME_PHASES
from .plot import plot
from .plots_errors import InsufficientAxesError
from .styles import apply as apply_style


def _kermit_rows(df: pd.DataFrame) -> pd.DataFrame:
    """``df`` without the external engines' rows. An engine row has no ``data_structure`` or
    ``algorithm``, which the figures below put on their axes and in their legends; they draw
    kermit's cells, and :func:`engines` draws the engines beside them. A row without ``system``
    (a v7 report, or a frame loaded without defaults) is kermit's."""
    if "system" not in df.columns:
        return df
    is_engine = df["system"].notna() & (df["system"] != KERMIT_SYSTEM)
    return df[~is_engine]


def scaling(df: pd.DataFrame, *, phase: str = "iteration", out: Optional[Path] = None) -> Figure:
    """Time against tuple count, a line per structure.

    Draws kermit's own cells only: the external engines' rows are dropped (they have no
    ``data_structure`` or ``algorithm``); see :func:`engines` for the engines beside kermit.
    """
    return plot(_kermit_rows(df), kind="line", x="tuples", y="time", colour="data_structure",
                style="algorithm", phase=phase, logx=True, logy=True,
                title="Scaling", out=out)


def bar_time(
    df: pd.DataFrame, *, query: str, phase: str = "iteration", out: Optional[Path] = None
) -> Figure:
    """Time of one query, a bar per structure.

    Draws kermit's own cells only: the external engines' rows are dropped (they have no
    ``data_structure`` or ``algorithm``); see :func:`engines` for the engines beside kermit.
    """
    return plot(_kermit_rows(df), kind="bar", x="data_structure", y="time",
                colour="data_structure",
                style="algorithm", filter={"query": query}, phase=phase,
                title=f"Time — {query}", out=out)


def bar_space(df: pd.DataFrame, *, out: Optional[Path] = None) -> Figure:
    """Space per data structure.

    Draws kermit's own cells only: the external engines' rows are dropped (they have no
    ``data_structure`` or ``algorithm``); see :func:`engines` for the engines beside kermit.
    """
    return plot(_kermit_rows(df), kind="bar", x="data_structure", y="space",
                colour="data_structure",
                title="Space per data structure", out=out)


def tradeoff(df: pd.DataFrame, *, phase: str = "iteration", out: Optional[Path] = None) -> Figure:
    """Space against time, a point per cell.

    Draws kermit's own cells only: the external engines' rows are dropped (they have no
    ``data_structure`` or ``algorithm``); see :func:`engines` for the engines beside kermit.
    """
    return plot(_kermit_rows(df), kind="scatter", x="space", y="time", colour="data_structure",
                style="algorithm", phase=phase, logx=True, title="Space vs time", out=out)


def dist(
    df: pd.DataFrame, *, samples: pd.DataFrame, phase: str = "iteration",
    out: Optional[Path] = None,
) -> Figure:
    """The per-iteration time distribution per structure.

    Draws kermit's own cells only: the external engines' rows are dropped (they have no
    ``data_structure`` or ``algorithm``); see :func:`engines` for the engines beside kermit.
    """
    return plot(_kermit_rows(df), kind="violin", x="data_structure", y="time",
                style="algorithm",
                phase=phase, samples=samples, title="Per-iter distribution", out=out)


def bar_queries(
    df: pd.DataFrame, *, ds: Optional[Sequence[str]] = None,
    algo: Optional[Sequence[str]] = None, phase: str = "iteration",
    out: Optional[Path] = None,
) -> Figure:
    """Time across queries, a bar per structure.

    Draws kermit's own cells only: the external engines' rows are dropped (they have no
    ``data_structure`` or ``algorithm``); see :func:`engines` for the engines beside kermit.
    """
    flt: dict[str, object] = {}
    if ds is not None:
        flt["data_structure"] = list(ds)
    if algo is not None:
        flt["algorithm"] = list(algo)
    return plot(_kermit_rows(df), kind="bar", x="query", y="time", colour="data_structure",
                style="algorithm", filter=flt, phase=phase,
                title="Across queries", out=out)


@dataclass(frozen=True)
class AxisScope:
    """The time phases an optimisation axis can affect, and why."""

    phases: frozenset[str]
    reason: str


# The time phases an optimisation axis can affect; an axis absent from this
# map can affect every phase. `ablation` refuses an axis on a phase outside
# its scope: two values there time the same code, so any difference between
# them is noise or binary drift, not the optimisation. Give a new axis an
# entry when its effect is confined to some phases.
#
# `ds_build_mode` has no entry. A BuildMode builds the same contents and
# capacities, so it cannot move `space` (not a time phase), but it may place
# keys in other slots of a hash table, and its allocations land elsewhere in
# memory, so it can move every time phase, `iteration` included (the
# optimization standard's Amendment 2, 2026-10-06).
AXIS_PHASES: dict[str, AxisScope] = {
    # A seek strategy changes how a built trie is searched; no build calls
    # seek (#80). A `materialize` runs the join, so it searches too.
    "ds_layout_seek": AxisScope(
        frozenset({"iteration", "end_to_end", "materialize"}),
        "it changes only how a built trie is searched, as a join does",
    ),
}


def ablation(
    df: pd.DataFrame, *, axis: str, phase: str = "iteration", out: Optional[Path] = None
) -> Figure:
    """Ablation: time vs an optimization axis, coloured by DS, faceted by query when >1.

    Raises :class:`InsufficientAxesError` for an axis on a phase outside its
    :data:`AXIS_PHASES` scope.
    """
    scope = AXIS_PHASES.get(axis)
    if scope is not None and phase not in scope.phases:
        allowed = " or ".join(repr(p) for p in sorted(scope.phases))
        raise InsufficientAxesError(
            f"{axis} cannot affect phase {phase!r}: {scope.reason}; plot it on {allowed}"
        )
    # Rows without the axis belong to structures that do not have it. (An
    # unknown axis falls through to `plot`, which names the missing column.)
    if axis in df.columns:
        df = df[df[axis].notna()]
    facet = "query" if "query" in df.columns and df["query"].nunique(dropna=True) > 1 else None
    return plot(df, kind="bar", x=axis, y="time", colour="data_structure",
                facet=facet, phase=phase, title=f"Ablation — {axis}", out=out)


def speedup(
    df: pd.DataFrame, *, phase: str = "build", baseline: str | None = None,
    out: Optional[Path] = None,
) -> Figure:
    """Build speedup over the baseline build against thread count (#94): each
    structure's default single-threaded build unless ``baseline`` is given.

    One line per case of :func:`~kermit_lab.analysis.speedup_table` and build
    (``parallel`` or ``presized``, the label naming the build when both
    appear), with its CI as a band, and the ideal ``speedup = N`` dashed. Both
    axes are log2, so each doubling of threads is one step and the ideal is a
    straight line. The title names the presized build when it is the only one
    plotted. Load one binary's reports only (see
    :func:`~kermit_lab.analysis.speedup_table`).

    Raises :class:`InsufficientAxesError` for a phase that is not a time
    phase, or when no case has both a baseline and a ``parallel:N`` /
    ``presized:N`` row. Every time phase is allowed: a build mode may move
    ``iteration`` too (Amendment 2).
    """
    if phase not in TIME_PHASES:
        allowed = " or ".join(repr(p) for p in TIME_PHASES)
        raise InsufficientAxesError(
            f"speedup compares time phases, not {phase!r}; plot it on {allowed}"
        )
    try:
        table = speedup_table(df, phase=phase, baseline=baseline)
    except ValueError as e:
        raise InsufficientAxesError(str(e)) from e

    apply_style()
    fig, axes = make_grid(1)
    ax = axes[0]
    identity = [c for c in table.columns if c not in SPEEDUP_MEASURES]
    varying = [c for c in identity if table[c].nunique(dropna=False) > 1] or ["data_structure"]
    # A line runs over thread counts, so it is keyed by the build
    # (`build_of`), not by `ds_build_mode`, which carries N: `parallel:N` and
    # `presized:N` are then two lines, never one through both.
    build = table["ds_build_mode"].map(build_of).rename("build")
    if build.nunique() > 1:
        varying = [*varying, build]
    for key, line in table.groupby(varying, dropna=False, sort=True):
        line = line.sort_values("threads")
        parts = key if isinstance(key, tuple) else (key,)
        label = " / ".join(str(p) for p in parts if not pd.isna(p))
        ax.plot(line["threads"], line["speedup"], marker="o", label=label)
        ax.fill_between(line["threads"], line["speedup_lo"], line["speedup_hi"], alpha=0.2)
    ticks = sorted(int(t) for t in table["threads"].unique())
    top = ticks[-1]
    ax.plot([1, top], [1, top], linestyle="--", color="grey", label="ideal")
    # Both axes log2: the ideal speedup = N is then the straight line through
    # (1, 1) and (top, top), and each doubling of threads is one step.
    ax.set_xscale("log", base=2)
    ax.set_yscale("log", base=2)
    ax.set_xticks(ticks, labels=[str(t) for t in ticks])
    ax.set_xlabel("threads")
    over = baseline
    if over is None:
        over = " / ".join(sorted({baseline_build_mode(s) for s in table["data_structure"]}))
    ax.set_ylabel(f"speedup over {over} ({phase})")
    # A figure of presized arms alone says so; parallel-only and mixed figures
    # keep the title they always had.
    lone_presized = set(build) == {"presized"}
    title = "Presized build speedup" if lone_presized else "Parallel build speedup"
    finish(fig, axes, title=title, out=out)
    return fig


#: The like-for-like pairs (the external engines' design, Summary), as ``(engine, y, phase)``:
#: an engine is drawn against kermit's own cells only where both do the same work. DuckDB's
#: index work (its hash tables) sits in its ``iteration`` while kermit's sits in ``build``, so
#: only ``end_to_end`` is the same work; Nemo builds its tries in ``build`` and stores its
#: result in ``materialize``, as kermit's structures and their ``materialize`` do, and its
#: exact ``space`` is a trie's, as theirs are.
ENGINE_PAIRS: tuple[tuple[str, str, str], ...] = (
    ("duckdb", "time", "end_to_end"),
    ("nemo", "time", "build"),
    ("nemo", "time", "materialize"),
    ("nemo", "space", "space"),
)


def _query_keys(rows: pd.DataFrame) -> list[tuple]:
    """The ``(benchmark, query)`` of every row of ``rows`` (``benchmark`` ``None`` for a frame
    without the column), a missing value as ``None``."""
    benchmarks = rows["benchmark"] if "benchmark" in rows.columns else [None] * len(rows)
    return [
        tuple(None if pd.isna(v) else v for v in pair) for pair in zip(benchmarks, rows["query"])
    ]


def _same_queries_as_engine(rows: pd.DataFrame, engine: str) -> pd.DataFrame:
    """``rows`` with the kermit rows of a ``(benchmark, query)`` the ``engine`` has no row for
    dropped: a ``bench ds`` row (no query) and a query only kermit ran have nothing to compare."""
    keys = _query_keys(rows)
    ran = {k for k, system in zip(keys, rows["system"]) if system == engine}
    keep = [system == engine or k in ran for k, system in zip(keys, rows["system"])]
    return rows[keep]


def _refuse_overlaid_bars(rows: pd.DataFrame, engine: str, phase: str) -> None:
    """Raise when two rows of a panel share a ``(cell, query)``: the bar chart places bars by
    query alone, so it draws one bar for each, at the same place, whatever their benchmark."""
    benchmarks: dict[tuple, list] = {}
    for cell, (benchmark, query) in zip(rows["cell"], _query_keys(rows)):
        benchmarks.setdefault((cell, query), []).append(benchmark)
    for (cell, query), found in benchmarks.items():
        if len(found) < 2:
            continue
        if len(set(found)) > 1:
            names = ", ".join(sorted(map(str, set(found))))
            raise InsufficientAxesError(
                f"engines: cell {cell!r}, query {query!r} has rows from more than one benchmark "
                f"({names}) in the {engine} vs kermit {phase} panel, and bars are placed by "
                "query alone; filter the frame by benchmark first"
            )
        raise InsufficientAxesError(
            f"engines: more than one row for cell {cell!r}, query {query!r} in the {engine} "
            f"vs kermit {phase} panel; filter the frame first (by optimiser or the ds_* axes) "
            "so that each cell has one row per query"
        )


def engines(df: pd.DataFrame, *, out: Optional[Path] = None) -> Figure:
    """Each engine against kermit's cells on the pairs that compare like with like
    (:data:`ENGINE_PAIRS`): DuckDB on ``end_to_end``; Nemo on ``build``, ``materialize`` and
    ``space``. One panel per pair that has engine rows, bars per query coloured by cell
    (:func:`~kermit_lab.analysis.cell_labels`), Criterion estimates for time and exact bytes for
    space. A row without ``system`` counts as kermit's.

    In each panel only the kermit rows of a ``(benchmark, query)`` that the pair's engine has
    rows for are drawn, so a ``bench ds`` row and a query no engine ran drop out (a frame
    without ``benchmark`` is keyed on ``query`` alone). A panel draws one bar per cell and
    query, placed by ``query`` alone: when several rows share a ``(cell, query)`` the figure
    would overlay them, so filter the frame first. The rows may differ in ``optimiser``, in the
    ``ds_*`` axes (the Layout variants of one structure), in ``kind`` (probe beside run
    reports), or in ``benchmark`` (benchmarks that share query names, such as ``h2o`` and
    ``h2o-sorted``).

    Raises :class:`InsufficientAxesError` when ``df`` has no ``system`` column (load v8
    reports), no engine row on any pair, or more than one row for a ``(cell, query)`` of a
    panel.
    """
    if "system" not in df.columns:
        raise InsufficientAxesError(
            "engines needs the 'system' column that report v8 adds; load v8 reports"
        )
    labelled = df.assign(
        cell=cell_labels(df),
        system=df["system"].mask(df["system"].isna(), KERMIT_SYSTEM),
    )
    panels = []
    for engine, y, phase in ENGINE_PAIRS:
        pair = labelled[labelled["system"].isin([engine, KERMIT_SYSTEM])]
        if not (pair["system"] == engine).any():
            continue
        amap = AestheticMap(kind="bar", x="query", y=y, colour="cell", phase=phase)
        try:
            rows = select(pair, amap)
        except InsufficientAxesError:
            continue
        if not (rows["system"] == engine).any():
            continue
        rows = _same_queries_as_engine(rows, engine)
        _refuse_overlaid_bars(rows, engine, phase)
        title = f"{engine} vs kermit: {phase}"
        ylabel = "space (bytes)" if y == "space" else "time (ns)"
        panels.append((title, ylabel, resolve_scalar(rows, amap)))
    if not panels:
        raise InsufficientAxesError(
            "no engine rows on a like-for-like pair (duckdb on end_to_end; nemo on build, "
            "materialize and space)"
        )

    apply_style()
    fig, axes = make_grid(len(panels))
    for ax, (title, ylabel, series) in zip(axes, panels):
        draw_bar(ax, series, xlabel="query", ylabel=ylabel)
        ax.set_title(title)
    finish(fig, axes, title="Engines against kermit, like-for-like pairs", out=out)
    return fig
