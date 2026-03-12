# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.2] - 2026-03-12

### Fixed

- Reject trailing input in `FromStr` implementation

### Changed

- Replace `unwrap` with `is_some_and` in `term()` parser
- Consolidate repetitive tests into table-driven cases
- Add edge case tests for parser rejection and stress

## [0.0.1] - 2026-03-03

### Added

- Datalog query parser extracted from `kermit-algos`
- `winnow`-based parser for join queries
- `FromStr` implementation for `JoinQuery`
