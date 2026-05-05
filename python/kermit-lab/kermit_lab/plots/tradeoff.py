"""Shape D — space vs time scatter (one point per ``(DS, algorithm)`` pair).

Default axes: log-x (space, bytes), linear-y (time, ns). Each report
contributes one point if it has both a time- and a space-metric Criterion
group; reports with only one metric are skipped.
"""
from __future__ import annotations

from pathlib import Path
from typing import Iterable

import matplotlib.pyplot as plt
import pandas as pd
from matplotlib.figure import Figure

from ..axis_mapping import colour_for_ds, marker_for_algo
from ..loader import BenchReport
from . import InsufficientAxesError


def plot(
    df: pd.DataFrame,
    *,
    phase: str = "iteration",
    out: Path | None = None,
) -> Figure:
    """Return a space-vs-time scatter Figure.

    Each point is one report's mean time (at ``phase``) on the y-axis vs its
    mean space on the x-axis. Reports lacking either metric are skipped.
    """
    time_rows = df[(df.metric == "time") & (df.phase == phase)]
    space_rows = df[df.metric == "space"]

    # One time point and one space point per (source_path, ds, algo). For
    # `bench run` outputs with multiple space rows per report, aggregate by
    # mean — same as the legacy implementation.
    keys = ["source_path", "data_structure", "algorithm"]
    time_agg = time_rows.groupby(keys, dropna=False)["mean_ns"].mean().reset_index(name="time_mean")
    space_agg = space_rows.groupby(keys, dropna=False)["mean_ns"].mean().reset_index(name="space_mean")
    merged = time_agg.merge(space_agg, on=keys, how="inner")

    if merged.empty:
        raise InsufficientAxesError(
            f"tradeoff needs ≥1 report with both space and time phase={phase!r} rows"
        )

    fig, ax = plt.subplots()
    for (ds, algo), grp in merged.groupby(["data_structure", "algorithm"], dropna=False):
        ax.scatter(
            grp.space_mean.tolist(),
            grp.time_mean.tolist(),
            label=f"{ds} / {algo}",
            color=colour_for_ds(str(ds)),
            marker=marker_for_algo(str(algo)),
            s=40,
            edgecolor="black",
            linewidth=0.5,
        )

    ax.set_xscale("log")
    ax.set_xlabel("space (bytes)")
    ax.set_ylabel("time (ns)")
    ax.set_title("Space–time tradeoff")
    ax.legend(title="DS / algorithm", loc="best")
    if out is not None:
        fig.savefig(out)
    return fig


def render(
    reports: Iterable[BenchReport],
    out_path: Path,
    criterion_root: Path,
    *,
    phase: str = "iteration",
) -> None:
    """Legacy API shim — builds the DataFrame, calls :func:`plot`, closes the figure."""
    from ..frame import _summary_from_reports

    df = _summary_from_reports(list(reports), criterion_root)
    fig = plot(df, phase=phase, out=out_path)
    plt.close(fig)
