# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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