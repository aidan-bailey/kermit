# Benchmarking Architecture

**Date:** 2026-03-17
**Updated:** 2026-03-17 (post-consolidation)

## Overview

All benchmarking in Kermit is driven through the CLI binary. There is a single
entry point (`kermit bench`) with three subcommands for different use cases:

- **`bench join`** — Criterion benchmarks on user-supplied data files.
- **`bench ds`** — Criterion benchmarks on a single data structure from a file.
- **`bench suite`** — Criterion benchmarks on named synthetic workloads.

```
kermit-bench (generation + workload configs)
    │ provides synthetic data + metadata
    ▼
kermit bench suite          Criterion on synthetic data (CLI)
kermit bench join           Criterion on user-supplied data (CLI)
kermit bench ds             Criterion on single DS from file (CLI)
```

## Common arguments (`BenchArgs`)

All `bench` subcommands accept:

| Flag | Default | Description |
|------|---------|-------------|
| `--name` | varies | Criterion benchmark group name |
| `--sample-size` | 100 | Criterion sample count (min 10) |
| `--measurement-time` | 5s | Measurement time per sample |
| `--warm-up-time` | 3s | Warm-up before sampling |

## `kermit bench join`

Benchmarks end-to-end join execution time on real data.

**Arguments:** `--relations` (file paths), `--query` (.dl file),
`--algorithm`, `--indexstructure`, optional `--output` (dumps one run's results).

**Flow:**
1. Load relation files (CSV or Parquet) into a `DatabaseEngine` via
   `instantiate_database`.
2. Parse the `.dl` query file via `kermit-parser`.
3. If `--output` is set, run the join once and write results.
4. Wrap `db.join(query)` in Criterion's `iter_batched` (cloning the
   `JoinQuery` per sample).

**Criterion output** goes to `target/criterion/` as usual. Metadata summary
(data structure, algorithm, relation count) goes to stderr.

## `kermit bench ds`

Benchmarks a single data structure on a single relation file.

**Arguments:** `--relation` (single file), `--indexstructure`,
`--metrics` (defaults to all three).

**Metrics:**

| Metric | How measured | Output |
|--------|-------------|--------|
| `Insertion` | `R::from_tuples(header, tuples)` via Criterion `iter_batched` | Criterion stats |
| `Iteration` | `relation.trie_iter().into_iter().collect()` via Criterion `iter` | Criterion stats |
| `Space` | `relation.heap_size_bytes()` | Printed to stderr only |

## `kermit bench suite`

Benchmarks a named workload from `kermit-bench` on synthetic data.

**Arguments:** `--benchmark` (workload name), `--indexstructure`,
`--metrics` (defaults to all three).

**Flow:**
1. Look up `BenchmarkConfig` by name via `Benchmark::config()`.
2. For each task/subtask, call `config.generate(subtask)` to get relations
   as `Vec<(usize, Vec<Vec<usize>>)>` (arity + tuples).
3. Build data structures via `R::from_tuples`.
4. Run requested metrics through Criterion (insertion, iteration) or
   print to stderr (space).

**Available workloads:**

| Workload | Generator | Scales | Description |
|----------|-----------|--------|-------------|
| `exponential` | `generate_exponential_tuples(k)` | k=1..5 | k^k tuples of arity k over domain 0..k |
| `factorial` | `generate_factorial_tuples(k)` | k=1..9 | k! tuples of arity k, position d has domain 0..=d |

**Equivalence:** `kermit bench suite --benchmark exponential --indexstructure tree-trie`
reproduces what the former `cargo bench --package kermit-ds` did for TreeTrie
exponential workloads.

## `kermit-bench` crate

Provides synthetic data generation and benchmark workload configuration with no
internal kermit dependencies.

### Module structure

```
kermit-bench/src/
├── lib.rs
├── benchmark.rs          BenchmarkConfig trait, GenerationParams, Task/SubTask
├── generation/
│   ├── mod.rs
│   ├── tuples.rs         generate_exponential_tuples, generate_factorial_tuples, generate_distinct_tuples
│   └── graphs.rs         GraphModel enum (stub — ErdosRenyi)
└── benchmarks/
    ├── mod.rs             Benchmark enum registry
    ├── exponential.rs     ExponentialBenchmark config
    └── factorial.rs       FactorialBenchmark config
```

### Key types

```rust
pub enum GenerationParams {
    Exponential { k: usize },
    Factorial { k: usize },
    Graph(GraphModel),
    Custom,
}

pub trait BenchmarkConfig {
    fn metadata(&self) -> &BenchmarkMetadata;
    fn generate(&self, subtask: &SubTask) -> Vec<(usize, Vec<Vec<usize>>)>;
}
```

`BenchmarkMetadata` carries a task/subtask hierarchy. Each `SubTask` holds
`GenerationParams`. Standard workloads delegate `generate()` to the declarative
params. Complex workloads can use `Custom` and override `generate()` directly.

## Space measurement

`kermit/src/measurement.rs` contains `SpaceMeasurement` (implements
`criterion::measurement::Measurement` with `type Value = usize`) and
`BytesFormatter` (scales to B/KiB/MiB/GiB). These were moved from the former
`kermit-ds/benches/space_benchmarks.rs`.

Currently `bench suite --metrics space` prints heap bytes to stderr as a scalar.
The full Criterion-based `SpaceMeasurement` path is available in the codebase
but not yet wired into the CLI subcommands.

Plots must be disabled (`.without_plots()`) when using `SpaceMeasurement`
because Criterion's plotters backend panics on zero-variance deterministic data.
