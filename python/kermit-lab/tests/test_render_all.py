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


def test_closes_every_figure_it_renders(fixture_tree, tmp_path: Path) -> None:
    """One figure per shape, query and ablation axis: on a ~100-query sweep,
    leaving them open trips pyplot's open-figure warning and holds every
    figure in memory (#87)."""
    plt.close("all")
    reports = load_reports(fixture_tree["paths"])
    render_all(reports, tmp_path, fixture_tree["criterion_root"], "pdf")
    rendered = {p.name for p in tmp_path.iterdir()}
    assert {"bar-time-triangle.pdf", "bar-time-chain.pdf", "bar-time-star.pdf"} <= rendered
    assert plt.get_fignums() == []


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


def test_ablation_preset_lets_build_mode_through_on_end_to_end(
    fixture_build_mode_tree,
) -> None:
    """``end_to_end`` times the build, so the guard must not refuse it. The
    fixture has no such rows, so the preset fails later and differently."""
    df = kl.load(
        fixture_build_mode_tree["paths"],
        criterion_root=fixture_build_mode_tree["criterion_root"],
    )
    with pytest.raises(InsufficientAxesError, match="no time rows"):
        presets.ablation(df, axis="ds_build_mode", phase="end_to_end")


def test_build_mode_ablation_leaves_out_structures_without_the_axis(
    fixture_build_mode_tree,
) -> None:
    """TreeTrie rows keep NaN for ``ds_build_mode``; charting them would add a
    bar for a structure that has no build mode."""
    df = kl.load(
        fixture_build_mode_tree["paths"],
        criterion_root=fixture_build_mode_tree["criterion_root"],
    )
    assert df.loc[df["data_structure"] == "TreeTrie", "ds_build_mode"].isna().all()
    fig = presets.ablation(df, axis="ds_build_mode", phase="insertion")
    labels = {t.get_text() for ax in fig.axes for t in ax.get_xticklabels()}
    plt.close(fig)
    assert labels == {"bulk", "incremental"}


_HASH_TRIE_AXES = (
    "ds_layout_hasher", "ds_layout_pruning", "ds_layout_expansion", "ds_config_load_factor",
)


def test_hash_trie_ablations_are_drawn_for_a_mixed_sweep(
    fixture_sweep_tree, tmp_path: Path
) -> None:
    plt.close("all")  # earlier tests leave figures open; stay under pyplot's cap of 20
    reports = load_reports(fixture_sweep_tree["paths"])
    render_all(reports, tmp_path, fixture_sweep_tree["criterion_root"], "pdf")
    plt.close("all")
    names = {p.name for p in tmp_path.iterdir()}
    for axis in _HASH_TRIE_AXES:
        assert f"ablation-{axis}.pdf" in names, (axis, names)


@pytest.mark.parametrize("axis", _HASH_TRIE_AXES)
def test_hash_trie_ablations_chart_hash_trie_only(fixture_sweep_tree, axis) -> None:
    """A back-fill on every row would chart TreeTrie and ColumnTrie under
    the HashTrie default, e.g. as a "sip" bar beside HashTrie's (#85)."""
    df = kl.load(fixture_sweep_tree["paths"], criterion_root=fixture_sweep_tree["criterion_root"])
    fig = presets.ablation(df, axis=axis)
    structures = {label for ax in fig.axes for label in ax.get_legend_handles_labels()[1]}
    plt.close(fig)
    assert structures == {"HashTrie"}


def test_seek_ablation_is_drawn_only_for_search_phases(
    fixture_seek_tree, tmp_path: Path
) -> None:
    """Both seek strategies build the same trie, so an insertion-time "seek
    ablation" would chart noise; iteration is where the strategy acts."""
    plt.close("all")  # earlier tests leave figures open; stay under pyplot's cap of 20
    reports = load_reports(fixture_seek_tree["paths"])
    for phase, drawn in (("iteration", True), ("insertion", False)):
        out = tmp_path / phase
        out.mkdir()
        render_all(reports, out, fixture_seek_tree["criterion_root"], "pdf", phase=phase)
        names = {p.name for p in out.iterdir()}
        plt.close("all")
        assert ("ablation-ds_layout_seek.pdf" in names) is drawn, (phase, names)
