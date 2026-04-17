"""Harvests per-query cardinalities from WatDiv-style `.desc` sidecars.

Each `.desc` file holds one integer per line, in the same order as the
queries in the sibling `.sparql` file. The generic API takes an explicit
iterable of SPARQL paths so it can serve any workload; the WatDiv-specific
``watdiv-stress-*`` convention lives in the shim functions at the bottom.
"""
from __future__ import annotations

import json
from collections.abc import Iterable
from pathlib import Path


def parse_desc(desc_path: Path) -> list[int]:
    """Parses a .desc file into a list of integer cardinalities."""
    out: list[int] = []
    for lineno, raw in enumerate(desc_path.read_text(encoding="utf-8").splitlines(), 1):
        stripped = raw.strip()
        if not stripped:
            continue
        try:
            out.append(int(stripped))
        except ValueError as e:
            raise ValueError(f"{desc_path}:{lineno}: not an integer: {stripped!r}") from e
    return out


def collect_expected(sparql_files: Iterable[Path]) -> dict[str, int]:
    """Scans each SPARQL file's sibling `.desc` and returns the merged map.

    Keys are ``<yaml_name>::q<index>``, where ``yaml_name`` is the same
    ``{parent.name}-{stem}`` convention used by
    :func:`watdiv_preprocess.yaml_emitter.emit_yaml`. SPARQL files
    without a matching `.desc` are skipped silently — they simply produce
    no expected entries.
    """
    expected: dict[str, int] = {}
    for sparql_path in sparql_files:
        desc_path = sparql_path.with_suffix(".desc")
        if not desc_path.is_file():
            continue
        parent = sparql_path.parent.name
        stem = sparql_path.stem.replace(".", "-")
        yaml_name = f"{parent}-{stem}"
        for idx, cardinality in enumerate(parse_desc(desc_path)):
            expected[f"{yaml_name}::q{idx:04d}"] = cardinality
    return expected


def write_expected(sparql_files: Iterable[Path], out_path: Path) -> int:
    """Writes `expected.json` and returns the number of query entries."""
    expected = collect_expected(sparql_files)
    out_path.write_text(
        json.dumps(expected, indent=2, sort_keys=True),
        encoding="utf-8",
    )
    return len(expected)


def _watdiv_sparql_files(input_dir: Path) -> list[Path]:
    """Returns every `*.sparql` under a `watdiv-stress-*` subdir of `input_dir`."""
    found: list[Path] = []
    for stress_dir in sorted(input_dir.glob("watdiv-stress-*")):
        if not stress_dir.is_dir():
            continue
        found.extend(sorted(stress_dir.glob("*.sparql")))
    return found


def watdiv_collect_expected(input_dir: Path) -> dict[str, int]:
    """WatDiv-specific shim: runs the `watdiv-stress-*` glob and delegates."""
    return collect_expected(_watdiv_sparql_files(input_dir))


def write_watdiv_expected(input_dir: Path, out_path: Path) -> int:
    """WatDiv-specific shim: runs the `watdiv-stress-*` glob and delegates."""
    return write_expected(_watdiv_sparql_files(input_dir), out_path)
