"""Notebook-first analysis of kermit Criterion benchmark output.

Primary surface: :func:`load` and :func:`load_samples` return tidy pandas
DataFrames. Plot functions take a DataFrame and return a
:class:`matplotlib.figure.Figure`. The CLI in :mod:`kermit_lab.drivers.main`
is a thin wrapper.
"""

SCHEMA_VERSION = 2
"""Highest BenchReport schema version this package can parse."""

from .frame import load, load_samples
from .plots.bar_queries import plot as bar_queries
from .plots.bar_space import plot as bar_space
from .plots.bar_time import plot as bar_time
from .plots.dist import plot as dist
from .plots.scaling import plot as scaling
from .plots.tradeoff import plot as tradeoff
from .styles import apply as apply_style

__all__ = [
    "SCHEMA_VERSION",
    "apply_style",
    "bar_queries",
    "bar_space",
    "bar_time",
    "dist",
    "load",
    "load_samples",
    "scaling",
    "tradeoff",
]
