"""render-all emits the fixed shapes plus one ablation figure per opt axis."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

from pathlib import Path

import matplotlib.pyplot as plt
import pytest

import kermit_lab as kl
from kermit_lab import presets
from kermit_lab.drivers.render_all import render_all
from kermit_lab.loader import load_reports
from kermit_lab.plots_errors import InsufficientAxesError


def test_emits_conventional_shapes(fixture_tree, tmp_path: Path) -> None:
    reports = load_reports(fixture_tree["paths"])
    out = tmp_path / "out"
    out.mkdir()
    render_all(reports, out, fixture_tree["criterion_root"], "pdf")
    names = {p.name for p in out.iterdir()}
    assert "scaling.pdf" in names
    assert "bar-space.pdf" in names
    assert any(n.startswith("bar-time-") for n in names)


def test_emits_ablation_for_opt_axis(fixture_opt_tree, tmp_path: Path) -> None:
    reports = load_reports(fixture_opt_tree["paths"])
    out = tmp_path / "out"
    out.mkdir()
    render_all(reports, out, fixture_opt_tree["criterion_root"], "pdf")
    names = {p.name for p in out.iterdir()}
    assert any("ablation-ds_config_load_factor" in n for n in names)
    assert any("ablation-ds_layout_hasher" in n for n in names)


def test_build_mode_ablation_is_drawn_only_for_build_phases(
    fixture_build_mode_tree, tmp_path: Path
) -> None:
    """Old ColumnTrie rows back-fill to ``incremental`` beside new ``bulk``
    ones, but both builds produce the same trie: an iteration-time
    "build-mode ablation" would chart drift, not the build mode."""
    plt.close("all")  # earlier tests leave figures open; stay under pyplot's cap of 20
    reports = load_reports(fixture_build_mode_tree["paths"])
    for phase, drawn in (("iteration", False), ("insertion", True)):
        out = tmp_path / phase
        out.mkdir()
        render_all(reports, out, fixture_build_mode_tree["criterion_root"], "pdf", phase=phase)
        names = {p.name for p in out.iterdir()}
        plt.close("all")
        assert ("ablation-ds_build_mode.pdf" in names) is drawn, (phase, names)


def test_ablation_preset_refuses_build_mode_outside_build_phases(
    fixture_build_mode_tree,
) -> None:
    df = kl.load(
        fixture_build_mode_tree["paths"],
        criterion_root=fixture_build_mode_tree["criterion_root"],
    )
    assert set(df["ds_build_mode"].dropna()) == {"incremental", "bulk"}
    with pytest.raises(InsufficientAxesError, match="built"):
        presets.ablation(df, axis="ds_build_mode", phase="iteration")
