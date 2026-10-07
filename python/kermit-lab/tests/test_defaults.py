"""Default back-fill for missing optimization axes."""
from __future__ import annotations

import pandas as pd
import pytest

from kermit_lab import defaults
from kermit_lab.defaults import (
    SCOPED_AXIS_DEFAULTS,
    apply_axis_defaults,
    apply_renamed_axis_values,
)

# The `data_structure` labels a report can carry, pinned on the Rust side by
# `IndexStructure::axis_value`.
STRUCTURES = ("TreeTrie", "ColumnTrie", "HashTrie")


def test_ignores_absent_columns() -> None:
    df = pd.DataFrame({"unrelated": [pd.NA]})
    out = apply_axis_defaults(df)
    assert out["unrelated"].isna().all()  # no crash, untouched


@pytest.mark.parametrize(
    ("axis", "default", "explicit"),
    [
        # HashTrie's historical hash function.
        ("ds_layout_hasher", "sip", "fx"),
        # Pre-Layout reports never pruned.
        ("ds_layout_pruning", "off", "on"),
        # Pre-Config reports used the historical load factor.
        ("ds_config_load_factor", 0.7, 0.5),
        # Every HashTrie root grew from 4 buckets before #88.
        ("ds_config_root_capacity", "grow", "tuples"),
        # Pre-#92 reports built every level eagerly.
        ("ds_layout_expansion", "eager", "lazy"),
    ],
)
def test_hash_trie_axes_backfill_hash_trie_rows_only(axis, default, explicit) -> None:
    """The sorted tries have no hasher, pruning, expansion or load factor, so a fill
    there would chart them under a HashTrie setting (#85)."""
    df = pd.DataFrame({
        "data_structure": ["HashTrie", "HashTrie", "TreeTrie", "ColumnTrie"],
        axis: [pd.NA, explicit, pd.NA, pd.NA],
    })
    out = apply_axis_defaults(df)
    assert out[axis].iloc[0] == default
    assert out[axis].iloc[1] == explicit
    assert out[axis].iloc[2:].isna().all()
    assert SCOPED_AXIS_DEFAULTS[(axis, "HashTrie")] == default


def test_singleton_pruning_config_axis_has_no_default() -> None:
    """Pruning was briefly a Config (`ds_config_singleton_pruning`); it is a
    Layout now, back-filled through `ds_layout_pruning` instead."""
    assert all(axis != "ds_config_singleton_pruning" for axis, _ in SCOPED_AXIS_DEFAULTS)


def test_every_default_fills_only_its_structure() -> None:
    """A cell is filled iff the registry names its (axis, structure) pair.
    Rows of every other structure, an unknown one or none at all stay NaN."""
    labels = [*STRUCTURES, "SomeFutureTrie", pd.NA]
    for axis in {axis for axis, _ in SCOPED_AXIS_DEFAULTS}:
        df = pd.DataFrame({
            "data_structure": pd.Series(labels, dtype="object"),
            axis: pd.Series([pd.NA] * len(labels), dtype="object"),
        })
        out = apply_axis_defaults(df)
        for label, value in zip(labels, out[axis]):
            expected = SCOPED_AXIS_DEFAULTS.get((axis, label)) if label is not pd.NA else None
            if expected is None:
                assert pd.isna(value), (axis, label, value)
            else:
                assert value == expected, (axis, label, value)


def test_every_default_names_a_known_structure() -> None:
    """A misspelt structure would match no row, silently disabling its fill."""
    for axis, data_structure in SCOPED_AXIS_DEFAULTS:
        assert data_structure in STRUCTURES, (axis, data_structure)


