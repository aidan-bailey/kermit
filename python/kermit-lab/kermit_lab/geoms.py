# kermit_lab/geoms.py
"""Dumb geom drawers. Each consumes a list[ResolvedSeries] and draws onto an
Axes — no statistics, no data access. Series carry precomputed y + CI.
"""
from __future__ import annotations

import numpy as np
from matplotlib.axes import Axes

from .encoding import ResolvedSeries


def _categories(series: list[ResolvedSeries]) -> list:
    cats: list = []
    for s in series:
        for x in s.xs:
            if x not in cats:
                cats.append(x)
    return sorted(cats, key=str)


def draw_bar(ax: Axes, series: list[ResolvedSeries], *, xlabel: str, ylabel: str) -> None:
    """Grouped bars: one cluster per x category, one bar per series."""
    cats = _categories(series)
    pos = {c: i for i, c in enumerate(cats)}
    base = np.arange(len(cats))
    n = max(len(series), 1)
    width = 0.8 / n
    for i, s in enumerate(series):
        xpos = [base[pos[x]] + (i - (n - 1) / 2) * width for x in s.xs]
        ax.bar(
            xpos, s.ys, width=width, yerr=[s.lo, s.hi], capsize=2,
            color=s.colour, edgecolor="black", linewidth=0.8, label=s.label,
        )
    ax.set_xticks(base)
    ax.set_xticklabels([str(c) for c in cats])
    ax.set_xlabel(xlabel)
    ax.set_ylabel(ylabel)


def draw_line(
    ax: Axes, series: list[ResolvedSeries], *, xlabel: str, ylabel: str,
    logx: bool = False, logy: bool = False,
) -> None:
    """Line plot with error bars; honours logx/logy."""
    for s in series:
        ax.errorbar(
            s.xs, s.ys, yerr=[s.lo, s.hi], capsize=2,
            color=s.colour, marker=s.marker, linestyle=s.linestyle, label=s.label,
        )
    if logx:
        ax.set_xscale("log")
    if logy:
        ax.set_yscale("log")
    ax.set_xlabel(xlabel)
    ax.set_ylabel(ylabel)
