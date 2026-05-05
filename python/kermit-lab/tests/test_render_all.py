"""Render-all: produces every applicable shape; skips others without raising."""
from __future__ import annotations

import logging
from pathlib import Path

import matplotlib

matplotlib.use("Agg")

from kermit_lab.drivers.render_all import render_all
from kermit_lab.loader import load_reports


def test_render_all_emits_all_shapes_for_full_fixture(
    fixture_tree, tmp_path: Path
) -> None:
    out_dir = tmp_path / "plots"
    out_dir.mkdir()
    reports = load_reports(fixture_tree["paths"])
    render_all(reports, out_dir, fixture_tree["criterion_root"], "pdf")
    files = sorted(p.name for p in out_dir.iterdir())
    assert "scaling.pdf" in files
    assert "bar-space.pdf" in files
    assert "tradeoff.pdf" in files
    assert "dist.pdf" in files
    # bar-time-<query> for each distinct query in the fixture (triangle, chain, star).
    assert "bar-time-triangle.pdf" in files
    # bar-queries-<ds>-<algo> for each pair (TreeTrie / LeapfrogTriejoin, ColumnTrie / LeapfrogTriejoin).
    assert "bar-queries-TreeTrie-LeapfrogTriejoin.pdf" in files
    assert "bar-queries-ColumnTrie-LeapfrogTriejoin.pdf" in files
    for f in out_dir.iterdir():
        assert f.stat().st_size > 0


def test_render_all_skips_inapplicable_shapes_without_raising(
    fixture_tree, tmp_path: Path, caplog
) -> None:
    out_dir = tmp_path / "plots"
    out_dir.mkdir()
    reports = load_reports(fixture_tree["paths"])
    only_one_size = [r for r in reports if r.axis("tuples") == 100]
    with caplog.at_level(logging.INFO, logger="kermit_lab.drivers.render_all"):
        render_all(only_one_size, out_dir, fixture_tree["criterion_root"], "pdf")
    files = {p.name for p in out_dir.iterdir()}
    # scaling needs ≥2 distinct tuples values — skipped.
    assert "scaling.pdf" not in files
    assert any("skipped scaling" in m for m in caplog.messages)
    # Everything that doesn't require scaling-of-tuples should still emit.
    assert "bar-space.pdf" in files
    assert "tradeoff.pdf" in files
    assert "dist.pdf" in files
    # Per-query bar-time: 'triangle' (TreeTrie + ColumnTrie) and 'chain'/'star'
    # (TreeTrie only). All three should appear from this slice.
    assert "bar-time-triangle.pdf" in files
    assert "bar-time-chain.pdf" in files
    assert "bar-time-star.pdf" in files
    # Per-pair bar-queries.
    assert "bar-queries-TreeTrie-LeapfrogTriejoin.pdf" in files
    assert "bar-queries-ColumnTrie-LeapfrogTriejoin.pdf" in files
