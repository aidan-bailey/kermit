"""Generic SPARQL workload driver — any N-Triples + SPARQL set.

Unlike `drivers.watdiv`, this driver makes no assumptions about the
input directory layout: the caller supplies the NT path explicitly,
plus one or more glob patterns (relative to ``--input``) that select
SPARQL files. Expected cardinalities, if wanted, must come as a
pre-computed JSON file passed via ``--expected-json`` — the driver
copies it verbatim into ``<output>/expected.json``.
"""
from __future__ import annotations

import argparse
import shutil
import sys
from pathlib import Path
from typing import Callable

from ..pipeline import run_pipeline

DEFAULT_SPARQL_GLOB = "**/*.sparql"


def _make_expected_writer(source: Path) -> Callable[[Path], int]:
    """Returns a writer that copies ``source`` to the pipeline's output path.

    We count entries by reading the JSON so the returned count matches
    what a downstream consumer will actually see — a malformed file
    would fail here rather than silently copying and lying about
    the count.
    """
    import json

    def writer(out_path: Path) -> int:
        shutil.copyfile(source, out_path)
        data = json.loads(out_path.read_text(encoding="utf-8"))
        if not isinstance(data, dict):
            raise ValueError(
                f"--expected-json {source} must contain a JSON object, got {type(data).__name__}"
            )
        return len(data)

    return writer


def main() -> int:
    p = argparse.ArgumentParser(
        description="Preprocess arbitrary SPARQL + N-Triples into kermit YAML + Parquet.",
    )
    p.add_argument(
        "--input",
        required=True,
        type=Path,
        help="root dir for SPARQL files; --sparql-glob patterns are relative to this",
    )
    p.add_argument(
        "--nt",
        required=True,
        type=Path,
        help="N-Triples file (one triple per line); pass explicitly so it can live anywhere",
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
        "--sparql-glob",
        action="append",
        default=[],
        help=(
            "glob pattern for SPARQL files relative to --input; repeatable. "
            f"Default if none given: {DEFAULT_SPARQL_GLOB!r}"
        ),
    )
    p.add_argument(
        "--expected-json",
        type=Path,
        default=None,
        help="optional pre-computed expected.json to copy into --output",
    )
    args = p.parse_args()

    globs = args.sparql_glob or [DEFAULT_SPARQL_GLOB]
    sparql_files: list[Path] = []
    for g in globs:
        sparql_files.extend(sorted(args.input.glob(g)))

    expected_writer = (
        _make_expected_writer(args.expected_json) if args.expected_json is not None else None
    )

    run_pipeline(
        nt_path=args.nt,
        sparql_files=sparql_files,
        output_dir=args.output,
        base_url=args.base_url,
        expected_writer=expected_writer,
        log_prefix="sparql-preprocess",
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
