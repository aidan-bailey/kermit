# Benchmarking Consolidation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Consolidate all benchmarking into the CLI by repurposing `kermit-bench` as a synthetic data generation + workload config crate, deleting all `kermit-ds` benchmarks, and adding `bench suite` to the CLI.

**Architecture:** `kermit-bench` sheds download/IO infrastructure and gains tuple generators + graph generation + declarative workload configs. The CLI (`kermit`) absorbs all Criterion execution via a new `bench suite` subcommand. `kermit-ds` becomes purely data structures with no benchmark binaries.

**Tech Stack:** Rust, Criterion 0.7, petgraph, clap 4.4

---

### Task 1: Move tuple generators into `kermit-bench`

**Files:**
- Create: `kermit-bench/src/generation/tuples.rs`
- Modify: `kermit-bench/src/generation/mod.rs`
- Modify: `kermit-bench/Cargo.toml`

**Step 1: Add `rand` and `num-traits` dependencies to `kermit-bench`**

In `kermit-bench/Cargo.toml`, add to `[dependencies]`:

```toml
rand = "0.9.1"
num-traits = "0.2.19"
```

**Step 2: Create `kermit-bench/src/generation/tuples.rs`**

Copy the three functions verbatim from `kermit-ds/benches/common/tuple_generation.rs`. Remove the `#[allow(dead_code)]` from `generate_distinct_tuples`.

```rust
use {
    num_traits::PrimInt,
    rand::{
        distr::{uniform::SampleUniform, Uniform},
        rng, Rng,
    },
    std::{collections::HashSet, hash::Hash},
};

pub fn generate_exponential_tuples<T>(k: T) -> Vec<Vec<T>>
where
    T: PrimInt + num_traits::NumCast,
{
    let k_usize = num_traits::cast::<T, usize>(k).expect("Failed to cast T to usize");
    let mut tuples: Vec<Vec<T>> = vec![];

    fn recurse<T>(k_curr: usize, k: usize, current: Vec<T>, result: &mut Vec<Vec<T>>)
    where
        T: PrimInt + num_traits::NumCast,
    {
        if k_curr == k {
            result.push(current);
            return;
        }

        for i in 0..k {
            let mut new_tuple = current.clone();
            new_tuple.push(num_traits::cast::<usize, T>(i).unwrap());
            recurse(k_curr + 1, k, new_tuple, result);
        }
    }

    recurse(0, k_usize, vec![], &mut tuples);

    tuples
}

pub fn generate_factorial_tuples<T>(k: T) -> Vec<Vec<T>>
where
    T: PrimInt + num_traits::NumCast,
{
    let k_usize = num_traits::cast::<T, usize>(k).expect("Failed to cast T to usize");
    let mut tuples: Vec<Vec<T>> = vec![];

    fn recurse<T>(k_curr: usize, k: usize, current: Vec<T>, result: &mut Vec<Vec<T>>)
    where
        T: PrimInt + num_traits::NumCast,
    {
        if k_curr == k {
            result.push(current);
            return;
        }

        for i in 0..=k_curr {
            let mut new_tuple = current.clone();
            new_tuple.push(num_traits::cast::<usize, T>(i).unwrap());
            recurse(k_curr + 1, k, new_tuple, result);
        }
    }

    recurse(0, k_usize, vec![], &mut tuples);

    tuples
}

pub fn generate_distinct_tuples<T>(n: usize, k: usize) -> Vec<Vec<T>>
where
    T: PrimInt + SampleUniform + Hash,
{
    let mut set = HashSet::new();
    let mut rng = rng();
    let dist = Uniform::new(T::min_value(), T::max_value()).ok().unwrap();

    while set.len() < n {
        let tuple: Vec<T> = (0..k).map(|_| rng.sample(&dist)).collect();
        set.insert(tuple);
    }

    set.into_iter().collect()
}
```

**Step 3: Update `kermit-bench/src/generation/mod.rs`**

```rust
pub mod tuples;
```

**Step 4: Verify it compiles**

Run: `cargo build --package kermit-bench`
Expected: success

**Step 5: Run existing tests**

