"""``python -m kermit_plot`` entry; delegates to the argparse driver."""
from __future__ import annotations

import sys

from .drivers.main import main

if __name__ == "__main__":
    sys.exit(main())
