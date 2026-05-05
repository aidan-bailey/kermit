"""Bar-space plot: file emit + missing-metric errors."""
from __future__ import annotations

from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import pytest

from kermit_lab.loader import load_reports
from kermit_lab.plots import InsufficientAxesError
from kermit_lab.plots import bar_space


def test_bar_space_writes_file(fixture_tree, tmp_path: Path) -> None:
    out = tmp_path / "bar-space.pdf"
    reports = load_reports(fixture_tree["paths"])
    bar_space.render(reports, out, fixture_tree["criterion_root"])
    assert out.exists()
    assert out.stat().st_size > 0


def test_bar_space_raises_when_no_space_groups(fixture_tree, tmp_path: Path) -> None:
    reports = load_reports(fixture_tree["paths"])
    no_space = [
        r
        for r in reports
        if not r.has_metric("space")
    ]
    with pytest.raises(InsufficientAxesError, match="≥1 space-metric"):
        bar_space.render(no_space, tmp_path / "x.pdf", fixture_tree["criterion_root"])
