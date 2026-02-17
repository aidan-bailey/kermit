# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build              # Build all workspace crates
cargo test               # Run all tests
cargo test -p kermit-ds  # Run tests for a specific crate
cargo clippy --all-targets  # Lint (CI uses RUSTFLAGS=-Dwarnings)
cargo fmt --all -- --check  # Check formatting
cargo doc --workspace    # Build documentation
cargo miri test          # Check for undefined behavior (requires nightly)
```

## Running Benchmarks

```bash
cargo bench -p kermit-ds  # Run kermit-ds benchmarks (criterion)
```

## Architecture

Kermit is a Rust library for relational algebra research and benchmarking, focused on the Leapfrog Triejoin algorithm. The workspace is organized into layered crates with clear dependencies:

### Crate Hierarchy (bottom to top)

- **kermit-iters**: Core iterator traits (`LinearIterator`, `TrieIterator`) that define the interface for traversing relational data structures. These traits enable the leapfrog join algorithm to work with different underlying data structures.

- **kermit-derive**: Proc-macro crate providing `#[derive(IntoTrieIter)]` for automatic `IntoIterator` implementation on trie iterators.

- **kermit-parser**: Datalog-style query parser using winnow. Parses queries like `P(A, C) :- Q(A, B), R(B, C).` into `JoinQuery` AST with predicates, variables, atoms, and placeholders.

- **kermit-ds**: Data structures implementing the iterator traits. Contains:
  - `TreeTrie`: Tree-based trie with nested `TrieNode` children
  - `ColumnTrie`: Column-oriented trie representation
  - `Relation` trait: Interface for relational data with file I/O (CSV, Parquet)

- **kermit-algos**: Join algorithms, primarily `LeapfrogTriejoin` implementing the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481). Uses `JoinAlgo` trait to combine parsed queries with data structures.

- **kermit-bench**: Benchmark infrastructure and datasets (e.g., Oxford benchmark). Handles dataset downloading and benchmark management.

- **kermit**: Main crate and CLI binary. Provides `join` and `benchmark` subcommands.

### Key Design Patterns

The iterator trait hierarchy enables algorithm-data structure decoupling:
- `LinearIterator`: seek-based iteration with `key()`, `next()`, `seek()`, `at_end()`
- `TrieIterator: LinearIterator`: adds `open()` (descend to child) and `up()` (ascend to parent)
- `TrieIterable`: types that can produce a `TrieIterator`

`LeapfrogTriejoinIter` composes multiple `TrieIterator`s to perform multi-way joins by coordinating seeks across iterators at each trie level.

### Query Syntax

Join queries follow Datalog syntax:
- Variables: uppercase (e.g., `X`, `Y`)
- Atoms: lowercase (e.g., `alice`)
- Placeholders: `_`
- Example: `ancestor(X, Z) :- parent(X, Y), parent(Y, Z).`

## Code Style

- Uses nightly Rust features in CI (clippy, miri)
- Match arms use `| pattern =>` style (leading pipe)
- Imports grouped with `use { ... };` blocks