Run: `cargo test --package kermit-bench`
Expected: the `oxford_benchmark` test will still exist and may fail/pass depending on network — that's fine, it gets deleted in Task 3.

**Step 6: Commit**

```bash
git add kermit-bench/src/generation/tuples.rs kermit-bench/src/generation/mod.rs kermit-bench/Cargo.toml
git commit -m "feat(kermit-bench): move tuple generators into generation module"
```

---

### Task 2: Create graph generation stub

**Files:**
- Create: `kermit-bench/src/generation/graphs.rs`
- Modify: `kermit-bench/src/generation/mod.rs`

**Step 1: Create `kermit-bench/src/generation/graphs.rs`**

Minimal stub with the `GraphModel` enum. Actual graph generation is future work.

```rust
/// Models for synthetic graph generation.
pub enum GraphModel {
    /// Erdos-Renyi random graph: each of the n*(n-1)/2 possible edges exists
    /// independently with probability p.
    ErdosRenyi { n: usize, p: f64 },
}
```

**Step 2: Update `kermit-bench/src/generation/mod.rs`**

```rust
pub mod graphs;
pub mod tuples;
```

**Step 3: Verify it compiles**

Run: `cargo build --package kermit-bench`
Expected: success (warning about unused `GraphModel` is OK)

**Step 4: Commit**

```bash
git add kermit-bench/src/generation/graphs.rs kermit-bench/src/generation/mod.rs
git commit -m "feat(kermit-bench): add graph generation stub with GraphModel enum"
```

---

### Task 3: Rework `kermit-bench` — delete download infrastructure, rewrite config types

**Files:**
- Delete: `kermit-bench/src/downloader.rs`
- Delete: `kermit-bench/src/manager.rs`
- Delete: `kermit-bench/src/utils.rs`
- Delete: `kermit-bench/src/benchmarks/oxford.rs`
- Modify: `kermit-bench/Cargo.toml` (remove `arrow`, `parquet`, `fs_extra`)
- Modify: `kermit-bench/src/lib.rs` (remove module declarations + old test)
- Modify: `kermit-bench/src/benchmark.rs` (rewrite types)
- Modify: `kermit-bench/src/benchmarks/mod.rs` (rewrite enum)

**Step 1: Remove download-related dependencies from `kermit-bench/Cargo.toml`**

Remove `arrow`, `fs_extra`, `parquet`. The `[dependencies]` section becomes:

```toml
[dependencies]
clap = { version = "4.4", features = ["derive" ] }
num-traits = "0.2.19"
petgraph = "0.8.3"
rand = "0.9.1"
```

**Step 2: Delete files**

```bash
rm kermit-bench/src/downloader.rs
rm kermit-bench/src/manager.rs
rm kermit-bench/src/utils.rs
rm kermit-bench/src/benchmarks/oxford.rs
```

**Step 3: Rewrite `kermit-bench/src/benchmark.rs`**

Replace the entire file:

```rust
use crate::generation::graphs::GraphModel;

/// Parameters for synthetic data generation within a [`SubTask`].
pub enum GenerationParams {
    /// k-ary tuples over domain 0..k, producing k^k tuples.
    Exponential { k: usize },
    /// k-ary tuples where position d has domain 0..=d, producing k! tuples.
    Factorial { k: usize },
    /// Graph-based generation using a [`GraphModel`].
    Graph(GraphModel),
    /// Custom generation logic — the [`BenchmarkConfig`] implementation
    /// handles generation directly in its [`generate`](BenchmarkConfig::generate) method.
    Custom,
}

/// A single benchmark sub-task: a specific scale or configuration.
pub struct SubTask {
    pub name: &'static str,
    pub description: &'static str,
    pub params: GenerationParams,
}

/// A group of related benchmark sub-tasks.
pub struct Task {
    pub name: &'static str,
    pub description: &'static str,
    pub subtasks: &'static [SubTask],
}

/// Static metadata describing a benchmark workload.
pub struct BenchmarkMetadata {
    pub name: &'static str,
    pub description: &'static str,
    pub tasks: &'static [Task],
}

/// Trait that each benchmark must implement to define its metadata and
/// data generation logic.
pub trait BenchmarkConfig {
    /// Returns the static metadata for this benchmark.
    fn metadata(&self) -> &BenchmarkMetadata;

    /// Generates the relation data for a given sub-task.
    ///
    /// Returns a list of `(arity, tuples)` pairs — one per relation needed
    /// by the benchmark. The arity is used to construct a [`RelationHeader`]
    /// in the CLI.
    fn generate(&self, subtask: &SubTask) -> Vec<(usize, Vec<Vec<usize>>)>;
}
```

