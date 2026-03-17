# Space Benchmarks via Custom Criterion Measurement

**Date:** 2026-03-16
**Status:** Accepted

## Context

The Criterion micro-benchmarks in `kermit-ds` currently measure insertion and iteration
time for TreeTrie and ColumnTrie. Space measurement (`HeapSize`) exists only in the CLI
`bench ds` subcommand, where it prints `heap_size_bytes()` to stderr without Criterion
integration. We want space complexity benchmarking in the Criterion suite to get:

1. Side-by-side reporting (space results alongside time in Criterion HTML output)
2. Scaling analysis (heap size growth across input sizes)

## Decision

Add a new Criterion benchmark binary with a custom `Measurement` implementation that
measures bytes instead of nanoseconds, using the existing `HeapSize` trait.

## Architecture

### Custom types

**`BytesFormatter`** implements `ValueFormatter`. Scales values to appropriate byte units:
- `< 1 KiB` -> `B`
- `< 1 MiB` -> `KiB`
- `< 1 GiB` -> `MiB`
- `>= 1 GiB` -> `GiB`

**`SpaceMeasurement`** implements `Measurement`:
- `type Intermediate = ()` (unused — `iter_custom` bypasses start/end)
- `type Value = usize` (bytes)
- `to_f64` converts bytes to f64 for Criterion's statistical analysis
- `formatter()` returns `&BytesFormatter`

### Benchmark structure

Uses `iter_custom`: the closure receives `iters`, constructs the relation `iters` times,
sums `heap_size_bytes()`, and returns the total. Criterion divides by iters to get
per-iteration value. Per-iteration value is deterministic (~zero variance), but the
scaling curves across input sizes are the valuable output.

Same input generators as existing benchmarks:
- Exponential: k in {1, 2, 3, 4, 5} -> k^k tuples
- Factorial: h in {1, 2, 3, 4, 5, 6, 7, 8, 9} -> h! tuples

Benchmark names: `{DataStructure}/Space/Exponential/{k}/{n}` and
`{DataStructure}/Space/Factorial/{h}/{n}`.

Same macro pattern (`define_trie_relation_benchmarks!` style) so new data structures
automatically get space benchmarks.

### Files changed

1. `kermit-ds/benches/space_benchmarks.rs` (new) — BytesFormatter, SpaceMeasurement,
   space benchmark functions, macro, criterion_group/main
2. `kermit-ds/Cargo.toml` — add `[[bench]] name = "space_benchmarks" harness = false`

### What this enables

- `cargo bench --package kermit-ds --bench space_benchmarks` runs space benchmarks
- `cargo bench --package kermit-ds` runs both time and space benchmarks
- Criterion HTML reports under `target/criterion/` with scaling curves
- Regression detection on space if a code change alters heap layout
