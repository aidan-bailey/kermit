"""Axis mapping is the single source of truth for colours/markers — tests pin its contract."""
from __future__ import annotations

from kermit_lab.axis_mapping import (
    DATA_STRUCTURE_COLOURS,
    WONG_PALETTE,
    colour_for_ds,
    linestyle_for_algo,
    marker_for_algo,
)


def test_palette_is_eight_distinct_hex_colours() -> None:
    assert len(WONG_PALETTE) == 8
    assert len(set(WONG_PALETTE)) == 8
    for c in WONG_PALETTE:
        assert c.startswith("#")
        assert len(c) == 7


def test_committed_data_structures_have_distinct_palette_colours() -> None:
    assert "TreeTrie" in DATA_STRUCTURE_COLOURS
    assert "ColumnTrie" in DATA_STRUCTURE_COLOURS
    colours = list(DATA_STRUCTURE_COLOURS.values())
    assert len(set(colours)) == len(colours)
    for c in colours:
        assert c in WONG_PALETTE


def test_unknown_ds_falls_back_stably() -> None:
    a = colour_for_ds("MysteryStruct")
    b = colour_for_ds("MysteryStruct")
    assert a == b
    assert a in WONG_PALETTE


def test_known_algo_returns_committed_linestyle_and_marker() -> None:
    assert linestyle_for_algo("LeapfrogTriejoin") == "-"
    assert marker_for_algo("LeapfrogTriejoin") == "o"


def test_unknown_algo_returns_visible_fallback() -> None:
    assert linestyle_for_algo("NotARealAlgo") == "--"
    # "s" (filled square) is intentional — see marker_for_algo docstring.
    assert marker_for_algo("NotARealAlgo") == "s"
