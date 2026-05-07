"""Bar-queries plot: filter to (DS, algo); raise when no match.

Single-pair mode (``ds`` / ``algo`` as strings) renders one bar per matched
row with error bars. Multi-pair mode (either or both as iterables of
strings) stacks segments per ``(ds, algo)`` at each x-position and emits a
legend instead of error bars.
"""
from __future__ import annotations

from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import pytest

import kermit_lab as kl
from kermit_lab.loader import load_reports
from kermit_lab.plots import InsufficientAxesError
from kermit_lab.plots import bar_queries


def test_bar_queries_writes_file_for_known_pair(fixture_tree, tmp_path: Path) -> None:
    out = tmp_path / "bar-queries.pdf"
    reports = load_reports(fixture_tree["paths"])
    bar_queries.render(
        reports,
        out,
        fixture_tree["criterion_root"],
        ds="TreeTrie",
        algo="LeapfrogTriejoin",
    )
    assert out.exists()
    assert out.stat().st_size > 0


def test_bar_queries_raises_for_unknown_pair(fixture_tree, tmp_path: Path) -> None:
    reports = load_reports(fixture_tree["paths"])
    with pytest.raises(InsufficientAxesError, match="no reports match"):
        bar_queries.render(
            reports,
            tmp_path / "x.pdf",
            fixture_tree["criterion_root"],
            ds="MysteryStruct",
            algo="LeapfrogTriejoin",
        )


def test_bar_queries_stacks_when_multiple_ds_selected(fixture_tree) -> None:
    df = kl.load(fixture_tree["paths"], fixture_tree["criterion_root"])
    fig = kl.bar_queries(
        df,
        ds=["TreeTrie", "ColumnTrie"],
        algo="LeapfrogTriejoin",
    )
    ax = fig.axes[0]

    # Three distinct queries (chain, star, triangle) ⇒ exactly three ticks,
    # one centered over each query's run of bars (triangle has 3 sub-bars
    # for sizes 10/100/1000).
    assert [str(label.get_text()) for label in ax.get_xticklabels()] == [
        "chain",
        "star",
        "triangle",
    ]

    # Two BarContainer artists are emitted — one per (ds, algo) pair drawn.
    # Each carries 5 patches: 1 chain + 1 star + 3 triangle sub-bars.
    from matplotlib.container import BarContainer

    bar_containers = [c for c in ax.containers if isinstance(c, BarContainer)]
    assert len(bar_containers) == 2
    assert all(len(c.patches) == 5 for c in bar_containers)

    # The second container's bars are stacked on top of the first one's.
    first_tops = [p.get_y() + p.get_height() for p in bar_containers[0].patches]
    second_bottoms = [p.get_y() for p in bar_containers[1].patches]
    assert all(
        abs(top - bottom) < 1e-9
        for top, bottom in zip(first_tops, second_bottoms)
        if bottom > 0
    )

    # Legend appears in stacked mode (single-pair mode passes label=None).
    assert ax.get_legend() is not None


def test_bar_queries_clusters_same_query_bars_together(fixture_tree) -> None:
    """Triangle's three sub-bars (sizes 10/100/1000) sit at consecutive x positions
    with the within-group step (1.0) smaller than the between-group gap (1.5)."""
    df = kl.load(fixture_tree["paths"], fixture_tree["criterion_root"])
    fig = kl.bar_queries(df, ds=["TreeTrie", "ColumnTrie"], algo="LeapfrogTriejoin")
    ax = fig.axes[0]

    from matplotlib.container import BarContainer

    bar_xs = [
        p.get_x() + p.get_width() / 2
        for c in ax.containers
        if isinstance(c, BarContainer)
        for p in c.patches
    ][: 5]  # first container has all 5 unique x-positions

    # Bars are sorted: chain (1 bar), star (1 bar), triangle (3 bars).
    chain_x, star_x, tri10_x, tri100_x, tri1000_x = sorted(set(bar_xs))

    # Within-query step (triangle 10 → 100 → 1000): 1.0
    assert tri100_x - tri10_x == pytest.approx(1.0)
    assert tri1000_x - tri100_x == pytest.approx(1.0)

    # Between-query gap (chain → star, star → triangle): 1.5 (step + extra gap).
    assert star_x - chain_x == pytest.approx(1.5)
    assert tri10_x - star_x == pytest.approx(1.5)


def test_bar_queries_defaults_to_all_when_omitted(fixture_tree) -> None:
    """Omitting ``ds`` / ``algo`` selects every value present in the DataFrame."""
    df = kl.load(fixture_tree["paths"], fixture_tree["criterion_root"])

    fig_default = kl.bar_queries(df)
    fig_explicit = kl.bar_queries(
        df,
        ds=["ColumnTrie", "TreeTrie"],
        algo=["LeapfrogTriejoin"],
    )

    from matplotlib.container import BarContainer

    def _heights(fig):
        return sorted(
            p.get_height()
            for c in fig.axes[0].containers
            if isinstance(c, BarContainer)
            for p in c.patches
        )

    assert _heights(fig_default) == _heights(fig_explicit)


def test_bar_queries_single_element_list_matches_string(fixture_tree) -> None:
    from matplotlib.container import BarContainer

    def _bar_heights(fig):
        return sorted(
            p.get_height()
            for c in fig.axes[0].containers
            if isinstance(c, BarContainer)
            for p in c.patches
        )

    df = kl.load(fixture_tree["paths"], fixture_tree["criterion_root"])
    fig_str = kl.bar_queries(df, ds="TreeTrie", algo="LeapfrogTriejoin")
    fig_list = kl.bar_queries(df, ds=["TreeTrie"], algo=["LeapfrogTriejoin"])
    assert _bar_heights(fig_str) == _bar_heights(fig_list)

    # Single-pair mode (whether str or 1-element list) shows no legend.
    assert fig_list.axes[0].get_legend() is None
