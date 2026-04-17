"""SPARQL BGP → Datalog translation. Fails loudly on non-BGP queries."""
from __future__ import annotations

from pathlib import Path
from typing import Iterator


class TranslationError(Exception):
    """Raised when a SPARQL query cannot be represented as a BGP-only Datalog rule."""


def iter_sparql_queries(text: str) -> Iterator[str]:
    """Yields one query per non-blank line of a WatDiv stress-test file."""
    for line in text.splitlines():
        stripped = line.strip()
        if stripped:
            yield stripped


def translate_query(
    sparql: str,
    uri_to_id: dict[str, int],
    predicate_map: dict[str, str],
    head_name: str,
) -> str:
    """Returns a Datalog rule string. Raises TranslationError on non-BGP input.

    ``predicate_map`` maps a predicate URI (with angle brackets, e.g.
    ``"<http://example/p>"``) to its canonical Datalog name. It is
    produced by :func:`watdiv_preprocess.partitioner.partition_triples`
    and must be consulted rather than re-running
    :func:`sanitize_predicate` — the partitioner resolves sanitization
    collisions between distinct URIs, and translating independently
    would silently alias different predicates to the same name.
    """
    from rdflib import URIRef
    from rdflib.plugins.sparql.algebra import translateQuery
    from rdflib.plugins.sparql.parser import parseQuery

    alg = translateQuery(parseQuery(sparql)).algebra
    project = alg.p
    if project.name != "Project":
        raise TranslationError(f"expected top-level Project, got {project.name}")
    bgp = project.p
    if bgp.name != "BGP":
        raise TranslationError(
            f"only BGP queries supported; got {bgp.name} (FILTER/OPTIONAL/UNION/subqueries?)"
        )

    body_parts: list[str] = []
    var_order: list[str] = []
    seen: set[str] = set()

    def note_var(name: str) -> None:
        if name not in seen:
            seen.add(name)
            var_order.append(name)

    for s, p, o in bgp.triples:
        if not isinstance(p, URIRef):
            raise TranslationError(f"non-ground predicate in BGP: {p!r}")
        pred_key = f"<{p}>"
        pred_name = predicate_map.get(pred_key)
        if pred_name is None:
            raise TranslationError(f"predicate URI not in partition map: {p}")
        s_term, s_var = _term_to_datalog(s, uri_to_id)
        o_term, o_var = _term_to_datalog(o, uri_to_id)
        if s_var is not None:
            note_var(s_var)
        if o_var is not None:
            note_var(o_var)
        body_parts.append(f"{pred_name}({s_term}, {o_term})")

    projected_vars = [_var_name(str(v)) for v in project.PV]
    if set(projected_vars) == seen:
        head_args = var_order
    else:
        head_args = projected_vars
        for v in head_args:
            if v not in seen:
                raise TranslationError(f"projected variable {v} not bound by BGP")

    head_terms = ", ".join(head_args) if head_args else ""
    body = ", ".join(body_parts)
    return f"{head_name}({head_terms}) :- {body}."


def _term_to_datalog(term, uri_to_id: dict[str, int]) -> tuple[str, str | None]:
    """Returns (datalog_form, var_name_or_none). Literals raise TranslationError."""
    from rdflib import URIRef, Variable

    if isinstance(term, Variable):
        v = _var_name(str(term))
        return v, v
    if isinstance(term, URIRef):
        key = f"<{term}>"
        if key not in uri_to_id:
            raise TranslationError(f"URI not in dictionary: {term}")
        return f"c{uri_to_id[key]}", None
    raise TranslationError(f"unsupported term type in BGP: {type(term).__name__}")


def _var_name(raw: str) -> str:
    """Normalises a SPARQL variable name to a Datalog-safe uppercase token."""
    name = raw.lstrip("?").lstrip("$")
    return name.upper()


def translate_file(
    sparql_path: Path,
    uri_to_id: dict[str, int],
    predicate_map: dict[str, str],
    head_prefix: str,
) -> list[tuple[str, str]]:
    """Translates every query in a WatDiv stress-test SPARQL file.

    Returns a list of (query_name, datalog) pairs.
    """
    results: list[tuple[str, str]] = []
    text = sparql_path.read_text(encoding="utf-8")
    for idx, q in enumerate(iter_sparql_queries(text)):
        qname = f"q{idx:04d}"
        head = f"{head_prefix}_{qname}"
        results.append((qname, translate_query(q, uri_to_id, predicate_map, head)))
    return results
