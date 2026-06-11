# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2] - 2026-06-11

### Added

- `watdiv-basic` `GeneratorSpec` variant for the WatDiv Basic Testing workload
- Public `base_cache_dir` and `load_cached_benchmark` helpers

### Changed

- Extract a `try_load_cache_subdir` cache-loading helper

## [0.1.1] - 2026-05-05

### Added

- Declarative YAML generator schema with `generator:` block (kind/scale/seed) for benchmark specs
- `GeneratorSpec` and `spec_hash` for stable cache validation across runs
- LUBM benchmark integration via shared `meta.json` schema
- Tuple-stream generators for synthetic relations
- Cloud-based YAML loading: `discovery::load_all_benchmarks_with_cache` reads both workspace and cache YAMLs
- Schema versioning bump on `meta.json` to add the optional `spec_hash` field

### Fixed

- Cache discovery now requires both `benchmark.yml` and `meta.json` to mark a directory as a generator-produced bench
- Cache-side `benchmark.yml` no longer leaks `generator:` blocks (provenance lives in `meta.json`)
- Fix tuple-cardinality reporting in benchmark validation

### Changed

- `describe_benchmark_status` distinguishes static (`cached` / `not cached`) from generator (`not generated` / `cached` / `stale`) provenance
- Expanded benchmark configuration test coverage
- Refactor preprocessor logic into module-level helpers

## [0.1.0] - 2026-03-12

### Added

- Generator module for synthetic benchmark data
- `petgraph` dependency for graph-based generation

## [0.0.5] - 2026-03-03

### Changed

- Propagate errors with context in Oxford benchmark loader
- Add `petgraph` dependency for graph-based benchmark generation
- Add inline documentation across benchmark infrastructure

## [0.0.4] - 2025-11-17

### Added

- Generation module for benchmark data generation

## [0.0.3] - 2025-10-20

### Added

- benchmark validation
- tasks/sub-tasks

### Fixed

- oxford benchmark

## [0.0.2] - 2025-10-03

### Added

- oxford benchmark

## [0.0.1] - 2025-10-03

### Added

- initial release of kermit-bench module
