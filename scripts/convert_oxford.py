#!/usr/bin/env python3
"""Download Oxford DSI benchmark .tbl files and convert to Parquet.

Also generates YAML benchmark definitions for kermit.

Usage:
    pip install pyarrow requests
    python scripts/convert_oxford.py                    # convert only
    python scripts/convert_oxford.py --generate-yaml    # convert + write YAML benchmarks

Output:
    output/oxford-{uniform|zipf}-s{1-6}/{P..W}.parquet
    benchmarks/oxford-{uniform|zipf}-s{1-6}.yml  (with --generate-yaml)
"""

from __future__ import annotations

import argparse
from pathlib import Path
from urllib.request import urlopen

try:
    import pyarrow as pa
    import pyarrow.parquet as pq
except ImportError:
    pa = None  # type: ignore[assignment]
    pq = None  # type: ignore[assignment]

BASE_URL = (
    "https://raw.githubusercontent.com"
    "/schroederdewitt/leapfrog-triejoin/master/datasets"
)

DISTRIBUTIONS = {
    "uniform": "dataset1-uniform",
    "zipf": "dataset2-zipf",
}

SCALES = range(1, 7)

RELATIONS = ["P", "Q", "R", "S", "T", "U", "V", "W"]

RELATION_SCHEMA: dict[str, list[str]] = {
    "P": ["A"],
    "Q": ["A"],
    "R": ["A", "B", "X"],
    "S": ["A", "C", "Y"],
    "T": ["B", "C", "Z"],
    "U": ["X", "D"],
    "V": ["Y", "E"],
    "W": ["Z", "F"],
}

QUERIES = [
    {
        "name": "binary-join",
        "description": "Binary intersection of P and Q on attribute A",
        "query": "Result(A) :- P(A), Q(A).",
    },
    {
        "name": "triangle",
        "description": "Triangle join of R, S, T on attributes A, B, C",
        "query": "Result(A, B, C) :- R(A, B, X), S(A, C, Y), T(B, C, Z).",
    },
    {
        "name": "six-way",
        "description": "Six-way join across all relations on A, B, C, X, Y, Z",
        "query": (
            "Result(A, B, C, X, Y, Z) :- "
            "R(A, B, X), S(A, C, Y), T(B, C, Z), "
            "U(X, D), V(Y, E), W(Z, F)."
        ),
    },
]

SCALE_DESCRIPTIONS = {
    1: "1K tuples per relation",
    2: "2K tuples per relation",
    3: "4K tuples per relation",
    4: "8K tuples per relation",
    5: "16K tuples per relation",
    6: "32K tuples per relation",
}


def download_tbl(dist_dir: str, scale: int, relation: str) -> str:
    """Download a .tbl file and return its text content."""
    url = f"{BASE_URL}/{dist_dir}/scale{scale}/{relation}.tbl"
    print(f"  downloading {url}")
    with urlopen(url) as resp:
        return resp.read().decode("utf-8")


def parse_tbl(text: str, columns: list[str]) -> pa.Table:
    """Parse a .tbl file (headerless CSV with trailing commas) into an Arrow table."""
    rows: list[list[int]] = []
    for line in text.strip().splitlines():
        line = line.strip()
        if not line:
            continue
        # Strip trailing comma
        if line.endswith(","):
            line = line[:-1]
        values = [int(v) for v in line.split(",")]
        if len(values) != len(columns):
            raise ValueError(
                f"expected {len(columns)} columns ({columns}), "
                f"got {len(values)} values: {values}"
            )
        rows.append(values)

    # Build columnar arrays
    arrays = []
    for col_idx in range(len(columns)):
        col_values = [row[col_idx] for row in rows]
        arrays.append(pa.array(col_values, type=pa.int64()))

    schema = pa.schema([(name, pa.int64()) for name in columns])
    return pa.table(arrays, schema=schema)


def convert_all(output_dir: Path) -> None:
    """Download and convert all .tbl files to Parquet."""
    if pa is None:
        raise SystemExit("pyarrow is required for conversion: pip install pyarrow")
    for dist_name, dist_dir in DISTRIBUTIONS.items():
        for scale in SCALES:
            bench_name = f"oxford-{dist_name}-s{scale}"
            bench_dir = output_dir / bench_name
            bench_dir.mkdir(parents=True, exist_ok=True)

            print(f"[{bench_name}]")
            for relation in RELATIONS:
                columns = RELATION_SCHEMA[relation]
                text = download_tbl(dist_dir, scale, relation)
                table = parse_tbl(text, columns)
                out_path = bench_dir / f"{relation}.parquet"
                pq.write_table(table, out_path)
                print(f"    wrote {out_path} ({table.num_rows} rows)")


def generate_yaml(benchmarks_dir: Path) -> None:
    """Generate YAML benchmark definitions."""
    benchmarks_dir.mkdir(parents=True, exist_ok=True)

    for dist_name in DISTRIBUTIONS:
        for scale in SCALES:
            bench_name = f"oxford-{dist_name}-s{scale}"
            desc = SCALE_DESCRIPTIONS.get(scale, "")
            yml_path = benchmarks_dir / f"{bench_name}.yml"

            lines = [
                f'name: {bench_name}',
                f'description: "Oxford DSI benchmark - {dist_name} distribution, scale {scale} ({desc})"',
                'relations:',
            ]
            for relation in RELATIONS:
                lines.append(f'  - name: {relation}')
                lines.append(f'    url: "https://zivahub.uct.ac.za/ndownloader/files/PLACEHOLDER"')
            lines.append('queries:')
            for q in QUERIES:
                lines.append(f'  - name: {q["name"]}')
                lines.append(f'    description: "{q["description"]}"')
                lines.append(f'    query: "{q["query"]}"')
            lines.append('')  # trailing newline

            yml_path.write_text('\n'.join(lines))
            print(f"wrote {yml_path}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("output"),
        help="directory for Parquet output (default: output/)",
    )
    parser.add_argument(
        "--generate-yaml",
        action="store_true",
        help="also generate YAML benchmark definitions in benchmarks/",
    )
    parser.add_argument(
        "--benchmarks-dir",
        type=Path,
        default=Path("benchmarks"),
        help="directory for YAML output (default: benchmarks/)",
    )
    parser.add_argument(
        "--yaml-only",
        action="store_true",
        help="only generate YAML files, skip downloading and converting",
    )
    args = parser.parse_args()

    if not args.yaml_only:
        convert_all(args.output_dir)
        print(f"\nConversion complete. Parquet files in {args.output_dir}/")

    if args.generate_yaml or args.yaml_only:
        generate_yaml(args.benchmarks_dir)
        print(f"YAML benchmarks written to {args.benchmarks_dir}/")


if __name__ == "__main__":
    main()
