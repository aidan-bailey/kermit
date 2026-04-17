"""Harvests per-query cardinalities from WatDiv .desc sidecar files.

Each `.desc` file holds one integer per line, in the same order as the
queries in the sibling `.sparql` file. We emit a single `expected.json`
keyed by ``"<yaml_name>::<query_name>"`` so the runtime correctness
test can look up expected counts directly.
"""
from __future__ import annotations

import json
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


def collect_expected(input_dir: Path) -> dict[str, int]:
    """Scans every `.desc` file next to a `.sparql` and returns the merged map."""
    expected: dict[str, int] = {}
    for stress_dir in sorted(input_dir.glob("watdiv-stress-*")):
        if not stress_dir.is_dir():
            continue
        for sparql_path in sorted(stress_dir.glob("*.sparql")):
            desc_path = sparql_path.with_suffix(".desc")
            if not desc_path.is_file():
                continue
            parent = sparql_path.parent.name
            stem = sparql_path.stem.replace(".", "-")
            yaml_name = f"{parent}-{stem}"
            for idx, cardinality in enumerate(parse_desc(desc_path)):
                expected[f"{yaml_name}::q{idx:04d}"] = cardinality
    return expected


def write_expected(input_dir: Path, out_path: Path) -> int:
    """Writes `expected.json` and returns the number of query entries."""
    expected = collect_expected(input_dir)
    out_path.write_text(
        json.dumps(expected, indent=2, sort_keys=True),
        encoding="utf-8",
    )
    return len(expected)