**Step 4: Rewrite `kermit-bench/src/benchmarks/mod.rs`**

Replace the entire file:

```rust
use {crate::benchmark::BenchmarkConfig, clap::ValueEnum, std::str::FromStr};

pub mod exponential;
pub mod factorial;

/// The set of available benchmarks.
#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
pub enum Benchmark {
    Exponential,
    Factorial,
}

impl Benchmark {
    pub fn from_name(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        match name {
            | "exponential" => Ok(Self::Exponential),
            | "factorial" => Ok(Self::Factorial),
            | _ => Err(format!("Benchmark '{}' not found", name).into()),
        }
    }

    pub fn names() -> Vec<&'static str> { vec!["exponential", "factorial"] }

    pub fn name(self) -> &'static str {
        match self {
            | Self::Exponential => "exponential",
            | Self::Factorial => "factorial",
        }
    }

    pub fn config(self) -> Box<dyn BenchmarkConfig + 'static> {
        match self {
            | Self::Exponential => Box::new(exponential::ExponentialBenchmark),
            | Self::Factorial => Box::new(factorial::FactorialBenchmark),
        }
    }
}

impl FromStr for Benchmark {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_name(s).map_err(|e| e.to_string())
    }
}
```

**Step 5: Rewrite `kermit-bench/src/lib.rs`**

Replace the entire file:

```rust
//! Benchmark infrastructure for Kermit.
//!
//! Provides synthetic data generation and benchmark workload definitions.
//! [`BenchmarkConfig`](benchmark::BenchmarkConfig) defines the interface each
//! benchmark must implement.

pub mod benchmark;
pub mod benchmarks;
pub mod generation;
```

**Step 6: Verify it compiles (it won't yet — exponential.rs and factorial.rs don't exist)**

Run: `cargo check --package kermit-bench`
Expected: errors about missing `exponential` and `factorial` modules. That's expected — Task 4 creates them.

**Step 7: Commit the deletions and rewrites**

```bash
git add -A kermit-bench/
git commit -m "refactor(kermit-bench): remove download infrastructure, rewrite config types

Drop downloader, manager, oxford benchmark, and parquet utils.
Rework BenchmarkConfig trait for synthetic data generation.
Add GenerationParams enum for declarative workload specification."
```

---

### Task 4: Create exponential and factorial benchmark configs

**Files:**
- Create: `kermit-bench/src/benchmarks/exponential.rs`
- Create: `kermit-bench/src/benchmarks/factorial.rs`

**Step 1: Create `kermit-bench/src/benchmarks/exponential.rs`**

```rust
use crate::{
    benchmark::{BenchmarkConfig, BenchmarkMetadata, GenerationParams, SubTask, Task},
    generation::tuples::generate_exponential_tuples,
};

pub struct ExponentialBenchmark;

static METADATA: BenchmarkMetadata = BenchmarkMetadata {
    name: "exponential",
    description: "k-ary tuples over domain 0..k, producing k^k tuples",
    tasks: &[Task {
        name: "Exponential",
        description: "Exponential growth workload",
        subtasks: &[
            SubTask {
                name: "k1",
                description: "k=1, 1 tuple",
                params: GenerationParams::Exponential { k: 1 },
            },
            SubTask {
                name: "k2",
                description: "k=2, 4 tuples",
                params: GenerationParams::Exponential { k: 2 },
            },
            SubTask {
                name: "k3",
                description: "k=3, 27 tuples",
                params: GenerationParams::Exponential { k: 3 },
            },
            SubTask {
                name: "k4",
                description: "k=4, 256 tuples",
                params: GenerationParams::Exponential { k: 4 },
            },
            SubTask {
                name: "k5",
                description: "k=5, 3125 tuples",
                params: GenerationParams::Exponential { k: 5 },
            },
        ],
    }],
};

impl BenchmarkConfig for ExponentialBenchmark {
    fn metadata(&self) -> &BenchmarkMetadata { &METADATA }

    fn generate(&self, subtask: &SubTask) -> Vec<(usize, Vec<Vec<usize>>)> {
        match subtask.params {
            | GenerationParams::Exponential { k } => {
                let tuples = generate_exponential_tuples(k);
                vec![(k, tuples)]
            },
            | _ => unreachable!("ExponentialBenchmark only uses Exponential params"),
        }
    }
}
```

