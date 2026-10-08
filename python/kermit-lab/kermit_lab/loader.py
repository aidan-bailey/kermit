"""Parse ``BenchReport`` JSON written by ``kermit bench --report-json``.

Top-level shape is always a JSON array (even for single-report subcommands).
Reports carry both ``metadata`` (label/value strings) and ``axes`` (typed
key/value map). Plot code consumes ``axes``; ``metadata`` is for humans.

See ``docs/specs/bench-report-schema.md`` for the full key catalogue.
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable, Iterator, Sequence

from . import SCHEMA_VERSION
from .criterion import CriterionIndex, FunctionData


class SchemaError(ValueError):
    """Report's ``schema_version`` is missing, unsupported, or mixed across an incompatible boundary."""


STREAMED_JOIN_SCHEMA = 3
"""First schema version whose ``iteration`` / ``end_to_end`` phases time a
streamed join with counted, never-materialised rows (issue #65)."""

FLAT_TUPLES_SCHEMA = 4
"""First schema version whose structures build from one flat tuple buffer
(issue #111): ``insertion``, ``copies`` and ``end_to_end`` time a different
input path and sort, HashTrie's ``space`` counts row ids into the buffer
instead of a ``Vec`` per tuple, and a lazy-expansion HashTrie's
``iteration``, which runs on a fresh build and times child expansion, times
Algorithm 2 grouping row ids and reading rows from the buffer."""

MEANING_CHANGES: tuple[tuple[int, str, tuple[str, ...]], ...] = (
    (
        STREAMED_JOIN_SCHEMA,
        "the iteration and end_to_end phases time a streamed, counted join",
        ("TreeTrie space", "ColumnTrie space", "HashTrie space"),
    ),
    (
        FLAT_TUPLES_SCHEMA,
        "every structure builds from one flat tuple buffer, which changes insertion, "
        "copies, end_to_end, HashTrie's space and lazy-expansion HashTrie's iteration",
        (
            "TreeTrie space",
            "ColumnTrie space",
            "bench run / bench join iteration outside lazy-expansion HashTrie cells",
        ),
    ),
)
"""Every schema version from which a metric changed meaning: the version,
what changed, and the metrics that keep their meaning across it. Reports on
either side of one measure different things, so one load may not mix them. A
load that straddles several of these may compare only the metrics every
straddled version keeps."""


@dataclass(frozen=True)
class CriterionGroupRef:
    """Pointer into ``target/criterion/`` produced by one bench function."""

    group: str
    function: str
    metric: str  # "time" | "space"


@dataclass(frozen=True)
class BenchReport:
    """One ``--report-json`` array element. Constructed by :func:`load_reports`."""

    schema_version: int
    kind: str  # "join" | "ds" | "run"
    metadata: list[dict[str, str]]
    axes: dict[str, Any]
    criterion_groups: list[CriterionGroupRef]
    source_path: Path = field(default_factory=Path)

    def axis(self, key: str, default: Any = None) -> Any:
        """Lookup an axis value by key (returns ``default`` if unset)."""
        return self.axes.get(key, default)

    def has_metric(self, metric: str) -> bool:
        """True iff at least one ``CriterionGroupRef`` records ``metric``."""
        return any(g.metric == metric for g in self.criterion_groups)


def _parse_one(obj: dict, source_path: Path) -> BenchReport:
    version = obj.get("schema_version")
    if not isinstance(version, int):
        raise SchemaError(f"{source_path}: missing or non-int schema_version")
    if version > SCHEMA_VERSION:
        raise SchemaError(
            f"{source_path}: schema_version {version} > supported {SCHEMA_VERSION}; "
            "upgrade kermit-lab or pin to an older bench output"
        )
    try:
        return BenchReport(
            schema_version=version,
            kind=obj["kind"],
            metadata=list(obj.get("metadata", [])),
            axes=dict(obj.get("axes", {})),
            criterion_groups=[
                CriterionGroupRef(
                    group=g["group"],
                    function=g["function"],
                    metric=g["metric"],
                )
                for g in obj.get("criterion_groups", [])
            ],
            source_path=source_path,
        )
    except (KeyError, TypeError) as exc:
        raise SchemaError(f"{source_path}: malformed report ({exc})") from exc


