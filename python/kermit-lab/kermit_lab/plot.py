"""kl.plot — the general entry point. Maps channels to columns via AestheticMap,
facets, dispatches to a geom drawer, applies thesis style, optionally saves.
"""
from __future__ import annotations

from pathlib import Path
from typing import Mapping, Optional

import matplotlib.pyplot as plt
import pandas as pd
from matplotlib.figure import Figure

from .analysis import KERMIT_SYSTEM, cell_labels
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


def _default_colour(df: pd.DataFrame) -> tuple[pd.DataFrame, Optional[str]]:
    """The frame and colour column ``plot`` uses when the caller names no colour.

    A frame holding a non-kermit row (a ``system`` other than ``kermit``; a missing value
    counts as kermit, as the back-fill does) gets a ``cell`` column of
    :func:`~kermit_lab.analysis.cell_labels`, on a copy, and is coloured by it. Any other frame
    comes back as it is, uncoloured.
    """
    system = df["system"] if "system" in df.columns else pd.Series(dtype="object")
    if not (system.notna() & (system != KERMIT_SYSTEM)).any():
        return df, None
    return df.assign(cell=cell_labels(df)), "cell"


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
    estimator: str = "criterion",
    samples: Optional[pd.DataFrame] = None,
    logx: bool = False,
    logy: bool = False,
    title: Optional[str] = None,
    out: Optional[Path] = None,
) -> Figure:
    """Render one figure. ``kind`` ∈ {bar, line, scatter, violin}. Time rows
    are Criterion estimates unless ``estimator="single"`` asks for
    ``bench probe``'s single runs.

    ``colour`` left as ``None`` colours by the cell label
    (:func:`~kermit_lab.analysis.cell_labels`: ``system`` for an engine,
    ``data_structure/algorithm`` for a kermit cell) when ``df`` holds a row of an external
    engine, so engines and kermit's cells get one colour each; a frame of kermit rows alone
    stays uncoloured. This holds for the tradeoff scatter too. An explicit ``colour`` always
    wins. ``facet`` has no default."""
    if colour is None:
        df, colour = _default_colour(df)
    amap = AestheticMap(
        kind=kind, x=x, y=y, colour=colour, style=style, facet=facet,
        filter=dict(filter or {}), phase=phase, estimator=estimator,
    )
    apply_style()

    # Tradeoff: both axes are distinct metrics, single panel, no facet/phase-select.
    if kind == "scatter" and x in ("time", "space") and y in ("time", "space") and x != y:
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