**Step 2: Create `kermit-bench/src/benchmarks/factorial.rs`**

```rust
use crate::{
    benchmark::{BenchmarkConfig, BenchmarkMetadata, GenerationParams, SubTask, Task},
    generation::tuples::generate_factorial_tuples,
};

pub struct FactorialBenchmark;

static METADATA: BenchmarkMetadata = BenchmarkMetadata {
    name: "factorial",
    description: "k-ary tuples where position d has domain 0..=d, producing k! tuples",
    tasks: &[Task {
        name: "Factorial",
        description: "Factorial growth workload",
        subtasks: &[
            SubTask {
                name: "k1",
                description: "k=1, 1 tuple",
                params: GenerationParams::Factorial { k: 1 },
            },
            SubTask {
                name: "k2",
                description: "k=2, 2 tuples",
                params: GenerationParams::Factorial { k: 2 },
            },
            SubTask {
                name: "k3",
                description: "k=3, 6 tuples",
                params: GenerationParams::Factorial { k: 3 },
            },
            SubTask {
                name: "k4",
                description: "k=4, 24 tuples",
                params: GenerationParams::Factorial { k: 4 },
            },
            SubTask {
                name: "k5",
                description: "k=5, 120 tuples",
                params: GenerationParams::Factorial { k: 5 },
            },
            SubTask {
                name: "k6",
                description: "k=6, 720 tuples",
                params: GenerationParams::Factorial { k: 6 },
            },
            SubTask {
                name: "k7",
                description: "k=7, 5040 tuples",
                params: GenerationParams::Factorial { k: 7 },
            },
            SubTask {
                name: "k8",
                description: "k=8, 40320 tuples",
                params: GenerationParams::Factorial { k: 8 },
            },
            SubTask {
                name: "k9",
                description: "k=9, 362880 tuples",
                params: GenerationParams::Factorial { k: 9 },
            },
        ],
    }],
};

impl BenchmarkConfig for FactorialBenchmark {
    fn metadata(&self) -> &BenchmarkMetadata { &METADATA }

    fn generate(&self, subtask: &SubTask) -> Vec<(usize, Vec<Vec<usize>>)> {
        match subtask.params {
            | GenerationParams::Factorial { k } => {
                let tuples = generate_factorial_tuples(k);
                vec![(k, tuples)]
            },
            | _ => unreachable!("FactorialBenchmark only uses Factorial params"),
        }
    }
}
```

**Step 3: Verify `kermit-bench` compiles**

Run: `cargo build --package kermit-bench`
Expected: success (possibly warnings about unused `GraphModel` — OK)

**Step 4: Commit**

```bash
git add kermit-bench/src/benchmarks/exponential.rs kermit-bench/src/benchmarks/factorial.rs
git commit -m "feat(kermit-bench): add exponential and factorial benchmark configs"
```

---

### Task 5: Add tests for `kermit-bench` generation and configs

**Files:**
- Modify: `kermit-bench/src/generation/tuples.rs` (add tests)
- Modify: `kermit-bench/src/lib.rs` (add integration test)

**Step 1: Add unit tests to `kermit-bench/src/generation/tuples.rs`**

