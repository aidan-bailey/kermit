# `bench ds` Subcommand Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `bench ds` CLI sub-subcommand that benchmarks index structures in isolation (insertion time, iteration time, heap space).

**Architecture:** Restructure `Commands::Bench` into sub-subcommands (`join` and `ds`). Add a `HeapSize` trait in `kermit-ds` with implementations for `TreeTrie` and `ColumnTrie`. The CLI dispatches to a generic inner function monomorphised on the concrete data structure type.

**Tech Stack:** Rust, clap (derive), Criterion, `std::mem::size_of`

---

### Task 1: Add `HeapSize` trait and `TreeTrie` implementation

**Files:**
- Create: `kermit-ds/src/heap_size.rs`
- Modify: `kermit-ds/src/lib.rs:14-22`
- Modify: `kermit-ds/src/ds/tree_trie/implementation.rs` (add `HeapSize` impl)

**Step 1: Write the failing test**

Add to the bottom of `kermit-ds/src/ds/tree_trie/implementation.rs` inside a new test:

```rust
#[cfg(test)]
mod heap_size_tests {
    use super::*;
    use crate::{HeapSize, Relation};

    #[test]
    fn empty_tree_trie_heap_size() {
        let trie = TreeTrie::new(2.into());
        // Empty trie has no children, heap size is just the empty Vec's capacity (0)
        assert_eq!(trie.heap_size_bytes(), 0);
    }

    #[test]
    fn single_tuple_tree_trie_heap_size() {
        let trie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        // Must be > 0 (at least the root Vec + two TrieNodes with their Vecs)
        assert!(trie.heap_size_bytes() > 0);
    }

    #[test]
    fn more_tuples_means_more_heap() {
        let small = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let large = TreeTrie::from_tuples(2.into(), vec![
            vec![1, 2], vec![1, 3], vec![2, 4], vec![3, 5],
        ]);
        assert!(large.heap_size_bytes() > small.heap_size_bytes());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --package kermit-ds heap_size`
Expected: FAIL — `HeapSize` trait does not exist.

**Step 3: Create `heap_size.rs` with trait definition**

Create `kermit-ds/src/heap_size.rs`:

```rust
/// Trait for calculating heap-allocated memory usage.
///
/// Returns only the heap bytes owned by the data structure (Vec backing
/// buffers, etc.), not the stack size of the struct itself.
pub trait HeapSize {
    fn heap_size_bytes(&self) -> usize;
}
```

**Step 4: Wire the module into `kermit-ds/src/lib.rs`**

Add `mod heap_size;` and re-export `HeapSize`. In `kermit-ds/src/lib.rs`, add the module declaration and update the `pub use` block:

```rust
mod ds;
mod heap_size;
mod relation;
mod shared;

pub use {
    ds::{ColumnTrie, IndexStructure, TreeTrie},
    heap_size::HeapSize,
    relation::{Projectable, Relation, RelationError, RelationFileExt, RelationHeader},
};
```

**Step 5: Implement `HeapSize` for `TreeTrie`**

Add to `kermit-ds/src/ds/tree_trie/implementation.rs`:

```rust
impl crate::heap_size::HeapSize for TreeTrie {
    fn heap_size_bytes(&self) -> usize {
        fn node_heap_bytes(node: &TrieNode) -> usize {
            let vec_capacity_bytes =
                node.children().capacity() * std::mem::size_of::<TrieNode>();
            vec_capacity_bytes
                + node.children().iter().map(node_heap_bytes).sum::<usize>()
        }

        let root_capacity_bytes =
            self.children().capacity() * std::mem::size_of::<TrieNode>();
        root_capacity_bytes
            + self.children().iter().map(node_heap_bytes).sum::<usize>()
    }
}
```

**Step 6: Run test to verify it passes**

Run: `cargo test --package kermit-ds heap_size`
Expected: PASS (3 tests)

**Step 7: Run full kermit-ds test suite**

Run: `cargo test --package kermit-ds --verbose`
Expected: All existing tests still pass.

**Step 8: Commit**

```
feat(kermit-ds): add HeapSize trait with TreeTrie implementation
```

---

### Task 2: Add `HeapSize` implementation for `ColumnTrie`

**Files:**
- Modify: `kermit-ds/src/ds/column_trie/implementation.rs` (add `HeapSize` impl)

**Step 1: Write the failing test**

Add to the bottom of `kermit-ds/src/ds/column_trie/implementation.rs`:

