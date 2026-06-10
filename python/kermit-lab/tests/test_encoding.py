# tests/test_encoding.py
"""Aesthetic-map resolution: filter, select, facet, scalar series."""
from __future__ import annotations

import pandas as pd

from kermit_lab.encoding import (
    AestheticMap,
    apply_filter,
    facet_values,
    metric_of,
    resolve_scalar,
    select,
)


def _df():
    return pd.DataFrame(
        {
            "metric": ["time", "time", "space"],
            "phase": ["iteration", "iteration", pd.NA],
            "data_structure": ["TreeTrie", "ColumnTrie", "TreeTrie"],
            "algorithm": ["LeapfrogTriejoin", "LeapfrogTriejoin", "LeapfrogTriejoin"],
            "tuples": pd.array([10, 10, 10], dtype="Int64"),
            "query": ["triangle", "triangle", "triangle"],
            "mean_ns": [100.0, 200.0, 64.0],
            "mean_lo": [99.0, 198.0, 64.0],
            "mean_hi": [101.0, 202.0, 64.0],
        }
    )


def test_metric_of() -> None:
    assert metric_of("time") == "time"
    assert metric_of("space") == "space"


def test_select_filters_metric_and_phase() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="time")
    out = select(_df(), amap)
    assert set(out.metric) == {"time"}
    assert len(out) == 2


def test_apply_filter() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="time",
                        filter={"data_structure": "TreeTrie"})
    out = apply_filter(_df(), amap)
    assert set(out.data_structure) == {"TreeTrie"}


def test_resolve_scalar_one_series_per_colour() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="time", colour="data_structure")
    series = resolve_scalar(select(_df(), amap), amap)
    assert len(series) == 2
    labels = sorted(s.label for s in series)
    assert labels == ["ColumnTrie", "TreeTrie"]


def test_facet_values_none_when_unset() -> None:
    amap = AestheticMap(kind="bar", x="data_structure", y="time")
    assert facet_values(_df(), amap) == [None]
