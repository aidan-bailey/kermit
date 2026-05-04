"""Shape B — bar of ``heap_size_bytes()`` across data structures.

Space measurements are deterministic by construction (Criterion's
``SpaceMeasurement`` returns the same byte count every iter), so the error
bars are width-zero. Showing them anyway is the convention; reviewers
otherwise ask "where are the CIs?".
"""
from __future__ import annotations

from pathlib import Path
from typing import Iterable

import matplotlib.pyplot as plt
import numpy as np

from ..axis_mapping import colour_for_ds
from ..criterion import load_function
from ..loader import BenchReport
from . import InsufficientAxesError


def render(
    reports: Iterable[BenchReport],
    out_path: Path,
    criterion_root: Path,
) -> None:
    """Render one bar per ``data_structure`` showing space (mean of bytes).

    Reports without space-metric criterion groups are skipped. Raises
    :class:`InsufficientAxesError` if no reports remain.
    """
    bars: list[tuple[str, float, float, float]] = []
    for report in reports:
        ds = report.axis("data_structure", "?")
        for group_ref in report.criterion_groups:
            if group_ref.metric != "space":
                continue
            data = load_function(criterion_root, group_ref.group, group_ref.function)
            bars.append((ds, data.mean.point, data.mean.lower, data.mean.upper))

    if not bars:
        raise InsufficientAxesError("bar-space needs ≥1 space-metric Criterion group")

    bars.sort()
    labels = [b[0] for b in bars]
    means = np.array([b[1] for b in bars])
    # Clamp to ≥0: width-zero CIs are expected (deterministic), but never negative.
    lo = np.clip(means - np.array([b[2] for b in bars]), 0.0, None)
    hi = np.clip(np.array([b[3] for b in bars]) - means, 0.0, None)
    colours = [colour_for_ds(b[0]) for b in bars]

    fig, ax = plt.subplots()
    x = np.arange(len(bars))
    ax.bar(x, means, yerr=[lo, hi], color=colours, edgecolor="black", linewidth=0.8)
    ax.set_xticks(x)
    ax.set_xticklabels(labels)
    ax.set_ylabel("heap (bytes)")
    ax.set_title("Space per data structure")
    fig.savefig(out_path)
    plt.close(fig)
