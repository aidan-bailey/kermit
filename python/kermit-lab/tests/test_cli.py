"""CLI smoke: general plot subcommand + a preset subcommand."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

import argparse
import json
from pathlib import Path

import pytest

from kermit_lab.drivers.main import _build_parser, main


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


# Every subcommand that takes --phase, with the extra arguments it needs to
# draw from `fixture_end_to_end_seek_tree`. `render-all` is tested apart.
_TIME_SHAPES: dict[str, list[str]] = {
    "plot": ["--kind", "bar", "--x", "data_structure"],
    "scaling": [],
    "bar-time": ["--query", "triangle"],
    "tradeoff": [],
    "dist": [],
    "bar-queries": [],
    "ablation": ["--axis", "ds_layout_seek"],
}


def _subcommands_taking_phase() -> set[str]:
    parser = _build_parser()
    subparsers = next(a for a in parser._actions if isinstance(a, argparse._SubParsersAction))
    return {
        name for name, sub in subparsers.choices.items()
        if any("--phase" in a.option_strings for a in sub._actions)
    }


def test_every_time_shape_is_covered() -> None:
    """A new time-using shape must join the end_to_end test below. `speedup`
    is the exception: it compares build modes, which that test's fixture has
    none of, so `test_speedup_preset_subcommand` covers it (#94)."""
    assert _subcommands_taking_phase() == {*_TIME_SHAPES, "render-all", "speedup"}


@pytest.mark.parametrize("shape", sorted(_TIME_SHAPES))
def test_time_shape_plots_end_to_end(shape, fixture_end_to_end_seek_tree, tmp_path: Path) -> None:
    """Reports carry end_to_end timings, and it is one of the phases a
    build-mode or seek ablation can be drawn on (#87)."""
    out = tmp_path / f"{shape}.pdf"
    rc = main([
        shape, *[str(p) for p in fixture_end_to_end_seek_tree["paths"]],
        "--criterion-root", str(fixture_end_to_end_seek_tree["criterion_root"]),
        "--out", str(out), "--phase", "end_to_end", *_TIME_SHAPES[shape],
    ])
    assert rc == 0
    assert out.exists() and out.stat().st_size > 0


def test_render_all_plots_end_to_end(fixture_end_to_end_seek_tree, tmp_path: Path) -> None:
    out_dir = tmp_path / "out"
    rc = main([
        "render-all", *[str(p) for p in fixture_end_to_end_seek_tree["paths"]],
        "--criterion-root", str(fixture_end_to_end_seek_tree["criterion_root"]),
        "--out-dir", str(out_dir), "--phase", "end_to_end",
    ])
    assert rc == 0
    names = {p.name for p in out_dir.iterdir()}
    assert {
        "scaling.pdf", "tradeoff.pdf", "dist.pdf", "bar-time-triangle.pdf",
        "ablation-ds_layout_seek.pdf",
    } <= names, names


def test_speedup_preset_subcommand(fixture_parallel_build_tree, tmp_path: Path) -> None:
    out = tmp_path / "speedup.pdf"
    rc = main([
        "speedup", *[str(p) for p in fixture_parallel_build_tree["paths"]],
        "--criterion-root", str(fixture_parallel_build_tree["criterion_root"]),
        "--out", str(out),
    ])
    assert rc == 0
    assert out.exists() and out.stat().st_size > 0


def test_speedup_subcommand_refuses_search_phases() -> None:
    with pytest.raises(SystemExit):
        _build_parser().parse_args(["speedup", "r.json", "--out", "s.pdf", "--phase", "iteration"])
