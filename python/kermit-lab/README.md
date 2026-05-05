# kermit-lab

Render thesis-quality plots from `kermit bench --report-json` outputs and the
Criterion JSON artefacts they reference. Six plot shapes are shipped, plus a
`render-all` meta-command.

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

## Workflow

1. Run `kermit bench …` one or more times, varying `(data_structure,
   algorithm, dataset)`. Pass `--report-json bench-runs/<name>.json` to each
   invocation. `bench-runs/` is gitignored at the workspace root.
2. Run `uv run kermit-lab <subcommand> bench-runs/*.json --out <path>`.

The reports point at `target/criterion/<group>/<dir>/{base,new}/...` artefacts
written by the same invocation. Don't `cargo clean` between bench runs and
plot generation.

## Subcommands

```
kermit-lab scaling     <report.json>... [--out PATH] [--format {pdf,png,svg,pgf}]
kermit-lab bar-time    <report.json>... --query QUERY [--out PATH] ...
kermit-lab bar-space   <report.json>... [--out PATH] ...
kermit-lab tradeoff    <report.json>... [--out PATH] ...
kermit-lab dist        <report.json>... [--out PATH] ...
kermit-lab bar-queries <report.json>... --ds DS --algo ALGO [--out PATH] ...
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
major versions.
