"""Shape A — bar+CI of time across ``(DS, algorithm)`` for one query."""
from __future__ import annotations

from pathlib import Path
from typing import Iterable

import matplotlib.pyplot as plt
import numpy as np

from ..axis_mapping import colour_for_ds, marker_for_algo
from ..criterion import load_function
from ..loader import BenchReport, phase_of
from . import InsufficientAxesError


def render(
    reports: Iterable[BenchReport],
    out_path: Path,
    criterion_root: Path,
    *,
    query: str,
    phase: str = "iteration",
) -> None:
    """Render a grouped bar chart of time per ``(DS, algorithm)`` for ``query``.

    Filters reports to those whose ``axes["query"]`` matches ``query`` and
    consumes only the time-metric Criterion group whose phase is ``phase``
    (default: iteration). Bars coloured by DS; markers encode algorithm.
    """
    reports = [r for r in reports if r.axis("query") == query]
    if not reports:
        raise InsufficientAxesError(f"no reports match query={query!r}")

    bars: list[tuple[str, str, float, float, float]] = []
    for report in reports:
        ds = report.axis("data_structure", "?")
        algo = report.axis("algorithm", "?")
        for group_ref in report.criterion_groups:
            if group_ref.metric != "time" or phase_of(group_ref.function) != phase:
                continue
            data = load_function(criterion_root, group_ref.group, group_ref.function)
            bars.append((ds, algo, data.mean.point, data.mean.lower, data.mean.upper))

    if not bars:
        raise InsufficientAxesError(
            f"query={query!r} matched reports but none had a "
            f"time-metric phase={phase!r} Criterion group"
        )

    bars.sort(key=lambda r: (r[0], r[1]))
    labels = [f"{ds}\n{algo}" for ds, algo, *_ in bars]
    means = np.array([b[2] for b in bars])
    # Clamp to ≥0: pathological CIs can fall outside the point estimate.
    lo = np.clip(means - np.array([b[3] for b in bars]), 0.0, None)
    hi = np.clip(np.array([b[4] for b in bars]) - means, 0.0, None)
    colours = [colour_for_ds(ds) for ds, *_ in bars]

    fig, ax = plt.subplots()
    x = np.arange(len(bars))
    ax.bar(x, means, yerr=[lo, hi], color=colours, edgecolor="black", linewidth=0.8)
    for i, (_ds, algo, mean, *_rest) in enumerate(bars):
        ax.scatter([i], [mean], marker=marker_for_algo(algo), color="black", s=20, zorder=3)
    ax.set_xticks(x)
    ax.set_xticklabels(labels)
    ax.set_ylabel("time (ns)")
    ax.set_title(f"Time per (DS, algorithm) — query: {query}")
    fig.savefig(out_path)
    plt.close(fig)
