"""Workload-agnostic orchestrator for the preprocessor.

`run_pipeline` wires together dict building, triple partitioning, per-file
YAML emission, and an optional expected-cardinality writer. Both the
`watdiv` and `generic` CLI drivers are thin wrappers that gather
arguments and delegate here; the pipeline itself knows nothing about
WatDiv-specific paths or `.desc` sidecars.
"""
from __future__ import annotations

import sys
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import IO, Callable, Optional

from .dict_builder import build_dict
from .partitioner import partition_triples
from .yaml_emitter import emit_yaml

ExpectedWriter = Callable[[Path], int]


@dataclass(frozen=True)
class PipelineResult:
    """Counts returned by `run_pipeline` — useful for CLI logging and tests."""

    dict_size: int
    partition_count: int
    yaml_count: int
    expected_count: int


def run_pipeline(
    *,
    nt_path: Path,
    sparql_files: Iterable[Path],
    output_dir: Path,
    base_url: str,
    expected_writer: Optional[ExpectedWriter] = None,
    log_prefix: str = "preprocess",
    log_stream: IO[str] = sys.stderr,
) -> PipelineResult:
    """Runs dict → partition → emit → expected over the provided inputs.

    Args:
        nt_path: path to an N-Triples file (one triple per line).
        sparql_files: SPARQL files to translate. Iterated once and
            materialised into a list so order is stable and the count
            can be reported.
        output_dir: directory where all artifacts are written; created
            if it doesn't exist.
        base_url: URL prefix spliced into each YAML's ``relations[].url``.
        expected_writer: optional callable ``(out_path) -> entry_count``.
            If provided, called once with ``<output_dir>/expected.json``.
            If omitted, ``expected.json`` is not written.
        log_prefix: tag prepended to progress lines; drivers pass their
            own name so output is self-identifying.
        log_stream: where progress lines are written. Tests inject an
            ``io.StringIO`` to capture; CLI drivers leave the default.
    """
    output_dir.mkdir(parents=True, exist_ok=True)

    uri_to_id = build_dict(nt_path, output_dir)
    print(
        f"[{log_prefix}] dictionary: {len(uri_to_id)} terms written to {output_dir}",
        file=log_stream,
    )

    predicate_map = partition_triples(nt_path, uri_to_id, output_dir)
    partition_count = len(set(predicate_map.values()))
    print(
        f"[{log_prefix}] partitioned into {partition_count} predicate Parquet files",
        file=log_stream,
    )

    sparql_files_list = list(sparql_files)
    for sparql_file in sparql_files_list:
        out = emit_yaml(sparql_file, output_dir, uri_to_id, base_url, predicate_map)
        print(f"[{log_prefix}] wrote {out}", file=log_stream)
    print(
        f"[{log_prefix}] emitted {len(sparql_files_list)} benchmark YAML files",
        file=log_stream,
    )

    expected_count = 0
    if expected_writer is not None:
        expected_path = output_dir / "expected.json"
        expected_count = expected_writer(expected_path)
        print(
            f"[{log_prefix}] wrote {expected_count} expected cardinalities to "
            f"{expected_path}",
            file=log_stream,
        )

    return PipelineResult(
        dict_size=len(uri_to_id),
        partition_count=partition_count,
        yaml_count=len(sparql_files_list),
        expected_count=expected_count,
    )
