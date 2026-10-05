"""Documented default values for optimization axes.

The loader leaves a missing optimization axis as NaN (sparsity is meaningful).
These registries record the *one* exception the schema mandates: reports that
predate an axis ran a known value, so back-filling recovers ground truth
rather than inventing it. Each default is documented here in one place — the
parser (`frame.py`) stays generic.

Every optimization-axis default is scoped to the data structure that has the
axis (`SCOPED_AXIS_DEFAULTS`). A row of any other structure keeps NaN, which
is what lets `presets.ablation` leave those structures out. A
structure-blind fill once stamped HashTrie's axes on TreeTrie and ColumnTrie
rows, so the ablations charted the sorted tries as "sip" (#85). The one
other registry, `JOIN_AXIS_DEFAULTS`, is scoped to join rows instead: it
holds planner axes every structure's joins share, and a `bench ds` row, which
joins nothing, keeps NaN.

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
    # Every HashTrie before issue #92 built all levels at construction.
    ("ds_layout_expansion", "HashTrie"): "eager",
    # Pre-Config HashTrie reports used the historical load factor.
    ("ds_config_load_factor", "HashTrie"): 0.7,
    # ColumnTrie's build before issue #84 inserted tuple by tuple. Every
    # ColumnTrie report since carries the axis ("bulk" by default).
    ("ds_build_mode", "ColumnTrie"): "incremental",
    # HashTrie built one insert per tuple before issue #91. Every HashTrie
    # report since carries the axis ("serial" by default).
    ("ds_build_mode", "HashTrie"): "serial",
    # ColumnTrie's seek has been a binary search (`partition_point`) since
    # 525c99f (2026-03-02), before the first JSON report writer (7dbdcaf,
    # 2026-04-26), so every ColumnTrie report without the axis ran `binary`.
    # TreeTrie has no entry: its seek was linear until 9604293 (#67) and
    # binary after, and a report cannot tell which side it came from (#80).
    ("ds_layout_seek", "ColumnTrie"): "binary",
}


# Axis -> value to substitute for NaN on *join* rows only (rows with an
# `algorithm`): a `bench ds` row joins nothing and has no such axis. Unlike
# the scoped registry this is structure-blind by design, because every
# structure's joins ran the same policy before the axis existed.
JOIN_AXIS_DEFAULTS: dict[str, object] = {
    # Every join before #93 read relations in their stored column order.
    "column_orders": "stored",
}


def apply_axis_defaults(df: pd.DataFrame) -> pd.DataFrame:
    """Return ``df`` with documented axis defaults filled in for NaN cells.

    Each optimization-axis default fills only the rows of its data
    structure, and each join default (:data:`JOIN_AXIS_DEFAULTS`) only rows
    whose ``algorithm`` is set, so a frame without the column a default keys
    on is left alone by it. Only columns present in ``df`` are touched;
    absent columns are ignored. Mutates a copy, leaving the caller's frame
    unchanged.
    """
    out = df.copy()
    if "algorithm" in out.columns:
        joined = out["algorithm"].notna()
        for col, default in JOIN_AXIS_DEFAULTS.items():
            if col in out.columns:
                out[col] = out[col].mask(out[col].isna() & joined, default)
    if "data_structure" not in out.columns:
        return out
    for (col, data_structure), default in SCOPED_AXIS_DEFAULTS.items():
        if col in out.columns:
            # `isin`, not `==`: a missing data_structure must not match.
            on_structure = out["data_structure"].isin([data_structure])
            out[col] = out[col].mask(out[col].isna() & on_structure, default)
    return out
