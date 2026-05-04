"""Shape F — bar across queries for a fixed ``(DS, algorithm)``.

Useful for showing how one (DS, algo) handles different workload shapes.
"""
from __future__ import annotations

from pathlib import Path
from typing import Iterable

import matplotlib.pyplot as plt
import numpy as np

from ..axis_mapping import colour_for_ds
from ..criterion import load_function
from ..loader import BenchReport, phase_of
from . import InsufficientAxesError


def render(
    reports: Iterable[BenchReport],
    out_path: Path,
    criterion_root: Path,
    *,
    ds: str,
    algo: str,
    phase: str = "iteration",
) -> None:
    """Render bars across ``query`` for the chosen ``(ds, algo)``.

    Consumes only the time-metric Criterion group whose phase matches
    ``phase`` (default: iteration).
    """
    matched = [
        r
        for r in reports
        if r.axis("data_structure") == ds and r.axis("algorithm") == algo
    ]
    if not matched:
        raise InsufficientAxesError(
            f"no reports match data_structure={ds!r} algorithm={algo!r}"
        )

    bars: list[tuple[str, float, float, float]] = []
    for report in matched:
        query = report.axis("query")
        if query is None:
            continue
        for group_ref in report.criterion_groups:
            if group_ref.metric != "time" or phase_of(group_ref.function) != phase:
                continue
            data = load_function(criterion_root, group_ref.group, group_ref.function)
            bars.append((str(query), data.mean.point, data.mean.lower, data.mean.upper))

    if not bars:
        raise InsufficientAxesError(
            f"({ds}, {algo}) matched but no time-metric phase={phase!r} "
            "Criterion groups carry a 'query' axis"
        )

    bars.sort()
    labels = [b[0] for b in bars]
    means = np.array([b[1] for b in bars])
    # Clamp to ≥0: pathological CIs can fall outside the point estimate.
    lo = np.clip(means - np.array([b[2] for b in bars]), 0.0, None)
    hi = np.clip(np.array([b[3] for b in bars]) - means, 0.0, None)
    colour = colour_for_ds(ds)

    fig, ax = plt.subplots()
    x = np.arange(len(bars))
    ax.bar(x, means, yerr=[lo, hi], color=colour, edgecolor="black", linewidth=0.8)
    ax.set_xticks(x)
    ax.set_xticklabels(labels, rotation=30, ha="right")
    ax.set_ylabel("time (ns)")
    ax.set_title(f"Query times — {ds} / {algo}")
    fig.savefig(out_path)
    plt.close(fig)
