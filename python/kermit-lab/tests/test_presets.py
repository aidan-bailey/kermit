"""Presets build the right aesthetic and return a Figure."""
from __future__ import annotations

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
import pandas as pd
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


def test_speedup_labels_its_axis_with_each_structures_default_baseline(
    fixture_parallel_build_tree,
) -> None:
    """The baseline is per structure unless ``baseline`` is given: TreeTrie
    divides by ``serial``, HashTrie by ``bulk``, and the label names both."""
    tree = load(fixture_parallel_build_tree["paths"], fixture_parallel_build_tree["criterion_root"])
    tree = tree[tree["data_structure"] == "TreeTrie"]
    hash_trie = tree.assign(
        data_structure="HashTrie",
        ds_build_mode=tree["ds_build_mode"].replace("serial", "bulk"),
        criterion_group=tree["criterion_group"] + "-hash",
        source_path=tree["source_path"] + "-hash",
    )

    def ylabel(df: pd.DataFrame, **kwargs) -> str:
        fig = presets.speedup(df, **kwargs)
        label = fig.axes[0].get_ylabel()
        plt.close(fig)
        return label

    assert ylabel(tree) == "speedup over serial (insertion)"
    assert ylabel(hash_trie) == "speedup over bulk (insertion)"
    assert ylabel(pd.concat([tree, hash_trie])) == "speedup over bulk / serial (insertion)"
    assert ylabel(tree, baseline="parallel:2") == "speedup over parallel:2 (insertion)"


def test_speedup_refuses_a_phase_that_is_not_a_time_phase(fixture_parallel_build_tree) -> None:
    """Every time phase is a speedup phase, ``iteration`` included (a build
    mode may move it, Amendment 2); ``space`` is not a time phase."""
    df = load(fixture_parallel_build_tree["paths"], fixture_parallel_build_tree["criterion_root"])
    with pytest.raises(InsufficientAxesError, match="time phases, not 'space'"):
        presets.speedup(df, phase="space")
    # The fixture times insertion only, so an iteration speedup passes the
    # phase check and finds no case to compare.
    with pytest.raises(InsufficientAxesError, match="no case"):
        presets.speedup(df, phase="iteration")


def test_speedup_without_a_threaded_build_is_insufficient_axes(fixture_build_mode_tree) -> None:
    """ColumnTrie and HashTrie rows, each with a baseline but no `parallel:N`."""
    df = load(fixture_build_mode_tree["paths"], fixture_build_mode_tree["criterion_root"])
    with pytest.raises(InsufficientAxesError, match="no case has both a 'bulk' row"):
        presets.speedup(df)


def test_speedup_ideal_line_is_y_equals_x_on_log_axes(fixture_parallel_build_tree) -> None:
    """Two points make a straight screen line, so the ideal is y = x only when
    both axes are logarithmic: on a log x-axis with a linear y-axis it sat above
    data that follows y = x."""
    df = load(fixture_parallel_build_tree["paths"], fixture_parallel_build_tree["criterion_root"])
    fig = presets.speedup(df)
    ax = fig.axes[0]
    ideal = next(line for line in ax.get_lines() if line.get_label() == "ideal")
    xs, ys = list(ideal.get_xdata()), list(ideal.get_ydata())
    scales = (ax.get_xscale(), ax.get_yscale())
    plt.close(fig)
    assert xs == ys
    assert scales == ("log", "log")


def test_speedup_draws_parallel_and_presized_as_separate_lines(
    fixture_parallel_build_tree,
) -> None:
    """`parallel:N` and `presized:N` arms of one case are two curves over the
    same thread counts, each labelled by its build, never one merged line."""
    df = load(fixture_parallel_build_tree["paths"], fixture_parallel_build_tree["criterion_root"])
    tree = df[df["data_structure"] == "TreeTrie"]
    threaded = tree[tree["threads"].notna()]
    presized = threaded.assign(
        ds_build_mode="presized:" + threaded["threads"].astype(str),
        criterion_group=threaded["criterion_group"] + "-presized",
    )
    fig = presets.speedup(pd.concat([tree, presized], ignore_index=True))
    lines = {
        line.get_label(): sorted(line.get_xdata())
        for line in fig.axes[0].get_lines() if line.get_label() != "ideal"
    }
    plt.close(fig)
    assert lines == {"TreeTrie / parallel": [2, 4], "TreeTrie / presized": [2, 4]}


def test_speedup_title_names_a_lone_presized_build(fixture_parallel_build_tree) -> None:
    """A figure of `presized:N` arms alone says so; parallel-only and mixed
    figures keep the title they always had."""
    df = load(fixture_parallel_build_tree["paths"], fixture_parallel_build_tree["criterion_root"])
    tree = df[df["data_structure"] == "TreeTrie"]
    threaded = tree["threads"].notna()
    presized = tree.assign(
        ds_build_mode=tree["ds_build_mode"].where(
            ~threaded, "presized:" + tree["threads"].astype(str)
        ),
    )
    mixed = pd.concat([
        tree,
        presized[threaded].assign(criterion_group=lambda d: d["criterion_group"] + "-presized"),
    ], ignore_index=True)
    titles = {}
    for name, frame in (("parallel", tree), ("presized", presized), ("mixed", mixed)):
        fig = presets.speedup(frame)
        titles[name] = fig.get_suptitle()
        plt.close(fig)
    assert titles == {
        "parallel": "Parallel build speedup",
        "presized": "Presized build speedup",
        "mixed": "Parallel build speedup",
    }


def test_speedup_draws_one_line_per_case(fixture_parallel_build_tree) -> None:
    """Cases that differ only in their relation get a line each, labelled by it."""
    df = load(fixture_parallel_build_tree["paths"], fixture_parallel_build_tree["criterion_root"])
    tree = df[df["data_structure"] == "TreeTrie"]
    a = tree.assign(relation_path="a.parquet")
    b = tree.assign(relation_path="b.parquet", criterion_group=tree["criterion_group"] + "-b")
    fig = presets.speedup(pd.concat([a, b], ignore_index=True))
    labels = {line.get_label() for line in fig.axes[0].get_lines()}
    plt.close(fig)
    assert labels - {"ideal"} == {"a.parquet", "b.parquet"}
