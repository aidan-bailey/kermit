"""Tests for the predicate partitioner, focused on naming collisions.

WatDiv's 86 unique predicate URIs contain 4 pairs that sanitize to the
same lowercase identifier (e.g. ``<http://ogp.me/ns#title>`` and
``<http://purl.org/stuff/rev#title>`` both become ``title``). The
partitioner must disambiguate them so no two Parquet files share a
filename and the returned URI → name map is injective.
"""
from __future__ import annotations

from pathlib import Path

from watdiv_preprocess.dict_builder import build_dict
from watdiv_preprocess.partitioner import partition_triples, sanitize_predicate


def test_sanitize_predicate_strips_scheme_and_fragment():
    assert sanitize_predicate("<http://ogp.me/ns#title>") == "title"
    assert sanitize_predicate("<http://schema.org/producer>") == "producer"


def test_partitioner_resolves_sanitize_collisions(tmp_path: Path):
    nt = tmp_path / "collide.nt"
    nt.write_text(
        "<http://s/a> <http://ogp.me/ns#title> <http://o/1> .\n"
        "<http://s/b> <http://purl.org/stuff/rev#title> <http://o/2> .\n",
        encoding="utf-8",
    )

    uri_to_id = build_dict(nt, tmp_path)
    predicate_map = partition_triples(nt, uri_to_id, tmp_path)

    # Map is keyed by full URI (with brackets) and values are unique.
    assert set(predicate_map.keys()) == {
        "<http://ogp.me/ns#title>",
        "<http://purl.org/stuff/rev#title>",
    }
    names = list(predicate_map.values())
    assert len(set(names)) == len(names), f"duplicate names: {names}"

    # One keeps the plain sanitized form; the loser gets a dict-id suffix.
    assert "title" in names
    loser_uri = "<http://purl.org/stuff/rev#title>"
    loser_name = predicate_map[loser_uri]
    assert loser_name.startswith("title_"), loser_name
    assert loser_name.endswith(str(uri_to_id[loser_uri])), loser_name

    # Both Parquet files were written.
    for name in names:
        assert (tmp_path / f"{name}.parquet").exists()


def test_partitioner_injective_on_distinct_names(tmp_path: Path):
    nt = tmp_path / "simple.nt"
    nt.write_text(
        "<http://s/a> <http://example/p> <http://o/1> .\n"
        "<http://s/b> <http://example/q> <http://o/2> .\n",
        encoding="utf-8",
    )

    uri_to_id = build_dict(nt, tmp_path)
    predicate_map = partition_triples(nt, uri_to_id, tmp_path)

    assert predicate_map == {
        "<http://example/p>": "p",
        "<http://example/q>": "q",
    }
