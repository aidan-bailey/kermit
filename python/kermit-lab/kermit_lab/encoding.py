# kermit_lab/encoding.py
"""Aesthetic map: bind visual channels to DataFrame columns, and resolve a
filtered/grouped frame into drawable series.

Y-values and CIs come straight from Criterion's precomputed estimates
(``mean_ns`` / ``mean_lo`` / ``mean_hi``); this layer performs no statistics.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Mapping, Optional, Sequence

import pandas as pd

from .axis_mapping import WONG_PALETTE, colour_for, linestyle_for, marker_for
from .plots_errors import InsufficientAxesError


@dataclass(frozen=True)
class AestheticMap:
    """Binding of visual channels to columns. ``y`` selects the metric."""

    kind: str  # "bar" | "line" | "scatter" | "violin"
    x: Optional[str]
    y: str = "time"  # "time" | "space"
    colour: Optional[str] = None
    style: Optional[str] = None
    facet: Optional[str] = None
    filter: Mapping[str, object] = field(default_factory=dict)
    phase: str = "iteration"


@dataclass
class ResolvedSeries:
    """One drawable series (a colour×style group), positioned by x."""

    key: tuple
    label: str
    colour: str
    marker: str
    linestyle: str
    xs: list
    ys: list
    lo: list
    hi: list
    samples: Optional[list[list[float]]] = None


def metric_of(y: str) -> str:
    """Validate ``y`` names a known metric ("time" or "space") and return it."""
    if y not in ("time", "space"):
        raise InsufficientAxesError(f"y must be 'time' or 'space', got {y!r}")
    return y


def apply_filter(df: pd.DataFrame, amap: AestheticMap) -> pd.DataFrame:
    out = df
    for col, val in amap.filter.items():
        if col not in out.columns:
            raise InsufficientAxesError(f"filter column {col!r} not in frame")
        if isinstance(val, (list, tuple, set)):
            out = out[out[col].isin(list(val))]
        else:
            out = out[out[col] == val]
    return out


def select(df: pd.DataFrame, amap: AestheticMap) -> pd.DataFrame:
    """Filter to the metric/phase rows this map needs; raise if empty."""
    metric = metric_of(amap.y)
    sub = apply_filter(df, amap)
    sub = sub[sub.metric == metric]
    if metric == "time":
        sub = sub[sub.phase == amap.phase]
    if sub.empty:
        raise InsufficientAxesError(
            f"no {metric} rows (phase={amap.phase}) after filter {dict(amap.filter)}"
        )
    return sub


def facet_values(df: pd.DataFrame, amap: AestheticMap) -> list:
    if amap.facet is None:
        return [None]
    if amap.facet not in df.columns:
        raise InsufficientAxesError(f"facet column {amap.facet!r} not in frame")
    return sorted(df[amap.facet].dropna().unique().tolist(), key=str)


def _group_cols(amap: AestheticMap) -> list[str]:
    return list(dict.fromkeys(c for c in (amap.colour, amap.style) if c))


def _make_series(
    keyt: tuple,
    group_cols: Sequence[str],
    xs: list,
    ys: list,
    lo: list,
    hi: list,
    amap: AestheticMap,
    samples: Optional[list[list[float]]] = None,
) -> ResolvedSeries:
    kv = dict(zip(group_cols, keyt))
    colour = colour_for(amap.colour, kv[amap.colour]) if amap.colour else WONG_PALETTE[0]
    marker = marker_for(amap.style, kv[amap.style]) if amap.style else "o"
    linestyle = linestyle_for(amap.style, kv[amap.style]) if amap.style else "-"
    label = " / ".join(str(kv[c]) for c in group_cols) if group_cols else amap.y
    return ResolvedSeries(keyt, label, colour, marker, linestyle, xs, ys, lo, hi, samples)


def _grouped_items(df: pd.DataFrame, group_cols: Sequence[str]):
    if group_cols:
        for key, g in df.groupby(list(group_cols), dropna=False):
            yield (key if isinstance(key, tuple) else (key,)), g
    else:
        yield (), df


def resolve_scalar(cell: pd.DataFrame, amap: AestheticMap) -> list[ResolvedSeries]:
    """Bar/line/scatter series from precomputed means + CIs, grouped by colour×style."""
    if amap.x is None or amap.x not in cell.columns:
        raise InsufficientAxesError(f"x column {amap.x!r} not in frame")
    group_cols = _group_cols(amap)
    series: list[ResolvedSeries] = []
    for keyt, g in _grouped_items(cell, group_cols):
        g = g.sort_values(amap.x)
        xs = g[amap.x].tolist()
        # mean_ns/lo/hi hold the metric's point estimate + CI regardless of
        # metric (ns for time, bytes for space).
        ys = g["mean_ns"].tolist()
        lo = (g["mean_ns"] - g["mean_lo"]).clip(lower=0).tolist()
        hi = (g["mean_hi"] - g["mean_ns"]).clip(lower=0).tolist()
        series.append(_make_series(keyt, group_cols, xs, ys, lo, hi, amap))
    return series
