"""CLI smoke: general plot subcommand + a preset subcommand."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

from pathlib import Path

from kermit_lab.drivers.main import main


def test_general_plot_subcommand(fixture_opt_tree, tmp_path: Path) -> None:
    out = tmp_path / "ablation.pdf"
    rc = main([
        "plot", *[str(p) for p in fixture_opt_tree["paths"]],
        "--criterion-root", str(fixture_opt_tree["criterion_root"]),
        "--out", str(out),
        "--kind", "bar", "--y", "time",
        "--x", "ds_config_load_factor",
        "--colour", "data_structure",
    ])
    assert rc == 0
    assert out.exists() and out.stat().st_size > 0


def test_scaling_preset_subcommand(fixture_tree, tmp_path: Path) -> None:
    out = tmp_path / "s.pdf"
    rc = main([
        "scaling", *[str(p) for p in fixture_tree["paths"]],
        "--criterion-root", str(fixture_tree["criterion_root"]),
        "--out", str(out),
    ])
    assert rc == 0
    assert out.exists()
