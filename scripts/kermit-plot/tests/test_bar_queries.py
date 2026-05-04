"""Bar-queries plot: filter to (DS, algo); raise when no match."""
from __future__ import annotations

from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import pytest

from kermit_plot.loader import load_reports
from kermit_plot.plots import InsufficientAxesError
from kermit_plot.plots import bar_queries


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
