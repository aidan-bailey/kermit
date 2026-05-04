# Remove Criterion auto-plots; add Python plotter

**Date:** 2026-05-04
**Status:** Implementation in progress
**Sibling specs:** `benchmarking-architecture.md`, `space-benchmarks.md`, `bench-report-schema.md`

## Problem

Criterion 0.7's bundled SVG/HTML plotting layer is unsuitable for the thesis:

1. **Style.** Plot fonts, colours, and axes don't match LaTeX-friendly publication
   conventions.
2. **Cross-run aggregation.** A single `kermit bench …` invocation pins exactly one
   `(data_structure, algorithm)` pair. Producing a TreeTrie-vs-ColumnTrie
   comparison for a fixed query — the central comparative shape this thesis
   needs — is impossible inside one Criterion process.
3. **Zero-variance panic.** Criterion's `plotters` backend panics on
   deterministic measurements. `SpaceMeasurement` is deterministic by
   construction (per-iter total = `heap_size_bytes() * iters`), so the
   workaround `.without_plots()` was applied in `build_space_criterion`. This
   is a footgun: any future deterministic measurement axis would re-trigger it.

## Decision

Strip Criterion's rendering layer entirely; keep its measurement output as the
source of truth; render plots in a separate Python tool (`scripts/kermit-plot/`)
that reads the per-function JSON Criterion writes plus the existing
`--report-json` `BenchReport`.

```
┌───────────────────────────────┐
│ kermit bench … --report-json   │ ── stderr metadata
│                                │ ── target/criterion/{group}/{dir}/{base,new}/
│                                │      ├── estimates.json
│                                │      ├── sample.json
│                                │      ├── benchmark.json
│                                │      └── tukey.json
│                                │ ── bench-runs/<run>.json (BenchReport v2)
└───────────────────────────────┘
                 │
                 ▼
┌───────────────────────────────┐
│ kermit-plot <subcmd> *.json    │ ── thesis-quality PDF/PNG/SVG/PGF
└───────────────────────────────┘
```

## Rust-side changes

### 1. Drop Criterion's default features

`kermit/Cargo.toml`:

```toml
criterion = { version = "0.7.0", default-features = false, features = ["rayon", "cargo_bench_support"] }
```

`html_reports` is an empty feature in 0.7.0; the `plotters` dep is in
Criterion's *default* feature set, not gated on `html_reports`. Opting out of
defaults and re-enabling `rayon` + `cargo_bench_support` removes `plotters`
from the dep tree entirely. `tinytemplate` remains as a non-optional Criterion
dep (no panic risk; harmless).

Verify: `cargo tree -p kermit | grep plotters` is empty.

### 2. Remove `.without_plots()` workaround

`kermit/src/main.rs::build_space_criterion`: drop the call and its panic
comment. The plot subsystem doesn't exist anymore — there's nothing to disable.

### 3. `BenchReport` schema v2 — add structured `axes`

`kermit/src/bench_report.rs`:

- `REPORT_SCHEMA_VERSION` bumps `1` → `2`.
- New field on `BenchReport`:
  ```rust
  pub axes: BTreeMap<String, serde_json::Value>,
  ```
  `BTreeMap` so JSON output is alphabetically ordered (diff-friendly,
  deterministic across reruns). `serde_json::Value` so the wire format
  preserves numeric / string / bool / nested values without inventing a
  closed enum.
- `BenchReport::new(kind, metadata, axes, criterion_groups)` — fourth
  argument.
