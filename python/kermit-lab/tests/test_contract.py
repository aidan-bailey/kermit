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
