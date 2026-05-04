"""Style loader. ``thesis.mplstyle`` ships as package data; load via :func:`apply`."""
from __future__ import annotations

from importlib import resources

import matplotlib.pyplot as plt


def apply() -> None:
    """Apply the bundled ``thesis.mplstyle`` to the current matplotlib state.

    Must be called *before* any seaborn calls, since ``sns.set_theme()`` would
    otherwise overwrite our settings.
    """
    style_path = resources.files(__package__).joinpath("thesis.mplstyle")
    with resources.as_file(style_path) as path:
        plt.style.use(str(path))
