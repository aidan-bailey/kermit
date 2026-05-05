"""Pytest fixtures: programmatically build a synthetic Criterion + BenchReport tree.

Programmatic generation beats committing dozens of identical JSON files: less
repo churn when the schema evolves, no copy-paste mistakes between fixtures.
The shapes mirror what ``kermit bench`` actually emits (verified against a
real run during phase-1 of this feature).
"""
from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence

import pytest


@dataclass
class _FunctionSpec:
    group: str
    function: str  # e.g. "TreeTrie/iteration"
    metric: str  # "time" | "space"
    point: float  # mean point estimate
    samples: Sequence[tuple[float, float]]  # (iters, total) per Criterion sample


def _write_function_dir(criterion_root: Path, spec: _FunctionSpec) -> None:
    """Mirror Criterion's on-disk layout for one function under ``new/``."""
    dirname = spec.function.replace("/", "_")
    new_dir = criterion_root / spec.group / dirname / "new"
    new_dir.mkdir(parents=True, exist_ok=True)

    (new_dir / "benchmark.json").write_text(
        json.dumps(
            {
                "group_id": spec.group,
                "function_id": spec.function,
                "value_str": None,
                "throughput": None,
                "full_id": f"{spec.group}/{spec.function}",
                "directory_name": f"{spec.group}/{dirname}",
                "title": f"{spec.group}/{spec.function}",
            }
        )
    )

    estimate = {
        "confidence_interval": {
            "confidence_level": 0.95,
            "lower_bound": spec.point * 0.99,
            "upper_bound": spec.point * 1.01,
        },
        "point_estimate": spec.point,
        "standard_error": spec.point * 0.005,
    }
    (new_dir / "estimates.json").write_text(
        json.dumps(
            {
                "mean": estimate,
                "median": estimate,
                "median_abs_dev": estimate,
                "slope": estimate,
                "std_dev": {
                    "confidence_interval": {
                        "confidence_level": 0.95,
                        "lower_bound": 0.0,
                        "upper_bound": spec.point * 0.01,
                    },
                    "point_estimate": spec.point * 0.005,
                    "standard_error": 0.001,
                },
            }
        )
    )

    (new_dir / "sample.json").write_text(
        json.dumps(
            {
                "sampling_mode": "Linear",
                "iters": [s[0] for s in spec.samples],
                "times": [s[1] for s in spec.samples],
            }
        )
    )

    (new_dir / "tukey.json").write_text(
        json.dumps(
            [spec.point * 0.9, spec.point * 0.95, spec.point * 1.05, spec.point * 1.1]
        )
    )


def _write_report(
    reports_dir: Path,
    name: str,
    *,
    kind: str,
    axes: dict,
    metadata: list[dict],
    groups: list[tuple[str, str, str]],  # (group, function, metric)
) -> Path:
    path = reports_dir / f"{name}.json"
    path.write_text(
        json.dumps(
            [
                {
                    "schema_version": 2,
                    "kind": kind,
                    "metadata": metadata,
                    "axes": axes,
                    "criterion_groups": [
                        {"group": g, "function": f, "metric": m} for g, f, m in groups
                    ],
                }
            ]
        )
    )
    return path


@pytest.fixture
def fixture_tree(tmp_path: Path) -> dict:
    """Build the canonical fixture set: 2 DS × 1 algo × 3 sizes + 2 extra queries.

    Returns a dict with ``criterion_root``, ``reports_dir``, and a sorted list
    of report ``paths``. Time samples scale linearly with the point estimate
    (10 samples each, doubling iters from one to next) so per-iter math stays
    a clean integer.
    """
    criterion_root = tmp_path / "target" / "criterion"
    reports_dir = tmp_path / "reports"
    criterion_root.mkdir(parents=True)
    reports_dir.mkdir()

    paths: list[Path] = []
    sizes = [10, 100, 1000]
    for ds in ("TreeTrie", "ColumnTrie"):
        for n in sizes:
            # Both insertion and iteration phases — plot modules filter by phase
            # (default: iteration). Insertion is ~3× slower per the typical
            # construct-then-query pattern; this gap is asserted by tests.
            iteration_function = f"{ds}/triangle/{n}/iteration"
            insertion_function = f"{ds}/triangle/{n}/insertion"
            space_function = f"{ds}/triangle/{n}/space"
            iteration_point = 100.0 * n  # ns
            insertion_point = 300.0 * n  # ns
            space_point = float(n * 64)  # bytes; deterministic, zero-variance

            iteration_samples = [(i + 1, iteration_point * (i + 1)) for i in range(10)]
            insertion_samples = [(i + 1, insertion_point * (i + 1)) for i in range(10)]
            space_samples = [(i + 1, space_point * (i + 1)) for i in range(10)]

            _write_function_dir(
                criterion_root,
                _FunctionSpec(
                    "run", iteration_function, "time", iteration_point, iteration_samples
                ),
            )
            _write_function_dir(
                criterion_root,
                _FunctionSpec(
                    "run", insertion_function, "time", insertion_point, insertion_samples
                ),
            )
            _write_function_dir(
                criterion_root,
                _FunctionSpec("run", space_function, "space", space_point, space_samples),
            )

            paths.append(
                _write_report(
                    reports_dir,
                    f"run-{ds}-triangle-{n}",
                    kind="run",
                    axes={
                        "benchmark": "triangle",
                        "query": "triangle",
                        "data_structure": ds,
                        "algorithm": "LeapfrogTriejoin",
                        "tuples": n,
                    },
                    metadata=[
                        {"label": "data structure", "value": ds},
                        {"label": "tuples", "value": str(n)},
                    ],
                    groups=[
                        ("run", iteration_function, "time"),
                        ("run", insertion_function, "time"),
                        ("run", space_function, "space"),
                    ],
                )
            )

    # Additional queries for bar-queries shape: TreeTrie + LeapfrogTriejoin × {chain, star}.
    # Only iteration phase here — keeps the fixture compact while still testing
    # the per-query branching logic.
    for query in ("chain", "star"):
        iteration_function = f"TreeTrie/{query}/100/iteration"
        iteration_point = 250.0
        iteration_samples = [(i + 1, iteration_point * (i + 1)) for i in range(10)]
        _write_function_dir(
            criterion_root,
            _FunctionSpec(
                "run", iteration_function, "time", iteration_point, iteration_samples
            ),
        )
        paths.append(
            _write_report(
                reports_dir,
                f"run-TreeTrie-{query}-100",
                kind="run",
                axes={
                    "benchmark": query,
                    "query": query,
                    "data_structure": "TreeTrie",
                    "algorithm": "LeapfrogTriejoin",
                    "tuples": 100,
                },
                metadata=[
                    {"label": "query", "value": query},
                ],
                groups=[("run", iteration_function, "time")],
            )
        )

    return {
        "criterion_root": criterion_root,
        "reports_dir": reports_dir,
        "paths": sorted(paths),
    }
