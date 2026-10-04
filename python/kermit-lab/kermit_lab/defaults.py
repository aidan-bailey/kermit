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

# (axis column, data_structure) -> value to substitute for NaN on rows of
# that structure only. An axis that exists on one structure needs this: a
# structure-blind fill would stamp the value on rows that never had the axis.
SCOPED_AXIS_DEFAULTS: dict[tuple[str, str], object] = {
    # ColumnTrie's build before issue #84 inserted tuple by tuple. Every
    # ColumnTrie report since carries the axis ("bulk" by default).
    ("ds_build_mode", "ColumnTrie"): "incremental",
    # ColumnTrie's seek has been a binary search (`partition_point`) since
    # 525c99f (2026-03-02), before the first JSON report writer (f76344d,
    # 2026-04-26), so every ColumnTrie report without the axis ran `binary`.
    # TreeTrie has no entry: its seek was linear until 9604293 (#67) and
    # binary after, and a report cannot tell which side it came from (#80).
    ("ds_layout_seek", "ColumnTrie"): "binary",
}


def apply_axis_defaults(df: pd.DataFrame) -> pd.DataFrame:
    """Return ``df`` with documented axis defaults filled in for NaN cells.

    Only columns present in ``df`` are touched; absent columns are ignored.
    Scoped defaults fill only the rows of their data structure.
    Mutates a copy, leaving the caller's frame unchanged.
    """
    out = df.copy()
    for col, default in AXIS_DEFAULTS.items():
        if col in out.columns:
            out[col] = out[col].fillna(default)
    if "data_structure" in out.columns:
        for (col, data_structure), default in SCOPED_AXIS_DEFAULTS.items():
            if col in out.columns:
                # `isin`, not `==`: a missing data_structure must not match.
                on_structure = out["data_structure"].isin([data_structure])
                out[col] = out[col].mask(out[col].isna() & on_structure, default)
    return out
