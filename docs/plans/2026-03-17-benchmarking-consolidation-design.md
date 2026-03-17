# Benchmarking Consolidation Design

**Date:** 2026-03-17

## Goal

Consolidate all benchmarking into the CLI. Remove downloaded dataset support.
Repurpose `kermit-bench` as a synthetic data generation + workload definition
crate. Delete all benchmarks from `kermit-ds`.

## Motivation

The current benchmarking architecture has three layers with three separate entry
points (`cargo bench`, `kermit bench`, `kermit benchmark`), partial overlap
between Layer 1 and Layer 2, and an incomplete Layer 3 stub. The thesis relies
entirely on synthetic data, making the download infrastructure unnecessary.

Consolidating under the CLI gives a single entry point with consistent
configuration, makes orchestration trivial, and removes near-duplicate
measurement code.

## What Gets Deleted

**From `kermit-ds`:**
- `benches/relation_benchmarks.rs`
- `benches/space_benchmarks.rs`
- `benches/common/` (`tuple_generation.rs`, `mod.rs`)
- Both `[[bench]]` entries in `Cargo.toml`
- `criterion` dev-dependency

**From `kermit-bench`:**
- `downloader.rs` (`Downloader`, `DownloadSpec`, `DownloadMethod`)
- `manager.rs` (`BenchmarkManager`)
- `benchmarks/oxford.rs` (`OxfordBenchmark`, `translate_dataset`, `translate_query`)
- `utils.rs` (`write_relation_to_parquet`)
- Dependencies: `arrow`, `parquet`, `fs_extra`

**From `kermit` (CLI):**
- `benchmarker.rs` (`Benchmarker<R, JA>` stub)
- `Commands::Benchmark` variant and its CLI args

## Repurposed `kermit-bench`

`kermit-bench` becomes two things: synthetic data generation and benchmark
workload configuration.

### Module structure

```
kermit-bench/src/
├── lib.rs
├── generation/
│   ├── mod.rs
│   ├── tuples.rs       (from kermit-ds/benches/common/tuple_generation.rs)
│   └── graphs.rs       (new — petgraph-based graph generators)
└── benchmarks/
    ├── mod.rs           (Benchmark enum registry)
    ├── exponential.rs   (replaces kermit-ds exponential benchmarks)
    └── factorial.rs     (replaces kermit-ds factorial benchmarks)
```

### Dependencies

Keeps: `petgraph`, `rand`, `num-traits`.
Drops: `arrow`, `parquet`, `fs_extra`.
No internal kermit dependencies.

### Generation layer

`tuples.rs` gets the existing generators:
- `generate_exponential_tuples(k)` — k^k tuples of arity k over domain 0..k
- `generate_factorial_tuples(k)` — k! tuples of arity k, position d has domain 0..=d
- `generate_distinct_tuples(n, k)` — n random distinct k-ary tuples

`graphs.rs` is new — graph generators using `petgraph`:

```rust
pub enum GraphModel {
    ErdosRenyi { n: usize, p: f64 },
    // future: BarabasiAlbert, WattsStrogatz, etc.
}

pub fn generate_graph(model: GraphModel, seed: Option<u64>) -> Vec<Vec<usize>>
```

Returns edge tuples `[src, dst]` as `Vec<Vec<usize>>`.

### Declarative generation parameters

```rust
pub enum GenerationParams {
    Exponential { k: usize },
    Factorial { k: usize },
    Graph(GraphModel),
    Custom, // for configs that override generate() with custom logic
}
```

### Benchmark config trait

```rust
pub trait BenchmarkConfig {
    fn metadata(&self) -> &BenchmarkMetadata;
    fn generate(&self, subtask: &SubTask) -> Vec<(RelationHeader, Vec<Vec<usize>>)>;
}
```

`BenchmarkMetadata` keeps its task/subtask hierarchy but drops `download_spec`.
`SubTask` carries `GenerationParams` instead of file paths. Common cases use the
declarative params directly. Complex scenarios override `generate()` with custom
logic.

