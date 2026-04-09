# Space Criterion Benchmarking Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Wire `SpaceMeasurement` into Criterion so `bench suite --metrics space` and `bench ds --metrics space` produce HTML line plots of heap bytes vs tuple count.

**Architecture:** Create separate `Criterion<SpaceMeasurement>` instances for space benchmarks (different type from `Criterion<WallTime>`). Use `iter_custom` to report `heap_size_bytes()` values. Use `bench_with_input` with numeric tuple-count parameters so Criterion auto-generates line plots.

**Tech Stack:** Criterion 0.7.0 (`html_reports` feature), existing `SpaceMeasurement`/`BytesFormatter` in `kermit/src/measurement.rs`.

---

### Task 1: Make `measurement` module public

**Files:**
- Modify: `kermit/src/main.rs:16-19`

**Step 1: Remove dead_code allow and old comment**

Replace lines 16-19:

```rust
// SpaceMeasurement infrastructure for future Criterion-based space
// benchmarking.
#[allow(dead_code)]
mod measurement;
```

With:

```rust
mod measurement;
```

**Step 2: Verify it compiles**

Run: `cargo build --package kermit --verbose`
Expected: Succeeds (measurement types will be used in Task 2, but unused warnings
are acceptable temporarily since we add usage in the same PR).

**Step 3: Commit**

```bash
git add kermit/src/main.rs
git commit -m "refactor: remove dead_code allow from measurement module"
```

---

### Task 2: Add `run_suite_space_bench`

**Files:**
- Modify: `kermit/src/main.rs` (add function after `run_suite_bench`, around line 333)

**Step 1: Write `run_suite_space_bench<R>`**

Add this function after `run_suite_bench` (after line 333):

```rust
fn run_suite_space_bench<R>(
    benchmark: Benchmark, indexstructure: IndexStructure,
    bench_args: &BenchArgs,
) -> anyhow::Result<()>
where
    R: Relation + TrieIterable + HeapSize + 'static,
{
    let config = benchmark.config();
    let metadata = config.metadata();
    let ds_name = format!("{:?}", indexstructure);
    let group_name = bench_args
        .name
        .as_deref()
        .unwrap_or(&format!("{}/space", metadata.name));

    let mut criterion = criterion::Criterion::default()
        .with_measurement(measurement::SpaceMeasurement)
        .sample_size(bench_args.sample_size)
        .warm_up_time(Duration::from_secs(bench_args.warm_up_time))
        .measurement_time(Duration::from_secs(bench_args.measurement_time));

    for task in metadata.tasks {
        let mut group = criterion.benchmark_group(
            &format!("{}/{}/space", metadata.name, task.name),
        );

        for subtask in task.subtasks {
            let relations = config.generate(subtask);

            for (arity, tuples) in &relations {
                let n = tuples.len();
                let header: kermit_ds::RelationHeader = (*arity).into();
                let tuples = tuples.clone();

                eprintln!(
                    "  {}/{}/{}: {} tuples, arity {}",
                    metadata.name, task.name, subtask.name, n, arity
                );

                group.bench_with_input(
                    criterion::BenchmarkId::new(&ds_name, n),
                    &n,
                    |b, _| {
                        b.iter_custom(|iters| {
                            let r = R::from_tuples(header.clone(), tuples.clone());
                            let bytes = r.heap_size_bytes();
                            // ±1 byte noise to avoid Criterion zero-variance panic
                            // (issues #873, #887)
                            let noise = if iters % 2 == 0 { 1 } else { 0 };
                            bytes * iters as usize + noise
                        });
                    },
                );
            }
        }

        group.finish();
    }

    criterion.final_summary();
    Ok(())
}
```

**Step 2: Verify it compiles**

Run: `cargo build --package kermit --verbose`
Expected: Succeeds (function exists but is not called yet — unused warning is OK).

**Step 3: Commit**

```bash
git add kermit/src/main.rs
git commit -m "feat: add run_suite_space_bench for Criterion space benchmarks"
```

---

### Task 3: Add `run_ds_space_bench`

**Files:**
- Modify: `kermit/src/main.rs` (add function after `run_ds_bench`, around line 267)

**Step 1: Write `run_ds_space_bench<R>`**

Add this function after `run_ds_bench` (after line 267):

```rust
fn run_ds_space_bench<R>(
    relation_path: &Path, indexstructure: IndexStructure, bench_args: &BenchArgs,
) -> anyhow::Result<()>
where
    R: Relation + TrieIterable + HeapSize + 'static,
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

    let tuples: Vec<Vec<usize>> = relation.trie_iter().into_iter().collect();
    let header = relation.header().clone();
    let n = tuples.len();
    let ds_name = format!("{:?}", indexstructure);
    let group_name = bench_args.name.as_deref().unwrap_or("ds/space");

    eprintln!("--- bench ds space metadata ---");
    eprintln!("  data structure:  {}", ds_name);
    eprintln!("  relation:        {}", relation_path.display());
    eprintln!("  tuples:          {}", n);
    eprintln!("  arity:           {}", header.arity());
    eprintln!("  heap bytes:      {}", relation.heap_size_bytes());

    let mut criterion = criterion::Criterion::default()
        .with_measurement(measurement::SpaceMeasurement)
        .sample_size(bench_args.sample_size)
        .warm_up_time(Duration::from_secs(bench_args.warm_up_time))
        .measurement_time(Duration::from_secs(bench_args.measurement_time));

    let mut group = criterion.benchmark_group(group_name);
    group.bench_with_input(
        criterion::BenchmarkId::new(&ds_name, n),
        &n,
        |b, _| {
            b.iter_custom(|iters| {
                let r = R::from_tuples(header.clone(), tuples.clone());
                let bytes = r.heap_size_bytes();
                let noise = if iters % 2 == 0 { 1 } else { 0 };
                bytes * iters as usize + noise
            });
        },
    );
    group.finish();
    criterion.final_summary();

    Ok(())
}
```

