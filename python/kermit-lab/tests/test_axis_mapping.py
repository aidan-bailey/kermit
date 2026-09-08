"""Column-aware colour/marker/linestyle lookups."""
from __future__ import annotations

from kermit_lab.axis_mapping import (
    DATA_STRUCTURE_COLOURS,
    colour_for,
    linestyle_for,
    marker_for,
)


def test_committed_ds_colours() -> None:
    assert colour_for("data_structure", "TreeTrie") == DATA_STRUCTURE_COLOURS["TreeTrie"]
    assert "HashTrie" in DATA_STRUCTURE_COLOURS  # newly committed


def test_unknown_value_is_stable() -> None:
    a = colour_for("ds_layout_hasher", "fx")
    b = colour_for("ds_layout_hasher", "fx")
    assert a == b  # stable within a process
    assert a != colour_for("ds_layout_hasher", "sip")  # distinct values differ


def test_marker_and_linestyle_fallback() -> None:
    assert marker_for("algorithm", "LeapfrogTriejoin") == "o"
    assert isinstance(marker_for("ds_config_load_factor", 0.5), str)
    assert isinstance(linestyle_for("algorithm", "Unknown"), str)
