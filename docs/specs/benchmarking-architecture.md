# Benchmarking Architecture

**Date:** 2026-03-17
**Updated:** 2026-04-26 (post-output-refinement); 2026-10-22 (per-query index builds, report schema 5)

## Overview

All benchmarking in Kermit is driven through the CLI binary. There is a
single entry point (`kermit bench`) with three measurement subcommands,
three management subcommands, and one generation subcommand:

- **`bench join`** — Criterion benchmarks on user-supplied data files.
- **`bench ds`** — Criterion benchmarks on a single data structure from a
  file (build / iteration / space).
- **`bench run`** — Criterion benchmarks on a YAML-defined named workload
  drawn from `benchmarks/` (with relation files cached locally on first run).
- **`bench list`** — Print all named workloads and their cache status.
- **`bench fetch`** — Pre-fetch (download) one or all workloads and verify
  declared `sha256` digests.
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

## The query lifecycle

Since report schema 5, every `bench join` and `bench run` query goes through
four steps (`docs/specs/2026-10-22-per-query-index-build-design.md`):

1. **Load (untimed).** Each relation the queries name is read once into a
   `kermit::db::Catalog`: its header, its flat `Tuples` and statistics at
   the optimiser's `required_statistics()` level. No structure is built.
2. **Prepare (untimed, once per query).** `kermit::db::prepare` validates
   the query against the catalog, extracts one access per atom (relation,
   filter of constants and repeated variables, kept columns), scans
   filtered accesses for exact statistics, plans freely, and turns the
   plan into index specs (relation, filter, column order), merging
   identical ones.
3. **Build (timed).** Each spec reads the catalog's flat tuples through a
   borrowed `TupleView` and copies its rows exactly once, in the
   structure's own way (`ExecutionFamily::build_indexes`). An atom with only
   constants becomes a row count.
4. **Execute (timed).** The unchanged `JoinAlgo` runs over the plain
   indexes (`ExecutionFamily::execute`); the entry point projects to the
   head and applies the all-constant multiplicity.

Freeing what a timed step built is never timed: Criterion drops it after the
timer stops.

## Common arguments (`BenchArgs`)

All `bench` subcommands accept:

| Flag | Default | Description |
|------|---------|-------------|
| `--name` | varies | Criterion group name (`bench ds`) or prefix (`bench join`, `bench run`) |
| `--sample-size` | 100 | Criterion sample count (min 10) |
| `--measurement-time` | 5s | Measurement time per sample |
| `--warm-up-time` | 3s | Warm-up before sampling |
| `--report-json` | `bench-runs/{kind}-{unix-millis}.json` | Override the path of the always-emitted machine-readable JSON report |

For `bench ds`, `--name` is the full Criterion group name (`ds` if unset).
For `bench join` and `bench run`, `--name` is a *prefix* on the
group — the full name becomes
`{name}/{benchmark}/{query}/{ds}/{algo}` (defaulting to `join/...` /
`run/...` if unset) so the workload identity remains in the Criterion path.

## `kermit bench join`

Benchmarks a Datalog query over ad-hoc relation files with the same metrics
as `bench run` (`build`, `iteration`, `space` by default; `end-to-end`
opt-in).

**Arguments:** `--relations` (file paths), `--query` (.dl file),
`--algorithm`, `--indexstructure`, optional `--optimiser` (query optimiser
planning the variable ordering; defaults to `lexicographic`), optional
`--output` (writes one run's results as CSV with a header row of head
variable names), `--metrics` (defaults to `build iteration space`;
`end-to-end` is opt-in), `--queries-per-build` (K for the `end-to-end`
metric, default 1), `--ds-config` and `--ds-build` alongside the `--ds-layout-*` flags.

**Flow:**
1. Validate the layout/config/build flags for the concrete structure, resolve
   them into a `DsChoices`, and resolve the `(structure, algorithm)` pair to
   its `Execution` cell.
2. Build `bench::Workload::adhoc` (name `adhoc`, one query named by the file
   stem) and prepare it once (`Workload::prepare`): one `Catalog` of the
   relation files the query names, and the query's `Prepared`. `--output`
   and the benchmark share that catalog, so the files are read once.
3. If `--output` is set, build the query's indexes and execute the join
   once, streaming each row to the CSV with a header row of head variable
   names (`Prepared::answer_header`).
4. Run the `PreparedWorkload` through `bench::run::dispatch_run_bench` — the
   same runner as `bench run` — with prefix `--name` or `join`; function
   names as for `bench run`.

## `kermit bench ds`

Benchmarks a single data structure on a single relation file.

**Arguments:** `--relation` (single CSV/Parquet file), `--indexstructure`,
`--metrics` (defaults to `build iteration space`; `end-to-end` is
opt-in), `--queries-per-build` (K for the `end-to-end` metric, default 1).

**Metrics:**

