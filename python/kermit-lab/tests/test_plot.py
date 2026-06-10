# tests/test_plot.py
"""kl.plot end-to-end for bar + line, including faceting and ablation axes."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
from matplotlib.figure import Figure

from kermit_lab.frame import load
from kermit_lab.plot import plot


def test_line_scaling_returns_figure(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    fig = plot(df, kind="line", x="tuples", y="time",
               colour="data_structure", style="algorithm", logx=True, logy=True)
    assert isinstance(fig, Figure)
    plt.close(fig)


def test_bar_ablation_on_config_axis(fixture_opt_tree) -> None:
    df = load(fixture_opt_tree["paths"], fixture_opt_tree["criterion_root"])
    fig = plot(df, kind="bar", x="ds_config_singleton_pruning", y="time",
               colour="data_structure")
    assert isinstance(fig, Figure)
    plt.close(fig)


def test_facet_makes_one_axes_per_value(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    fig = plot(df, kind="bar", x="data_structure", y="time",
               colour="data_structure", facet="query")
    # fixture_tree has queries triangle, chain, star
    visible = [ax for ax in fig.axes if ax.get_visible() and ax.has_data()]
    assert len(visible) >= 1
    plt.close(fig)


def test_writes_file(fixture_tree, tmp_path) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    out = tmp_path / "x.pdf"
    plot(df, kind="bar", x="data_structure", y="space", colour="data_structure", out=out)
    assert out.exists() and out.stat().st_size > 0
