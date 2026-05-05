"""Shape E — violin / box of per-iter Criterion samples.

Each violin is a (DS, algorithm) group's per-iter durations
(``sample.json``'s ``times[i] / iters[i]``). Box overlay shows median +
quartiles for readability.
"""
from __future__ import annotations

from pathlib import Path
from typing import Iterable

import matplotlib.pyplot as plt
import pandas as pd
from matplotlib.figure import Figure

from ..axis_mapping import colour_for_ds
from ..loader import BenchReport
from . import InsufficientAxesError


def plot(
    df: pd.DataFrame,
    *,
    phase: str = "iteration",
    df_samples: pd.DataFrame | None = None,
    criterion_root: Path | str = "target/criterion",
    out: Path | None = None,
) -> Figure:
    """Return a violin+box Figure of per-iter samples per ``(DS, algorithm)``.

    Loads samples via :func:`kermit_lab.load_samples` if ``df_samples`` is
    None. Pass an existing samples frame to avoid the second disk scan.
    """
    sub = df[(df.metric == "time") & (df.phase == phase)]
    if sub.empty:
        raise InsufficientAxesError(
            f"dist needs ≥1 time-metric phase={phase!r} row"
        )

    samples_df: pd.DataFrame
    if df_samples is None:
        from ..frame import load_samples

        samples_df = load_samples(sub.source_path.unique().tolist(), criterion_root)
    else:
        samples_df = df_samples

    merged = samples_df.merge(
        sub[["criterion_group", "criterion_function", "data_structure", "algorithm"]],
        on=["criterion_group", "criterion_function"],
    )
    if merged.empty:
        raise InsufficientAxesError("samples DataFrame has no overlap with summary rows")

    grouped = merged.groupby(["data_structure", "algorithm"], dropna=False)
    keys = sorted(grouped.groups.keys())
    data_lists = [grouped.get_group(k).per_iter_ns.tolist() for k in keys]
    labels = [f"{ds}\n{algo}" for ds, algo in keys]
    colours = [colour_for_ds(str(ds)) for ds, _algo in keys]

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
    fig = plot(df, phase=phase, criterion_root=criterion_root, out=out_path)
    plt.close(fig)
