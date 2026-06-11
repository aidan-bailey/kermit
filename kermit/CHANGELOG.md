# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2] - 2026-06-11

### Added

- `HashTrie` and `HashTriejoin` wired into the CLI selectors and bench dispatch (`-i hash-trie`, `-a hash-triejoin`)
- `HasherChoice` / `LayoutChoices` CLI scaffolding; bench-report `axes` now include `ds_layout_hasher`
- `watdiv-basic` generator dispatch to `run_basic_pipeline`
- `IndexStructureSelector::supports_algorithm` to gate incompatible structure/algorithm pairs

### Changed

- Make `hash_join` generic over `HashStrategy`
- Load each parquet relation once in `run_benchmark`, avoiding redundant re-parsing
- `kermit-ds` dependency to 0.2.0
- `kermit-iters` dependency to 0.0.10
- `kermit-algos` dependency to 0.0.12
- `kermit-bench` dependency to 0.1.2
- `kermit-rdf` dependency to 0.1.1

### Fixed

- Propagate cache-dir errors and dedupe cache lookups
- Report the missing relation name in DB diagnostics

## [0.1.1] - 2026-05-05

### Added

- Sweep `--indexstructure all` and `--algorithm all` selectors for full Cartesian benchmark sweeps via `bench run`
- `bench run --all` flag to sweep across every discovered benchmark
- Declarative-generator runner: `bench run <name>` materialises data on demand from YAMLs declaring a `generator: { kind, scale, ... }` block
- `bench gen watdiv` and `bench gen lubm` subcommands for imperative on-the-fly generation
- `bench fetch` and `bench clean` subcommands for managing the platform cache (`~/.cache/kermit/benchmarks/`)
- `--report-json <PATH>` flag emitting versioned `BenchReport` arrays per invocation; defaults to `bench-runs/{kind}-{unix-millis}.json`
- `SpaceMeasurement` Criterion `Measurement` and `BytesFormatter` for memory benchmarks; `bench ds --metrics space` and `bench run --metrics space` route through `Criterion<SpaceMeasurement>` via `iter_custom`
- Const-view atom rewrite: `DatabaseEngine::join` now invokes `kermit_algos::rewrite_atoms` so LFTJ never sees `Term::Atom("c<id>")`
- Integration test for WatDiv mini fixture (`kermit/tests/watdiv_correctness.rs`) and CLI smoke for `bench gen watdiv` (`kermit/tests/cli_watdiv_gen.rs`)
- LUBM end-to-end test coverage and miri-gating on the kermit-rdf driver fs tests

### Fixed

- Cache spec drift detection requires explicit `--force` to opt into rebuild; legacy `meta.json` files without `spec_hash` are treated as drift
- CLI join CSV header derived from head variable names (via `head_column_names`); test fixtures must skip the first non-empty line when parsing as integer tuples
- `bench run` benchmark ID semantics: `--name` is now a *prefix* on the auto-generated `{benchmark}/{query}/{ds}/{algo}` identity, defaulting to `run`
- WatDiv driver alignment with vendored binary CLI (`-d`, `-s`, `-q` modes write only to stdout; no per-template files emitted)

### Changed

- Restructure `bench gen` into nested subcommands (`watdiv`, `lubm`)
- Opt out of Criterion's default features (no `plotters`, no SVG/HTML rendering); analysis lives in `python/kermit-lab/`
- `kermit-ds` dependency to 0.1.1
- `kermit-iters` dependency to 0.0.9
- `kermit-algos` dependency to 0.0.11
- `kermit-bench` dependency to 0.1.1
- `kermit-parser` dependency to 0.0.3
- `kermit-rdf` dependency added at 0.1.0

## [0.1.0] - 2026-03-12

### Added

- `bench ds` subcommand for benchmarking index structures (insertion, iteration, space)
- `bench join` subcommand (restructured from previous `bench` command)
- `Metric` enum for selectable benchmark metrics (insertion, iteration, space)
- Criterion-based benchmarking with configurable sample size, measurement time, and warm-up time
- Integration tests for `bench ds` subcommand

