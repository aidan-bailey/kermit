# External Engines: DuckDB and Nemo In-Process, and a `materialize` Metric

**Date:** 2026-11-02
**Status:** Design approved in conversation, decision by decision and then section by section
(2026-11-02).
**Scope:** kermit gains a seam for running other systems' joins on its own workloads, measured by
its own measure layer:
- an `Engine` trait in a new `kermit::engine` module;
- two engines, each behind a cargo feature that is off by default: DuckDB (the `duckdb` crate) and
  Nemo (`nemo-physical`);
- an opt-in `materialize` metric on every cell, kermit's own included.

kermit's structures, algorithms and existing metrics are unchanged. The report schema moves to v7.

**Related:**
- Thesis RQ1.4 (`kermit-msc-thesis/artifacts/plan/outline.md`): "does Kermit's performance sit
  within a defensible range of an established peer engine (Nemo)?", a bounded sanity baseline
  with explicit threats to validity, not a head-to-head claim.
- The measure layer (`docs/specs/2026-10-25-bench-runner-measure-layer-design.md`): `Subject`,
  the describe step, the mutating path.
- Per-query index builds (`docs/specs/2026-10-22-per-query-index-build-design.md`): `Prepared`,
  `IndexSpec`, `TupleView`.
- The report schema (`docs/specs/bench-report-schema.md`), v6 today.
- kraken, a private sibling harness that drives DuckDB, Nemo and five other systems in-process.
  Its trie systems build their indexes in the untimed load, while its hash-join engines build
  theirs inside the timed query. This design avoids that asymmetry.

## Summary

An external engine runs exactly the query kermit's structures run. It reads `Prepared` (the
executed query over index ids, each index's `IndexSpec`, the plan) after `prepare` has validated,
rewritten, extracted accesses and planned. It never parses Datalog. Every engine answers the full
conjunctive query (every variable of the executed query), so on duplicate-free inputs its count
equals kermit's bag count and the benchmark's `expected`.

Each engine maps onto kermit's phases:

| Phase | kermit cells (unchanged) | DuckDB | Nemo |
|---|---|---|---|
| `load` (untimed) | flat tuples in the `Catalog` | in-memory database, `PRAGMA threads = N` (default 1), each relation the query reads appended from the catalog | shared handles on the catalog's tuples |
| `build` (timed) | one filtered copy per `IndexSpec`, plus building the index | per spec, `CREATE TEMP TABLE` of the spec's filtered, projected rows (one copy; no index exists) | a fresh `DatabaseInstance`; per spec a `TableProvider` streams the view's rows into Nemo's loader, and `load_trie_into_memory` sorts, deduplicates and builds the trie (one copy) |
| `iteration` (timed) | streamed, counted join | the join over the spec tables, Arrow batches counted; includes DuckDB building its hash tables | not offered |
| `materialize` (timed, opt-in) | join, full assignments into `Tuples`, sort, count distinct | the same query, batches copied into `Tuples`, the same sort and count | `execute_plan` (Nemo stores the result as a deduplicated trie), then `count_rows_in_memory` |
| `space` (exact) | `HeapSize` sum | not reported | `table_size_bytes` per table |

Like-for-like comparisons follow from where each engine's index work sits:
- **DuckDB is compared with kermit on `end_to_end`.** DuckDB's index work (its hash tables) is in
  its `iteration`, while kermit's is in `build`, so only the sum is the same work.
- **Nemo is compared on `build`, `materialize` and `space`.**

kermit-lab's `kl.engines()` preset plots only these pairs.

## Motivation

RQ1.4 needs Nemo on kermit's workloads, measured on kermit's boundaries. Nemo is the same family as
ColumnTrie: LFTJ over column-oriented tries. A production binary-join engine gives the comparison
context. DuckDB is the natural one: it is already kermit's JOB oracle (`expected` is its
`COUNT(*)`), the flake already provides it, and it is the engine Free Join and the CIDR 2025
Linear-Chained work measure against. Without a seam, each system would be a one-off.

