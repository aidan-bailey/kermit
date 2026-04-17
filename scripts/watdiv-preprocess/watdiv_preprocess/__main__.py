"""CLI entry point for the WatDiv preprocessor."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--input",
        required=True,
        type=Path,
        help="dir containing watdiv.10M.nt plus watdiv-stress-{100,1000}/ subdirs",
    )
    p.add_argument(
        "--output",
        required=True,
        type=Path,
        help="output artifacts dir (created if missing)",
    )
    p.add_argument(
        "--base-url",
        required=True,
        help="URL prefix written into generated YAML relation URLs",
    )
    p.add_argument(
        "--nt-name",
        default="watdiv.10M.nt",
        help="N-Triples filename under --input (default: watdiv.10M.nt)",
    )
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    print(
        f"[watdiv-preprocess] stub — input={args.input} output={args.output} "
        f"base_url={args.base_url} nt={args.nt_name}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
