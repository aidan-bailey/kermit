"""Build pandas DataFrames from ``BenchReport`` JSON + Criterion artefacts.

The primary public surface for notebook users: ``load()`` returns the
summary frame (one row per report+function), ``load_samples()`` returns the
per-iteration samples frame. Both are tidy long-form — call ``.groupby``,
``.pivot_table``, ``.query`` directly.

The dataclasses in :mod:`.loader` and :mod:`.criterion` are the internal
currency of the loading layer; ``frame`` exposes them as DataFrames, which
the plot modules consume.
"""
from __future__ import annotations

import glob
import re
from pathlib import Path
from typing import Iterable, Sequence

import pandas as pd

from .criterion import FunctionData
from .loader import BenchReport, CriterionGroupRef, iter_function_data, load_reports, phase_of

# Matches ds_layout_*, ds_config_*, ds_build_mode, and the algo_* analogues.
_OPT_AXIS_RE = re.compile(r"^(ds|algo)_(layout_|config_|build_mode\b)")

# Explicit include-list for axis columns. Unknown axis keys in the input
# JSON are silently dropped — when ``docs/specs/bench-report-schema.md``
# adds a new key, extend the matching tuple here.
_AXIS_STR_KEYS: tuple[str, ...] = (
    "data_structure",
    "algorithm",
    "query",
    "benchmark",
    "relation_path",
)
_AXIS_INT_KEYS: tuple[str, ...] = (
    "tuples",
    "arity",
    "relations",
    "relation_bytes",
)

# Fixed core columns (ordered). Optimization-axis tail and stat columns are
# appended dynamically in ``_summary_from_reports``.
_SUMMARY_COLUMNS_CORE: tuple[str, ...] = (
    "kind", "metric", "phase",
    *_AXIS_STR_KEYS,
    *_AXIS_INT_KEYS,
)
_SUMMARY_COLUMNS_STATS: tuple[str, ...] = (
    "mean_ns", "mean_lo", "mean_hi", "mean_se",
    "median_ns", "median_lo", "median_hi",
    "source_path", "criterion_group", "criterion_function",
)


def _summary_row(
    report: BenchReport,
    group_ref: CriterionGroupRef,
    data: FunctionData,
    opt_axes: Sequence[str],
) -> dict:
    phase = phase_of(group_ref.function)
    row: dict = {
        "kind": report.kind,
        "metric": group_ref.metric,
        "phase": phase if phase is not None else pd.NA,
    }
    for key in _AXIS_STR_KEYS:
        v = report.axis(key)
        row[key] = v if isinstance(v, str) else pd.NA
    for key in _AXIS_INT_KEYS:
        v = report.axis(key)
        row[key] = v if isinstance(v, int) and not isinstance(v, bool) else pd.NA
    for key in opt_axes:
        v = report.axis(key)
        # Keep native type (str / bool / int); only None becomes NA. Booleans
        # (config flags) intentionally pass through — they render as True/False
        # categories when bound to a channel.
        row[key] = v if v is not None else pd.NA
    row["mean_ns"] = data.mean.point
    row["mean_lo"] = data.mean.lower
    row["mean_hi"] = data.mean.upper
    row["mean_se"] = data.mean.standard_error
    row["median_ns"] = data.median.point
    row["median_lo"] = data.median.lower
    row["median_hi"] = data.median.upper
    row["source_path"] = str(report.source_path)
    row["criterion_group"] = group_ref.group
    row["criterion_function"] = group_ref.function
    return row


def _discover_opt_axes(reports: Sequence[BenchReport]) -> list[str]:
    """Sorted union of every axes key matching the ds_*/algo_* optimization prefixes."""
    keys: set[str] = set()
    for r in reports:
        for k in r.axes:
            if _OPT_AXIS_RE.match(k):
                keys.add(k)
    return sorted(keys)


