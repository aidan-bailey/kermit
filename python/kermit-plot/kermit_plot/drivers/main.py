"""``kermit-plot`` argparse dispatcher.

Each plot module exports a ``render(reports, out_path, criterion_root, **kwargs)``
function; the CLI maps subcommands to those modules and threads kwargs.
"""
from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path

from ..loader import load_reports
from ..plots import InsufficientAxesError
from ..plots import bar_queries, bar_space, bar_time, dist, scaling, tradeoff
from ..styles import apply as apply_style
from . import render_all

log = logging.getLogger("kermit-plot")


def _add_common(p: argparse.ArgumentParser) -> None:
    p.add_argument("reports", nargs="+", type=Path, help="BenchReport JSON file(s)")
    p.add_argument(
        "--out",
        type=Path,
        required=True,
        help="output file path (suffix determines format: pdf, png, svg, pgf)",
    )
    p.add_argument(
        "--criterion-root",
        type=Path,
        default=Path("target/criterion"),
        help="Criterion artefact directory (default: target/criterion)",
    )


def _add_phase(p: argparse.ArgumentParser) -> None:
    p.add_argument(
        "--phase",
        choices=["insertion", "iteration"],
        default="iteration",
        help="time-metric phase to plot (default: iteration — the join-execution phase)",
    )


def _build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="kermit-plot",
        description="Render thesis-quality plots from kermit BenchReport JSON.",
    )
    p.add_argument(
        "-v",
        "--verbose",
        action="store_true",
        help="enable info-level logging",
    )
    sub = p.add_subparsers(dest="command", required=True)

    p_scaling = sub.add_parser("scaling", help="log-log scaling plot")
    _add_common(p_scaling)
    _add_phase(p_scaling)

    p_bar_time = sub.add_parser("bar-time", help="bar+CI of time across (DS, algorithm)")
    _add_common(p_bar_time)
    _add_phase(p_bar_time)
    p_bar_time.add_argument("--query", required=True, help="query name to filter on")

    p_bar_space = sub.add_parser("bar-space", help="bar of heap_size_bytes across DS")
    _add_common(p_bar_space)

    p_tradeoff = sub.add_parser("tradeoff", help="space vs time scatter")
    _add_common(p_tradeoff)
    _add_phase(p_tradeoff)

    p_dist = sub.add_parser("dist", help="violin / box of per-iter samples")
    _add_common(p_dist)
    _add_phase(p_dist)

    p_bar_queries = sub.add_parser("bar-queries", help="bar across queries")
    _add_common(p_bar_queries)
    _add_phase(p_bar_queries)
    p_bar_queries.add_argument("--ds", required=True, help="data_structure to filter on")
    p_bar_queries.add_argument("--algo", required=True, help="algorithm to filter on")

    p_render_all = sub.add_parser(
        "render-all",
        help="render every applicable shape into --out-dir (skip those lacking required axes)",
    )
    p_render_all.add_argument("reports", nargs="+", type=Path, help="BenchReport JSON file(s)")
    p_render_all.add_argument(
        "--out-dir",
        type=Path,
        required=True,
        help="output directory (created if missing)",
    )
    p_render_all.add_argument(
        "--criterion-root",
        type=Path,
        default=Path("target/criterion"),
        help="Criterion artefact directory (default: target/criterion)",
    )
    p_render_all.add_argument(
        "--format",
        default="pdf",
        choices=["pdf", "png", "svg", "pgf"],
        help="output format for every emitted plot (default: pdf)",
    )
    _add_phase(p_render_all)
    return p


def main(argv: list[str] | None = None) -> int:
    """Argparse entry; dispatches to one plot module or to ``render-all``."""
    args = _build_parser().parse_args(argv)
    logging.basicConfig(
        level=logging.INFO if args.verbose else logging.WARNING,
        format="%(name)s %(levelname)s: %(message)s",
    )
    apply_style()

    reports = load_reports(args.reports)
    log.info("loaded %d report(s) from %d file(s)", len(reports), len(args.reports))

    try:
        if args.command == "render-all":
            args.out_dir.mkdir(parents=True, exist_ok=True)
            render_all.render_all(
                reports,
                args.out_dir,
                args.criterion_root,
                args.format,
                phase=args.phase,
            )
            return 0

        if args.command == "scaling":
            scaling.render(reports, args.out, args.criterion_root, phase=args.phase)
        elif args.command == "bar-time":
            bar_time.render(
                reports, args.out, args.criterion_root, query=args.query, phase=args.phase
            )
        elif args.command == "bar-space":
            bar_space.render(reports, args.out, args.criterion_root)
        elif args.command == "tradeoff":
            tradeoff.render(reports, args.out, args.criterion_root, phase=args.phase)
        elif args.command == "dist":
            dist.render(reports, args.out, args.criterion_root, phase=args.phase)
        elif args.command == "bar-queries":
            bar_queries.render(
                reports,
                args.out,
                args.criterion_root,
                ds=args.ds,
                algo=args.algo,
                phase=args.phase,
            )
        else:
            log.error("unknown command: %s", args.command)
            return 2
    except InsufficientAxesError as e:
        log.error("%s: %s", args.command, e)
        return 3

    log.info("wrote %s", args.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
