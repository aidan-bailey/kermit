"""CLI entry point for the WatDiv preprocessor."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .dict_builder import build_dict
from .partitioner import partition_triples


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

    nt_path = args.input / args.nt_name
    uri_to_id = build_dict(nt_path, args.output)
    print(
        f"[watdiv-preprocess] dictionary: {len(uri_to_id)} terms written to {args.output}",
        file=sys.stderr,
    )

    filename_map = partition_triples(nt_path, uri_to_id, args.output)
    print(
        f"[watdiv-preprocess] partitioned into {len(filename_map)} predicate Parquet files",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
