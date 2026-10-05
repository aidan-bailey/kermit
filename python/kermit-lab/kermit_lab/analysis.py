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
# Columns `kl.load` derives from another column. `threads` varies exactly when
# `ds_build_mode` does, so it is never a key that pairs rows.
_DERIVED_COLS: frozenset[str] = frozenset({"threads"})


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
        and c not in _DERIVED_COLS
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


# The columns that tell runs of one case apart, or that hold its numbers:
# everything else identifies the case.
_SPEEDUP_NON_KEYS: frozenset[str] = (
    _PROVENANCE_COLS | _VALUE_FAMILY | frozenset({"ds_build_mode", "threads"})
)


def speedup_table(
    df: pd.DataFrame,
    *,
    phase: str = "insertion",
    baseline: str = "serial",
    value: str = "mean_ns",
    n_resamples: int = 9999,
    rng: int | np.random.Generator | None = 0,
) -> pd.DataFrame:
    """Speedup of every ``parallel:N`` build over the ``baseline`` build.

    A *case* is everything a row says apart from its build mode and
    provenance: one structure, workload and relation, measured under several
    build modes. Replicates of one case and mode (one report each, told apart
    by ``criterion_group`` / ``source_path``) are pooled. Load one binary's
    reports only, or codegen drift between binaries enters the speedup.

    One row per case and thread count ``N``, with the case's columns and:

    - ``speedup``: mean baseline ``value`` over mean ``parallel:N`` ``value``;
      above 1 means the parallel build is faster;
    - ``speedup_lo`` / ``speedup_hi``: a percentile-bootstrap CI over the
      replicates when both sides have at least two, else the conservative
      envelope of Criterion's own CIs, as in :func:`compare`;
    - ``efficiency``: ``speedup / N``;
    - ``karp_flatt``: the experimentally determined serial fraction
      ``(1/speedup - 1/N) / (1 - 1/N)``, NaN at ``N = 1``. Flat across ``N``
      means a fixed sequential share limits the build; rising means a cost
      that grows with ``N`` does;
    - ``baseline_runs`` / ``runs``: the replicates pooled on each side.

    Raises ``ValueError`` when a needed column is missing or no case has both
    a baseline row and a ``parallel:N`` row on ``phase``.
    """
    value_lo, value_hi = _ci_columns_for(value)
    needed = ["metric", "phase", "ds_build_mode", "threads", value, value_lo, value_hi]
    missing = [c for c in needed if c not in df.columns]
    if missing:
        raise ValueError(f"speedup_table needs columns {missing}")
    on_phase = ((df["metric"] == "time") & (df["phase"] == phase)).fillna(False).astype(bool)
    rows = df[on_phase & df["ds_build_mode"].notna()]
    case_keys = [c for c in rows.columns if c not in _SPEEDUP_NON_KEYS]

    records: list[dict] = []
    for key, case in rows.groupby(case_keys, dropna=False, sort=True):
        is_base = case["ds_build_mode"] == baseline
        base = case.loc[is_base, value]
        if base.empty:
            continue
        identity = dict(zip(case_keys, key if isinstance(key, tuple) else (key,)))
        for threads, arm in case[case["threads"].notna()].groupby("threads", sort=True):
            n = int(threads)
            speedup = base.mean() / arm[value].mean()
            if len(base) >= 2 and len(arm) >= 2:
                lo, hi = bootstrap_ratio_ci(base, arm[value], n_resamples=n_resamples, rng=rng)
            else:
                lo = case.loc[is_base, value_lo].mean() / arm[value_hi].mean()
                hi = case.loc[is_base, value_hi].mean() / arm[value_lo].mean()
            records.append({
                **identity,
                "threads": n,
                "speedup": speedup,
                "speedup_lo": lo,
                "speedup_hi": hi,
                "efficiency": speedup / n,
                "karp_flatt": (1 / speedup - 1 / n) / (1 - 1 / n) if n > 1 else float("nan"),
                "baseline_runs": len(base),
                "runs": len(arm),
            })
    if not records:
        raise ValueError(
            f"no case has both a {baseline!r} row and a parallel:N row on {phase!r}"
        )
    return pd.DataFrame.from_records(records)
