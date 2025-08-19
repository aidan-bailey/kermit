# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.12] - 2025-08-19

### Added

- `RelationHeader` 

## [0.0.11] - 2025-07-30

### Changed

- `ColumnTrie` recursive insertion into loop

## [0.0.10] - 2025-07-28

### Added

- `ColumnTrie` to benchmarks

## [0.0.9] - 2025-07-15

### Added

- Nemo `ColumnTrie` data structure
- Hard test case

### Changed

- `kermit-iters` version to 0.0.4

## [0.0.8] - 2025-07-07

### Added

- Trie iteration tests

## [0.0.7] - 2025-06-26

### Removed

- `i8` benchmark

## [0.0.6] - 2025-06-26

### Added

- `num-traits` dev-dependency 
- Relation benchmarks

### Changed

- `criterion` dependency to version 0.6.0
- `rand` dependency to version 0.9.1

## [0.0.5] - 2025-06-25

### Changed

- To use new Kermit iterators
- `kermit-iters` dependency to version 0.0.3
- `kermit-derive` dependency to version 0.0.2

## [0.0.4] - 2025-06-24

### Changed

- Intregration tests location

## [0.0.3] - 2025-06-23

### Added

- Implementation of `IntoIterator` for `RelationTrie` using the `TrieIteratorWrapper`
- Dependency on `kermit-derive`

### Changed

- `RelationTrie` not allowing an arity of 0
- `RelationTrie` open implementation not initialising to the first element of the child iterator
- `kermit-iters` version to 0.0.2

### Fixed

- Using the term 'cardinality' instead of 'arity'

## [0.0.2] - 2025-06-18

### Fixed

- `RelationalTrie`'s `LinearIterator` implementation
- `Relation` trait requiring cardinality be specified when creating from tuples

## [0.0.1] - 2025-06-09

### Added

- CHANGELOG.md