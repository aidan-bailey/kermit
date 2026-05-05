"""Verify each plot's ``plot()`` returns a Figure and ``out=`` writes a file.

The legacy ``render(reports, out_path, criterion_root, **kw)`` shims keep
the existing 31 plot tests green; these tests exercise the new
DataFrame-first ``plot(df, …, out=None) -> Figure`` surface that notebooks
consume.
"""
from __future__ import annotations

from pathlib import Path

import matplotlib.figure
import pytest

import kermit_lab as kl


@pytest.fixture
def df(fixture_tree):
    return kl.load(fixture_tree["paths"], fixture_tree["criterion_root"])


def test_scaling_returns_figure(df):
    assert isinstance(kl.scaling(df), matplotlib.figure.Figure)


def test_scaling_out_writes_and_returns_figure(df, tmp_path: Path):
    out = tmp_path / "scaling.pdf"
    fig = kl.scaling(df, out=out)
    assert isinstance(fig, matplotlib.figure.Figure)
    assert out.is_file() and out.stat().st_size > 0


def test_bar_time_returns_figure(df):
    assert isinstance(kl.bar_time(df, query="triangle"), matplotlib.figure.Figure)


def test_bar_space_returns_figure(df):
    assert isinstance(kl.bar_space(df), matplotlib.figure.Figure)


def test_tradeoff_returns_figure(df):
    assert isinstance(kl.tradeoff(df), matplotlib.figure.Figure)


def test_dist_returns_figure(df, fixture_tree):
    fig = kl.dist(df, criterion_root=fixture_tree["criterion_root"])
    assert isinstance(fig, matplotlib.figure.Figure)


def test_bar_queries_returns_figure(df):
    fig = kl.bar_queries(df, ds="TreeTrie", algo="LeapfrogTriejoin")
    assert isinstance(fig, matplotlib.figure.Figure)
