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
    fig = plot(df, kind="bar", x="ds_config_load_factor", y="time",
               colour="data_structure")
    assert isinstance(fig, Figure)
    plt.close(fig)


def test_line_end_to_end_faceted_by_queries_per_build(fixture_end_to_end_tree) -> None:
    # The acceptance shape for the end-to-end metric: T(scale, k) curves,
    # one facet cell per K.
    df = load(fixture_end_to_end_tree["paths"], fixture_end_to_end_tree["criterion_root"])
    fig = plot(df, kind="line", x="tuples", y="time",
               colour="data_structure", facet="queries_per_build", phase="end_to_end")
    assert isinstance(fig, Figure)
    # Fixture has K ∈ {1, 4} → exactly two populated facet cells.
    visible = [ax for ax in fig.axes if ax.get_visible() and ax.has_data()]
    assert len(visible) == 2
    plt.close(fig)


def test_facet_makes_one_axes_per_value(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    fig = plot(df, kind="bar", x="data_structure", y="time",
               colour="data_structure", facet="query")
    # fixture_tree has exactly 3 query values: triangle, chain, star — one
    # facet cell per value, each with data, proving faceting happened.
    visible = [ax for ax in fig.axes if ax.get_visible() and ax.has_data()]
    assert len(visible) == 3
    plt.close(fig)


def test_writes_file(fixture_tree, tmp_path) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    out = tmp_path / "x.pdf"
    plot(df, kind="bar", x="data_structure", y="space", colour="data_structure", out=out)
    assert out.exists() and out.stat().st_size > 0


def test_tradeoff_scatter(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    fig = plot(df, kind="scatter", x="space", y="time",
               colour="data_structure", style="algorithm", logx=True)
    assert isinstance(fig, Figure)
    assert any(len(ax.collections) > 0 for ax in fig.axes)
    plt.close(fig)


def test_violin_dist(fixture_tree) -> None:
    from kermit_lab.frame import load_samples
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    samples = load_samples(fixture_tree["paths"], fixture_tree["criterion_root"])
    fig = plot(df, kind="violin", x="data_structure", y="time",
               style="algorithm", samples=samples)
    assert isinstance(fig, Figure)
    # Violin bodies render as PolyCollections — proves a violin actually drew.
    assert any(len(ax.collections) > 0 for ax in fig.axes)
    plt.close(fig)