def _join_words(items: Sequence[str]) -> str:
    return items[0] if len(items) == 1 else ", ".join(items[:-1]) + " and " + items[-1]


def _refuse_mixed_schema(reports: Sequence[BenchReport]) -> None:
    straddled = [
        (boundary, change, comparable)
        for boundary, change, comparable in MEANING_CHANGES
        if any(r.schema_version < boundary for r in reports)
        and any(r.schema_version >= boundary for r in reports)
    ]
    if not straddled:
        return
    boundary, change, _ = straddled[0]
    older = next(r for r in reports if r.schema_version < boundary)
    newer = next(r for r in reports if r.schema_version >= boundary)
    message = (
        f"refusing to mix schema_version {older.schema_version} ({older.source_path}) "
        f"with schema_version {newer.schema_version} ({newer.source_path}): from "
        f"v{boundary} {change}, so those values are not comparable with earlier reports."
    )
    for later, later_change, _ in straddled[1:]:
        message += f" This load also straddles v{later}, where {later_change}."
    kept = [m for m in straddled[0][2] if all(m in comparable for _, _, comparable in straddled)]
    if kept:
        message += (
            " Load each side separately, or (Python API) pass allow_mixed_schema=True "
            f"to compare {_join_words(kept)} only."
        )
    else:
        message += " Load each side separately: no metric is comparable across them all."
    raise SchemaError(message)


def load_reports(
    paths: Iterable[Path], *, allow_mixed_schema: bool = False
) -> list[BenchReport]:
    """Load all reports from one or more JSON files; flattens the array shape.

    Raises :class:`SchemaError` when the reports straddle a version in
    :data:`MEANING_CHANGES`, unless ``allow_mixed_schema`` is true.
    """
    out: list[BenchReport] = []
    for path in paths:
        with Path(path).open() as f:
            data = json.load(f)
        if not isinstance(data, list):
            raise SchemaError(f"{path}: top level must be a JSON array")
        for obj in data:
            out.append(_parse_one(obj, Path(path)))
    if not allow_mixed_schema:
        _refuse_mixed_schema(out)
    return out


TIME_PHASES: tuple[str, ...] = ("insertion", "copies", "iteration", "end_to_end")


def phase_of(function_id: str) -> str | None:
    """Return the time phase (``"insertion"`` / ``"copies"`` /
    ``"iteration"`` / ``"end_to_end"``) if ``function_id`` encodes one.

    ``copies`` is the reordered copies a query needs under
    ``--column-orders any``, emitted beside ``insertion`` when the plan
    needs one (#93).

    `bench ds` writes ``"{ds}/<phase>"``; `bench run` writes the bare
    ``"<phase>"``. Both end with the phase token, so a final-segment check
    covers both. (``end_to_end`` carries its K in the report's
    ``queries_per_build`` axis, never in the function id, so the
    final-segment rule holds for it too.) Space-metric functions
    (``"space/{rel}"`` etc.) return None and are filtered out by callers
    that only consume time-metric phases.
    """
    last = function_id.rsplit("/", 1)[-1]
    return last if last in TIME_PHASES else None


def iter_function_data(
    reports: Iterable[BenchReport], criterion_root: Path
) -> Iterator[tuple[BenchReport, CriterionGroupRef, FunctionData]]:
    """Yield ``(report, group_ref, FunctionData)`` for every Criterion function in ``reports``.

    Resolves each ``CriterionGroupRef`` against one :class:`CriterionIndex` of
    ``criterion_root`` and loads the per-function JSON. Missing on-disk
    artefacts raise :class:`FileNotFoundError` rather than being silently
    skipped — a stale-cargo-clean bug should fail loudly, not produce an empty
    plot.
    """
    index = CriterionIndex(criterion_root)
    for report in reports:
        for group_ref in report.criterion_groups:
            data = index.load(group_ref.group, group_ref.function)
            yield report, group_ref, data