def test_the_registries_are_scoped() -> None:
    """A structure-blind registry stamps an axis on every structure (#85):
    an optimization-axis default goes in `SCOPED_AXIS_DEFAULTS`. Of the
    registries beside it, `JOIN_AXIS_DEFAULTS` is scoped to join rows (those
    with an `algorithm`), for planner axes every structure's joins share, and
    `BINARY_AXIS_DEFAULTS` fills every row, for properties of the binary that
    wrote the report. Neither may hold an optimization axis.
    `RENAMED_AXIS_VALUES` holds no default at all: it rewrites a value."""
    registries = sorted(
        name for name, value in vars(defaults).items()
        if name.isupper() and isinstance(value, dict)
    )
    assert registries == [
        "BINARY_AXIS_DEFAULTS", "JOIN_AXIS_DEFAULTS", "RENAMED_AXIS_VALUES",
        "SCOPED_AXIS_DEFAULTS",
    ]
    for registry in (defaults.JOIN_AXIS_DEFAULTS, defaults.BINARY_AXIS_DEFAULTS):
        assert not any(axis.startswith(("ds_", "algo_")) for axis in registry)


def test_allocator_backfills_every_row_that_lacks_it() -> None:
    """Every report before #112 ran on the system allocator, whatever its
    kind or structure, so the fill ignores both. A row that carries the axis
    keeps it. An all-NaN float column (only pre-#112 rows) is filled too."""
    df = pd.DataFrame({
        "data_structure": ["TreeTrie", "HashTrie", "ColumnTrie", pd.NA],
        "algorithm": [pd.NA, "HashTriejoin", "LeapfrogTriejoin", pd.NA],
        "allocator": [pd.NA, "jemalloc", pd.NA, pd.NA],
    })
    out = apply_axis_defaults(df)
    assert out["allocator"].tolist() == ["system", "jemalloc", "system", "system"]
    assert defaults.BINARY_AXIS_DEFAULTS["allocator"] == "system"

    old = pd.DataFrame({
        "data_structure": ["TreeTrie"],
        "allocator": pd.Series([float("nan")]),
    })
    assert apply_axis_defaults(old)["allocator"].tolist() == ["system"]


def test_build_mode_backfills_each_structures_pre_axis_build() -> None:
    """Each structure back-fills its own pre-axis build: ColumnTrie's pre-#84
    ``incremental``, TreeTrie's pre-#94 ``serial`` and HashTrie's pre-#91
    per-tuple build, ``incremental`` since #107. A row that carries the axis
    keeps it."""
    df = pd.DataFrame({
        "data_structure": [
            "ColumnTrie", "ColumnTrie", "TreeTrie", "TreeTrie", "HashTrie", "HashTrie",
        ],
        "ds_build_mode": [pd.NA, "bulk", pd.NA, "parallel:4", pd.NA, "radix:8"],
    })
    out = apply_axis_defaults(df)
    assert out["ds_build_mode"].tolist() == [
        "incremental", "bulk", "serial", "parallel:4", "incremental", "radix:8",
    ]
    assert SCOPED_AXIS_DEFAULTS[("ds_build_mode", "ColumnTrie")] == "incremental"
    assert SCOPED_AXIS_DEFAULTS[("ds_build_mode", "TreeTrie")] == "serial"
    assert SCOPED_AXIS_DEFAULTS[("ds_build_mode", "HashTrie")] == "incremental"


def test_renamed_values_rewrite_hash_tries_old_serial_only() -> None:
    """HashTrie's per-tuple build was ``serial`` until #107 and is
    ``incremental`` since; TreeTrie's ``serial`` is another build."""
    df = pd.DataFrame({
        "data_structure": ["HashTrie", "HashTrie", "TreeTrie", "HashTrie"],
        "ds_build_mode": ["serial", "bulk", "serial", pd.NA],
    })
    out = apply_renamed_axis_values(df)
    assert out["ds_build_mode"].tolist()[:3] == ["incremental", "bulk", "serial"]
    assert pd.isna(out["ds_build_mode"].iloc[3])
    assert df["ds_build_mode"].iloc[0] == "serial", "the caller's frame is untouched"


def test_child_capacity_backfills_grow_on_hash_trie_only() -> None:
    df = pd.DataFrame({
        "data_structure": ["HashTrie", "TreeTrie"],
        "ds_config_child_capacity": [pd.NA, pd.NA],
    })
    out = apply_axis_defaults(df)
    assert out["ds_config_child_capacity"].iloc[0] == "grow"
    assert pd.isna(out["ds_config_child_capacity"].iloc[1])


