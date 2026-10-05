"""Documented default values for optimization axes.

The loader leaves a missing optimization axis as NaN (sparsity is meaningful).
This registry records the *one* exception the schema mandates: pre-standard
reports that predate an axis ran a known value, so back-filling recovers
ground truth rather than inventing it. Each optimization documents its default
here in one place — the parser (`frame.py`) stays generic.

Every default is scoped to the data structure that has the axis. A row of
any other structure keeps NaN, which is what lets `presets.ablation` leave
those structures out. A structure-blind fill once stamped HashTrie's axes on
TreeTrie and ColumnTrie rows, so the ablations charted the sorted tries as
"sip" (#85).

See `docs/specs/bench-report-schema.md` ("Standard axis prefixes").
"""
from __future__ import annotations

import pandas as pd

# (axis column, data_structure) -> value to substitute for NaN on rows of
# that structure only.
SCOPED_AXIS_DEFAULTS: dict[tuple[str, str], object] = {
    # HashTrie's historical hash function.
    ("ds_layout_hasher", "HashTrie"): "sip",
    # Pre-Layout HashTrie reports never pruned.
    ("ds_layout_pruning", "HashTrie"): "off",
    # Pre-Config HashTrie reports used the historical load factor.
    ("ds_config_load_factor", "HashTrie"): 0.7,
    # ColumnTrie's build before issue #84 inserted tuple by tuple. Every
    # ColumnTrie report since carries the axis ("bulk" by default).
    ("ds_build_mode", "ColumnTrie"): "incremental",
    # TreeTrie built serially until #94 added `--ds-build parallel:N`. Every
    # TreeTrie report since carries the axis ("serial" by default).
    ("ds_build_mode", "TreeTrie"): "serial",
    # ColumnTrie's seek has been a binary search (`partition_point`) since
    # 525c99f (2026-03-02), before the first JSON report writer (7dbdcaf,
    # 2026-04-26), so every ColumnTrie report without the axis ran `binary`.
    # TreeTrie has no entry: its seek was linear until 9604293 (#67) and
    # binary after, and a report cannot tell which side it came from (#80).
    ("ds_layout_seek", "ColumnTrie"): "binary",
}


def apply_axis_defaults(df: pd.DataFrame) -> pd.DataFrame:
    """Return ``df`` with documented axis defaults filled in for NaN cells.

    Each default fills only the rows of its data structure, so a frame
    without a ``data_structure`` column is returned unchanged. Only columns
    present in ``df`` are touched; absent columns are ignored. Mutates a
    copy, leaving the caller's frame unchanged.
    """
    out = df.copy()
    if "data_structure" not in out.columns:
        return out
    for (col, data_structure), default in SCOPED_AXIS_DEFAULTS.items():
        if col in out.columns:
            # `isin`, not `==`: a missing data_structure must not match.
            on_structure = out["data_structure"].isin([data_structure])
            out[col] = out[col].mask(out[col].isna() & on_structure, default)
    return out
