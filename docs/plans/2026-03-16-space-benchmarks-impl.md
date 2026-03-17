# Space Benchmarks Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add Criterion micro-benchmarks measuring heap space (via `HeapSize` trait) for each data structure across exponential and factorial input sizes.

**Architecture:** New benchmark binary `space_benchmarks` with a custom Criterion `Measurement` implementation that measures bytes instead of nanoseconds. Uses `iter_custom` to construct data structures and return `heap_size_bytes()`. Same macro pattern as existing `relation_benchmarks.rs`.

**Tech Stack:** Criterion 0.7 custom `Measurement` + `ValueFormatter` traits, existing `HeapSize` trait, `paste` macro crate.

---

### Task 1: Register new benchmark binary in Cargo.toml

**Files:**
- Modify: `kermit-ds/Cargo.toml:20-22`

**Step 1: Add bench entry**

Add a second `[[bench]]` section after the existing one:

```toml
[[bench]]
name = "space_benchmarks"
harness = false
```

So lines 20-26 become:

```toml
[[bench]]
name = "relation_benchmarks"
harness = false

[[bench]]
name = "space_benchmarks"
harness = false
```

**Step 2: Verify Cargo.toml parses**

Run: `cargo metadata --no-deps --format-version=1 -q 2>&1 | head -1`
Expected: JSON output (no parse errors)

**Step 3: Commit**

```bash
git add kermit-ds/Cargo.toml
git commit -m "chore(kermit-ds): register space_benchmarks binary"
```

---

### Task 2: Create space_benchmarks.rs with custom Measurement

**Files:**
- Create: `kermit-ds/benches/space_benchmarks.rs`

**Step 1: Write the benchmark file**

```rust
use {
    common::tuple_generation::{generate_exponential_tuples, generate_factorial_tuples},
    criterion::{
        criterion_group, criterion_main,
        measurement::{Measurement, ValueFormatter},
        BenchmarkGroup, Criterion, Throughput,
    },
    kermit_ds::{ColumnTrie, HeapSize, Relation, TreeTrie},
};

mod common;

// --- Custom Measurement ---

struct BytesFormatter;

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
        &self,
        _typical_value: f64,
        throughput: &Throughput,
        values: &mut [f64],
    ) -> &'static str {
        match *throughput {
            | Throughput::Elements(elems) => {
                for val in values {
                    *val /= elems as f64;
                }
                "B/elem"
            },
            | _ => {
                // No meaningful throughput interpretation for other variants
                "B"
            },
        }
    }

    fn scale_for_machines(&self, _values: &mut [f64]) -> &'static str { "B" }
}

struct SpaceMeasurement;

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

// --- Benchmark functions ---

fn bench_relation_space<R: Relation + HeapSize>(
    group: &mut BenchmarkGroup<SpaceMeasurement>,
) {
    for k in [1, 2, 3, 4, 5] {
        let tuples = generate_exponential_tuples(num_traits::cast(k).unwrap());
        let n = tuples.len();
        group.throughput(Throughput::Elements(n as u64));
        group.bench_function(format!("Space/Exponential/{k}/{n}"), |b| {
            b.iter_custom(|iters| {
                let mut total = 0usize;
                for _ in 0..iters {
                    let relation = R::from_tuples(k.into(), tuples.clone());
                    total += relation.heap_size_bytes();
                }
                total
            });
        });
    }

    for h in [1, 2, 3, 4, 5, 6, 7, 8, 9] {
        let tuples = generate_factorial_tuples(num_traits::cast(h).unwrap());
        let n = tuples.len();
        group.throughput(Throughput::Elements(n as u64));
        group.bench_function(format!("Space/Factorial/{h}/{n}"), |b| {
            b.iter_custom(|iters| {
                let mut total = 0usize;
                for _ in 0..iters {
                    let relation = R::from_tuples(h.into(), tuples.clone());
                    total += relation.heap_size_bytes();
                }
                total
            });
        });
    }
}

fn bench_trie_relation_space<R: Relation + HeapSize>(
    groupname: &str,
    c: &mut Criterion<SpaceMeasurement>,
) {
    let mut group = c.benchmark_group(groupname);
    group.sample_size(10);
    bench_relation_space::<R>(&mut group);
    group.finish();
}

// --- Macro + harness ---

macro_rules! define_space_benchmarks {
    (
        $(
            $relation_type:ident
        ),+
    ) => {
        paste::paste! {
            $(
                fn [<bench_space_ $relation_type:lower>](
                    c: &mut Criterion<SpaceMeasurement>,
                ) {
                    bench_trie_relation_space::<$relation_type>(
                        stringify!($relation_type),
                        c,
                    );
                }
            )+

            criterion_group! {
                name = space_benches;
                config = Criterion::default().with_measurement(SpaceMeasurement);
                targets = $(
                    [<bench_space_ $relation_type:lower>]
                ),+
            }
        }
    };
}

define_space_benchmarks!(TreeTrie, ColumnTrie);

criterion_main!(space_benches);
```

**Step 2: Verify it compiles**

Run: `cargo bench --package kermit-ds --bench space_benchmarks --no-run`
Expected: Compiles without errors.

**Step 3: Run the space benchmarks**

Run: `cargo bench --package kermit-ds --bench space_benchmarks`
Expected: Output showing byte measurements for each data structure at each input size.
Values should be deterministic (tight confidence intervals). Unit labels show B, KiB, or MiB.

**Step 4: Run rustfmt**

Run: `cargo fmt --all`
Expected: No or minimal formatting changes (code was written to match project style).

**Step 5: Run clippy**

Run: `RUSTFLAGS=-Dwarnings cargo clippy --package kermit-ds --all-targets --verbose`
Expected: No warnings or errors.

**Step 6: Verify existing benchmarks still work**

Run: `cargo bench --package kermit-ds --bench relation_benchmarks --no-run`
Expected: Compiles without errors. Existing benchmarks unaffected.

**Step 7: Commit**

```bash
git add kermit-ds/benches/space_benchmarks.rs
git commit -m "feat(kermit-ds): add Criterion space benchmarks via custom Measurement"
```