### Workload definitions

**Exponential benchmark** — replaces `kermit-ds` exponential benchmarks:

```rust
static METADATA: BenchmarkMetadata = BenchmarkMetadata {
    name: "exponential",
    description: "k-ary tuples over domain 0..k, producing k^k tuples",
    tasks: &[Task {
        name: "Exponential",
        subtasks: &[
            SubTask { name: "k1", params: GenerationParams::Exponential { k: 1 } },
            SubTask { name: "k2", params: GenerationParams::Exponential { k: 2 } },
            SubTask { name: "k3", params: GenerationParams::Exponential { k: 3 } },
            SubTask { name: "k4", params: GenerationParams::Exponential { k: 4 } },
            SubTask { name: "k5", params: GenerationParams::Exponential { k: 5 } },
        ],
    }],
};
```

**Factorial benchmark** — replaces `kermit-ds` factorial benchmarks:

```rust
static METADATA: BenchmarkMetadata = BenchmarkMetadata {
    name: "factorial",
    description: "k-ary tuples where position d has domain 0..=d, producing k! tuples",
    tasks: &[Task {
        name: "Factorial",
        subtasks: &[
            SubTask { name: "k1", params: GenerationParams::Factorial { k: 1 } },
            // ... through k9
            SubTask { name: "k9", params: GenerationParams::Factorial { k: 9 } },
        ],
    }],
};
```

Both delegate `generate()` to the declarative params, calling the corresponding
function from `generation/tuples.rs`.

**Benchmark registry:**

```rust
pub enum Benchmark {
    Exponential,
    Factorial,
    // future: Triangle, RandomGraph, etc.
}
```

## CLI Changes

### New `bench suite` sub-subcommand

```
kermit bench suite --benchmark <NAME> --indexstructure <DS> [--algorithm <ALG>] [--metrics <M>...]
```

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--benchmark` | yes | — | Benchmark config name (enum variant) |
| `--indexstructure` | yes | — | Data structure to benchmark |
| `--algorithm` | no | — | Join algorithm (only needed if metrics include join) |
| `--metrics` | no | all | `insertion`, `iteration`, `space`, `join` |

Flow:
1. Look up `BenchmarkConfig` by name
2. For each task/subtask, call `config.generate(subtask)` to get relations
3. Build a `DatabaseEngine` from the generated relations
4. Run requested metrics through Criterion

### `SpaceMeasurement` + `BytesFormatter`

Move from `kermit-ds/benches/space_benchmarks.rs` to `kermit/src/measurement.rs`.
These are Criterion-specific and belong with the Criterion execution code.

### Unchanged

- `bench join` — file-based, user-supplied query + relations
- `bench ds` — file-based, single DS on single file
- Common `BenchArgs` (sample size, measurement time, warm-up)

## Dependency Flow

```
kermit-iters        (unchanged — zero deps)
kermit-derive       (unchanged — proc macro)
kermit-parser       (unchanged — winnow)
kermit-bench        (repurposed — generation + configs. petgraph, rand, num-traits)
kermit-ds           (lighter — no benches/, no criterion)
kermit-algos        (unchanged)
kermit              (CLI — depends on all above + criterion)
```

`kermit-bench` remains isolated with no internal kermit dependencies.

## What Stays The Same

- `bench join` and `bench ds` CLI interface and behavior
- `HeapSize` trait in `kermit-ds`
- `Relation`, `TrieIterable`, `JoinAlgo` traits
- `DB` trait and `instantiate_database` dispatch
- All correctness test suites (`define_multiway_join_test_suite!`, etc.)

## Equivalence

`kermit bench suite --benchmark exponential --indexstructure tree-trie --metrics insertion iteration space`
reproduces what `cargo bench --package kermit-ds` does today for `TreeTrie` exponential workloads.
