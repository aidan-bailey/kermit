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


def test_ablation_lets_seek_through_on_end_to_end(fixture_seek_tree) -> None:
    df = kl.load(
        fixture_seek_tree["paths"], criterion_root=fixture_seek_tree["criterion_root"]
    )
    # The guard lets end_to_end through; the error comes from the fixture
    # having no end_to_end rows, not from the seek scope.
    with pytest.raises(InsufficientAxesError, match="no time rows"):
        presets.ablation(df, axis="ds_layout_seek", phase="end_to_end")


def test_speedup(fixture_parallel_build_tree) -> None:
    df = load(fixture_parallel_build_tree["paths"], fixture_parallel_build_tree["criterion_root"])
    fig = presets.speedup(df)
    labels = {label for ax in fig.axes for label in ax.get_legend_handles_labels()[1]}
    plt.close(fig)
    assert isinstance(fig, Figure)
    assert {"TreeTrie", "ideal"} <= labels


def test_speedup_refuses_phases_a_build_mode_cannot_affect(fixture_parallel_build_tree) -> None:
    df = load(fixture_parallel_build_tree["paths"], fixture_parallel_build_tree["criterion_root"])
    with pytest.raises(InsufficientAxesError, match="built"):
        presets.speedup(df, phase="iteration")


def test_speedup_without_a_serial_baseline_is_insufficient_axes(fixture_build_mode_tree) -> None:
    df = load(fixture_build_mode_tree["paths"], fixture_build_mode_tree["criterion_root"])
    with pytest.raises(InsufficientAxesError, match="no case"):
        presets.speedup(df)
