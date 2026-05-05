# kermit-lab

Notebook-first analysis of `kermit bench --report-json` output and the
Criterion JSON artefacts it points at. Pandas DataFrames are the primary
surface; six plot shapes return `matplotlib.figure.Figure` for inline
display; pivot/comparison/stats helpers are shipped. The CLI is preserved
as a thin wrapper for headless thesis builds.

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

## Notebook usage

```python
import kermit_lab as kl

kl.apply_style()  # once per notebook session
df = kl.load("bench-runs/*.json", criterion_root="target/criterion")

# Plot inline — every plot returns a matplotlib.figure.Figure
fig = kl.scaling(df)
fig = kl.bar_queries(df, ds="TreeTrie", algo="LeapfrogTriejoin")

# Slice + pivot
kl.summary(df[df.phase == "iteration"], rows="data_structure", cols="tuples")

# Pairwise speedup with deterministic envelope
kl.compare(df, baseline="TreeTrie", target="ColumnTrie")

# Bootstrap CI on raw samples
samples = kl.load_samples("bench-runs/*.json", "target/criterion")
lo, hi = kl.bootstrap_ratio_ci(a, b, rng=42)
```

Five worked examples ship in [`notebooks/`](notebooks/):

- `01_quick_start.ipynb` — load a DataFrame and plot inline.
- `02_scaling.ipynb` — scaling plot + pivot of mean times by DS × scale.
- `03_compare_ds.ipynb` — `compare()` envelope + `bootstrap_ratio_ci()`.
- `04_watdiv_queries.ipynb` — multi-query bars across WatDiv stress runs.
- `05_distributions.ipynb` — violin plot from samples + Mann-Whitney U test.

## CLI workflow

1. Run `kermit bench …` one or more times, varying `(data_structure,
   algorithm, dataset)`. Pass `--report-json bench-runs/<name>.json` to each
   invocation. `bench-runs/` is gitignored at the workspace root.
2. Run `uv run kermit-lab <subcommand> bench-runs/*.json --out <path>`.

The reports point at `target/criterion/<group>/<dir>/{base,new}/...` artefacts
written by the same invocation. Don't `cargo clean` between bench runs and
plot generation.

```
kermit-lab scaling     <report.json>... --out PATH [--format {pdf,png,svg,pgf}]
kermit-lab bar-time    <report.json>... --query QUERY --out PATH ...
kermit-lab bar-space   <report.json>... --out PATH ...
kermit-lab tradeoff    <report.json>... --out PATH ...
kermit-lab dist        <report.json>... --out PATH ...
kermit-lab bar-queries <report.json>... --ds DS --algo ALGO --out PATH ...
kermit-lab render-all  <report.json>... --out-dir DIR [--format ...]
```

| Subcommand    | Shape                                          | Required axes in input set |
|---------------|------------------------------------------------|----------------------------|
| `scaling`     | log-log line plot of time vs `tuples`          | ≥2 distinct `tuples` values |
| `bar-time`    | bar+CI for time across `(DS, algo)` for one query | `query` matches `--query` |
| `bar-space`   | bar of `heap_size_bytes()` across DS           | space metric measurements |
| `tradeoff`    | space-vs-time scatter, log-x linear-y          | both time and space metrics |
| `dist`        | violin / box of per-iter samples               | `sample.json` per group |
| `bar-queries` | bar across `query` for one `(DS, algo)`        | `--ds`, `--algo` matches |

`render-all` emits every shape for which the input set has sufficient axes.
Shapes that lack the necessary axes are skipped with an info-level log
message rather than erroring.

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
major versions. The DataFrame column include-list lives in
`kermit_lab/frame.py` — extend it when the Rust schema adds an axis key.
