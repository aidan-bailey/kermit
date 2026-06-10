"""The package's public surface exposes the engine + presets."""
from __future__ import annotations

import kermit_lab as kl


def test_public_names() -> None:
    for name in ("plot", "scaling", "bar_time", "bar_space", "tradeoff",
                 "dist", "bar_queries", "ablation", "load", "load_samples",
                 "colour_for", "summary"):
        assert hasattr(kl, name), name


def test_plots_subpackage_gone() -> None:
    import importlib
    try:
        importlib.import_module("kermit_lab.plots")
    except ModuleNotFoundError:
        return
    raise AssertionError("kermit_lab.plots should be deleted")