```rust
#[cfg(test)]
mod heap_size_tests {
    use super::*;
    use crate::{HeapSize, Relation};

    #[test]
    fn empty_column_trie_heap_size() {
        let trie = ColumnTrie::new(2.into());
        // Empty layers have zero-capacity Vecs
        assert_eq!(trie.heap_size_bytes(), 0);
    }

    #[test]
    fn single_tuple_column_trie_heap_size() {
        let trie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        assert!(trie.heap_size_bytes() > 0);
    }

    #[test]
    fn more_tuples_means_more_heap() {
        let small = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let large = ColumnTrie::from_tuples(2.into(), vec![
            vec![1, 2], vec![1, 3], vec![2, 4], vec![3, 5],
        ]);
        assert!(large.heap_size_bytes() > small.heap_size_bytes());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --package kermit-ds heap_size_tests -- --test-threads=1`
Expected: FAIL for `ColumnTrie` tests — no `HeapSize` impl for `ColumnTrie`.

**Step 3: Implement `HeapSize` for `ColumnTrie`**

Add to `kermit-ds/src/ds/column_trie/implementation.rs`:

```rust
impl crate::heap_size::HeapSize for ColumnTrie {
    fn heap_size_bytes(&self) -> usize {
        let layers_vec_bytes =
            self.layers.capacity() * std::mem::size_of::<ColumnTrieLayer>();
        let layer_contents_bytes: usize = self
            .layers
            .iter()
            .map(|layer| {
                layer.data.capacity() * std::mem::size_of::<usize>()
                    + layer.interval.capacity() * std::mem::size_of::<usize>()
            })
            .sum();
        layers_vec_bytes + layer_contents_bytes
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --package kermit-ds heap_size_tests`
Expected: PASS (all 6 HeapSize tests — 3 for TreeTrie, 3 for ColumnTrie)

**Step 5: Run full kermit-ds test suite**

Run: `cargo test --package kermit-ds --verbose`
Expected: All tests pass.

**Step 6: Commit**

```
feat(kermit-ds): add HeapSize implementation for ColumnTrie
```

---

### Task 3: Restructure CLI — `BenchArgs`, `Metric`, `BenchSubcommand`

**Files:**
- Modify: `kermit/src/main.rs:1-118` (CLI definitions)

**Step 1: Add `Metric` enum and `BenchArgs` struct**

Add these new types in `kermit/src/main.rs` after the `QueryArgs` struct (after line 50):

```rust
#[derive(Copy, Clone, Debug, clap::ValueEnum)]
enum Metric {
    Insertion,
    Iteration,
    Space,
}

#[derive(Args)]
struct BenchArgs {
    /// Name for the Criterion benchmark group
    #[arg(short, long, value_name = "NAME")]
    name: Option<String>,

    /// Number of samples to collect (min 10)
    #[arg(long, value_name = "N", default_value = "100")]
    sample_size: usize,

    /// Measurement time per sample in seconds
    #[arg(long, value_name = "SECS", default_value = "5")]
    measurement_time: u64,

    /// Warm-up time in seconds before sampling
    #[arg(long, value_name = "SECS", default_value = "3")]
    warm_up_time: u64,
}
```

**Step 2: Add `BenchSubcommand` enum**

Add after `BenchArgs`:

```rust
#[derive(Subcommand)]
enum BenchSubcommand {
    /// Benchmark a join query
    Join {
        #[command(flatten)]
        query_args: QueryArgs,

        /// Output file for one run's results (optional)
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
    },

    /// Benchmark an index structure (insertion, iteration, space)
    Ds {
        /// Input relation data path (single file)
        #[arg(short, long, value_name = "PATH", required = true)]
        relation: PathBuf,

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
}
```

**Step 3: Replace `Commands::Bench` variant**

Replace the existing `Bench` variant (lines 64-88) in the `Commands` enum with:

```rust
    /// Run a Criterion benchmark
    Bench {
        #[command(flatten)]
        bench_args: BenchArgs,

        #[command(subcommand)]
        subcommand: BenchSubcommand,
    },
```

**Step 4: Verify it compiles**

Run: `cargo build --package kermit --verbose`
Expected: FAIL — the match arm in `main()` still references the old `Bench` fields. That's expected; we'll fix it in the next task.

**Step 5: Commit**

```
refactor(cli): restructure Bench into sub-subcommands (join, ds)

Breaking change: `kermit bench ...` becomes `kermit bench join ...`
```

---

### Task 4: Update `bench join` match arm to use new structure

**Files:**
- Modify: `kermit/src/main.rs:170-208` (the `Commands::Bench` match arm)

**Step 1: Rewrite the `Commands::Bench` match arm**

Replace the existing `Commands::Bench { ... }` match arm (lines 170-208) with:

```rust
        | Commands::Bench {
            bench_args,
            subcommand,
        } => {
            let mut criterion = criterion::Criterion::default()
                .sample_size(bench_args.sample_size)
                .measurement_time(Duration::from_secs(bench_args.measurement_time))
                .warm_up_time(Duration::from_secs(bench_args.warm_up_time));

            match subcommand {
                | BenchSubcommand::Join {
                    query_args,
                    output,
                } => {
                    let (db, join_query) = load_query(&query_args)?;

                    if let Some(path) = &output {
                        let tuples = db.join(join_query.clone());
                        let writer = BufWriter::new(fs::File::create(path)?);
                        write_tuples(writer, &tuples)?;
                    }

                    let group_name =
                        bench_args.name.as_deref().unwrap_or("join");
                    let bench_id = format!(
                        "{:?}/{:?}",
                        query_args.indexstructure, query_args.algorithm
                    );

                    eprintln!("--- bench metadata ---");
                    eprintln!(
                        "  data structure:  {:?}",
                        query_args.indexstructure
                    );
                    eprintln!(
                        "  algorithm:       {:?}",
                        query_args.algorithm
                    );
                    eprintln!(
                        "  relations:       {}",
                        query_args.relations.len()
                    );

                    let mut group = criterion.benchmark_group(group_name);
                    group.bench_function(&bench_id, |b| {
                        b.iter_batched(
                            || join_query.clone(),
                            |q| db.join(q),
                            criterion::BatchSize::SmallInput,
                        );
                    });
                    group.finish();
                    criterion.final_summary();
                },
                | BenchSubcommand::Ds { .. } => {
                    todo!("bench ds implementation — Task 5")
                },
            }
        },
```

**Step 2: Verify it compiles**

Run: `cargo build --package kermit --verbose`
Expected: PASS — compiles with `todo!()` stub for `Ds`.

**Step 3: Run existing tests**

Run: `cargo test --package kermit --verbose`
Expected: All existing tests pass (the CLI test just checks `write_tuples`, not the subcommand parsing).

**Step 4: Commit**

```
refactor(cli): wire bench join into new subcommand structure
```

---

### Task 5: Implement `bench ds` execution logic

**Files:**
- Modify: `kermit/src/main.rs` (replace `todo!()` in `BenchSubcommand::Ds` arm)

**Step 1: Add `kermit_iters` import if not already present**

Ensure these imports are at the top of `main.rs`:

```rust
use kermit_ds::{HeapSize, Relation, RelationFileExt, RelationHeader};
use kermit_iters::TrieIterable;
```

**Step 2: Write the generic inner function**

Add this function before `main()`:

```rust
fn run_ds_bench<R>(
    relation_path: &Path,
    metrics: &[Metric],
    group_name: &str,
    criterion: &mut criterion::Criterion,
) -> anyhow::Result<()>
where
    R: Relation + HeapSize + 'static,
    for<'a> R::Iter<'a>: Iterator<Item = Vec<usize>>,
{
    let extension = relation_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let relation: R = match extension.to_lowercase().as_str() {
        | "csv" => R::from_csv(relation_path)
            .map_err(|e| anyhow::anyhow!("Failed to load relation: {e}"))?,
        | "parquet" => R::from_parquet(relation_path)
            .map_err(|e| anyhow::anyhow!("Failed to load relation: {e}"))?,
        | _ => anyhow::bail!("Unsupported file extension: {extension}"),
    };

    // Extract tuples for the insertion benchmark setup closure
    let tuples: Vec<Vec<usize>> = relation.trie_iter().into_iter().collect();
    let header = relation.header().clone();

    eprintln!("--- bench ds metadata ---");
    eprintln!("  data structure:  {}", std::any::type_name::<R>().rsplit("::").next().unwrap_or("unknown"));
    eprintln!("  relation:        {}", relation_path.display());
    eprintln!("  tuples:          {}", tuples.len());
    eprintln!("  arity:           {}", header.arity());

    if metrics.contains(&Metric::Space) {
        eprintln!("  heap bytes:      {}", relation.heap_size_bytes());
    }

    let has_criterion_metrics = metrics.iter().any(|m| matches!(m, Metric::Insertion | Metric::Iteration));

    if has_criterion_metrics {
        let bench_id_prefix = std::any::type_name::<R>()
            .rsplit("::")
            .next()
            .unwrap_or("unknown");

        let mut group = criterion.benchmark_group(group_name);

        if metrics.contains(&Metric::Insertion) {
            let insertion_tuples = tuples.clone();
            let insertion_header = header.clone();
            group.bench_function(
                &format!("{bench_id_prefix}/insertion"),
                |b| {
                    b.iter_batched(
                        || (insertion_header.clone(), insertion_tuples.clone()),
                        |(h, t)| R::from_tuples(h, t),
                        criterion::BatchSize::SmallInput,
                    );
                },
            );
        }

        if metrics.contains(&Metric::Iteration) {
            group.bench_function(
                &format!("{bench_id_prefix}/iteration"),
                |b| {
                    b.iter(|| {
                        relation.trie_iter().into_iter().collect::<Vec<_>>()
                    });
                },
            );
        }

        group.finish();
        criterion.final_summary();
    }

    Ok(())
}
```

