"""DataFrame-based analysis: pivots, pairwise comparisons, stats tests.

All functions take/return DataFrames or numpy arrays. No matplotlib imports
— this module is callable from headless contexts (CI, scripted reports).
"""
from __future__ import annotations

import warnings
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


# Columns that identify neither a case nor a measurement: provenance, the
# value family, the build mode itself (and the `threads` derived from it), and
# whether `--verify` ran.
_SPEEDUP_NON_KEYS: frozenset[str] = (
    _PROVENANCE_COLS | _VALUE_FAMILY | frozenset({"ds_build_mode", "threads", "verified"})
)

#: The columns `speedup_table` adds after a case's identifying columns.
SPEEDUP_MEASURES: tuple[str, ...] = (
    "threads", "speedup", "speedup_lo", "speedup_hi", "efficiency", "karp_flatt",
    "baseline_runs", "runs",
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
    build modes. Whether ``--verify`` ran does not identify a case, nor does
    ``queries_per_build`` on any phase but ``end_to_end``, the only one it
    shapes. Replicates of one case and mode (one report each, told apart by
    ``criterion_group`` / ``source_path``) are pooled; rows that read one
    Criterion directory are one measurement, not replicates. Load one binary's
    reports only, or codegen drift between binaries enters the speedup, and
    load with ``apply_defaults=False`` (as ``kermit-lab speedup`` does): that
    keeps TreeTrie reports from before #94 out of the baseline, where
    back-filling would count them as ``serial``.

    One row per case and thread count ``N``, with the case's columns followed
    by :data:`SPEEDUP_MEASURES`:

    - ``speedup``: mean baseline ``value`` over mean ``parallel:N`` ``value``;
      above 1 means the parallel build is faster;
    - ``speedup_lo`` / ``speedup_hi``: a percentile-bootstrap CI over the
      replicates when both sides have at least two runs. Otherwise the widest
      ratio the runs' own Criterion CIs allow:
      ``min(baseline lo) / max(parallel hi)`` to
      ``max(baseline hi) / min(parallel lo)``. With one run per side that is
      the envelope :func:`compare` reports, and with more it also covers the
      spread between runs;
    - ``efficiency``: ``speedup / N``;
    - ``karp_flatt``: the experimentally determined serial fraction
      ``(1/speedup - 1/N) / (1 - 1/N)``, NaN at ``N = 1``. Flat across ``N``
      means a fixed sequential share limits the build; rising means a cost
      that grows with ``N`` does. It is negative when the speedup is
      superlinear (``speedup > N``), which the per-partition sort saving can
      cause;
    - ``baseline_runs`` / ``runs``: the replicates pooled on each side.

    Raises ``ValueError`` when a needed column is missing, when rows share a
    Criterion directory (runs that shared a ``--name`` overwrote each other),
    or when no case has both a baseline row and a ``parallel:N`` row on
    ``phase``. Warns about ``parallel:N`` rows whose case has no baseline row,
    and leaves them out.
    """
    value_lo, value_hi = _ci_columns_for(value)
    needed = [
        "metric", "phase", "ds_build_mode", "threads", "criterion_group",
        "criterion_function", "source_path", value, value_lo, value_hi,
    ]
    missing = [c for c in needed if c not in df.columns]
    if missing:
        raise ValueError(f"speedup_table needs columns {missing}")
    on_phase = ((df["metric"] == "time") & (df["phase"] == phase)).fillna(False).astype(bool)
    # Fresh labels, so `paired` below cannot mix up the rows of a frame whose
    # index repeats.
    rows = df[on_phase & df["ds_build_mode"].notna()].reset_index(drop=True)

    # Rows that read one Criterion directory are one measurement, not
    # replicates: runs that shared a `--name` overwrote each other.
    shared = rows.duplicated(["criterion_group", "criterion_function"], keep=False)
    if shared.any():
        groups = sorted(set(rows.loc[shared, "criterion_group"].astype(str)))
        raise ValueError(
            f"rows share a Criterion directory, so they are one measurement, not "
            f"replicates: {groups}; give every run its own --name"
        )
    # `queries_per_build` shapes only end_to_end; on any other phase a run that
    # also timed end_to_end must still pair with one that did not.
    non_keys = _SPEEDUP_NON_KEYS
    if phase != "end_to_end":
        non_keys = non_keys | {"queries_per_build"}
    case_keys = [c for c in rows.columns if c not in non_keys]

    paired: set = set()
    records: list[dict] = []
    for key, case in rows.groupby(case_keys, dropna=False, sort=True):
        base = case[case["ds_build_mode"] == baseline]
        if base.empty:
            continue
        identity = dict(zip(case_keys, key if isinstance(key, tuple) else (key,)))
        for threads, arm in case[case["threads"].notna()].groupby("threads", sort=True):
            paired.update(arm.index)
            n = int(threads)
            speedup = base[value].mean() / arm[value].mean()
            if len(base) >= 2 and len(arm) >= 2:
                lo, hi = bootstrap_ratio_ci(
                    base[value], arm[value], n_resamples=n_resamples, rng=rng
                )
            else:
                # Too few runs to resample: the widest ratio the runs' own
                # Criterion intervals allow, which also covers the spread
                # between runs.
                lo = base[value_lo].min() / arm[value_hi].max()
                hi = base[value_hi].max() / arm[value_lo].min()
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
    orphans = rows[rows["threads"].notna() & ~rows.index.isin(paired)]
    if not orphans.empty:
        reports = ", ".join(sorted(set(orphans["source_path"].astype(str))))
        warnings.warn(
            f"{len(orphans)} parallel:N row(s) on {phase!r} have no {baseline!r} row in "
            f"their case and are left out (a key such as relation_path, optimiser or "
            f"a layout axis differs): {reports}",
            stacklevel=2,
        )
    if not records:
        raise ValueError(
            f"no case has both a {baseline!r} row and a parallel:N row on {phase!r}"
        )
    return pd.DataFrame.from_records(records)
