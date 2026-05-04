# Benchmarking Architecture

**Date:** 2026-03-17
**Updated:** 2026-04-26 (post-output-refinement)

## Overview

All benchmarking in Kermit is driven through the CLI binary. There is a
single entry point (`kermit bench`) with three measurement subcommands and
three management subcommands:

- **`bench join`** — Criterion benchmarks on user-supplied data files.
- **`bench ds`** — Criterion benchmarks on a single data structure from a
  file (insertion / iteration / space).
- **`bench run`** — Criterion benchmarks on a YAML-defined named workload
  drawn from `benchmarks/` (with relation files cached locally on first run).
- **`bench list`** — Print all named workloads and their cache status.
- **`bench fetch`** — Pre-fetch (download) one or all workloads.
- **`bench clean`** — Remove cached workload data.

```
benchmarks/*.yml (named workloads, ZivaHub URLs)
    │ loaded via kermit-bench::discovery
    ▼
kermit bench run            Criterion on a YAML-defined workload (CLI)
kermit bench join           Criterion on user-supplied data (CLI)
kermit bench ds             Criterion on single DS from file (CLI)
```

## Common arguments (`BenchArgs`)

All `bench` subcommands accept:

| Flag | Default | Description |
|------|---------|-------------|
| `--name` | varies | Criterion benchmark group name (or prefix, for `bench run`) |
| `--sample-size` | 100 | Criterion sample count (min 10) |
| `--measurement-time` | 5s | Measurement time per sample |
| `--warm-up-time` | 3s | Warm-up before sampling |
| `--report-json` | none | Write a machine-readable JSON report to this path |

For `bench join` and `bench ds`, `--name` is the full Criterion group name
(`join`/`ds` if unset). For `bench run`, `--name` is a *prefix* on the
group — the full name becomes
`{name}/{benchmark}/{query}/{ds}/{algo}` (defaulting to `run/...` if
unset) so the workload identity remains in the Criterion path.

## `kermit bench join`

Benchmarks end-to-end join execution time on real data.

**Arguments:** `--relations` (file paths), `--query` (.dl file),
`--algorithm`, `--indexstructure`, optional `--output` (writes one run's
results as CSV with a header row of head variable names).

**Flow:**
1. Load relation files (CSV or Parquet) into a `DatabaseEngine` via
   `instantiate_database`.
2. Parse the `.dl` query file via `kermit-parser`.
3. If `--output` is set, run the join once and write results
   (`head_column_names(query)` produces the header row).
4. Wrap `db.join(query)` in Criterion's `iter_batched` (cloning the
   `JoinQuery` per sample).

## `kermit bench ds`

Benchmarks a single data structure on a single relation file.

**Arguments:** `--relation` (single CSV/Parquet file), `--indexstructure`,
`--metrics` (defaults to all three).

**Metrics:**

| Metric | How measured |
|--------|--------------|
| `Insertion` | `R::from_tuples(header, tuples)` via Criterion `iter_batched` |
| `Iteration` | `relation.trie_iter().into_iter().collect()` via Criterion `iter` |
| `Space` | `R::from_tuples(...).heap_size_bytes()` via Criterion `iter_custom` with `SpaceMeasurement` |

Each metric becomes a separate Criterion `bench_function`:
`{ds_name}/insertion`, `{ds_name}/iteration`, `{ds_name}/space`.

## `kermit bench run`

Benchmarks a named YAML workload from `benchmarks/`.

**Arguments:** positional `name` or `--all`, optional `--query` (run only
that named query within the workload), `--indexstructure`, `--algorithm`,
`--metrics` (defaults to all three).

**Flow:**
1. Resolve workload(s) via `kermit_bench::discovery::load_benchmark` or
   `load_all_benchmarks`.
2. For each workload, ensure relation files are cached locally
   (`kermit_bench::cache::ensure_cached`, downloading from the URLs in the
   YAML when missing).
3. Load relations into a `DatabaseEngine` and as raw `R` values for the
   space metric.
4. For each query in the workload (filtered by `--query` if set), run the
   chosen metrics. `Insertion` and `Iteration` go through wall-clock
   Criterion; `Space` goes through `SpaceMeasurement`.

**Function names:** `insertion`, `iteration`, and `space/{relation_name}`.
The `iteration` function name is shared with `bench ds`'s iteration metric
so external tooling can correlate `Metric::Iteration` outputs across
subcommands; the underlying work differs (trie traversal vs. join
execution), but both record wall-clock time.

## YAML workload definitions

Workloads live under `benchmarks/*.yml`. See `benchmarks/README.md` for the
schema. Each YAML lists a name, description, relation URLs (ZivaHub), and
one or more named queries (Datalog `Head :- Body, ... .` strings). Cached
relation files live under the platform cache dir
(`~/.cache/kermit/benchmarks/` on Linux).

The `kermit-bench` crate is a thin layer over this:

```
kermit-bench/src/
├── lib.rs
├── definition.rs    BenchmarkDefinition, QueryDefinition, RelationSource
├── discovery.rs     load_benchmark, load_all_benchmarks
├── cache.rs         ensure_cached, is_cached, clean_benchmark, clean_all
└── error.rs         BenchError (thiserror)
```

It has zero internal `kermit-*` dependencies and is excluded from miri
tests because of the network code.

## Output channels

Each `bench` subcommand emits three independent output streams:

1. **stderr metadata block** — a labelled, column-aligned summary of the
   benchmark configuration (data structure, algorithm, relation sizes,
   etc.). Built from a `&[MetadataLine]` slice and rendered by
   `bench_report::write_metadata_block`. Byte-valued fields use
   `measurement::format_bytes` for B/KiB/MiB/GiB scaling.

2. **Criterion artefacts** — the usual `target/criterion/{group}/{function}/`
   directory tree (HTML reports, JSON estimates, raw samples).

3. **JSON report (`--report-json <path>`)** — a machine-readable
   `BenchReport` describing the same metadata plus pointers into the
   Criterion artefact tree (`group`, `function`, `metric`). Always emitted
   as a JSON array (single-element for `bench join`/`bench ds`,
   multi-element for `bench run` with multiple queries) so downstream
   tooling has one parser shape. The schema is versioned via
   `REPORT_SCHEMA_VERSION`; bump on any breaking change to field names or
   value types.

## Space measurement

`kermit/src/measurement.rs` contains:

- `SpaceMeasurement` — implements `criterion::measurement::Measurement`
  with `type Value = usize` (heap bytes).
- `BytesFormatter` — scales to B/KiB/MiB/GiB.
- `format_bytes(n: u64) -> String` — one-shot formatting helper used by
  metadata blocks.

The space metric in `bench ds` and `bench run` is wired through
`build_space_criterion`, which uses `iter_custom` to reconstruct the data
structure per iteration so Criterion's calibration sees real work; the
returned total equals `heap_size_bytes() * iters`, giving a deterministic
per-iter mean equal to the heap size in bytes (see
`docs/specs/space-benchmarks.md`). Criterion's plot subsystem isn't
compiled in (`kermit/Cargo.toml` opts out of default features), so the
zero-variance signal flows through to JSON without rendering — see
`docs/specs/2026-05-04-remove-criterion-graphs-design.md` for the
Python plotter that consumes it.
