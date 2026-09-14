"""Parse Criterion's per-function output (``estimates.json`` / ``sample.json`` / ``benchmark.json``).

Criterion writes one directory per benchmark function under
``target/criterion/<group dir>/<function dir>/{base,new}/``. Both directory
names are *derived* from the ids and cannot be recomputed reliably: Criterion
0.8.2 escapes ``/`` and other unsafe characters to ``_``, clamps each name to
64 bytes (so long ``bench run`` groups such as
``run/<benchmark>/<query>/<DS>/<Algo>`` are cut short), and appends ``_2``,
``_3``… to a function directory whose name collides with one already used by
the same ``Criterion`` instance. ``kermit bench`` builds a fresh instance per
group, so there colliding groups overwrite each other instead. The authoritative ``group_id`` / ``function_id`` live in each
``new/benchmark.json``, so :class:`CriterionIndex` reads those instead.

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


_MAX_DIRECTORY_NAME_BYTES = 64
"""Criterion 0.8.2's ``MAX_DIRECTORY_NAME_LEN``; used only to explain errors."""


def _criterion_group_dirname(group: str) -> str:
    """Criterion's ``make_filename_safe`` on a group id (non-Windows).

    Only used to point an error message at the directory a missing group
    *would* have written to — resolution itself never relies on it.
    """
    for ch in '?"/\\*<>:|^':
        group = group.replace(ch, "_")
    return group.encode()[:_MAX_DIRECTORY_NAME_BYTES].decode(errors="ignore")


class CriterionIndex:
    """Every Criterion function under ``criterion_root``, keyed by its ids.

    Built by reading each ``<group dir>/<function dir>/new/benchmark.json``
    once, so a loader resolving many functions pays one directory walk rather
    than one per lookup. ``base/`` copies (the previous run) are ignored.
    """

    def __init__(self, criterion_root: Path) -> None:
        self.root = Path(criterion_root)
        self._new_dirs: dict[tuple[str, str], list[Path]] = {}
        self._groups_by_dir: dict[str, set[str]] = {}
        for bench_json in sorted(self.root.glob("*/*/new/benchmark.json")):
            with bench_json.open() as f:
                meta = json.load(f)
            new_dir = bench_json.parent
            key = (meta["group_id"], meta["function_id"])
            self._new_dirs.setdefault(key, []).append(new_dir)
            self._groups_by_dir.setdefault(new_dir.parent.parent.name, set()).add(
                meta["group_id"]
            )

    def resolve(self, group: str, function_id: str) -> Path:
        """Return the ``new/`` directory holding ``(group, function_id)``.

        Raises ``FileNotFoundError`` if no directory holds it — naming the
        groups found in its truncated directory when a later run of a group
        sharing the 64-byte prefix has replaced it — and ``ValueError`` if
        more than one ``new/`` directory claims it (one of them is stale).
        """
        candidates = self._new_dirs.get((group, function_id), [])
        if len(candidates) == 1:
            return candidates[0]
        if candidates:
            listed = ", ".join(str(c.parent) for c in candidates)
            raise ValueError(
                f"{len(candidates)} Criterion directories claim group={group!r} "
                f"function_id={function_id!r}: {listed}. Colliding group names made "
                "Criterion suffix one of them in an earlier run; delete the stale "
                "directory and re-run."
            )

        dirname = _criterion_group_dirname(group)
        others = sorted(self._groups_by_dir.get(dirname, set()) - {group})
        if others:
            raise FileNotFoundError(
                f"No Criterion results for group={group!r} function_id={function_id!r}. "
                f"Its directory {self.root / dirname} now holds {others}: Criterion "
                f"truncates directory names to {_MAX_DIRECTORY_NAME_BYTES} bytes, so a "
                "group sharing that prefix, benchmarked later, replaced these results."
            )
        raise FileNotFoundError(
            f"No Criterion results under {self.root} for group={group!r} "
            f"function_id={function_id!r}"
        )

    def load(self, group: str, function_id: str) -> FunctionData:
        """Load the three JSON files for one Criterion function into ``FunctionData``."""
        return _load_new_dir(self.resolve(group, function_id))


def resolve_function_dir(criterion_root: Path, group: str, function_id: str) -> Path:
    """One-off :meth:`CriterionIndex.resolve`; build a :class:`CriterionIndex`
    to resolve many functions against the same root."""
    return CriterionIndex(criterion_root).resolve(group, function_id)


def load_function(criterion_root: Path, group: str, function_id: str) -> FunctionData:
    """One-off :meth:`CriterionIndex.load`; build a :class:`CriterionIndex` to
    load many functions against the same root."""
    return CriterionIndex(criterion_root).load(group, function_id)


def _load_new_dir(new_dir: Path) -> FunctionData:
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
