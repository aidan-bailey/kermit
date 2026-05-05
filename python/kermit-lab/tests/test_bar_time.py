"""Bar-time plot: query filter + missing-axis errors."""
from __future__ import annotations

from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import pytest

from kermit_lab.loader import load_reports
from kermit_lab.plots import InsufficientAxesError
from kermit_lab.plots import bar_time


def test_bar_time_writes_file_for_known_query(fixture_tree, tmp_path: Path) -> None:
    out = tmp_path / "bar-time.pdf"
    reports = load_reports(fixture_tree["paths"])
    bar_time.render(reports, out, fixture_tree["criterion_root"], query="triangle")
    assert out.exists()
    assert out.stat().st_size > 0


def test_bar_time_raises_for_unknown_query(fixture_tree, tmp_path: Path) -> None:
    reports = load_reports(fixture_tree["paths"])
    with pytest.raises(InsufficientAxesError, match="no reports match"):
        bar_time.render(
            reports, tmp_path / "x.pdf", fixture_tree["criterion_root"], query="nonsense"
        )
