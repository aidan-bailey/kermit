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


def draw_scatter(
    ax: Axes, series: list[ResolvedSeries], *, xlabel: str, ylabel: str,
    logx: bool = False, logy: bool = False,
) -> None:
    """Scatter plot; honours logx/logy."""
    for s in series:
        ax.scatter(s.xs, s.ys, color=s.colour, marker=s.marker,
                   edgecolor="black", label=s.label)
    if logx:
        ax.set_xscale("log")
    if logy:
        ax.set_yscale("log")
    ax.set_xlabel(xlabel)
    ax.set_ylabel(ylabel)


def draw_violin(ax: Axes, series: list[ResolvedSeries], *, xlabel: str, ylabel: str) -> None:
    """Violin plot; one violin per (series, x) group."""
    datasets: list[list[float]] = []
    labels: list[str] = []
    colours: list[str] = []
    for s in series:
        for x, samp in zip(s.xs, s.samples or []):
            datasets.append(samp)
            labels.append(f"{s.label}\n{x}" if s.label else str(x))
            colours.append(s.colour)
    if not datasets:
        # Empty series list: nothing to draw (violinplot requires ≥1 dataset).
        return
    positions = list(range(1, len(datasets) + 1))
    parts = ax.violinplot(datasets, positions=positions, showmedians=True)
    for body, colour in zip(parts["bodies"], colours):
        body.set_facecolor(colour)
        body.set_alpha(0.7)
    ax.set_xticks(positions)
    ax.set_xticklabels(labels)
    ax.set_xlabel(xlabel)
    ax.set_ylabel(ylabel)
