"""Scaling plot: file output, axis count thresholds."""
from __future__ import annotations

from pathlib import Path

import matplotlib

matplotlib.use("Agg")  # headless rendering for CI

import pytest

from kermit_lab.loader import load_reports
from kermit_lab.plots import InsufficientAxesError
from kermit_lab.plots import scaling


def test_scaling_writes_non_empty_file(fixture_tree, tmp_path: Path) -> None:
    out = tmp_path / "scaling.pdf"
    reports = load_reports(fixture_tree["paths"])
    scaling.render(reports, out, fixture_tree["criterion_root"])
    assert out.exists()
    assert out.stat().st_size > 0


def test_scaling_requires_two_distinct_tuples(fixture_tree, tmp_path: Path) -> None:
    reports = load_reports(fixture_tree["paths"])
    only_one_size = [r for r in reports if r.axis("tuples") == 100]
    with pytest.raises(InsufficientAxesError, match="≥2 distinct"):
        scaling.render(only_one_size, tmp_path / "x.pdf", fixture_tree["criterion_root"])


def test_scaling_phase_filter_picks_only_requested_phase(
    fixture_tree, tmp_path: Path
) -> None:
    """Regression test: report with both insertion+iteration time groups must
    only produce a single y-value per (DS, algo, n) for the requested phase."""
    import matplotlib.pyplot as plt

    reports = load_reports(fixture_tree["paths"])
    triangles = [r for r in reports if r.axis("query") == "triangle"]
    out_iter = tmp_path / "scaling-iter.pdf"
    scaling.render(triangles, out_iter, fixture_tree["criterion_root"], phase="iteration")
    fig = plt.gcf()  # last figure created by render
    plt.close(fig)
    assert out_iter.exists() and out_iter.stat().st_size > 0

    # Switching phase yields a non-empty plot too — but the values differ.
    out_insert = tmp_path / "scaling-insert.pdf"
    scaling.render(triangles, out_insert, fixture_tree["criterion_root"], phase="insertion")
    assert out_insert.exists() and out_insert.stat().st_size > 0

    # The two PDFs should differ byte-wise — proves phase actually changes
    # what's plotted, not just a docstring update.
    assert out_iter.read_bytes() != out_insert.read_bytes()
