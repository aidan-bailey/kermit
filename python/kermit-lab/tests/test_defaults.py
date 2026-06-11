"""Default back-fill for missing optimization axes."""
from __future__ import annotations

import pandas as pd

from kermit_lab.defaults import AXIS_DEFAULTS, apply_axis_defaults


def test_backfills_documented_default() -> None:
    df = pd.DataFrame({"ds_layout_hasher": [pd.NA, "fx"], "other": [1, 2]})
    out = apply_axis_defaults(df)
    assert out["ds_layout_hasher"].tolist() == ["sip", "fx"]
    assert AXIS_DEFAULTS["ds_layout_hasher"] == "sip"


def test_ignores_absent_columns() -> None:
    df = pd.DataFrame({"unrelated": [pd.NA]})
    out = apply_axis_defaults(df)
    assert out["unrelated"].isna().all()  # no crash, untouched
