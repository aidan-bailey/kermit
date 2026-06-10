"""``kermit-lab`` argparse dispatcher over the general plot engine + presets."""
from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path

import matplotlib.pyplot as plt

from .. import presets
from ..frame import load, load_samples
from ..loader import load_reports
from ..plot import plot
from ..plots_errors import InsufficientAxesError
from ..styles import apply as apply_style
from . import render_all

log = logging.getLogger("kermit-lab")


def _add_common(p: argparse.ArgumentParser) -> None:
    p.add_argument("reports", nargs="+", type=Path, help="BenchReport JSON file(s)")
    p.add_argument("--out", type=Path, required=True,
                   help="output file path (suffix determines format: pdf, png, svg, pgf)")
    p.add_argument("--criterion-root", type=Path, default=Path("target/criterion"),
                   help="Criterion artefact directory (default: target/criterion)")


def _add_phase(p: argparse.ArgumentParser) -> None:
    p.add_argument("--phase", choices=["insertion", "iteration"], default="iteration",
                   help="time-metric phase to plot (default: iteration)")


def _parse_filter(items: list[str] | None) -> dict[str, str]:
    out: dict[str, str] = {}
    for item in items or []:
        if "=" not in item:
            raise SystemExit(f"--filter expects k=v, got {item!r}")
        k, v = item.split("=", 1)
        out[k] = v
    return out


def _build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="kermit-lab",
                                description="Render thesis-quality plots from kermit BenchReport JSON.")
    p.add_argument("-v", "--verbose", action="store_true", help="enable info-level logging")
    sub = p.add_subparsers(dest="command", required=True)

    p_plot = sub.add_parser("plot", help="general plot: bind any axis to any channel")
    _add_common(p_plot)
    _add_phase(p_plot)
    p_plot.add_argument("--kind", required=True, choices=["bar", "line", "scatter", "violin"])
    p_plot.add_argument("--x", default=None, help="x column (or 'space'/'time' for tradeoff)")
    p_plot.add_argument("--y", default="time", choices=["time", "space"])
    p_plot.add_argument("--colour", default=None)
    p_plot.add_argument("--style", default=None)
    p_plot.add_argument("--facet", default=None)
    p_plot.add_argument("--filter", action="append", default=None, metavar="K=V")
    p_plot.add_argument("--logx", action="store_true")
    p_plot.add_argument("--logy", action="store_true")

    p_scaling = sub.add_parser("scaling", help="log-log scaling preset")
    _add_common(p_scaling)
    _add_phase(p_scaling)

    p_bar_time = sub.add_parser("bar-time", help="time across DS for one query")
    _add_common(p_bar_time)
    _add_phase(p_bar_time)
    p_bar_time.add_argument("--query", required=True)

    p_bar_space = sub.add_parser("bar-space", help="space across DS")
    _add_common(p_bar_space)

    p_tradeoff = sub.add_parser("tradeoff", help="space vs time scatter")
    _add_common(p_tradeoff)
    _add_phase(p_tradeoff)

    p_dist = sub.add_parser("dist", help="violin of per-iter samples")
    _add_common(p_dist)
    _add_phase(p_dist)

    p_bar_queries = sub.add_parser("bar-queries", help="bars across queries")
    _add_common(p_bar_queries)
    _add_phase(p_bar_queries)
    p_bar_queries.add_argument("--ds", nargs="+", default=None)
    p_bar_queries.add_argument("--algo", nargs="+", default=None)

    p_ablation = sub.add_parser("ablation", help="time vs an optimization axis")
    _add_common(p_ablation)
    _add_phase(p_ablation)
    p_ablation.add_argument("--axis", required=True, help="optimization axis column name")

    p_render_all = sub.add_parser("render-all", help="render every applicable shape into --out-dir")
    p_render_all.add_argument("reports", nargs="+", type=Path)
    p_render_all.add_argument("--out-dir", type=Path, required=True)
    p_render_all.add_argument("--criterion-root", type=Path, default=Path("target/criterion"))
    p_render_all.add_argument("--format", default="pdf", choices=["pdf", "png", "svg", "pgf"])
    _add_phase(p_render_all)
    return p


def _dispatch(args: argparse.Namespace) -> int:
    df = load(args.reports, args.criterion_root)
    log.info("loaded %d row(s) from %d file(s)", len(df), len(args.reports))

    if args.command == "plot":
        fig = plot(df, kind=args.kind, x=args.x, y=args.y, colour=args.colour,
                   style=args.style, facet=args.facet, filter=_parse_filter(args.filter),
                   phase=args.phase, logx=args.logx, logy=args.logy, out=args.out)
    elif args.command == "scaling":
        fig = presets.scaling(df, phase=args.phase, out=args.out)
    elif args.command == "bar-time":
        fig = presets.bar_time(df, query=args.query, phase=args.phase, out=args.out)
    elif args.command == "bar-space":
        fig = presets.bar_space(df, out=args.out)
    elif args.command == "tradeoff":
        fig = presets.tradeoff(df, phase=args.phase, out=args.out)
    elif args.command == "dist":
        samples = load_samples(args.reports, args.criterion_root)
        fig = presets.dist(df, samples=samples, phase=args.phase, out=args.out)
    elif args.command == "bar-queries":
        fig = presets.bar_queries(df, ds=args.ds, algo=args.algo, phase=args.phase, out=args.out)
    elif args.command == "ablation":
        fig = presets.ablation(df, axis=args.axis, phase=args.phase, out=args.out)
    else:
        log.error("unknown command: %s", args.command)
        return 2
    plt.close(fig)
    return 0


def main(argv: list[str] | None = None) -> int:
    args = _build_parser().parse_args(argv)
    logging.basicConfig(
        level=logging.INFO if args.verbose else logging.WARNING,
        format="%(name)s %(levelname)s: %(message)s",
    )
    apply_style()
    try:
        if args.command == "render-all":
            args.out_dir.mkdir(parents=True, exist_ok=True)
            reports = load_reports(args.reports)
            log.info("loaded %d report(s)", len(reports))
            render_all.render_all(reports, args.out_dir, args.criterion_root,
                                  args.format, phase=args.phase)
            return 0
        rc = _dispatch(args)
        if rc != 0:
            return rc
    except InsufficientAxesError as e:
        log.error("%s: %s", args.command, e)
        return 3
    log.info("wrote %s", args.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
