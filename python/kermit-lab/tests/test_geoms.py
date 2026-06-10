# tests/test_geoms.py
"""Geom drawers add artists to an Axes."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt

from kermit_lab.encoding import ResolvedSeries
from kermit_lab.geoms import draw_bar, draw_line


def _series(label, colour="#000000"):
    return ResolvedSeries(
        key=(label,), label=label, colour=colour, marker="o", linestyle="-",
        xs=["A", "B"], ys=[1.0, 2.0], lo=[0.1, 0.1], hi=[0.1, 0.1],
    )


def test_draw_bar_adds_patches() -> None:
    fig, ax = plt.subplots()
    draw_bar(ax, [_series("TreeTrie"), _series("ColumnTrie")], xlabel="ds", ylabel="ns")
    assert len(ax.patches) > 0
    assert [t.get_text() for t in ax.get_xticklabels()] == ["A", "B"]
    plt.close(fig)


def test_draw_line_adds_lines() -> None:
    fig, ax = plt.subplots()
    draw_line(ax, [_series("TreeTrie")], xlabel="tuples", ylabel="ns", logx=True)
    assert len(ax.lines) > 0
    assert ax.get_xscale() == "log"
    plt.close(fig)