Append at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_k1_produces_1_tuple() {
        let tuples = generate_exponential_tuples(1usize);
        assert_eq!(tuples, vec![vec![0]]);
    }

    #[test]
    fn exponential_k3_produces_27_tuples() {
        let tuples = generate_exponential_tuples(3usize);
        assert_eq!(tuples.len(), 27);
        assert_eq!(tuples[0].len(), 3);
    }

    #[test]
    fn factorial_k1_produces_1_tuple() {
        let tuples = generate_factorial_tuples(1usize);
        assert_eq!(tuples, vec![vec![0]]);
    }

    #[test]
    fn factorial_k4_produces_24_tuples() {
        let tuples = generate_factorial_tuples(4usize);
        assert_eq!(tuples.len(), 24);
        assert_eq!(tuples[0].len(), 4);
    }

    #[test]
    fn distinct_tuples_count() {
        let tuples: Vec<Vec<usize>> = generate_distinct_tuples(10, 3);
        assert_eq!(tuples.len(), 10);
        assert!(tuples.iter().all(|t| t.len() == 3));
    }
}
```

**Step 2: Add benchmark config test to `kermit-bench/src/lib.rs`**

Append at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use crate::{benchmark::BenchmarkConfig, benchmarks::Benchmark};

    #[test]
    fn exponential_benchmark_generates_correct_tuple_counts() {
        let config = Benchmark::Exponential.config();
        let metadata = config.metadata();
        assert_eq!(metadata.name, "exponential");

        for task in metadata.tasks {
            for subtask in task.subtasks {
                let relations = config.generate(subtask);
                assert_eq!(relations.len(), 1);
                let (arity, tuples) = &relations[0];
                assert!(tuples.iter().all(|t| t.len() == *arity));
                assert!(!tuples.is_empty());
            }
        }
    }

    #[test]
    fn factorial_benchmark_generates_correct_tuple_counts() {
        let config = Benchmark::Factorial.config();
        let metadata = config.metadata();
        assert_eq!(metadata.name, "factorial");

        for task in metadata.tasks {
            for subtask in task.subtasks {
                let relations = config.generate(subtask);
                assert_eq!(relations.len(), 1);
                let (arity, tuples) = &relations[0];
                assert!(tuples.iter().all(|t| t.len() == *arity));
                assert!(!tuples.is_empty());
            }
        }
    }

    #[test]
    fn benchmark_names_match_variants() {
        assert_eq!(Benchmark::Exponential.name(), "exponential");
        assert_eq!(Benchmark::Factorial.name(), "factorial");
    }

    #[test]
    fn benchmark_from_name_roundtrips() {
        for name in Benchmark::names() {
            assert!(Benchmark::from_name(name).is_ok());
        }
    }
}
```

**Step 3: Run tests**

Run: `cargo test --package kermit-bench`
Expected: all tests pass

**Step 4: Commit**

```bash
git add kermit-bench/src/generation/tuples.rs kermit-bench/src/lib.rs
git commit -m "test(kermit-bench): add tests for tuple generators and benchmark configs"
```

---

### Task 6: Delete `kermit-ds` benchmarks

**Files:**
- Delete: `kermit-ds/benches/relation_benchmarks.rs`
- Delete: `kermit-ds/benches/space_benchmarks.rs`
- Delete: `kermit-ds/benches/common/mod.rs`
- Delete: `kermit-ds/benches/common/tuple_generation.rs`
- Modify: `kermit-ds/Cargo.toml`

**Step 1: Delete benchmark files**

```bash
rm -r kermit-ds/benches/
```

**Step 2: Remove bench entries and dev-dependencies from `kermit-ds/Cargo.toml`**

Remove lines 20–33 (both `[[bench]]` stanzas and the entire `[dev-dependencies]` block). The file should end after line 18 (`clap = ...`).

The resulting `kermit-ds/Cargo.toml`:

```toml
[package]
name = "kermit-ds"
description = "Data structures used in Kermit"
version = "0.1.0"
authors.workspace = true
edition.workspace = true
homepage.workspace = true
license.workspace = true
readme.workspace = true
repository.workspace = true

[dependencies]
arrow = "56.2.0"
csv = "1.1"
parquet = "56.2.0"
kermit-iters = { version = "0.0.8", path = "../kermit-iters" }
kermit-derive = { version = "0.0.5", path = "../kermit-derive" }
clap = { version = "4.4", features = ["derive" ] }
```

