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
import pandas as pd
from matplotlib.figure import Figure

from ..axis_mapping import colour_for_ds
from ..loader import BenchReport
from . import InsufficientAxesError


def plot(df: pd.DataFrame, *, out: Path | None = None) -> Figure:
    """Return one bar per ``data_structure`` showing space (mean of bytes)."""
    sub = df[df.metric == "space"]
    if sub.empty:
        raise InsufficientAxesError("bar-space needs ≥1 space-metric row")

    sub = sub.sort_values("data_structure").reset_index(drop=True)
    means = sub.mean_ns.to_numpy()
    lo = np.clip(means - sub.mean_lo.to_numpy(), 0.0, None)
    hi = np.clip(sub.mean_hi.to_numpy() - means, 0.0, None)
    labels = sub.data_structure.tolist()
    colours = [colour_for_ds(str(ds)) for ds in labels]

    fig, ax = plt.subplots()
    x = np.arange(len(sub))
    ax.bar(x, means, yerr=[lo, hi], color=colours, edgecolor="black", linewidth=0.8)
    ax.set_xticks(x)
    ax.set_xticklabels(labels)
    ax.set_ylabel("heap (bytes)")
    ax.set_title("Space per data structure")
    if out is not None:
        fig.savefig(out)
    return fig


def render(
    reports: Iterable[BenchReport],
    out_path: Path,
    criterion_root: Path,
) -> None:
    """Legacy API shim — builds the DataFrame, calls :func:`plot`, closes the figure."""
    from ..frame import _summary_from_reports

    df = _summary_from_reports(list(reports), criterion_root)
    fig = plot(df, out=out_path)
    plt.close(fig)
