# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.2] - 2025-06-23

### Added

- `Sized` constraint to `KeyType`
- `IntoIterator` constraint to `TrieIterable` return
- `Iterator` implementation for `TrieIteratorWrapper`, allowing structs that implement the `TrieIterator` trait to easily be iterated

### Fixed

- `LinearIterator` implementation for `VecLinearIter` not incrementing before checking `at_end`

## [0.0.1] - 2025-06-09

### Added

- CHANGELOG.md