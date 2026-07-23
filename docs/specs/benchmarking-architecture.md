# Benchmarking Architecture

**Date:** 2026-03-17
**Updated:** 2026-04-26 (post-output-refinement)

## Overview

All benchmarking in Kermit is driven through the CLI binary. There is a
single entry point (`kermit bench`) with three measurement subcommands,
three management subcommands, and one generation subcommand:

- **`bench join`** — Criterion benchmarks on user-supplied data files.
- **`bench ds`** — Criterion benchmarks on a single data structure from a
  file (insertion / iteration / space).
- **`bench run`** — Criterion benchmarks on a YAML-defined named workload
  drawn from `benchmarks/` (with relation files cached locally on first run).
- **`bench list`** — Print all named workloads and their cache status.
- **`bench fetch`** — Pre-fetch (download) one or all workloads.
- **`bench clean`** — Remove cached workload data.
- **`bench gen [watdiv|lubm]`** — Generate a fresh benchmark on the fly
  (materialises generator-driven data via `kermit-rdf`).

```
benchmarks/*.yml (named workloads, ZivaHub URLs)
    │ loaded via kermit-bench::discovery
    ▼
kermit bench run            Criterion on a YAML-defined workload (CLI)
kermit bench join           Criterion on user-supplied data (CLI)
kermit bench ds             Criterion on single DS from file (CLI)
kermit bench gen [watdiv|lubm]  Materialise a generator benchmark (CLI)
```

## Common arguments (`BenchArgs`)

All `bench` subcommands accept:

| Flag | Default | Description |
|------|---------|-------------|
| `--name` | varies | Criterion benchmark group name (or prefix, for `bench run`) |
| `--sample-size` | 100 | Criterion sample count (min 10) |
| `--measurement-time` | 5s | Measurement time per sample |
| `--warm-up-time` | 3s | Warm-up before sampling |
| `--report-json` | `bench-runs/{kind}-{unix-millis}.json` | Override the path of the always-emitted machine-readable JSON report |

For `bench join` and `bench ds`, `--name` is the full Criterion group name
(`join`/`ds` if unset). For `bench run`, `--name` is a *prefix* on the
group — the full name becomes
`{name}/{benchmark}/{query}/{ds}/{algo}` (defaulting to `run/...` if
unset) so the workload identity remains in the Criterion path.

## `kermit bench join`

Benchmarks end-to-end join execution time on real data.

**Arguments:** `--relations` (file paths), `--query` (.dl file),
`--algorithm`, `--indexstructure`, optional `--optimiser` (query optimiser
planning the variable ordering; defaults to `lexicographic`), optional
`--output` (writes one run's results as CSV with a header row of head
variable names).

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
`--metrics` (defaults to `insertion iteration space`; `end-to-end` is
opt-in), `--queries-per-build` (K for the `end-to-end` metric, default 1).

**Metrics:**

| Metric | How measured |
|--------|--------------|
| `Insertion` | `R::from_tuples(header, tuples)` via Criterion `iter_batched` |
| `Iteration` | `relation.trie_iter().into_iter().collect()` via Criterion `iter` |
| `EndToEnd` | `R::from_tuples` then K full-trie iterations, one timed body, via `iter_batched` with `BatchSize::PerIteration` (fresh build per sample) |
| `Space` | `R::from_tuples(...).heap_size_bytes()` via Criterion `iter_custom` with `SpaceMeasurement` |

Each metric becomes a separate Criterion `bench_function`:
`{ds_name}/insertion`, `{ds_name}/iteration`, `{ds_name}/end_to_end`,
`{ds_name}/space`. K is stamped into the report's `queries_per_build` axis
(only when the metric is requested), never into the function id.

## `kermit bench run`

Benchmarks a named YAML workload from `benchmarks/`.

