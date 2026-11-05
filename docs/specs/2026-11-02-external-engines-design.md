# External Engines: DuckDB and Nemo In-Process, and a `materialize` Metric

**Date:** 2026-11-02
**Status:** Design approved in conversation, decision by decision and then section by section
(2026-11-02). The premises were then checked (§ Premises), decisions 9–13 followed, and the
supervised plan's skeleton is `docs/superpowers/plans/2026-11-02-external-engines.md`.
**Scope:** kermit gains a seam for running other systems' joins on its own workloads, measured by
its own measure layer:
- an `Engine` trait in a new `kermit::engine` module;
- two engines, each behind a cargo feature that is off by default: DuckDB (the `duckdb` crate) and
  Nemo (`nemo-physical`);
- an opt-in `materialize` metric on every cell, kermit's own included.

kermit's structures, algorithms and existing metrics are unchanged. The report schema moves to v8
(decision 14).

**Related:**
- Thesis RQ1.4 (`kermit-msc-thesis/artifacts/plan/outline.md`): "does Kermit's performance sit
  within a defensible range of an established peer engine (Nemo)?", a bounded sanity baseline
  with explicit threats to validity, not a head-to-head claim.
- The measure layer (`docs/specs/2026-10-25-bench-runner-measure-layer-design.md`): `Subject`,
  the describe step, the mutating path.
- Per-query index builds (`docs/specs/2026-10-22-per-query-index-build-design.md`): `Prepared`,
  `IndexSpec`, `TupleView`.
- The report schema (`docs/specs/bench-report-schema.md`), v7 today: since master's
  7d7eb5fe, `iteration` excludes the join's per-query setup.
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

Decisions 9 to 13 followed the premise check (§ Premises), on 2026-11-02. The user took the
recommendation on all five:

