"""Shared plotting errors (kept separate so the engine doesn't import plots/)."""
from __future__ import annotations


class InsufficientAxesError(ValueError):
    """The input reports lack the axis values a plot shape requires."""
