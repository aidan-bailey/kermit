"""``render-all`` — emit every plot shape the input set supports.

Builds the summary + samples DataFrames once, then renders the fixed shapes,
per-query bar-time, per-(ds,algo) bar-queries, and — new — one ablation figure
per optimization axis carrying ≥2 distinct values. ``InsufficientAxesError`` is
demoted to an info log; other exceptions propagate. Skips are logged so
ablation coverage is never silently truncated.
"""
from __future__ import annotations

import logging
from pathlib import Path

from .. import presets
from ..frame import _samples_from_reports, _summary_from_reports, discover_opt_columns
from ..loader import BenchReport
from ..plots_errors import InsufficientAxesError

log = logging.getLogger(__name__)


def _ablation_axes(df) -> list[str]:
    """Optimization columns with ≥2 distinct non-null values."""
    out = []
    for col in discover_opt_columns(df):
        if df[col].nunique(dropna=True) >= 2:
            out.append(col)
        else:
            log.info("skipped ablation %s: <2 distinct values", col)
    return out


def render_all(
    reports: list[BenchReport],
    out_dir: Path,
    criterion_root: Path,
    fmt: str,
    *,
    phase: str = "iteration",
) -> None:
    """Render every applicable shape; skip with info-log on insufficient axes.

    ``phase`` is forwarded to every time-using shape (scaling, bar-time,
    bar-queries, tradeoff, dist) so insertion- and iteration-time can be
    rendered into separate output sets without re-running benchmarks.
    """
    suffix = f".{fmt}"
    df = _summary_from_reports(reports, criterion_root)
    samples = _samples_from_reports(reports, criterion_root)

    def _try(label: str, fn) -> None:
        try:
            fn()
            log.info("rendered %s", label)
        except InsufficientAxesError as e:
            log.info("skipped %s: %s", label, e)

    _try("scaling", lambda: presets.scaling(df, phase=phase, out=out_dir / f"scaling{suffix}"))
    _try("bar-space", lambda: presets.bar_space(df, out=out_dir / f"bar-space{suffix}"))
    _try("tradeoff", lambda: presets.tradeoff(df, phase=phase, out=out_dir / f"tradeoff{suffix}"))
    _try("dist", lambda: presets.dist(df, samples=samples, phase=phase,
                                      out=out_dir / f"dist{suffix}"))

    queries = sorted(df["query"].dropna().unique().tolist()) if "query" in df.columns else []
    for q in queries:
        _try(
            f"bar-time-{q}",
            lambda q=q: presets.bar_time(df, query=q, phase=phase,
                                         out=out_dir / f"bar-time-{q}{suffix}"),
        )

    if {"data_structure", "algorithm"} <= set(df.columns):
        pairs = sorted(
            df.dropna(subset=["data_structure", "algorithm"])
            .groupby(["data_structure", "algorithm"]).groups.keys()
        )
        for ds, algo in pairs:
            _try(
                f"bar-queries-{ds}-{algo}",
                lambda ds=ds, algo=algo: presets.bar_queries(
                    df, ds=[ds], algo=[algo], phase=phase,
                    out=out_dir / f"bar-queries-{ds}-{algo}{suffix}",
                ),
            )

    for axis in _ablation_axes(df):
        _try(
            f"ablation-{axis}",
            lambda axis=axis: presets.ablation(
                df, axis=axis, phase=phase, out=out_dir / f"ablation-{axis}{suffix}"
            ),
        )