**Step 3: Verify `kermit-ds` still builds and its tests pass**

Run: `cargo test --package kermit-ds`
Expected: all unit and integration tests pass (these test correctness, not benchmarks)

**Step 4: Commit**

```bash
git add -A kermit-ds/
git commit -m "refactor(kermit-ds): remove Criterion benchmarks

Benchmarks are now defined as workloads in kermit-bench and
executed through the CLI bench suite subcommand."
```

---

### Task 7: Create `SpaceMeasurement` module in CLI crate

**Files:**
- Create: `kermit/src/measurement.rs`
- Modify: `kermit/src/main.rs` (add `mod measurement;`)

**Step 1: Create `kermit/src/measurement.rs`**

Move `BytesFormatter` and `SpaceMeasurement` from the deleted `kermit-ds/benches/space_benchmarks.rs`:

```rust
use criterion::{
    measurement::{Measurement, ValueFormatter},
    Throughput,
};

pub struct BytesFormatter;

impl BytesFormatter {
    fn scale(typical: f64) -> (f64, &'static str) {
        if typical < 1024.0 {
            (1.0, "B")
        } else if typical < 1024.0 * 1024.0 {
            (1.0 / 1024.0, "KiB")
        } else if typical < 1024.0 * 1024.0 * 1024.0 {
            (1.0 / (1024.0 * 1024.0), "MiB")
        } else {
            (1.0 / (1024.0 * 1024.0 * 1024.0), "GiB")
        }
    }
}

impl ValueFormatter for BytesFormatter {
    fn scale_values(&self, typical_value: f64, values: &mut [f64]) -> &'static str {
        let (factor, unit) = Self::scale(typical_value);
        for val in values {
            *val *= factor;
        }
        unit
    }

    fn scale_throughputs(
        &self, _typical_value: f64, throughput: &Throughput, values: &mut [f64],
    ) -> &'static str {
        match *throughput {
            | Throughput::Elements(elems) => {
                for val in values {
                    *val /= elems as f64;
                }
                "B/elem"
            },
            | _ => {
                "B"
            },
        }
    }

    fn scale_for_machines(&self, _values: &mut [f64]) -> &'static str { "B" }
}

pub struct SpaceMeasurement;

impl Measurement for SpaceMeasurement {
    type Intermediate = ();
    type Value = usize;

    fn start(&self) -> Self::Intermediate {}

    fn end(&self, _i: Self::Intermediate) -> Self::Value { 0 }

    fn add(&self, v1: &Self::Value, v2: &Self::Value) -> Self::Value { v1 + v2 }

    fn zero(&self) -> Self::Value { 0 }

    fn to_f64(&self, value: &Self::Value) -> f64 { *value as f64 }

    fn formatter(&self) -> &dyn ValueFormatter { &BytesFormatter }
}
```

**Step 2: Add `mod measurement;` to `kermit/src/main.rs`**

Add after the imports block (after line 14), before the `Cli` struct:

```rust
mod measurement;
```

**Step 3: Verify it compiles**

Run: `cargo build --package kermit`
Expected: success (warning about unused `measurement` module is OK for now)

**Step 4: Commit**

```bash
git add kermit/src/measurement.rs kermit/src/main.rs
git commit -m "feat(kermit): add SpaceMeasurement module for Criterion space benchmarks"
```

---

### Task 8: Delete `Benchmarker` and `Commands::Benchmark` from CLI

**Files:**
- Delete: `kermit/src/benchmarker.rs`
- Modify: `kermit/src/lib.rs` (remove `pub mod benchmarker;`)
- Modify: `kermit/src/main.rs` (remove `Commands::Benchmark` variant + match arm + unused import)

**Step 1: Delete `kermit/src/benchmarker.rs`**

```bash
rm kermit/src/benchmarker.rs
```

**Step 2: Remove `pub mod benchmarker;` from `kermit/src/lib.rs`**

Remove line 16 (`pub mod benchmarker;`).

**Step 3: Remove `Commands::Benchmark` from `kermit/src/main.rs`**

Three changes:

