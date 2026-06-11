# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-06-11

### Added

- General `plot` subcommand backed by a unified plot engine, with preset routing in the CLI
- Geoms: bar, line, scatter, and violin drawers; facet grid with a shared-legend helper
- Aesthetic-map encoding (column-aware colour / marker / linestyle) with deterministic, stable colour fallback and a dedicated HashTrie colour
- Optimization-axis defaults registry and wildcard-wide optimization-axis ingest in the loader
- `00_full_timeline.ipynb` end-to-end notebook covering `bench gen lubm`, `bench run` time/space, report loading, and plots

### Changed

- **Breaking:** public API rewired to the engine + presets model; preset compatibility shims removed and the obsolete top-level-API surface dropped

### Removed

- Legacy `plots/` package and its tests

## [0.1.0] - 2026-05-04

### Added

- Initial release: notebook-first analysis of kermit Criterion benchmark output
- `kl.load()` returning a pandas DataFrame from `target/criterion/` and `bench-runs/` reports
- Plotting helpers (`kl.scaling()`, `kl.bar_time()`, and friends) returning `matplotlib.figure.Figure`
- Statistics helpers: `kl.summary`, `compare`, `bootstrap_ratio_ci`, `mannwhitney_u`
- Thin CLI wrapper around the library
