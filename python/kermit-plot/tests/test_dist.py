"""Dist plot: violin + box across (DS, algo) groups."""
from __future__ import annotations

from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import pytest

from kermit_plot.loader import load_reports
from kermit_plot.plots import InsufficientAxesError
from kermit_plot.plots import dist


def test_dist_writes_file(fixture_tree, tmp_path: Path) -> None:
    out = tmp_path / "dist.pdf"
    reports = load_reports(fixture_tree["paths"])
    dist.render(reports, out, fixture_tree["criterion_root"])
    assert out.exists()
    assert out.stat().st_size > 0


def test_dist_raises_when_no_time_groups(fixture_tree, tmp_path: Path) -> None:
    reports = load_reports(fixture_tree["paths"])
    no_time = [r for r in reports if not r.has_metric("time")]
    with pytest.raises(InsufficientAxesError, match="≥1 time-metric"):
        dist.render(no_time, tmp_path / "x.pdf", fixture_tree["criterion_root"])