a) Remove the `Benchmark` import on line 5:
```rust
    kermit_bench::benchmarks::Benchmark,
```

b) Remove the `Commands::Benchmark` variant (lines 140–168):
```rust
    /// Run a benchmark suite
    Benchmark {
        ...
    },
```

c) Remove the match arm (lines 360–373):
```rust
        | Commands::Benchmark {
            benchmark,
            dataset_dir,
            results_dir,
            algorithm,
            indexstructure,
        } => {
            println!("Running benchmarks:");
            println!("  Benchmark: {:?}", benchmark.name());
            println!("  Index structure: {:?}", indexstructure);
            println!("  Algorithm: {:?}", algorithm);
            println!("  Dataset directory: {:?}", dataset_dir);
            println!("  Results directory: {:?}", results_dir);
        },
```

**Step 4: Verify it compiles and existing tests pass**

Run: `cargo test --package kermit`
Expected: pass (existing `bench join` / `bench ds` CLI tests still work)

**Step 5: Commit**

```bash
git add -A kermit/
git commit -m "refactor(kermit): remove Benchmarker stub and Commands::Benchmark

The benchmark suite subcommand will be replaced by bench suite
in the next task."
```

---

### Task 9: Add `BenchSubcommand::Suite` to the CLI

**Files:**
- Modify: `kermit/src/main.rs` (add Suite variant, add import, add match arm, add `run_suite_bench` function)

**Step 1: Add `Benchmark` import back**

In the imports block at the top of `kermit/src/main.rs`, add:

```rust
    kermit_bench::{benchmark::BenchmarkConfig, benchmarks::Benchmark},
```

**Step 2: Add `Suite` variant to `BenchSubcommand` enum**

After the `Ds { ... }` variant (around line 117), add:

```rust
    /// Run a named benchmark suite on generated data
    Suite {
        /// Benchmark workload to run
        #[arg(short, long, value_name = "BENCHMARK", required = true, value_enum)]
        benchmark: Benchmark,

        /// Data structure
        #[arg(
            short,
            long,
            value_name = "INDEXSTRUCTURE",
            required = true,
            value_enum
        )]
        indexstructure: IndexStructure,

        /// Metrics to benchmark
        #[arg(
            short,
            long,
            value_enum,
            num_args = 1..,
            default_values_t = vec![Metric::Insertion, Metric::Iteration, Metric::Space]
        )]
        metrics: Vec<Metric>,
    },
```

**Step 3: Add `run_suite_bench` function**

Add after the `run_ds_bench` function:

```rust
fn run_suite_bench<R>(
    benchmark: Benchmark, metrics: &[Metric], indexstructure: IndexStructure,
    criterion: &mut criterion::Criterion,
) -> anyhow::Result<()>
where
    R: Relation + TrieIterable + HeapSize + 'static,
{
    let config = benchmark.config();
    let metadata = config.metadata();
    let ds_name = format!("{:?}", indexstructure);

    eprintln!("--- bench suite metadata ---");
    eprintln!("  benchmark:       {}", metadata.name);
    eprintln!("  data structure:  {}", ds_name);

    let has_criterion_metrics = metrics
        .iter()
        .any(|m| matches!(m, Metric::Insertion | Metric::Iteration));

    for task in metadata.tasks {
        for subtask in task.subtasks {
            let relations = config.generate(subtask);
            let group_name = format!("{}/{}/{}", metadata.name, task.name, subtask.name);

            for (arity, tuples) in &relations {
                let n = tuples.len();
                let header = (*arity).into();
                let relation = R::from_tuples(header, tuples.clone());

                if metrics.contains(&Metric::Space) {
                    let bytes = relation.heap_size_bytes();
                    eprintln!("  {group_name}: {n} tuples, arity {arity}, {bytes} heap bytes");
                }

                if has_criterion_metrics {
                    let mut group = criterion.benchmark_group(&group_name);
                    group.throughput(criterion::Throughput::Elements(n as u64));

                    if metrics.contains(&Metric::Insertion) {
                        let ins_tuples = tuples.clone();
                        let ins_header = (*arity).into();
                        group.bench_function(format!("{ds_name}/insertion"), |b| {
                            b.iter_batched(
                                || (ins_header, ins_tuples.clone()),
                                |(h, t)| R::from_tuples(h, t),
                                criterion::BatchSize::SmallInput,
                            );
                        });
                    }

                    if metrics.contains(&Metric::Iteration) {
                        group.bench_function(format!("{ds_name}/iteration"), |b| {
                            b.iter(|| {
                                relation.trie_iter().into_iter().collect::<Vec<_>>()
                            });
                        });
                    }

                    group.finish();
                }
            }
        }
    }

    criterion.final_summary();
    Ok(())
}
```

