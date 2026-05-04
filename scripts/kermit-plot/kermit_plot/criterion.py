"""Parse Criterion's per-function output (``estimates.json`` / ``sample.json`` / ``benchmark.json``).

Criterion writes one directory per benchmark function under
``target/criterion/<group>/<directory_name>/{base,new}/``. The ``directory_name``
replaces ``/`` in ``function_id`` with ``_`` (per Criterion's own escaping
rules). Always read it from each candidate's ``benchmark.json`` rather than
recomputing — works for special characters too.

Per-iter math: ``sample.json["times"][i]`` is the *total* over ``iters[i]``
iterations, so per-iter is ``times[i] / iters[i]``.
"""
from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Optional


@dataclass(frozen=True)
class Estimate:
    """One Criterion point-estimate plus 95% confidence interval.

    Units match the source measurement: nanoseconds for time, bytes for space.
    """

    point: float
    lower: float
    upper: float
    standard_error: float


@dataclass(frozen=True)
class FunctionData:
    """Parsed contents of one Criterion ``new/`` directory.

    ``slope`` is ``None`` for measurements Criterion couldn't fit a linear
    model to — most often deterministic / zero-variance space measurements.
    """

    group: str
    function: str
    directory_name: str
    mean: Estimate
    median: Estimate
    slope: Optional[Estimate]
    std_dev: Estimate
    iters: list[float]
    times: list[float]

    @property
    def per_iter_times(self) -> list[float]:
        """Per-iteration durations (or sizes), one per Criterion sample.

        ``sample.json`` records *total* over each batch, so we divide here
        rather than at every call site.
        """
        return [t / i for t, i in zip(self.times, self.iters)]


def _estimate(node: dict) -> Estimate:
    return Estimate(
        point=float(node["point_estimate"]),
        lower=float(node["confidence_interval"]["lower_bound"]),
        upper=float(node["confidence_interval"]["upper_bound"]),
        standard_error=float(node["standard_error"]),
    )


def resolve_function_dir(criterion_root: Path, group: str, function_id: str) -> Path:
    """Resolve ``(group, function_id)`` to the ``new/`` directory on disk.

    Criterion flattens slashes in the ``BenchmarkGroup`` name to underscores
    when laying out ``target/criterion/<dirname>/`` — so a group reported as
    ``"run/oxford-uniform-s1/triangle/TreeTrie/LeapfrogTriejoin"`` lives at
    ``run_oxford-uniform-s1_triangle_TreeTrie_LeapfrogTriejoin`` on disk. We
    apply the same translation here, then read each candidate subdir's
    ``benchmark.json:function_id`` to find the one matching ``function_id``.

    Raises ``FileNotFoundError`` if no subdir of the resolved group dir
    contains a ``new/benchmark.json`` whose ``function_id`` matches.
    """
    group_dir = criterion_root / group.replace("/", "_")
    if not group_dir.is_dir():
        raise FileNotFoundError(f"Criterion group dir not found: {group_dir}")
    for candidate in sorted(group_dir.iterdir()):
        if not candidate.is_dir():
            continue
        bench_json = candidate / "new" / "benchmark.json"
        if not bench_json.is_file():
            continue
        with bench_json.open() as f:
            meta = json.load(f)
        if meta.get("function_id") == function_id:
            return candidate / "new"
    raise FileNotFoundError(
        f"No Criterion subdir under {group_dir} has function_id={function_id!r}"
    )


def load_function(criterion_root: Path, group: str, function_id: str) -> FunctionData:
    """Load the four JSON files for one Criterion function into ``FunctionData``."""
    new_dir = resolve_function_dir(criterion_root, group, function_id)

    with (new_dir / "benchmark.json").open() as f:
        bench = json.load(f)
    with (new_dir / "estimates.json").open() as f:
        est = json.load(f)
    with (new_dir / "sample.json").open() as f:
        sample = json.load(f)

    return FunctionData(
        group=bench["group_id"],
        function=bench["function_id"],
        directory_name=bench["directory_name"],
        mean=_estimate(est["mean"]),
        median=_estimate(est["median"]),
        slope=_estimate(est["slope"]) if est.get("slope") is not None else None,
        std_dev=_estimate(est["std_dev"]),
        iters=[float(x) for x in sample["iters"]],
        times=[float(x) for x in sample["times"]],
    )
