"""CLI entry point for the WatDiv preprocessor."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .dict_builder import build_dict
from .expected import write_watdiv_expected
from .partitioner import partition_triples
from .yaml_emitter import emit_yaml


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

    predicate_map = partition_triples(nt_path, uri_to_id, args.output)
    print(
        f"[watdiv-preprocess] partitioned into {len(set(predicate_map.values()))} "
        f"predicate Parquet files",
        file=sys.stderr,
    )

    emitted = 0
    for d in sorted(args.input.glob("watdiv-stress-*")):
        if not d.is_dir():
            continue
        for sparql_file in sorted(d.glob("*.sparql")):
            out = emit_yaml(sparql_file, args.output, uri_to_id, args.base_url, predicate_map)
            emitted += 1
            print(f"[watdiv-preprocess] wrote {out}", file=sys.stderr)
    print(f"[watdiv-preprocess] emitted {emitted} benchmark YAML files", file=sys.stderr)

    expected_path = args.output / "expected.json"
    n_expected = write_watdiv_expected(args.input, expected_path)
    print(
        f"[watdiv-preprocess] wrote {n_expected} expected cardinalities to {expected_path}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
