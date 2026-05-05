"""DataFrame-based analysis: pivots, pairwise comparisons, stats tests.

All functions take/return DataFrames or numpy arrays. No matplotlib imports
— this module is callable from headless contexts (CI, scripted reports).
"""
from __future__ import annotations

from typing import Any

import numpy as np
import pandas as pd
from scipy.stats import bootstrap, mannwhitneyu

# Columns that are not "natural grouping" keys for `compare`: provenance and
# any value-family column (mean/median estimates and their CI bounds).
_PROVENANCE_COLS: frozenset[str] = frozenset({
    "source_path", "criterion_group", "criterion_function",
})
_VALUE_FAMILY: frozenset[str] = frozenset({
    "mean_ns", "mean_lo", "mean_hi", "mean_se",
    "median_ns", "median_lo", "median_hi",
})


def summary(
    df: pd.DataFrame,
    *,
    rows: str | list[str],
    cols: str | list[str],
    value: str = "mean_ns",
    aggfunc: Any = np.mean,
) -> pd.DataFrame:
    """Pivot ``df`` into a 2-D summary table.

    Thin wrapper over :meth:`pandas.DataFrame.pivot_table` that picks
    sensible defaults for benchmark workflows.
    """
    return df.pivot_table(values=value, index=rows, columns=cols, aggfunc=aggfunc)


def compare(
    df: pd.DataFrame,
    *,
    baseline: str,
    target: str,
    group_by: str = "data_structure",
    value: str = "mean_ns",
) -> pd.DataFrame:
    """Pair every ``baseline`` row with the matching ``target`` row; compute speedup.

    Pairs are matched on every column except ``group_by``, the value-family
    columns, and provenance. ``speedup = baseline / target``: a speedup > 1
    means ``target`` is faster than ``baseline``.

    The returned ``speedup_lo``/``speedup_hi`` are a deterministic envelope
    from the summary's stored CIs (``baseline_lo / target_hi`` and
    ``baseline_hi / target_lo``) — wide and conservative. For tighter CIs
    use :func:`bootstrap_ratio_ci` on per-iteration samples.
    """
    base_mask = df[group_by] == baseline
    tgt_mask = df[group_by] == target
    if not base_mask.any() or not tgt_mask.any():
        raise ValueError(
            f"need at least one row each for {group_by}=={baseline!r} and =={target!r}"
        )

    value_lo, value_hi = _ci_columns_for(value)
    for col in (value, value_lo, value_hi):
        if col not in df.columns:
            raise ValueError(f"missing column required by value={value!r}: {col!r}")

    join_keys = [
        c for c in df.columns
        if c != group_by and c not in _PROVENANCE_COLS and c not in _VALUE_FAMILY
    ]

    base = df.loc[base_mask, join_keys + [value, value_lo, value_hi]].rename(columns={
        value: "baseline_value",
        value_lo: "baseline_lo",
        value_hi: "baseline_hi",
    })
    tgt = df.loc[tgt_mask, join_keys + [value, value_lo, value_hi]].rename(columns={
        value: "target_value",
        value_lo: "target_lo",
        value_hi: "target_hi",
    })

    merged = base.merge(tgt, on=join_keys, how="inner")
    merged["speedup"] = merged.baseline_value / merged.target_value
    merged["speedup_lo"] = merged.baseline_lo / merged.target_hi
    merged["speedup_hi"] = merged.baseline_hi / merged.target_lo
    return merged


def _ci_columns_for(value: str) -> tuple[str, str]:
    """Map ``value`` to its conventional ``_lo``/``_hi`` companion columns.

    ``"mean_ns"`` → ``("mean_lo", "mean_hi")``; ``"median_ns"`` →
    ``("median_lo", "median_hi")``. Other values yield ``"<value>_lo"`` /
    ``"<value>_hi"`` and will fail the column-presence check in
    :func:`compare` if they don't exist.
    """
    stem = value.removesuffix("_ns")
    return f"{stem}_lo", f"{stem}_hi"


def bootstrap_ratio_ci(
    a: np.ndarray | list[float],
    b: np.ndarray | list[float],
    *,
    n_resamples: int = 9999,
    ci: float = 0.95,
    rng: int | np.random.Generator | None = None,
) -> tuple[float, float]:
    """Percentile bootstrap CI for ``mean(a) / mean(b)``.

    Delegates to :func:`scipy.stats.bootstrap` with the percentile method
    (the most permissive — BCa requires more samples than typical Criterion
    runs produce). Pass an int seed via ``rng`` for deterministic results.
    """
    a_arr = np.asarray(a, dtype=float)
    b_arr = np.asarray(b, dtype=float)

    def _ratio(x: np.ndarray, y: np.ndarray, axis: int) -> np.ndarray:
        return np.mean(x, axis=axis) / np.mean(y, axis=axis)

    result = bootstrap(
        (a_arr, b_arr),
        _ratio,
        n_resamples=n_resamples,
        confidence_level=ci,
        method="percentile",
        paired=False,
        vectorized=True,
        random_state=rng,
    )
    return float(result.confidence_interval.low), float(result.confidence_interval.high)


def mannwhitney_u(
    a: np.ndarray | list[float],
    b: np.ndarray | list[float],
    *,
    alternative: str = "two-sided",
) -> tuple[float, float]:
    """Mann-Whitney U test on two independent samples.

    Returns ``(U-statistic, p-value)``. A small p-value supports rejecting
    the null hypothesis that the two distributions have equal medians under
    ``alternative`` (``"two-sided"`` / ``"less"`` / ``"greater"``).
    """
    res = mannwhitneyu(np.asarray(a, dtype=float), np.asarray(b, dtype=float), alternative=alternative)
    return float(res.statistic), float(res.pvalue)
