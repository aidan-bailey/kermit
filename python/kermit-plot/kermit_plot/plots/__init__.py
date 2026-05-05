"""Plot modules. Each exports a ``render(...)`` taking parsed reports + a Path.

All plot modules follow the same shape:

- Required input: a ``list[BenchReport]`` (already parsed) plus a
  ``criterion_root: Path`` for resolving on-disk artefacts.
- Required output: a non-empty file at ``out_path`` (suffix determines
  format).
- Plot-specific kwargs: documented per module. Missing required axes raise
  :class:`InsufficientAxesError`.

The orchestration (style application, figure save, format handling) lives in
``kermit_plot.drivers.main``; plot modules only build the figure.
"""
from __future__ import annotations


class InsufficientAxesError(ValueError):
    """The input reports lack the axis values this plot shape requires."""
