# kermit_lab/facet.py
"""Subplot-grid layout + shared, de-duplicated legend across facet cells."""
from __future__ import annotations

import math
from pathlib import Path
from typing import Optional

import matplotlib.pyplot as plt
from matplotlib.axes import Axes
from matplotlib.figure import Figure

_MAX_COLS = 3


def make_grid(n: int) -> tuple[Figure, list[Axes]]:
    """Return a figure + exactly ``n`` visible axes (extra cells hidden)."""
    n = max(n, 1)
    ncols = min(n, _MAX_COLS)
    nrows = math.ceil(n / ncols)
    fig, axes = plt.subplots(nrows, ncols, squeeze=False, figsize=(5 * ncols, 4 * nrows))
    flat = [ax for row in axes for ax in row]
    for ax in flat[n:]:
        ax.set_visible(False)
    return fig, flat[:n]


def shared_legend(fig: Figure, axes: list[Axes]) -> None:
    """Collect handles across all axes, dedupe by label, draw one figure legend."""
    seen: dict[str, object] = {}
    for ax in axes:
        for handle, label in zip(*ax.get_legend_handles_labels()):
            seen.setdefault(label, handle)
    if seen:
        fig.legend(
            list(seen.values()), list(seen.keys()),
            loc="upper center", ncol=min(len(seen), 4),
        )


def finish(
    fig: Figure, axes: list[Axes], *, title: Optional[str], out: Optional[Path]
) -> None:
    """Apply shared legend, optional suptitle, tight layout, optional save."""
    shared_legend(fig, axes)
    if title:
        fig.suptitle(title)
    fig.tight_layout()
    if out is not None:
        fig.savefig(out)
