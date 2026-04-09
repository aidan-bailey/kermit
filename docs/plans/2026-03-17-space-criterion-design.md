# Space Benchmarking via Criterion

**Date:** 2026-03-17

## Goal

Produce Criterion HTML line plots of heap size (bytes) vs tuple count (N) for
each data structure, using the existing synthetic workload generators.

## Approach

Use `SpaceMeasurement` (custom `criterion::measurement::Measurement`) with
`iter_custom` and parameterized `bench_with_input`. Criterion auto-detects
numeric parameters and generates line plots.

### Zero-variance workaround

`heap_size_bytes()` is deterministic — identical across all samples. Criterion
0.7.0 panics on zero-variance data (issues #873, #887). Workaround: add ±1
byte noise per sample via `iters % 2`. This is negligible relative to actual
heap sizes.

## Design

### Separate Criterion instances for time vs space

`Criterion<WallTime>` and `Criterion<SpaceMeasurement>` are different types, so
space benchmarks run as a separate pass. When `--metrics` includes both time and
space metrics, both passes run sequentially.

### New functions

**`run_suite_space_bench<R>(...)`** — space benchmarks for `bench suite`:

1. Create `Criterion::default().with_measurement(SpaceMeasurement)` with user's
   `BenchArgs`.
2. For each task/subtask, generate data via `config.generate(subtask)`.
3. Create a benchmark group per task (e.g., `"exponential/Exponential/space"`).
4. Use `bench_with_input(BenchmarkId::new(ds_name, n_tuples), ...)` — numeric
   `n_tuples` parameter triggers Criterion's line plot.
5. Inside closure:

```rust
b.iter_custom(|iters| {
    let r = R::from_tuples(header.clone(), tuples.clone());
    let bytes = r.heap_size_bytes();
    let noise = if iters % 2 == 0 { 1 } else { 0 };
    bytes * iters as usize + noise
});
```

**`run_ds_space_bench<R>(...)`** — space benchmarks for `bench ds`:

Same pattern but a single data point from the relation file. No line plot (only
one N), just a Criterion measurement of heap bytes.

### SpaceMeasurement (no changes needed)

`start()`/`end()` are no-ops — `iter_custom` bypasses them and returns
`M::Value` directly. The parts Criterion uses:

- `add()`, `zero()`, `to_f64()` — statistics pipeline
- `formatter()` → `BytesFormatter` — y-axis units (B/KiB/MiB/GiB)

### CLI surface (no changes)

Existing `--metrics space` on `bench suite` and `bench ds` triggers the new
path. The stderr print is retained for quick inspection. `BenchArgs` fields
(`measurement_time`, `warm_up_time`) are harmless no-ops for space.

## Files changed

| File | Change |
|------|--------|
| `kermit/src/main.rs` | Add `run_suite_space_bench<R>`, `run_ds_space_bench<R>`; call when `metrics` contains `Space`; remove `#[allow(dead_code)]` on `mod measurement` |
| `kermit/src/measurement.rs` | No changes |
