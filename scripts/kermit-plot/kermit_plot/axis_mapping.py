"""Single source of truth for visual encoding of benchmark axes.

Adding a new data structure or algorithm means extending one of the
dictionaries here; no other module decides on colours or markers.

The colour palette is the 8-colour Wong / Okabe-Ito set, recommended for
colour-blind safety in publications. Reference:
https://www.nature.com/articles/nmeth.1618 (Wong, 2011).
"""
from __future__ import annotations

# Wong / Okabe-Ito 8-colour palette.
WONG_PALETTE: list[str] = [
    "#000000",  # black
    "#E69F00",  # orange
    "#56B4E9",  # sky blue
    "#009E73",  # bluish green
    "#F0E442",  # yellow
    "#0072B2",  # blue
    "#D55E00",  # vermilion
    "#CC79A7",  # reddish purple
]

# data_structure → colour. Reserved for kermit's two committed data
# structures; extend when adding a new IndexStructure.
DATA_STRUCTURE_COLOURS: dict[str, str] = {
    "TreeTrie": WONG_PALETTE[5],   # blue
    "ColumnTrie": WONG_PALETTE[6],  # vermilion
}

# algorithm → linestyle for line plots (scaling, dist).
ALGORITHM_LINESTYLES: dict[str, str] = {
    "LeapfrogTriejoin": "-",
}

# algorithm → marker for scatter / bar / tradeoff plots.
ALGORITHM_MARKERS: dict[str, str] = {
    "LeapfrogTriejoin": "o",
}

# Fallback colour for unknown DS values; rotates through the palette.
_UNKNOWN_COLOURS: list[str] = [c for c in WONG_PALETTE if c not in DATA_STRUCTURE_COLOURS.values()]


def colour_for_ds(ds: str) -> str:
    """Return the committed colour for ``ds``, or a stable fallback.

    Stable means: the same unknown DS string maps to the same fallback colour
    across calls within a process. We don't promise stability across the
    palette mutating between releases — committed mappings are the contract.
    """
    if ds in DATA_STRUCTURE_COLOURS:
        return DATA_STRUCTURE_COLOURS[ds]
    if not _UNKNOWN_COLOURS:
        return WONG_PALETTE[0]
    return _UNKNOWN_COLOURS[hash(ds) % len(_UNKNOWN_COLOURS)]


def linestyle_for_algo(algo: str) -> str:
    """Return the committed linestyle for ``algo``, or ``"--"`` for unknown."""
    return ALGORITHM_LINESTYLES.get(algo, "--")


def marker_for_algo(algo: str) -> str:
    """Return the committed marker for ``algo``, or ``"s"`` (square) for unknown.

    The fallback is intentionally a *filled* shape: matplotlib warns when an
    unfilled marker (``"x"``, ``"+"``) is given an ``edgecolor``, which our
    plotting code does for visibility on light backgrounds.
    """
    return ALGORITHM_MARKERS.get(algo, "s")
