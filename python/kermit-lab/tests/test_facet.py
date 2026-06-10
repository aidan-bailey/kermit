# tests/test_facet.py
"""Facet grid sizing and shared-legend dedup."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt

from kermit_lab.facet import make_grid, shared_legend


def test_make_grid_returns_n_visible_axes() -> None:
    fig, axes = make_grid(4)
    assert len(axes) == 4
    assert all(ax.get_visible() for ax in axes)
    plt.close(fig)


def test_shared_legend_dedupes_labels() -> None:
    fig, axes = make_grid(2)
    for ax in axes:
        ax.plot([0, 1], [0, 1], label="TreeTrie")
    shared_legend(fig, axes)
    legend = fig.legends[0]
    assert [t.get_text() for t in legend.get_texts()] == ["TreeTrie"]
    plt.close(fig)