| Metric | How measured |
|--------|--------------|
| `Build` | `RelationFamily::build_index(header, TupleView::all(&tuples))` (which applies the family's config or build mode) via Criterion `iter_batched` with a setup that clones only the header and `BatchSize::PerIteration`, on the relation's tuples in file order: no setup copy of the rows, and the built relation is freed after the timer stops |
| `Iteration` | `F::scan(&relation)` via Criterion `iter`: walks every stored tuple through `RelationFamily::for_each_tuple` (`TrieIteratorWrapper::advance` / `HashTrie::for_each_tuple`), counting each through a `black_box` sink; no tuple is materialised |
| `EndToEnd` | `RelationFamily::build_index` (same input) then K `F::scan` traversals, one timed body, via `iter_batched` with `BatchSize::PerIteration` (fresh build per sample) |
| `Space` | `heap_size_bytes()` of the relation `build_index` built once, outside the timer, via Criterion `iter_custom` with `SpaceMeasurement` |

Each metric becomes a separate Criterion `bench_function`:
`{ds_name}/build`, `{ds_name}/iteration`, `{ds_name}/end_to_end`,
`{ds_name}/space`. The two building metrics build from the tuples as the
reader produced them (`read_csv` / `read_parquet`: one flat `Tuples` in file
order), read once and viewed whole, not from the structure's own iteration
order: that order is sorted and deduplicated for the sorted tries (their
best case) and was a degenerate build order for the hash trie (issue #66),
whereas file order is the same input for every structure and Layout. K is
stamped into the report's `queries_per_build` axis (only when the metric is
requested), never into the function id.

## `kermit bench run`

Benchmarks a named YAML workload from `benchmarks/`.

**Arguments:** positional `name` or `--all`, optional `--query` (run only
that named query within the workload), `--indexstructure`, `--algorithm`,
optional `--optimiser` (defaults to `lexicographic`; no `all` sweep —
enumerate values per run), `--metrics` (defaults to `build iteration
space`; `end-to-end` is opt-in), `--queries-per-build` (K for the
`end-to-end` metric, default 1), `--verify` (run each query once before
timing and compare its result count with the YAML's `expected`).

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
4. Once per benchmark, before the loop over cells: `Workload::prepare`
   validates every query from the relation headers, reads every relation the
   queries name into one `Catalog` (flat `Tuples` plus statistics at the
   optimiser's `required_statistics()` level), and prepares each query once
   (access extraction, filtered statistics, free planning, index specs). No
   structure is built at load, and the result is shared by every cell.
5. For each query in the workload (filtered by `--query` if set), per
   cell: with `--verify`, run the query once and compare its count with
   `expected` (mismatch aborts; a query without `expected` is noted as not
   verified) — the count comes from `ExecutionFamily::count_rows`, which
   streams the join through a `black_box` sink and never materialises it, on
   fresh indexes when the family's joins mutate them (`JOIN_MUTATES`) — then
   run the chosen metrics. `Build`, `Iteration` and `EndToEnd` go through
   wall-clock Criterion; `Space` goes through `SpaceMeasurement`:
   - `build`: `ExecutionFamily::build_indexes(catalog, prepared)` — every
     index the query needs plus its all-constant checks — via `iter_batched`
     with a no-op setup and `BatchSize::PerIteration`: no setup copy, at most
     one set of indexes alive, freed after the timer stops;
   - `iteration`: `ExecutionFamily::execute`, counted through a `black_box`
     sink, over indexes built once outside Criterion, or, when
     `JOIN_MUTATES`, over a fresh build per sample made in untimed setup.
     Planning is excluded;
   - `end_to_end`: `build_indexes` plus K × `execute` in one timed body
     (`--queries-per-build`); it runs the same build and K executions as
     `build` + K × `iteration`, so a gap beyond noise between them is the
     build-to-query interaction it measures;
   - `space`: the heap bytes of the query's indexes together, one Criterion
     function per query. A lazy HashTrie keeps the check that its footprint
     is as built.

The runner itself (`run_benchmark`, in `kermit/src/bench/run.rs`) takes a
`PreparedWorkload`, which `Workload::prepare` builds from a `Workload`, and
`Workload::from_definition` makes the `Workload`: it performs the
`ensure_cached` step and the query parsing described in step 3.

`EndToEnd` is the only metric whose timed body spans the build→query
boundary: each Criterion sample builds the query's indexes from the
catalog's flat tuples through the same call as `build`, then executes and
counts the query K times (`--queries-per-build`). `BatchSize::PerIteration`
is deliberate — batching would amortise away the per-build cost the metric
exists to measure.

**Function names:** `build`, `iteration`, `end_to_end` and `space`, per
query (the Criterion group, `{name}/{benchmark}/{query}/{ds}/{algo}`, already
names the query). Before schema 5 they were `insertion`, `iteration`,
`end_to_end`, `copies`, `space/{relation_name}` and `space/Index_<π>_<base>`.
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
   directory tree (JSON estimates and raw samples; no HTML, since
   Criterion's plotting is compiled out).

3. **JSON report (`--report-json <path>`)** — a machine-readable
   `BenchReport` describing the same metadata, a structured `axes` map of
   axis values for downstream tooling (conventional keys: `data_structure`,
   `algorithm`, `optimiser`, `query`, `benchmark`, `relation_path`,
   `relation_bytes`, `tuples`, `arity`, and `index_rows` on join reports; `optimiser` is emitted by
   `bench join` and `bench run` only — `bench ds` performs no join;
   `benchmark` and `query` are now emitted by `bench join` as well (`adhoc` /
   the query file's stem); `relations` is no longer emitted), plus
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
`build_space_criterion` and `add_space_bench`, whose `iter_custom` closure
sums `heap_size_bytes()` over a slice of the measured relations (one for
`bench ds`, a query's indexes together for `bench run`) in every iteration,
behind `black_box`, so Criterion's calibration sees real work; the returned
total equals the summed heap size times `iters`, giving a deterministic
per-iter mean equal to the heap size in bytes (see
`docs/specs/space-benchmarks.md`). Criterion's plot subsystem isn't
compiled in (`kermit/Cargo.toml` opts out of default features), so the
zero-variance signal flows through to JSON without rendering — see
`docs/specs/2026-05-04-remove-criterion-graphs-design.md` for the
Python plotter that consumes it.
