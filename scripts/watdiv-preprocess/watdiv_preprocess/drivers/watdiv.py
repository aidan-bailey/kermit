"""WatDiv CLI driver — preserves the original `watdiv-preprocess` behaviour."""
from __future__ import annotations

import argparse
import functools
import sys
from pathlib import Path

from ..expected import write_watdiv_expected
from ..pipeline import run_pipeline


def main() -> int:
    p = argparse.ArgumentParser(
        description="Preprocess WatDiv N-Triples + SPARQL into kermit YAML + Parquet.",
    )
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

    sparql_files: list[Path] = []
    for stress_dir in sorted(args.input.glob("watdiv-stress-*")):
        if not stress_dir.is_dir():
            continue
        sparql_files.extend(sorted(stress_dir.glob("*.sparql")))

    run_pipeline(
        nt_path=args.input / args.nt_name,
        sparql_files=sparql_files,
        output_dir=args.output,
        base_url=args.base_url,
        expected_writer=functools.partial(write_watdiv_expected, args.input),
        log_prefix="watdiv-preprocess",
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
