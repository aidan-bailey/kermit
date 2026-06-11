# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.10] - 2026-06-11

### Added

- `optimization` trait family (`LayoutOption` / `ConfigOption` / `BuildMode`) underpinning the optimization-standard axes
- `HashStrategy` trait with `SipHash` and `FxHash` strategies for hash-trie keying
- `HashTrieIterator` and `HashTrieIterable` traits, re-exported from the crate root, for the hash-trie iterator family

### Changed

- Add `rustc-hash` and `serde_json` dependencies

### Fixed

- `seek` now behaves correctly when called on an iterator before its first `next`

## [0.0.9] - 2026-05-05

### Changed

- Expand crate-level rustdoc with usage examples
- Clean up `JoinIterable` documentation
- Apply nightly rustfmt formatting pass

## [0.0.8] - 2026-03-12

### Changed

- Version bump for downstream compatibility

## [0.0.7] - 2026-03-03

### Fixed

- Filter partial tuples in `TrieIteratorWrapper` for correct triejoin results

### Changed

- Simplify `register_var` and add phase comments to `TrieIteratorWrapper`
- Add inline documentation across iterator traits and implementations
- Add test coverage for `TrieIteratorWrapper` and `VecLinearIter`

## [0.0.6] - 2025-11-17

### Changed

- Refactored to remove generic key types
- Key type is now fixed to `usize` across the library
- Removed dependency on `kermit-kvs` module
- Updated `Joinable` trait documentation

## [0.0.5] - 2025-09-30

### Changed

- `JoinIterable` to `Joinable`
- trait bounds and re-exports accordingly

## [0.0.4] - 2025-07-15

### Added

- `Display` constraint to `KeyType`

## [0.0.3] - 2025-06-25

### Changed

- `KeyType` constraints to `Debug`, `Ord` and `Copy`
- `LinearIterator` producing references
- `TrieIterator` producing references

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