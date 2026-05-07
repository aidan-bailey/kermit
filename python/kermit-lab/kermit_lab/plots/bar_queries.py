"""Shape F — bar across queries for one or more ``(DS, algorithm)`` pairs.

Useful for showing how one (DS, algo) handles different workload shapes, or
for comparing multiple (DS, algo) pairs across the same workloads.

When more than one ``(ds, algo)`` pair is selected, each x-position becomes a
stacked bar — one segment per pair, coloured by data structure. Per-segment
error bars are dropped in stacked mode: propagating individual CIs to the
stack total assumes independence, and rendering them at segment boundaries
clutters the figure.
"""
from __future__ import annotations

from pathlib import Path
from typing import Iterable

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
from matplotlib.figure import Figure

from ..axis_mapping import colour_for_ds
from ..loader import BenchReport
from . import InsufficientAxesError

_GROUPING_AXES: tuple[str, ...] = (
    "benchmark",
    "query",
    "relation_path",
    "tuples",
    "arity",
    "relations",
    "relation_bytes",
)


def _resolve_selection(
    df: pd.DataFrame, column: str, value: str | Iterable[str] | None
) -> list[str]:
    """Return ``[value]`` / ``list(value)`` if given, else every value in ``df[column]``."""
    if value is None:
        return sorted(df[column].dropna().astype(str).unique().tolist())
    return [value] if isinstance(value, str) else list(value)


def _layout_positions(
    subgroups: list[tuple[str, pd.DataFrame]],
    *,
    within_step: float = 1.0,
    between_gap: float = 0.5,
) -> tuple[np.ndarray, list[float], list[str]]:
    """Cluster bars by query: tight spacing within a query, wider gap between queries.

    Returns ``(bar_positions, tick_positions, tick_labels)`` where each query
    gets exactly one tick centered over its run of bars.
    """
    bar_positions: list[float] = []
    tick_positions: list[float] = []
    tick_labels: list[str] = []
    cursor = 0.0
    prev_query: str | None = None
    current_run_xs: list[float] = []

    for query_name, _ in subgroups:
        if prev_query is not None and query_name != prev_query:
            tick_positions.append((current_run_xs[0] + current_run_xs[-1]) / 2)
            tick_labels.append(prev_query)
            cursor += between_gap
            current_run_xs = []
        bar_positions.append(cursor)
        current_run_xs.append(cursor)
        cursor += within_step
        prev_query = query_name

    if current_run_xs and prev_query is not None:
        tick_positions.append((current_run_xs[0] + current_run_xs[-1]) / 2)
        tick_labels.append(prev_query)

    return np.array(bar_positions), tick_positions, tick_labels


def plot(
    df: pd.DataFrame,
    *,
    ds: str | Iterable[str] | None = None,
    algo: str | Iterable[str] | None = None,
    phase: str = "iteration",
    out: Path | None = None,
) -> Figure:
    """Return a Figure of bars across ``query`` for the chosen ``(ds, algo)``.

    ``ds`` and ``algo`` accept either a single string, an iterable of
    strings, or ``None`` (default — use every value present in ``df``).
    With more than one resulting ``(ds, algo)`` pair, each x-position is
    rendered as a stacked bar (one segment per pair).
    """
    ds_list = _resolve_selection(df, "data_structure", ds)
    algo_list = _resolve_selection(df, "algorithm", algo)
    pairs = [(d, a) for d in ds_list for a in algo_list]

    matched = df[df.data_structure.isin(ds_list) & df.algorithm.isin(algo_list)]
    if matched.empty:
        raise InsufficientAxesError(
            f"no reports match data_structure in {ds_list!r} algorithm in {algo_list!r}"
        )
    sub = matched[
        (matched.metric == "time") & (matched.phase == phase) & matched["query"].notna()
    ]
    if sub.empty:
        raise InsufficientAxesError(
            f"({ds_list}, {algo_list}) matched but no time-metric phase={phase!r} "
            "Criterion groups carry a 'query' axis"
        )

    grouping_cols = [c for c in _GROUPING_AXES if c in sub.columns]
    inner_sort = ["query"] + [c for c in grouping_cols if c != "query"]
    sub = sub.sort_values(inner_sort + ["data_structure", "algorithm"]).reset_index(drop=True)
    subgroups: list[tuple[str, pd.DataFrame]] = []
    for query_name, query_df in sub.groupby("query", dropna=False, sort=False):
        for _, sub_df in query_df.groupby(grouping_cols, dropna=False, sort=False):
            subgroups.append((str(query_name), sub_df))

    x, xtick_positions, xtick_labels = _layout_positions(subgroups)
    bottom = np.zeros(len(subgroups))
    stacked = len(pairs) > 1

    fig, ax = plt.subplots()

    for d, a in pairs:
        heights = np.zeros(len(subgroups))
        lo = np.zeros(len(subgroups))
        hi = np.zeros(len(subgroups))
        for j, (_, sub_df) in enumerate(subgroups):
            match = sub_df[(sub_df.data_structure == d) & (sub_df.algorithm == a)]
            if match.empty:
                continue
            row = match.iloc[0]
            heights[j] = row.mean_ns
            lo[j] = max(row.mean_ns - row.mean_lo, 0.0)
            hi[j] = max(row.mean_hi - row.mean_ns, 0.0)
        if heights.sum() == 0:
            continue
        ax.bar(
            x,
            heights,
            bottom=bottom,
            color=colour_for_ds(d),
            edgecolor="black",
            linewidth=0.8,
            label=f"{d} / {a}" if stacked else None,
            yerr=None if stacked else [lo, hi],
        )
        bottom += heights

    ax.set_xticks(xtick_positions)
    ax.set_xticklabels(xtick_labels, rotation=30, ha="right")
    ax.set_ylabel("time (ns)")
    if stacked:
        ax.set_title("Query times — stacked")
        ax.legend(title="DS / algorithm", loc="best")
    else:
        d, a = pairs[0]
        ax.set_title(f"Query times — {d} / {a}")
    if out is not None:
        fig.savefig(out)
    return fig


def render(
    reports: Iterable[BenchReport],
    out_path: Path,
    criterion_root: Path,
    *,
    ds: str | Iterable[str] | None = None,
    algo: str | Iterable[str] | None = None,
    phase: str = "iteration",
) -> None:
    """Legacy API shim — builds the DataFrame, calls :func:`plot`, closes the figure."""
    from ..frame import _summary_from_reports

    df = _summary_from_reports(list(reports), criterion_root)
    fig = plot(df, ds=ds, algo=algo, phase=phase, out=out_path)
    plt.close(fig)
