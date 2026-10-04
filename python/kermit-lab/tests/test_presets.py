"""Presets build the right aesthetic and return a Figure."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
import pytest
from matplotlib.figure import Figure

import kermit_lab as kl
from kermit_lab import presets
from kermit_lab.frame import load, load_samples
from kermit_lab.plots_errors import InsufficientAxesError


def test_scaling(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert isinstance(presets.scaling(df), Figure)
    plt.close("all")


def test_bar_time(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert isinstance(presets.bar_time(df, query="triangle"), Figure)
    plt.close("all")


def test_bar_space(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert isinstance(presets.bar_space(df), Figure)
    plt.close("all")


def test_tradeoff(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert isinstance(presets.tradeoff(df), Figure)
    plt.close("all")


def test_dist(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    samples = load_samples(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert isinstance(presets.dist(df, samples=samples), Figure)
    plt.close("all")


def test_bar_queries(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert isinstance(presets.bar_queries(df, ds=["TreeTrie"]), Figure)
    plt.close("all")


def test_ablation(fixture_opt_tree) -> None:
    df = load(fixture_opt_tree["paths"], fixture_opt_tree["criterion_root"])
    assert isinstance(presets.ablation(df, axis="ds_config_load_factor"), Figure)
    plt.close("all")


def test_ablation_on_an_unknown_axis_is_insufficient_axes(fixture_opt_tree) -> None:
    """The CLI maps this error to a clean exit; a bare ``KeyError`` would
    surface as a traceback."""
    df = load(fixture_opt_tree["paths"], fixture_opt_tree["criterion_root"])
    with pytest.raises(InsufficientAxesError):
        presets.ablation(df, axis="ds_no_such_axis")


def test_ablation_refuses_seek_outside_search_phases(fixture_seek_tree) -> None:
    df = kl.load(
        fixture_seek_tree["paths"], criterion_root=fixture_seek_tree["criterion_root"]
    )
    assert set(df["ds_layout_seek"].dropna()) == {"binary", "galloping"}
    with pytest.raises(InsufficientAxesError, match="searched"):
        presets.ablation(df, axis="ds_layout_seek", phase="insertion")
    assert isinstance(
        presets.ablation(df, axis="ds_layout_seek", phase="iteration"), Figure
    )
    plt.close("all")
