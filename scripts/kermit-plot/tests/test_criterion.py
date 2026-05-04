"""Criterion artefact parsing: directory_name resolution + per-iter math."""
from __future__ import annotations

import pytest

from kermit_plot.criterion import load_function, resolve_function_dir


def test_resolves_function_id_with_slashes(fixture_tree) -> None:
    new_dir = resolve_function_dir(
        fixture_tree["criterion_root"], "run", "TreeTrie/triangle/10/iteration"
    )
    assert new_dir.name == "new"
    assert new_dir.parent.name == "TreeTrie_triangle_10_iteration"


def test_load_function_populates_estimate_and_samples(fixture_tree) -> None:
    data = load_function(
        fixture_tree["criterion_root"], "run", "TreeTrie/triangle/100/iteration"
    )
    assert data.function == "TreeTrie/triangle/100/iteration"
    assert data.mean.point == pytest.approx(10000.0)  # 100.0 * 100
    assert data.mean.lower < data.mean.point < data.mean.upper
    assert len(data.iters) == 10
    assert len(data.times) == 10


def test_per_iter_times_divides_total_by_iters(fixture_tree) -> None:
    data = load_function(
        fixture_tree["criterion_root"], "run", "TreeTrie/triangle/10/iteration"
    )
    # Fixture builder writes total = point * (i + 1), iters = i + 1, so per-iter is constant.
    assert all(t == pytest.approx(1000.0) for t in data.per_iter_times)


def test_unknown_function_raises(fixture_tree) -> None:
    with pytest.raises(FileNotFoundError):
        resolve_function_dir(fixture_tree["criterion_root"], "run", "Nope/never")
