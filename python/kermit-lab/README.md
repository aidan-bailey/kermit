# kermit-lab

Notebook-first analysis of `kermit bench --report-json` output and the
Criterion JSON artefacts it points at. Pandas DataFrames are the primary
surface; a general `kl.plot(...)` engine plus named presets return
`matplotlib.figure.Figure` for inline display; pivot/comparison/stats helpers
are shipped. The CLI is preserved as a thin wrapper for headless thesis builds.

## Install

This project is managed with [uv](https://docs.astral.sh/uv/). From this
directory:

```bash
uv sync --group test     # creates .venv, installs deps + test extras
uv run kermit-lab --help
```

`uv sync` is deterministic — `uv.lock` is committed, so every contributor
gets identical resolved versions. Activate the venv with
`source .venv/bin/activate` if you prefer a shell over `uv run`.

On NixOS, run install and subsequent `kermit-lab` invocations from inside
`nix develop` — the dev shell exports an `LD_LIBRARY_PATH` covering
`libstdc++.so.6` / `libz.so.1` so numpy's C extensions load. Outside the dev
shell, set `LD_LIBRARY_PATH=/run/current-system/sw/share/nix-ld/lib` manually.

## Public API

```python
import kermit_lab as kl

df = kl.load("bench-runs/*.json")          # tidy summary frame (incl. ds_*/algo_* axes)
samples = kl.load_samples("bench-runs/*.json")

# General engine — bind any axis to any channel:
kl.plot(df, kind="bar", x="ds_config_singleton_pruning",
        y="time", colour="data_structure", facet="query", out="ablation.pdf")

# Presets (configurations of kl.plot):
kl.scaling(df)            # line: time vs tuples, colour=DS, style=algo
kl.bar_time(df, query="triangle")
kl.bar_space(df)
kl.tradeoff(df)
kl.dist(df, samples=samples)
kl.bar_queries(df, ds=["TreeTrie"])
kl.ablation(df, axis="ds_layout_hasher")   # time vs an optimization axis
```

Optimization axes (`ds_layout_*`, `ds_config_*`, `ds_build_mode`) are loaded as
columns automatically; `kl.discover_opt_columns(df)` lists them.

## Notebook usage

```python
import kermit_lab as kl

kl.apply_style()  # once per notebook session
df = kl.load("bench-runs/*.json", criterion_root="target/criterion")

# General engine: bind any DataFrame column to any visual channel
fig = kl.plot(df, kind="line", x="tuples", y="time",
              colour="data_structure", style="algorithm", logx=True, logy=True)

# Presets are named configurations of kl.plot
fig = kl.scaling(df)
fig = kl.bar_queries(df, ds=["TreeTrie", "ColumnTrie"])

# Ablation: compare time across an optimization axis value
fig = kl.ablation(df, axis="ds_layout_hasher")

# Slice + pivot
kl.summary(df[df.phase == "iteration"], rows="data_structure", cols="tuples")

# Pairwise speedup with deterministic envelope
kl.compare(df, baseline="TreeTrie", target="ColumnTrie")

# Bootstrap CI on raw samples
samples = kl.load_samples("bench-runs/*.json", "target/criterion")
lo, hi = kl.bootstrap_ratio_ci(a, b, rng=42)
```

Eight worked examples ship in [`notebooks/`](notebooks/):

- `00_full_timeline.ipynb` — generate a LUBM-1 dataset, run the bench sweep, then load and plot. **Start here.**
- `01_quick_start.ipynb` — load a DataFrame and plot inline.
- `02_scaling.ipynb` — scaling plot + pivot of mean times by DS × scale.
- `03_compare_ds.ipynb` — `compare()` envelope + `bootstrap_ratio_ci()`.
- `04_watdiv_queries.ipynb` — multi-query bars across WatDiv stress runs.
- `05_distributions.ipynb` — violin plot from samples + Mann-Whitney U test.
- `06_ablation.ipynb` — plotting optimization axes (`ds_layout_*`/`ds_config_*`/`ds_build_mode`).
- `07_lubm_reference_comparison.ipynb` — reference end-to-end comparison of all three index structures on LUBM(1, 0).

## CLI workflow

1. Run `kermit bench …` one or more times, varying `(data_structure,
   algorithm, dataset)`. Pass `--report-json bench-runs/<name>.json` to each
   invocation. `bench-runs/` is gitignored at the workspace root.
2. Run `uv run kermit-lab <subcommand> bench-runs/*.json --out <path>`.

The reports point at `target/criterion/<group>/<dir>/{base,new}/...` artefacts
written by the same invocation. Don't `cargo clean` between bench runs and
plot generation.

```
kermit-lab plot        <report.json>... --kind {bar,line,scatter,violin} --out PATH ...
kermit-lab scaling     <report.json>... --out PATH
kermit-lab bar-time    <report.json>... --query QUERY --out PATH ...
kermit-lab bar-space   <report.json>... --out PATH ...
kermit-lab tradeoff    <report.json>... --out PATH ...
kermit-lab dist        <report.json>... --out PATH ...
kermit-lab bar-queries <report.json>... --ds DS --algo ALGO --out PATH ...
kermit-lab ablation    <report.json>... --axis COLUMN --out PATH ...
kermit-lab render-all  <report.json>... --out-dir DIR [--format ...]
```

| Subcommand    | Shape                                          | Required axes in input set |
|---------------|------------------------------------------------|----------------------------|
| `plot`        | general engine: any kind, any axis bound to any channel | depends on `--kind`, `--x`, `--colour` etc. |
| `scaling`     | log-log line plot of time vs `tuples`          | ≥2 distinct `tuples` values |
| `bar-time`    | bar+CI for time across `(DS, algo)` for one query | `query` matches `--query` |
| `bar-space`   | bar of `heap_size_bytes()` across DS           | space metric measurements |
| `tradeoff`    | space-vs-time scatter, log-x linear-y          | both time and space metrics |
| `dist`        | violin / box of per-iter samples               | `sample.json` per group |
| `bar-queries` | bar across `query` for one `(DS, algo)`        | `--ds`, `--algo` matches |
| `ablation`    | bar of time vs an optimization axis            | `--axis` column present in frame |

`render-all` emits every shape for which the input set has sufficient axes,
including one ablation figure per optimization axis (`ds_layout_*`,
`ds_config_*`, `ds_build_mode`) that carries ≥2 distinct values. Shapes that
lack the necessary axes are skipped with an info-level log message rather than
erroring.

## Style

- Wong / Okabe-Ito 8-colour palette (colour-blind safe). See
  `kermit_lab/axis_mapping.py`.
- `data_structure` → colour; `algorithm` → linestyle (`scaling`, `dist`) /
  marker shape (`bar-time`, `tradeoff`, `bar-queries`).
- Default output format is **PDF** (vector, LaTeX-compatible). Override per
  invocation with `--format {pdf,png,svg,pgf}`.
- PGF is supported but never tested in CI (would require LaTeX on PATH);
  rendering PGF requires `xelatex` or `lualatex`.

## Schema

`kermit-lab` parses `BenchReport` JSON v2 (see
`docs/specs/bench-report-schema.md`). The loader refuses to parse unknown
major versions. Fixed axis columns (`data_structure`, `algorithm`, `query`,
`tuples`, etc.) are listed in `kermit_lab/frame.py`; optimization axes
(`ds_layout_*`, `ds_config_*`, `ds_build_mode`) are discovered dynamically
from the reports and added as additional DataFrame columns. Use
`kl.discover_opt_columns(df)` to enumerate them.
