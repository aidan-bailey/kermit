"""Shape D — space vs time scatter (one point per ``(DS, algorithm)`` pair).

Default axes: log-x (space, bytes), linear-y (time, ns). Each report
contributes one point if it has both a time- and a space-metric Criterion
group; reports with only one metric are skipped.
"""
from __future__ import annotations

from collections import defaultdict
from pathlib import Path
from typing import Iterable

import matplotlib.pyplot as plt

from ..axis_mapping import colour_for_ds, marker_for_algo
from ..criterion import load_function
from ..loader import BenchReport, phase_of
from . import InsufficientAxesError


def render(
    reports: Iterable[BenchReport],
    out_path: Path,
    criterion_root: Path,
    *,
    phase: str = "iteration",
) -> None:
    """Render a space-vs-time scatter plot to ``out_path``.

    Time axis uses the phase ``phase`` (default: iteration); space axis is
    averaged across all space-metric groups in the report (one per relation
    for `bench run`, exactly one for `bench ds`).
    """
    points: dict[tuple[str, str], list[tuple[float, float]]] = defaultdict(list)

    for report in reports:
        ds = report.axis("data_structure", "?")
        algo = report.axis("algorithm", "?")
        time_means: list[float] = []
        space_means: list[float] = []
        for group_ref in report.criterion_groups:
            if group_ref.metric == "time" and phase_of(group_ref.function) != phase:
                continue
            data = load_function(criterion_root, group_ref.group, group_ref.function)
            if group_ref.metric == "time":
                time_means.append(data.mean.point)
            elif group_ref.metric == "space":
                space_means.append(data.mean.point)
        if time_means and space_means:
            points[(ds, algo)].append(
                (sum(space_means) / len(space_means), sum(time_means) / len(time_means))
            )

    if not points:
        raise InsufficientAxesError(
            f"tradeoff needs ≥1 report with both space and time phase={phase!r} groups"
        )

    fig, ax = plt.subplots()
    for (ds, algo), pairs in sorted(points.items()):
        xs = [p[0] for p in pairs]
        ys = [p[1] for p in pairs]
        ax.scatter(
            xs,
            ys,
            label=f"{ds} / {algo}",
            color=colour_for_ds(ds),
            marker=marker_for_algo(algo),
            s=40,
            edgecolor="black",
            linewidth=0.5,
        )

    ax.set_xscale("log")
    ax.set_xlabel("space (bytes)")
    ax.set_ylabel("time (ns)")
    ax.set_title("Space–time tradeoff")
    ax.legend(title="DS / algorithm", loc="best")
    fig.savefig(out_path)
    plt.close(fig)
