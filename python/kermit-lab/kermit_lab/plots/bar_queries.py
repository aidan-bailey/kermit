"""Shape F — bar across queries for a fixed ``(DS, algorithm)``.

Useful for showing how one (DS, algo) handles different workload shapes.
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


def plot(
    df: pd.DataFrame,
    *,
    ds: str,
    algo: str,
    phase: str = "iteration",
    out: Path | None = None,
) -> Figure:
    """Return a Figure of bars across ``query`` for the chosen ``(ds, algo)``."""
    matched = df[(df.data_structure == ds) & (df.algorithm == algo)]
    if matched.empty:
        raise InsufficientAxesError(
            f"no reports match data_structure={ds!r} algorithm={algo!r}"
        )
    sub = matched[
        (matched.metric == "time") & (matched.phase == phase) & matched["query"].notna()
    ]
    if sub.empty:
        raise InsufficientAxesError(
            f"({ds}, {algo}) matched but no time-metric phase={phase!r} "
            "Criterion groups carry a 'query' axis"
        )

    sub = sub.sort_values("query").reset_index(drop=True)
    means = sub.mean_ns.to_numpy()
    lo = np.clip(means - sub.mean_lo.to_numpy(), 0.0, None)
    hi = np.clip(sub.mean_hi.to_numpy() - means, 0.0, None)
    labels = sub["query"].astype(str).tolist()
    colour = colour_for_ds(ds)

    fig, ax = plt.subplots()
    x = np.arange(len(sub))
    ax.bar(x, means, yerr=[lo, hi], color=colour, edgecolor="black", linewidth=0.8)
    ax.set_xticks(x)
    ax.set_xticklabels(labels, rotation=30, ha="right")
    ax.set_ylabel("time (ns)")
    ax.set_title(f"Query times — {ds} / {algo}")
    if out is not None:
        fig.savefig(out)
    return fig


def render(
    reports: Iterable[BenchReport],
    out_path: Path,
    criterion_root: Path,
    *,
    ds: str,
    algo: str,
    phase: str = "iteration",
) -> None:
    """Legacy API shim — builds the DataFrame, calls :func:`plot`, closes the figure."""
    from ..frame import _summary_from_reports

    df = _summary_from_reports(list(reports), criterion_root)
    fig = plot(df, ds=ds, algo=algo, phase=phase, out=out_path)
    plt.close(fig)