## Decisions

All made by the user on 2026-11-02.

1. **Scope: Nemo and DuckDB.** Rejected: Nemo only (no binary-join context); kraken-scale
   positioning (DuckDB, Umbra, Soufflé, Kuzu, …), which is the most upkeep and cuts against RQ1.4's
   "bounded" framing.
2. **Both engines in-process.** Rejected: driving both command-line tools; Nemo in-process with
   DuckDB external; DuckDB external on the large workloads only. The reason is DuckDB's
   command-line timer, which prints wall time to the millisecond. With the flake's DuckDB v1.4.4,
   `SELECT 42` reported `real 0.017`. kermit's LUBM queries build in about 0.4 ms and iterate in
   about 6 µs (geometric means, 2026-10-24 sweep), and WatDiv's build in about 10 ms and iterate in
   under 0.1 ms. So an external DuckDB could not be measured on either.
3. **Nemo through upstream's public API, at a pinned revision, with no fork.** Upstream's streaming
   join (`evaluate_operation` plus `RowScan`, which kraken's vendored 0.8.1 copy calls) is private.
   The public `execute_plan` stores its result as a deduplicated trie. So Nemo has no streamed
   `iteration`, and is measured on `build`, `materialize` and `space`. Rejected: a fork exposing
   the streaming join (a fork to maintain); the public API now and the fork later (the first
   measurements would need redoing).
4. **kermit keeps streaming, and `materialize` is an opt-in extra metric, not a replacement.** The
   user asked whether kermit should materialise as Nemo does. Kept streaming because:
   - storing results adds a cost that is roughly the same for every structure, so it would dilute
     the structure differences RQ3 measures;
   - SNAP dblp `path3` returns 1,453,845,122 rows of 4 columns, about 46.5 GB as `usize`, more than
     three times the sweep's 14 GiB cap;
   - materialising "as Nemo does" means set semantics;
   - `iteration` has meant a streamed, counted join since v3.
5. **Bag projection stays** (#71, #78). Engines answer the full conjunctive query, so a
   set-semantics engine (Nemo) and a bag-semantics one (DuckDB) both match `expected` on
   duplicate-free inputs.
6. **DuckDB runs single-threaded by default** (`PRAGMA threads=1`), with `--engine-threads N` for
   more. That gives the same parallelism as kermit's joins and serial builds.
7. **Approach: an `Engine` trait at the measure-layer seam.** Rejected: hard-coded adapters in the
   runner (each system edits the runner, and there is no recipe); engines as `ExecutionFamily`
   cells (an engine would have to pose as a kermit `Relation`).
8. **Engine tests run in a separate `engines` CI job.** Rejected: engine tests run locally only.

## Background: what upstream offers (checked 2026-11-02)

**Nemo** (`knowsys/nemo`: public, Apache-2.0, nightly toolchain, workspace version 0.10.2-dev;
`main` at `04f0962b91ed`, 2026-10-18):
- **In-memory input.** `DatabaseInstance::register_table`, then
  `add_source_external(id, ColumnOrder, Box<dyn TableProvider>)`. `TableProvider` (an
  `async_trait`, `?Send`) writes `AnyDataValue`s into the public `TupleWriter`
  (`add_tuple_value`). `datasources::{table_providers, tuple_writer}` are public modules.
- **Trie building.** `load_trie_into_memory(id, order)` (async) builds the trie; Nemo's loader sorts
  and deduplicates. The trie constructors themselves (`Trie::from_rows`, …) are `pub(crate)`.
- **Plans.** A public `ExecutionPlan` builder: `fetch_table`, `join_empty`, `add_subnode`,
  `write_permanent`. A join's variable order is the order its markers are pushed.
- **Execution and results.** `execute_plan` (async) returns the result tables' ids;
  `count_rows_in_memory`, `table_row_iterator_inmemory` and `table_size_bytes` read results and
  sizes.
- **Private:** `evaluate_operation` and `collect_requiured_tries`.
- **Formats:** DSV, JSON and RDF only, no Parquet. In-memory input avoids needing them.

**DuckDB.** With the `bundled` feature, the `duckdb` crate compiles DuckDB's C++ amalgamation.
kraken uses 1.4.0 with Arrow streaming (`stream_arrow`) and `PRAGMA threads`. The flake provides
the command-line tool at v1.4.4, which the JOB generator drives (`kermit-generate/src/job/duckdb.rs`).

## Design

### Placement

- **`kermit/src/engine/` (library, `kermit::engine`).** `mod.rs` (`Engine`, `Unsupported`,
  `EngineError`), `sql.rs` (the shared SQL translator), `duckdb.rs` (feature `engine-duckdb`) and
  `nemo.rs` (feature `engine-nemo`). Engines live in the library, beside `kermit::db`, so
  `kermit/tests` can check them against the nested-loop oracle.
- **`kermit/src/bench/engine.rs` (binary).** `EngineSubject<E>` (implements `Subject`) and
  `EngineCell<E>` (implements `Cell`, below).
- **`kermit/Cargo.toml`.** `engine-duckdb = ["dep:duckdb"]` (with `bundled`) and
  `engine-nemo = ["dep:nemo-physical", "dep:futures"]`. `nemo-physical` is a git dependency at the
  pinned revision; `futures` provides `executor::block_on`. Neither feature is in `default`.
- **Root `Cargo.toml`.** `[profile.dev.package.libduckdb-sys] opt-level = 3`, beside the existing
  `tikv-jemalloc-sys` entry, so debug-built tests do not crawl.

No new crate (minimise new crates). kermit's structures and algorithms are not touched
(Priority 6).

### The `Engine` trait

```rust
pub trait Engine {
    /// The report's `system` axis: "duckdb", "nemo".
    const NAME: &'static str;
    /// Whether `stream` is implemented (`iteration`, `end_to_end`).
    const STREAMS: bool;
    /// Whether an all-constant atom's row count multiplies the results (bag) or is clamped to 1
    /// (set), as `Relation::KEEPS_DUPLICATES`. DuckDB: true. Nemo: false.
    const KEEPS_DUPLICATES: bool;
    /// Whether `materialize` changes the built state (Nemo stores its result), so that every
    /// timed sample needs a fresh build.
    const MATERIALIZE_MUTATES: bool;

    type Settings: Default;        // DuckDB: threads (default 1). Nemo: ().
    type Loaded;                   // the query's relations inside the engine
    type Built<'l> where Self: 'l; // what one timed build leaves; borrows `Loaded`

    fn version(settings: &Self::Settings) -> String;
    fn supports(prepared: &Prepared) -> Result<(), Unsupported>;
    fn load(settings: &Self::Settings, catalog: &Catalog, prepared: &Prepared)
        -> Result<Self::Loaded, EngineError>;                                       // untimed
    fn build<'l>(loaded: &'l Self::Loaded, prepared: &Prepared)
        -> Result<Self::Built<'l>, EngineError>;                                    // `build`
    fn stream(built: &Self::Built<'_>) -> Result<u64, EngineError>;                 // `iteration`
    fn materialize(built: &Self::Built<'_>) -> Result<u64, EngineError>;            // `materialize`
    fn space(built: &Self::Built<'_>) -> Option<usize>;                             // exact `space`
}
```

**Errors.** Every engine method is fallible, but timed bodies cannot return an error. So
`EngineCell` runs one fallible preflight per query, untimed and before the describe step: `load`,
`build`, and one count (`stream`, or else `materialize`). An error there becomes the cell's error,
naming benchmark, query and engine. Timed bodies then `expect`: an error that appears only after a
clean preflight is a broken invariant, and a panic is acceptable for that (Priority 5).

**All-constant atoms** are handled once, in `EngineSubject`, by the lifecycle's rule. Each check's
`TupleView::count()` is taken in `build` (timed, as kermit's are). A zero count means no rows.
Otherwise the counts multiply the result, each clamped to 1 unless `E::KEEPS_DUPLICATES`. A query
made only of checks never calls the engine. Translators therefore never see checks.

### The `Cell` refactor (`kermit/src/bench/run.rs`)

`run_benchmark<F: ExecutionFamily>` becomes `run_benchmark<C: Cell>`. `Cell` is what the per-query
loop needs from a cell:
- its labels and report axes;
- its Criterion group segment: `{ds}/{algo}` for kermit cells; for engines, the engine's name
  plus its settings (`duckdb-t{N}`, `nemo`);
- the trie-level policy it prepares under;
- the metrics it supports;
- building its `Subject` for a prepared query;
- a fallible preflight (a no-op for kermit cells).

`FamilyCell<F>` wraps every existing `ExecutionFamily` and behaves exactly as today.
`EngineCell<E>` wraps an engine. Engines prepare under `TrieLevels::All`, so they share the sorted
cells' prepared queries. Validation, `--verify`, `ReportSink`, group naming, the describe step and
`measure` stay one copy.

`Subject` gains `materialize(&self, built: &Self::Built) -> Option<Materialized>` (`None` for
`bench ds`'s `RelationSubject`) and `const MATERIALIZE_MUTATES: bool`.

### Translation

Both translators read the executed query and its specs. `extract_accesses` and `plan_indexes`
guarantee the shape:
- every atom names an index id `#k`;
- each atom has one variable per column, in index-column order;
- there are no constants, and no variable repeats within an atom.

**SQL (`engine/sql.rs`), shared by every SQL engine:**
- each spec `#k` becomes a table `idx_k`, followed by a per-build suffix;
- each atom becomes an alias `a{i}` over its spec's table, with column `c{j}` bound to the atom's
  `j`-th variable;
- every variable is selected once, at its first binding, in canonical order (the order kermit's
  executors emit);
- every later binding of a variable becomes an equality with its first;
- an atom that shares no variable with the atoms before it joins by `CROSS JOIN`;
- identifiers are double-quoted.

For the triangle under the plan `X, Y, Z`, where all three atoms read the merged spec `#0`:

```sql
SELECT a0."c0" AS "X", a0."c1" AS "Y", a1."c1" AS "Z"
FROM "idx_0" AS a0
JOIN "idx_0" AS a1 ON a1."c0" = a0."c1"
JOIN "idx_0" AS a2 ON a2."c0" = a0."c0" AND a2."c1" = a1."c1";
```

A spec's build statement selects the spec's columns from the base relation under its `Filter`.
Constants become `c{c} = v` and equalities `c{a} = c{b}`. For a ternary `r` read as `r(Y, X, c42)`
under the plan `X, Y`:
`CREATE TEMP TABLE "idx_0_<n>" AS SELECT "c1", "c0" FROM "rel_r" WHERE "c2" = 42`.

**Nemo (`engine/nemo.rs`).**
- One `OperationTable` of result markers, pushed in **kermit's plan order** (`Prepared::plan()`),
  so Nemo runs kermit's plan.
- One `fetch_table(markers, id)` per atom, over its spec's Nemo table.
- `join_empty(result_markers)`, then `add_subnode` per atom, then `write_permanent`.
- Merged specs are one Nemo table, fetched by several atoms.

### DuckDB (`engine/duckdb.rs`)

- **`load`.** An in-memory `Connection`, `PRAGMA threads = N`, and one table `rel_<name>` (columns
  `c0…`, `UBIGINT`) per relation the query reads, appended from the catalog's `Tuples` through the
  crate's appender.
- **`build`.** One `CREATE TEMP TABLE` per spec, with a per-build suffix, so the describe set and a
  timed build can coexist. `Built` holds the connection, its table names and a once-prepared
  statement. Dropping a `Built` drops its tables, which happens after the timer stops.
- **`stream`.** The statement, prepared on first use, is streamed as Arrow batches and their
  `num_rows` summed. First use falls in the preflight, the describe step or Criterion's warm-up,
  so `iteration` samples never include DuckDB's planning. Each `end_to_end` sample includes it
  once.
- **`materialize`.** The same statement. Every batch's columns are copied into kermit `Tuples`
  (row-major), then the shared sort and count (below).
- **`space`:** `None`. DuckDB keeps no index; its hash tables exist only inside a query.
- **`version`:** `SELECT library_version FROM pragma_version()`. `engine_threads` reports `N`.

### Nemo (`engine/nemo.rs`)

- **`load`.** `Arc` handles on the tuples of each relation the query reads.
- **`build`.**
  - A fresh `DatabaseInstance`.
  - Per spec, `register_table` and then `add_source_external` with a `ViewProvider { tuples: Arc,
    filter, columns }`. The provider writes each matching row's output columns as integer
    `AnyDataValue`s.
  - Then `load_trie_into_memory(id, ColumnOrder::default())` per spec, under
    `futures::executor::block_on`. That is the one copy, plus the sort, dedup and trie.
  - The plan is built here, in kermit's order. `Built` holds the database in a `RefCell` and the
    plan in a `RefCell<Option<_>>`.
- **`materialize`.** Takes the plan, runs `execute_plan` and returns `count_rows_in_memory` of the
  result. `MATERIALIZE_MUTATES = true`, so every sample runs on a fresh build in untimed setup
  (the path lazy HashTrie uses), and a second `materialize` on one `Built` panics.
- **`space`.** The sum of `table_size_bytes` over the spec tables, read in the describe step before
  anything runs.
- **`STREAMS = false`.** `iteration` and `end_to_end` are skipped with a stderr note.
- **`version`.** `nemo-physical <version> @ <rev>`, a constant that a unit test pins against the
  dependency's `rev` in `kermit/Cargo.toml` (the precedent is the Python `SCHEMA_VERSION` pin).

**The one change outside the new code:** `Catalog` holds each relation's `Tuples` behind an `Arc`.
`Box<dyn TableProvider>` is `'static`, and an `Arc` lets the provider stream from the catalog with
one copy rather than two. The existing accessors keep returning `&Tuples`, and kermit's builds
still borrow through `TupleView`, so nothing they measure changes.

### The `materialize` metric

- **Surface.** `Metric::Materialize`, `--metrics materialize`. Off by default, like `end-to-end`.
  Criterion function `materialize`. `bench ds` rejects it, since it has no join.
- **kermit cells.** A new `kermit::db::execute_assignments`, beside `execute`, lends every variable
  of the executed query in canonical order, with no head slice and with multiplicity applied as
  `execute` does. The rows are pushed into a `Tuples` whose arity is the variable count.
- **Shared sort and count.** `materialize_count(rows: Tuples) -> (Tuples, u64)` runs
  `Tuples::sort`, then counts adjacent-distinct rows. kermit and DuckDB cells both call it.
- **Why full assignments.** Deduplicating head rows would collapse duplicates that the bag
  `expected` counts, so counts could no longer be verified. On full assignments over
  duplicate-free inputs, deduplication removes nothing but pays for the sort, which is the cost
  Nemo's result trie pays.
- **Timing.** kermit and DuckDB: `iter_batched` with no setup and `BatchSize::PerIteration`; the
  routine returns its `Tuples`, so freeing happens after the timer stops. Nemo: a fresh build per
  sample.
- **Memory guard.** `materialize` is skipped, with a stderr note and a metadata line, when
  rows × variables × 8 B exceeds `--materialize-max-bytes` (default 4 GiB). The row count is
  `expected`, else the describe step's streamed count: the describe step counts whenever
  `materialize` is requested on a cell that streams. An engine that cannot stream, on a query
  without `expected`, runs unguarded with a stderr warning.
- **Verification.** `--verify` checks the `materialize` count against `expected` once, in the
  describe step (the preflight for engines).

## CLI, reports and kermit-lab

**CLI** (`bench run`, `bench join`, `bench probe`):
- **`--engine duckdb,nemo`** adds engine cells beside the `-i`/`-a` cells. `-i`/`-a` are required
  only without `--engine`. Naming an engine whose feature was not compiled in is a usage error
  naming the feature.
- **`--engine-threads N`** sets DuckDB's thread count (default 1). `--ds-*` flags apply to kermit
  cells only.
- **`--materialize-max-bytes B`** sets the guard's limit (default 4 GiB).
- **Criterion group** is `{prefix}/{workload}/{query}/{engine segment}`, e.g.
  `run/…/q0007/duckdb-t1`.
  `check_sweep_group_directories`' 64-byte clash check covers engine cells.
- **`bench probe`:** engine cells get the single-run `build`, `iteration` (streaming engines only),
  and `materialize` when asked.

**Report schema v7:**
- `system` on every report (`kermit`, `duckdb`, `nemo`);
- `engine_version`, plus `engine_threads` (DuckDB), on engine rows;
- engine rows omit `data_structure`, `algorithm` and every `ds_*` axis;
- the new function `materialize`.

The bump is not for a metric: none changes meaning, so v6 and v7 load together. It is because
every v6 run report carries `data_structure` and `algorithm`, and kermit-lab relies on that (one
back-fill keys on `algorithm` being present), so an old kermit-lab would misread engine rows.
`REPORT_SCHEMA_VERSION` and Python's `SCHEMA_VERSION` bump together; the existing `cargo test`
keeps them in step.

**kermit-lab:**
- back-fill `system = "kermit"` on rows that lack it;
- recognise `materialize` as a phase;
- a cell label (`system` for engines, `data_structure` / `algorithm` for kermit) used by the colour
  and facet defaults and by `speedup_table`;
- a preset `kl.engines()` that plots only the like-for-like pairs from the Summary.

## Testing

- **Engine oracle (`kermit/tests/engine_oracle.rs`, built only with an engine feature).**
  - Reuses `tests/common/oracle.rs` and `all_orders.rs`'s query shapes: the 16 suite patterns, plus
    the constant, repeated-variable, placeholder and all-constant cases, over seeded random
    relations.
  - Per engine: the count (DuckDB `stream`; both engines' `materialize`) and the contents
    (DuckDB's collected rows; Nemo's result rows through `table_row_iterator_inmemory`), sorted,
    against the oracle's sorted full assignments.
  - Contents matter beyond counts: on small fixtures a wrong join condition can still produce the
    right count.
- **`materialize` on kermit's cells.** The same oracle check for TreeTrie, ColumnTrie and HashTrie.
- **Translator unit tests.** Golden SQL for representative queries: constants and equalities as
  filters, a self-join with two aliases of one merged spec, a disconnected query (`CROSS JOIN`),
  the full variable list selected. A Nemo plan test that the join's markers follow kermit's plan
  order.
- **Measure-layer tests,** reusing the existing patterns:
  - `materialize` is recorded under its function name;
  - the memory guard skips and notes;
  - Nemo takes the mutating path, so every sample gets a fresh build;
  - `FamilyCell` leaves every existing report unchanged.
- **CLI and report tests:**
  - `--engine` without its feature is a usage error;
  - `-i`/`-a` are optional with `--engine`;
  - the group-name clash check covers engine cells;
  - engine rows carry the v7 axes;
  - the Nemo version constant matches `kermit/Cargo.toml`.
- **kermit-lab pytest:** v6 and v7 load together, `system` is back-filled, and `kl.engines()`
  selects only the like-for-like pairs.
- **CI.**
  - A new `engines` job runs clippy, rustdoc, the unit tests and the oracle with
    `--features engine-duckdb,engine-nemo`. The existing jobs build without the features and are
    unchanged.
  - Sweep binaries are built with both features. Each report's `system` and `engine_version`
    axes say what ran.

## Premises to verify first

Check these before steps 3 and 4 of the sequencing. Each one either holds or changes the design
above.
1. **DuckDB:** a prepared statement executes without re-planning; Arrow streaming
   (`stream_arrow`) and the appender (for `UBIGINT`) exist in the version pinned.
2. **Nemo:** `load_trie_into_memory` and `execute_plan` complete under a plain
   `futures::executor::block_on`, with no Tokio runtime.
3. **Nemo:** whether `nemo-physical` uses threads when loading or joining. If it does, record it
   as `engine_threads`, and limit it if it can be configured.
4. **Nemo:** an integer `AnyDataValue` constructor takes kermit's ids losslessly (they are below
   2^63), with no dictionary needed.
5. **Nemo:** `table_size_bytes` counts the loaded tries, not only the sources.
6. **Builds:** both dependency trees compile on kermit's nightly toolchain. Duplicate `arrow`
   versions (DuckDB's against `kermit-ds`'s Parquet stack) cost compile time only.

## Measurement

None in this change. The next sweep (#99) adds the engine cells (DuckDB at one thread), and
`materialize` on the queries under the guard.

## Out of scope

- Command-line engines (Umbra, Soufflé). They would need a second, out-of-process runner reusing
  the translators and reports.
- A streamed `iteration` for Nemo (the fork, decision 3).
- Multi-threaded DuckDB as the default.
- DuckDB `space`.
- Per-index measurements.
- A `DISTINCT` projection (#71's open decision).
- `materialize` on `bench ds`.

## Sequencing

1. `materialize` on kermit's cells: `execute_assignments`, `materialize_count`, `Metric`, the guard,
   `--verify`. It is useful on its own.
2. The `Cell` refactor of `run_benchmark` (`FamilyCell`; no behaviour change).
3. `Engine`, `EngineSubject`, `EngineCell`, the SQL translator and DuckDB, with the oracle and the
   `engines` CI job.
4. Nemo, with the `Catalog` `Arc` change.
5. Report v7 and kermit-lab (`system`, `kl.engines()`).
6. Documentation.

Each step leaves the gate green. Steps 1 and 2 change nothing in an existing report except adding
`materialize` where it is asked for.

## Acceptance

1. With both features, the engine oracle passes for DuckDB and Nemo on counts and contents, in the
   `engines` job.
2. `bench run triangle -i all -a all --engine duckdb,nemo --metrics build iteration space
   materialize end-to-end --verify` writes a v7 report with three kermit cells and two engine
   cells. It skips Nemo's `iteration` and `end_to_end` with notes, and every count is verified.
3. `bench probe --engine duckdb,nemo` with `--verify` passes on one query from every benchmark
   family (triangle, WatDiv, LUBM, Oxford, SNAP, H2O, JOB).
4. kermit-lab loads v6 and v7 reports together, and `kl.engines()` renders.
5. A default `cargo build` compiles neither engine, and the existing CI jobs are unchanged.
6. The documentation below is written.

## Documentation

- `docs/engines/TEMPLATE.md`, `docs/engines/duckdb.md` and `docs/engines/nemo.md`. Each covers its
  phase mapping, fairness caveats, version pin and how to move the pin.
- `CLAUDE.md`:
  - a new recipe, "Adding an external engine": module, `Engine` impl, feature, CLI value, oracle
    cell, doc;
  - `kermit::engine` in the architecture section;
  - gotchas: features off by default, full-assignment `materialize`, schema v7, the like-for-like
    pairs.
- `docs/specs/bench-report-schema.md`: v7.
- `BENCHMARKING.md`: running engines, and which pairs compare.
