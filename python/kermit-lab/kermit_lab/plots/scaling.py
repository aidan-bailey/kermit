"""Shape C — log-log scaling: time vs ``tuples`` for each ``(DS, algorithm)``.

Aggregates many ``BenchReport``s (one per ``(DS, algo, dataset)``) into
grouped lines. The ``tuples`` axis comes from each report; the y-axis is
Criterion's ``mean`` point estimate (with 95% CI as error bars).
"""
from __future__ import annotations

from collections import defaultdict
from pathlib import Path
from typing import Iterable

import matplotlib.pyplot as plt

from ..axis_mapping import colour_for_ds, linestyle_for_algo, marker_for_algo
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
    """Render a log-log scaling plot to ``out_path``.

    Groups reports by ``(data_structure, algorithm)`` and draws one line per
    group, x = ``axes["tuples"]``, y = ``mean.point`` of the time-metric
    function whose phase is ``phase`` (default: iteration — the join-execution
    phase the thesis cares about; pass ``"insertion"`` to plot construction
    cost instead).

    Raises :class:`InsufficientAxesError` if fewer than 2 distinct ``tuples``
    values are present across the input.
    """
    reports = list(reports)
    grouped: dict[tuple[str, str], list[tuple[float, float, float, float]]] = defaultdict(list)

    for report in reports:
        n = report.axis("tuples")
        ds = report.axis("data_structure", "?")
        algo = report.axis("algorithm", "?")
        if n is None:
            continue
        for group_ref in report.criterion_groups:
            if group_ref.metric != "time" or phase_of(group_ref.function) != phase:
                continue
            data = load_function(criterion_root, group_ref.group, group_ref.function)
            grouped[(ds, algo)].append(
                (float(n), data.mean.point, data.mean.lower, data.mean.upper)
            )

    distinct_tuples = {n for points in grouped.values() for (n, *_rest) in points}
    if len(distinct_tuples) < 2:
        raise InsufficientAxesError(
            f"scaling (phase={phase}) needs ≥2 distinct 'tuples' values; got "
            f"{sorted(distinct_tuples)}"
        )

    fig, ax = plt.subplots()
    for (ds, algo), points in sorted(grouped.items()):
        points.sort()
        xs = [p[0] for p in points]
        ys = [p[1] for p in points]
        # Clamp to ≥0: pathological CIs can fall outside the point estimate.
        # matplotlib silently flips a bar with a negative half-width.
        lo = [max(0.0, p[1] - p[2]) for p in points]
        hi = [max(0.0, p[3] - p[1]) for p in points]
        ax.errorbar(
            xs,
            ys,
            yerr=[lo, hi],
            label=f"{ds} / {algo}",
            color=colour_for_ds(ds),
            linestyle=linestyle_for_algo(algo),
            marker=marker_for_algo(algo),
        )

    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.set_xlabel("tuples")
    ax.set_ylabel("time (ns)")
    ax.set_title("Scaling: time vs input size")
    ax.legend(title="DS / algorithm", loc="best")
    fig.savefig(out_path)
    plt.close(fig)
