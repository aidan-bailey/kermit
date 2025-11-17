# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.3] - 2025-11-17

### Changed

- `IntoTrieIter` derive macro now uses fixed `usize` key type instead of generic `KeyType`
- Removed generic type parameter from generated `IntoIterator` implementation

## [0.0.2] - 2025-06-25

### Changed

- To use new Kermit iterators

## [0.0.1] - 2025-06-23

### Added

- `IntoTrieIter` auto-derive
- CHANGELOG.md