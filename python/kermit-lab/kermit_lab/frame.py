"""Build pandas DataFrames from ``BenchReport`` JSON + Criterion artefacts.

The primary public surface for notebook users: ``load()`` returns the
summary frame (one row per report+function), ``load_samples()`` returns the
per-iteration samples frame. Both are tidy long-form — call ``.groupby``,
``.pivot_table``, ``.query`` directly.

The dataclasses in :mod:`.loader` and :mod:`.criterion` are still the
internal currency (plot modules build on them); ``frame`` is the layer that
exposes them as DataFrames.
"""
from __future__ import annotations

from pathlib import Path
from typing import Iterable, Sequence

import pandas as pd

from .criterion import FunctionData
from .loader import BenchReport, CriterionGroupRef, iter_function_data, load_reports, phase_of

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

# Fixed column order for the summary frame. Used by ``pd.DataFrame(rows,
# columns=...)`` so the schema is consistent even when ``rows`` is empty.
_SUMMARY_COLUMNS: tuple[str, ...] = (
    "kind", "metric", "phase",
    *_AXIS_STR_KEYS,
    *_AXIS_INT_KEYS,
    "mean_ns", "mean_lo", "mean_hi", "mean_se",
    "median_ns", "median_lo", "median_hi",
    "source_path", "criterion_group", "criterion_function",
)


def _summary_row(
    report: BenchReport,
    group_ref: CriterionGroupRef,
    data: FunctionData,
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


def _summary_from_reports(
    reports: Sequence[BenchReport],
    criterion_root: Path | str,
) -> pd.DataFrame:
    """Build the summary DataFrame from already-parsed reports.

    Used by plot-module shims to avoid re-parsing JSON when callers pass
    ``list[BenchReport]`` through the legacy ``render(...)`` API.
    """
    rows = [
        _summary_row(report, gref, data)
        for report, gref, data in iter_function_data(reports, Path(criterion_root))
    ]
    df = pd.DataFrame(rows, columns=list(_SUMMARY_COLUMNS))
    for key in _AXIS_INT_KEYS:
        df[key] = df[key].astype("Int64")
    return df


def load(
    paths: Iterable[Path | str],
    criterion_root: Path | str = "target/criterion",
) -> pd.DataFrame:
    """Return the summary DataFrame for the given report JSON files.

    One row per ``(report × criterion_group)``. Columns: ``kind``, ``metric``,
    ``phase``, the axis columns, ``mean_*``/``median_*`` estimates, plus
    ``criterion_group`` / ``criterion_function`` join keys into
    :func:`load_samples`.
    """
    reports = load_reports([Path(p) for p in paths])
    return _summary_from_reports(reports, criterion_root)


def load_samples(
    paths: Iterable[Path | str],
    criterion_root: Path | str = "target/criterion",
) -> pd.DataFrame:
    """Return the per-iteration samples DataFrame.

    One row per Criterion sample point. Join back to :func:`load` on
    ``(criterion_group, criterion_function)``.
    """
    reports = load_reports([Path(p) for p in paths])
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
