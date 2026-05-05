"""Shape C — log-log scaling: time vs ``tuples`` for each ``(DS, algorithm)``.

Aggregates many ``BenchReport``s (one per ``(DS, algo, dataset)``) into
grouped lines. The ``tuples`` axis comes from each report; the y-axis is
Criterion's ``mean`` point estimate (with 95% CI as error bars).
"""
from __future__ import annotations

from pathlib import Path
from typing import Iterable

import matplotlib.pyplot as plt
import pandas as pd
from matplotlib.figure import Figure

from ..axis_mapping import colour_for_ds, linestyle_for_algo, marker_for_algo
from ..loader import BenchReport
from . import InsufficientAxesError


def plot(
    df: pd.DataFrame,
    *,
    phase: str = "iteration",
    out: Path | None = None,
) -> Figure:
    """Return a log-log scaling Figure (and optionally save to ``out``).

    Filters ``df`` to time-metric rows matching ``phase`` and groups by
    ``(data_structure, algorithm)``. Raises :class:`InsufficientAxesError` if
    fewer than 2 distinct ``tuples`` values remain.
    """
    sub = df[(df.metric == "time") & (df.phase == phase) & df.tuples.notna()]
    distinct = sorted(set(sub.tuples.dropna().tolist()))
    if len(distinct) < 2:
        raise InsufficientAxesError(
            f"scaling (phase={phase}) needs ≥2 distinct 'tuples' values; got {distinct}"
        )

    fig, ax = plt.subplots()
    for (ds, algo), grp in sub.groupby(["data_structure", "algorithm"], dropna=False):
        grp = grp.sort_values("tuples")
        xs = grp.tuples.tolist()
        ys = grp.mean_ns.tolist()
        # Clamp to ≥0: pathological CIs can fall outside the point estimate.
        lo = [max(0.0, m - l) for m, l in zip(ys, grp.mean_lo.tolist())]
        hi = [max(0.0, h - m) for m, h in zip(ys, grp.mean_hi.tolist())]
        ax.errorbar(
            xs,
            ys,
            yerr=[lo, hi],
            label=f"{ds} / {algo}",
            color=colour_for_ds(str(ds)),
            linestyle=linestyle_for_algo(str(algo)),
            marker=marker_for_algo(str(algo)),
        )

    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.set_xlabel("tuples")
    ax.set_ylabel("time (ns)")
    ax.set_title("Scaling: time vs input size")
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
