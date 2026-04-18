"""Smoke test for the `run_pipeline` orchestrator.

Runs the pipeline end-to-end against the committed mini fixture at
``kermit/tests/fixtures/watdiv-mini/`` to pin the counts returned by
`PipelineResult` and confirm `expected.json` is not written when no
`expected_writer` is supplied.
"""
from __future__ import annotations

from pathlib import Path

from watdiv_preprocess.pipeline import PipelineResult, run_pipeline

REPO_ROOT = Path(__file__).resolve().parent.parent.parent.parent
FIXTURE_DIR = REPO_ROOT / "kermit" / "tests" / "fixtures" / "watdiv-mini"


def test_run_pipeline_on_mini_fixture(tmp_path: Path):
    result = run_pipeline(
        nt_path=FIXTURE_DIR / "watdiv.mini.nt",
        sparql_files=[FIXTURE_DIR / "watdiv-stress-mini" / "tiny.sparql"],
        output_dir=tmp_path,
        base_url="https://example.invalid/watdiv/",
        expected_writer=None,
    )

    assert isinstance(result, PipelineResult)
    assert result.dict_size == 20
    assert result.partition_count == 3
    assert result.yaml_count == 1
    assert result.expected_count == 0

    assert not (tmp_path / "expected.json").exists(), (
        "expected.json should be absent when expected_writer is None"
    )
    assert (tmp_path / "dict.json").exists()
    assert (tmp_path / "dict.parquet").exists()
    assert (tmp_path / "watdiv-stress-mini-tiny.yml").exists()


def test_run_pipeline_invokes_expected_writer(tmp_path: Path):
    calls: list[Path] = []

    def writer(out_path: Path) -> int:
        calls.append(out_path)
        out_path.write_text('{"foo": 1}', encoding="utf-8")
        return 1

    result = run_pipeline(
        nt_path=FIXTURE_DIR / "watdiv.mini.nt",
        sparql_files=[FIXTURE_DIR / "watdiv-stress-mini" / "tiny.sparql"],
        output_dir=tmp_path,
        base_url="https://example.invalid/watdiv/",
        expected_writer=writer,
    )

    assert calls == [tmp_path / "expected.json"]
    assert result.expected_count == 1
    assert (tmp_path / "expected.json").read_text(encoding="utf-8") == '{"foo": 1}'
