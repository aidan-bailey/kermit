#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = ["pyyaml>=6.0"]
# ///
"""Install a committed WatDiv stress snapshot into the kermit cache and derive a
one-query-per-template sample benchmark from it.

The committed `benchmarks/watdiv-stress-{100,1000}-*.yml` snapshots reference
ZivaHub URLs that do not serve their Parquet files, so the data has to be
rebuilt from the upstream WatDiv release with `scripts/watdiv-preprocess`. That
rebuild is deterministic: it emits YAMLs byte-identical to the committed ones,
which is what proves the regenerated Parquet is the data the snapshots' `c<id>`
constants refer to. This script checks that identity, then:

1. links the snapshot's relations into `<cache>/<snapshot>/`, so
   `kermit bench run <snapshot>` finds them instead of downloading;
2. writes `<cache>/<snapshot>-prelim/`, a cache-side benchmark holding the
   first instantiation of each stress template.

Each upstream stress file is 124 templates x 100 instantiations, interleaved; a
template is a query with its `wsdbm:` entity constants masked. The sample lets
`bench run` time every query shape with one database load per data structure
rather than one load per query.

Usage:
    # Upstream release + preprocessing, once (~15 min):
    U=~/.cache/kermit/watdiv-upstream; mkdir -p $U && cd $U
    curl -LO https://dsg.uwaterloo.ca/watdiv/watdiv.10M.tar.bz2
    curl -LO https://dsg.uwaterloo.ca/watdiv/stress-workloads.tar.gz
    tar -xjf watdiv.10M.tar.bz2 && tar -xzf stress-workloads.tar.gz && cd -
    uvx --from ./scripts/watdiv-preprocess watdiv-preprocess --input $U \\
        --output $U/out --base-url https://zivahub.uct.ac.za/ndownloader/files

    # test-1's q0005 has 4,169,173,508 result rows, beyond what the
    # `iteration` metric can materialise, so leave it out:
    uv run scripts/watdiv_stress_sample.py watdiv-stress-100-test-1 --exclude q0005

Output:
    <cache>/<snapshot>/*.parquet
    <cache>/<snapshot>-prelim/{benchmark.yml,meta.json,*.parquet}
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import sys
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parent.parent

SNAPSHOT_NAME = re.compile(r"watdiv-stress-(\d+)-(.+)")
ENTITY_CONSTANT = re.compile(r"<http://db\.uwaterloo\.ca/~galuc/wsdbm/[A-Za-z]+[0-9]+>")
BODY_ATOM = re.compile(r"([a-z_0-9]+)\(")


def default_cache_root() -> Path:
    """Mirrors `kermit_bench::cache::base_cache_dir` (the `dirs` crate's cache dir)."""
    if sys.platform == "darwin":
        base = Path.home() / "Library" / "Caches"
    elif sys.platform == "win32":
        base = Path(os.environ["LOCALAPPDATA"])
    else:
        base = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    return base / "kermit" / "benchmarks"


def link_or_copy(src: Path, dst: Path) -> None:
    """Hard-links `src` to `dst`, copying when they sit on different filesystems.

    Replaces any existing `dst` — notably the zero-byte files `bench fetch`
    leaves behind when ZivaHub answers without a body.
    """
    dst.unlink(missing_ok=True)
    try:
        os.link(src, dst)
    except OSError:
        shutil.copy2(src, dst)


def body_relations(query: str) -> set[str]:
    return set(BODY_ATOM.findall(query.split(":-", 1)[1]))


def main() -> int:
    p = argparse.ArgumentParser(
        description="Install a WatDiv stress snapshot and derive a per-template sample.",
    )
    p.add_argument("snapshot", help="committed snapshot, e.g. watdiv-stress-100-test-1")
    p.add_argument(
        "--upstream",
        type=Path,
        default=Path.home() / ".cache" / "kermit" / "watdiv-upstream",
        help="extracted upstream release, with the preprocessor's output in out/",
    )
    p.add_argument(
        "--exclude",
        default="",
        help="comma-separated query names to leave out of the sample",
    )
    p.add_argument(
        "--cache-root",
        type=Path,
        default=default_cache_root(),
        help="kermit benchmark cache (default: the platform cache dir kermit uses)",
    )
    args = p.parse_args()

    m = SNAPSHOT_NAME.fullmatch(args.snapshot)
    if m is None:
        p.error(f"{args.snapshot!r} is not a watdiv-stress-<scale>-<file> snapshot name")
    committed = REPO_ROOT / "benchmarks" / f"{args.snapshot}.yml"
    regenerated = args.upstream / "out" / f"{args.snapshot}.yml"
    sparql = args.upstream / f"watdiv-stress-{m[1]}" / f"{m[2].replace('-', '.')}.sparql"
    if committed.read_bytes() != regenerated.read_bytes():
        sys.exit(
            f"{regenerated} differs from {committed}: the regenerated Parquet does not "
            "carry the dictionary the snapshot's constants were encoded with"
        )

    snapshot = yaml.safe_load(committed.read_text())
    queries = snapshot["queries"]
    lines = [line for line in sparql.read_text().splitlines() if line.strip()]
    if len(lines) != len(queries):
        sys.exit(f"{sparql} has {len(lines)} queries; {committed} has {len(queries)}")

    snapshot_dir = args.cache_root / args.snapshot
    snapshot_dir.mkdir(parents=True, exist_ok=True)
    for rel in snapshot["relations"]:
        name = f"{rel['name']}.parquet"
        link_or_copy(args.upstream / "out" / name, snapshot_dir / name)

    first: dict[str, int] = {}
    for i, line in enumerate(lines):
        first.setdefault(ENTITY_CONSTANT.sub("<C>", line), i)
    picked = [queries[i] for i in sorted(first.values())]
    for i, q in zip(sorted(first.values()), picked):
        if q["name"] != f"q{i:04d}":
            sys.exit(f"query {i} of {committed} is named {q['name']!r}, expected q{i:04d}")

    excluded = {name for name in args.exclude.split(",") if name}
    unknown = excluded - {q["name"] for q in picked}
    if unknown:
        sys.exit(f"--exclude names not in the sample: {', '.join(sorted(unknown))}")
    sample = [q for q in picked if q["name"] not in excluded]
    used = set().union(*(body_relations(q["query"]) for q in sample))
    relations = [rel for rel in snapshot["relations"] if rel["name"] in used]

    sample_name = f"{args.snapshot}-prelim"
    sample_dir = args.cache_root / sample_name
    sample_dir.mkdir(parents=True, exist_ok=True)
    for rel in relations:
        name = f"{rel['name']}.parquet"
        link_or_copy(snapshot_dir / name, sample_dir / name)

    description = f"First instantiation of each of the {len(first)} templates in {args.snapshot}"
    if excluded:
        description += f", excluding {', '.join(sorted(excluded))}"
    definition = {
        "name": sample_name,
        "description": description,
        "relations": relations,
        "queries": sample,
    }
    (sample_dir / "benchmark.yml").write_text(
        yaml.safe_dump(definition, sort_keys=False, width=10_000)
    )
    # kermit's discovery only loads a cache subdir that also holds meta.json.
    meta = {
        "kind": "watdiv-stress-template-sample",
        "source_benchmark": args.snapshot,
        "templates": len(first),
        "excluded": sorted(excluded),
    }
    (sample_dir / "meta.json").write_text(json.dumps(meta, indent=2) + "\n")

    print(f"{snapshot_dir}: {len(snapshot['relations'])} relations")
    print(f"{sample_dir}: {len(sample)} queries over {len(relations)} relations")
    return 0


if __name__ == "__main__":
    sys.exit(main())