- Conventional keys (each call site populates the subset that's meaningful):
  `data_structure`, `algorithm`, `query`, `benchmark`, `relation_path`,
  `relation_bytes`, `tuples`, `arity`, `relations`. Full catalogue lives in
  `docs/specs/bench-report-schema.md`.
- `metadata` (the label/value array driving the stderr block) **stays** —
  it's the human-readable surface. `axes` is the structured surface for
  tooling. The two are populated from the same locals at each call site;
  duplication is intentional.

### 4. CLAUDE.md gotchas

Two updates:

- **Replace** the "plotters panic / `.without_plots()`" workaround note with
  a "no Criterion auto-plots" gotcha pointing at `scripts/kermit-plot/`.
- **Update** the JSON bench reports gotcha for schema v2: document `axes`,
  conventional keys, schema doc location.

### 5. `criterion.toml` — left untouched

`criterion.toml` only affects the external `cargo-criterion` CLI runner.
`kermit` doesn't use that runner — bench subcommands construct `Criterion`
in-process via `criterion::Criterion::default()`. No-op for in-process
benches; harmless.

### 6. `.gitignore` — add `bench-runs/`

The convention is to write `--report-json` outputs to `bench-runs/` at the
workspace root before passing them to `kermit-plot`. Not enforced in code (the
flag still takes any path), just documented in `scripts/kermit-plot/README.md`
and gitignored to prevent accidentally checking benchmark outputs into the
repo.

## Python-side: `scripts/kermit-plot/`

New project at `scripts/kermit-plot/`, mirroring `scripts/watdiv-preprocess/`'s
layout (pyproject + setuptools, console_scripts, pytest, editable install).

### Layout

```
scripts/kermit-plot/
├── pyproject.toml
├── README.md
├── kermit_plot/
│   ├── __init__.py
│   ├── __main__.py
│   ├── loader.py             # parse BenchReport JSON; resolve directory_name
│   ├── criterion.py          # parse estimates.json / sample.json / benchmark.json
│   ├── axis_mapping.py       # canonical (DS → colour, algo → linestyle/marker)
│   ├── styles/thesis.mplstyle
│   ├── plots/{bar_time,bar_space,scaling,tradeoff,dist,bar_queries}.py
│   └── drivers/{main,render_all}.py
└── tests/
    ├── fixtures/             # synthetic mini Criterion + report JSON
    └── test_*.py
```

### CLI

```
kermit-plot scaling     <report.json>... [--out PATH] [--format {pdf,png,svg,pgf}]
kermit-plot bar-time    <report.json>... --query QUERY [--out PATH] ...
kermit-plot bar-space   <report.json>... [--out PATH] ...
kermit-plot tradeoff    <report.json>... [--out PATH] ...
kermit-plot dist        <report.json>... [--out PATH] ...
kermit-plot bar-queries <report.json>... --ds DS --algo ALGO [--out PATH] ...
kermit-plot render-all  <report.json>... --out-dir DIR [--format ...]
```

`render-all` emits every plot for which the input set has sufficient axes
(e.g. `scaling.<format>` only when `tuples` has ≥2 distinct values across
inputs). Skipped shapes log at info level rather than erroring.

### Data flow

1. User runs one or many `kermit bench run … --report-json bench-runs/<run>.json`
   commands, varying `(DS, algo, dataset)` between invocations.
2. Each invocation writes both `target/criterion/{group}/{dir}/...` artefacts
   and a `BenchReport` JSON.
3. `kermit-plot scaling bench-runs/*.json --out plots/scaling-triangle.pdf`
   loads all reports. For each `CriterionGroupRef{group, function}`, the
   loader reads `target/criterion/{group}/{directory_name}/new/` JSON, where
   `directory_name` is read from each candidate subdir's
   `benchmark.json:directory_name` field (Criterion replaces `/` in
   `function_id` with `_`). Reports are grouped by axes (`data_structure`,
   `algorithm`) and rendered as one log-log line per group over `tuples`.

### Style commitments

- **Wong / Okabe-Ito 8-colour palette** (colour-blind safe), encoded once in
  `axis_mapping.py`.
- `data_structure` → colour; `algorithm` → linestyle (line plots) / marker
  shape (scatter, bar). Adding a new DS or algo means extending one file.
- Error bars always shown — including width-zero for deterministic space
  measurements (defensible; reviewers will ask).
- Axis defaults: `scaling` log-log; `tradeoff` log-linear (space log, time
  linear). CLI overridable.
- seaborn integration: `plt.style.use("thesis.mplstyle")` first, then
  seaborn calls without `sns.set_theme()` so seaborn doesn't override
  thesis style.

### Output format

PDF default (vector, LaTeX-compatible via `\includegraphics`, no LaTeX runtime
needed). `--format {pdf,png,svg,pgf}` overrides per invocation. PGF works but
is never tested in CI/fixtures (would require LaTeX on PATH); README documents
that PGF requires `xelatex` or `lualatex`.

### Dependencies

Runtime: `matplotlib>=3.8`, `seaborn>=0.13`, `numpy>=1.26`, `pyyaml>=6.0`.
Dev: `pytest>=8.0`. **No pandas** — the data shapes are small (a few hundred
points, tops), and seaborn accepts numpy arrays + dict-of-list inputs
directly. Re-evaluate only if shapes grow.

## Sequencing

1. Cargo.toml + main.rs cleanup (`.without_plots()` removal). Validate
   `target/criterion/` layout post-feature-removal **before** any Python
   work — load-bearing experiment.
2. `bench_report.rs` schema v2; thread `axes` through three call sites
   (`BenchSubcommand::Join`, `run_ds_bench`, `run_benchmark`); update tests.
3. CLAUDE.md + `docs/specs/` (this file + `bench-report-schema.md`).
4. `scripts/kermit-plot/` scaffold (pyproject, README, package skeleton,
   loader/criterion/axis_mapping/style, fixtures).
5. All six plot subcommands + `render-all` (one commit, optionally split if
   it grows).
6. End-to-end validation against real benchmark output.

Steps 1–3 are reversible; step 1 is the load-bearing tracer bullet. Halt and
reassess if `target/criterion/` layout differs from expectations.

## Out of scope (deliberate YAGNI)

- pandas / DataFrame pipelines. Re-evaluate if data shapes grow.
- Adding `kermit-plot` to CI. `watdiv-preprocess` isn't gated; this isn't
  either, yet.
- Nix flake integration for Python. Match `watdiv-preprocess`.
- Run-aggregation registry / database. `bench-runs/*.json` directory globbing
  is sufficient.
- Pre-rendering thesis figures via Justfile / Makefile. Add when the figure
  set stabilises.
- Config-file driven plot definitions (`plots.yml`). Defer until the CLI
  argument count per `render-all` becomes painful.

## Verification

1. `cargo build -p kermit` succeeds; `cargo tree -p kermit | grep plotters`
   is empty.
2. `cargo test --workspace` passes (existing tests updated for v2; new
   `axes_preserve_value_types_and_order` round-trip).
3. `cargo clippy --all-targets -- -Dwarnings` clean.
4. `cargo run -- bench ds -r kermit/tests/fixtures/edge.csv -i tree-trie
   --report-json /tmp/r.json -m space` writes `/tmp/r.json` with
   `schema_version: 2` and an `axes` map. `find target/criterion -type f`
   shows `benchmark.json`, `estimates.json`, `sample.json`, `tukey.json`
   under `target/criterion/{group}/{dir}/{base,new}/` — and **no** SVG/HTML
   anywhere.
5. `pip install -e scripts/kermit-plot && pytest scripts/kermit-plot` passes
   without running `cargo bench` (committed fixtures).
6. `kermit-plot scaling bench-runs/*.json --out /tmp/scaling.pdf` produces a
   non-empty PDF with one line per (DS, algorithm) and a log-log axis.
7. `kermit-plot render-all bench-runs/*.json --out-dir /tmp/plots` produces
   every applicable shape's PDF; shapes lacking sufficient axes are skipped
   with an info-level log.
8. Visual eyeball pass on each PDF: legend correctness, error-bar presence,
   axis scales, font consistency.
