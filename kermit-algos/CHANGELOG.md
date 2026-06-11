# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.12] - 2026-06-11

### Added

- `HashTriejoin` multi-way join (paper Algorithm 3), with `SingletonHashTrieIter`, leaf-level emission (`enumerate` / `emit_leaf` / `verify_and_construct`), and a `HashTrieIterKind` dispatch enum

### Changed

- Rename LFTJ `iterator_pool` to `idle_iterators`; clarify LFTJ naming and state machine, and document its invariants
- Remove the dead `Node` trait and a stale commented-out test block
- `kermit-iters` dependency to 0.0.10
- `kermit-derive` dependency to 0.0.7

### Fixed

- Enforce a valid global attribute order in triejoins

## [0.0.11] - 2026-05-05

### Added

- `const_rewrite` for paper-canonical atom filtering: rewrites `Term::Atom("c<id>")` into a fresh variable plus a synthetic unary `Const_c<id>` predicate
- `TrieIterKind` dispatch enum to unify static and rewritten iterator sources
- `SingletonTrieIter` to back the synthetic unary predicates produced by the const-view rewrite

### Fixed

- Predicate aliasing in `const_rewrite` when the same atom appears in multiple body positions

### Changed

- Enforce `#![deny(missing_docs)]` across the crate
- Apply nightly rustfmt formatting pass
- `kermit-iters` dependency to 0.0.9
- `kermit-derive` dependency to 0.0.6
- `kermit-parser` dependency to 0.0.3

## [0.0.10] - 2026-03-12

### Changed

- `kermit-iters` dependency to 0.0.8
- `kermit-derive` dependency to 0.0.5
- `kermit-parser` dependency to 0.0.2

## [0.0.9] - 2026-03-03

### Changed

- Extract `build_variable_index` from `join_iter` for clarity
- Rename `LeapfrogTriejoinIter` fields for improved readability
- Add inline documentation across algorithm implementations
- Add end-to-end `LeapfrogTriejoinIter` test coverage
- `kermit-iters` dependency to 0.0.7
- `kermit-derive` dependency to 0.0.4

## [0.0.8] - 2025-11-17

### Added

- Datalog query parser using `winnow`
- `JoinQuery` struct moved from kermit to kermit-algos
- `JoinAlgorithm` enum for algorithm selection
- `queries` module with parser and join query types

### Changed

- Refactored to use `JoinQuery` everywhere
- Restructured queries module
- Removed dependency on `kermit-kvs` module
- `kermit-iters` dependency to 0.0.6

### Removed

- Old source files and redundant code

## [0.0.7] - 2025-09-30

### Changed

- `JoinIterable` to `Joinable`
- adapt to `TreeTrie` rename

## [0.0.6] - 2025-08-19

### Changed

- to use `RelationHeader`

## [0.0.5] - 2025-07-15

### Changed

- `kermit-iters` dependency to version 0.0.4

## [0.0.4] - 2025-06-25

### Changed

- `LeapfrogJoin` and `LeapfrogTriejoin` to use new non-reference iterators
- `kermit-iters` dependency to version 0.0.3
- `kermit-derive` dependency to version 0.0.2

### Removed

- Redundant `JoinAlgo` function

## [0.0.3] - 2025-06-23

### Added

- `TrieIterator` implementation for `LeapfrogTriejoinIter` 
- `IntoIterator` implementation for `LeapfrogTriejoinIter` pointing to `TrieIteratorWrapper`
- `kermit-derive` dependency

### Changed

- `kermit-iters` version to 0.0.2

### Removed

- `LeapfrogTriejoinIterWrapper`

## [0.0.2] - 2025-06-18

### Fixed

- `LeapfrogTriejoin` not being aware of how `LeapfrogJoin` sorts its iterators
- `LeapfrogTrie`'s `Iterator` implementation

## [0.0.1] - 2025-06-09

### Added

- CHANGELOG.md