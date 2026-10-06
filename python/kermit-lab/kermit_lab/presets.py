"""Named presets: each fills an AestheticMap and calls kl.plot.

These are the legacy shapes, re-expressed as configurations of the general
engine. They exist for ergonomics and for the CLI / render-all to call.
"""
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Optional, Sequence

import pandas as pd
from matplotlib.figure import Figure

from .analysis import SPEEDUP_MEASURES, speedup_table
from .facet import finish, make_grid
from .loader import TIME_PHASES
from .plot import plot
from .plots_errors import InsufficientAxesError
from .styles import apply as apply_style


def scaling(df: pd.DataFrame, *, phase: str = "iteration", out: Optional[Path] = None) -> Figure:
    return plot(df, kind="line", x="tuples", y="time", colour="data_structure",
                style="algorithm", phase=phase, logx=True, logy=True,
                title="Scaling", out=out)


def bar_time(
    df: pd.DataFrame, *, query: str, phase: str = "iteration", out: Optional[Path] = None
) -> Figure:
    return plot(df, kind="bar", x="data_structure", y="time", colour="data_structure",
                style="algorithm", filter={"query": query}, phase=phase,
                title=f"Time — {query}", out=out)


def bar_space(df: pd.DataFrame, *, out: Optional[Path] = None) -> Figure:
    return plot(df, kind="bar", x="data_structure", y="space", colour="data_structure",
                title="Space per data structure", out=out)


def tradeoff(df: pd.DataFrame, *, phase: str = "iteration", out: Optional[Path] = None) -> Figure:
    return plot(df, kind="scatter", x="space", y="time", colour="data_structure",
                style="algorithm", phase=phase, logx=True, title="Space vs time", out=out)


def dist(
    df: pd.DataFrame, *, samples: pd.DataFrame, phase: str = "iteration",
    out: Optional[Path] = None,
) -> Figure:
    return plot(df, kind="violin", x="data_structure", y="time", style="algorithm",
                phase=phase, samples=samples, title="Per-iter distribution", out=out)


def bar_queries(
    df: pd.DataFrame, *, ds: Optional[Sequence[str]] = None,
    algo: Optional[Sequence[str]] = None, phase: str = "iteration",
    out: Optional[Path] = None,
) -> Figure:
    flt: dict[str, object] = {}
    if ds is not None:
        flt["data_structure"] = list(ds)
    if algo is not None:
        flt["algorithm"] = list(algo)
    return plot(df, kind="bar", x="query", y="time", colour="data_structure",
                style="algorithm", filter=flt, phase=phase,
                title="Across queries", out=out)


@dataclass(frozen=True)
class AxisScope:
    """The time phases an optimisation axis can affect, and why."""

    phases: frozenset[str]
    reason: str


# The time phases an optimisation axis can affect; an axis absent from this
# map can affect every phase. `ablation` refuses an axis on a phase outside
# its scope: two values there time the same code, so any difference between
# them is noise or binary drift, not the optimisation. Give a new axis an
# entry when its effect is confined to some phases.
#
# `ds_build_mode` has no entry. A BuildMode builds the same contents and
# capacities, so it cannot move `space` (not a time phase), but it may place
# keys in other slots of a hash table, and its allocations land elsewhere in
# memory, so it can move every time phase, `iteration` included (the
# optimization standard's Amendment 2, 2026-10-06).
AXIS_PHASES: dict[str, AxisScope] = {
    # A seek strategy changes how a built trie is searched; no build calls
    # seek (#80).
    "ds_layout_seek": AxisScope(
        frozenset({"iteration", "end_to_end"}),
        "it changes only how a built trie is searched",
    ),
}


def ablation(
    df: pd.DataFrame, *, axis: str, phase: str = "iteration", out: Optional[Path] = None
) -> Figure:
    """Ablation: time vs an optimization axis, coloured by DS, faceted by query when >1.

    Raises :class:`InsufficientAxesError` for an axis on a phase outside its
    :data:`AXIS_PHASES` scope.
    """
    scope = AXIS_PHASES.get(axis)
    if scope is not None and phase not in scope.phases:
        allowed = " or ".join(repr(p) for p in sorted(scope.phases))
        raise InsufficientAxesError(
            f"{axis} cannot affect phase {phase!r}: {scope.reason}; plot it on {allowed}"
        )
    # Rows without the axis belong to structures that do not have it. (An
    # unknown axis falls through to `plot`, which names the missing column.)
    if axis in df.columns:
        df = df[df[axis].notna()]
    facet = "query" if "query" in df.columns and df["query"].nunique(dropna=True) > 1 else None
    return plot(df, kind="bar", x=axis, y="time", colour="data_structure",
                facet=facet, phase=phase, title=f"Ablation — {axis}", out=out)


def speedup(
    df: pd.DataFrame, *, phase: str = "insertion", baseline: str = "serial",
    out: Optional[Path] = None,
) -> Figure:
    """Build speedup over the ``baseline`` build against thread count (#94).

    One line per case of :func:`~kermit_lab.analysis.speedup_table`, with its
    CI as a band, and the ideal ``speedup = N`` dashed. Both axes are log2, so
    each doubling of threads is one step and the ideal is a straight line. Load
    one binary's reports only (see :func:`~kermit_lab.analysis.speedup_table`).

    Raises :class:`InsufficientAxesError` for a phase that is not a time
    phase, or when no case has both a baseline and a ``parallel:N`` row. Every
    time phase is allowed: a build mode may move ``iteration`` too
    (Amendment 2).
    """
    if phase not in TIME_PHASES:
        allowed = " or ".join(repr(p) for p in TIME_PHASES)
        raise InsufficientAxesError(
            f"speedup compares time phases, not {phase!r}; plot it on {allowed}"
        )
    try:
        table = speedup_table(df, phase=phase, baseline=baseline)
    except ValueError as e:
        raise InsufficientAxesError(str(e)) from e

    apply_style()
    fig, axes = make_grid(1)
    ax = axes[0]
    identity = [c for c in table.columns if c not in SPEEDUP_MEASURES]
    varying = [c for c in identity if table[c].nunique(dropna=False) > 1] or ["data_structure"]
    for key, line in table.groupby(varying, dropna=False, sort=True):
        line = line.sort_values("threads")
        parts = key if isinstance(key, tuple) else (key,)
        label = " / ".join(str(p) for p in parts if not pd.isna(p))
        ax.plot(line["threads"], line["speedup"], marker="o", label=label)
        ax.fill_between(line["threads"], line["speedup_lo"], line["speedup_hi"], alpha=0.2)
    ticks = sorted(int(t) for t in table["threads"].unique())
    top = ticks[-1]
    ax.plot([1, top], [1, top], linestyle="--", color="grey", label="ideal")
    # Both axes log2: the ideal speedup = N is then the straight line through
    # (1, 1) and (top, top), and each doubling of threads is one step.
    ax.set_xscale("log", base=2)
    ax.set_yscale("log", base=2)
    ax.set_xticks(ticks, labels=[str(t) for t in ticks])
    ax.set_xlabel("threads")
    ax.set_ylabel(f"speedup over {baseline} ({phase})")
    finish(fig, axes, title="Parallel build speedup", out=out)
    return fig
