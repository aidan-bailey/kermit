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
cargo bench --package kermit-ds # Run Criterion benchmarks
cargo miri setup && cargo miri test   # Check for undefined behavior
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
kermit-bench    → Benchmark infrastructure: dataset download, task/subtask definitions.
kermit          → CLI binary (clap). Subcommands: join, benchmark.
```

**Dependency flow:** `kermit-iters` → `kermit-derive`, `kermit-parser` → `kermit-ds`, `kermit-algos` → `kermit-bench` → `kermit`

## Key Trait Hierarchy

- **JoinIterable** (marker) → **LinearIterable** → **LinearIterator** (`key`, `next`, `seek`, `at_end`)
- **JoinIterable** (marker) → **TrieIterable** → **TrieIterator** : LinearIterator + `open`, `up`
- **Relation**: JoinIterable + Projectable — core data abstraction (`from_tuples`, `insert`, `header`)
- **JoinAlgo\<DS\>**: algorithm trait decoupled from data structures

## Testing Patterns

Tests use macro-generated suites that combinatorially test all data structures against all algorithms:
- `define_multiway_join_test!()` — individual parametrized test
- `define_multiway_join_test_suite!()` — generates 6 standard join patterns (unary, triangle, chain, star, self-join, existential)
- Uses `paste!` crate for macro hygiene

Unit tests live inline in `#[cfg(test)]` blocks. Integration tests in `tests/` directories.

## Extending the System

- **New data structure**: implement `Relation` + `TrieIterable` in `kermit-ds`, create a `TrieIterator`, add to `IndexStructure` CLI enum.
- **New join algorithm**: implement `JoinAlgo<DS>` in `kermit-algos`, add to `JoinAlgorithm` CLI enum.
- **New benchmark**: add module in `kermit-bench/src/benchmarks/`, implement `BenchmarkConfig`, add to `Benchmark` enum.

## Code Style

- `rustfmt.toml` is extensively configured: `max_width=100`, `trailing_comma="Vertical"`, `imports_granularity="One"`, `group_imports="StdExternalCrate"`, `match_arm_leading_pipes="Always"`.
- Always run `cargo fmt --all` before committing — nightly rustfmt required due to unstable features.