def discover_opt_columns(df: pd.DataFrame) -> list[str]:
    """Optimization-axis columns present in a loaded summary frame, sorted."""
    return sorted(c for c in df.columns if _OPT_AXIS_RE.match(c))


def _summary_from_reports(
    reports: Sequence[BenchReport],
    criterion_root: Path | str,
    *,
    apply_defaults: bool = True,
) -> pd.DataFrame:
    """Build the summary DataFrame: fixed core columns, then the discovered
    optimization tail, then the stat/join columns."""
    from .defaults import apply_axis_defaults

    opt_axes = _discover_opt_axes(reports)
    rows = [
        _summary_row(report, gref, data, opt_axes)
        for report, gref, data in iter_function_data(reports, Path(criterion_root))
    ]
    columns = [*_SUMMARY_COLUMNS_CORE, *opt_axes, *_SUMMARY_COLUMNS_STATS]
    df = pd.DataFrame(rows, columns=columns)
    for key in _AXIS_INT_KEYS:
        df[key] = df[key].astype("Int64")
    for key in opt_axes:
        non_null = df[key].dropna()
        if len(non_null) and non_null.map(lambda v: isinstance(v, bool)).all():
            df[key] = df[key].astype("boolean")
    if apply_defaults:
        df = apply_axis_defaults(df)
    return df


def _resolve_paths(paths: Iterable[Path | str] | Path | str) -> list[Path]:
    """Accept a single path, a glob pattern, or any iterable of paths.

    A bare string containing ``*`` / ``?`` / ``[`` is expanded with
    :func:`glob.glob`; a bare string without those characters is treated as a
    single path. Iterables are passed through unchanged.
    """
    if isinstance(paths, (str, Path)):
        s = str(paths)
        if any(ch in s for ch in "*?["):
            expanded = sorted(glob.glob(s))
            if not expanded:
                raise FileNotFoundError(f"no files match {s!r}")
            return [Path(p) for p in expanded]
        return [Path(s)]
    return [Path(p) for p in paths]


def load(
    paths: Iterable[Path | str] | Path | str,
    criterion_root: Path | str = "target/criterion",
    *,
    apply_defaults: bool = True,
) -> pd.DataFrame:
    """Return the summary DataFrame for the given report JSON files.

    ``paths`` accepts a single path, a glob pattern (e.g.
    ``"bench-runs/*.json"``), or an iterable of paths. One row per
    ``(report × criterion_group)``. Columns: ``kind``, ``metric``, ``phase``,
    the axis columns (conventional + discovered optimization axes),
    ``mean_*``/``median_*`` estimates, plus
    ``criterion_group`` / ``criterion_function`` join keys into
    :func:`load_samples`.
    """
    reports = load_reports(_resolve_paths(paths))
    return _summary_from_reports(reports, criterion_root, apply_defaults=apply_defaults)


def _samples_from_reports(
    reports: Sequence[BenchReport],
    criterion_root: Path | str,
) -> pd.DataFrame:
    """Build the samples DataFrame from already-parsed reports.

    Factored out so callers can reuse it without re-reading paths.
    """
    rows = [
        {
            "criterion_group": gref.group,
            "criterion_function": gref.function,
            "sample_idx": idx,
            "iters": it,
            "total_ns": tot,
            "per_iter_ns": tot / it,
        }
        for _, gref, data in iter_function_data(reports, Path(criterion_root))
        for idx, (it, tot) in enumerate(zip(data.iters, data.times))
    ]
    return pd.DataFrame(rows)


def load_samples(
    paths: Iterable[Path | str] | Path | str,
    criterion_root: Path | str = "target/criterion",
) -> pd.DataFrame:
    """Return the per-iteration samples DataFrame.

    ``paths`` accepts the same forms as :func:`load`. One row per Criterion
    sample point. Join back to :func:`load` on ``(criterion_group,
    criterion_function)``.
    """
    reports = load_reports(_resolve_paths(paths))
    return _samples_from_reports(reports, criterion_root)
