"""Loader: wildcard-wide optimization-axis ingest + samples/opt helpers.

Fixture composition (see ``conftest.py``):
- 6 ``run`` reports (TreeTrie, ColumnTrie × 3 sizes for triangle), each with 3
  criterion groups (insertion + iteration + space) → 18 summary rows.
- 2 ``run`` reports (TreeTrie × {chain, star}, single size 100), each with 1
  criterion group (iteration only) → 2 summary rows.
Total: 20 summary rows; 200 sample rows (20 functions × 10 samples each).
"""
from __future__ import annotations

import pandas as pd
import pytest

from kermit_lab.frame import (
    discover_opt_columns,
    load,
    load_samples,
)


@pytest.fixture
def df(fixture_tree):
    return load(fixture_tree["paths"], fixture_tree["criterion_root"])


@pytest.fixture
def samples(fixture_tree):
    return load_samples(fixture_tree["paths"], fixture_tree["criterion_root"])


def test_load_returns_dataframe(df):
    assert isinstance(df, pd.DataFrame)
    assert len(df) == 20


def test_summary_columns_present(df):
    expected = {
        "kind", "metric", "phase",
        "data_structure", "algorithm", "query", "benchmark", "relation_path",
        "tuples", "arity", "relations", "relation_bytes",
        "verified",
        "mean_ns", "mean_lo", "mean_hi", "mean_se",
        "median_ns", "median_lo", "median_hi",
        "source_path", "criterion_group", "criterion_function",
    }
    assert expected.issubset(df.columns)


def test_phase_column_populated(df):
    # Time rows must have insertion or iteration; space rows must have NA.
    time_rows = df[df.metric == "time"]
    space_rows = df[df.metric == "space"]
    assert set(time_rows.phase.dropna().unique()) == {"insertion", "iteration"}
    assert space_rows.phase.isna().all()


def test_metric_column_values(df):
    assert set(df.metric.unique()) == {"time", "space"}


def test_tuples_is_nullable_int64(df):
    assert str(df.tuples.dtype) == "Int64"
    # The fixture sets tuples on every report, so no NAs expected here.
    assert not df.tuples.isna().any()
    assert set(df.tuples.dropna().unique()) == {10, 100, 1000}


def test_unknown_axes_become_pdNA(df):
    # The fixture never sets `arity` or `relations`, so those columns are
    # entirely NA but still present (and Int64-typed).
    assert df.arity.isna().all()
    assert df.relations.isna().all()
    assert str(df.arity.dtype) == "Int64"


def test_estimates_are_finite_floats(df):
    for col in ("mean_ns", "mean_lo", "mean_hi", "median_ns"):
        assert df[col].dtype.kind == "f"
        assert df[col].notna().all()


def test_load_samples_shape(samples):
    assert isinstance(samples, pd.DataFrame)
    # 20 functions × 10 samples each = 200 rows.
    assert len(samples) == 200


def test_per_iter_ns_matches_total_over_iters(samples):
    # Constructed invariant: per_iter_ns == total_ns / iters.
    expected = samples.total_ns / samples.iters
    pd.testing.assert_series_equal(samples.per_iter_ns, expected, check_names=False)


def test_samples_join_keys_in_summary(df, samples):
    # Every (group, function) in samples must exist in summary.
    sample_keys = set(zip(samples.criterion_group, samples.criterion_function))
    summary_keys = set(zip(df.criterion_group, df.criterion_function))
    assert sample_keys.issubset(summary_keys)


def test_load_accepts_glob_string(fixture_tree):
    # A bare glob pattern as a single string should expand to every match,
    # matching what every README snippet and notebook implies.
    pattern = str(fixture_tree["reports_dir"] / "*.json")
    df = load(pattern, fixture_tree["criterion_root"])
    assert len(df) == 20  # same as test_load_returns_dataframe


