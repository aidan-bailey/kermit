"""Criterion artefact parsing: directory_name resolution + per-iter math."""
from __future__ import annotations

import json
from pathlib import Path

import pytest

from kermit_lab.criterion import load_function, resolve_function_dir


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


def test_load_function_handles_null_slope(tmp_path: Path) -> None:
    """Criterion writes ``"slope": null`` for deterministic / zero-variance
    measurements (e.g. ``SpaceMeasurement`` results). Loading must not crash.
    """
    criterion_root = tmp_path / "target" / "criterion"
    new_dir = criterion_root / "g" / "f" / "new"
    new_dir.mkdir(parents=True)
    estimate = {
        "confidence_interval": {
            "confidence_level": 0.95,
            "lower_bound": 8272.0,
            "upper_bound": 8272.0,
        },
        "point_estimate": 8272.0,
        "standard_error": 0.0,
    }
    (new_dir / "benchmark.json").write_text(
        json.dumps({"group_id": "g", "function_id": "f", "directory_name": "g/f"})
    )
    (new_dir / "estimates.json").write_text(
        json.dumps(
            {
                "mean": estimate,
                "median": estimate,
                "median_abs_dev": estimate,
                "slope": None,
                "std_dev": estimate,
            }
        )
    )
    (new_dir / "sample.json").write_text(
        json.dumps({"sampling_mode": "Linear", "iters": [1.0], "times": [8272.0]})
    )

    data = load_function(criterion_root, "g", "f")
    assert data.slope is None
    assert data.mean.point == pytest.approx(8272.0)


def test_resolves_group_with_slashes(tmp_path: Path) -> None:
    """Real ``bench run`` reports use slashed group names like
    ``run/<benchmark>/<query>/<DS>/<algo>``. Criterion flattens the slashes
    to underscores on disk; the resolver must apply the same translation.
    """
    criterion_root = tmp_path / "target" / "criterion"
    group = "run/oxford-uniform-s1/triangle/TreeTrie/LeapfrogTriejoin"
    function_id = "iteration"
    on_disk_group = group.replace("/", "_")
    new_dir = criterion_root / on_disk_group / function_id / "new"
    new_dir.mkdir(parents=True)
    (new_dir / "benchmark.json").write_text(
        json.dumps(
            {
                "group_id": group,
                "function_id": function_id,
                "directory_name": f"{on_disk_group}/{function_id}",
            }
        )
    )

    resolved = resolve_function_dir(criterion_root, group, function_id)
    assert resolved == new_dir


# Criterion 0.8.2 clamps each on-disk directory name to 64 bytes
# (`report::make_filename_safe`), so realistic `bench run` groups land in
# truncated directories (issue #69). Two groups sharing that 64-byte prefix
# share a directory: inside one process Criterion keeps them apart with an
# `_2` suffix on the function directory; a later process overwrites instead.
_LONG_PREFIX = "run/" + "watdiv-stress-100-test-1-prelim" + "-x" * 20
_GROUP_TREE = f"{_LONG_PREFIX}/q0000/TreeTrie/LeapfrogTriejoin"
_GROUP_COLUMN = f"{_LONG_PREFIX}/q0000/ColumnTrie/LeapfrogTriejoin"
_TRUNCATED = _LONG_PREFIX.replace("/", "_")[:64]


def _write_benchmark_json(new_dir: Path, group: str, function_id: str) -> Path:
    new_dir.mkdir(parents=True)
    (new_dir / "benchmark.json").write_text(
        json.dumps(
            {
                "group_id": group,
                "function_id": function_id,
                "directory_name": f"{new_dir.parent.parent.name}/{new_dir.parent.name}",
            }
        )
    )
    return new_dir


def test_resolves_group_whose_directory_name_criterion_truncated(tmp_path: Path) -> None:
    assert len(_GROUP_TREE.replace("/", "_")) > 64
    new_dir = _write_benchmark_json(
        tmp_path / _TRUNCATED / "iteration" / "new", _GROUP_TREE, "iteration"
    )

    assert resolve_function_dir(tmp_path, _GROUP_TREE, "iteration") == new_dir


def test_tells_apart_groups_sharing_a_truncated_directory(tmp_path: Path) -> None:
    """One process, two colliding groups: Criterion suffixed the second
    function directory. Both carry ``function_id: iteration``, so matching on
    ``function_id`` alone would hand back whichever sorts first."""
    tree_dir = _write_benchmark_json(
        tmp_path / _TRUNCATED / "iteration_2" / "new", _GROUP_TREE, "iteration"
    )
    column_dir = _write_benchmark_json(
        tmp_path / _TRUNCATED / "iteration" / "new", _GROUP_COLUMN, "iteration"
    )

    assert resolve_function_dir(tmp_path, _GROUP_TREE, "iteration") == tree_dir
    assert resolve_function_dir(tmp_path, _GROUP_COLUMN, "iteration") == column_dir


def test_overwritten_group_error_names_the_group_that_replaced_it(tmp_path: Path) -> None:
    """A later process wrote a colliding group into the same directory, so
    the requested group's results are gone; say why instead of just 'not
    found'."""
    _write_benchmark_json(tmp_path / _TRUNCATED / "iteration" / "new", _GROUP_COLUMN, "iteration")

    with pytest.raises(FileNotFoundError) as excinfo:
        resolve_function_dir(tmp_path, _GROUP_TREE, "iteration")

    message = str(excinfo.value)
    assert _GROUP_COLUMN in message
    assert "64" in message


def test_two_directories_claiming_one_function_is_an_error(tmp_path: Path) -> None:
    """Colliding groups run together (the second lands in ``iteration_2``),
    then one is re-run alone and lands in ``iteration``: both directories now
    claim it, one of them stale. Refuse to guess."""
    _write_benchmark_json(tmp_path / _TRUNCATED / "iteration" / "new", _GROUP_TREE, "iteration")
    _write_benchmark_json(tmp_path / _TRUNCATED / "iteration_2" / "new", _GROUP_TREE, "iteration")

    with pytest.raises(ValueError, match="iteration_2"):
        resolve_function_dir(tmp_path, _GROUP_TREE, "iteration")


def test_base_directories_are_not_candidates(tmp_path: Path) -> None:
    """Criterion keeps the previous run's copy under ``base/``; only ``new/``
    is the current result, so ``base/`` must not count as a second claim."""
    new_dir = _write_benchmark_json(
        tmp_path / _TRUNCATED / "iteration" / "new", _GROUP_TREE, "iteration"
    )
    _write_benchmark_json(tmp_path / _TRUNCATED / "iteration" / "base", _GROUP_TREE, "iteration")

    assert resolve_function_dir(tmp_path, _GROUP_TREE, "iteration") == new_dir
