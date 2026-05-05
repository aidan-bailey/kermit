"""Tradeoff plot: needs both time and space metrics."""
from __future__ import annotations

from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import pytest

from kermit_plot.loader import load_reports
from kermit_plot.plots import InsufficientAxesError
from kermit_plot.plots import tradeoff


def test_tradeoff_writes_file(fixture_tree, tmp_path: Path) -> None:
    out = tmp_path / "tradeoff.pdf"
    reports = load_reports(fixture_tree["paths"])
    tradeoff.render(reports, out, fixture_tree["criterion_root"])
    assert out.exists()
    assert out.stat().st_size > 0


def test_tradeoff_raises_without_both_metrics(fixture_tree, tmp_path: Path) -> None:
    reports = load_reports(fixture_tree["paths"])
    only_time = [r for r in reports if not r.has_metric("space")]
    with pytest.raises(InsufficientAxesError, match="both space and time"):
        tradeoff.render(only_time, tmp_path / "x.pdf", fixture_tree["criterion_root"])
