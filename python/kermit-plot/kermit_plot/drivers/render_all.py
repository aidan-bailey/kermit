"""``render-all`` meta-command — emit every plot shape the input set supports.

Iterates the six plot modules in turn. Each ``InsufficientAxesError`` is
demoted to an info-level log message; other exceptions propagate. This is
intentional: a missing axis means "no plot to draw," but a corrupt JSON
report or missing Criterion artefact should fail loudly.
"""
from __future__ import annotations

import logging
from pathlib import Path
from typing import Iterable

from ..loader import BenchReport
from ..plots import InsufficientAxesError, bar_queries, bar_space, bar_time, dist, scaling, tradeoff

log = logging.getLogger(__name__)


def _candidate_queries(reports: Iterable[BenchReport]) -> list[str]:
    qs: set[str] = set()
    for r in reports:
        q = r.axis("query")
        if isinstance(q, str):
            qs.add(q)
    return sorted(qs)


def _candidate_ds_algo(reports: Iterable[BenchReport]) -> list[tuple[str, str]]:
    pairs: set[tuple[str, str]] = set()
    for r in reports:
        ds = r.axis("data_structure")
        algo = r.axis("algorithm")
        if isinstance(ds, str) and isinstance(algo, str):
            pairs.add((ds, algo))
    return sorted(pairs)


def render_all(
    reports: list[BenchReport],
    out_dir: Path,
    criterion_root: Path,
    fmt: str,
    *,
    phase: str = "iteration",
) -> None:
    """Render every applicable shape; skip with info-log on insufficient axes.

    ``phase`` is forwarded to every time-using shape (scaling, bar-time,
    bar-queries, tradeoff, dist) so insertion- and iteration-time can be
    rendered into separate output sets without re-running benchmarks.
    """
    suffix = f".{fmt}"

    def _try(label: str, fn) -> None:
        try:
            fn()
            log.info("rendered %s", label)
        except InsufficientAxesError as e:
            log.info("skipped %s: %s", label, e)

    _try(
        f"scaling{suffix}",
        lambda: scaling.render(
            reports, out_dir / f"scaling{suffix}", criterion_root, phase=phase
        ),
    )
    _try(
        f"bar-space{suffix}",
        lambda: bar_space.render(reports, out_dir / f"bar-space{suffix}", criterion_root),
    )
    _try(
        f"tradeoff{suffix}",
        lambda: tradeoff.render(
            reports, out_dir / f"tradeoff{suffix}", criterion_root, phase=phase
        ),
    )
    _try(
        f"dist{suffix}",
        lambda: dist.render(reports, out_dir / f"dist{suffix}", criterion_root, phase=phase),
    )
    for query in _candidate_queries(reports):
        _try(
            f"bar-time-{query}{suffix}",
            lambda q=query: bar_time.render(
                reports,
                out_dir / f"bar-time-{q}{suffix}",
                criterion_root,
                query=q,
                phase=phase,
            ),
        )
    for ds, algo in _candidate_ds_algo(reports):
        _try(
            f"bar-queries-{ds}-{algo}{suffix}",
            lambda d=ds, a=algo: bar_queries.render(
                reports,
                out_dir / f"bar-queries-{d}-{a}{suffix}",
                criterion_root,
                ds=d,
                algo=a,
                phase=phase,
            ),
        )
