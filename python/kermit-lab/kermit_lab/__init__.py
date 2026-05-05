"""Notebook-first analysis of kermit Criterion benchmark output.

Primary surface: :func:`load` and :func:`load_samples` return tidy pandas
DataFrames. The CLI in :mod:`kermit_lab.drivers.main` is a thin wrapper.
"""

SCHEMA_VERSION = 2
"""Highest BenchReport schema version this package can parse."""

from .frame import load, load_samples

__all__ = ["SCHEMA_VERSION", "load", "load_samples"]
