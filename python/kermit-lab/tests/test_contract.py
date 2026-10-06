"""Contract test: run the real ``kermit`` binary and load its output.

Skipped unless ``KERMIT_BIN`` names a built binary (CI sets it after
``cargo build -p kermit``). This is the only test that catches a Criterion
JSON-layout change or a Rust-side axis rename.
"""
from __future__ import annotations

import os
import subprocess
from pathlib import Path

import pytest

import kermit_lab as kl

KERMIT_BIN = os.environ.get("KERMIT_BIN")
WORKSPACE = Path(__file__).resolve().parents[3]
assert (WORKSPACE / "Cargo.toml").is_file(), WORKSPACE
FIXTURES = WORKSPACE / "kermit" / "tests" / "fixtures"
FAST = ["--sample-size", "10", "--measurement-time", "1", "--warm-up-time", "1"]

pytestmark = pytest.mark.skipif(not KERMIT_BIN, reason="KERMIT_BIN not set")


def _run(cwd: Path, report: Path, *args: str) -> None:
    """Run ``kermit bench`` in ``cwd`` so Criterion writes under
    ``cwd/target/criterion`` rather than into the repo tree; the binary
    resolves ``benchmarks/`` via ``KERMIT_WORKSPACE`` instead of the cwd."""
    env = {**os.environ, "KERMIT_WORKSPACE": str(WORKSPACE)}
    proc = subprocess.run(
        [KERMIT_BIN, "bench", *FAST, "--report-json", str(report), *args],
        cwd=cwd,
        env=env,
        check=False,
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 0, (
        f"kermit exited {proc.returncode}\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
    )


def test_bench_ds_report_loads(tmp_path: Path) -> None:
    report = tmp_path / "ds.json"
    _run(
        tmp_path, report,
        "ds", "--relation", str(FIXTURES / "edge.csv"), "-i", "tree-trie", "-m", "space",
    )
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion")
    assert len(df) == 1
    row = df.iloc[0]
    assert row["kind"] == "ds"
    assert row["metric"] == "space"
    assert row["data_structure"] == "TreeTrie"
    assert row["mean_ns"] > 0


def test_bench_run_verify_reaches_the_frame(tmp_path: Path) -> None:
    report = tmp_path / "run.json"
    _run(
        tmp_path, report,
        "run", "triangle", "-i", "tree-trie", "-a", "leapfrog-triejoin",
        "-m", "iteration", "--verify",
    )
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion")
    assert len(df) == 1
    row = df.iloc[0]
    assert row["benchmark"] == "triangle"
    assert row["query"] == "triangle"
    assert bool(row["verified"]) is True


def test_bench_ds_column_trie_reports_its_build_mode(tmp_path: Path) -> None:
    report = tmp_path / "ds.json"
    _run(
        tmp_path, report,
        "ds", "--relation", str(FIXTURES / "edge.csv"), "-i", "column-trie", "-m", "space",
        "--ds-build", "column-trie=incremental",
    )
    # Without the back-fill, so a missing key cannot pass as "incremental".
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion", apply_defaults=False)
    assert len(df) == 1
    assert df.iloc[0]["ds_build_mode"] == "incremental"


def test_bench_ds_reports_its_allocator(tmp_path: Path) -> None:
    report = tmp_path / "ds.json"
    _run(
        tmp_path, report,
        "ds", "--relation", str(FIXTURES / "edge.csv"), "-i", "tree-trie", "-m", "space",
    )
    # Without the back-fill, so a missing key cannot pass as "system" (#112).
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion", apply_defaults=False)
    assert len(df) == 1
    assert df.iloc[0]["allocator"] in {"jemalloc", "system"}


def test_bench_join_reports_its_seek_strategy(tmp_path: Path) -> None:
    report = tmp_path / "join.json"
    _run(
        tmp_path, report,
        "join", "--relations", str(FIXTURES / "first.csv"), str(FIXTURES / "second.csv"),
        "--query", str(FIXTURES / "intersect_query.dl"),
        "-i", "tree-trie", "-a", "leapfrog-triejoin", "-m", "space",
        "--ds-layout-seek", "galloping",
    )
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion")
    assert len(df) >= 1
    assert set(df["ds_layout_seek"]) == {"galloping"}


def test_bench_ds_tree_trie_reports_its_parallel_build(tmp_path: Path) -> None:
    report = tmp_path / "ds.json"
    _run(
        tmp_path, report,
        "ds", "--relation", str(FIXTURES / "edge.csv"), "-i", "tree-trie", "-m", "space",
        "--ds-build", "tree-trie=parallel:2",
    )
    # Without the back-fill, so a missing key cannot pass as "serial".
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion", apply_defaults=False)
    assert len(df) == 1
    assert df.iloc[0]["ds_build_mode"] == "parallel:2"
    assert df.iloc[0]["threads"] == 2


def test_bench_ds_hash_trie_reports_its_parallel_build(tmp_path: Path) -> None:
    report = tmp_path / "ds.json"
    _run(
        tmp_path, report,
        "ds", "--relation", str(FIXTURES / "edge.csv"), "-i", "hash-trie", "-m", "space",
        "--ds-build", "hash-trie=parallel:2",
    )
    # Without the back-fill, so a missing key cannot pass as "serial".
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion", apply_defaults=False)
    assert len(df) == 1
    assert df.iloc[0]["ds_build_mode"] == "parallel:2"
    assert df.iloc[0]["threads"] == 2


def test_bench_join_reports_its_column_orders(tmp_path: Path) -> None:
    report = tmp_path / "join.json"
    _run(
        tmp_path, report,
        "join", "--relations", str(FIXTURES / "first.csv"), str(FIXTURES / "second.csv"),
        "--query", str(FIXTURES / "intersect_query.dl"),
        "-i", "tree-trie", "-a", "leapfrog-triejoin", "-m", "space",
        "--column-orders", "any",
    )
    # Without the back-fill, so a missing key cannot pass as "stored".
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion", apply_defaults=False)
    assert len(df) >= 1
    assert set(df["column_orders"]) == {"any"}


def test_bench_join_copies_reach_the_frame(tmp_path: Path) -> None:
    """A query `stored` rejects runs under `any` over a reordered copy: the
    real binary's `copies` function loads as the `copies` phase and the
    copy's footprint as `space/Index_1_0_edge` (#93)."""
    query = tmp_path / "mutual.dl"
    query.write_text("Q(X, Y) :- edge(X, Y), edge(Y, X).\n")
    report = tmp_path / "join.json"
    _run(
        tmp_path, report,
        "join", "--relations", str(FIXTURES / "edge.csv"), "--query", str(query),
        "-i", "tree-trie", "-a", "leapfrog-triejoin", "-m", "insertion", "space",
        "--column-orders", "any",
    )
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion")
    assert set(df["phase"].dropna()) == {"insertion", "copies"}
    space = set(df.loc[df["metric"] == "space", "criterion_function"])
    assert space == {"space/edge", "space/Index_1_0_edge"}
