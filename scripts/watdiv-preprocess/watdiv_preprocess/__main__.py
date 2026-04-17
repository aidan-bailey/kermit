"""Back-compat entry for `python -m watdiv_preprocess`; delegates to the WatDiv driver."""
from __future__ import annotations

import sys

from .drivers.watdiv import main

if __name__ == "__main__":
    sys.exit(main())
