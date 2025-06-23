# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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