"""CLI smoke: general plot subcommand + a preset subcommand."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

import json
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


def test_mixed_schema_reports_exit_with_code_4(tmp_path: Path) -> None:
    paths = []
    for version in (2, 3):
        p = tmp_path / f"v{version}.json"
        p.write_text(json.dumps([{"schema_version": version, "kind": "run",
                                  "metadata": [], "axes": {}, "criterion_groups": []}]))
        paths.append(str(p))
    rc = main(["scaling", *paths, "--criterion-root", str(tmp_path),
               "--out", str(tmp_path / "s.pdf")])
    assert rc == 4
