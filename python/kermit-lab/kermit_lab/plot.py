# kermit_lab/plot.py
"""kl.plot — the general entry point. Maps channels to columns via AestheticMap,
facets, dispatches to a geom drawer, applies thesis style, optionally saves.
"""
from __future__ import annotations

from pathlib import Path
from typing import Mapping, Optional

import pandas as pd
from matplotlib.figure import Figure

from .encoding import AestheticMap, facet_values, resolve_scalar, select
from .facet import finish, make_grid
from .geoms import draw_bar, draw_line
from .plots_errors import InsufficientAxesError
from .styles import apply as apply_style


def plot(
    df: pd.DataFrame,
    *,
    kind: str,
    x: Optional[str] = None,
    y: str = "time",
    colour: Optional[str] = None,
    style: Optional[str] = None,
    facet: Optional[str] = None,
    filter: Optional[Mapping[str, object]] = None,
    phase: str = "iteration",
    samples: Optional[pd.DataFrame] = None,
    logx: bool = False,
    logy: bool = False,
    title: Optional[str] = None,
    out: Optional[Path] = None,
) -> Figure:
    """Render one figure. ``kind`` ∈ {bar, line} (scatter/violin added in Task 8)."""
    amap = AestheticMap(
        kind=kind, x=x, y=y, colour=colour, style=style, facet=facet,
        filter=dict(filter or {}), phase=phase,
    )
    apply_style()

    sub = select(df, amap)
    fvals = facet_values(sub, amap)
    fig, axes = make_grid(len(fvals))
    for ax, fv in zip(axes, fvals):
        cell = sub if fv is None else sub[sub[amap.facet] == fv]
        series = resolve_scalar(cell, amap)
        if kind == "bar":
            draw_bar(ax, series, xlabel=str(amap.x), ylabel=amap.y)
        elif kind == "line":
            draw_line(ax, series, xlabel=str(amap.x), ylabel=amap.y, logx=logx, logy=logy)
        else:
            raise InsufficientAxesError(f"kind {kind!r} not yet supported")
        if fv is not None:
            ax.set_title(f"{amap.facet}={fv}")
    finish(fig, axes, title=title, out=out)
    return fig
