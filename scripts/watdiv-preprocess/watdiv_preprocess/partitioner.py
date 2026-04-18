"""Partitions N-Triples by predicate, writing one Parquet file per predicate."""
from __future__ import annotations

import re
from collections import defaultdict
from pathlib import Path

from .dict_builder import iter_ntriples

_NAME_CHARS = re.compile(r"[^A-Za-z0-9]+")


def sanitize_predicate(uri: str) -> str:
    """Converts a predicate URI into a Datalog-safe lowercase identifier.

    Strips angle brackets, prefers the fragment or last path segment,
    then replaces non-alphanumeric characters with underscores. Falls
    back to a `p_` prefix if the result would start with a digit.
    """
    core = uri.strip("<>")
    for sep in ("#", "/"):
        if sep in core:
            core = core.rsplit(sep, 1)[-1]
    cleaned = _NAME_CHARS.sub("_", core).strip("_")
    if not cleaned or cleaned[0].isdigit():
        cleaned = f"p_{cleaned}"
    return cleaned.lower()


def partition_triples(
    nt_path: Path,
    uri_to_id: dict[str, int],
    out_dir: Path,
) -> dict[str, str]:
    """Second pass over the N-Triples file, flushing per-predicate Parquet.

    Literal objects are dictionary-encoded just like URIs — the kermit
    engine joins on ``usize`` keys without distinguishing the two, and
    WatDiv stress queries reference literal-only predicates (e.g.
    ``ogp:title``) as projected variables, so dropping those triples
    would leave the queries with no relation to join against.

    Returns a ``uri_to_predicate`` map from the full predicate URI (with
    angle brackets, exactly as it appears in the N-Triples file) to the
    canonical predicate name used for both the Parquet filename and the
    Datalog identifier. Collisions between sanitized names are resolved
    by appending ``_<dict-id>`` to all but the first occurrence, so the
    map is the single source of truth — translator and emitter must use
    it rather than re-running :func:`sanitize_predicate` independently.
    """
    import pyarrow as pa
    import pyarrow.parquet as pq

    buckets: dict[str, list[tuple[int, int]]] = defaultdict(list)
    for s, p, o in iter_ntriples(nt_path):
        buckets[p].append((uri_to_id[s], uri_to_id[o]))

    uri_to_predicate: dict[str, str] = {}
    used_names: set[str] = set()
    for pred_uri, tuples in buckets.items():
        name = sanitize_predicate(pred_uri)
        if name in used_names:
            name = f"{name}_{uri_to_id[pred_uri]}"
        used_names.add(name)
        uri_to_predicate[pred_uri] = name
        ss = pa.array([t[0] for t in tuples], type=pa.int64())
        oo = pa.array([t[1] for t in tuples], type=pa.int64())
        pq.write_table(pa.table({"s": ss, "o": oo}), out_dir / f"{name}.parquet")
    return uri_to_predicate
