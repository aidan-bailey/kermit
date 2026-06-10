"""Named presets: each fills an AestheticMap and calls kl.plot.

These are the legacy shapes, re-expressed as configurations of the general
engine. They exist for ergonomics and for the CLI / render-all to call.
"""
from __future__ import annotations

from pathlib import Path
from typing import Optional, Sequence

import pandas as pd
from matplotlib.figure import Figure

from .plot import plot


def scaling(df: pd.DataFrame, *, phase: str = "iteration", out: Optional[Path] = None) -> Figure:
    return plot(df, kind="line", x="tuples", y="time", colour="data_structure",
                style="algorithm", phase=phase, logx=True, logy=True,
                title="Scaling", out=out)


def bar_time(
    df: pd.DataFrame, *, query: str, phase: str = "iteration", out: Optional[Path] = None
) -> Figure:
    return plot(df, kind="bar", x="data_structure", y="time", colour="data_structure",
                style="algorithm", filter={"query": query}, phase=phase,
                title=f"Time — {query}", out=out)


def bar_space(df: pd.DataFrame, *, out: Optional[Path] = None) -> Figure:
    return plot(df, kind="bar", x="data_structure", y="space", colour="data_structure",
                title="Space per data structure", out=out)


def tradeoff(df: pd.DataFrame, *, phase: str = "iteration", out: Optional[Path] = None) -> Figure:
    return plot(df, kind="scatter", x="space", y="time", colour="data_structure",
                style="algorithm", phase=phase, logx=True, title="Space vs time", out=out)


def dist(
    df: pd.DataFrame, *, samples: pd.DataFrame, phase: str = "iteration",
    out: Optional[Path] = None,
) -> Figure:
    return plot(df, kind="violin", x="data_structure", y="time", style="algorithm",
                phase=phase, samples=samples, title="Per-iter distribution", out=out)


def bar_queries(
    df: pd.DataFrame, *, ds: Optional[Sequence[str]] = None,
    algo: Optional[Sequence[str]] = None, phase: str = "iteration",
    out: Optional[Path] = None,
) -> Figure:
    flt: dict[str, object] = {}
    if ds is not None:
        flt["data_structure"] = list(ds)
    if algo is not None:
        flt["algorithm"] = list(algo)
    return plot(df, kind="bar", x="query", y="time", colour="data_structure",
                style="algorithm", filter=flt, phase=phase,
                title="Across queries", out=out)


def ablation(
    df: pd.DataFrame, *, axis: str, phase: str = "iteration", out: Optional[Path] = None
) -> Figure:
    """Ablation: time vs an optimization axis, coloured by DS, faceted by query when >1."""
    facet = "query" if "query" in df.columns and df["query"].nunique(dropna=True) > 1 else None
    return plot(df, kind="bar", x=axis, y="time", colour="data_structure",
                facet=facet, phase=phase, title=f"Ablation — {axis}", out=out)