**Step 3: Replace the `todo!()` in the `Ds` arm**

```rust
                | BenchSubcommand::Ds {
                    relation,
                    indexstructure,
                    metrics,
                } => {
                    let group_name =
                        bench_args.name.as_deref().unwrap_or("ds");

                    match indexstructure {
                        | IndexStructure::TreeTrie => {
                            run_ds_bench::<kermit_ds::TreeTrie>(
                                &relation,
                                &metrics,
                                group_name,
                                &mut criterion,
                            )?;
                        },
                        | IndexStructure::ColumnTrie => {
                            run_ds_bench::<kermit_ds::ColumnTrie>(
                                &relation,
                                &metrics,
                                group_name,
                                &mut criterion,
                            )?;
                        },
                    }
                },
```

**Step 4: Verify it compiles**

Run: `cargo build --package kermit --verbose`
Expected: PASS. Note: the generic function references `R::Iter<'a>` — check that the `TrieIterable` trait provides this. If it uses `IntoIterator` on the trie iterator type directly, adjust the bound. The key requirement is that `relation.trie_iter().into_iter()` yields `Vec<usize>`. If this bound doesn't work as written, simplify to just `R: Relation + HeapSize + TrieIterable + 'static` and let the compiler infer the rest.

**Step 5: Run all tests**

Run: `cargo test --verbose`
Expected: All tests pass across the entire workspace.

**Step 6: Run clippy and fmt**

Run: `cargo clippy --all-targets --verbose && cargo fmt --all`
Expected: No warnings, code formatted.

**Step 7: Commit**

```
feat(cli): implement bench ds subcommand

Benchmarks index structures in isolation, measuring insertion time,
iteration time, and heap space via Criterion and HeapSize trait.
```

---

### Task 6: Manual smoke test

**Files:** None (verification only)

**Step 1: Create a small test CSV**

Create a temp file `/tmp/test_relation.csv`:

```csv
a,b,c
1,2,3
4,5,6
7,8,9
1,3,5
2,4,6
```

**Step 2: Run `bench ds` with all metrics on TreeTrie**

Run: `cargo run -- bench ds -r /tmp/test_relation.csv -i tree-trie --sample-size 10 --measurement-time 1 --warm-up-time 1`
Expected: Criterion output for `TreeTrie/insertion` and `TreeTrie/iteration`, plus metadata with heap bytes on stderr.

**Step 3: Run `bench ds` with single metric on ColumnTrie**

Run: `cargo run -- bench ds -r /tmp/test_relation.csv -i column-trie -m space --sample-size 10`
Expected: Only metadata with heap bytes, no Criterion benchmarks.

**Step 4: Run `bench join` to verify no regression**

Run: `cargo run -- bench join -r /tmp/test_relation.csv -r /tmp/test_relation.csv -q <some query file> -a leapfrog-triejoin -i tree-trie --sample-size 10 --measurement-time 1 --warm-up-time 1`
Expected: Same behavior as before restructuring.

**Step 5: Run full CI check**

Run: `cargo test --verbose && cargo clippy --all-targets --verbose && cargo fmt --all --check && cargo doc --workspace`
Expected: All pass.

**Step 6: Commit (if any fixups needed)**

```
fix(cli): address smoke test findings
```

---

### Summary

| Task | What | Files |
|------|------|-------|
| 1 | `HeapSize` trait + `TreeTrie` impl | `kermit-ds/src/heap_size.rs`, `lib.rs`, `tree_trie/implementation.rs` |
| 2 | `HeapSize` for `ColumnTrie` | `column_trie/implementation.rs` |
| 3 | CLI restructure (types only) | `kermit/src/main.rs` (top half) |
| 4 | Wire `bench join` into new structure | `kermit/src/main.rs` (match arm) |
| 5 | Implement `bench ds` logic | `kermit/src/main.rs` (new function + match arm) |
| 6 | Smoke test all paths | No file changes |
