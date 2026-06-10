# kermit_lab/plot.py
"""kl.plot — the general entry point. Maps channels to columns via AestheticMap,
facets, dispatches to a geom drawer, applies thesis style, optionally saves.
"""
from __future__ import annotations

from pathlib import Path
from typing import Mapping, Optional

import pandas as pd
from matplotlib.figure import Figure

from .encoding import (
    AestheticMap,
    apply_filter,
    facet_values,
    resolve_scalar,
    resolve_tradeoff,
    resolve_violin,
    select,
)
from .facet import finish, make_grid
from .geoms import draw_bar, draw_line, draw_scatter, draw_violin
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
    """Render one figure. ``kind`` ∈ {bar, line, scatter, violin}."""
    amap = AestheticMap(
        kind=kind, x=x, y=y, colour=colour, style=style, facet=facet,
        filter=dict(filter or {}), phase=phase,
    )
    apply_style()

    # Tradeoff: both axes are metrics, single panel, no facet/phase-select.
    if kind == "scatter" and x in ("time", "space"):
        import matplotlib.pyplot as plt

        series = resolve_tradeoff(apply_filter(df, amap), amap)
        fig, ax = plt.subplots()
        draw_scatter(ax, series, xlabel=str(x), ylabel=y, logx=logx, logy=logy)
        finish(fig, [ax], title=title, out=out)
        return fig

    sub = select(df, amap)
    fvals = facet_values(sub, amap)
    fig, axes = make_grid(len(fvals))
    for ax, fv in zip(axes, fvals):
        cell = sub if fv is None else sub[sub[amap.facet] == fv]
        if kind == "violin":
            draw_violin(ax, resolve_violin(cell, samples, amap),
                        xlabel=str(amap.x), ylabel=amap.y)
        else:
            series = resolve_scalar(cell, amap)
            if kind == "bar":
                draw_bar(ax, series, xlabel=str(amap.x), ylabel=amap.y)
            elif kind == "line":
                draw_line(ax, series, xlabel=str(amap.x), ylabel=amap.y, logx=logx, logy=logy)
            elif kind == "scatter":
                draw_scatter(ax, series, xlabel=str(amap.x), ylabel=amap.y, logx=logx, logy=logy)
            else:
                raise InsufficientAxesError(f"unknown kind {kind!r}")
        if fv is not None:
            ax.set_title(f"{amap.facet}={fv}")
    finish(fig, axes, title=title, out=out)
    return fig
