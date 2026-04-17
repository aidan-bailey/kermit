"""Smoke tests for the generic `sparql-preprocess` driver.

Exercises `drivers.generic.main` via `sys.argv` monkeypatching rather
than spawning a subprocess, so the whole test stays inside pytest.
"""
from __future__ import annotations

import json
import shutil
from pathlib import Path

import pytest

from watdiv_preprocess.drivers import generic

REPO_ROOT = Path(__file__).resolve().parent.parent.parent.parent
FIXTURE_DIR = REPO_ROOT / "kermit" / "tests" / "fixtures" / "watdiv-mini"


def _materialise_fixture(tmp_path: Path) -> Path:
    """Copies the mini fixture's inputs (not artifacts) into ``tmp_path``."""
    dst = tmp_path / "workload"
    (dst / "watdiv-stress-mini").mkdir(parents=True)
    shutil.copy(FIXTURE_DIR / "watdiv.mini.nt", dst / "watdiv.mini.nt")
    shutil.copy(
        FIXTURE_DIR / "watdiv-stress-mini" / "tiny.sparql",
        dst / "watdiv-stress-mini" / "tiny.sparql",
    )
    return dst


def test_generic_driver_smoke(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    src = _materialise_fixture(tmp_path)
    out = tmp_path / "out"

    monkeypatch.setattr(
        "sys.argv",
        [
            "sparql-preprocess",
            "--input",
            str(src),
            "--nt",
            str(src / "watdiv.mini.nt"),
            "--sparql-glob",
            "watdiv-stress-mini/*.sparql",
            "--output",
            str(out),
            "--base-url",
            "https://example.invalid/watdiv/",
        ],
    )
    rc = generic.main()

    assert rc == 0
    assert (out / "dict.json").exists()
    assert (out / "dict.parquet").exists()
    assert (out / "watdiv-stress-mini-tiny.yml").exists()
    # Three URI-object predicates in the fixture.
    assert {p.name for p in out.glob("*.parquet")} == {
        "dict.parquet",
        "eligibleregion.parquet",
        "includes.parquet",
        "parentcountry.parquet",
    }
    # No --expected-json passed → no expected.json produced.
    assert not (out / "expected.json").exists()


def test_generic_driver_copies_expected_json(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    src = _materialise_fixture(tmp_path)
    out = tmp_path / "out"

    expected_in = tmp_path / "my-expected.json"
    payload = {"watdiv-stress-mini-tiny::q0000": 1}
    expected_in.write_text(json.dumps(payload), encoding="utf-8")

    monkeypatch.setattr(
        "sys.argv",
        [
            "sparql-preprocess",
            "--input",
            str(src),
            "--nt",
            str(src / "watdiv.mini.nt"),
            "--sparql-glob",
            "watdiv-stress-mini/*.sparql",
            "--output",
            str(out),
            "--base-url",
            "https://example.invalid/watdiv/",
            "--expected-json",
            str(expected_in),
        ],
    )
    rc = generic.main()

    assert rc == 0
    written = out / "expected.json"
    assert written.exists()
    assert json.loads(written.read_text(encoding="utf-8")) == payload


def test_generic_driver_default_glob(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    # No --sparql-glob provided → default "**/*.sparql" walks subdirs.
    src = _materialise_fixture(tmp_path)
    out = tmp_path / "out"

    monkeypatch.setattr(
        "sys.argv",
        [
            "sparql-preprocess",
            "--input",
            str(src),
            "--nt",
            str(src / "watdiv.mini.nt"),
            "--output",
            str(out),
            "--base-url",
            "https://example.invalid/watdiv/",
        ],
    )
    rc = generic.main()
    assert rc == 0
    assert (out / "watdiv-stress-mini-tiny.yml").exists()
