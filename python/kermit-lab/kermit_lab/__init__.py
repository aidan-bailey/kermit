"""Notebook-first analysis of kermit Criterion benchmark output.

Primary surface: :func:`load` / :func:`load_samples` return tidy DataFrames;
:func:`plot` renders any aesthetic; the named presets are thin configurations
of it. The CLI in :mod:`kermit_lab.drivers.main` is a thin wrapper.
"""

SCHEMA_VERSION = 2
"""Highest BenchReport schema version this package can parse."""

from .analysis import bootstrap_ratio_ci, compare, mannwhitney_u, summary
from .axis_mapping import colour_for, linestyle_for, marker_for
from .frame import discover_opt_columns, load, load_samples
from .plot import plot
from .plots_errors import InsufficientAxesError
from .presets import (
    ablation,
    bar_queries,
    bar_space,
    bar_time,
    dist,
    scaling,
    tradeoff,
)
from .styles import apply as apply_style

__all__ = [
    "SCHEMA_VERSION",
    "InsufficientAxesError",
    "ablation",
    "apply_style",
    "bar_queries",
    "bar_space",
    "bar_time",
    "bootstrap_ratio_ci",
    "colour_for",
    "compare",
    "discover_opt_columns",
    "dist",
    "linestyle_for",
    "load",
    "load_samples",
    "mannwhitney_u",
    "marker_for",
    "plot",
    "scaling",
    "summary",
    "tradeoff",
]