def test_build_mode_backfills_an_all_nan_float_column() -> None:
    """A frame with no ds_build_mode value at all (only pre-#84 ColumnTrie rows
    plus other structures) holds a float64 NaN column, which a ``.loc`` fill
    rejects once pandas stops upcasting."""
    df = pd.DataFrame({
        "data_structure": ["ColumnTrie", "TreeTrie"],
        "ds_build_mode": pd.Series([float("nan")] * 2),
    })
    assert df["ds_build_mode"].dtype == "float64"
    out = apply_axis_defaults(df)
    assert out["ds_build_mode"].iloc[0] == "incremental"
    assert out["ds_build_mode"].iloc[1] == "serial"


def test_scoped_default_skips_rows_with_no_data_structure() -> None:
    df = pd.DataFrame({
        "data_structure": pd.Series(["ColumnTrie", pd.NA], dtype="string"),
        "ds_build_mode": pd.Series([pd.NA, pd.NA], dtype="object"),
    })
    out = apply_axis_defaults(df)
    assert out["ds_build_mode"].iloc[0] == "incremental"
    assert pd.isna(out["ds_build_mode"].iloc[1])


def test_scoped_default_needs_the_data_structure_column() -> None:
    df = pd.DataFrame({axis: [pd.NA] for axis, _ in SCOPED_AXIS_DEFAULTS})
    out = apply_axis_defaults(df)
    assert out.isna().all().all()  # no crash, untouched


def test_seek_backfills_column_trie_rows_only() -> None:
    df = pd.DataFrame({
        "data_structure": ["ColumnTrie", "ColumnTrie", "TreeTrie", "HashTrie"],
        "ds_layout_seek": [pd.NA, "galloping", pd.NA, pd.NA],
    })
    out = apply_axis_defaults(df)
    assert out["ds_layout_seek"].iloc[0] == "binary"
    assert out["ds_layout_seek"].iloc[1] == "galloping"
    # TreeTrie's seek was linear before #67 and binary after it, and a
    # report cannot tell which; HashTrie has no seek. Both stay unlabelled.
    assert out["ds_layout_seek"].iloc[2:].isna().all()
    assert SCOPED_AXIS_DEFAULTS[("ds_layout_seek", "ColumnTrie")] == "binary"
    assert ("ds_layout_seek", "TreeTrie") not in SCOPED_AXIS_DEFAULTS


def test_column_orders_backfills_join_rows_only() -> None:
    """Every join before #93 ran `stored`; a `bench ds` row joins nothing
    and keeps NaN."""
    df = pd.DataFrame({
        "data_structure": ["TreeTrie", "HashTrie", "ColumnTrie", "TreeTrie"],
        "algorithm": ["LeapfrogTriejoin", "HashTriejoin", pd.NA, "LeapfrogTriejoin"],
        "column_orders": [pd.NA, "any", pd.NA, pd.NA],
    })
    out = apply_axis_defaults(df)
    assert out["column_orders"].iloc[0] == "stored"
    assert out["column_orders"].iloc[1] == "any"
    assert pd.isna(out["column_orders"].iloc[2])
    assert out["column_orders"].iloc[3] == "stored"
    assert defaults.JOIN_AXIS_DEFAULTS == {"column_orders": "stored"}


def test_column_orders_backfill_needs_the_algorithm_column() -> None:
    df = pd.DataFrame({"data_structure": ["TreeTrie"], "column_orders": [pd.NA]})
    out = apply_axis_defaults(df)
    assert pd.isna(out["column_orders"].iloc[0])


def test_join_defaults_apply_without_a_data_structure_column() -> None:
    """The join-row fill keys off `algorithm` alone, so it does not depend on
    the structure-scoped fill's early return."""
    df = pd.DataFrame({"algorithm": ["HashTriejoin"], "column_orders": [pd.NA]})
    out = apply_axis_defaults(df)
    assert out["column_orders"].iloc[0] == "stored"