### Fixed

- Remove redundant "join" nesting in bench benchmark ID

### Changed

- Restructure `Bench` command into sub-subcommands (`join`, `ds`)
- `kermit-ds` dependency to 0.1.0
- `kermit-iters` dependency to 0.0.8
- `kermit-algos` dependency to 0.0.10
- `kermit-bench` dependency to 0.1.0
- `kermit-parser` dependency to 0.0.2

## [0.0.16] - 2026-03-03

### Added

- `join` subcommand execution for running joins from the CLI
- `--bench` (`-b`) flag on `join` subcommand for timing statistics

### Changed

- `DB::join()` now returns result tuples directly
- Rename `Database` to `DatabaseEngine`
- Introduce `RelationError` for file parsing operations
- Expand multiway join test suite with five new patterns
- Add inline documentation across CLI crate
- `kermit-ds` dependency to 0.0.15
- `kermit-iters` dependency to 0.0.7
- `kermit-algos` dependency to 0.0.9
- `kermit-bench` dependency to 0.0.5

## [0.0.15] - 2025-11-17

### Added

- CLI interface with `Join` and `Benchmark` commands
- `benchmarker` module for benchmark execution
- `add_file` method to `DB` trait for loading relations from CSV and Parquet files
- Generic instantiation method for databases
- Support for `IndexStructure` and `JoinAlgorithm` selection

### Changed

- `Database` struct now takes join algorithm as a generic parameter
- Refactored DB methods to be entirely generic
- Removed dependency on `kermit-kvs` module
- `kermit-ds` dependency to 0.0.14
- `kermit-algos` dependency to 0.0.8
- `kermit-bench` dependency to 0.0.4

### Removed

- Builder pattern for database construction

## [0.0.14] - 2025-09-30

### Changed

- `kermit-ds` dependency to 0.0.13
- `kermit-iters` dependency to 0.0.5
- `kermit-algos` dependency to 0.0.7

## [0.0.13] - 2025-08-19

### Changed

- `kermit-ds` dependency to 0.0.12
- `kermit-algos` dependency to 0.0.6

## [0.0.12] - 2025-07-30

### Changed

- `kermit-ds` dependency to 0.0.11

## [0.0.11] - 2025-07-28

### Changed

- `kermit-ds` dependency to 0.0.10

## [0.0.10] - 2025-07-15

### Added

- `ColumnTrie` lftj tests

### Changed

- `kermit-ds` dependency to 0.0.9
- `kermit-iters` dependency to 0.0.4
- `kermit-algos` dependency to 0.0.5

## [0.0.9] - 2025-07-07

### Changed

- `kermit-ds` dependency to 0.0.8

## [0.0.8] - 2025-06-26

### Changed

- `kermit-ds` dependency to 0.0.7

## [0.0.7] - 2025-06-26

### Changed

- `kermit-ds` dependency to 0.0.6

## [0.0.6] - 2025-06-25

### Changed

-  `kermit-ds` dependency to 0.0.5
-  `kermit-iters` dependency to 0.0.3
-  `kermit-algos` dependency to 0.0.4
-  `kermit-kvs` dependency to 0.0.2

## [0.0.5] - 2025-06-24

### Changed

-  `paste` to be a dev-dependency

## [0.0.4] - 2025-06-24

### Changed

- Intregration tests location
- `kermit-ds` dependency to version 0.0.4

## [0.0.3] - 2025-06-23

### Changed

- `kermit-ds` dependency to version 0.0.3
- `kermit-iters` dependency to version 0.0.2
- `kermit-algos` dependency to version 0.0.3

### Fixed

- Header syntax for `define_multiway_join_test_suite` macro

## [0.0.2] - 2025-06-09

### Changed

- `kermit-algos` version to `0.0.2`  
- `kermit-ds` version to `0.0.2`

### Added

- Multiway join test macros
- Unit testing for `LeapfrogTriejoin` + `RelationalTrie`

## [0.0.1] - 2025-06-09

### Added

- CHANGELOG.md