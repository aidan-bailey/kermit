# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Kermit is a Rust library for relational algebra research and benchmarking, built as a platform for a Masters thesis investigating the Leapfrog Triejoin algorithm across different data structures. It is a Cargo workspace with 7 crates. All keys are `usize` (dictionary-encoded). The codebase uses entirely safe Rust with no unsafe blocks.

## Build Commands

```bash
cargo build --verbose           # Build entire workspace
cargo test --verbose            # Run all tests
cargo test --package kermit-ds  # Run a single crate's tests
cargo test test_tree_trie       # Run a single test by name
cargo clippy --all-targets --verbose  # Lint (CI uses RUSTFLAGS=-Dwarnings)
cargo fmt --all                 # Format (CI checks with --check)
cargo doc --workspace           # Generate docs (CI uses RUSTDOCFLAGS=-Dwarnings)
cargo run -- bench run triangle -i tree-trie -a leapfrog-triejoin  # Run a named benchmark
MIRIFLAGS="-Zmiri-disable-isolation" cargo miri setup && cargo miri test  # Check for UB (flag matches CI)
```

## Toolchain

Rust **nightly** (pinned in `rust-toolchain.toml`). Required components: clippy, miri, rust-analyzer, rustfmt. The `rustfmt.toml` uses `unstable_features=true` so nightly rustfmt is required.

## CI Checks (PR gate)

All of these must pass: `cargo test`, `cargo clippy` (warnings are errors), `cargo fmt --check`, `cargo doc` (doc warnings are errors), `cargo miri test`.

## Workspace Architecture

```
kermit-iters    → Core iterator traits (LinearIterator, TrieIterator). Zero dependencies.
kermit-derive   → Proc macros (#[derive(IntoTrieIter)]) for iterator boilerplate.
kermit-parser   → Datalog query parser (winnow). Parses "Q(X,Z) :- R(X,Y), S(Y,Z)."
kermit-ds       → Data structures: TreeTrie (pointer-based), ColumnTrie (column-oriented).
                  Both implement Relation + TrieIterable traits.
kermit-algos    → Join algorithms: LeapfrogJoinIter (binary), LeapfrogTriejoinIter (multi-way).
                  Generic over any TrieIterable data structure via JoinAlgo<DS> trait.
kermit-bench    → Benchmark definitions, discovery, and caching. No internal deps.
                  YAML-based benchmark declarations, ZivaHub download, ~/.cache/kermit/.
kermit          → CLI binary (clap). Subcommands: join, bench (join|ds|run|fetch|clean).
                  Provides DB abstraction layer (db::DB trait, DatabaseEngine).
                  All Criterion execution lives here (including SpaceMeasurement).
```

**Dependency flow:** `kermit-iters` → `kermit-derive`, `kermit-parser` → `kermit-ds` → `kermit-algos` → `kermit` (binary). `kermit-bench` is isolated (no internal deps); `kermit` depends on it.

## Key Trait Hierarchy

- **JoinIterable** (marker) → **LinearIterable** → **LinearIterator** (`key`, `next`, `seek`, `at_end`)
- **JoinIterable** (marker) → **TrieIterable** → **TrieIterator** : LinearIterator + `open`, `up`
- **Relation**: JoinIterable + Projectable — core data abstraction (`from_tuples`, `insert`, `header`)
- **JoinAlgo\<DS\>**: algorithm trait decoupled from data structures
- **HeapSize**: heap-allocated byte count for space benchmarking (`heap_size_bytes()`)

## Testing Patterns

Tests use macro-generated suites that combinatorially test all data structures against all algorithms:
- `define_multiway_join_test!()` — individual parametrized test
- `define_multiway_join_test_suite!()` — generates 6 standard join patterns (unary, triangle, chain, star, self-join, existential)
- Uses `paste!` crate for macro hygiene

Unit tests live inline in `#[cfg(test)]` blocks. Integration tests in `tests/` directories.

## Extending the System

- **New data structure**: implement `Relation` + `TrieIterable` + `HeapSize` in `kermit-ds`, create a `TrieIterator`, add to `IndexStructure` CLI enum, and add match arms in `run_ds_bench`/`run_benchmark` in `kermit/src/main.rs`.
- **New join algorithm**: implement `JoinAlgo<DS>` in `kermit-algos`, add to `JoinAlgorithm` CLI enum.
- **New benchmark**: add a YAML file in `benchmarks/` with name, description, relation URLs, and Datalog query. See `benchmarks/triangle.yml` for the schema.

## Code Style

- `rustfmt.toml` is extensively configured: `max_width=100`, `trailing_comma="Vertical"`, `imports_granularity="One"`, `group_imports="StdExternalCrate"`, `match_arm_leading_pipes="Always"`.
- Always run `cargo fmt --all` before committing — nightly rustfmt required due to unstable features.

## Gotchas

- **Miri isolation**: CI runs miri with `MIRIFLAGS="-Zmiri-disable-isolation"`. Use the same flag locally or tests may fail differently.
- **git-cliff**: `cliff.toml` configures changelog generation via [git-cliff](https://git-cliff.org/). The release workflow auto-generates changelogs from conventional commits.
- **Space benchmarks**: `kermit/src/measurement.rs` contains `SpaceMeasurement` (custom Criterion `Measurement`) and `BytesFormatter`. Currently used only via `bench run --metrics space` which prints heap bytes to stderr. The full Criterion-based space measurement path is available but not yet wired into the CLI.
- **CI env vars**: All CI jobs set `RUST_BACKTRACE=1`. Release workflow requires `CARGO_REGISTRY_TOKEN` secret.
