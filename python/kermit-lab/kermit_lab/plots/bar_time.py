"""Shape A — bar+CI of time across ``(DS, algorithm)`` for one query."""
from __future__ import annotations

from pathlib import Path
from typing import Iterable

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
from matplotlib.figure import Figure

from ..axis_mapping import colour_for_ds, marker_for_algo
from ..loader import BenchReport
from . import InsufficientAxesError


def plot(
    df: pd.DataFrame,
    *,
    query: str,
    phase: str = "iteration",
    out: Path | None = None,
) -> Figure:
    """Return a grouped-bar Figure of time per ``(DS, algorithm)`` for ``query``."""
    matched = df[df["query"] == query]
    if matched.empty:
        raise InsufficientAxesError(f"no reports match query={query!r}")
    sub = matched[(matched.metric == "time") & (matched.phase == phase)]
    if sub.empty:
        raise InsufficientAxesError(
            f"query={query!r} matched reports but none had a "
            f"time-metric phase={phase!r} Criterion group"
        )

    sub = sub.sort_values(["data_structure", "algorithm"]).reset_index(drop=True)
    means = sub.mean_ns.to_numpy()
    lo = np.clip(means - sub.mean_lo.to_numpy(), 0.0, None)
    hi = np.clip(sub.mean_hi.to_numpy() - means, 0.0, None)
    labels = [f"{ds}\n{algo}" for ds, algo in zip(sub.data_structure, sub.algorithm)]
    colours = [colour_for_ds(str(ds)) for ds in sub.data_structure]

    fig, ax = plt.subplots()
    x = np.arange(len(sub))
    ax.bar(x, means, yerr=[lo, hi], color=colours, edgecolor="black", linewidth=0.8)
    for i, (algo, mean) in enumerate(zip(sub.algorithm, means)):
        ax.scatter([i], [mean], marker=marker_for_algo(str(algo)), color="black", s=20, zorder=3)
    ax.set_xticks(x)
    ax.set_xticklabels(labels)
    ax.set_ylabel("time (ns)")
    ax.set_title(f"Time per (DS, algorithm) — query: {query}")
    if out is not None:
        fig.savefig(out)
    return fig


def render(
    reports: Iterable[BenchReport],
    out_path: Path,
    criterion_root: Path,
    *,
    query: str,
    phase: str = "iteration",
) -> None:
    """Legacy API shim — builds the DataFrame, calls :func:`plot`, closes the figure."""
    from ..frame import _summary_from_reports

    df = _summary_from_reports(list(reports), criterion_root)
    fig = plot(df, query=query, phase=phase, out=out_path)
    plt.close(fig)
