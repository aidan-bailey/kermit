"""Streams an N-Triples file once to build a URI → usize dictionary."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Iterator


def iter_ntriples(path: Path) -> Iterator[tuple[str, str, str]]:
    """Yields (s, p, o) token strings from an N-Triples file.

    Tokens keep their surrounding punctuation (angle brackets around URIs,
    quotes around literals) so callers can distinguish URI objects from
    literal objects without re-parsing.
    """
    with path.open("r", encoding="utf-8") as f:
        for lineno, raw in enumerate(f, 1):
            line = raw.rstrip("\n").rstrip()
            if not line or line.startswith("#"):
                continue
            if not line.endswith("."):
                raise ValueError(f"line {lineno}: missing trailing '.'")
            body = line[:-1].rstrip()
            tokens = _split_three(body)
            if tokens is None:
                raise ValueError(f"line {lineno}: could not parse three terms: {raw!r}")
            yield tokens


def _split_three(body: str) -> tuple[str, str, str] | None:
    """Splits an N-Triples line body on whitespace into (s, p, o).

    The object position may itself contain whitespace inside a quoted
    literal, so split only on the first two whitespace runs.
    """
    parts = body.split(None, 2)
    if len(parts) != 3:
        return None
    return parts[0], parts[1], parts[2]


def build_dict(nt_path: Path, out_dir: Path) -> dict[str, int]:
    """Assigns fresh IDs to unseen terms; writes dict.json and dict.parquet.

    Literal objects are included in the dictionary alongside URIs so they
    can be referenced by SPARQL constants if needed later.
    """
    import pyarrow as pa
    import pyarrow.parquet as pq

    uri_to_id: dict[str, int] = {}

    def intern(token: str) -> None:
        if token not in uri_to_id:
            uri_to_id[token] = len(uri_to_id)

    for s, p, o in iter_ntriples(nt_path):
        intern(s)
        intern(p)
        intern(o)

    (out_dir / "dict.json").write_text(
        json.dumps({"uri_to_id": uri_to_id}, ensure_ascii=False),
        encoding="utf-8",
    )
    ids = list(uri_to_id.values())
    uris = list(uri_to_id.keys())
    table = pa.table(
        {
            "id": pa.array(ids, type=pa.uint64()),
            "uri": pa.array(uris, type=pa.string()),
        }
    )
    pq.write_table(table, out_dir / "dict.parquet")
    return uri_to_id
