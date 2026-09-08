"""Documented default values for optimization axes.

The loader leaves a missing optimization axis as NaN (sparsity is meaningful).
This registry records the *one* exception the schema mandates: pre-standard
reports that predate an axis ran a known value, so back-filling recovers
ground truth rather than inventing it. Each optimization documents its default
here in one place — the parser (`frame.py`) stays generic.

See `docs/specs/bench-report-schema.md` ("Standard axis prefixes").
"""
from __future__ import annotations

import pandas as pd

# axis column name -> value to substitute for NaN.
AXIS_DEFAULTS: dict[str, object] = {
    "ds_layout_hasher": "sip",  # HashTrie's historical hash function
    "ds_layout_pruning": "off",  # pre-Layout reports never pruned
    "ds_config_load_factor": 0.7,  # pre-Config reports used the historical load factor
}


def apply_axis_defaults(df: pd.DataFrame) -> pd.DataFrame:
    """Return ``df`` with documented axis defaults filled in for NaN cells.

    Only columns present in ``df`` are touched; absent columns are ignored.
    Mutates a copy, leaving the caller's frame unchanged.
    """
    out = df.copy()
    for col, default in AXIS_DEFAULTS.items():
        if col in out.columns:
            out[col] = out[col].fillna(default)
    return out
