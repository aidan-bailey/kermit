"""Default back-fill for missing optimization axes."""
from __future__ import annotations

import pandas as pd

from kermit_lab.defaults import AXIS_DEFAULTS, SCOPED_AXIS_DEFAULTS, apply_axis_defaults


def test_backfills_documented_default() -> None:
    df = pd.DataFrame({"ds_layout_hasher": [pd.NA, "fx"], "other": [1, 2]})
    out = apply_axis_defaults(df)
    assert out["ds_layout_hasher"].tolist() == ["sip", "fx"]
    assert AXIS_DEFAULTS["ds_layout_hasher"] == "sip"


def test_ignores_absent_columns() -> None:
    df = pd.DataFrame({"unrelated": [pd.NA]})
    out = apply_axis_defaults(df)
    assert out["unrelated"].isna().all()  # no crash, untouched


def test_pruning_layout_defaults_to_off() -> None:
    df = pd.DataFrame({"ds_layout_pruning": [pd.NA, "on"]})
    out = apply_axis_defaults(df)
    assert out["ds_layout_pruning"].tolist() == ["off", "on"]
    assert AXIS_DEFAULTS["ds_layout_pruning"] == "off"


def test_load_factor_defaults_to_seventy_percent() -> None:
    df = pd.DataFrame({"ds_config_load_factor": [pd.NA, 0.5]})
    out = apply_axis_defaults(df)
    assert out["ds_config_load_factor"].tolist() == [0.7, 0.5]
    assert AXIS_DEFAULTS["ds_config_load_factor"] == 0.7
    assert "ds_config_singleton_pruning" not in AXIS_DEFAULTS


def test_build_mode_backfills_column_trie_rows_only() -> None:
    df = pd.DataFrame({
        "data_structure": ["ColumnTrie", "ColumnTrie", "TreeTrie", "HashTrie"],
        "ds_build_mode": [pd.NA, "bulk", pd.NA, pd.NA],
    })
    out = apply_axis_defaults(df)
    assert out["ds_build_mode"].iloc[0] == "incremental"
    assert out["ds_build_mode"].iloc[1] == "bulk"
    assert out["ds_build_mode"].iloc[2:].isna().all()
    assert SCOPED_AXIS_DEFAULTS[("ds_build_mode", "ColumnTrie")] == "incremental"
    assert "ds_build_mode" not in AXIS_DEFAULTS


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
    assert pd.isna(out["ds_build_mode"].iloc[1])


def test_scoped_default_skips_rows_with_no_data_structure() -> None:
    df = pd.DataFrame({
        "data_structure": pd.Series(["ColumnTrie", pd.NA], dtype="string"),
        "ds_build_mode": pd.Series([pd.NA, pd.NA], dtype="object"),
    })
    out = apply_axis_defaults(df)
    assert out["ds_build_mode"].iloc[0] == "incremental"
    assert pd.isna(out["ds_build_mode"].iloc[1])


def test_scoped_default_needs_the_data_structure_column() -> None:
    df = pd.DataFrame({"ds_build_mode": [pd.NA]})
    out = apply_axis_defaults(df)
    assert out["ds_build_mode"].isna().all()  # no crash, untouched
