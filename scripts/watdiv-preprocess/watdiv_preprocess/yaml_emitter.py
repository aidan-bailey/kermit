"""Emits one kermit BenchmarkDefinition YAML per SPARQL file."""
from __future__ import annotations

import re
from pathlib import Path

from .sparql_translator import translate_file

_PRED_USED = re.compile(r"([a-z][a-z0-9_]*)\(")


def collect_predicates(datalog: str) -> set[str]:
    """Returns the set of body predicate names mentioned in a Datalog rule."""
    _, _, body = datalog.partition(":-")
    return set(_PRED_USED.findall(body))


def emit_yaml(
    sparql_path: Path,
    output_dir: Path,
    uri_to_id: dict[str, int],
    base_url: str,
    predicate_map: dict[str, str],
) -> Path:
    """Translates one SPARQL file and writes the matching benchmark YAML.

    ``predicate_map`` is the URI → canonical-name map produced by
    :func:`watdiv_preprocess.partitioner.partition_triples` and is
    threaded through to the translator so both sides agree on names
    (including collision-resolved suffixes).

    The YAML's ``relations`` block lists only the predicates actually
    referenced by the file, so each benchmark downloads the minimum set
    of Parquet files needed.
    """
    import yaml

    parent = sparql_path.parent.name
    stem = sparql_path.stem.replace(".", "-")
    yaml_name = f"{parent}-{stem}"
    head_prefix = f"Q_{stem.replace('-', '_')}"

    queries_datalog = translate_file(sparql_path, uri_to_id, predicate_map, head_prefix)
    used_predicates: set[str] = set()
    for _, datalog in queries_datalog:
        used_predicates |= collect_predicates(datalog)

    known_names = set(predicate_map.values())
    relations = [
        {"name": p, "url": f"{base_url.rstrip('/')}/{p}.parquet"}
        for p in sorted(used_predicates)
        if p in known_names
    ]

    doc = {
        "name": yaml_name,
        "description": (
            f"WatDiv stress test, file {parent}/{sparql_path.name}, "
            f"{len(queries_datalog)} queries"
        ),
        "relations": relations,
        "queries": [
            {
                "name": qname,
                "description": f"WatDiv query {qname}",
                "query": datalog,
            }
            for qname, datalog in queries_datalog
        ],
    }

    out_path = output_dir / f"{yaml_name}.yml"
    with out_path.open("w", encoding="utf-8") as f:
        yaml.safe_dump(doc, f, sort_keys=False, default_flow_style=False, allow_unicode=True)
    return out_path
