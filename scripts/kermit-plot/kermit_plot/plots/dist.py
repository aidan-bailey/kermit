"""Shape E — violin / box of per-iter Criterion samples.

Each violin is a (DS, algorithm) group's per-iter durations
(``sample.json``'s ``times[i] / iters[i]``). Box overlay shows median +
quartiles for readability.
"""
from __future__ import annotations

from collections import defaultdict
from pathlib import Path
from typing import Iterable

import matplotlib.pyplot as plt

from ..axis_mapping import colour_for_ds
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
    """Render a violin+box plot of per-iter samples per ``(DS, algorithm)``.

    Consumes only the time-metric Criterion group whose phase matches
    ``phase`` (default: iteration); insertion samples come from a different
    distribution and would muddy the violin if mixed in.
    """
    samples: dict[tuple[str, str], list[float]] = defaultdict(list)
    for report in reports:
        ds = report.axis("data_structure", "?")
        algo = report.axis("algorithm", "?")
        for group_ref in report.criterion_groups:
            if group_ref.metric != "time" or phase_of(group_ref.function) != phase:
                continue
            data = load_function(criterion_root, group_ref.group, group_ref.function)
            samples[(ds, algo)].extend(data.per_iter_times)

    if not samples:
        raise InsufficientAxesError(
            f"dist needs ≥1 time-metric phase={phase!r} Criterion group"
        )

    keys = sorted(samples.keys())
    data_lists = [samples[k] for k in keys]
    labels = [f"{ds}\n{algo}" for ds, algo in keys]
    colours = [colour_for_ds(ds) for ds, _algo in keys]

    fig, ax = plt.subplots()
    parts = ax.violinplot(data_lists, showmeans=False, showmedians=False, showextrema=False)
    for body, c in zip(parts["bodies"], colours):
        body.set_facecolor(c)
        body.set_edgecolor("black")
        body.set_alpha(0.6)
    ax.boxplot(
        data_lists,
        widths=0.15,
        patch_artist=False,
        showfliers=False,
        medianprops={"color": "black", "linewidth": 1.2},
    )
    ax.set_xticks(range(1, len(keys) + 1))
    ax.set_xticklabels(labels)
    ax.set_ylabel("time per iter (ns)")
    ax.set_title("Per-iter time distribution")
    fig.savefig(out_path)
    plt.close(fig)