**Step 2: Verify it compiles**

Run: `cargo build --package kermit --verbose`
Expected: Succeeds.

**Step 3: Commit**

```bash
git add kermit/src/main.rs
git commit -m "feat: add run_ds_space_bench for Criterion space benchmarks"
```

---

### Task 4: Wire space functions into `main()` dispatch

**Files:**
- Modify: `kermit/src/main.rs` (the `BenchSubcommand::Suite` and `BenchSubcommand::Ds` match arms)

**Step 1: Update `BenchSubcommand::Suite` arm**

Replace the Suite match arm (lines 426-447) with logic that calls the existing
`run_suite_bench` for insertion/iteration and the new `run_suite_space_bench`
for space:

```rust
| BenchSubcommand::Suite {
    benchmark,
    indexstructure,
    metrics,
} => {
    let has_time_metrics = metrics
        .iter()
        .any(|m| matches!(m, Metric::Insertion | Metric::Iteration));

    if has_time_metrics {
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
    }

    if metrics.contains(&Metric::Space) {
        match indexstructure {
            | IndexStructure::TreeTrie => {
                run_suite_space_bench::<kermit_ds::TreeTrie>(
                    benchmark,
                    indexstructure,
                    &bench_args,
                )?;
            },
            | IndexStructure::ColumnTrie => {
                run_suite_space_bench::<kermit_ds::ColumnTrie>(
                    benchmark,
                    indexstructure,
                    &bench_args,
                )?;
            },
        }
    }
},
```

**Step 2: Update `BenchSubcommand::Ds` arm**

Replace the Ds match arm (lines 398-424) similarly:

```rust
| BenchSubcommand::Ds {
    relation,
    indexstructure,
    metrics,
} => {
    let has_time_metrics = metrics
        .iter()
        .any(|m| matches!(m, Metric::Insertion | Metric::Iteration));

    if has_time_metrics {
        let group_name = bench_args.name.as_deref().unwrap_or("ds");
        match indexstructure {
            | IndexStructure::TreeTrie => {
                run_ds_bench::<kermit_ds::TreeTrie>(
                    &relation,
                    indexstructure,
                    &metrics,
                    group_name,
                    &mut criterion,
                )?;
            },
            | IndexStructure::ColumnTrie => {
                run_ds_bench::<kermit_ds::ColumnTrie>(
                    &relation,
                    indexstructure,
                    &metrics,
                    group_name,
                    &mut criterion,
                )?;
            },
        }
    }

    if metrics.contains(&Metric::Space) {
        match indexstructure {
            | IndexStructure::TreeTrie => {
                run_ds_space_bench::<kermit_ds::TreeTrie>(
                    &relation,
                    indexstructure,
                    &bench_args,
                )?;
            },
            | IndexStructure::ColumnTrie => {
                run_ds_space_bench::<kermit_ds::ColumnTrie>(
                    &relation,
                    indexstructure,
                    &bench_args,
                )?;
            },
        }
    }
},
```

**Step 3: Remove the stderr-only space printing from `run_suite_bench`**

In `run_suite_bench` (around lines 298-300), remove the space stderr block since
`run_suite_space_bench` now handles it:

Remove:
```rust
if metrics.contains(&Metric::Space) {
    let bytes = relation.heap_size_bytes();
    eprintln!("  {group_name}: {n} tuples, arity {arity}, {bytes} heap bytes");
}
```

Similarly in `run_ds_bench` (around lines 233-235), remove:
```rust
if metrics.contains(&Metric::Space) {
    eprintln!("  heap bytes:      {}", relation.heap_size_bytes());
}
```

**Step 4: Verify it compiles**

Run: `cargo build --package kermit --verbose`
Expected: Succeeds with no warnings.

**Step 5: Run existing tests**

Run: `cargo test --package kermit --verbose`
Expected: All pass (existing `write_tuples` tests are unaffected).

**Step 6: Commit**

```bash
git add kermit/src/main.rs
git commit -m "feat: wire space benchmarks into CLI dispatch"
```

---

### Task 5: Format, lint, and verify

**Files:**
- All modified files

**Step 1: Format**

Run: `cargo fmt --all`

**Step 2: Clippy**

Run: `RUSTFLAGS=-Dwarnings cargo clippy --all-targets --verbose`
Expected: No warnings or errors.

**Step 3: Full test suite**

Run: `cargo test --verbose`
Expected: All pass.

**Step 4: Doc check**

Run: `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace`
Expected: No warnings.

**Step 5: Commit if formatting changed anything**

```bash
git add -A
git commit -m "style: apply formatting"
```

---

### Task 6: Manual smoke test

**Step 1: Run space benchmark via CLI**

Run: `cargo run -- bench suite --benchmark exponential --indexstructure tree-trie --metrics space`
Expected: Criterion runs, prints space metadata to stderr, generates HTML report
in `target/criterion/`.

**Step 2: Check HTML output exists**

Run: `ls target/criterion/exponential/Exponential/space/`
Expected: Directory contains Criterion report files including `report/index.html`.

**Step 3: Verify the line plot**

Open `target/criterion/exponential/Exponential/space/report/index.html` in a
browser. Expected: line plot with tuple count on x-axis and bytes on y-axis.