def test_load_raises_when_glob_matches_nothing(fixture_tree):
    import pytest as _pytest
    with _pytest.raises(FileNotFoundError, match="no files match"):
        load(str(fixture_tree["reports_dir"] / "missing-*.json"), fixture_tree["criterion_root"])


def test_load_accepts_single_path_string(fixture_tree):
    one_path = str(fixture_tree["paths"][0])
    df = load(one_path, fixture_tree["criterion_root"])
    assert len(df) >= 1  # one report → ≥1 criterion_group rows


# --- New Task 2 tests: optimization-axis ingest ---


def test_optimization_axes_become_columns(fixture_opt_tree) -> None:
    df = load(fixture_opt_tree["paths"], fixture_opt_tree["criterion_root"])
    assert "ds_layout_hasher" in df.columns
    assert "ds_config_load_factor" in df.columns
    assert set(df["ds_layout_hasher"].dropna()) == {"sip", "fx"}
    # config value stays float-valued
    assert set(df["ds_config_load_factor"].dropna()) == {0.5, 0.7}


def test_conventional_run_has_no_opt_columns(fixture_tree) -> None:
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert discover_opt_columns(df) == []


def test_default_backfill_applied(fixture_tree) -> None:
    # fixture_tree has no ds_layout_hasher; default must NOT fabricate a column.
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert "ds_layout_hasher" not in df.columns


def test_discover_opt_columns(fixture_opt_tree) -> None:
    df = load(fixture_opt_tree["paths"], fixture_opt_tree["criterion_root"])
    cols = discover_opt_columns(df)
    assert "ds_layout_hasher" in cols
    assert "ds_config_load_factor" in cols


def test_load_samples_unchanged(fixture_tree) -> None:
    s = load_samples(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert {"criterion_group", "criterion_function", "per_iter_ns"} <= set(s.columns)


# --- end-to-end metric: phase + queries_per_build/optimiser axes ---


def test_end_to_end_phase_populated(fixture_end_to_end_tree) -> None:
    df = load(fixture_end_to_end_tree["paths"], fixture_end_to_end_tree["criterion_root"])
    # Every fixture function ends in the end_to_end token, so no NA phases.
    assert set(df.phase.dropna().unique()) == {"end_to_end"}
    assert not df.phase.isna().any()


def test_queries_per_build_column(fixture_end_to_end_tree) -> None:
    df = load(fixture_end_to_end_tree["paths"], fixture_end_to_end_tree["criterion_root"])
    assert "queries_per_build" in df.columns
    assert str(df.queries_per_build.dtype) == "Int64"
    assert set(df.queries_per_build.dropna().unique()) == {1, 4}


def test_optimiser_axis_reaches_frame(fixture_end_to_end_tree) -> None:
    # The optimiser axis has been written by the Rust side since 93be8f5;
    # it must surface as a string column, not be silently dropped.
    df = load(fixture_end_to_end_tree["paths"], fixture_end_to_end_tree["criterion_root"])
    assert "optimiser" in df.columns
    assert set(df.optimiser.dropna().unique()) == {"lexicographic"}


def test_queries_per_build_na_for_legacy_reports(fixture_tree) -> None:
    # Reports that predate the end-to-end metric carry no queries_per_build
    # axis; the column must still exist (NA-filled), so old and new report
    # sets concatenate cleanly in one frame.
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert "queries_per_build" in df.columns
    assert df.queries_per_build.isna().all()


def test_verified_column_is_nullable_boolean(verified_tree):
    df = load(verified_tree["paths"], verified_tree["criterion_root"])
    assert "verified" in df.columns
    assert str(df["verified"].dtype) == "boolean"
    by_group = df.set_index("criterion_group")["verified"]
    assert bool(by_group["run/triangle/triangle/TreeTrie/LeapfrogTriejoin/v"]) is True
    assert pd.isna(by_group["run/triangle/triangle/TreeTrie/LeapfrogTriejoin/nv"])
