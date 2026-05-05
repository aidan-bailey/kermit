"""Analysis layer tests: pivot summaries, pairwise comparison, stats."""
from __future__ import annotations

import numpy as np
import pytest

import kermit_lab as kl


@pytest.fixture
def df(fixture_tree):
    return kl.load(fixture_tree["paths"], fixture_tree["criterion_root"])


# --- summary ---------------------------------------------------------------


def test_summary_pivot_shape(df):
    # Filter to triangle/iteration so the pivot has dense rows.
    sub = df[(df.metric == "time") & (df.phase == "iteration") & (df["query"] == "triangle")]
    pivot = kl.summary(sub, rows="data_structure", cols="tuples", value="mean_ns")
    # Two DS × three tuples values.
    assert pivot.shape == (2, 3)
    assert set(pivot.index) == {"TreeTrie", "ColumnTrie"}
    assert set(pivot.columns) == {10, 100, 1000}


def test_summary_uses_mean_aggfunc_by_default(df):
    sub = df[(df.metric == "time") & (df.phase == "iteration") & (df["query"] == "triangle")]
    pivot = kl.summary(sub, rows="data_structure", cols="tuples")
    # Fixture point estimate: iteration_point = 100.0 * n; identical for both DS.
    for ds in ("TreeTrie", "ColumnTrie"):
        for n in (10, 100, 1000):
            assert pivot.loc[ds, n] == pytest.approx(100.0 * n)


# --- compare ---------------------------------------------------------------


def test_compare_speedup_one_when_means_match(df):
    # Fixture has identical iteration_point (100.0 * n) for both DS, so the
    # speedup of TreeTrie-vs-ColumnTrie at every (query, tuples) pair is 1.0.
    sub = df[(df.metric == "time") & (df.phase == "iteration") & (df["query"] == "triangle")]
    result = kl.compare(sub, baseline="TreeTrie", target="ColumnTrie")
    assert len(result) == 3  # one per tuples value
    assert (result.speedup == 1.0).all()


def test_compare_raises_for_missing_group(df):
    with pytest.raises(ValueError, match="MysteryStruct"):
        kl.compare(df, baseline="TreeTrie", target="MysteryStruct")


def test_compare_returns_speedup_envelope(df):
    sub = df[(df.metric == "time") & (df.phase == "iteration") & (df["query"] == "triangle")]
    result = kl.compare(sub, baseline="TreeTrie", target="ColumnTrie")
    # Envelope: speedup_lo ≤ speedup ≤ speedup_hi (assuming non-negative values).
    assert (result.speedup_lo <= result.speedup).all()
    assert (result.speedup <= result.speedup_hi).all()


# --- bootstrap_ratio_ci ----------------------------------------------------


def test_bootstrap_ratio_ci_contains_true_ratio():
    rng = np.random.default_rng(42)
    a = rng.normal(loc=10.0, scale=0.5, size=200)
    b = rng.normal(loc=5.0, scale=0.5, size=200)  # mean(a)/mean(b) ≈ 2.0
    lo, hi = kl.bootstrap_ratio_ci(a, b, n_resamples=2000, rng=42)
    assert lo < 2.0 < hi


def test_bootstrap_ratio_ci_deterministic_with_seed():
    a = np.arange(1.0, 11.0)
    b = np.arange(2.0, 22.0, 2.0)  # exactly 2x a; ratio = 0.5
    lo1, hi1 = kl.bootstrap_ratio_ci(a, b, n_resamples=500, rng=7)
    lo2, hi2 = kl.bootstrap_ratio_ci(a, b, n_resamples=500, rng=7)
    assert (lo1, hi1) == (lo2, hi2)


# --- mannwhitney_u ---------------------------------------------------------


def test_mannwhitney_p_value_in_range():
    rng = np.random.default_rng(0)
    _u, p = kl.mannwhitney_u(rng.normal(size=50), rng.normal(size=50))
    assert 0.0 <= p <= 1.0


def test_mannwhitney_identical_samples_not_significant():
    rng = np.random.default_rng(0)
    a = rng.normal(loc=5.0, scale=1.0, size=100)
    b = rng.normal(loc=5.0, scale=1.0, size=100)
    _u, p = kl.mannwhitney_u(a, b)
    # Drawn from same distribution → cannot reject null at α=0.05.
    assert p > 0.05