**Step 4: Add match arm for `BenchSubcommand::Suite` in the `Commands::Bench` handler**

After the `BenchSubcommand::Ds { ... }` arm, add:

```rust
                | BenchSubcommand::Suite {
                    benchmark,
                    indexstructure,
                    metrics,
                } => {
                    match indexstructure {
                        | IndexStructure::TreeTrie => {
                            run_suite_bench::<kermit_ds::TreeTrie>(
                                benchmark,
                                &metrics,
                                indexstructure,
                                &mut criterion,
                            )?;
                        },
                        | IndexStructure::ColumnTrie => {
                            run_suite_bench::<kermit_ds::ColumnTrie>(
                                benchmark,
                                &metrics,
                                indexstructure,
                                &mut criterion,
                            )?;
                        },
                    }
                },
```

**Step 5: Verify it compiles**

Run: `cargo build --package kermit`
Expected: success

**Step 6: Commit**

```bash
git add kermit/src/main.rs
git commit -m "feat(kermit): add bench suite subcommand for running named benchmark workloads"
```

---

### Task 10: Test the full pipeline

**Step 1: Run the full workspace build**

Run: `cargo build --verbose`
Expected: success

**Step 2: Run the full workspace tests**

Run: `cargo test --verbose`
Expected: all tests pass across all crates

**Step 3: Run clippy**

Run: `cargo clippy --all-targets --verbose`
Expected: no errors (warnings about unused `GraphModel` are acceptable)

**Step 4: Run fmt**

Run: `cargo fmt --all`
Then: `cargo fmt --all -- --check`
Expected: no formatting changes needed (or apply them)

**Step 5: Smoke-test the new CLI subcommand**

Run: `cargo run -- bench --sample-size 10 --measurement-time 1 --warm-up-time 1 suite --benchmark exponential --indexstructure tree-trie --metrics space`
Expected: prints heap byte counts for each exponential subtask to stderr

Run: `cargo run -- bench --sample-size 10 --measurement-time 1 --warm-up-time 1 suite --benchmark factorial --indexstructure column-trie --metrics insertion`
Expected: Criterion output for factorial insertion benchmarks

**Step 6: Verify existing CLI tests still pass**

Run: `cargo test --package kermit`
Expected: all pass including `cli_join_tests`

**Step 7: Commit any formatting fixes**

```bash
cargo fmt --all
git add -A
git commit -m "style: apply rustfmt formatting"
```

---

### Task 11: Update documentation

**Files:**
- Modify: `CLAUDE.md`
- Modify: `docs/specs/benchmarking-architecture.md`

**Step 1: Update `CLAUDE.md`**

In the "Build Commands" section, remove `cargo bench --package kermit-ds`. Add:
```
cargo run -- bench suite --benchmark exponential --indexstructure tree-trie  # Run benchmark suite
```

In the "Extending the System" section, update the "New data structure" bullet to remove "add to `define_space_benchmarks!()` in `kermit-ds/benches/space_benchmarks.rs`".

In the "Gotchas" section, update the "Space benchmarks" bullet to reference `kermit/src/measurement.rs` instead of `kermit-ds/benches/space_benchmarks.rs`.

**Step 2: Update `docs/specs/benchmarking-architecture.md`**

Mark it as superseded or update to reflect the new single-layer architecture.

**Step 3: Commit**

```bash
git add CLAUDE.md docs/specs/benchmarking-architecture.md
git commit -m "docs: update benchmarking documentation for CLI consolidation"
```
