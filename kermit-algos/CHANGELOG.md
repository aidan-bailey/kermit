# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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