**Arguments:** positional `name` or `--all`, optional `--query` (run only
that named query within the workload), `--indexstructure`, `--algorithm`,
optional `--optimiser` (defaults to `lexicographic`; no `all` sweep —
enumerate values per run), `--metrics` (defaults to `insertion iteration
space`; `end-to-end` is opt-in), `--queries-per-build` (K for the
`end-to-end` metric, default 1).

**Flow:**
1. Resolve workload(s) via `resolve_benchmarks`, which uses
   `kermit_bench::discovery::load_all_benchmarks_with_cache` (for `--all`),
   `load_benchmark`, and `load_cached_benchmark`.
2. Materialise each workload via `materialize::materialize` — for
   generator-driven (watdiv/lubm) benchmarks this generates the data on
   demand and enforces spec-hash drift detection (erroring unless
   `--force`); static workloads pass through unchanged.
3. For each workload, ensure relation files are cached locally
   (`kermit_bench::cache::ensure_cached`, downloading from the URLs in the
   YAML when missing).
4. Load relations into a `DatabaseEngine` and as raw `R` values for the
   space metric.
5. For each query in the workload (filtered by `--query` if set), run the
   chosen metrics. `Insertion`, `Iteration`, and `EndToEnd` go through
   wall-clock Criterion; `Space` goes through `SpaceMeasurement`.

`EndToEnd` is the only metric whose timed body spans the build→query
boundary: each Criterion sample constructs a fresh database from the
pre-loaded tuples **through the same pipeline as the untimed step-4 build**
(`instantiate_database` + `add_relation` + `add_keys_batch` for the sorted
family; a fresh `HashMap<String, HashTrie<H>>` via `from_tuples` handed to
`hash_join` for the hash family) and then executes the query K times
(`--queries-per-build`). `BatchSize::PerIteration` is deliberate — batching
would amortise away the per-build cost the metric exists to measure. Because
the sorted-family build path is `insert_all` (input order), the build term is
*not* comparable with the `Insertion` metric, which times the presorting
`from_tuples` path.

**Function names:** `insertion`, `iteration`, `end_to_end`, and
`space/{relation_name}`.
Note that `bench run`'s time functions are the bare strings, whereas
`bench ds` prefixes the structure name (`{ds_name}/iteration`), so the two
are *not* identical and a naive string match would not correlate them.
External tooling should key off the JSON report's `metric` field
(`ReportMetric::Time`) rather than the function string; the underlying work
also differs (trie traversal vs. join execution), though both record
wall-clock time. (`kermit-lab`'s `phase_of` relies on every time function id
*ending* with its phase token — keep that invariant when adding phases.)

## YAML workload definitions

Workloads live under `benchmarks/*.yml`. See `benchmarks/README.md` for the
schema. Each YAML lists a name, description, relation URLs (ZivaHub), and
one or more named queries (Datalog `Head :- Body, ... .` strings). Cached
relation files live under the platform cache dir
(`~/.cache/kermit/benchmarks/` on Linux).

The `kermit-bench` crate handles static-YAML workloads as well as
declarative generator declarations and their spec-hash drift detection:

```
kermit-bench/src/
├── lib.rs
├── definition.rs    BenchmarkDefinition (with generator: Option<GeneratorSpec>),
│                    QueryDefinition, RelationSource, GeneratorSpec,
│                    WatdivStressSpec, spec_hash(), validate()
├── discovery.rs     load_benchmark, load_all_benchmarks, list_benchmarks,
│                    load_all_benchmarks_with_cache, load_cached_benchmark
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
   `BenchReport` describing the same metadata, a structured `axes` map of
   axis values for downstream tooling (conventional keys: `data_structure`,
   `algorithm`, `optimiser`, `query`, `benchmark`, `relation_path`,
   `relation_bytes`, `tuples`, `arity`; `optimiser` is emitted by
   `bench join` and `bench run` only — `bench ds` performs no join), plus
   pointers into the Criterion artefact tree
   (`group`, `function`, `metric`). Always emitted
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
