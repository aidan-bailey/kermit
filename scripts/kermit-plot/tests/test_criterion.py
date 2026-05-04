"""Criterion artefact parsing: directory_name resolution + per-iter math."""
from __future__ import annotations

import json
from pathlib import Path

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