9. **The flake's toolchain moves to `nightly-2026-11-02` or later, as the first step.**
   `nemo-physical` at the pinned revision does not compile on the flake's `nightly-2026-10-31`.
   The error is `E0391` ("cycle detected when computing when `TrieScanFilter` needs async drop"),
   from a rustc regression on mutually recursive types held across `.await`
   (rust-lang/rust#165263, closed 2026-11-01). CI's floating nightly already has the fix.
10. **DuckDB is the `duckdb` crate's `~1.4` line (DuckDB 1.4.x), linked two ways.**
    - Pinning `~1.4` matches the flake's DuckDB 1.4.4, which computes JOB's `expected`. The newest
      crate (1.20000.1) is DuckDB 2.0.
    - `engine-duckdb` links the system `libduckdb`, found through `DUCKDB_LIB_DIR` and
      `DUCKDB_INCLUDE_DIR`. The flake's dev shell exports both, from Nix's `duckdb.lib` and
      `duckdb.dev` outputs, so a dev-shell build compiles no C++.
    - `engine-duckdb-bundled` adds `duckdb/bundled`, which compiles DuckDB's amalgamation, for CI
      (no Nix).
    - Sweeps build in the dev shell, so they measure 1.4.4. Every report's `engine_version` records
      the library that actually ran (`pragma_version()`).
11. **DuckDB prepares its statement at the end of `build`, not on first use.** This needs no
    `OnceCell`, which the flow profile marks as a landmine (interior mutability). DuckDB's `build`
    therefore includes its planning (well under a millisecond on these queries). `iteration` still
    excludes it, and `end_to_end` includes it once per sample, as before.
12. **Bounds and limits:**
    - `--engine-threads` is in `1..=available_parallelism()`, default 1;
    - `--materialize-max-bytes` is in `1 MiB..=1 TiB`, default 4 GiB;
    - a value outside its range is a usage error naming the range;
    - kermit sets no timeout of its own; the sweep harness's per-step timeout and memory cap apply;
    - no engine calls an outside service at run time, since Nemo is a build-time git dependency;
    - an engine that fails ends its cell with an error, and nothing is retried.
13. **The flow profile's gates build no cargo features.** Every engine step's Verify line passes its
    `--features` explicitly. The owner adds the `engines` CI job's command to the profile's `final`
    gates.
14. **The report schema moves to v8, not v7, and local master is merged in first** (the owner,
    2026-11-02, during Story 9). Local master had already taken v7 (7d7eb5fe: `iteration`
    excludes the join's per-query setup, a meaning change that kermit-lab refuses to mix with v6),
    together with the `ExecutedQuery` join API (2987d9dd), which Story 10's runner code meets.
    The merge (180ee8c0) lands before Story 9, so this change's bump is v7 → v8. It is
    structural, so v7 and v8 load together. Rejected: v8 without the merge (Stories 10–12 would
    be built on the old join API); keeping v7 (two v7s with different meanings).

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
- **`kermit/Cargo.toml`:**
  - `engine-duckdb = ["dep:duckdb"]`, with `duckdb = "~1.4"` and no default features, linking the
    system `libduckdb` (decision 10);
  - `engine-duckdb-bundled = ["engine-duckdb", "duckdb/bundled"]` for CI;
  - `engine-nemo = ["dep:nemo-physical", "dep:futures", "dep:async-trait"]`. `nemo-physical` is a
    git dependency at `rev = "04f0962b91edc1bbe73e6f65f08d41d1ee491980"`; `futures` provides
    `executor::block_on`; `async-trait` implements `TableProvider`.
  
  None of these is in `default`.
- **`kermit/build.rs` (new).** Under `engine-duckdb` without `engine-duckdb-bundled`, it emits
  `cargo:rustc-link-arg=-Wl,-rpath,<DUCKDB_LIB_DIR>`. A dev-shell binary then finds Nix's
  `libduckdb.so` when run outside the shell, as sweep binaries are. With `DUCKDB_LIB_DIR` unset and
  no bundling, it fails the build with "engine-duckdb needs DUCKDB_LIB_DIR (the nix dev shell sets
  it) or the engine-duckdb-bundled feature".
- **`flake.nix`.** The dev shell exports `DUCKDB_LIB_DIR = "${pkgs.duckdb.lib}/lib"` and
  `DUCKDB_INCLUDE_DIR = "${pkgs.duckdb.dev}/include"`, and its `rust-overlay` input is locked at a
  nightly of 2026-11-02 or later (decision 9).
- **Root `Cargo.toml`.** `[profile.dev.package.libduckdb-sys] opt-level = 3`, beside the existing
  `tikv-jemalloc-sys` entry, so a bundled debug build's tests do not crawl.

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
    /// Whether `space` reports anything (DuckDB: false; it keeps no index).
    const HAS_SPACE: bool;

    type Settings: Default + Clone + std::fmt::Debug; // DuckDB: threads (default 1). Nemo: ().
    type Loaded;                   // the query's relations inside the engine
    type Built<'l> where Self: 'l; // what one timed build leaves; may borrow `Loaded`

    /// The Criterion group segment and stderr label: "duckdb-t1", "nemo".
    fn label(settings: &Self::Settings) -> String;
    /// The `engine_threads` axis: `Some(n)` for DuckDB, `None` for Nemo.
    fn threads(settings: &Self::Settings) -> Option<usize>;
    /// The `engine_version` axis, read from the running engine (DuckDB: `pragma_version()`).
    fn version(loaded: &Self::Loaded) -> String;
    fn supports(prepared: &Prepared) -> Result<(), Unsupported>;
    fn load(settings: &Self::Settings, catalog: &Catalog, prepared: &Prepared)
        -> Result<Self::Loaded, EngineError>;                                       // untimed
    fn build<'l>(loaded: &'l Self::Loaded, catalog: &Catalog, prepared: &Prepared)
        -> Result<Self::Built<'l>, EngineError>;                                    // `build`
    fn stream(built: &Self::Built<'_>) -> Result<u64, EngineError>;                 // `iteration`
    fn materialize(built: &Self::Built<'_>) -> Result<u64, EngineError>;            // `materialize`
    fn space(built: &Self::Built<'_>) -> Option<usize>;                             // exact `space`
    /// The full assignments in canonical order, for the oracle tests; never timed. Nemo's result
    /// columns follow kermit's plan order, so its `rows` permutes them.
    fn rows(built: &Self::Built<'_>) -> Result<Vec<Vec<usize>>, EngineError>;
}
```

`build` takes the catalog because Nemo's providers stream from `Catalog::shared_tuples`.
(Superseded by Amendment 1: Nemo took its handles in `load`, so `build` no longer takes the
catalog, and the five constants and `label` are one `CAPABILITIES`.)

**Errors.** Every engine method is fallible, but timed bodies cannot return an error. So
`EngineCell` runs one fallible preflight per query, untimed and before the describe step: `load`,
`build`, and one count (`stream`, or else `materialize`). An error there becomes the cell's error,
naming benchmark, query and engine. Timed bodies then `expect`: an error that appears only after a
clean preflight is a broken invariant, and a panic is acceptable for that (Priority 5).

`EngineError` is a struct, `{ engine: &'static str, message: String }`, with a manual `Display`
(`"{engine}: {message}"`) and `std::error::Error`, following kermit's library convention.
`Unsupported` is `{ reason: String }`.

**All-constant atoms** are handled once, by the lifecycle's rule, factored out as
`kermit::db::check_multiplicity(catalog, prepared, keeps_duplicates) -> usize`. `build_with` calls
it, and so does `EngineSubject`, with `E::KEEPS_DUPLICATES`, in its timed `build`. A zero count
means no rows. Otherwise the counts multiply the result, each clamped to 1 unless the
duplicate-keeping flag is set. A query made only of checks never calls the engine, so translators
never see checks.

**DuckDB's own limits.** Every connection sets `SET memory_limit = '8GB'`, under the sweep's 14 GiB
cap. DuckDB's default is 80% of RAM, which this host cannot spare. Each connection also gets
`SET temp_directory` to a private directory, `<std::env::temp_dir()>/kermit-duckdb-<pid>-<n>`,
removed when the connection drops. DuckDB deletes every spill file in its temp directory on exit,
so two connections sharing one would corrupt each other's spills, the hazard the JOB generator
already guards against. (Amendment 1 moves the directory onto disk, under
`DuckDbSettings::spill_parent`.)

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

`Subject` changes in four ways:
- `type Built<'s> where Self: 's`, so an engine's built state can borrow the loaded database its
  subject owns;
- `const STREAMS: bool`, so the describe step counts a non-streaming subject (Nemo) through
  `materialize`;
- `const MATERIALIZE_MUTATES: bool`;
- `materialize(&self, built: &Self::Built<'_>) -> Option<Materialized>`, which returns `None` for
  `bench ds`'s `RelationSubject`.

`Materialized` carries the distinct-row count and, for kermit and DuckDB, the sorted `Tuples`.
Returning the batch means Criterion frees it after the timer stops.

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

A spec's build statement selects the spec's columns from the base relation under its `Filter`,
renamed `c0…` in output order, so the join reads index column `j` as `c{j}`. Every source column
is qualified by its table, because an output name can equal another source column's name (`"c1"
AS "c0"`); the reference then never depends on an engine's alias rules. Constants become
`c{c} = v` and equalities `c{a} = c{b}`, joined by `AND`. For a ternary `r` read as
`r(Y, X, c42)` under the plan `X, Y` (one line in the code):

```sql
CREATE TEMP TABLE "idx_0_<n>" AS
SELECT "rel_r"."c1" AS "c0", "rel_r"."c0" AS "c1"
FROM "rel_r"
WHERE "rel_r"."c2" = 42
```

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
  timed build can coexist. Then the join statement is prepared, as the last step of the build
  (decision 11). `Built` holds the connection, its table names and the prepared statement.
  Dropping a `Built` drops its tables, which happens after the timer stops.
- **`stream`.** The prepared statement is streamed as Arrow batches and their `num_rows` summed.
  `iteration` samples therefore never include DuckDB's planning; `build` and each `end_to_end`
  sample include it once.
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

- **Surface.**
  - `Metric::Materialize`, `--metrics materialize` on `bench run` and `bench join`. Off by default,
    like `end-to-end`. Criterion function `materialize`.
  - `bench probe`, which has no `--metrics`, takes a `--materialize` switch that adds a single-run
    `materialize` value.
  - `bench ds` rejects the metric with "bench ds has no join, so it cannot measure materialize".
- **kermit cells.** A new library function `kermit::db::execute_assignments`, beside `execute`,
  lends every variable of the executed query in canonical order. It slices no head and applies
  multiplicity as `execute` does. The rows are pushed into a `Tuples` whose arity is
  `Prepared::assignment_width()`, the variable count.
- **Shared sort and count.** `kermit::db::materialize_count(rows: Tuples) -> (Tuples, u64)` runs
  `Tuples::sort`, then counts adjacent-distinct rows. kermit and DuckDB cells both call it, so it
  lives in the library.
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
- **`--engine duckdb,nemo`** adds engine cells beside the `-i`/`-a` cells.
  - On `bench run` and `bench probe`, `-i`/`-a` are required only without `--engine`.
  - On `bench join` they stay required, because its `QueryArgs` is shared with `kermit join`,
    which has no `--engine`. Engines there are additive.
  
  Naming an engine whose feature was not compiled in is a usage error naming the feature.
- **`--engine-threads N`** sets DuckDB's thread count: default 1, range
  `1..=available_parallelism()`, and outside it a usage error naming the range. `--ds-*` flags
  apply to kermit cells only.
- **`--materialize-max-bytes B`** sets the guard's limit in bytes: default 4 GiB
  (4294967296), range 1 MiB to 1 TiB, and outside it a usage error naming the range.
- **Criterion group** is `{prefix}/{workload}/{query}/{engine segment}`, e.g.
  `run/…/q0007/duckdb-t1`.
  `check_sweep_group_directories`' 64-byte clash check covers engine cells.
- **`bench probe`:** engine cells get the single-run `build`, `iteration` (streaming engines only),
  and, with `--materialize`, `materialize`.
- **Stderr notes, exactly as tests match them:**
  - `note: {label} does not stream, so it skips iteration and end-to-end` (Nemo);
  - `note: {label} reports no space` (DuckDB);
  - `note: skipping materialize for {benchmark}/{query} on {label}: {bytes} bytes over --materialize-max-bytes {limit}`.
  
  `{label}` is `Engine::label`, and kermit cells use `{ds}/{algo}`. A skipped `materialize` also
  adds a metadata line labelled `materialize` with the value `skipped: {bytes} bytes over {limit}`.
- **Feature-missing error:** `--engine {name} needs the engine-{name} feature (build with --features engine-{name})`.

**Report schema v8:**
- `system` on every report (`kermit`, `duckdb`, `nemo`);
- `engine_version`, plus `engine_threads` (DuckDB), on engine rows;
- engine rows omit `data_structure`, `algorithm` and every `ds_*` axis;
- the new function `materialize`.

The bump is not for a metric: none changes meaning, so v7 and v8 load together. It is because
every v7 run report carries `data_structure` and `algorithm`, and kermit-lab relies on that (one
back-fill keys on `algorithm` being present), so an old kermit-lab would misread engine rows.
`REPORT_SCHEMA_VERSION` and Python's `SCHEMA_VERSION` bump together; the existing `cargo test`
keeps them in step.

**kermit-lab:**
- back-fill `system = "kermit"` on rows that lack it;
- recognise `materialize` as a phase;
- a cell label (`system` for engines, `data_structure` / `algorithm` for kermit) used by the colour
  and facet defaults and by `speedup_table`;
- a preset `kl.engines()` that plots only the like-for-like pairs from the Summary;
- the six structure figures (`scaling`, `bar_time`, `bar_space`, `tradeoff`, `dist`,
  `bar_queries`) draw kermit rows only, and say so in their docstrings. Engines appear in
  `kl.engines()` and through `kl.plot`'s cell-label defaults, so a generic figure never puts an
  engine beside kermit on a metric that is not a like-for-like pair (the owner's decision,
  2026-11-02, during drafting: the alternative drew engine rows in every figure).

## Testing

- **Shared shapes.** `all_orders.rs`'s private `Shape`, `shapes()`, `random_data` and catalog
  helpers move to `kermit/tests/common/shapes.rs`. `all_orders.rs` and both oracle tests below use
  them, so every test reads the same 16 suite patterns plus the constant, repeated-variable,
  placeholder and all-constant cases.
- **Engine oracle (`kermit/tests/engine_oracle.rs`, built only with an engine feature).**
  - Runs the shared shapes over seeded random relations.
  - Per engine: the count (DuckDB `stream`; both engines' `materialize`) and the contents
    (DuckDB's collected rows; Nemo's result rows through `table_row_iterator_inmemory`), sorted,
    against the oracle's sorted full assignments.
  - Contents matter beyond counts: on small fixtures a wrong join condition can still produce the
    right count.
- **`materialize` on kermit's cells (`kermit/tests/materialize_oracle.rs`).** The same oracle check
  for TreeTrie, ColumnTrie and HashTrie, through `execute_assignments` and `materialize_count`.
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
  - engine rows carry the v8 axes;
  - the Nemo version constant matches `kermit/Cargo.toml`.
- **kermit-lab pytest:** v7 and v8 load together, `system` is back-filled, and `kl.engines()`
  selects only the like-for-like pairs.
- **CI.**
  - A new `engines` job runs clippy, rustdoc, the unit tests and the oracle with
    `--features engine-duckdb-bundled,engine-nemo`. CI has no Nix, so it compiles DuckDB's
    amalgamation. The existing jobs build without the features and are unchanged.
  - In the dev shell, every engine step's Verify line uses
    `--features engine-duckdb,engine-nemo`, which links Nix's `libduckdb` and compiles no C++
    (decision 10).
  - Sweep binaries are built in the dev shell with `engine-duckdb,engine-nemo`. Each report's
    `system` and `engine_version` axes say what ran.

## Premises

Verified on 2026-11-02 by a scratch crate outside the repo.
- **Setup:** it depends on `nemo-physical` at the pinned revision, `duckdb ~1.4` (linked against
  Nix's `libduckdb` 1.4.4) and the `kermit` crate itself, so the whole dependency graph resolves
  together.
- **Workload:** a triangle over 3,001 directed edges, with ids up to about 2^62, checked against a
  nested-loop oracle (1,249 walks).

1. **DuckDB: holds.**
   - The appender writes `UBIGINT`; `query_arrow` returns `UInt64` columns whose rows equal the
     oracle's; `stream_arrow` works.
   - The linked library reports `v1.4.4` (the crate is 1.4.5).
   - A prepared statement reuses its plan: a 10-way chain over one-row tables re-executed in
     1.26 ms, against 2.06 ms for prepare plus execute (mean of 200).
   - The 1.26 ms that remains is DuckDB's per-execution cost (executor and pipeline setup), and it
     belongs in the measurement. On the smallest queries (LUBM's `iteration` is about 6 µs in
     kermit), DuckDB's numbers will mostly be that fixed cost, and the analysis says so.
2. **Nemo under `futures::executor::block_on`: holds.** `load_trie_into_memory` (0.40 ms) and
   `execute_plan` (0.82 ms) complete with no Tokio runtime.
3. **Nemo's threads: single-threaded.** The process stays at one thread throughout load and
   execute, and `nemo-physical` contains no threading code. Nemo rows record no `engine_threads`.
4. **Nemo integers: hold.** `AnyDataValue::new_integer_from_u64` and `DataValue::to_u64` round-trip
   every id, including the one near 2^62. Nemo's triangle equals the oracle row for row. Its
   loader deduplicates (3,002 rows in, 3,001 stored), as set semantics requires.
5. **Nemo's `table_size_bytes`: holds, read after loading.**
   - Before loading, it returns the source's own `ByteSized` figure (48,032 B, the provider's
     3,002 × 16).
   - After loading, the source has been consumed and it returns the trie's figure (35,938 B).
   - So `space` is read after `build`, as § Nemo specifies. It is Nemo's own accounting, so beside
     kermit's `HeapSize` it supports a sanity check, not a ratio claim.
6. **Builds: hold on `nightly-2026-11-02` (rustc `3e92e1a2d`, 2026-11-01), and fail on the flake's
   `nightly-2026-10-31`** (decision 9).
   - With Nix's `libduckdb` (no C++), the whole graph builds in 5 min 57 s at two jobs.
   - Nemo's `reqwest` 0.13 unifies with the 0.13.5 kermit already uses (through `kermit-bench`),
     and Tokio is already in kermit's graph, so `engine-nemo` adds no second HTTP or TLS stack.

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

0. The toolchain: lock the flake's `rust-overlay` at a nightly of 2026-11-02 or later
   (`nix flake update rust-overlay`), and fix any clippy lint the new nightly raises (decision 9).
   Also export `DUCKDB_LIB_DIR` and `DUCKDB_INCLUDE_DIR` from the dev shell (decision 10).
1. `materialize` on kermit's cells: `execute_assignments`, `materialize_count`, `Metric`, the guard,
   `--verify`. It is useful on its own.
2. The `Cell` refactor of `run_benchmark` (`FamilyCell`; no behaviour change).
3. `Engine`, `EngineSubject`, `EngineCell`, the SQL translator and DuckDB, with the oracle and the
   `engines` CI job.
4. Nemo, with the `Catalog` `Arc` change.
5. Report v8 and kermit-lab (`system`, `kl.engines()`).
6. Documentation.

Each step leaves the gate green. Steps 1 and 2 change nothing in an existing report except adding
`materialize` where it is asked for.

## Acceptance

1. With both features, the engine oracle passes for DuckDB and Nemo on counts and contents, in the
   `engines` job.
2. `bench run triangle -i all -a all --engine duckdb,nemo --metrics build iteration space
   materialize end-to-end --verify` writes a v8 report with three kermit cells and two engine
   cells. It skips Nemo's `iteration` and `end_to_end` with notes, and every count is verified.
3. `bench probe … -i all -a all --engine duckdb,nemo --materialize` passes on one query from every
   benchmark family (a probe always verifies, so it takes no `--verify`). Every cache is read
   without writing (no `--force`, no `bench gen`).
   - From the shared cache: `triangle`, `lubm-reference -q q1`, `watdiv-stress-100-test-1 -q q0001`
     and `oxford-uniform-s3 -q triangle`. The last two declare no `expected`, so their counts are
     compared across the five cells (the report's `answer_rows`) instead: all five agree on an
     input without duplicate rows, and otherwise the cells of each semantics agree. `R` and `S` of
     `oxford-uniform-s3` each hold one duplicate row, so its bag cells (`HashTrie`, DuckDB) count
     7433 and its set cells (`TreeTrie`, `ColumnTrie`, Nemo) 7430.
   - From the 2026-10-24 sweep's private cache, with
     `XDG_CACHE_HOME=/tb/Source/Academia/kermit-bench-runs/full-sweep-2026-10-24/cache`:
     `sw-snap-amazon-pre -q triangle`, `sw-h2o-pre -q q1`, `sw-job-1-pre -q 1a`.
4. kermit-lab loads v7 and v8 reports together, and `kl.engines()` renders.
5. A default `cargo build` compiles neither engine, and the existing CI jobs are unchanged.
6. The documentation below is written.

## Documentation

- `docs/engines/TEMPLATE.md`, `docs/engines/duckdb.md` and `docs/engines/nemo.md`. Each covers its
  phase mapping, fairness caveats, version pin and how to move the pin.
- `CLAUDE.md`:
  - a new recipe, "Adding an external engine": module, `Engine` impl, feature, CLI value, oracle
    cell, doc;
  - `kermit::engine` in the architecture section;
  - gotchas: features off by default, full-assignment `materialize`, schema v8, the like-for-like
    pairs.
- `docs/specs/bench-report-schema.md`: v8.
- `ARCHITECTURE.md`: `kermit::engine` among the `kermit` crate's modules.
- `BENCHMARKING.md`: running engines, and which pairs compare.

## Amendment 1 (2026-11-05): the audit's fixes

An audit of the integration found nothing that gives a wrong answer, and the user had all of its
findings fixed:

1. **DuckDB spills on disk.** `std::env::temp_dir()` is a 15 GiB tmpfs on the sweep host, so a
   spill past `memory_limit` was memory charged to the run's own cap, and a SIGKILLed run left it
   resident. `DuckDbSettings::spill_parent` (default `<platform cache>/kermit/duckdb.tmp`, beside
   the benchmark cache and the JOB raw cache's own `duckdb.tmp`) replaces it. Each spill directory
   holds a `kermit.lock`, locked while its connection lives; making one first removes every sibling
   whose lock can be taken (the OS frees a dead process's locks). A name already taken is skipped,
   never deleted.
2. **The `engines` CI job runs on pushes to master**, in its own workflow
   (`.github/workflows/engines.yml`), since work lands on master by direct push; it is separate so
   that Release, which follows CI, does not wait on its cold DuckDB build.
3. **`Engine::build` takes no catalog**: neither engine read it.
4. **The preflight keeps its load.** It runs inside `Cell::subject` (`Cell::preflight` is gone),
   drops only its trial build, and the subject keeps the load, so each query is loaded into an
   engine once.
5. **Capabilities are stated once.** `kermit::engine::EngineKind` (the `--engine` `ValueEnum`,
   formerly the binary's `EngineSelector`) gives each engine's `Capabilities`: `name`, `threaded`,
   `streams`, `keeps_duplicates`, `materialize_mutates`, `has_space`. An `Engine` impl takes them as
   `CAPABILITIES`, in place of `NAME`, `STREAMS`, `KEEPS_DUPLICATES`, `MATERIALIZE_MUTATES`,
   `HAS_SPACE` and `label`; `threads` returns a `usize` (1 for an engine that takes none). The
   binary's `EngineChoice` labels and plans from the same capabilities (`Capabilities::label`,
   `Metric::measured_by`), so the hand-written copies and their feature-gated pin tests are gone.
6. **DuckDB's Arrow copy is a documented caveat.** `stream` converts every result chunk into Arrow
   arrays before counting it, a copy kermit's `iteration` does not make; `docs/engines/duckdb.md`
   says so. It is not measured separately.
7. **Smaller cleanups.** One `EngineError::new` and one `engine::indexed_relations` serve both
   engines, and a unit test pins `NEMO_PHYSICAL_VERSION` (with `NEMO_REV`) against `Cargo.lock`.
