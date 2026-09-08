"""render-all emits the fixed shapes plus one ablation figure per opt axis."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

from pathlib import Path

from kermit_lab.drivers.render_all import render_all
from kermit_lab.loader import load_reports


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
