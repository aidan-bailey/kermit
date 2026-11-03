# External Engines Implementation Plan

> **Supervised plan.** `/flow-draft` writes the stories below the marker from this skeleton,
> then `/flow-supervise` runs them story by story. Steps use checkbox (`- [ ]`) syntax for
> tracking.

**Goal:** kermit runs DuckDB and Nemo in-process on its own prepared queries, measured by its own
measure layer, beside its three structure cells. Every cell gains an opt-in `materialize` metric,
and reports move to schema v7.

**Architecture:**
- **Prerequisite (Story 1).** The flake's nightly moves to 2026-11-02 or later, because
  `nemo-physical` hits a rustc regression on 2026-10-31. The dev shell exports Nix's `libduckdb`.
- **`materialize` on kermit's own cells (Stories 2–3).** The library gains `execute_assignments`,
  `materialize_count` and `check_multiplicity`. The binary gains the metric, the memory guard and
  the CLI flags.
- **The seams (Stories 4–6):**
  - `run_benchmark` becomes generic over a `Cell`, with `FamilyCell` wrapping today's families
    unchanged;
  - `Catalog` shares its tuples through `Arc`;
  - the library gains `kermit::engine`: the `Engine` trait and the shared SQL translator.
- **The engines (Stories 7–8).** DuckDB, then Nemo, each behind its cargo feature, each checked by
  the engine oracle, each in the `engines` CI job.
- **Reports and the binary's engine cells (Stories 9–10).** Schema v7 with the `system` axis, then
  `EngineSubject`, `EngineCell`, `--engine` and `--engine-threads`.
- **kermit-lab and docs (Stories 11–12).** The last story runs the acceptance probes.

**Tech Stack:** Rust nightly in the nix dev shell (`rust-toolchain.toml`; the profile's prefix is
`nix develop --command`); Criterion 0.8.2; `duckdb` crate `~1.4` (DuckDB 1.4.x, linked against
Nix's 1.4.4 in the dev shell, bundled in CI); `nemo-physical` at git revision
`04f0962b91edc1bbe73e6f65f08d41d1ee491980`; `futures` and `async-trait`; kermit-lab on Python 3.13
under uv (pandas, pytest).

**Spec:** `docs/specs/2026-11-02-external-engines-design.md`. Every decision it records is
settled, including the premise results (§ Premises) and decisions 9–13.

**Branch:** loom branch `aidanb/state-of-kermit`. Never switch branches, amend or push. The change
lands on master once, through the owner, after the final gate.

**Gates.** The profile's gates build without cargo features. Every story that touches engine code
names its `--features` in its Verify line. Dev-shell builds use `engine-duckdb`, which links Nix's
`libduckdb` and compiles no C++; `engine-duckdb-bundled` is for CI only. Never build the bundled
amalgamation on this host: its compiler process needs several GB, and `earlyoom` prefers to kill
`cc1plus`.

## File structure

| File | Change | Stories |
|---|---|---|
| `flake.lock` | `rust-overlay` locked at a nightly of 2026-11-02 or later | 1 |
| `flake.nix` | dev shell exports `DUCKDB_LIB_DIR`, `DUCKDB_INCLUDE_DIR` | 1 |
| `kermit/src/db/lifecycle.rs` | `execute_assignments`, `Prepared::assignment_width`, `materialize_count`, `check_multiplicity` (`build_with` calls it) | 2 |
| `kermit/src/db.rs` | re-export the new functions | 2 |
| `kermit/tests/common/shapes.rs` | create: `Shape`, `shapes()`, `random_data`, `catalog_for`, `catalog_at`, `full_query`, `DOMAIN`, `SEEDS`, moved from `all_orders.rs` | 2 |
| `kermit/tests/common/mod.rs` | `pub mod shapes;` | 2 |
| `kermit/tests/all_orders.rs` | uses `common::shapes` | 2 |
| `kermit/tests/materialize_oracle.rs` | create: `materialize` on the three structures against the oracle | 2 |
| `kermit/src/bench/mod.rs` | `Metric::Materialize`; `mod cell;` and `mod engine;` | 3, 4, 10 |
| `kermit/src/bench/measure.rs` | `Subject` GAT, `STREAMS`, `MATERIALIZE_MUTATES`, `materialize`, `Materialized`; the `materialize` timing | 3 |
| `kermit/src/bench/run.rs` | guard and `--verify` for `materialize`; `RunSettings::materialize_max_bytes`; then generic over `Cell`; then `SweepCell` | 3, 4, 10 |
| `kermit/src/bench/ds.rs` | rejects `materialize`; `RelationSubject` follows the new `Subject`; then the `system` axis | 3, 9 |
| `kermit/src/main.rs` | `MaterializeArgs`, probe's `--materialize`; then `EngineChoices` flattened into the run, probe and bench-join commands | 3, 10 |
| `kermit/tests/cli_materialize.rs` | create: `materialize` CLI behaviour | 3 |
| `kermit/src/execution.rs` | `ExecutionFamily::execute_assignments`, implemented by `TrieLftj` and `HashHtj` | 3 |
| `kermit/src/bench/cell.rs` | create: `Cell`, `FamilyCell`; then the `system` axis | 4, 9 |
| `kermit/src/db/catalog.rs` | `Arc<Tuples>` per entry; `shared_tuples` | 5 |
| `kermit/src/engine/mod.rs` | create: `Engine`, `EngineError`, `Unsupported` | 6 |
| `kermit/src/engine/sql.rs` | create: the SQL translator | 6 |
| `kermit/src/lib.rs` | `pub mod engine;` | 6 |
| `kermit/src/engine/duckdb.rs` | create: `DuckDb` | 7 |
| `kermit/build.rs` | create: rpath to `DUCKDB_LIB_DIR` | 7 |
| `kermit/Cargo.toml` | features `engine-duckdb`, `engine-duckdb-bundled`, then `engine-nemo`, and their dependencies | 7, 8 |
| `Cargo.toml` | `[profile.dev.package.libduckdb-sys] opt-level = 3` | 7 |
| `kermit/tests/engine_oracle.rs` | create with DuckDB's cells; then Nemo's | 7, 8 |
| `.github/workflows/pr.yml` | the `engines` job; then Nemo joins it | 7, 8 |
| `kermit/src/engine/nemo.rs` | create: `Nemo`, `ViewProvider` | 8 |
| `kermit/src/bench_report.rs` | `REPORT_SCHEMA_VERSION = 7` | 9 |
| `python/kermit-lab/kermit_lab/__init__.py` | `SCHEMA_VERSION = 7` | 9 |
| `kermit/tests/cli_bench_probe.rs`, `kermit/tests/cli_join_tests.rs`, `python/kermit-lab/tests/test_contract.py` | the pinned `schema_version` moves to 7 | 9 |
| `kermit/src/bench/engine.rs` | create: `EngineSubject`, `EngineCell` | 10 |
| `kermit/src/options.rs` | `EngineSelector`, `EngineChoices`, `EngineChoice` | 10 |
| `kermit/tests/cli_engines.rs` | create: engine CLI behaviour | 10 |
| `python/kermit-lab/kermit_lab/defaults.py` | back-fill `system` | 11 |
| `python/kermit-lab/kermit_lab/presets.py` | `materialize` phase; `engines()` | 11 |
| `python/kermit-lab/kermit_lab/analysis.py` | cell label | 11 |
| `python/kermit-lab/kermit_lab/frame.py`, `python/kermit-lab/kermit_lab/loader.py` | the v7 axes kept; `materialize` in `TIME_PHASES` | 11 |
| `python/kermit-lab/tests/test_loader.py`, `python/kermit-lab/tests/test_presets.py`, `python/kermit-lab/tests/test_public_api.py` | `TIME_PHASES`, the seek axis's `materialize` phase, `engines` | 11 |
| `python/kermit-lab/tests/test_engines.py` | create | 11 |
| `docs/engines/TEMPLATE.md`, `docs/engines/duckdb.md`, `docs/engines/nemo.md` | create | 12 |
| `CLAUDE.md`, `ARCHITECTURE.md`, `BENCHMARKING.md`, `docs/specs/bench-report-schema.md`, `README.md`, `USAGE.md`, `kermit/README.md`, `python/kermit-lab/README.md` | the recipe, the module, v7, the like-for-like pairs, the new metric and flags | 12 |

## Names contract

Every name, type and signature one story defines for another. After this skeleton, only
`/flow-draft`'s manager edits this section.

**Library, `kermit::db` (Story 2).** In `kermit/src/db/lifecycle.rs`, re-exported from
`kermit/src/db.rs`'s `pub use` block.

```rust
impl Prepared {
    /// The executed query's variable count, which is the width of `execute_assignments`' rows.
    pub fn assignment_width(&self) -> usize;
}
/// `execute`, lending every variable of the executed query in canonical order (the order the
/// executors emit: head variables first), not only the head prefix. Multiplicity, the zero
/// check, and a checks-only query (`emit(&[])` `multiplicity` times) behave as in `execute`.
pub fn execute_assignments<R, JA>(prepared: &Prepared, indexes: &Indexes<R>, emit: impl FnMut(&[usize]))
where R: JoinIterable, JA: JoinAlgo<R>;
/// Sorts `rows` (`Tuples::sort`) and counts adjacent-distinct rows. Returns the sorted batch and the count.
pub fn materialize_count(rows: Tuples) -> (Tuples, u64);
/// The lifecycle's check rule. 1 with no checks; 0 when any check's view counts no row; otherwise
/// the product of the checks' counts, each clamped to 1 unless `keeps_duplicates`.
/// `build_with` uses it with `R::KEEPS_DUPLICATES`.
pub fn check_multiplicity(catalog: &Catalog, prepared: &Prepared, keeps_duplicates: bool) -> usize;
```

**Test helpers, `kermit/tests/common/shapes.rs` (Story 2).** Moved from `all_orders.rs`, behaviour
unchanged, plus `full_query`.

```rust
pub const DOMAIN: usize = 3;
pub const SEEDS: std::ops::Range<u64> = 0..6;
pub struct Shape { pub name: &'static str, pub query: &'static str, pub arities: &'static [usize] }
pub fn shapes() -> Vec<Shape>;
pub fn random_data(rng: &mut Lcg, arities: &[usize]) -> Relations;           // Lcg, Relations: common::oracle
pub fn catalog_for(relations: &Relations) -> Catalog;
pub fn catalog_at(relations: &Relations, level: StatisticsLevel) -> Catalog;
/// `query` after `rewrite_placeholders`, with every variable in the head, in `prepared`'s canonical
/// order (`canonical_names(prepared.query())`): the full conjunctive query, whose oracle rows
/// compare with `execute_assignments`' and every engine's rows as they come, with no permutation.
pub fn full_query(query: &JoinQuery, prepared: &Prepared) -> JoinQuery;
```

**Canonical order**, here and in `kermit::engine`, is `canonical_names(prepared.query())`: the
executed query's order, head variables first. For a body-only variable (a `_`'s `K<n>`, or one
missing from the head) it can differ from the source query's canonical order, because the executed
atoms list their variables in plan order. `execute_assignments`, `join_select`, `Engine::rows` and
`full_query`'s head all use it.

How the oracles compare (the fixtures hold duplicate rows):
- DuckDB's `stream` rows compare with `oracle::<HashTrieSip>(&relations, &full_query(q, &prepared))`,
  which has bag semantics. `prepared` is the query prepared under `TrieLevels::All`, as engines run it.
- Every cell's `materialize` count, and Nemo's rows, compare with the **distinct** rows of that
  oracle. Distinct full assignments are the same set under set and bag semantics.

**Binary, the measure layer (Story 3).** In `kermit/src/bench/measure.rs`:

```rust
pub(crate) trait Subject {
    type Built<'s> where Self: 's;
    const EXERCISE_MUTATES: bool;
    /// Whether `exercise` streams. When false, the describe step counts through `materialize`.
    const STREAMS: bool;
    const MATERIALIZE_MUTATES: bool;
    fn build(&self) -> Self::Built<'_>;
    fn heap_bytes(&self, built: &Self::Built<'_>) -> Vec<usize>;
    fn exercise(&self, built: &Self::Built<'_>) -> u64;
    /// `None` for a subject with no join (`RelationSubject`).
    fn materialize(&self, built: &Self::Built<'_>) -> Option<Materialized>;
}
pub(crate) struct Materialized {
    pub(crate) rows: u64,
    /// The sorted batch (kermit and DuckDB), returned so that Criterion frees it after the timer.
    pub(crate) kept: Option<Tuples>,
}
```

In `kermit/src/bench/mod.rs`: `Metric::Materialize`, whose clap value is `materialize`.
`Metric::is_timed` is true for it.

In `kermit/src/bench/run.rs`:
- `RunSettings` gains `pub materialize_max_bytes: u64`.
- `pub(crate) fn materialize_bytes(rows: u64, width: usize) -> u64` returns `rows × width × 8`,
  saturating.

In `kermit/src/main.rs`:
- `#[derive(Args)] struct MaterializeArgs { materialize_max_bytes: u64 }`. The flag is
  `--materialize-max-bytes <BYTES>`, default 4294967296, range 1048576..=1099511627776; outside it,
  the error is `--materialize-max-bytes must be between 1048576 (1 MiB) and 1099511627776 (1 TiB)`.
  It is flattened into `bench run`, `bench join` and `bench probe`.
- `bench probe` gains `--materialize`, a switch that adds a single-run `materialize` value.

Messages tests match (spec § CLI, reports and kermit-lab):
- `note: skipping materialize for {benchmark}/{query} on {label}: {bytes} bytes over --materialize-max-bytes {limit}`;
- the metadata line `materialize`, with the value `skipped: {bytes} bytes over {limit}`;
- `bench ds has no join, so it cannot measure materialize`;
- `warning: {label} does not stream and {benchmark}/{query} has no expected count, so materialize runs without the --materialize-max-bytes guard`.

`{label}` is `{ds}/{algo}` for kermit cells and `Engine::label` for engines.

Rules for `materialize` (seams decision, 2026-11-02):
- **One `materialize` per build on a mutating subject.** For a `!S::STREAMS` subject the describe
  step's count is its one `materialize`, so `Described::materialize_once` returns that run's rows
  and time and never runs it again. For a streaming `MATERIALIZE_MUTATES` subject (lazy HashTrie),
  `materialize_once` runs on a fresh build.
- **Guard order.** For a streaming subject, the guard reads `expected`, else the describe step's
  streamed count, and runs before any `materialize`. For a `!S::STREAMS` subject it runs before the
  describe step, from `expected`: over the limit, nothing materialises (no describe-step count, no
  preflight count, no `materialize`; the note and the metadata line as above); without `expected`,
  the warning above is printed once and everything runs unguarded. Story 3 writes the guard in
  terms of the subject type's `STREAMS`, so Story 4's generic `run_benchmark` keeps it as it is.
- **The guard's one function** (Story 3, `kermit/src/bench/run.rs`, private):
  `fn materialize_guard(requested, streams, rows, width, limit) -> MaterializeVerdict`, with the
  verdicts `NotAsked`, `Run`, `Skip`, `SkipUncounted`, `AfterStreamedCount` and `Unguarded`, and
  `MaterializeVerdict::counts()`, false only for `SkipUncounted`. For a streaming subject,
  `requested` is whether the `materialize` metric is asked for. For a non-streaming subject it is
  whether anything would materialise: the `materialize` metric, `--verify` with `expected`, or a
  single run (`bench probe`), so that a `--verify`-only run over the limit materialises nothing
  either (owner, 2026-11-02). Story 4 binds `streams` as `<C::Subject<'_> as Subject>::STREAMS`
  (keeping `Subject` imported) and passes Story 3's already-bound `guard.counts()` to
  `cell.preflight`; it keeps no second copy of the decision.

Story 3 also adds, for later stories:
- `ExecutionFamily::execute_assignments` in `kermit/src/execution.rs` (required, implemented by
  `TrieLftj` and `HashHtj`), which `QuerySubject::materialize` calls; `QuerySubject`'s
  `MATERIALIZE_MUTATES` is `F::JOIN_MUTATES`.
- `Described::materialize_once` and `Described::materialize_time`. `Described::new` counts
  through `materialize` when `!S::STREAMS`, and `Described::measure` skips `iteration` and
  `end_to_end` for such a subject.
- `bench::ds::refuse_materialize`, called from `main.rs`.
- A named text Story 10 keeps: `bench probe`'s Criterion clash-check condition in `main.rs`,
  `!single_run && any(is_timed)`.

**Binary, `Cell` (Story 4).** In `kermit/src/bench/cell.rs`, with `mod cell;` in
`kermit/src/bench/mod.rs`:

```rust
pub(crate) trait Cell {
    type Subject<'a>: Subject where Self: 'a;
    /// For stderr notes and metadata lines: "{ds}/{algo}", or `Engine::label`.
    fn label(&self) -> String;
    /// The `data structure` and `algorithm` metadata lines (kermit), or `system` (engines).
    fn metadata(&self) -> Vec<MetadataLine>;
    /// Report axes this cell sets: `data_structure`, `algorithm`, `ds_*` (kermit; from Story 9 also
    /// `system`), or `system`, `engine_version`, `engine_threads` (engines).
    fn axes(&self) -> BTreeMap<String, serde_json::Value>;
    /// The Criterion group's last segments: "{ds}/{algo}", or `Engine::label`.
    fn group_segment(&self) -> String;
    fn trie_levels(&self) -> TrieLevels;
    fn supports(&self, metric: Metric) -> bool;
    /// Untimed and fallible, once per query, before the describe step. A no-op for kermit cells.
    /// `count` is false when the memory guard skips `materialize` for a non-streaming subject;
    /// the engine then runs `load` and `build` only.
    fn preflight(&self, catalog: &Catalog, prepared: &Prepared, count: bool) -> anyhow::Result<()>;
    fn subject<'a>(&'a self, catalog: &'a Catalog, prepared: &'a Prepared) -> anyhow::Result<Self::Subject<'a>>;
    /// Axes read from the query's subject, merged into that query's report axes: `engine_version`
    /// (engines, from `E::version` of the subject's `Loaded`). Provided: empty, for kermit cells.
    fn subject_axes(&self, subject: &Self::Subject<'_>) -> BTreeMap<String, serde_json::Value> { BTreeMap::new() }
    /// Whether `Subject::heap_bytes` gives one figure per `IndexSpec`, so each `index` metadata line
    /// pairs a spec with its bytes. Provided: true. `EngineCell` sets false and gets no `index` lines.
    const PER_INDEX_BYTES: bool = true;
}
pub(crate) struct FamilyCell<'f, F: ExecutionFamily>(pub(crate) &'f F);
// run.rs: run_benchmark<F: ExecutionFamily> becomes
fn run_benchmark<C: Cell>(cell: &C, workload: &PreparedWorkload, settings: RunSettings<'_>, sink: &mut ReportSink) -> anyhow::Result<()>;
```

`dispatch_run_bench` wraps each family in `FamilyCell`. Every existing report, stderr line and
Criterion group is byte-for-byte unchanged.

Per query, `run_benchmark` calls `preflight`, then `subject`, then `subject_axes` (merged into that
query's axes after `axes()`), and emits the `index` lines only when `C::PER_INDEX_BYTES`.
`run_benchmark` filters `settings.metrics` through `Cell::supports` (a no-op for kermit cells). It
prints no engine skip notes: Story 10's dispatch prints them, once per engine cell and benchmark,
before it calls `run_benchmark`, and does not filter.

Texts a later story quotes, which a fixer of the earlier story leaves unchanged:
- Story 4's `cell.rs`: the `pub(crate) trait Cell` declaration line, the doc comment on `axes`, and
  `FamilyCell::axes` (Story 9 edits them).
- Story 3's `run.rs` lines `// The cell as notes name it.` and
  `let label = format!("{ds_name}/{algo_name}");` (Story 4's Edit D quotes them, and replaces the
  second with `let label = cell.label();`, so one `label` is bound).
- Story 3's `run_bench_join` signature, whose last parameter is `materialize_max_bytes: u64`, and
  its call in `main` (Story 10 appends `engines` after it in both).
- Story 4's `run.rs`: `criterion_group_name`, `group_with_segment`, `execution_segment` and the test
  `a_kermit_cells_group_is_prefix_…` (Story 10 edits them). `run_benchmark` names its group through
  `group_with_segment(…, cell.group_segment())` (Story 4's Edit H), which Story 10 leaves alone.

**Library, `Catalog` (Story 5).** `Entry::tuples` becomes `Arc<Tuples>`. `tuples(&self, name)`
keeps returning `Option<&Tuples>`, and a new
`pub fn shared_tuples(&self, name: &str) -> Option<Arc<Tuples>>` is added.

**Library, `kermit::engine` (Story 6).** `kermit/src/engine/mod.rs`, with `pub mod engine;` in
`kermit/src/lib.rs`:

```rust
pub trait Engine {
    const NAME: &'static str;            // the `system` axis: "duckdb", "nemo"
    const STREAMS: bool;
    const KEEPS_DUPLICATES: bool;
    const MATERIALIZE_MUTATES: bool;
    const HAS_SPACE: bool;
    type Settings: Default + Clone + std::fmt::Debug;
    type Loaded;
    type Built<'l> where Self: 'l;
    fn label(settings: &Self::Settings) -> String;       // "duckdb-t{threads}", "nemo"
    fn threads(settings: &Self::Settings) -> Option<usize>;
    fn version(loaded: &Self::Loaded) -> String;
    fn supports(prepared: &Prepared) -> Result<(), Unsupported>;
    fn load(settings: &Self::Settings, catalog: &Catalog, prepared: &Prepared) -> Result<Self::Loaded, EngineError>;
    fn build<'l>(loaded: &'l Self::Loaded, catalog: &Catalog, prepared: &Prepared) -> Result<Self::Built<'l>, EngineError>
    where Self: 'l;
    fn stream(built: &Self::Built<'_>) -> Result<u64, EngineError>;         // only if STREAMS
    /// The distinct-row count, and the sorted batch when the engine copies its rows into `Tuples`
    /// (DuckDB: `Some`, from `materialize_count`; Nemo: `None`), returned so that Criterion frees it
    /// after the timer stops. `EngineSubject` maps it onto `Materialized { rows, kept }`.
    fn materialize(built: &Self::Built<'_>) -> Result<(u64, Option<Tuples>), EngineError>;
    fn space(built: &Self::Built<'_>) -> Option<usize>;                     // Some only if HAS_SPACE
    /// The full assignments, in canonical order, for tests; never timed. Nemo runs its plan first if
    /// `materialize` has not.
    fn rows(built: &Self::Built<'_>) -> Result<Vec<Vec<usize>>, EngineError>;
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported { pub reason: String }
#[derive(Debug)]
pub struct EngineError { pub engine: &'static str, pub message: String } // Display: "{engine}: {message}"; impl std::error::Error
```

`build` takes the catalog as well, because Nemo's providers stream from `Catalog::shared_tuples`;
`materialize` returns the sorted batch beside its count (spec § The `materialize` metric, Timing),
and `build` carries `where Self: 'l` for the `Built<'l>` GAT. The spec's sketch is otherwise
unchanged. `Unsupported` also implements `Display` (`unsupported: {reason}`) and `std::error::Error`.
`sql.rs` may add helpers of its own (Story 6 adds `relation_table_name` and `column`).

`kermit/src/engine/sql.rs`:

```rust
pub fn quote_identifier(name: &str) -> String;                         // "\"" + name with '"' doubled + "\""
pub fn relation_table(relation: &str) -> String;                       // quote_identifier("rel_{relation}")
pub fn spec_table(index: usize, build: u64) -> String;                 // quote_identifier("idx_{index}_{build}")
/// CREATE TEMP TABLE spec_table(index, build) AS SELECT "c{col}"… FROM relation_table(..) WHERE <filter>;
/// the selected columns are named "c0".. in output order.
pub fn create_spec_table(spec: &IndexSpec, index: usize, build: u64) -> String;
/// The join over the spec tables (spec § Translation): aliases "a{i}", every variable selected once
/// in canonical order, later bindings as equalities, CROSS JOIN for an atom sharing no variable.
pub fn join_select(prepared: &Prepared, build: u64) -> String;
```

**Library, the engines (Stories 7–8):**
- **`kermit::engine::duckdb`,** behind `engine-duckdb`:
  `pub struct DuckDb; pub struct DuckDbSettings { pub threads: usize }` (`Default` gives
  threads 1), plus `pub struct DuckDbLoaded` and `pub struct DuckDbBuilt<'l>`. The constants are
  `NAME "duckdb"`, `STREAMS true`, `KEEPS_DUPLICATES true`, `MATERIALIZE_MUTATES false` and
  `HAS_SPACE false`. Its connection settings are `memory_limit = '8GB'`, plus a private
  `temp_directory` at `kermit-duckdb-<pid>-<n>`, removed on drop.
- **`kermit::engine::nemo`,** behind `engine-nemo`:
  `pub struct Nemo; pub struct NemoLoaded; pub struct NemoBuilt`. `Settings = ()`. The constants
  are `NAME "nemo"`, `STREAMS false`, `KEEPS_DUPLICATES false`, `MATERIALIZE_MUTATES true` and
  `HAS_SPACE true`. It also exports `pub const NEMO_REV: &str =
  "04f0962b91edc1bbe73e6f65f08d41d1ee491980";` and
  `pub const NEMO_PHYSICAL_VERSION: &str = "0.10.2-dev";`. `version()` returns
  `"nemo-physical {NEMO_PHYSICAL_VERSION} @ {first 12 of NEMO_REV}"`, and a unit test pins
  `NEMO_REV` against the `rev` in `kermit/Cargo.toml`.

`Cargo.lock` is gitignored (`.gitignore:8`): no story commits it, and dependency changes reach it
by building.

Texts a later story quotes, which a fixer of the earlier story leaves unchanged:
- Story 6's `kermit/src/engine/mod.rs`: the `pub mod sql;` line (Story 7 inserts before it).
- Story 7's texts that Story 8 edits: `kermit/Cargo.toml`'s last `[features]` line, its
  `[dependencies]` header and `duckdb = …` line; `mod.rs`'s `#[cfg(feature = "engine-duckdb")]
  pub mod duckdb;` and `pub mod sql;` lines; `engine_oracle.rs`'s `#![cfg(feature =
  "engine-duckdb")]` gate, the head of `struct Case`, `cases()`, `distinct_rows` and the `mod
  duckdb_cells {` line; `pr.yml`'s `ENGINE_FEATURES: engine-duckdb-bundled` line and the job's
  `name: Engines (DuckDB bundled)`.

Cargo features (`kermit/Cargo.toml`): `engine-duckdb`, `engine-duckdb-bundled`, `engine-nemo`.
`build.rs` fails a build that has `engine-duckdb` with neither `DUCKDB_LIB_DIR` nor bundling,
saying `engine-duckdb needs DUCKDB_LIB_DIR (the nix dev shell sets it) or the
engine-duckdb-bundled feature`. The CI job is called `engines`.

**Report v7 (Story 9).** `REPORT_SCHEMA_VERSION = 7`, and `SCHEMA_VERSION = 7` in kermit-lab.
- The axis keys are `system` (`"kermit"`, `"duckdb"`, `"nemo"`), `engine_version` and
  `engine_threads`.
- Every report gets `system`: `FamilyCell::axes` and `bench ds` set `"kermit"`.
- Engine rows omit `data_structure`, `algorithm` and every `ds_*` axis.

**Binary, engine cells (Story 10).**

```rust
// kermit/src/options.rs
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum EngineSelector { Duckdb, Nemo }            // clap values "duckdb", "nemo"
#[derive(Args, Clone, Debug, Default)]
pub(crate) struct EngineChoices {
    /// --engine duckdb,nemo (value_delimiter ',')
    pub(crate) engine: Vec<EngineSelector>,
    /// --engine-threads N, default 1, range 1..=available_parallelism()
    pub(crate) engine_threads: usize,
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct EngineChoice { pub(crate) engine: EngineSelector, pub(crate) threads: usize }
impl EngineChoices { pub(crate) fn resolve(&self) -> anyhow::Result<Vec<EngineChoice>>; }
// kermit/src/bench/run.rs
pub(crate) enum SweepCell { Kermit(Execution), Engine(EngineChoice) }
// check_sweep_group_directories takes `cells: &[SweepCell]`; criterion_group_name takes a SweepCell,
// built on Story 4's group_with_segment. SweepCell::group_segment delegates to Story 4's
// execution_segment (Kermit) and EngineChoice::label (Engine); Story 10 moves Story 4's
// a_kermit_cells_group_is_prefix_… test onto SweepCell::Kermit.
// kermit/src/bench/engine.rs
pub(crate) struct EngineSubject<'a, E: Engine> { /* loaded: E::Loaded, catalog, prepared */ }
pub(crate) struct EngineCell<E: Engine> { pub(crate) settings: E::Settings }
impl<E: Engine> Cell for EngineCell<E> { /* … */ }
```

- **Where the flags go.** `EngineChoices` is flattened into `RunSelection`, used by `bench run` and
  `bench probe`; there `-i` and `-a` become `required_unless_present = "engine"`. It is also
  flattened into `bench join`, where `-i` and `-a` stay required, because `QueryArgs` is shared
  with `kermit join` (spec § CLI).
- **Engine cell behaviour.** `EngineSubject::heap_bytes` is `E::space(..)` as `vec![total]`, or
  empty without space. `E::supports` returning `Unsupported` ends the cell with an error naming
  benchmark, query and engine, as any engine failure does (decision 12); it is not a skip. A query
  made only of checks never calls the engine, and its rows come from `check_multiplicity`.
- **Messages tests match:**
  - `--engine {name} needs the engine-{name} feature (build with --features engine-{name})`;
  - `--engine-threads must be between 1 and {available_parallelism}`;
  - `note: {label} does not stream, so it skips iteration and end-to-end`;
  - `note: {label} reports no space`.

**kermit-lab (Story 11).**
- `kermit_lab.engines(df, *, out=None) -> Figure`, exported in `__all__`. It plots the
  like-for-like pairs only: DuckDB against kermit on `end_to_end`, Nemo against kermit on `build`,
  `materialize` and `space`.
- Rows without `system` are back-filled `"kermit"`.
- The cell label (`analysis.cell_labels`): `{data_structure}/{algorithm}` for kermit rows, and for
  engines `system` plus `-t{engine_threads}` when set (`duckdb-t1`, `nemo`), as `Engine::label`.
- The six structure figures (`scaling`, `bar_time`, `bar_space`, `tradeoff`, `dist`,
  `bar_queries`) draw kermit rows only, through `presets._kermit_rows` (a row without `system`
  is kermit's), and their docstrings say so. Engines are drawn by `kl.engines()` and by
  `kl.plot` through the cell label (spec, owner's decision 2026-11-02). Story 12 documents this.

**Design stage:** /flow-draft session 7f47f6a8-52f1-44c3-bd8b-ecf1b2513f6e, 2026-11-02 13:02 to 14:22 UTC: $53.51 logged, $53.59 with estimated thinking; 14,027 plan lines, $3.81 per 1,000 lines.

## Stories

- **Story 1** — The toolchain moves past the async-drop regression, and the dev shell finds
  DuckDB.

  Locks the flake's `rust-overlay` at a nightly of 2026-11-02 or later, which `nemo-physical`
  needs: on 2026-10-31 it fails with `E0391` (rust-lang/rust#165263). The dev shell exports
  `DUCKDB_LIB_DIR` and `DUCKDB_INCLUDE_DIR` from Nix's `duckdb.lib` and `duckdb.dev`. On
  `nightly-2026-11-02` the current tree already passes `cargo fmt --check`, clippy with
  `-Dwarnings` on all targets, and rustdoc. Cargo also prints a manifest warning,
  "`package.homepage` is redundant with `package.repository`", once per crate; it fails no gate
  and this story leaves it alone.

  - **Developer:** junior-dev
  - **Files:** `flake.lock`, `flake.nix`
  - **Interfaces:** exposes the 2026-11-02+ nightly and `DUCKDB_LIB_DIR` / `DUCKDB_INCLUDE_DIR` in the dev shell to Stories 7–8 and 10; uses nothing
  - **Verify:** `bash -c 'd=$(rustc -V | grep -oE "[0-9]{4}-[0-9]{2}-[0-9]{2}") && [[ "$d" > "2026-10-31" ]] && test -f "$DUCKDB_LIB_DIR/libduckdb.so" && test -f "$DUCKDB_INCLUDE_DIR/duckdb.h" && RUSTFLAGS=-Dwarnings cargo clippy --all-targets && cargo fmt --all -- --check'`
  - **Review:** light (a lock bump and two environment variables)
  - **Reading:** spec decisions 9 and 10, § Premises item 6; `flake.nix:20-60`; `CLAUDE.md` gotcha "The flake's nightly must track CI's"

- **Story 2** — `execute_assignments`, `materialize_count`, `check_multiplicity`, and the shared shapes.

  The library side of `materialize`: full assignments instead of head rows, the shared sort and
  distinct count, and the check rule factored out of `build_with` for engines to reuse.
  `all_orders.rs`'s shapes move to `tests/common/shapes.rs` with `full_query`, and a new oracle
  test runs `materialize` on TreeTrie, ColumnTrie and HashTrie.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/db/lifecycle.rs`, `kermit/src/db.rs`, `kermit/tests/common/shapes.rs`, `kermit/tests/common/mod.rs`, `kermit/tests/all_orders.rs`, `kermit/tests/materialize_oracle.rs`
  - **Interfaces:** exposes `execute_assignments`, `Prepared::assignment_width`, `materialize_count`, `check_multiplicity` (Names contract: `kermit::db`) and `common::shapes` to Stories 3, 7, 8 and 10; uses `Prepared`, `Indexes`, `JoinAlgo`, `Tuples::sort`, `common::oracle`
  - **Verify:** `bash -c 'cargo test -p kermit --lib && cargo test -p kermit --test materialize_oracle --test all_orders'`
  - **Review:** deep
  - **Reading:** spec § The `materialize` metric, § Testing; `kermit/src/db/lifecycle.rs:300-440`; `kermit/tests/all_orders.rs:1-330`; `kermit/tests/common/oracle.rs:1-80`

- **Story 3** — The `materialize` metric in the binary.

  Adds `Metric::Materialize` and changes `Subject` (the `Built` GAT, `STREAMS`,
  `MATERIALIZE_MUTATES`, `materialize`). `QuerySubject` materialises through Story 2's functions.
  Adds the memory guard and its note, `--verify` on the `materialize` count,
  `--materialize-max-bytes` with its bounds, `bench probe --materialize`, and `bench ds`'s
  rejection. Reports that don't ask for `materialize` are unchanged.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/bench/mod.rs`, `kermit/src/bench/measure.rs`, `kermit/src/bench/run.rs`, `kermit/src/bench/ds.rs`, `kermit/src/main.rs`, `kermit/src/execution.rs`, `kermit/tests/cli_materialize.rs`
  - **Interfaces:** exposes `Subject` (new shape), `Materialized`, `Metric::Materialize`, `RunSettings::materialize_max_bytes`, `materialize_bytes` and `MaterializeArgs` (Names contract: the measure layer) to Stories 4 and 10; uses Story 2's `execute_assignments` and `materialize_count`
  - **Verify:** `bash -c 'cargo test -p kermit --bin kermit && cargo test -p kermit --test cli_materialize'`
  - **Review:** deep
  - **Reading:** spec § The `materialize` metric, § CLI, reports and kermit-lab; `kermit/src/bench/measure.rs:1-360`; `kermit/src/bench/run.rs:36-330`; `kermit/src/bench/mod.rs:25-50`; `kermit/src/main.rs:180-400`

- **Story 4** — `run_benchmark` over a `Cell`.

  Introduces `Cell` and `FamilyCell`, and makes `run_benchmark` generic over `Cell` rather than
  `ExecutionFamily`. This is a pure refactor: every report, stderr line and Criterion group is
  unchanged, and the existing CLI and report tests are the check.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/bench/cell.rs`, `kermit/src/bench/mod.rs`, `kermit/src/bench/run.rs`
  - **Interfaces:** exposes `Cell`, `FamilyCell` and `run_benchmark<C: Cell>` (Names contract: `Cell`) to Stories 9 and 10; uses Story 3's `Subject`
  - **Verify:** `cargo test -p kermit`
  - **Review:** deep
  - **Reading:** spec § The `Cell` refactor; `kermit/src/bench/run.rs:100-370`; `kermit/src/execution.rs:440-520`

- **Story 5** — The catalog shares its tuples.

  `Entry::tuples` becomes `Arc<Tuples>`, and `shared_tuples` hands out a clone of the `Arc`, so
  Nemo's `TableProvider` (whose box is `'static`) can stream from the catalog with one copy.
  Nothing that measures changes.

  - **Developer:** junior-dev
  - **Files:** `kermit/src/db/catalog.rs`
  - **Interfaces:** exposes `Catalog::shared_tuples` (Names contract: `Catalog`) to Story 8; uses nothing
  - **Verify:** `cargo test -p kermit --lib`
  - **Review:** light (one file, an `Arc` behind an unchanged accessor)
  - **Reading:** spec § Nemo (the one change outside the new code); `kermit/src/db/catalog.rs:1-130`

- **Story 6** — `kermit::engine`: the trait and the SQL translator.

  Adds the `Engine` trait, `EngineError` and `Unsupported`, and the SQL translator
  (`create_spec_table`, `join_select`, quoting), with golden-SQL unit tests: constants and
  equalities as filters, a self-join with two aliases over one merged spec, a disconnected query
  (`CROSS JOIN`), the full variable list. No engine yet, and no cargo feature.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/engine/mod.rs`, `kermit/src/engine/sql.rs`, `kermit/src/lib.rs`
  - **Interfaces:** exposes `Engine`, `EngineError`, `Unsupported` and the `sql` functions (Names contract: `kermit::engine`) to Stories 7, 8 and 10; uses `Prepared`, `IndexSpec`, `Filter`, `Catalog`
  - **Verify:** `cargo test -p kermit --lib`
  - **Review:** deep
  - **Reading:** spec § The `Engine` trait, § Translation; `kermit/src/db/lifecycle.rs:40-110`; `kermit-algos/src/index_plan.rs`; `kermit-algos/src/access.rs:40-100`

- **Story 7** — DuckDB.

  `kermit::engine::duckdb` behind `engine-duckdb`, linking Nix's `libduckdb` in the dev shell
  through `build.rs`'s rpath, with `engine-duckdb-bundled` for CI. It loads through the appender,
  builds the spec tables and prepares the statement at the end of `build`, streams Arrow batches,
  materialises through `materialize_count`, and sets its own memory limit and private temp
  directory. It creates `engine_oracle.rs` with DuckDB's cells (bag rows, distinct counts) and
  adds the `engines` CI job.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/engine/duckdb.rs`, `kermit/src/engine/mod.rs`, `kermit/Cargo.toml`, `kermit/build.rs`, `Cargo.toml`, `kermit/tests/engine_oracle.rs`, `.github/workflows/pr.yml`
  - **Interfaces:** exposes `DuckDb`, `DuckDbSettings` and the features `engine-duckdb` / `engine-duckdb-bundled` (Names contract: the engines) to Stories 10 and 12; uses Stories 2 and 6
  - **Verify:** `bash -c 'cargo test -p kermit --features engine-duckdb --lib && cargo test -p kermit --features engine-duckdb --test engine_oracle && RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets --features engine-duckdb && cargo test -p kermit --lib'`
  - **Review:** deep (the `temp_directory` landmine: a private spill directory per connection, as the JOB generator keeps one per call)
  - **Reading:** spec § DuckDB, § The `Engine` trait (DuckDB's own limits), § Premises item 1, decisions 10–12; `kermit-generate/src/job/duckdb.rs:1-120`; `.github/workflows/pr.yml`

- **Story 8** — Nemo.

  `kermit::engine::nemo` behind `engine-nemo`. A fresh `DatabaseInstance` per build; a
  `ViewProvider` streaming each spec's view from `Catalog::shared_tuples` as integer
  `AnyDataValue`s; `load_trie_into_memory` under `futures::executor::block_on`; the plan in
  kermit's variable order; `materialize` through `execute_plan` and `count_rows_in_memory`; `space`
  from `table_size_bytes`, read after loading. `rows` permutes the plan-ordered result columns into
  canonical order. Nemo joins the oracle and the `engines` job. Unit tests pin `NEMO_REV` against
  `kermit/Cargo.toml` and check that the join's markers follow kermit's plan order.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/engine/nemo.rs`, `kermit/src/engine/mod.rs`, `kermit/Cargo.toml`, `kermit/tests/engine_oracle.rs`, `.github/workflows/pr.yml`
  - **Interfaces:** exposes `Nemo`, `NEMO_REV`, `NEMO_PHYSICAL_VERSION` and the feature `engine-nemo` (Names contract: the engines) to Stories 10 and 12; uses Stories 2, 5 and 6
  - **Verify:** `bash -c 'cargo test -p kermit --features engine-duckdb,engine-nemo --lib && cargo test -p kermit --features engine-duckdb,engine-nemo --test engine_oracle && RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets --features engine-duckdb,engine-nemo'`
  - **Review:** deep
  - **Reading:** spec § Nemo, § Background (Nemo's API), § Premises items 2–6; the premise check's Nemo module at `/tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-state-of-kermit-18e430b26aaa0f70/b3663c65-7a34-4d42-bcaa-8dc41bfcea9c/scratchpad/premise-check/src/main.rs` (may be gone after a reboot; the spec records what it proved)

- **Story 9** — Report schema v7 and the `system` axis.

  Bumps `REPORT_SCHEMA_VERSION` and kermit-lab's `SCHEMA_VERSION` to 7 together. Every kermit
  report gets `system: "kermit"`, through `FamilyCell::axes` and `bench ds`. No engine row exists
  yet. kermit-lab's existing tests must still pass on v7 reports.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/bench_report.rs`, `kermit/src/bench/cell.rs`, `kermit/src/bench/ds.rs`, `python/kermit-lab/kermit_lab/__init__.py`, `kermit/tests/cli_bench_probe.rs`, `kermit/tests/cli_join_tests.rs`, `python/kermit-lab/tests/test_contract.py`
  - **Interfaces:** exposes report v7 and the `system` axis (Names contract: Report v7) to Stories 10–12; uses Story 4's `Cell`
  - **Verify:** `bash -c 'cargo test -p kermit && cd python/kermit-lab && uv run --locked --group test pytest -q'`
  - **Review:** deep
  - **Reading:** spec § CLI, reports and kermit-lab (Report schema v7); `kermit/src/bench_report.rs:100-140`; `docs/specs/bench-report-schema.md`; `python/kermit-lab/kermit_lab/__init__.py:1-40`

- **Story 10** — Engine cells in the binary.

  Adds `EngineSubject`, `EngineCell`, `EngineChoices` / `EngineSelector` and `SweepCell`.
  `--engine` and `--engine-threads` arrive on run, probe and bench join, with their bounds and the
  feature-missing error. Engine cells join the sweep, its group-name clash check and
  `bench probe`. The cell runs a fallible preflight, calls `check_multiplicity` inside the timed
  `build`, and prints the skip notes for `iteration`, `end_to_end` and `space`. Engine axes go on
  the report. The story ends by running spec Acceptance 2 on `triangle`.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/bench/engine.rs`, `kermit/src/bench/mod.rs`, `kermit/src/bench/run.rs`, `kermit/src/options.rs`, `kermit/src/main.rs`, `kermit/tests/cli_engines.rs`
  - **Interfaces:** exposes `EngineCell`, `EngineChoices`, `SweepCell` and the engine CLI (Names contract: engine cells) to Story 12; uses Stories 3, 4, 6, 7, 8 and 9
  - **Verify:** `bash -c 'cargo test -p kermit --features engine-duckdb,engine-nemo --bin kermit && cargo test -p kermit --features engine-duckdb,engine-nemo --test cli_engines && cargo test -p kermit && RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets --features engine-duckdb,engine-nemo && cargo run -p kermit --features engine-duckdb,engine-nemo -- bench --sample-size 10 --measurement-time 1 --warm-up-time 1 run triangle -i all -a all --engine duckdb,nemo --metrics build iteration space materialize end-to-end --verify'`
  - **Review:** deep
  - **Reading:** spec § The `Cell` refactor, § CLI, reports and kermit-lab, § Acceptance item 2; `kermit/src/bench/run.rs:370-470`; `kermit/src/options.rs` (DsChoices and the `DsFlag` table); `kermit/src/main.rs:180-420`, `:840-960`

- **Story 11** — kermit-lab reads engine rows.

  Back-fills `system = "kermit"`, recognises `materialize` as a phase, and labels cells by
  `system` for engines and `data_structure` / `algorithm` for kermit, in the plot defaults and
  `speedup_table`. Adds `kl.engines()`, which plots only the like-for-like pairs. Tests load v6
  and v7 reports together.

  - **Developer:** intermediate-dev
  - **Files:** `python/kermit-lab/kermit_lab/defaults.py`, `python/kermit-lab/kermit_lab/presets.py`, `python/kermit-lab/kermit_lab/analysis.py`, `python/kermit-lab/kermit_lab/__init__.py`, `python/kermit-lab/kermit_lab/frame.py`, `python/kermit-lab/kermit_lab/loader.py`, `python/kermit-lab/tests/test_loader.py`, `python/kermit-lab/tests/test_presets.py`, `python/kermit-lab/tests/test_public_api.py`, `python/kermit-lab/tests/test_engines.py`
  - **Interfaces:** exposes `kermit_lab.engines` (Names contract: kermit-lab) to Story 12; uses Story 9's v7 axes
  - **Verify:** `bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q'`
  - **Review:** deep
  - **Reading:** spec § Summary (the like-for-like pairs), § CLI, reports and kermit-lab (kermit-lab); `python/kermit-lab/kermit_lab/defaults.py`, `presets.py`, `analysis.py:280-370`

- **Story 12** — Docs, and the acceptance probes.

  Writes `docs/engines/` (the template, then DuckDB and Nemo pages), the CLAUDE.md recipe "Adding
  an external engine" with the module and gotchas, ARCHITECTURE.md's `kermit::engine`,
  `BENCHMARKING.md`'s like-for-like pairs, and the v7 catalogue in `bench-report-schema.md`. It
  runs spec Acceptance 3's probes and records each outcome on the engine pages.

  - **Developer:** intermediate-dev
  - **Files:** `docs/engines/TEMPLATE.md`, `docs/engines/duckdb.md`, `docs/engines/nemo.md`, `CLAUDE.md`, `ARCHITECTURE.md`, `BENCHMARKING.md`, `docs/specs/bench-report-schema.md`, `README.md`, `USAGE.md`, `kermit/README.md`, `python/kermit-lab/README.md`
  - **Interfaces:** exposes nothing; uses every earlier story
  - **Verify:** `bash -c 'test -f docs/engines/TEMPLATE.md && test -f docs/engines/duckdb.md && test -f docs/engines/nemo.md && grep -q "Adding an external engine" CLAUDE.md && grep -q "engine_version" docs/specs/bench-report-schema.md && cargo build --release -p kermit --features engine-duckdb,engine-nemo && for b in "triangle" "lubm-reference -q q1" "watdiv-stress-100-test-1 -q q0001" "oxford-uniform-s3 -q triangle"; do target/release/kermit bench probe $b -i all -a all --engine duckdb,nemo --verify --materialize || exit 1; done && for b in "sw-snap-amazon-pre -q triangle" "sw-h2o-pre -q q1" "sw-job-1-pre -q 1a"; do XDG_CACHE_HOME=/tb/Source/Academia/kermit-bench-runs/full-sweep-2026-10-24/cache target/release/kermit bench probe $b -i all -a all --engine duckdb,nemo --verify --materialize || exit 1; done'`
  - **Review:** light (docs, and probes that change nothing)
  - **Reading:** spec § Documentation, § Acceptance item 3, § Summary; `CLAUDE.md` "Extending the System"; `docs/data-structures/TEMPLATE.md`

<!-- plan_join: the stories below are generated from the drafts; edit the drafts, then re-join -->

## Story 1: The toolchain moves past the async-drop regression, and the dev shell finds DuckDB.

- **Developer:** junior-dev
- **Files:** `flake.lock`, `flake.nix`
- **Interfaces:** exposes the 2026-11-02+ nightly and `DUCKDB_LIB_DIR` / `DUCKDB_INCLUDE_DIR` in the dev shell to Stories 7–8 and 10; uses nothing
- **Verify:** `bash -c 'd=$(rustc -V | grep -oE "[0-9]{4}-[0-9]{2}-[0-9]{2}") && [[ "$d" > "2026-10-31" ]] && test -f "$DUCKDB_LIB_DIR/libduckdb.so" && test -f "$DUCKDB_INCLUDE_DIR/duckdb.h" && RUSTFLAGS=-Dwarnings cargo clippy --all-targets && cargo fmt --all -- --check'`
- **Review:** light (a lock bump and two environment variables)

The flake's `rust-overlay` input is locked at a nightly whose compiler cannot build `nemo-physical` (`E0391`, rust-lang/rust#165263), so Task 1.1 re-locks it to a nightly of 2026-11-02 or later, and Task 1.2 makes the dev shell export `DUCKDB_LIB_DIR` and `DUCKDB_INCLUDE_DIR` from Nix's `duckdb.lib` and `duckdb.dev` outputs, which the `engine-duckdb` feature (Stories 7 and 10) links against. Both tasks run in the dev shell, so a shell opened before Task 1.2 does not see the variables: every command here goes through `nix develop --command`, which re-evaluates the flake each time. The manifest warning "`package.homepage` is redundant with `package.repository`" that cargo prints once per crate fails no gate; leave it alone.

### Task 1.1: Lock `rust-overlay` at a nightly of 2026-11-02 or later

**Files:**
- Modify: `flake.lock`

The check below is the story's own Verify cut to the toolchain. `rustc -V` prints the compiler's commit date, which for the 2026-11-02 nightly is 2026-11-01, so the check compares that date with `2026-10-31` (the flake's present nightly prints a date on or before it).

- [ ] **Step 1: Write the failing check**

```bash
nix develop --command bash -c 'rustc -V; d=$(rustc -V | grep -oE "[0-9]{4}-[0-9]{2}-[0-9]{2}"); [[ "$d" > "2026-10-31" ]] && echo "toolchain ok" || { echo "toolchain too old: $d"; exit 1; }'
```

- [ ] **Step 2: Run it to see it fail**

Run the command from Step 1. Expected: a `rustc` line for the 2026-10-31 nightly, then `toolchain too old: <date on or before 2026-10-31>`, and exit status 1.

- [ ] **Step 3: Implement**

Refresh only the `rust-overlay` input (the other inputs keep their locked revisions):

```bash
nix flake update rust-overlay
git diff --stat -- flake.lock
```

The diff must touch `flake.lock` only, and only the `rust-overlay` node (`lastModified`, `narHash`, `rev`). If `rustc -V` in Step 4 still prints a date on or before 2026-10-31, the overlay has not yet published the 2026-11-02 manifest: stop and report it rather than editing the lock by hand, and rerun this step later.

- [ ] **Step 4: Run it to see it pass**

```bash
nix develop --command bash -c 'rustc -V; d=$(rustc -V | grep -oE "[0-9]{4}-[0-9]{2}-[0-9]{2}"); [[ "$d" > "2026-10-31" ]] && echo "toolchain ok"'
nix develop --command bash -c 'RUSTFLAGS=-Dwarnings cargo clippy --all-targets && cargo fmt --all -- --check'
```

Expected: a `rustc` line whose date is 2026-11-01 or later, then `toolchain ok`; clippy finishes with no warnings and `cargo fmt --check` prints nothing. (The tree passes both on `nightly-2026-11-02`; a failure here means the lock moved to a different nightly than intended, so stop and report.)

- [ ] **Step 5: Commit**

```bash
git commit -m "chore(nix): bump rust-overlay (nightly 2026-11-02), for nemo-physical (rust-lang/rust#165263)" -- flake.lock
```

### Task 1.2: Export `DUCKDB_LIB_DIR` and `DUCKDB_INCLUDE_DIR` from the dev shell

**Files:**
- Modify: `flake.nix:50-51`

- [ ] **Step 1: Write the failing check**

```bash
nix develop --command bash -c 'test -f "$DUCKDB_LIB_DIR/libduckdb.so" && test -f "$DUCKDB_INCLUDE_DIR/duckdb.h" && echo "duckdb ok"'
```

- [ ] **Step 2: Run it to see it fail**

Run the command from Step 1. Expected: no output and exit status 1, because both variables are unset (`test -f "/libduckdb.so"` is false).

- [ ] **Step 3: Implement**

In `flake.nix`, replace

```nix
          MIRIFLAGS = "-Zmiri-disable-isolation";
          RUST_BACKTRACE = "1";
```

with

```nix
          MIRIFLAGS = "-Zmiri-disable-isolation";
          RUST_BACKTRACE = "1";

          # The `engine-duckdb` feature of `kermit` links Nix's libduckdb instead of
          # compiling DuckDB's C++ amalgamation (`kermit/build.rs` reads both, and
          # emits an rpath to the first). `engine-duckdb-bundled` is for CI, which
          # has no Nix.
          DUCKDB_LIB_DIR = "${pkgs.duckdb.lib}/lib";
          DUCKDB_INCLUDE_DIR = "${pkgs.duckdb.dev}/include";
```

- [ ] **Step 4: Run it to see it pass**

```bash
nix develop --command bash -c 'test -f "$DUCKDB_LIB_DIR/libduckdb.so" && test -f "$DUCKDB_INCLUDE_DIR/duckdb.h" && echo "duckdb ok"'
nix develop --command bash -c 'd=$(rustc -V | grep -oE "[0-9]{4}-[0-9]{2}-[0-9]{2}") && [[ "$d" > "2026-10-31" ]] && test -f "$DUCKDB_LIB_DIR/libduckdb.so" && test -f "$DUCKDB_INCLUDE_DIR/duckdb.h" && RUSTFLAGS=-Dwarnings cargo clippy --all-targets && cargo fmt --all -- --check && echo "story ok"'
```

Expected: `duckdb ok`, then clippy and the format check pass and `story ok` is printed.

- [ ] **Step 5: Commit**

```bash
git commit -m "build(flake): export DUCKDB_LIB_DIR and DUCKDB_INCLUDE_DIR from the dev shell" -- flake.nix
```

## Story 2: `execute_assignments`, `materialize_count`, `check_multiplicity`, and the shared shapes.

- **Developer:** intermediate-dev
- **Files:** `kermit/src/db/lifecycle.rs`, `kermit/src/db.rs`, `kermit/tests/common/shapes.rs`, `kermit/tests/common/mod.rs`, `kermit/tests/all_orders.rs`, `kermit/tests/materialize_oracle.rs`
- **Interfaces:** exposes `execute_assignments`, `Prepared::assignment_width`, `materialize_count`, `check_multiplicity` (Names contract: `kermit::db`) and `common::shapes` to Stories 3, 7, 8 and 10; uses `Prepared`, `Indexes`, `JoinAlgo`, `Tuples::sort`, `common::oracle`
- **Verify:** `bash -c 'cargo test -p kermit --lib && cargo test -p kermit --test materialize_oracle --test all_orders'`
- **Review:** deep

The library side of `materialize`. `execute_assignments` is `execute` lending every variable of the executed query instead of the head prefix; `materialize_count` is the sort and distinct count that kermit's cells and DuckDB share; `check_multiplicity` is the all-constant-atom rule that `build_with` applied inline, factored out so the engines can reuse it. `execute` keeps its behaviour and now shares a private body with `execute_assignments`, which differs only in how many leading values it lends. `all_orders.rs`'s shapes and catalog helpers move to `tests/common/shapes.rs` (behaviour unchanged) beside a new `full_query`, and a new oracle test runs `materialize` on `TreeTrie`, `ColumnTrie` and `HashTrie` under every optimiser and a forced reversed order. No landmine marker appears in this story's code.

Two facts the later stories need. First, the columns of an `execute_assignments` row follow `canonical_names(prepared.query())`, the executed query's own canonical order; the head's variables come first, but body-only variables (a `_`'s fresh `K<n>`, or a variable missing from the head) can come in a different order than in the query as written, because the executed atoms list their variables in plan order. `full_query(query, &prepared)` therefore takes the `Prepared` the test ran and heads the rewritten query with `canonical_names(prepared.query())`, so the oracle's rows for it compare with `execute_assignments`' rows as they come, with no permutation; the oracle runs once per planner, since the order can change with the plan. Second, that head rests on one premise: `rewrite_placeholders` is deterministic (`kermit-algos/src/placeholder_rewrite.rs:31-39` numbers each `_` from one counter, past the query's highest `K<n>`), so the `K<n>` it gives a `_` in `full_query`'s call is the one it gave in `prepare`'s.

### Task 2.1: The three library functions

**Files:**
- Modify: `kermit/src/db/lifecycle.rs:45-120` (`Prepared::assignment_width`)
- Modify: `kermit/src/db/lifecycle.rs:293-432` (`check_multiplicity`, `build_with`, `execute`, `execute_assignments`)
- Modify: `kermit/src/db/lifecycle.rs` (unit tests, in `mod tests`)
- Modify: `kermit/src/db.rs:31-35`

- [ ] **Step 1: Write the failing tests**

Insert these tests in `mod tests` of `kermit/src/db/lifecycle.rs`, immediately above the `#[test]` of `an_empty_relation_joins_to_nothing`. They use the module's existing helpers (`catalog`, `parse`, `prepared_for`, `lexicographic`, `join_with`) and its `super::*` imports (`Tuples`, `Relation`, `JoinAlgo`, `JoinQuery`, `TreeTrie`, `HashTrie`, `LeapfrogTriejoin`, `HashTriejoin`).

```rust
    /// The sorted full assignments of `query` (planned in canonical order) as
    /// `R` through `JA`, each row `assignment_width` values wide.
    fn assignments_with<R: Relation, JA: JoinAlgo<R>>(
        catalog: &Catalog, query: &JoinQuery,
    ) -> Vec<Vec<usize>> {
        let prepared =
            prepare(catalog, query, &lexicographic(), R::TRIE_LEVELS).expect("a valid query");
        let indexes = build::<R>(catalog, &prepared);
        let mut rows = Vec::new();
        execute_assignments::<R, JA>(&prepared, &indexes, |row| {
            assert_eq!(row.len(), prepared.assignment_width());
            rows.push(row.to_vec());
        });
        rows.sort();
        rows
    }

    /// A head of `X` leaves `Y` (named, or a rewritten `_`) out of `execute`'s
    /// rows; `execute_assignments` lends it, after the head's variables.
    #[test]
    fn execute_assignments_lends_every_variable_where_execute_lends_the_head() {
        let c = catalog(vec![("r", 2, vec![vec![1, 2], vec![1, 3], vec![2, 4]])]);
        let want = vec![vec![1, 2], vec![1, 3], vec![2, 4]];
        for text in ["Q(X) :- r(X, Y).", "Q(X) :- r(X, _).", "Q(X, Y) :- r(X, Y)."] {
            let query = parse(text);
            assert_eq!(prepared_for(&c, text).assignment_width(), 2, "{text}");
            assert_eq!(
                assignments_with::<TreeTrie, LeapfrogTriejoin>(&c, &query),
                want,
                "{text}"
            );
            assert_eq!(
                assignments_with::<HashTrie, HashTriejoin>(&c, &query),
                want,
                "{text}"
            );
        }
        // `execute` still lends the head alone.
        let narrow = "Q(X) :- r(X, Y).";
        assert_eq!(prepared_for(&c, narrow).head_len(), 1);
        assert_eq!(
            join_with::<TreeTrie, LeapfrogTriejoin>(&c, &parse(narrow), &lexicographic()),
            vec![vec![1], vec![1], vec![2]]
        );
    }

    #[test]
    fn the_assignment_width_is_the_executed_querys_variable_count() {
        let c = catalog(vec![
            ("r", 2, vec![vec![1, 2]]),
            ("s", 2, vec![vec![2, 3]]),
        ]);
        assert_eq!(
            prepared_for(&c, "Q(X) :- r(X, Y), s(Y, Z).").assignment_width(),
            3
        );
        assert_eq!(
            prepared_for(&c, "Q(X, Y, Z) :- r(X, Y), s(Y, Z).").assignment_width(),
            3
        );
        // A constant is no variable, and a repeated variable counts once.
        assert_eq!(
            prepared_for(&c, "Q(X) :- r(X, X), s(c2, c3).").assignment_width(),
            1
        );
    }

    /// Multiplicity applies to assignments as to head rows: hash tries answer
    /// once per copy of an all-constant atom's row, sorted tries once, and a
    /// check that holds nowhere leaves nothing.
    #[test]
    fn execute_assignments_applies_the_multiplicity_of_the_checks() {
        let c = catalog(vec![
            ("r", 2, vec![vec![1, 2], vec![2, 3]]),
            ("s", 2, vec![vec![7, 8], vec![7, 8], vec![9, 9]]),
        ]);
        let query = parse("Q(X) :- r(X, Y), s(c7, c8).");
        assert_eq!(
            assignments_with::<TreeTrie, LeapfrogTriejoin>(&c, &query),
            vec![vec![1, 2], vec![2, 3]]
        );
        assert_eq!(
            assignments_with::<HashTrie, HashTriejoin>(&c, &query),
            vec![vec![1, 2], vec![1, 2], vec![2, 3], vec![2, 3]]
        );
        let absent = parse("Q(X) :- r(X, Y), s(c7, c9).");
        assert!(assignments_with::<TreeTrie, LeapfrogTriejoin>(&c, &absent).is_empty());
        assert!(assignments_with::<HashTrie, HashTriejoin>(&c, &absent).is_empty());
    }

    /// When every atom is a check there is no variable to lend: the empty row
    /// is emitted once per unit of multiplicity.
    #[test]
    fn execute_assignments_of_a_query_of_checks_emits_the_empty_row_per_multiplicity() {
        let c = catalog(vec![("s", 2, vec![vec![7, 8], vec![7, 8], vec![9, 9]])]);
        let mut query = parse("Q(X) :- s(c7, c8).");
        query.head.terms.clear();
        let prepared =
            prepare(&c, &query, &lexicographic(), TrieLevels::All).expect("a valid query");
        assert_eq!(prepared.assignment_width(), 0);
        assert_eq!(
            assignments_with::<TreeTrie, LeapfrogTriejoin>(&c, &query),
            vec![Vec::<usize>::new()]
        );
        assert_eq!(
            assignments_with::<HashTrie, HashTriejoin>(&c, &query),
            vec![Vec::<usize>::new(); 2]
        );
    }

    /// The factored rule is the one `build_with` applied inline: one unit per
    /// check unless the structure keeps duplicates, then the check's own
    /// count, and zero when a check admits no row.
    #[test]
    fn check_multiplicity_is_the_rule_the_build_applies() {
        let c = catalog(vec![
            ("r", 1, vec![vec![1], vec![2]]),
            ("s", 2, vec![vec![7, 8], vec![7, 8], vec![9, 9]]),
            ("t", 1, vec![vec![5], vec![5], vec![5]]),
        ]);
        let multiplicity = |text: &str, keeps_duplicates: bool| {
            check_multiplicity(&c, &prepared_for(&c, text), keeps_duplicates)
        };
        assert_eq!(multiplicity("Q(X) :- r(X).", false), 1);
        assert_eq!(multiplicity("Q(X) :- r(X).", true), 1);
        assert_eq!(multiplicity("Q(X) :- r(X), s(c7, c8).", false), 1);
        assert_eq!(multiplicity("Q(X) :- r(X), s(c7, c8).", true), 2);
        assert_eq!(multiplicity("Q(X) :- r(X), s(c7, c8), t(c5).", false), 1);
        assert_eq!(multiplicity("Q(X) :- r(X), s(c7, c8), t(c5).", true), 6);
        let nowhere = "Q(X) :- r(X), s(c7, c8), s(c7, c9).";
        assert_eq!(multiplicity(nowhere, false), 0);
        assert_eq!(multiplicity(nowhere, true), 0);

        // `build` still takes it, per structure.
        let prepared = prepared_for(&c, "Q(X) :- r(X), s(c7, c8), t(c5).");
        assert_eq!(
            build::<TreeTrie>(&c, &prepared).multiplicity(),
            check_multiplicity(&c, &prepared, <TreeTrie as Relation>::KEEPS_DUPLICATES)
        );
        assert_eq!(
            build::<HashTrie>(&c, &prepared).multiplicity(),
            check_multiplicity(&c, &prepared, <HashTrie as Relation>::KEEPS_DUPLICATES)
        );
    }

    /// A batch of `arity`-ary rows.
    fn rows_of(arity: usize, rows: &[&[usize]]) -> Tuples {
        let mut batch = Tuples::new(arity);
        for row in rows {
            batch.push(row);
        }
        batch
    }

    #[test]
    fn materialize_count_sorts_the_rows_and_counts_the_distinct_ones() {
        let (sorted, distinct) = materialize_count(rows_of(2, &[&[2, 1], &[1, 2], &[2, 1], &[1, 1]]));
        assert_eq!(sorted.to_vecs(), vec![
            vec![1, 1],
            vec![1, 2],
            vec![2, 1],
            vec![2, 1]
        ]);
        assert_eq!(distinct, 3, "the duplicate row counts once");
    }

    #[test]
    fn materialize_count_of_no_rows_is_zero_and_keeps_the_arity() {
        let (sorted, distinct) = materialize_count(Tuples::new(3));
        assert_eq!((sorted.len(), sorted.arity(), distinct), (0, 3, 0));
    }

    /// Arity 0 has nothing to order, and every row is the empty row.
    #[test]
    fn materialize_count_of_empty_rows_is_one() {
        let (sorted, distinct) = materialize_count(rows_of(0, &[&[], &[], &[]]));
        assert_eq!((sorted.len(), distinct), (3, 1));
    }

    /// Rows wider than four sort by a permutation of row ids, not as arrays.
    #[test]
    fn materialize_count_sorts_wide_rows() {
        let (sorted, distinct) = materialize_count(rows_of(5, &[
            &[1, 2, 3, 4, 5],
            &[0, 9, 9, 9, 9],
            &[1, 2, 3, 4, 5],
        ]));
        assert_eq!(sorted.to_vecs(), vec![
            vec![0, 9, 9, 9, 9],
            vec![1, 2, 3, 4, 5],
            vec![1, 2, 3, 4, 5]
        ]);
        assert_eq!(distinct, 2);
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --lib db::lifecycle`

Expected: the crate fails to compile. The errors include ``error[E0425]: cannot find function `execute_assignments` in this scope``, ``error[E0425]: cannot find function `check_multiplicity` in this scope``, ``error[E0425]: cannot find function `materialize_count` in this scope`` and ``error[E0599]: no method named `assignment_width` found for struct `Prepared` ``.

- [ ] **Step 3: Implement**

All edits are in `kermit/src/db/lifecycle.rs` unless stated.

**(a) `Prepared::assignment_width`.** In `impl Prepared`, insert after `head_len`:

Current (lines 66-68):

```rust
    /// The number of head terms: the length of the prefix of each joined row
    /// that `execute` lends to its sink.
    pub fn head_len(&self) -> usize { self.head_len }
```

Replace with:

```rust
    /// The number of head terms: the length of the prefix of each joined row
    /// that `execute` lends to its sink.
    pub fn head_len(&self) -> usize { self.head_len }

    /// The executed query's variable count: the width of the rows
    /// [`execute_assignments`] lends, whose column `i` is the variable
    /// `canonical_names(self.query())[i]`. The head's variables come first, so
    /// this is at least [`head_len`](Self::head_len), and it exceeds it by the
    /// body-only variables (the fresh `K<n>` of each `_` included). A query of
    /// checks has none.
    ///
    /// Counted from the executed query on each call rather than stored, so
    /// [`execute`], which reads a `Prepared` in the timed region, sees no new
    /// field. Call it once per query, not once per row.
    pub fn assignment_width(&self) -> usize { canonical_names(&self.query).len() }
```

**(b) `check_multiplicity`, and `build_with` using it.** Insert `check_multiplicity` immediately above the doc comment of `build_with` (the line `/// Builds the indexes of \`prepared\` from \`catalog\`, constructing each with`):

```rust
/// How many times `execute` emits each answer, decided by the constant checks
/// alone: 1 with no checks; 0 when any check's view counts no row; otherwise
/// the product of the checks' counts, each clamped to 1 unless
/// `keeps_duplicates` ([`Relation::KEEPS_DUPLICATES`]: a structure that keeps
/// duplicates answers once per row, one that does not answers once). It reads
/// the catalog only, so an engine that builds no [`Indexes`] applies the same
/// rule, and [`build_with`] calls it with its structure's flag.
///
/// # Panics
///
/// If `prepared` was made over another catalog and a relation a check reads
/// is missing.
pub fn check_multiplicity(catalog: &Catalog, prepared: &Prepared, keeps_duplicates: bool) -> usize {
    let mut multiplicity = 1usize;
    for check in &prepared.checks {
        let tuples = tuples_of(catalog, &check.relation);
        let count = TupleView::new(tuples, check.filter.clone(), Vec::new()).count();
        let copies = if keeps_duplicates {
            count
        } else {
            count.min(1)
        };
        multiplicity = multiplicity.saturating_mul(copies);
    }
    multiplicity
}

```

Then, in `build_with`:

Current (lines 323-336):

```rust
    // A check keeps no column: it only counts the rows its filter admits. A
    // structure that keeps duplicates answers once per row; one that does
    // not answers once.
    let mut multiplicity = 1usize;
    for check in &prepared.checks {
        let tuples = tuples_of(catalog, &check.relation);
        let count = TupleView::new(tuples, check.filter.clone(), Vec::new()).count();
        let copies = if R::KEEPS_DUPLICATES {
            count
        } else {
            count.min(1)
        };
        multiplicity = multiplicity.saturating_mul(copies);
    }
```

Replace with:

```rust
    // A check keeps no column: it only counts the rows its filter admits.
    let multiplicity = check_multiplicity(catalog, prepared, R::KEEPS_DUPLICATES);
```

**(c) `execute` and `execute_assignments`.** The body of `execute` moves into a private `execute_prefix`, which lends the first `lent` values of each row.

Current (lines 387-432):

```rust
pub fn execute<R, JA>(prepared: &Prepared, indexes: &Indexes<R>, mut emit: impl FnMut(&[usize]))
where
    R: JoinIterable,
    JA: JoinAlgo<R>,
{
    debug_assert_eq!(
        indexes.specs, prepared.indexes,
        "the indexes were not built from this `Prepared`"
    );
    let multiplicity = indexes.multiplicity;
    if multiplicity == 0 {
        return;
    }
    if prepared.query.body.is_empty() {
        debug_assert_eq!(prepared.head_len, 0, "a query of checks has an empty head");
        for _ in 0..multiplicity {
            emit(&[]);
        }
        return;
    }

    let ds_map: HashMap<String, &R> = prepared
        .query
        .body
        .iter()
        .map(|atom| {
            let index = indexes.get(&atom.name);
            (
                atom.name.clone(),
                index.expect("the executed query names only built indexes"),
            )
        })
        .collect();

    // The executors emit every variable of the query they run in canonical
    // order, and `analyse` numbers the head's variables `0..head_len` in
    // head order, so the head is each row's prefix. Lending it allocates
    // nothing.
    let head_len = prepared.head_len;
    JA::join_for_each(&prepared.plan, prepared.query.clone(), ds_map, |row| {
        let head = &row[..head_len];
        for _ in 0..multiplicity {
            emit(head);
        }
    });
}
```

Replace with:

```rust
pub fn execute<R, JA>(prepared: &Prepared, indexes: &Indexes<R>, emit: impl FnMut(&[usize]))
where
    R: JoinIterable,
    JA: JoinAlgo<R>,
{
    execute_prefix::<R, JA>(prepared, indexes, prepared.head_len, emit);
}

/// [`execute`], lending every variable of the executed query instead of the
/// head: each row is [`assignment_width`](Prepared::assignment_width) values
/// in the executed query's canonical order (column `i` is
/// `canonical_names(prepared.query())[i]`, the head's variables first), the
/// order the executors emit. It slices no
/// head, and applies [`multiplicity`](Indexes::multiplicity) as `execute`
/// does: a check that holds nowhere emits nothing, and a query of checks emits
/// the empty row `multiplicity` times. Nothing is collected here; the
/// `materialize` metric pushes the rows into a `Tuples`.
///
/// # Panics
///
/// As [`execute`].
pub fn execute_assignments<R, JA>(
    prepared: &Prepared, indexes: &Indexes<R>, emit: impl FnMut(&[usize]),
) where
    R: JoinIterable,
    JA: JoinAlgo<R>,
{
    execute_prefix::<R, JA>(prepared, indexes, prepared.assignment_width(), emit);
}

/// The one body of [`execute`] and [`execute_assignments`]: the join, lending
/// the first `lent` values of each row, `multiplicity` times. The executors
/// emit every variable of the query they run in its canonical order
/// (`canonical_names(prepared.query())`), and `analyse` numbers the head's
/// variables `0..head_len` in head order, so the head is each row's prefix, and
/// the whole row is its first `assignment_width` values. Lending a prefix allocates nothing.
#[inline]
fn execute_prefix<R, JA>(
    prepared: &Prepared, indexes: &Indexes<R>, lent: usize, mut emit: impl FnMut(&[usize]),
) where
    R: JoinIterable,
    JA: JoinAlgo<R>,
{
    debug_assert_eq!(
        indexes.specs, prepared.indexes,
        "the indexes were not built from this `Prepared`"
    );
    let multiplicity = indexes.multiplicity;
    if multiplicity == 0 {
        return;
    }
    if prepared.query.body.is_empty() {
        debug_assert_eq!(lent, 0, "a query of checks has no variable to lend");
        for _ in 0..multiplicity {
            emit(&[]);
        }
        return;
    }

    let ds_map: HashMap<String, &R> = prepared
        .query
        .body
        .iter()
        .map(|atom| {
            let index = indexes.get(&atom.name);
            (
                atom.name.clone(),
                index.expect("the executed query names only built indexes"),
            )
        })
        .collect();

    JA::join_for_each(&prepared.plan, prepared.query.clone(), ds_map, |row| {
        let lent_row = &row[..lent];
        for _ in 0..multiplicity {
            emit(lent_row);
        }
    });
}
```

**(d) Sort and count.** Insert `materialize_count` after `execute_prefix` (before `#[cfg(test)]`):

```rust
/// Sorts `rows` (`Tuples::sort`, the sort both sorted tries build with) and
/// counts the adjacent-distinct rows, returning the sorted batch and the count.
/// The `materialize` metric of kermit's cells and of DuckDB both end here, so
/// the sort costs the same everywhere; the batch is returned so that the
/// caller frees it after its timer stops. Over the full assignments of
/// duplicate-free inputs nothing is removed by the count, and the sort is the
/// cost a deduplicating result trie pays.
pub fn materialize_count(mut rows: Tuples) -> (Tuples, u64) {
    rows.sort();
    let mut distinct = 0u64;
    let mut previous: Option<&[usize]> = None;
    for row in rows.rows() {
        if previous != Some(row) {
            distinct += 1;
        }
        previous = Some(row);
    }
    (rows, distinct)
}

```

**(e) Re-export.** In `kermit/src/db.rs`:

Current (lines 31-35):

```rust
pub use {
    catalog::Catalog,
    lifecycle::{build, build_with, execute, prepare, Indexes, Prepared},
    validation::{validate_query, JoinError, RelationArities},
};
```

Replace with:

```rust
pub use {
    catalog::Catalog,
    lifecycle::{
        build, build_with, check_multiplicity, execute, execute_assignments, materialize_count,
        prepare, Indexes, Prepared,
    },
    validation::{validate_query, JoinError, RelationArities},
};
```

Then format: `nix develop --command cargo fmt --all`.

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --lib db::lifecycle`

Expected: all tests in `db::lifecycle::tests` pass, including the eight new ones and every pre-existing one (`checks_multiply_on_hash_tries_only`, `all_constant_atoms_multiply_the_answer_on_hash_tries_only`, `a_count_multiplies_a_zero_level_index` pin `build_with`'s multiplicity through the factored rule).

Then run: `nix develop --command cargo clippy -p kermit --all-targets -- -D warnings`

Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(db): execute_assignments, materialize_count and check_multiplicity" -- kermit/src/db/lifecycle.rs kermit/src/db.rs
```

### Task 2.2: Move the shapes to `common::shapes`, and add `full_query`

**Files:**
- Create: `kermit/tests/common/shapes.rs`
- Modify: `kermit/tests/common/mod.rs:11-17`
- Modify: `kermit/tests/all_orders.rs:18-35` (imports), `:115-251` (the moved block), end of file (the new test)
- Test: `kermit/tests/all_orders.rs`

- [ ] **Step 1: Write the failing test**

In `kermit/tests/all_orders.rs`, add `full_query` to the imports and a test for it (it uses `prepare`, `Planner`, `FixedOrder`, `Relation`, `TreeTrie`, `JoinQuery`, `canonical_names` and `catalog_for`, which the file already has). First the import:

Current (lines 20-23):

```rust
    common::{
        oracle::{oracle, Lcg, Relations},
        orders::{every_order, FixedOrder},
    },
```

Replace with:

```rust
    common::{
        oracle::{oracle, Lcg, Relations},
        orders::{every_order, FixedOrder},
        shapes::full_query,
    },
```

Then append this test at the end of the file:

```rust
/// `full_query` puts every variable of the rewritten query in the head, in
/// the executed query's canonical order (`canonical_names(prepared.query())`):
/// a `_` as its fresh variable, a body-only variable after the head's, and,
/// under a forced order, in the order the executed atoms list them, which is
/// not the order of the query as written. Constants stay in the body.
#[test]
fn full_query_heads_every_variable_in_the_executed_canonical_order() {
    let levels = <TreeTrie as Relation>::TRIE_LEVELS;

    let relations: Relations = vec![
        ("r0".to_string(), 3, vec![]),
        ("r1".to_string(), 1, vec![]),
    ];
    let catalog = catalog_for(&relations);
    let query: JoinQuery = "Q(V0) :- r0(V0, _, V2), r1(V2).".parse().unwrap();
    let planner = Planner::new(Box::new(FixedOrder(names(&["V2", "K0", "V0"]))));
    let prepared = prepare(&catalog, &query, &planner, levels).unwrap();
    let full = full_query(&query, &prepared);
    // `#0` lists r0's columns in plan order, V2 then K0 then V0, so the
    // body-only variables come after the head's as V2, K0.
    assert_eq!(canonical_names(prepared.query()), ["V0", "V2", "K0"]);
    assert_eq!(full.head.to_string(), "Q(V0, V2, K0)");
    let body: Vec<String> = full.body.iter().map(ToString::to_string).collect();
    assert_eq!(body, ["r0(V0, K0, V2)", "r1(V2)"]);

    let relations: Relations = vec![
        ("r0".to_string(), 2, vec![]),
        ("r1".to_string(), 2, vec![]),
        ("r2".to_string(), 2, vec![]),
    ];
    let catalog = catalog_for(&relations);
    let query: JoinQuery = "Q(X, Z) :- r0(X, Y), r1(Y, c2), r2(Y, Z).".parse().unwrap();
    let planner = Planner::new(Box::new(FixedOrder(names(&["X", "Y", "Z"]))));
    let prepared = prepare(&catalog, &query, &planner, levels).unwrap();
    let full = full_query(&query, &prepared);
    assert_eq!(full.head.to_string(), "Q(X, Z, Y)");
    assert_eq!(canonical_names(prepared.query()), ["X", "Z", "Y"]);
    assert_eq!(full.body[1].to_string(), "r1(Y, c2)");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --test all_orders`

Expected: the test binary fails to compile with ``error[E0432]: unresolved import `common::shapes` ``.

- [ ] **Step 3: Implement**

**(a) Create `kermit/tests/common/shapes.rs`.** The shapes, the generator and the catalog helpers are `all_orders.rs`'s, moved verbatim except for `pub` and the imports; `full_query` is new.

```rust
//! The query shapes the oracle tests share: the 16 standard join patterns'
//! shapes and the constant cases, the seeded random relations they run over,
//! and the catalog helpers. `all_orders.rs` runs them under every variable
//! order, `materialize_oracle.rs` and `engine_oracle.rs` under each
//! optimiser, so every test reads the same cases.

use {
    super::oracle::{Lcg, Relations},
    kermit::db::{Catalog, Prepared},
    kermit_algos::{canonical_names, rewrite_placeholders, JoinQuery, StatisticsLevel},
    kermit_ds::{RelationHeader, Tuples},
    kermit_parser::Term,
};

/// The values in the random relations, and so the constants `c0`..`c2` the
/// shapes use.
pub const DOMAIN: usize = 3;

/// Seeds per shape. Each seed gives the shape's relations other tuples,
/// duplicates included.
pub const SEEDS: std::ops::Range<u64> = 0..6;

/// A query over relations `r0`, `r1`, …, with the arity of each.
pub struct Shape {
    pub name: &'static str,
    pub query: &'static str,
    pub arities: &'static [usize],
}

fn shape(name: &'static str, query: &'static str, arities: &'static [usize]) -> Shape {
    Shape {
        name,
        query,
        arities,
    }
}

/// The 16 standard patterns' shapes (those whose fixtures differ only in
/// data share a shape and keep their names), then the constant cases.
pub fn shapes() -> Vec<Shape> {
    vec![
        shape("unary", "Q(V0) :- r0(V0).", &[1]),
        shape(
            "triangle",
            "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2), r2(V0, V2).",
            &[2, 2, 2],
        ),
        shape(
            "chain",
            "Q(V0, V1, V2, V3) :- r0(V0, V1), r1(V1, V2), r2(V2, V3).",
            &[2, 2, 2],
        ),
        shape("star", "Q(V0, V1, V2) :- r0(V0, V1), r1(V0, V2).", &[2, 2]),
        shape("selfjoin", "Q(V0, V1, V2) :- r0(V0, V1), r0(V1, V2).", &[2]),
        shape("existential", "Q(V0) :- r0(V0), r1(V0).", &[1, 1]),
        shape(
            "empty-result",
            "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2).",
            &[2, 2],
        ),
        shape("single-relation", "Q(V0, V1) :- r0(V0, V1).", &[2]),
        shape(
            "four-way-chain",
            "Q(V0, V1, V2, V3, V4) :- r0(V0, V1), r1(V1, V2), r2(V2, V3), r3(V3, V4).",
            &[2, 2, 2, 2],
        ),
        shape(
            "wide-fanout",
            "Q(V0, V1, V2) :- r0(V0, V1), r1(V0, V2).",
            &[2, 2],
        ),
        shape("dead-end", "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2).", &[
            2, 2,
        ]),
        shape(
            "deep-dead-end",
            "Q(V0, V1, V2, V3) :- r0(V0, V1), r1(V1, V2), r2(V2, V3).",
            &[2, 2, 2],
        ),
        shape("diagonal", "Q(V0) :- r0(V0, V0).", &[2]),
        shape(
            "repeated-nonadjacent",
            "Q(V0, V1) :- r0(V0, V1, V0), r1(V1).",
            &[3, 1],
        ),
        shape("trailing-placeholder", "Q(V0) :- r0(V0, _), r1(V0).", &[
            2, 1,
        ]),
        shape(
            "middle-placeholder",
            "Q(V0, V2) :- r0(V0, _, V2), r1(V2).",
            &[3, 1],
        ),
        shape("constant-object", "Q(X) :- r0(X, c1).", &[2]),
        shape("constant-with-repeat", "Q(X) :- r0(X, X, c1).", &[3]),
        shape(
            "constant-in-a-join",
            "Q(X, Z) :- r0(X, Y), r1(Y, c2), r2(Y, Z).",
            &[2, 2, 2],
        ),
        shape("constant-subject", "Q(Y) :- r0(c1, Y), r1(Y).", &[2, 1]),
        shape("atom-of-only-constants", "Q(X) :- r0(X), r1(c0, c1).", &[
            1, 2,
        ]),
        shape(
            "mixed-constants-and-repeats",
            "Q(X, Z) :- r0(X, c1, Y), r1(Y, Z, Z), r2(c0, c1).",
            &[3, 3, 2],
        ),
    ]
}

/// One relation per arity, `r0`, `r1`, …, each with 8–12 tuples drawn from
/// `0..DOMAIN`, so duplicates are likely and joins are dense.
pub fn random_data(rng: &mut Lcg, arities: &[usize]) -> Relations {
    arities
        .iter()
        .enumerate()
        .map(|(i, &arity)| {
            let tuples = (0..8 + rng.below(5))
                .map(|_| (0..arity).map(|_| rng.below(DOMAIN)).collect())
                .collect();
            (format!("r{i}"), arity, tuples)
        })
        .collect()
}

/// `relations` as a catalog at the tuple-count level, which is all
/// `FixedOrder` reads. Each relation is built row by row, so an empty one
/// still has its arity.
pub fn catalog_for(relations: &Relations) -> Catalog {
    catalog_at(relations, StatisticsLevel::TupleCounts)
}

/// A catalog of `relations` with the statistics `level` asks for.
pub fn catalog_at(relations: &Relations, level: StatisticsLevel) -> Catalog {
    Catalog::new(
        relations
            .iter()
            .map(|(name, arity, tuples)| {
                let mut flat = Tuples::new(*arity);
                for tuple in tuples {
                    flat.push(tuple);
                }
                (RelationHeader::new_positional(name.as_str(), *arity), flat)
            })
            .collect(),
        level,
    )
}

/// `query` after `rewrite_placeholders`, with every variable in the head, in
/// `prepared`'s canonical order (`canonical_names(prepared.query())`): the
/// full conjunctive query, whose oracle rows compare with
/// `execute_assignments`' and every engine's rows as they come, with no
/// permutation. The oracle's rows for it have bag semantics; their distinct
/// rows are the same set under set and bag semantics.
///
/// The head rests on one premise: `rewrite_placeholders` is deterministic
/// (`kermit-algos/src/placeholder_rewrite.rs:31-39` numbers each `_` from one
/// counter, past the query's highest `K<n>`), so a `_` gets the same `K<n>`
/// here as in the call `prepare` made, and the names in the executed query's
/// canonical order are the rewritten body's own.
pub fn full_query(query: &JoinQuery, prepared: &Prepared) -> JoinQuery {
    let mut full = rewrite_placeholders(query.clone());
    full.head.terms = canonical_names(prepared.query())
        .into_iter()
        .map(Term::Var)
        .collect();
    full
}
```

**(b) Register the module.** In `kermit/tests/common/mod.rs`:

Current (lines 14-17):

```rust
#[allow(dead_code)]
pub mod snap;
#[allow(dead_code)]
pub mod utils;
```

Replace with:

```rust
#[allow(dead_code)]
pub mod shapes;
#[allow(dead_code)]
pub mod snap;
#[allow(dead_code)]
pub mod utils;
```

**(c) Delete the moved block from `kermit/tests/all_orders.rs`.** Delete lines 115-251 of the file as it stood before Step 1's edits (Step 1 added one import line, so they are now 116-252): from the doc line `/// The values in the random relations, and so the constants \`c0\`..\`c2\` the` through the blank line after the closing `}` of `catalog_at`. The block holds, in order, `DOMAIN`, `SEEDS`, `Shape`, `shape`, `shapes`, `random_data`, `catalog_for` and `catalog_at`, and is exactly what `shapes.rs` now defines. After the deletion, `/// The sorted rows of \`query\` over \`catalog\` as \`R\` through \`JA\`, planned in` follows the `a_placeholder_is_ordered_like_a_variable` test directly.

Check: `grep -nE 'fn (shapes|random_data|catalog_for|catalog_at)|const (DOMAIN|SEEDS)|struct Shape' kermit/tests/all_orders.rs` prints nothing.

**(d) Fix the imports of `all_orders.rs`.** The file still calls `catalog_for`, `catalog_at`, `random_data`, `shapes` and `SEEDS`, and no longer needs `RelationHeader`.

Current (after Step 1):

```rust
    common::{
        oracle::{oracle, Lcg, Relations},
        orders::{every_order, FixedOrder},
        shapes::full_query,
    },
```

Replace with:

```rust
    common::{
        oracle::{oracle, Lcg, Relations},
        orders::{every_order, FixedOrder},
        shapes::{catalog_at, catalog_for, full_query, random_data, shapes, SEEDS},
    },
```

Current (lines 30-33):

```rust
    kermit_ds::{
        Cardinality, ColumnTrie, EagerExpansion, HashTrie, KeyLevels, LazyExpansion, NoPruning,
        Relation, RelationHeader, SingletonPruning, TreeTrie, Tuples,
    },
```

Replace with:

```rust
    kermit_ds::{
        Cardinality, ColumnTrie, EagerExpansion, HashTrie, KeyLevels, LazyExpansion, NoPruning,
        Relation, SingletonPruning, TreeTrie, Tuples,
    },
```

`Catalog`, `StatisticsLevel` and `Tuples` stay imported: `rows_in_order` takes a `&Catalog` and builds a `Tuples`, and `every_optimiser_plans_payload_last` names `StatisticsLevel::ColumnDistinct`.

Then format: `nix develop --command cargo fmt --all`.

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --test all_orders`

Expected: every test passes, `full_query_heads_every_variable_in_the_executed_canonical_order` included, and the same tests as before the move run (the shapes' behaviour is unchanged).

Then run: `nix develop --command cargo clippy -p kermit --all-targets -- -D warnings`

Expected: no warnings (an unused import in `all_orders.rs` or a missing `RelationHeader` would show here).

- [ ] **Step 5: Commit**

```bash
git add -N kermit/tests/common/shapes.rs
git commit -m "test: move the join shapes to common::shapes and add full_query" -- kermit/tests/common/shapes.rs kermit/tests/common/mod.rs kermit/tests/all_orders.rs
```

### Task 2.3: `materialize` on the three structures against the oracle

**Files:**
- Create: `kermit/tests/materialize_oracle.rs`
- Test: `kermit/tests/materialize_oracle.rs`

- [ ] **Step 1: Write the test**

Create `kermit/tests/materialize_oracle.rs`:

```rust
//! `materialize` on kermit's own cells against the nested-loop oracle. For
//! each structure under its algorithm, each shape of `common::shapes` runs
//! over seeded random relations under every optimiser and a forced reversed
//! order, through `kermit::db::{prepare, build, execute_assignments,
//! materialize_count}`:
//! - the assignments `execute_assignments` lends equal, as they come, the
//!   oracle's rows for the full query (`full_query(query, &prepared)`), bag
//!   semantics: its head is `canonical_names(prepared.query())`, the order
//!   the assignments' columns are in;
//! - `materialize_count` returns those rows sorted and counts the distinct
//!   ones, which are the same set under set and bag semantics.
//! Contents are compared, not only counts: on fixtures this small a wrong
//! join condition can still produce the right count.

mod common;

use {
    clap::ValueEnum,
    common::{
        oracle::{oracle, Lcg, Relations},
        orders::FixedOrder,
        shapes::{catalog_at, full_query, random_data, shapes, SEEDS},
    },
    kermit::db::{build, execute_assignments, materialize_count, prepare, Catalog},
    kermit_algos::{
        canonical_names, rewrite_placeholders, HashTriejoin, JoinAlgo, JoinQuery,
        LeapfrogTriejoin, Optimiser, Planner, StatisticsLevel,
    },
    kermit_ds::{Cardinality, ColumnTrie, HashTrie, Relation, TreeTrie, Tuples},
    kermit_iters::SipHashStrategy,
};

/// Every optimiser, then the reversed canonical order of the rewritten
/// query's variables, which puts the body-only variables of a `_` or a narrow
/// head before the head's in some plans.
fn planners(query: &JoinQuery) -> Vec<(String, Planner)> {
    let mut planners: Vec<(String, Planner)> = Optimiser::value_variants()
        .iter()
        .map(|optimiser| {
            (
                optimiser.axis_value().to_string(),
                Planner::new(optimiser.instantiate()),
            )
        })
        .collect();
    let reversed: Vec<String> = canonical_names(&rewrite_placeholders(query.clone()))
        .into_iter()
        .rev()
        .collect();
    planners.push((
        "reversed".to_string(),
        Planner::new(Box::new(FixedOrder(reversed))),
    ));
    planners
}

/// What one case showed, for the cell's bounds.
struct Facts {
    /// The assignments were wider than the head.
    wider: bool,
    /// Rows in the oracle's bag for the full query.
    bag_len: usize,
    /// Distinct rows among them.
    distinct_len: usize,
}

/// One query over one catalog, planned by `planner`, as `R` through `JA`.
/// The oracle runs `full_query(query, &prepared)`, whose head is the
/// executed query's canonical order, so its rows compare with the
/// assignments as they come.
fn check_case<R: Relation, JA: JoinAlgo<R>>(
    relations: &Relations, catalog: &Catalog, query: &JoinQuery, planner: &Planner, case: &str,
) -> Facts {
    let prepared = prepare(catalog, query, planner, R::TRIE_LEVELS)
        .unwrap_or_else(|e| panic!("{case}: {e}"));
    let full = full_query(query, &prepared);
    let bag = oracle::<R>(relations, &full);
    let mut distinct = bag.clone();
    distinct.dedup();

    let indexes = build::<R>(catalog, &prepared);
    let width = prepared.assignment_width();
    assert_eq!(
        width,
        canonical_names(&full).len(),
        "{case}: the assignment width"
    );
    let mut rows = Tuples::new(width);
    execute_assignments::<R, JA>(&prepared, &indexes, |row| rows.push(row));

    let executed = rows.to_vecs();
    let mut got = executed.clone();
    got.sort();
    assert_eq!(got, bag, "{case}: execute_assignments");

    let (sorted, count) = materialize_count(rows);
    let sorted = sorted.to_vecs();
    assert!(
        sorted.windows(2).all(|pair| pair[0] <= pair[1]),
        "{case}: materialize_count did not sort"
    );
    assert_eq!(sorted, got, "{case}: materialize_count changed the rows");
    assert_eq!(count, distinct.len() as u64, "{case}: the distinct count");
    Facts {
        wider: width > prepared.head_len(),
        bag_len: bag.len(),
        distinct_len: distinct.len(),
    }
}

/// Every shape over `SEEDS` random relations, each under every planner.
/// Returns how many cases held a duplicate full assignment in the oracle's
/// bag. The final bounds catch a generator that stops producing answers
/// (every comparison would be `[] == []`) and a suite whose assignments are
/// never wider than the head.
fn check_cell<R: Relation + Cardinality, JA: JoinAlgo<R>>() -> usize {
    let (mut total, mut with_rows, mut wider, mut duplicated) = (0, 0, 0, 0);
    for (index, shape) in shapes().iter().enumerate() {
        let query: JoinQuery = shape.query.parse().expect("a shape parses");
        for seed in SEEDS {
            let relations = random_data(&mut Lcg(1000 * index as u64 + seed), shape.arities);
            let catalog = catalog_at(&relations, StatisticsLevel::ColumnDistinct);
            for (label, planner) in planners(&query) {
                let case = format!("{} seed {seed} under {label}", shape.name);
                let facts = check_case::<R, JA>(&relations, &catalog, &query, &planner, &case);
                total += 1;
                with_rows += usize::from(facts.bag_len > 0);
                wider += usize::from(facts.wider);
                duplicated += usize::from(facts.distinct_len < facts.bag_len);
            }
        }
    }
    assert!(
        with_rows * 4 >= total,
        "only {with_rows} of {total} cases returned a row"
    );
    assert!(
        wider > 0,
        "no case had a body-only variable, so the head was never narrower than the assignment"
    );
    duplicated
}

/// An atom of only constants matching a tuple held `copies` times (once for a
/// set, three times for the hash trie's bag): the bag holds `2 × copies`
/// assignments, the distinct count is 2, so a count that forgot to
/// deduplicate (or deduplicated the wrong thing) fails here. The random
/// shapes' `r1(c0, c1)` matches at most one row.
fn check_duplicated_constant_atom<R: Relation + Cardinality, JA: JoinAlgo<R>>(copies: usize) {
    let relations: Relations = vec![
        ("r0".to_string(), 1, vec![vec![1], vec![2]]),
        ("r1".to_string(), 2, vec![
            vec![0, 1],
            vec![0, 1],
            vec![0, 1],
            vec![0, 2],
        ]),
    ];
    let query: JoinQuery = "Q(X) :- r0(X), r1(c0, c1).".parse().unwrap();
    let catalog = catalog_at(&relations, StatisticsLevel::ColumnDistinct);
    for (label, planner) in planners(&query) {
        let case = format!("duplicated constant atom under {label}");
        let facts = check_case::<R, JA>(&relations, &catalog, &query, &planner, &case);
        assert_eq!(facts.bag_len, 2 * copies, "{case}: the oracle's bag");
        assert_eq!(facts.distinct_len, 2, "{case}: the distinct rows");
    }
}

#[test]
fn tree_trie_materializes_the_oracles_assignments() {
    assert_eq!(
        check_cell::<TreeTrie, LeapfrogTriejoin>(),
        0,
        "a set holds no duplicate full assignment"
    );
}

#[test]
fn column_trie_materializes_the_oracles_assignments() {
    assert_eq!(
        check_cell::<ColumnTrie, LeapfrogTriejoin>(),
        0,
        "a set holds no duplicate full assignment"
    );
}

#[test]
fn hash_trie_materializes_the_oracles_assignments() {
    assert!(
        check_cell::<HashTrie<SipHashStrategy>, HashTriejoin>() > 0,
        "the random relations hold duplicate rows, which a bag answers twice"
    );
}

#[test]
fn tree_trie_counts_a_duplicated_constant_atom_once() {
    check_duplicated_constant_atom::<TreeTrie, LeapfrogTriejoin>(1);
}

#[test]
fn column_trie_counts_a_duplicated_constant_atom_once() {
    check_duplicated_constant_atom::<ColumnTrie, LeapfrogTriejoin>(1);
}

#[test]
fn hash_trie_counts_a_duplicated_constant_atom_once_after_deduplicating() {
    check_duplicated_constant_atom::<HashTrie<SipHashStrategy>, HashTriejoin>(3);
}
```

Then format: `nix develop --command cargo fmt --all`.

- [ ] **Step 2: Run it to see it fail**

The functions exist (Task 2.1), so first prove the oracle can fail. Temporarily delete the line `    rows.sort();` from the body of `materialize_count` in `kermit/src/db/lifecycle.rs`, and check the mutant applied: `grep -n 'pub fn materialize_count' -A2 kermit/src/db/lifecycle.rs` shows `let mut distinct = 0u64;` directly after the signature.

Run: `nix develop --command cargo test -p kermit --test materialize_oracle`

Expected: `tree_trie_materializes_the_oracles_assignments`, `column_trie_materializes_the_oracles_assignments` and `hash_trie_materializes_the_oracles_assignments` fail with a message of the form ``<shape> seed <n> under <planner>: materialize_count did not sort`` (the reversed order emits rows out of order), or ``the distinct count`` where the adjacent-distinct count of an unsorted batch is too high.

Restore the line `    rows.sort();` (first line of `materialize_count`'s body, before `let mut distinct = 0u64;`) and check with `git diff --stat kermit/src/db/lifecycle.rs`, which prints nothing.

- [ ] **Step 3: Implement**

Nothing to add: the test exercises Task 2.1's code. If Step 2's mutant did not fail, the test is vacuous; fix the test, not the library.

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --test materialize_oracle`

Expected: 6 tests pass.

Then run the story's gates for the package:
`nix develop --command cargo fmt --all -- --check`, then `nix develop --command cargo clippy -p kermit --all-targets -- -D warnings`, then `nix develop --command cargo test -p kermit --lib`, then `nix develop --command cargo test -p kermit --test all_orders --test materialize_oracle`.

Expected: all pass with no warnings.

- [ ] **Step 5: Commit**

```bash
git add -N kermit/tests/materialize_oracle.rs
git commit -m "test: materialize on every structure against the nested-loop oracle" -- kermit/tests/materialize_oracle.rs
```

## Story 3: The `materialize` metric in the binary.

- **Developer:** intermediate-dev
- **Files:** `kermit/src/bench/mod.rs`, `kermit/src/bench/measure.rs`, `kermit/src/bench/run.rs`, `kermit/src/bench/ds.rs`, `kermit/src/main.rs`, `kermit/src/execution.rs`, `kermit/tests/cli_materialize.rs`
- **Interfaces:** exposes `Subject` (new shape), `Materialized`, `Metric::Materialize`, `RunSettings::materialize_max_bytes`, `materialize_bytes` and `MaterializeArgs` (Names contract: the measure layer) to Stories 4 and 10; uses Story 2's `execute_assignments` and `materialize_count`
- **Verify:** `bash -c 'cargo test -p kermit --bin kermit && cargo test -p kermit --test cli_materialize'`
- **Review:** deep

Adds `Metric::Materialize` and the new `Subject` shape (the `Built<'s>` GAT, `STREAMS`, `MATERIALIZE_MUTATES`, `materialize`). `QuerySubject` materialises through Story 2's `execute_assignments` and `materialize_count`: every full assignment is pushed into a `Tuples`, sorted, and the distinct rows counted, and the sorted batch is returned so Criterion frees it after the timer stops. `Described` runs `materialize` on the describe set beside `iteration` when the subject's materialize does not mutate it (a fresh build per sample when it does), records a probe's single-run value, and counts through `materialize` for a subject that does not stream. A subject whose `materialize` mutates runs it once per build: for one that does not stream, the describe step's count is that run and `Described::materialize_once` returns it; for a streaming one, `materialize_once` runs on a fresh build. The runner adds the memory guard, decided by one function, `materialize_guard`, from the subject type's `STREAMS` (a subject that does not stream is guarded before the describe step, from `expected`), and its note, `--verify` on the `materialize` count, `--materialize-max-bytes` with its bounds, `bench probe --materialize`, and `bench ds`'s rejection. A run that does not ask for `materialize` reports exactly as before. No landmine marker appears in this story's code.

`kermit/src/execution.rs` is in the **Files** line because `QuerySubject<F>` is generic over `ExecutionFamily`, and only the two impls there name the join algorithm, so a provided method cannot call `db::execute_assignments`. Task 3.1 adds a required `ExecutionFamily::execute_assignments` beside `execute`, with the `TrieLftj` and `HashHtj` impls; it is private to the binary and no other story shares it.

### Task 3.1: The measure layer

**Files:**
- Modify: `kermit/src/bench/mod.rs:29-41` (`Metric`), tests
- Modify: `kermit/src/execution.rs:482-490`, `:672-679`, `:730-740` (`ExecutionFamily::execute_assignments`)
- Modify: `kermit/src/bench/measure.rs:8-22`, `:24-100`, `:154-169`, `:195-366`, and its tests
- Test: `kermit/src/bench/mod.rs`, `kermit/src/bench/measure.rs`

- [ ] **Step 1: Write the failing tests**

(a) In `kermit/src/bench/mod.rs`, in `mod tests` (which has `use {super::*, std::time::Duration};`), add:

```rust
    #[test]
    fn materialize_is_a_timed_opt_in_metric_named_on_the_command_line() {
        use clap::ValueEnum;
        assert!(Metric::Materialize.is_timed());
        assert!(Metric::EndToEnd.is_timed());
        assert!(!Metric::Space.is_timed());
        assert_eq!(
            Metric::Materialize.to_possible_value().unwrap().get_name(),
            "materialize"
        );
        assert_eq!(
            Metric::EndToEnd.to_possible_value().unwrap().get_name(),
            "end-to-end"
        );
    }
```

(b) In `kermit/src/bench/measure.rs`'s `mod tests`, make the test subject follow the new `Subject` shape and add tests. First the event log:

Current (lines 380-386):

```rust
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Event {
        Build(usize),
        Bytes(usize),
        Exercise(usize),
        Drop(usize),
    }
```

Replace with:

```rust
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Event {
        Build(usize),
        Bytes(usize),
        Exercise(usize),
        Materialize(usize),
        Drop(usize),
    }
```

The `Recording` subject takes two more flags, defaulted to a streaming subject whose materialize does not mutate, so every existing `Recording::<false>` / `Recording::<true>` keeps its meaning.

Current (lines 402-413):

```rust
    /// A subject that logs every call. A build while another set is alive
    /// panics, which pins the at-most-one-set rule wherever Criterion calls
    /// it.
    struct Recording<const MUTATES: bool> {
        log: Rc<RefCell<Vec<Event>>>,
        live: Rc<Cell<usize>>,
        next: Cell<usize>,
        bytes: Vec<usize>,
    }

    impl<const MUTATES: bool> Recording<MUTATES> {
        fn new(bytes: Vec<usize>) -> Self {
```

Replace with:

```rust
    /// A subject that logs every call. A build while another set is alive
    /// panics, which pins the at-most-one-set rule wherever Criterion calls
    /// it. `MUTATES` is its `EXERCISE_MUTATES`, `STREAMING` its `STREAMS` and
    /// `MATERIALIZE_MUTATING` its `MATERIALIZE_MUTATES`.
    struct Recording<
        const MUTATES: bool,
        const STREAMING: bool = true,
        const MATERIALIZE_MUTATING: bool = false,
    > {
        log: Rc<RefCell<Vec<Event>>>,
        live: Rc<Cell<usize>>,
        next: Cell<usize>,
        bytes: Vec<usize>,
    }

    impl<const MUTATES: bool, const STREAMING: bool, const MATERIALIZE_MUTATING: bool>
        Recording<MUTATES, STREAMING, MATERIALIZE_MUTATING>
    {
        fn new(bytes: Vec<usize>) -> Self {
```

Current (lines 425-430):

```rust
    impl<const MUTATES: bool> Subject for Recording<MUTATES> {
        type Built = Tracked;

        const EXERCISE_MUTATES: bool = MUTATES;

        fn build(&self) -> Tracked {
```

Replace with:

```rust
    impl<const MUTATES: bool, const STREAMING: bool, const MATERIALIZE_MUTATING: bool> Subject
        for Recording<MUTATES, STREAMING, MATERIALIZE_MUTATING>
    {
        type Built<'s>
            = Tracked
        where
            Self: 's;

        const EXERCISE_MUTATES: bool = MUTATES;
        const STREAMS: bool = STREAMING;
        const MATERIALIZE_MUTATES: bool = MATERIALIZE_MUTATING;

        fn build(&self) -> Tracked {
```

Current (lines 452-456):

```rust
        fn exercise(&self, built: &Tracked) -> u64 {
            self.log.borrow_mut().push(Event::Exercise(built.id));
            7
        }
    }
```

Replace with:

```rust
        fn exercise(&self, built: &Tracked) -> u64 {
            self.log.borrow_mut().push(Event::Exercise(built.id));
            7
        }

        fn materialize(&self, built: &Tracked) -> Option<Materialized> {
            self.log.borrow_mut().push(Event::Materialize(built.id));
            Some(Materialized {
                rows: 5,
                kept: None,
            })
        }
    }
```

Now the new `Described` tests. Insert them after `the_prefix_names_every_function` (before the `use { crate::execution::{HashHtj, SortedTrieFamily, TrieLftj}, ...` block):

```rust
    #[test]
    fn materialize_runs_on_the_describe_set_before_it_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let subject = Recording::<false>::new(vec![8]);
        let metrics = [Metric::Materialize, Metric::Build];
        let measured =
            Described::new(&subject, false).measure(&request(&metrics, false, &settings));
        let events = subject.events();
        let dropped = first(&events, |e| *e == Event::Drop(0));
        assert!(events[..dropped].contains(&Event::Materialize(0)));
        assert!(first(&events, |e| matches!(e, Event::Build(id) if *id > 0)) > dropped);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::Materialize(id) if *id > 0)),
            "a materialize that does not mutate never needs a fresh build"
        );
        let functions: Vec<&str> = measured
            .criterion_groups
            .iter()
            .map(|g| g.function.as_str())
            .collect();
        assert_eq!(functions, ["materialize", "build"]);
    }

    #[test]
    fn a_mutating_materialize_runs_on_fresh_builds_after_the_describe_set_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let subject = Recording::<false, true, true>::new(vec![8]);
        Described::new(&subject, false).measure(&request(&[Metric::Materialize], false, &settings));
        let events = subject.events();
        let dropped = first(&events, |e| *e == Event::Drop(0));
        assert!(!events.contains(&Event::Materialize(0)));
        assert!(first(&events, |e| matches!(e, Event::Materialize(id) if *id > 0)) > dropped);
    }

    #[test]
    fn a_single_run_records_materialize_as_a_value_and_runs_no_criterion_group() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let subject = Recording::<false>::new(vec![1]);
        let mut described = Described::new(&subject, true);
        assert_eq!(described.materialize_time(), None);
        assert_eq!(described.materialize_once(), Some(5));
        assert!(described.materialize_time().is_some());
        let metrics = [Metric::Materialize];
        let measured = described.measure(&request(&metrics, true, &settings));
        assert!(measured.criterion_groups.is_empty());
        let single: Vec<(&str, ReportMetric, Estimator)> = measured
            .values
            .iter()
            .map(|v| (v.function.as_str(), v.metric, v.estimator))
            .collect();
        assert_eq!(single, [
            ("build", ReportMetric::Time, Estimator::Single),
            ("iteration", ReportMetric::Time, Estimator::Single),
            ("materialize", ReportMetric::Time, Estimator::Single),
        ]);
        let builds = subject
            .events()
            .iter()
            .filter(|e| matches!(e, Event::Build(_)))
            .count();
        assert_eq!(builds, 1, "Criterion never started");
    }

    #[test]
    fn a_subject_that_does_not_stream_counts_through_materialize() {
        let subject = Recording::<false, false>::new(vec![1]);
        let described = Described::new(&subject, true);
        assert_eq!(subject.events(), [
            Event::Build(0),
            Event::Bytes(0),
            Event::Materialize(0)
        ]);
        assert_eq!(described.rows(), Some(5));
    }

    #[test]
    fn a_subject_that_does_not_stream_has_no_iteration_or_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let subject = Recording::<false, false>::new(vec![1]);
        // A single run records `build` and no `iteration` value.
        let measured = Described::new(&subject, true).measure(&request(&[], true, &settings));
        let functions: Vec<&str> = measured
            .values
            .iter()
            .map(|v| v.function.as_str())
            .collect();
        assert_eq!(functions, ["build"]);

        // Neither Criterion function runs, and `exercise` is never called.
        let subject = Recording::<false, false>::new(vec![1]);
        let metrics = [Metric::Iteration, Metric::EndToEnd, Metric::Build];
        let measured =
            Described::new(&subject, false).measure(&request(&metrics, false, &settings));
        let functions: Vec<&str> = measured
            .criterion_groups
            .iter()
            .map(|g| g.function.as_str())
            .collect();
        assert_eq!(functions, ["build"]);
        assert!(!subject
            .events()
            .iter()
            .any(|e| matches!(e, Event::Exercise(_))));
    }

    /// Nemo panics on a second `materialize` on one build, so a subject that
    /// does not stream and whose `materialize` mutates (`Recording::<false,
    /// false, true>`) must see exactly one per build.
    #[test]
    fn a_mutating_materialize_runs_once_per_build_on_the_single_run_and_verify_paths() {
        let dir = tempfile::tempdir().unwrap();
        let settings = quick(&dir);
        let metrics = [Metric::Materialize];
        let materializes = |events: &[Event], id: usize| {
            events
                .iter()
                .filter(|e| **e == Event::Materialize(id))
                .count()
        };
        let builds = |events: &[Event]| {
            events
                .iter()
                .filter(|e| matches!(e, Event::Build(_)))
                .count()
        };

        // A single run: the describe step's count is the one `materialize`,
        // and `materialize_once` returns it, however often it is asked.
        let subject = Recording::<false, false, true>::new(vec![1]);
        let mut described = Described::new(&subject, true);
        assert_eq!(described.materialize_once(), Some(5));
        assert_eq!(described.materialize_once(), Some(5));
        assert!(described.materialize_time().is_some());
        let measured = described.measure(&request(&metrics, true, &settings));
        assert!(measured.criterion_groups.is_empty());
        let functions: Vec<&str> = measured
            .values
            .iter()
            .map(|v| v.function.as_str())
            .collect();
        assert_eq!(functions, ["build", "materialize"]);
        let events = subject.events();
        assert_eq!(builds(&events), 1);
        assert_eq!(materializes(&events, 0), 1);

        // `--verify` and then Criterion: the describe set's one `materialize`
        // is the check, and each fresh build Criterion makes gets one.
        let subject = Recording::<false, false, true>::new(vec![1]);
        let mut described = Described::new(&subject, true);
        assert_eq!(described.materialize_once(), Some(5));
        let measured = described.measure(&request(&metrics, false, &settings));
        assert_eq!(measured.criterion_groups.len(), 1);
        let events = subject.events();
        assert!(builds(&events) > 1, "Criterion built fresh sets");
        assert_eq!(materializes(&events, 0), 1);
        for id in 1..builds(&events) {
            assert_eq!(materializes(&events, id), 1, "fresh build {id}");
        }
    }
```

(c) The `QuerySubject` and `RelationSubject` tests. Add `EagerExpansion` to the test imports.

Current (lines 630-635):

```rust
    use {
        crate::execution::{HashHtj, SortedTrieFamily, TrieLftj},
        kermit::db::{Catalog, Prepared},
        kermit_ds::{HeapSize, LazyExpansion, NoPruning, RelationHeader, Tuples},
        kermit_iters::SipHashStrategy,
    };
```

Replace with:

```rust
    use {
        crate::execution::{HashHtj, SortedTrieFamily, TrieLftj},
        kermit::db::{Catalog, Prepared},
        kermit_ds::{EagerExpansion, HeapSize, LazyExpansion, NoPruning, RelationHeader, Tuples},
        kermit_iters::SipHashStrategy,
    };
```

The two-hop helper takes the relation, so a test can hold a duplicate row.

Current (lines 646-656):

```rust
    /// The two-hop paths over [`edges`], prepared under the lexicographic
    /// optimiser.
    fn two_hops() -> (Catalog, Prepared) {
        use kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel};
        let catalog = Catalog::new(vec![edges()], StatisticsLevel::TupleCounts);
        let query: JoinQuery = "Q(X, Z) :- edge(X, Y), edge(Y, Z).".parse().unwrap();
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared =
            kermit::db::prepare(&catalog, &query, &planner, kermit_ds::TrieLevels::All).unwrap();
        (catalog, prepared)
    }
```

Replace with:

```rust
    /// `edge` with the row 1 → 2 held twice (a set keeps one copy, a bag
    /// both), then 2 → 3.
    fn duplicated_edges() -> (RelationHeader, Tuples) {
        let mut tuples = Tuples::new(2);
        for row in [[1, 2], [1, 2], [2, 3]] {
            tuples.push(&row);
        }
        (RelationHeader::new_positional("edge", 2), tuples)
    }

    /// The two-hop paths over [`edges`], prepared under the lexicographic
    /// optimiser.
    fn two_hops() -> (Catalog, Prepared) { two_hops_over(edges()) }

    /// The two-hop paths over `edge`, prepared under the lexicographic
    /// optimiser.
    fn two_hops_over(edge: (RelationHeader, Tuples)) -> (Catalog, Prepared) {
        use kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel};
        let catalog = Catalog::new(vec![edge], StatisticsLevel::TupleCounts);
        let query: JoinQuery = "Q(X, Z) :- edge(X, Y), edge(Y, Z).".parse().unwrap();
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared =
            kermit::db::prepare(&catalog, &query, &planner, kermit_ds::TrieLevels::All).unwrap();
        (catalog, prepared)
    }
```

Add these tests after `a_lazy_hash_trie_query_subject_mutates`:

```rust
    #[test]
    fn a_query_subject_streams_and_materialize_mutates_with_its_joins() {
        const {
            assert!(<QuerySubject<'static, TrieLftj<kermit_ds::TreeTrie>> as Subject>::STREAMS);
            assert!(
                !<QuerySubject<'static, TrieLftj<kermit_ds::TreeTrie>> as Subject>::MATERIALIZE_MUTATES
            );
            assert!(
                <QuerySubject<'static, HashHtj<SipHashStrategy, NoPruning, LazyExpansion>> as Subject>::MATERIALIZE_MUTATES
            );
        };
    }

    /// `Q(X, Z) :- edge(X, Y), edge(Y, Z)` has the head `X, Z` and the
    /// body-only `Y`, so an assignment is `(X, Z, Y)`: `(1, 3, 2)`.
    #[test]
    fn a_query_subject_materializes_the_sorted_full_assignments() {
        let (catalog, prepared) = two_hops();
        let family = TrieLftj::<kermit_ds::TreeTrie>::new(Default::default());
        let subject = QuerySubject {
            family: &family,
            catalog: &catalog,
            prepared: &prepared,
        };
        let built = subject.build();
        let materialized = subject
            .materialize(&built)
            .expect("a query subject materializes");
        assert_eq!(materialized.rows, 1, "only 1 → 2 → 3");
        let kept = materialized.kept.expect("the sorted batch is kept");
        assert_eq!((kept.arity(), kept.to_vecs()), (3, vec![vec![1, 3, 2]]));
    }

    /// The bag answers `(1, 3, 2)` twice; the distinct count says one, and the
    /// sorted batch it keeps still holds both rows. A set holds one copy.
    #[test]
    fn the_distinct_count_removes_the_duplicate_assignments_a_bag_keeps() {
        let (catalog, prepared) = two_hops_over(duplicated_edges());
        let hash = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
            Default::default(),
            Default::default(),
        );
        let subject = QuerySubject {
            family: &hash,
            catalog: &catalog,
            prepared: &prepared,
        };
        let materialized = subject
            .materialize(&subject.build())
            .expect("a query subject materializes");
        assert_eq!(materialized.rows, 1);
        assert_eq!(materialized.kept.expect("kept").len(), 2);

        let tree = TrieLftj::<kermit_ds::TreeTrie>::new(Default::default());
        let subject = QuerySubject {
            family: &tree,
            catalog: &catalog,
            prepared: &prepared,
        };
        let materialized = subject
            .materialize(&subject.build())
            .expect("a query subject materializes");
        assert_eq!(materialized.rows, 1);
        assert_eq!(materialized.kept.expect("kept").len(), 1);
    }
```

A streaming subject whose `materialize` mutates (a lazy `HashTrie`) runs `Described::materialize_once` on a fresh build, so the describe set stays as built:

```rust
    #[test]
    fn a_lazy_query_subject_materializes_once_on_a_fresh_build() {
        let (catalog, prepared) = two_hops();
        let family = HashHtj::<SipHashStrategy, NoPruning, LazyExpansion>::new(
            Default::default(),
            Default::default(),
        );
        let subject = QuerySubject {
            family: &family,
            catalog: &catalog,
            prepared: &prepared,
        };
        let mut described = Described::new(&subject, false);
        assert_eq!(described.materialize_once(), Some(1));
        assert!(described.materialize_time().is_some());
        assert_eq!(
            subject.heap_bytes(described.built()),
            described.bytes(),
            "the describe set was not expanded by the materialize"
        );
    }
```

(d) Add to `a_relation_subject_builds_the_relation_and_scans_it`'s neighbours:

```rust
    #[test]
    fn a_relation_subject_has_no_join_to_materialize() {
        let (header, tuples) = edges();
        let family = SortedTrieFamily::<kermit_ds::TreeTrie>::default();
        let subject = RelationSubject {
            family: &family,
            header: &header,
            tuples: &tuples,
        };
        let built = subject.build();
        assert!(subject.materialize(&built).is_none());
        const {
            assert!(<RelationSubject<'static, SortedTrieFamily<kermit_ds::TreeTrie>> as Subject>::STREAMS)
        };
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::`

Expected: the binary's tests fail to compile. The errors include ``error[E0599]: no variant or associated item named `Materialize` found for enum `Metric` ``, ``error[E0412]: cannot find type `Materialized` in this scope``, ``error[E0407]: method `materialize` is not a member of trait `Subject` `` and ``error[E0437]: type `Built` is not a member of trait `Subject` `` (the test subject now declares the GAT).

- [ ] **Step 3: Implement**

**(a) `Metric::Materialize`.** In `kermit/src/bench/mod.rs`:

Current (lines 35-41):

```rust
    /// Build + K × query in one timed body (`T = build + K × query`), with
    /// K set by `--queries-per-build`. Deliberately NOT in the `--metrics`
    /// default set: a fresh build per Criterion sample
    /// (`BatchSize::PerIteration`) is expensive, and existing invocations
    /// must keep their historical measurement surface.
    EndToEnd,
}
```

Replace with:

```rust
    /// Build + K × query in one timed body (`T = build + K × query`), with
    /// K set by `--queries-per-build`. Deliberately NOT in the `--metrics`
    /// default set: a fresh build per Criterion sample
    /// (`BatchSize::PerIteration`) is expensive, and existing invocations
    /// must keep their historical measurement surface.
    EndToEnd,
    /// The join with its result kept: every full assignment of the executed
    /// query pushed into a flat batch, sorted, and the distinct rows counted.
    /// Opt-in like `end-to-end`, because streaming stays the baseline
    /// (`iteration`), and guarded by `--materialize-max-bytes`, since the
    /// result can dwarf memory. `bench ds` has no join and refuses it.
    Materialize,
}
```

`is_timed` (`!matches!(self, Metric::Space)`) already counts it.

**(b) `ExecutionFamily::execute_assignments`.** In `kermit/src/execution.rs`:

Current (lines 488-490):

```rust
    fn execute<S: FnMut(&[usize])>(
        &self, prepared: &Prepared, indexes: &Indexes<Self::Rel>, emit: S,
    );
```

Replace with:

```rust
    fn execute<S: FnMut(&[usize])>(
        &self, prepared: &Prepared, indexes: &Indexes<Self::Rel>, emit: S,
    );

    /// [`execute`](Self::execute), lending every variable of the executed
    /// query instead of the head ([`kermit::db::execute_assignments`]): the
    /// rows the `materialize` metric keeps. Like `execute`, it expands the
    /// indexes it is given when [`JOIN_MUTATES`](Self::JOIN_MUTATES).
    fn execute_assignments<S: FnMut(&[usize])>(
        &self, prepared: &Prepared, indexes: &Indexes<Self::Rel>, emit: S,
    );
```

Current (lines 676-679):

```rust
    fn execute<S: FnMut(&[usize])>(&self, prepared: &Prepared, indexes: &Indexes<R>, emit: S) {
        db::execute::<R, LeapfrogTriejoin>(prepared, indexes, emit);
    }
}
```

Replace with:

```rust
    fn execute<S: FnMut(&[usize])>(&self, prepared: &Prepared, indexes: &Indexes<R>, emit: S) {
        db::execute::<R, LeapfrogTriejoin>(prepared, indexes, emit);
    }

    fn execute_assignments<S: FnMut(&[usize])>(
        &self, prepared: &Prepared, indexes: &Indexes<R>, emit: S,
    ) {
        db::execute_assignments::<R, LeapfrogTriejoin>(prepared, indexes, emit);
    }
}
```

Current (lines 734-740):

```rust
    fn execute<S: FnMut(&[usize])>(
        &self, prepared: &Prepared, indexes: &Indexes<HashTrie<H, P, E, L>>, emit: S,
    ) {
        db::execute::<HashTrie<H, P, E, L>, HashTriejoin>(prepared, indexes, emit);
    }
}
```

Replace with:

```rust
    fn execute<S: FnMut(&[usize])>(
        &self, prepared: &Prepared, indexes: &Indexes<HashTrie<H, P, E, L>>, emit: S,
    ) {
        db::execute::<HashTrie<H, P, E, L>, HashTriejoin>(prepared, indexes, emit);
    }

    fn execute_assignments<S: FnMut(&[usize])>(
        &self, prepared: &Prepared, indexes: &Indexes<HashTrie<H, P, E, L>>, emit: S,
    ) {
        db::execute_assignments::<HashTrie<H, P, E, L>, HashTriejoin>(prepared, indexes, emit);
    }
}
```

**(c) `kermit/src/bench/measure.rs`.** First the import of the shared sort and count.

Current (line 16):

```rust
    kermit::db::{Catalog, Indexes, Prepared},
```

Replace with:

```rust
    kermit::db::{materialize_count, Catalog, Indexes, Prepared},
```

Now the trait and the two subjects.

Current (lines 24-33):

```rust
/// What a bench subcommand measures: something built from flat tuples and
/// then exercised.
pub(crate) trait Subject {
    /// What one build produces: a query's indexes, or one relation.
    type Built;

    /// Whether [`exercise`](Self::exercise) changes what it exercises (a
    /// lazy `HashTrie` builds the children a join reaches). Then `iteration`
    /// runs on a fresh build per sample, never on the describe set.
    const EXERCISE_MUTATES: bool;
```

Replace the whole region from this doc comment through the closing `}` of `impl<F: RelationFamily> Subject for RelationSubject<'_, F>` (lines 24-100) with:

```rust
/// What a bench subcommand measures: something built from flat tuples and
/// then exercised.
pub(crate) trait Subject {
    /// What one build produces: a query's indexes, one relation, or an
    /// engine's loaded tables. It may borrow the subject.
    type Built<'s>
    where
        Self: 's;

    /// Whether [`exercise`](Self::exercise) changes what it exercises (a
    /// lazy `HashTrie` builds the children a join reaches). Then `iteration`
    /// runs on a fresh build per sample, never on the describe set.
    const EXERCISE_MUTATES: bool;

    /// Whether [`exercise`](Self::exercise) streams its rows. When false the
    /// subject has no `iteration` or `end_to_end` (`exercise` is never
    /// called), and the describe step counts through
    /// [`materialize`](Self::materialize) instead.
    const STREAMS: bool;

    /// Whether [`materialize`](Self::materialize) changes the build it runs
    /// on (Nemo stores its result beside the tables; a lazy `HashTrie`
    /// expands the children the join reaches). Then `materialize` runs on a
    /// fresh build per sample, never on the describe set, and exactly once on
    /// any one build: a second run on a build can panic (Nemo's does). A
    /// subject that does not stream ([`STREAMS`](Self::STREAMS)) has its one
    /// `materialize` on the describe set as the describe step's count, and
    /// [`Described::materialize_once`] returns that run instead of running
    /// again; one that streams has `materialize_once` run it on a fresh
    /// build.
    const MATERIALIZE_MUTATES: bool;

    /// One build: the timed body of `build`, and the build half of
    /// `end_to_end`.
    fn build(&self) -> Self::Built<'_>;

    /// The heap bytes of each part of `built` (each index, or the one
    /// relation), as built.
    fn heap_bytes(&self, built: &Self::Built<'_>) -> Vec<usize>;

    /// One traversal of `built` (a join's rows counted, or a relation
    /// scanned), returning how many rows it visited: the timed body of
    /// `iteration`, and K times in `end_to_end`.
    fn exercise(&self, built: &Self::Built<'_>) -> u64;

    /// The join with its result kept (see [`Materialized`]): the timed body of
    /// `materialize`. `None` for a subject with no join
    /// ([`RelationSubject`]).
    fn materialize(&self, built: &Self::Built<'_>) -> Option<Materialized>;
}

/// What one [`Subject::materialize`] produced.
pub(crate) struct Materialized {
    /// The distinct full assignments.
    pub(crate) rows: u64,
    /// The sorted batch (kermit and DuckDB), returned so that Criterion frees
    /// it after the timer stops. `None` where the engine keeps its result
    /// itself.
    pub(crate) kept: Option<Tuples>,
}

/// A prepared query on one execution cell. A build is the query's indexes,
/// each copied once out of the catalog's flat tuples, an exercise counts the
/// join's rows, and a materialize keeps them.
pub(crate) struct QuerySubject<'a, F> {
    pub(crate) family: &'a F,
    pub(crate) catalog: &'a Catalog,
    pub(crate) prepared: &'a Prepared,
}

impl<F: ExecutionFamily> Subject for QuerySubject<'_, F> {
    type Built<'s>
        = Indexes<F::Rel>
    where
        Self: 's;

    const EXERCISE_MUTATES: bool = F::JOIN_MUTATES;
    const STREAMS: bool = true;
    /// The join `materialize` runs is the one `iteration` runs, so it expands
    /// a lazy trie as well.
    const MATERIALIZE_MUTATES: bool = F::JOIN_MUTATES;

    fn build(&self) -> Indexes<F::Rel> { self.family.build_indexes(self.catalog, self.prepared) }

    fn heap_bytes(&self, built: &Indexes<F::Rel>) -> Vec<usize> {
        built
            .iter()
            .map(|(_, index)| index.heap_size_bytes())
            .collect()
    }

    fn exercise(&self, built: &Indexes<F::Rel>) -> u64 {
        self.family.count_rows(self.prepared, built)
    }

    /// Every full assignment of the executed query pushed into a batch of
    /// `Prepared::assignment_width` columns, then `materialize_count`'s sort
    /// and distinct count. The memory guard bounds the batch before this runs.
    ///
    /// # Panics
    ///
    /// If the answer has more than `RowId::MAX` rows, the most a `Tuples`
    /// holds.
    fn materialize(&self, built: &Indexes<F::Rel>) -> Option<Materialized> {
        let mut rows = Tuples::new(self.prepared.assignment_width());
        self.family
            .execute_assignments(self.prepared, built, |row| rows.push(row));
        let (sorted, distinct) = materialize_count(rows);
        Some(Materialized {
            rows: distinct,
            kept: Some(sorted),
        })
    }
}

/// One relation on one structure: `bench ds`'s subject. A build is the
/// relation, from its file's tuples in file order, and an exercise scans it.
/// It has no join, so nothing to materialize.
pub(crate) struct RelationSubject<'a, F> {
    pub(crate) family: &'a F,
    pub(crate) header: &'a RelationHeader,
    pub(crate) tuples: &'a Tuples,
}

impl<F: RelationFamily> Subject for RelationSubject<'_, F> {
    type Built<'s>
        = F::Rel
    where
        Self: 's;

    /// A scan reads an unexpanded child's pending rows and expands nothing
    /// (`HashTrie::for_each_tuple`), so no structure's scan mutates it.
    const EXERCISE_MUTATES: bool = false;
    const STREAMS: bool = true;
    const MATERIALIZE_MUTATES: bool = false;

    fn build(&self) -> F::Rel {
        self.family
            .build_index(self.header.clone(), TupleView::all(self.tuples))
    }

    fn heap_bytes(&self, built: &F::Rel) -> Vec<usize> { vec![built.heap_size_bytes()] }

    fn exercise(&self, built: &F::Rel) -> u64 { F::scan(built) }

    fn materialize(&self, _built: &F::Rel) -> Option<Materialized> { None }
}
```

`MeasureRequest` learns which metrics Criterion times. A single run (`bench probe`) records `materialize` as a value instead.

Current (lines 154-160):

```rust
impl MeasureRequest<'_> {
    fn function_id(&self, phase: &str) -> String {
        match self.function_prefix {
            | Some(prefix) => format!("{prefix}/{phase}"),
            | None => phase.to_string(),
        }
    }
```

Replace with:

```rust
impl MeasureRequest<'_> {
    fn function_id(&self, phase: &str) -> String {
        match self.function_prefix {
            | Some(prefix) => format!("{prefix}/{phase}"),
            | None => phase.to_string(),
        }
    }

    /// Whether `metric` was asked for and runs under Criterion: every timed
    /// metric, except that a single run records `materialize` as a value
    /// (`Described::materialize_once`) and starts no Criterion function.
    fn criterion_runs(&self, metric: Metric) -> bool {
        self.metrics.contains(&metric)
            && metric.is_timed()
            && !(self.single_run && metric == Metric::Materialize)
    }
```

Now `Described`.

Current (lines 191-201):

```rust
/// A subject built once, outside every Criterion timer: the describe step.
///
/// The bytes are read before anything exercises the set, so they are its
/// as-built footprint even when exercising mutates it.
pub(crate) struct Described<'s, S: Subject> {
    subject: &'s S,
    built: S::Built,
    bytes: Vec<usize>,
    build_time: Duration,
    counted: Option<Counted>,
}
```

Replace with:

```rust
/// A subject built once, outside every Criterion timer: the describe step.
///
/// The bytes are read before anything exercises the set, so they are its
/// as-built footprint even when exercising mutates it.
pub(crate) struct Described<'s, S: Subject + 's> {
    subject: &'s S,
    built: S::Built<'s>,
    bytes: Vec<usize>,
    build_time: Duration,
    counted: Option<Counted>,
    /// The one materialize of [`materialize_once`](Self::materialize_once).
    materialized: Option<Counted>,
}
```

Current (lines 203-226):

```rust
impl<'s, S: Subject> Described<'s, S> {
    /// Builds `subject` once, timing the build, then reads its bytes. With
    /// `count`, it then exercises the set once, timing that too.
    pub(crate) fn new(subject: &'s S, count: bool) -> Self {
        let started = Instant::now();
        let built = subject.build();
        let build_time = started.elapsed();
        let bytes = subject.heap_bytes(&built);
        let counted = count.then(|| {
            let started = Instant::now();
            let rows = subject.exercise(&built);
            Counted {
                rows,
                time: started.elapsed(),
            }
        });
        Self {
            subject,
            built,
            bytes,
            build_time,
            counted,
        }
    }

    /// The describe set.
    pub(crate) fn built(&self) -> &S::Built { &self.built }
```

Replace with:

```rust
impl<'s, S: Subject + 's> Described<'s, S> {
    /// Builds `subject` once, timing the build, then reads its bytes. With
    /// `count`, it then counts the set once, timing that too: a streamed
    /// `exercise`, or, for a subject that does not stream
    /// ([`Subject::STREAMS`]), a `materialize`.
    pub(crate) fn new(subject: &'s S, count: bool) -> Self {
        let started = Instant::now();
        let built = subject.build();
        let build_time = started.elapsed();
        let bytes = subject.heap_bytes(&built);
        let counted = count.then(|| {
            let started = Instant::now();
            let rows = if S::STREAMS {
                subject.exercise(&built)
            } else {
                subject
                    .materialize(&built)
                    .expect("a subject that does not stream materializes")
                    .rows
            };
            Counted {
                rows,
                time: started.elapsed(),
            }
        });
        // A subject that does not stream counted through its one `materialize`,
        // so that run is the `materialize` of `materialize_once`.
        let materialized = counted.filter(|_| !S::STREAMS);
        Self {
            subject,
            built,
            bytes,
            build_time,
            counted,
            materialized,
        }
    }

    /// The describe set.
    pub(crate) fn built(&self) -> &S::Built<'s> { &self.built }
```

Current (lines 240-244):

```rust
    /// How long the describe step's exercise took, if it counted.
    pub(crate) fn exercise_time(&self) -> Option<Duration> {
        self.counted.map(|counted| counted.time)
    }
}
```

Replace with:

```rust
    /// How long the describe step's exercise took, if it counted.
    pub(crate) fn exercise_time(&self) -> Option<Duration> {
        self.counted.map(|counted| counted.time)
    }

    /// The distinct row count of one timed `materialize`, outside every
    /// Criterion timer: `--verify`'s check of the `materialize` count, and a
    /// probe's single-run `materialize` value (kept for
    /// [`measure`](Self::measure)). It never runs `materialize` twice on one
    /// build:
    /// - a subject that does not stream returns the describe step's count,
    ///   which was its one `materialize` (`None` if the describe step did not
    ///   count);
    /// - a streaming subject whose `materialize` mutates
    ///   ([`Subject::MATERIALIZE_MUTATES`]) runs it on a fresh build, which is
    ///   dropped, untimed, after the time is taken;
    /// - any other subject runs it on the describe set, which it leaves as it
    ///   was.
    ///
    /// The result is freed after the time is taken. `None` also for a subject
    /// with no join.
    pub(crate) fn materialize_once(&mut self) -> Option<u64> {
        if !S::STREAMS {
            return self.materialized.map(|materialized| materialized.rows);
        }
        let fresh = S::MATERIALIZE_MUTATES.then(|| self.subject.build());
        let started = Instant::now();
        let materialized = match &fresh {
            | Some(fresh) => self.subject.materialize(fresh),
            | None => self.subject.materialize(&self.built),
        }?;
        let time = started.elapsed();
        drop(fresh);
        let rows = materialized.rows;
        drop(materialized);
        self.materialized = Some(Counted {
            rows,
            time,
        });
        Some(rows)
    }

    /// How long [`materialize_once`](Self::materialize_once) took, if it ran.
    pub(crate) fn materialize_time(&self) -> Option<Duration> {
        self.materialized.map(|materialized| materialized.time)
    }
}
```

Now `measure`: it keeps its order (`iteration` on the describe set, then the describe set is dropped, then `build` and `end_to_end`), with `materialize` beside `iteration`: on the describe set when it does not mutate, on a fresh build per sample after the drop when it does.

Current (lines 246-261):

```rust
impl<S: Subject> Described<'_, S> {
    /// Runs `request`'s metrics in the one order every cell follows:
    /// `iteration` on the describe set (on a fresh build per sample when
    /// exercising mutates), then the describe set is dropped, then `build`
    /// and `end_to_end`. Consuming `self` is what keeps any other set from
    /// being alive while a build is timed. Every timed routine returns what
    /// it built, so Criterion frees it after the timer stops.
    pub(crate) fn measure(self, request: &MeasureRequest<'_>) -> Measured {
        let Described {
            subject,
            built,
            bytes,
            build_time,
            counted,
        } = self;
```

Replace with:

```rust
impl<'s, S: Subject + 's> Described<'s, S> {
    /// Runs `request`'s metrics in the one order every cell follows:
    /// `iteration` and `materialize` on the describe set (each on a fresh
    /// build per sample when it mutates), then the describe set is dropped,
    /// then `build` and `end_to_end`. Consuming `self` is what keeps any
    /// other set from being alive while a build is timed. Every timed routine
    /// returns what it built (`materialize`, its sorted batch), so Criterion
    /// frees it after the timer stops. A subject that does not stream has
    /// neither `iteration` nor `end_to_end`.
    pub(crate) fn measure(self, request: &MeasureRequest<'_>) -> Measured {
        let Described {
            subject,
            built,
            bytes,
            build_time,
            counted,
            materialized,
        } = self;
```

Current (lines 274-295):

```rust
        if request.single_run {
            values.push(ValueEntry {
                function: request.function_id("build"),
                metric: ReportMetric::Time,
                estimator: Estimator::Single,
                value: nanos(build_time),
            });
            if let Some(counted) = counted {
                values.push(ValueEntry {
                    function: request.function_id("iteration"),
                    metric: ReportMetric::Time,
                    estimator: Estimator::Single,
                    value: nanos(counted.time),
                });
            }
        }
        if !request.metrics.iter().any(|metric| metric.is_timed()) {
            return Measured {
                criterion_groups: Vec::new(),
                values,
            };
        }
```

Replace with:

```rust
        if request.single_run {
            values.push(ValueEntry {
                function: request.function_id("build"),
                metric: ReportMetric::Time,
                estimator: Estimator::Single,
                value: nanos(build_time),
            });
            if let Some(counted) = counted.filter(|_| S::STREAMS) {
                values.push(ValueEntry {
                    function: request.function_id("iteration"),
                    metric: ReportMetric::Time,
                    estimator: Estimator::Single,
                    value: nanos(counted.time),
                });
            }
            if let Some(materialized) = materialized.filter(|_| {
                request.metrics.contains(&Metric::Materialize)
            }) {
                values.push(ValueEntry {
                    function: request.function_id("materialize"),
                    metric: ReportMetric::Time,
                    estimator: Estimator::Single,
                    value: nanos(materialized.time),
                });
            }
        }
        if !request
            .metrics
            .iter()
            .any(|&metric| request.criterion_runs(metric))
        {
            return Measured {
                criterion_groups: Vec::new(),
                values,
            };
        }
```

Current (lines 297-330):

```rust
        let mut criterion = request.criterion.criterion();
        let mut group = criterion.benchmark_group(request.group);
        let mut criterion_groups = Vec::new();
        let iteration = request.metrics.contains(&Metric::Iteration);
        if iteration && !S::EXERCISE_MUTATES {
            // Over the describe set, built once outside Criterion. The routine
            // counts rows and returns only the count, so memory does not grow
            // with the answer (#65).
            group.bench_function(request.function_id("iteration"), |b| {
                b.iter(|| subject.exercise(&built));
            });
        }
        // From here on, no set but Criterion's own is alive.
        drop(built);
        if iteration && S::EXERCISE_MUTATES {
            // Cold (#92): each timed exercise runs on a set built in the
            // untimed setup, so it pays the expansion it causes instead of
            // finding it done by an earlier sample. The routine hands the set
            // back, so Criterion frees it after the timer stops.
            group.bench_function(request.function_id("iteration"), |b| {
                b.iter_batched(
                    || subject.build(),
                    |fresh| {
                        let rows = subject.exercise(&fresh);
                        (fresh, rows)
                    },
                    BatchSize::PerIteration,
                );
            });
        }
        if iteration {
            criterion_groups.push(request.time_ref("iteration"));
        }
        if request.metrics.contains(&Metric::Build) {
```

Replace with:

```rust
        let mut criterion = request.criterion.criterion();
        let mut group = criterion.benchmark_group(request.group);
        let mut criterion_groups = Vec::new();
        let iteration = S::STREAMS && request.criterion_runs(Metric::Iteration);
        let materialize = request.criterion_runs(Metric::Materialize);
        if iteration && !S::EXERCISE_MUTATES {
            // Over the describe set, built once outside Criterion. The routine
            // counts rows and returns only the count, so memory does not grow
            // with the answer (#65).
            group.bench_function(request.function_id("iteration"), |b| {
                b.iter(|| subject.exercise(&built));
            });
        }
        if materialize && !S::MATERIALIZE_MUTATES {
            // Over the describe set too, with no setup. The routine returns
            // its sorted batch, so Criterion frees it after the timer stops,
            // and the batch is the only other thing alive.
            group.bench_function(request.function_id("materialize"), |b| {
                b.iter_batched(|| (), |()| subject.materialize(&built), BatchSize::PerIteration);
            });
        }
        // From here on, no set but Criterion's own is alive.
        drop(built);
        if iteration && S::EXERCISE_MUTATES {
            // Cold (#92): each timed exercise runs on a set built in the
            // untimed setup, so it pays the expansion it causes instead of
            // finding it done by an earlier sample. The routine hands the set
            // back, so Criterion frees it after the timer stops.
            group.bench_function(request.function_id("iteration"), |b| {
                b.iter_batched(
                    || subject.build(),
                    |fresh| {
                        let rows = subject.exercise(&fresh);
                        (fresh, rows)
                    },
                    BatchSize::PerIteration,
                );
            });
        }
        if materialize && S::MATERIALIZE_MUTATES {
            // Cold, like a mutating `iteration`: the result lives beside the
            // set it was made on, and the routine hands both back.
            group.bench_function(request.function_id("materialize"), |b| {
                b.iter_batched(
                    || subject.build(),
                    |fresh| {
                        let result = subject.materialize(&fresh);
                        (fresh, result)
                    },
                    BatchSize::PerIteration,
                );
            });
        }
        if iteration {
            criterion_groups.push(request.time_ref("iteration"));
        }
        if materialize {
            criterion_groups.push(request.time_ref("materialize"));
        }
        if request.metrics.contains(&Metric::Build) {
```

Current (line 339):

```rust
        if request.metrics.contains(&Metric::EndToEnd) {
```

Replace with (the one occurrence inside `measure`, directly after the `build` block's `criterion_groups.push(request.time_ref("build"));`; `end_to_end` calls `exercise`, which a subject that does not stream has not):

```rust
        if S::STREAMS && request.metrics.contains(&Metric::EndToEnd) {
```

Format: `nix develop --command cargo fmt --all`.

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::`

Expected: every test in `bench::` passes: the new `materialize_*`, `a_*_materialize*`, `a_subject_that_does_not_stream_*`, `a_mutating_materialize_runs_once_per_build_*`, `a_lazy_query_subject_materializes_once_on_a_fresh_build`, `the_distinct_count_*` and `a_relation_subject_has_no_join_to_materialize` tests, and every existing measure-layer test unchanged (their `Recording` defaults keep their meaning).

Then run: `nix develop --command cargo clippy -p kermit --all-targets -- -D warnings`

Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(bench): Metric::Materialize and the Subject materialize shape" -- kermit/src/bench/mod.rs kermit/src/bench/measure.rs kermit/src/execution.rs
```

### Task 3.2: The runner, the memory guard and the CLI

**Files:**
- Create: `kermit/tests/cli_materialize.rs`
- Modify: `kermit/src/bench/run.rs:31-58`, `:78-92`, `:116-269`, and its tests
- Modify: `kermit/src/bench/ds.rs`
- Modify: `kermit/src/main.rs:69-81`, `:186-209`, `:294-299`, `:366-388`, `:768-797`, `:801-806`, `:850-949`, `:974-1027`
- Test: `kermit/tests/cli_materialize.rs`, `kermit/src/bench/run.rs`, `kermit/src/bench/ds.rs`

- [ ] **Step 1: Write the failing tests**

(a) Create `kermit/tests/cli_materialize.rs`:

```rust
//! The opt-in `materialize` metric on kermit's own cells: the join with every
//! full assignment kept, sorted and counted distinct. `bench run` and
//! `bench join` take `--metrics materialize`; `bench probe` takes
//! `--materialize`; `bench ds` has no join and refuses it. The memory guard
//! (`--materialize-max-bytes`) skips it with a note, `--verify` checks its
//! count against `expected`, and a run that does not ask for it reports as
//! before.
//!
//! The workspace's benchmark `probe` reads `e` (0 → 0 .. 0 → 399, so the
//! query `star` has 400 × 400 answers of 3 variables, 3,840,000 bytes) and
//! `dup` (the row 1 → 2 twice, which a bag counts twice and the distinct
//! count once).

mod common;

use {
    common::cli::{bench_join, edge_fixture, kermit_bin, reports_of},
    std::{fs, process::{Command, Output}},
    tempfile::TempDir,
};

const FAN: usize = 400;

/// The workspace and an empty cache directory.
fn workspace() -> (TempDir, TempDir) {
    let workspace = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let data = workspace.path().join("data");
    fs::create_dir_all(&data).unwrap();
    let mut e = String::from("src,dst\n");
    for i in 0..FAN {
        e.push_str(&format!("0,{i}\n"));
    }
    fs::write(data.join("e.csv"), e).unwrap();
    fs::write(data.join("dup.csv"), "src,dst\n1,2\n1,2\n").unwrap();
    let benchmarks = workspace.path().join("benchmarks");
    fs::create_dir_all(&benchmarks).unwrap();
    fs::write(
        benchmarks.join("probe.yml"),
        format!(
            r#"name: probe
description: "the materialize metric"
relations:
  - name: e
    path: "data/e.csv"
  - name: dup
    path: "data/dup.csv"
queries:
  - name: small
    description: "400 answers of 2 variables"
    query: "Q(A, B) :- e(A, B)."
    expected: {FAN}
  - name: star
    description: "160000 answers of 3 variables"
    query: "Q(A, B, C) :- e(A, B), e(A, C)."
    expected: {}
  - name: twice
    description: "a duplicated row: a bag answers it twice"
    query: "Q(X, Y) :- dup(X, Y)."
    expected: 2
"#,
            FAN * FAN
        ),
    )
    .unwrap();
    (workspace, cache)
}

/// `kermit bench <Criterion settings> --report-json <report> <args>` in
/// `workspace`. Returns the output and the parsed report array (empty when
/// none was written).
fn kermit(workspace: &TempDir, cache: &TempDir, args: &[&str]) -> (Output, Vec<serde_json::Value>) {
    let report = workspace.path().join("report.json");
    let output = Command::new(kermit_bin())
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LIB_BACKTRACE", "0")
        .env("KERMIT_WORKSPACE", workspace.path())
        .env("XDG_CACHE_HOME", cache.path())
        .env("CRITERION_HOME", workspace.path().join("criterion"))
        .current_dir(workspace.path())
        .args([
            "bench",
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
            "--report-json",
        ])
        .arg(&report)
        .args(args)
        .output()
        .unwrap();
    let reports = fs::read_to_string(&report)
        .ok()
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap_or_default();
    (output, reports)
}

fn stderr(output: &Output) -> String { String::from_utf8_lossy(&output.stderr).into_owned() }

/// The Criterion function ids of `report`, in order.
fn functions(report: &serde_json::Value) -> Vec<String> {
    report["criterion_groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| group["function"].as_str().unwrap().to_string())
        .collect()
}

/// The `values` entry of `report` named `function`, if any.
fn value<'r>(report: &'r serde_json::Value, function: &str) -> Option<&'r serde_json::Value> {
    report["values"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["function"] == function)
}

/// `bench run probe -q <query> -i <ds> -a <algo>` plus `extra`.
fn run(
    workspace: &TempDir, cache: &TempDir, query: &str, ds: &str, algo: &str, extra: &[&str],
) -> (Output, Vec<serde_json::Value>) {
    let mut args = vec!["run", "probe", "-q", query, "-i", ds, "-a", algo];
    args.extend_from_slice(extra);
    kermit(workspace, cache, &args)
}

#[test]
fn materialize_is_recorded_under_its_function_name() {
    let (workspace, cache) = workspace();
    for (ds, algo) in [
        ("tree-trie", "leapfrog-triejoin"),
        ("hash-trie", "hash-triejoin"),
    ] {
        let (output, reports) = run(&workspace, &cache, "small", ds, algo, &["-m", "materialize"]);
        assert!(output.status.success(), "{ds}: {}", stderr(&output));
        assert_eq!(reports.len(), 1);
        assert_eq!(functions(&reports[0]), ["materialize"], "{ds}");
        let group = &reports[0]["criterion_groups"][0];
        assert_eq!(group["metric"], "time");
        assert!(
            group["group"].as_str().unwrap().contains("/small/"),
            "{group}"
        );
        // The guard needed the row count, so the describe step counted.
        assert_eq!(reports[0]["axes"]["answer_rows"], FAN);
    }
}

#[test]
fn a_run_that_does_not_ask_for_materialize_reports_as_before() {
    let (workspace, cache) = workspace();
    let (output, reports) = run(&workspace, &cache, "small", "tree-trie", "leapfrog-triejoin", &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(functions(&reports[0]), ["iteration", "build"]);
    assert!(value(&reports[0], "materialize").is_none());
    assert!(
        reports[0]["axes"].get("answer_rows").is_none(),
        "nothing counted: {}",
        reports[0]["axes"]
    );
    assert!(!stderr(&output).contains("materialize"), "{}", stderr(&output));
}

#[test]
fn the_guard_skips_a_result_over_the_limit_with_a_note_and_a_metadata_line() {
    let (workspace, cache) = workspace();
    for (ds, algo, label) in [
        ("tree-trie", "leapfrog-triejoin", "TreeTrie/LeapfrogTriejoin"),
        ("hash-trie", "hash-triejoin", "HashTrie/HashTriejoin"),
    ] {
        let (output, reports) = run(&workspace, &cache, "star", ds, algo, &[
            "-m",
            "build",
            "materialize",
            "--materialize-max-bytes",
            "1048576",
        ]);
        let err = stderr(&output);
        assert!(output.status.success(), "{err}");
        assert!(
            err.contains(&format!(
                "note: skipping materialize for probe/star on {label}: 3840000 bytes over \
                 --materialize-max-bytes 1048576"
            )),
            "{err}"
        );
        assert!(err.contains("materialize:"), "{err}");
        assert!(err.contains("skipped: 3840000 bytes over 1048576"), "{err}");
        assert_eq!(functions(&reports[0]), ["build"], "{ds}");
    }
}

#[test]
fn a_result_under_the_limit_runs() {
    let (workspace, cache) = workspace();
    let (output, reports) = run(&workspace, &cache, "star", "tree-trie", "leapfrog-triejoin", &[
        "-m",
        "build",
        "materialize",
    ]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(!err.contains("skipping materialize"), "{err}");
    assert_eq!(functions(&reports[0]), ["materialize", "build"]);
}

#[test]
fn verify_checks_the_materialized_count_against_expected() {
    let (workspace, cache) = workspace();
    let (output, reports) = run(&workspace, &cache, "star", "tree-trie", "leapfrog-triejoin", &[
        "--verify",
        "-m",
        "materialize",
    ]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(err.contains("verified materialize:"), "{err}");
    assert_eq!(reports[0]["axes"]["verified"], true);
}

/// The bag counts the duplicated row twice (`expected: 2` holds), the
/// distinct count once: the check fails, naming both numbers.
#[test]
fn verify_aborts_when_the_distinct_count_differs_from_expected() {
    let (workspace, cache) = workspace();
    let (output, _) = run(&workspace, &cache, "twice", "hash-trie", "hash-triejoin", &[
        "--verify",
        "-m",
        "materialize",
    ]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains(
            "verification failed: benchmark 'probe' query 'twice' on HashTrie/HashTriejoin \
             materialized 1 distinct full assignments, expected 2"
        ),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_limit_is_bounded_to_one_mebibyte_through_one_tebibyte() {
    let (workspace, cache) = workspace();
    let message = "--materialize-max-bytes must be between 1048576 (1 MiB) and 1099511627776 (1 TiB)";
    for bad in ["1048575", "0", "1099511627777", "abc"] {
        let (output, _) = run(&workspace, &cache, "small", "tree-trie", "leapfrog-triejoin", &[
            "-m",
            "space",
            "--materialize-max-bytes",
            bad,
        ]);
        assert!(!output.status.success(), "{bad}");
        assert!(stderr(&output).contains(message), "{bad}: {}", stderr(&output));
    }
    for good in ["1048576", "1099511627776"] {
        let (output, _) = run(&workspace, &cache, "small", "tree-trie", "leapfrog-triejoin", &[
            "-m",
            "space",
            "--materialize-max-bytes",
            good,
        ]);
        assert!(output.status.success(), "{good}: {}", stderr(&output));
    }
}

#[test]
fn a_probe_records_a_single_run_materialize_value_when_asked() {
    let (workspace, cache) = workspace();
    let base = [
        "probe",
        "probe",
        "-q",
        "small",
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
    ];
    let mut asked = base.to_vec();
    asked.push("--materialize");
    let (output, reports) = kermit(&workspace, &cache, &asked);
    assert!(output.status.success(), "{}", stderr(&output));
    let single = value(&reports[0], "materialize").expect("a materialize value");
    assert_eq!(single["metric"], "time");
    assert_eq!(single["estimator"], "single");
    assert!(single["value"].is_u64(), "{single}");
    assert!(reports[0]["criterion_groups"].as_array().unwrap().is_empty());
    assert!(
        stderr(&output).contains("materialize"),
        "{}",
        stderr(&output)
    );
    assert!(
        !workspace.path().join("criterion").exists(),
        "a probe writes no Criterion output"
    );

    let (output, reports) = kermit(&workspace, &cache, &base);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(value(&reports[0], "materialize").is_none());
}

#[test]
fn a_probe_skips_materialize_over_the_limit() {
    let (workspace, cache) = workspace();
    let (output, reports) = kermit(&workspace, &cache, &[
        "probe",
        "probe",
        "-q",
        "star",
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "--materialize",
        "--materialize-max-bytes",
        "1048576",
    ]);
    let err = stderr(&output);
    assert!(output.status.success(), "{err}");
    assert!(err.contains("note: skipping materialize for probe/star"), "{err}");
    assert!(value(&reports[0], "materialize").is_none());
    assert!(value(&reports[0], "build").is_some());
}

#[test]
fn bench_ds_refuses_materialize() {
    let output = Command::new(kermit_bin())
        .env("RUST_BACKTRACE", "0")
        .args(["bench", "ds", "--relation"])
        .arg(edge_fixture())
        .args(["--indexstructure", "tree-trie", "-m", "materialize"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("bench ds has no join, so it cannot measure materialize"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn bench_join_takes_materialize() {
    let (output, report) = bench_join("tree-trie", "leapfrog-triejoin", &["-m", "materialize"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let reports = reports_of(&report);
    assert_eq!(functions(&reports[0]), ["materialize"]);
}
```

(b) In `kermit/src/bench/run.rs`, add to `mod tests` (the end of the module):

```rust
    #[test]
    fn materialize_bytes_is_rows_times_width_times_eight() {
        assert_eq!(materialize_bytes(0, 3), 0);
        assert_eq!(materialize_bytes(10, 3), 240);
        assert_eq!(
            materialize_bytes(5, 0),
            0,
            "a query of checks has no variable"
        );
        assert_eq!(materialize_bytes(u64::MAX, 2), u64::MAX, "it saturates");
        assert_eq!(
            materialize_bytes(1_453_845_122, 4),
            46_523_043_904,
            "SNAP dblp path3: about 46.5 GB"
        );
    }

    #[test]
    fn the_guard_skips_only_what_exceeds_the_limit() {
        // 131072 rows of one variable are exactly 1 MiB.
        assert_eq!(materialize_skip(131_072, 1, 1_048_576), None);
        assert_eq!(materialize_skip(131_073, 1, 1_048_576), Some(1_048_584));
    }

    /// 131072 rows of one variable are exactly the 1 MiB limit; one more row
    /// is over it.
    const LIMIT: u64 = 1_048_576;

    #[test]
    fn the_guard_decides_a_streaming_subject_from_expected_else_the_streamed_count() {
        use MaterializeVerdict::*;
        // With `expected`: decided before the describe step.
        assert_eq!(materialize_guard(true, true, Some(131_072), 1, LIMIT), Run);
        assert_eq!(
            materialize_guard(true, true, Some(131_073), 1, LIMIT),
            Skip(1_048_584)
        );
        // Without it: decided again once the describe step has counted.
        assert_eq!(
            materialize_guard(true, true, None, 1, LIMIT),
            AfterStreamedCount
        );
        assert_eq!(materialize_guard(true, true, Some(10), 1, LIMIT), Run);
        assert_eq!(
            materialize_guard(true, true, Some(1_000_000), 1, LIMIT),
            Skip(8_000_000)
        );
    }

    #[test]
    fn the_guard_decides_a_subject_that_does_not_stream_before_anything_materialises() {
        use MaterializeVerdict::*;
        assert_eq!(materialize_guard(true, false, Some(131_072), 1, LIMIT), Run);
        // Over the limit nothing materialises, not even the describe step's count.
        let over = materialize_guard(true, false, Some(131_073), 1, LIMIT);
        assert_eq!(over, SkipUncounted(1_048_584));
        assert!(!over.counts());
        // No `expected`: the warning, then everything runs unguarded.
        let unguarded = materialize_guard(true, false, None, 1, LIMIT);
        assert_eq!(unguarded, Unguarded);
        assert!(unguarded.counts());
    }

    #[test]
    fn the_guard_counts_unless_a_non_streaming_subject_is_skipped() {
        use MaterializeVerdict::*;
        for streams in [true, false] {
            assert_eq!(
                materialize_guard(false, streams, Some(u64::MAX), 4, LIMIT),
                NotAsked,
                "nothing is decided when nothing would materialise"
            );
            assert!(NotAsked.counts());
        }
        assert!(Run.counts());
        assert!(Skip(9).counts(), "a streamed count is cheap");
        assert!(AfterStreamedCount.counts());
        assert!(!SkipUncounted(9).counts());
        assert_eq!(Skip(9).skipped(), Some(9));
        assert_eq!(SkipUncounted(9).skipped(), Some(9));
        assert_eq!(Run.skipped(), None);
    }

    #[test]
    fn a_non_streaming_subject_is_guarded_when_only_verify_or_a_probe_would_materialise() {
        use MaterializeVerdict::*;
        // (streams, metric, verifies, single_run)
        assert!(guard_requested(false, false, true, false), "--verify with expected");
        assert!(guard_requested(false, false, false, true), "a probe's single run");
        assert!(guard_requested(false, true, false, false), "the metric");
        assert!(!guard_requested(false, false, false, false), "nothing would materialise");
        // A streaming subject counts by streaming, so only the metric asks.
        assert!(!guard_requested(true, false, true, true));
        assert!(guard_requested(true, true, false, false));
        // `--verify` alone, `expected` over the limit: skipped, and uncounted.
        let verdict = materialize_guard(
            guard_requested(false, false, true, false),
            false,
            Some(131_073),
            1,
            LIMIT,
        );
        assert_eq!(verdict, SkipUncounted(1_048_584));
        assert!(!verdict.counts(), "so there is no describe-step count to verify");
        // The same run on a streaming subject is not guarded at all.
        assert_eq!(
            materialize_guard(guard_requested(true, false, true, false), true, Some(131_073), 1, LIMIT),
            NotAsked
        );
    }
```

(c) In `kermit/src/bench/ds.rs`, add a test module at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_ds_refuses_the_materialize_metric_and_only_that() {
        let message = refuse_materialize(&[Metric::Build, Metric::Materialize])
            .unwrap_err()
            .to_string();
        assert_eq!(
            message,
            "bench ds has no join, so it cannot measure materialize"
        );
        refuse_materialize(&[Metric::Build, Metric::Iteration, Metric::Space, Metric::EndToEnd])
            .unwrap();
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::`

Expected: the binary's tests fail to compile: ``error[E0425]: cannot find function `materialize_bytes` in this scope``, ``cannot find function `materialize_skip` ``, ``cannot find function `materialize_guard` ``, ``cannot find type `MaterializeVerdict` ``, ``cannot find function `refuse_materialize` ``.

Run: `nix develop --command cargo test -p kermit --test cli_materialize`

Expected: the tests that pass `--materialize-max-bytes` or `--materialize` fail on clap's ``error: unexpected argument '--materialize-max-bytes' found`` / ``error: unexpected argument '--materialize' found``; `materialize_is_recorded_under_its_function_name` fails because the report has no `answer_rows` (nothing counted yet); `verify_checks_the_materialized_count_against_expected` fails for lack of the `verified materialize` line; `bench_ds_refuses_materialize` fails because `bench ds` accepts the metric (the metric parses since Task 3.1). `a_run_that_does_not_ask_for_materialize_reports_as_before` and `bench_join_takes_materialize` pass already.

- [ ] **Step 3: Implement**

**(a) `kermit/src/bench/run.rs`: the settings, the helpers and the runner.**

`RunSettings` gains the guard's limit.

Current (lines 53-58):

```rust
    /// Count every query and record the describe step's build and
    /// execution times as single-run values (`bench probe`).
    pub single_run: bool,
    /// Criterion sample/measurement/warm-up settings.
    pub criterion: &'a CriterionSettings,
}
```

Replace with:

```rust
    /// Count every query and record the describe step's build and
    /// execution times as single-run values (`bench probe`).
    pub single_run: bool,
    /// `--materialize-max-bytes`: the `materialize` metric is skipped, with a
    /// note, for a query whose full assignments would take more bytes than
    /// this (see [`materialize_bytes`]).
    pub materialize_max_bytes: u64,
    /// Criterion sample/measurement/warm-up settings.
    pub criterion: &'a CriterionSettings,
}
```

The two guard helpers, after `tuples_read`:

Current (lines 78-92):

```rust
/// The rows the relations `prepared` reads hold in `catalog`, each relation
/// counted once however many atoms read it: the report's `tuples` axis, the
/// same for every structure.
fn tuples_read(catalog: &Catalog, prepared: &Prepared) -> usize {
    prepared
        .relations()
        .into_iter()
        .map(|name| {
            catalog
                .tuples(name)
                .expect("prepare checked that every relation a query reads is in the catalog")
                .len()
        })
        .sum()
}
```

Replace with:

```rust
/// The rows the relations `prepared` reads hold in `catalog`, each relation
/// counted once however many atoms read it: the report's `tuples` axis, the
/// same for every structure.
fn tuples_read(catalog: &Catalog, prepared: &Prepared) -> usize {
    prepared
        .relations()
        .into_iter()
        .map(|name| {
            catalog
                .tuples(name)
                .expect("prepare checked that every relation a query reads is in the catalog")
                .len()
        })
        .sum()
}

/// The bytes `materialize` keeps for `rows` assignments of `width` variables:
/// `rows × width × 8`, saturating. The memory guard compares it with
/// `--materialize-max-bytes`.
pub(crate) fn materialize_bytes(rows: u64, width: usize) -> u64 {
    let width = u64::try_from(width).unwrap_or(u64::MAX);
    let value = u64::try_from(std::mem::size_of::<usize>()).unwrap_or(u64::MAX);
    rows.saturating_mul(width).saturating_mul(value)
}

/// What the guard decides: `Some(bytes)`, the result's size, when it exceeds
/// `limit` and `materialize` must be skipped; `None` when it fits.
fn materialize_skip(rows: u64, width: usize, limit: u64) -> Option<u64> {
    let bytes = materialize_bytes(rows, width);
    (bytes > limit).then_some(bytes)
}

/// What the memory guard decides for one query's `materialize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaterializeVerdict {
    /// `materialize` is not asked for.
    NotAsked,
    /// The result fits the limit.
    Run,
    /// A streaming subject's result is over the limit, by this many bytes.
    /// The describe step may still count: its streamed count is cheap.
    Skip(u64),
    /// A subject that does not stream is over the limit, by this many bytes.
    /// Its count is a `materialize`, so nothing materialises: no describe-step
    /// count, no preflight count.
    SkipUncounted(u64),
    /// A streaming subject with no `expected`: ask again with the describe
    /// step's streamed count.
    AfterStreamedCount,
    /// A subject that does not stream, with no `expected`: the guard has no
    /// row count to read, so it warns and everything runs unguarded.
    Unguarded,
}

impl MaterializeVerdict {
    /// Whether the describe step and a cell's preflight may count: false only
    /// when a subject that does not stream is skipped (its count is the
    /// `materialize` the guard refused).
    pub(crate) fn counts(self) -> bool { !matches!(self, Self::SkipUncounted(_)) }

    /// The result's size in bytes, when the guard skips `materialize`.
    pub(crate) fn skipped(self) -> Option<u64> {
        match self {
            | Self::Skip(bytes) | Self::SkipUncounted(bytes) => Some(bytes),
            | _ => None,
        }
    }
}

/// The memory guard, in one place for every subject. `requested` is whether
/// anything would materialise, as [`guard_requested`] computes it; `streams` is the subject type's
/// [`Subject::STREAMS`]; `rows` is the full-assignment count the guard reads:
/// the query's `expected`, or, on a second call for a streaming subject, the
/// describe step's streamed count; `width` is `Prepared::assignment_width`.
///
/// A streaming subject with no `rows` answers
/// [`AfterStreamedCount`](MaterializeVerdict::AfterStreamedCount). A subject
/// that does not stream is decided before its describe step, from `expected`
/// alone, so over the limit it answers
/// [`SkipUncounted`](MaterializeVerdict::SkipUncounted) and nothing
/// materialises; with no `expected` it answers
/// [`Unguarded`](MaterializeVerdict::Unguarded).
pub(crate) fn materialize_guard(
    requested: bool,
    streams: bool,
    rows: Option<u64>,
    width: usize,
    limit: u64,
) -> MaterializeVerdict {
    use MaterializeVerdict::*;
    if !requested {
        return NotAsked;
    }
    match (rows, streams) {
        | (Some(rows), _) => match materialize_skip(rows, width, limit) {
            | None => Run,
            | Some(bytes) if streams => Skip(bytes),
            | Some(bytes) => SkipUncounted(bytes),
        },
        | (None, true) => AfterStreamedCount,
        | (None, false) => Unguarded,
    }
}

/// The guard's `requested`. For a streaming subject it is whether the
/// `materialize` metric is asked for (`metric`): the describe step streams its
/// count, so only the metric materialises. A subject that does not stream
/// counts through `materialize`, so it is also whether `--verify` has an
/// `expected` to check (`verifies`) or the run is a probe's single run
/// (`single_run`): a `--verify`-only run over the limit then materialises
/// nothing either.
fn guard_requested(streams: bool, metric: bool, verifies: bool, single_run: bool) -> bool {
    metric || (!streams && (verifies || single_run))
}
```

In `run_benchmark`, first the doc paragraph (append to the doc comment before `fn run_benchmark`):

Current (lines 113-116):

```rust
/// `build` and `end_to_end`. With `settings.verify`, a query with an
/// `expected` count is checked before any timer starts, and a mismatch aborts
/// with every earlier query's report already in `sink`.
fn run_benchmark<F: ExecutionFamily>(
```

Replace with:

```rust
/// `build` and `end_to_end`. With `settings.verify`, a query with an
/// `expected` count is checked before any timer starts, and a mismatch aborts
/// with every earlier query's report already in `sink`.
///
/// `materialize`, when asked for, is guarded first (see
/// [`materialize_guard`]): its result is every full assignment, so it is
/// skipped, with a stderr note and a `materialize` metadata line, when
/// `expected` (for a streaming subject, else the describe step's streamed
/// count) times the variable count times 8 bytes exceeds
/// `settings.materialize_max_bytes`. Otherwise `--verify` also checks its
/// distinct count against `expected`, once.
fn run_benchmark<F: ExecutionFamily>(
```

The destructure and the label:

Current (lines 119-128):

```rust
    let RunSettings {
        kind,
        prefix,
        planner,
        metrics,
        queries_per_build,
        verify,
        single_run,
        criterion,
    } = settings;
```

Replace with:

```rust
    let RunSettings {
        kind,
        prefix,
        planner,
        metrics,
        queries_per_build,
        verify,
        single_run,
        materialize_max_bytes,
        criterion,
    } = settings;
```

Current (lines 141-143):

```rust
    let execution = family.execution();
    let ds_name = execution.index_structure().axis_value();
    let algo_name = execution.algorithm().axis_value();
```

Replace with:

```rust
    let execution = family.execution();
    let ds_name = execution.index_structure().axis_value();
    let algo_name = execution.algorithm().axis_value();
    // The cell as notes name it.
    let label = format!("{ds_name}/{algo_name}");
```

The `Subject` trait is needed in scope for its constant:

Current (lines 6-7):

```rust
        measure::{CriterionSettings, Described, MeasureRequest, QuerySubject},
```

Replace with:

```rust
        measure::{CriterionSettings, Described, MeasureRequest, QuerySubject, Subject},
```

The guard's first decision comes before the subject and its describe step, from `expected` and the subject type's `STREAMS`: for a subject that does not stream it must precede the describe step, whose count is a `materialize`. Its `counts()` is what a cell's `preflight` takes as `count` (Story 4).

Current (lines 169-172):

```rust
    for (query_def, prepared) in definition.queries.iter().zip(prepared_queries) {
        // The describe step: this query's one build outside Criterion. Every
        // untimed number in its report is read from it, and `measure`
        // consumes it before any build is timed.
```

Replace with:

```rust
    for (query_def, prepared) in definition.queries.iter().zip(prepared_queries) {
        // The memory guard of `materialize`, first from `expected` alone; a
        // streaming subject with none is decided again after its describe
        // step has counted.
        let streams = <QuerySubject<'_, F> as Subject>::STREAMS;
        let materialize_requested = metrics.contains(&Metric::Materialize);
        let width = prepared.assignment_width();
        let mut guard = materialize_guard(
            guard_requested(
                streams,
                materialize_requested,
                verify && query_def.expected.is_some(),
                single_run,
            ),
            streams,
            query_def.expected,
            width,
            materialize_max_bytes,
        );
        if guard == MaterializeVerdict::Unguarded {
            eprintln!(
                "warning: {label} does not stream and {}/{} has no expected count, so \
                 materialize runs without the --materialize-max-bytes guard",
                definition.name, query_def.name
            );
        }

        // The describe step: this query's one build outside Criterion. Every
        // untimed number in its report is read from it, and `measure`
        // consumes it before any build is timed.
```

The describe step counts whenever `materialize` is asked for and the guard has not skipped a subject that does not stream (a streaming subject's guard needs the row count when `expected` is absent):

Current (lines 172-173):

```rust
        let count = (verify && query_def.expected.is_some()) || single_run;
        let described = Described::new(&subject, count);
```

Replace with:

```rust
        let count = guard.counts()
            && ((verify && query_def.expected.is_some()) || single_run || materialize_requested);
        let mut described = Described::new(&subject, count);
```

The correctness gate reads the describe step's count, which a guard-skipped subject that does not stream never took (its count is the `materialize` the guard refused). That cell is not verified, and the skip note below says why; the gate must not expect a count. Insert a new arm first in the gate's `match`:

Current (lines 199-201):

```rust
        let verified = match (verify, query_def.expected) {
            | (true, Some(expected)) => {
                let actual = described
```

Replace with:

```rust
        let verified = match (verify, query_def.expected) {
            | (true, Some(_)) if !guard.counts() => {
                eprintln!(
                    "bench {kind}: query '{}' in benchmark '{}' on {label} is not verified: its \
                     count would materialize the result, which the guard skips",
                    query_def.name, definition.name
                );
                false
            },
            | (true, Some(expected)) => {
                let actual = described
```

The guard, the `--verify` check and the probe's single run, after the `answer rows` metadata:

Current (lines 229-232):

```rust
        let answer_rows = described.rows();
        if let Some(rows) = answer_rows {
            metadata.push(MetadataLine::new("answer rows", rows));
        }
```

Replace with:

```rust
        let answer_rows = described.rows();
        if let Some(rows) = answer_rows {
            metadata.push(MetadataLine::new("answer rows", rows));
        }
        // A streaming subject with no `expected` is decided now, from the
        // streamed count the describe step just took.
        if guard == MaterializeVerdict::AfterStreamedCount {
            guard = materialize_guard(true, streams, answer_rows, width, materialize_max_bytes);
            assert_ne!(
                guard,
                MaterializeVerdict::AfterStreamedCount,
                "the describe step counts when materialize is requested"
            );
        }
        let mut run_materialize = materialize_requested;
        if let Some(bytes) = guard.skipped() {
            eprintln!(
                "note: skipping materialize for {}/{} on {label}: {bytes} bytes over \
                 --materialize-max-bytes {materialize_max_bytes}",
                definition.name, query_def.name
            );
            metadata.push(MetadataLine::new(
                "materialize",
                format!("skipped: {bytes} bytes over {materialize_max_bytes}"),
            ));
            run_materialize = false;
        }
        // `--verify`'s check of the materialized count, once; a probe's
        // single-run `materialize` value comes from the same run. A subject
        // that does not stream returns the describe step's count, so
        // `materialize` has still run once on that build.
        if run_materialize && (single_run || (verify && query_def.expected.is_some())) {
            let distinct = described
                .materialize_once()
                .expect("a query subject materializes");
            if let (true, Some(expected)) = (verify, query_def.expected) {
                if distinct != expected {
                    anyhow::bail!(
                        "verification failed: benchmark '{}' query '{}' on {label} materialized \
                         {distinct} distinct full assignments, expected {expected}",
                        definition.name,
                        query_def.name,
                    );
                }
                metadata.push(MetadataLine::new("verified materialize", "yes"));
            }
        }
        // A skipped `materialize` is not measured.
        let measured_metrics: Vec<Metric> = metrics
            .iter()
            .copied()
            .filter(|&metric| metric != Metric::Materialize || run_materialize)
            .collect();
```

The probe's metadata line gains the materialize time. A subject that does not stream has no `iteration`, and a skipped one did not count at all, so neither time is assumed:

Current (lines 233-242):

```rust
        if single_run {
            let build = described.build_time();
            let iteration = described
                .exercise_time()
                .expect("a probe counts every query");
            metadata.push(MetadataLine::new(
                "single run",
                format!("build {build:?}, iteration {iteration:?}"),
            ));
        }
```

Replace with:

```rust
        if single_run {
            let build = described.build_time();
            let mut single = format!("build {build:?}");
            if streams {
                let iteration = described
                    .exercise_time()
                    .expect("a probe counts every query it streams");
                single = format!("{single}, iteration {iteration:?}");
            }
            if let Some(materialize) = described.materialize_time() {
                single = format!("{single}, materialize {materialize:?}");
            }
            metadata.push(MetadataLine::new("single run", single));
        }
```

The measure request uses the metrics that survived the guard:

Current (lines 261-269):

```rust
        let group_name = criterion_group_name(prefix, &definition.name, &query_def.name, execution);
        let measured = described.measure(&MeasureRequest {
            metrics,
            single_run,
            queries_per_build,
            criterion,
            group: &group_name,
            function_prefix: None,
        });
```

Replace with:

```rust
        let group_name = criterion_group_name(prefix, &definition.name, &query_def.name, execution);
        let measured = described.measure(&MeasureRequest {
            metrics: &measured_metrics,
            single_run,
            queries_per_build,
            criterion,
            group: &group_name,
            function_prefix: None,
        });
```

**(b) `kermit/src/bench/ds.rs`: the rejection.** Insert above `fn run_ds_bench` 's doc comment (above the line `/// Benchmarks one index structure over one relation file.`):

```rust
/// `bench ds` measures one relation, which has no join to materialize: it
/// refuses the `materialize` metric before anything is read.
///
/// # Errors
///
/// If `metrics` contains [`Metric::Materialize`].
pub(crate) fn refuse_materialize(metrics: &[Metric]) -> anyhow::Result<()> {
    if metrics.contains(&Metric::Materialize) {
        anyhow::bail!("bench ds has no join, so it cannot measure materialize");
    }
    Ok(())
}

```

**(c) `kermit/src/main.rs`: the flags and the handlers.**

The constants and the bounds-checking parser, after `DEFAULT_DS_GROUP`:

Current (lines 79-81):

```rust
/// Default Criterion group name for `bench ds` (full group name, not a
/// prefix).
const DEFAULT_DS_GROUP: &str = "ds";
```

Replace with:

```rust
/// Default Criterion group name for `bench ds` (full group name, not a
/// prefix).
const DEFAULT_DS_GROUP: &str = "ds";

/// `--materialize-max-bytes`'s default (4 GiB) and bounds (1 MiB, 1 TiB).
const DEFAULT_MATERIALIZE_MAX_BYTES: u64 = 4_294_967_296;
const MIN_MATERIALIZE_MAX_BYTES: u64 = 1_048_576;
const MAX_MATERIALIZE_MAX_BYTES: u64 = 1_099_511_627_776;

/// Parses `--materialize-max-bytes`: whole bytes in
/// `MIN_MATERIALIZE_MAX_BYTES..=MAX_MATERIALIZE_MAX_BYTES`. Outside it, the
/// error names the range.
fn parse_materialize_max_bytes(text: &str) -> Result<u64, String> {
    let range = format!(
        "--materialize-max-bytes must be between {MIN_MATERIALIZE_MAX_BYTES} (1 MiB) and \
         {MAX_MATERIALIZE_MAX_BYTES} (1 TiB)"
    );
    let bytes: u64 = text
        .parse()
        .map_err(|_| format!("`{text}` is not a whole number of bytes; {range}"))?;
    if (MIN_MATERIALIZE_MAX_BYTES..=MAX_MATERIALIZE_MAX_BYTES).contains(&bytes) {
        Ok(bytes)
    } else {
        Err(range)
    }
}
```

`MaterializeArgs`, after `BenchArgs`:

Current (lines 204-209):

```rust
    /// Override the path of the machine-readable JSON report. Default:
    /// `bench-runs/<kind>-<unix-millis>.json` (parent dir auto-created;
    /// `bench-runs/` is gitignored at the workspace root).
    #[arg(long, value_name = "PATH")]
    report_json: Option<PathBuf>,
}
```

Replace with:

```rust
    /// Override the path of the machine-readable JSON report. Default:
    /// `bench-runs/<kind>-<unix-millis>.json` (parent dir auto-created;
    /// `bench-runs/` is gitignored at the workspace root).
    #[arg(long, value_name = "PATH")]
    report_json: Option<PathBuf>,
}

/// The memory guard of the opt-in `materialize` metric, on `bench run`,
/// `bench join` and `bench probe`.
#[derive(Args, Clone, Copy, Debug)]
struct MaterializeArgs {
    /// Skip `materialize` (with a note) for a query whose full assignments
    /// would take more than this many bytes (rows × variables × 8), between
    /// 1 MiB and 1 TiB
    #[arg(
        long,
        value_name = "BYTES",
        default_value_t = DEFAULT_MATERIALIZE_MAX_BYTES,
        value_parser = parse_materialize_max_bytes
    )]
    materialize_max_bytes: u64,
}
```

`bench join` and `bench run` take it:

Current (lines 294-301):

```rust
        #[command(flatten)]
        config: ConfigChoices,

        #[command(flatten)]
        build: BuildChoices,
    },

    /// Benchmark an index structure (build, iteration, space,
```

Replace with:

```rust
        #[command(flatten)]
        config: ConfigChoices,

        #[command(flatten)]
        build: BuildChoices,

        #[command(flatten)]
        materialize: MaterializeArgs,
    },

    /// Benchmark an index structure (build, iteration, space,
```

Current (lines 366-373):

```rust
        /// Run each query once before timing and check its result count
        /// against the benchmark's `expected` value; a mismatch aborts.
        /// Queries without `expected` are noted and skipped.
        #[arg(long)]
        verify: bool,
    },

    /// Screen a named benchmark without Criterion: verify each query, time
```

Replace with:

```rust
        /// Run each query once before timing and check its result count
        /// against the benchmark's `expected` value; a mismatch aborts.
        /// Queries without `expected` are noted and skipped.
        #[arg(long)]
        verify: bool,

        #[command(flatten)]
        materialize: MaterializeArgs,
    },

    /// Screen a named benchmark without Criterion: verify each query, time
```

`bench probe`:

Current (lines 380-388):

```rust
    /// a mismatch aborts. Takes `bench run`'s selection flags. `bench`'s
    /// Criterion settings (`--name`, `--sample-size`, `--measurement-time`,
    /// `--warm-up-time`) do not apply; `--report-json` does. A single run is
    /// cold: compare it with other single runs, never with a Criterion
    /// estimate.
    Probe {
        #[command(flatten)]
        selection: RunSelection,
    },
```

Replace with:

```rust
    /// a mismatch aborts. Takes `bench run`'s selection flags. `bench`'s
    /// Criterion settings (`--name`, `--sample-size`, `--measurement-time`,
    /// `--warm-up-time`) do not apply; `--report-json` does. `--materialize`
    /// adds a single-run `materialize` value (the join with every full
    /// assignment kept, sorted and counted distinct), skipped when over
    /// `--materialize-max-bytes`, as in `bench run`. A single run is cold:
    /// compare it with other single runs, never with a Criterion estimate.
    Probe {
        #[command(flatten)]
        selection: RunSelection,

        /// Also record a single-run `materialize` value
        #[arg(long)]
        materialize: bool,

        #[command(flatten)]
        limits: MaterializeArgs,
    },
```

The handlers. `bench join`:

Current (lines 768-771):

```rust
fn run_bench_join(
    bench_args: &BenchArgs, query_args: QueryArgs, output: Option<PathBuf>, metrics: &[Metric],
    queries_per_build: u32, config: ConfigChoices, build: BuildChoices,
) -> anyhow::Result<()> {
```

Replace with:

```rust
#[allow(clippy::too_many_arguments)]
fn run_bench_join(
    bench_args: &BenchArgs, query_args: QueryArgs, output: Option<PathBuf>, metrics: &[Metric],
    queries_per_build: u32, config: ConfigChoices, build: BuildChoices, materialize_max_bytes: u64,
) -> anyhow::Result<()> {
```

Current (lines 783-792):

```rust
    let settings = RunSettings {
        kind: BenchKind::Join,
        prefix: bench_args.name.as_deref().unwrap_or(DEFAULT_JOIN_GROUP),
        planner: query_args.planner,
        metrics,
        queries_per_build,
        verify: false,
        single_run: false,
        criterion: &criterion,
    };
```

Replace with:

```rust
    let settings = RunSettings {
        kind: BenchKind::Join,
        prefix: bench_args.name.as_deref().unwrap_or(DEFAULT_JOIN_GROUP),
        planner: query_args.planner,
        metrics,
        queries_per_build,
        verify: false,
        single_run: false,
        materialize_max_bytes,
        criterion: &criterion,
    };
```

`bench ds` refuses the metric first:

Current (lines 801-811):

```rust
#[allow(clippy::too_many_arguments)]
fn run_ds_bench_command(
    bench_args: &BenchArgs, relation: PathBuf, indexstructure: IndexStructureSelector,
    metrics: Vec<Metric>, queries_per_build: u32, layout: LayoutChoices, config: ConfigChoices,
    build: BuildChoices,
) -> anyhow::Result<()> {
    // None of `bench ds`'s metrics calls `seek`: build constructs the
```

Replace with:

```rust
#[allow(clippy::too_many_arguments)]
fn run_ds_bench_command(
    bench_args: &BenchArgs, relation: PathBuf, indexstructure: IndexStructureSelector,
    metrics: Vec<Metric>, queries_per_build: u32, layout: LayoutChoices, config: ConfigChoices,
    build: BuildChoices,
) -> anyhow::Result<()> {
    // A relation has no join to materialize.
    bench::ds::refuse_materialize(&metrics)?;
    // None of `bench ds`'s metrics calls `seek`: build constructs the
```

`bench run` and `bench probe`:

Current (lines 847-853):

```rust
/// Handler for `bench run` and `bench probe`: materialise the selected
/// benchmarks and run every valid (index-structure, algorithm) cell of the
/// requested sweep, writing each query's report as it finishes.
fn run_bench_run_command(
    bench_args: &BenchArgs, selection: RunSelection, kind: BenchKind, metrics: Vec<Metric>,
    queries_per_build: u32, verify: bool, single_run: bool,
) -> anyhow::Result<()> {
```

Replace with:

```rust
/// Handler for `bench run` and `bench probe`: materialise the selected
/// benchmarks and run every valid (index-structure, algorithm) cell of the
/// requested sweep, writing each query's report as it finishes.
#[allow(clippy::too_many_arguments)]
fn run_bench_run_command(
    bench_args: &BenchArgs, selection: RunSelection, kind: BenchKind, metrics: Vec<Metric>,
    queries_per_build: u32, verify: bool, single_run: bool, materialize_max_bytes: u64,
) -> anyhow::Result<()> {
```

Current (lines 884-901):

```rust
    let settings = RunSettings {
        kind,
        prefix,
        planner,
        metrics: &metrics,
        queries_per_build,
        verify,
        single_run,
        criterion: &criterion,
    };
    // Names only — nothing is fetched — so a sweep whose cells would
    // overwrite each other's Criterion results fails before any is timed.
    // A run with no time metric (`bench probe`, `-m space`) writes no
    // Criterion group, so nothing can collide.
    if metrics.iter().any(|metric| metric.is_timed()) {
        check_sweep_group_directories(prefix, &materialized, query.as_deref(), &cells)?;
    }
```

Replace with:

```rust
    let settings = RunSettings {
        kind,
        prefix,
        planner,
        metrics: &metrics,
        queries_per_build,
        verify,
        single_run,
        materialize_max_bytes,
        criterion: &criterion,
    };
    // Names only — nothing is fetched — so a sweep whose cells would
    // overwrite each other's Criterion results fails before any is timed.
    // A run with no time metric (`-m space`) and a probe (its single runs go
    // through no Criterion function, `materialize` included) write no
    // Criterion group, so nothing can collide.
    if !single_run && metrics.iter().any(|metric| metric.is_timed()) {
        check_sweep_group_directories(prefix, &materialized, query.as_deref(), &cells)?;
    }
```

Current (lines 934-949):

```rust
/// Handler for `bench probe`: `bench run`'s runner with the probe's fixed
/// settings. Every query is counted and, where it declares `expected`,
/// verified; its describe step's build and execution times are recorded as
/// single-run values beside its exact `space`; nothing goes through
/// Criterion.
fn run_bench_probe_command(bench_args: &BenchArgs, selection: RunSelection) -> anyhow::Result<()> {
    run_bench_run_command(
        bench_args,
        selection,
        BenchKind::Probe,
        vec![Metric::Space],
        1,
        true,
        true,
    )
}
```

Replace with:

```rust
/// Handler for `bench probe`: `bench run`'s runner with the probe's fixed
/// settings. Every query is counted and, where it declares `expected`,
/// verified; its describe step's build and execution times are recorded as
/// single-run values beside its exact `space`; nothing goes through
/// Criterion. With `materialize`, the query is also materialized once and
/// that time recorded as a single-run `materialize` value.
fn run_bench_probe_command(
    bench_args: &BenchArgs, selection: RunSelection, materialize: bool, materialize_max_bytes: u64,
) -> anyhow::Result<()> {
    let mut metrics = vec![Metric::Space];
    if materialize {
        metrics.push(Metric::Materialize);
    }
    run_bench_run_command(
        bench_args,
        selection,
        BenchKind::Probe,
        metrics,
        1,
        true,
        true,
        materialize_max_bytes,
    )
}
```

The dispatch in `main`:

Current (lines 974-989):

```rust
            | BenchSubcommand::Join {
                query_args,
                output,
                metrics,
                queries_per_build,
                config,
                build,
            } => run_bench_join(
                &bench_args,
                query_args,
                output,
                &metrics,
                queries_per_build,
                config,
                build,
            )?,
```

Replace with:

```rust
            | BenchSubcommand::Join {
                query_args,
                output,
                metrics,
                queries_per_build,
                config,
                build,
                materialize,
            } => run_bench_join(
                &bench_args,
                query_args,
                output,
                &metrics,
                queries_per_build,
                config,
                build,
                materialize.materialize_max_bytes,
            )?,
```

Current (lines 1010-1027):

```rust
            | BenchSubcommand::Run {
                selection,
                metrics,
                queries_per_build,
                verify,
            } => run_bench_run_command(
                &bench_args,
                selection,
                BenchKind::Run,
                metrics,
                queries_per_build,
                verify,
                false,
            )?,

            | BenchSubcommand::Probe {
                selection,
            } => run_bench_probe_command(&bench_args, selection)?,
```

Replace with:

```rust
            | BenchSubcommand::Run {
                selection,
                metrics,
                queries_per_build,
                verify,
                materialize,
            } => run_bench_run_command(
                &bench_args,
                selection,
                BenchKind::Run,
                metrics,
                queries_per_build,
                verify,
                false,
                materialize.materialize_max_bytes,
            )?,

            | BenchSubcommand::Probe {
                selection,
                materialize,
                limits,
            } => run_bench_probe_command(
                &bench_args,
                selection,
                materialize,
                limits.materialize_max_bytes,
            )?,
```

Format: `nix develop --command cargo fmt --all`.

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --bin kermit`

Expected: all binary tests pass, `materialize_bytes_is_rows_times_width_times_eight`, `the_guard_skips_only_what_exceeds_the_limit`, the three `the_guard_*` tests of `materialize_guard` and `bench_ds_refuses_the_materialize_metric_and_only_that` included.

Run: `nix develop --command cargo test -p kermit --test cli_materialize`

Expected: 11 tests pass. (`verify_aborts_when_the_distinct_count_differs_from_expected` fails the process on purpose; the test asserts the message.)

Then the story's remaining checks for the package: `nix develop --command cargo fmt --all -- --check`, `nix develop --command cargo clippy -p kermit --all-targets -- -D warnings`, `nix develop --command cargo test -p kermit`, and `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit'`; since the changed docs are in the binary, also `nix develop --command cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings` and compare with the two pre-existing errors CLAUDE.md names: any other error is new.

Expected: no new warnings or errors.

- [ ] **Step 5: Commit**

```bash
git add -N kermit/tests/cli_materialize.rs
git commit -m "feat(bench): the materialize guard, --verify, --materialize-max-bytes and bench probe --materialize" -- kermit/src/bench/run.rs kermit/src/bench/ds.rs kermit/src/main.rs kermit/tests/cli_materialize.rs
```

## Story 4: `run_benchmark` over a `Cell`

- **Developer:** intermediate-dev
- **Files:** `kermit/src/bench/cell.rs`, `kermit/src/bench/mod.rs`, `kermit/src/bench/run.rs`
- **Interfaces:** exposes `Cell`, `FamilyCell` and `run_benchmark<C: Cell>` (Names contract: `Cell`) to Stories 9 and 10; uses Story 3's `Subject`
- **Verify:** `cargo test -p kermit`
- **Review:** deep

`run_benchmark` stops being generic over an `ExecutionFamily` and becomes generic over a `Cell`, the
unit the per-query loop actually needs: its labels, its report axes, its Criterion group segment,
its trie-level policy, the metrics it supports, a fallible preflight and the `Subject` it builds
for a prepared query. `FamilyCell` wraps every existing family and reproduces today's behaviour
exactly, so this is a pure refactor: every report, stderr line and Criterion group is unchanged,
and the existing CLI and report tests are the check. Two tasks: first the group-name segment moves
out of `criterion_group_name` into `cell.rs` (the sweep's clash check still names cells by
`Execution`), then the `Cell` trait, `FamilyCell` and the generic runner land together. Story 3
has already added code to `run_benchmark` (the `materialize` guard and its notes, the `--verify`
check on the `materialize` count); Task 4.2 therefore edits the function by small quoted blocks
and ends with a search that finds anything of Story 3's that still names the family.

### Task 4.1: the group segment lives in `cell.rs`

**Files:**
- Create: `kermit/src/bench/cell.rs`
- Modify: `kermit/src/bench/mod.rs:15-19`
- Modify: `kermit/src/bench/run.rs` (imports, `criterion_group_name`, its tests)
- Test: `kermit/src/bench/cell.rs`, `kermit/src/bench/run.rs`

- [ ] **Step 1: Write the failing test**

Register the module in `kermit/src/bench/mod.rs`. The current block is:

```rust
pub mod ds;
pub mod gen;
pub mod measure;
pub mod run;
pub mod workload;
```

Replace it with:

```rust
mod cell;
pub mod ds;
pub mod gen;
pub mod measure;
pub mod run;
pub mod workload;
```

Create `kermit/src/bench/cell.rs` holding only its test:

```rust
#[cfg(test)]
mod tests {
    use {super::*, crate::options::DsChoices, kermit_ds::IndexStructure};

    #[test]
    fn a_kermit_cells_segment_is_its_structure_and_algorithm() {
        for (structure, segment) in [
            (IndexStructure::TreeTrie, "TreeTrie/LeapfrogTriejoin"),
            (IndexStructure::ColumnTrie, "ColumnTrie/LeapfrogTriejoin"),
            (IndexStructure::HashTrie, "HashTrie/HashTriejoin"),
        ] {
            let cell = Execution::for_structure(structure, DsChoices::default());
            assert_eq!(execution_segment(cell), segment);
        }
    }
}
```

In `kermit/src/bench/run.rs`, add a test beside the others, directly before
`fn groups_sharing_a_truncated_directory_are_rejected`'s `#[test]` attribute (that is, as the
second test of `mod tests`):

```rust
    /// The group name `bench run` has always used, pinned before it moves.
    #[test]
    fn a_kermit_cells_group_is_prefix_workload_query_structure_algorithm() {
        let cell =
            Execution::for_structure(kermit_ds::IndexStructure::ColumnTrie, DsChoices::default());
        assert_eq!(
            criterion_group_name("run", "triangle", "q0000", cell),
            "run/triangle/q0000/ColumnTrie/LeapfrogTriejoin"
        );
        let cell =
            Execution::for_structure(kermit_ds::IndexStructure::HashTrie, DsChoices::default());
        assert_eq!(
            criterion_group_name("join", "adhoc", "q", cell),
            "join/adhoc/q/HashTrie/HashTriejoin"
        );
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::`

Expected: the binary's tests do not compile:
`error[E0433]: failed to resolve: use of undeclared type `Execution`` and
`error[E0425]: cannot find function `execution_segment` in this scope`, ending
`error: could not compile `kermit` (bin "kermit" test) due to 2 previous errors`.

- [ ] **Step 3: Implement**

Put the following above the `#[cfg(test)]` block in `kermit/src/bench/cell.rs`:

```rust
//! The unit the generic runner measures: a [`Cell`]. `bench run`, `bench join`
//! and `bench probe` run one cell over a prepared workload; this module holds
//! what the runner needs from a cell, and the one cell kermit's own
//! structures use, [`FamilyCell`].

use crate::execution::Execution;

/// The last two segments of a kermit cell's Criterion group, and its label in
/// stderr notes: `{ds}/{algo}`. The strings are pinned by
/// `IndexStructure::axis_value` and `JoinAlgorithm::axis_value`; see the tests
/// there before changing one, since they name the on-disk
/// `target/criterion/{group}` directories.
pub(crate) fn execution_segment(cell: Execution) -> String {
    format!(
        "{}/{}",
        cell.index_structure().axis_value(),
        cell.algorithm().axis_value()
    )
}
```

In `kermit/src/bench/run.rs`, add `cell::execution_segment,` to the `super::{…}` import. The
current block is:

```rust
    super::{
        criterion_directory_name,
        measure::{CriterionSettings, Described, MeasureRequest, QuerySubject, Subject},
```

and becomes:

```rust
    super::{
        cell::execution_segment,
        criterion_directory_name,
        measure::{CriterionSettings, Described, MeasureRequest, QuerySubject, Subject},
```

Replace `criterion_group_name`, currently:

```rust
/// The Criterion group under which one query of one cell is measured:
/// `{prefix}/{workload}/{query}/{ds}/{algo}`.
fn criterion_group_name(prefix: &str, workload: &str, query: &str, cell: Execution) -> String {
    format!(
        "{prefix}/{workload}/{query}/{}/{}",
        cell.index_structure().axis_value(),
        cell.algorithm().axis_value()
    )
}
```

with:

```rust
/// The Criterion group under which one query of one cell is measured:
/// `{prefix}/{workload}/{query}/{segment}`, where the segment is the cell's
/// own (`{ds}/{algo}` for a kermit cell).
fn group_with_segment(prefix: &str, workload: &str, query: &str, segment: &str) -> String {
    format!("{prefix}/{workload}/{query}/{segment}")
}

/// [`group_with_segment`] for an [`Execution`] cell, which is how the sweep's clash
/// check names the groups before any cell runs.
fn criterion_group_name(prefix: &str, workload: &str, query: &str, cell: Execution) -> String {
    group_with_segment(prefix, workload, query, &execution_segment(cell))
}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::`

Expected: `bench::cell::tests::a_kermit_cells_segment_is_its_structure_and_algorithm ... ok`,
`bench::run::tests::a_kermit_cells_group_is_prefix_workload_query_structure_algorithm ... ok`, and
every other `bench::` test still `ok`.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git add -N kermit/src/bench/cell.rs
git commit -m "refactor(bench): name a kermit cell's Criterion group segment in cell.rs" -- kermit/src/bench/cell.rs kermit/src/bench/mod.rs kermit/src/bench/run.rs
```

### Task 4.2: `Cell`, `FamilyCell` and the generic runner

**Files:**
- Modify: `kermit/src/bench/cell.rs`
- Modify: `kermit/src/bench/run.rs` (imports, `run_benchmark`, `dispatch_run_bench`)
- Test: `kermit/src/bench/cell.rs`

- [ ] **Step 1: Write the failing tests**

In `kermit/src/bench/cell.rs`, replace the whole `#[cfg(test)] mod tests` block, which is currently:

```rust
#[cfg(test)]
mod tests {
    use {super::*, crate::options::DsChoices, kermit_ds::IndexStructure};

    #[test]
    fn a_kermit_cells_segment_is_its_structure_and_algorithm() {
        for (structure, segment) in [
            (IndexStructure::TreeTrie, "TreeTrie/LeapfrogTriejoin"),
            (IndexStructure::ColumnTrie, "ColumnTrie/LeapfrogTriejoin"),
            (IndexStructure::HashTrie, "HashTrie/HashTriejoin"),
        ] {
            let cell = Execution::for_structure(structure, DsChoices::default());
            assert_eq!(execution_segment(cell), segment);
        }
    }
}
```

with:

```rust
#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            bench::measure::Described,
            execution::{HashHtj, TrieLftj},
            options::DsChoices,
        },
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{
            EagerExpansion, HashTrieBuildMode, HashTrieConfig, IndexStructure, KeyLevels,
            NoPruning, TreeTrie,
        },
        kermit_iters::SipHashStrategy,
    };

    #[test]
    fn a_kermit_cells_segment_is_its_structure_and_algorithm() {
        for (structure, segment) in [
            (IndexStructure::TreeTrie, "TreeTrie/LeapfrogTriejoin"),
            (IndexStructure::ColumnTrie, "ColumnTrie/LeapfrogTriejoin"),
            (IndexStructure::HashTrie, "HashTrie/HashTriejoin"),
        ] {
            let cell = Execution::for_structure(structure, DsChoices::default());
            assert_eq!(execution_segment(cell), segment);
        }
    }

    /// `edge` (1 → 2, 2 → 3, 1 → 3) and the two-hop query over it, prepared
    /// under `levels`: the only answer is 1 → 2 → 3.
    fn two_hops(levels: TrieLevels) -> (Catalog, Prepared) {
        let mut tuples = Tuples::new(2);
        for row in [[1, 2], [2, 3], [1, 3]] {
            tuples.push(&row);
        }
        let catalog = Catalog::new(
            vec![(RelationHeader::new_positional("edge", 2), tuples)],
            StatisticsLevel::TupleCounts,
        );
        let query: JoinQuery = "Q(X, Z) :- edge(X, Y), edge(Y, Z).".parse().unwrap();
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared = kermit::db::prepare(&catalog, &query, &planner, levels).unwrap();
        (catalog, prepared)
    }

    #[test]
    fn a_family_cell_names_itself_by_structure_and_algorithm() {
        let family = TrieLftj::<TreeTrie>::new(Default::default());
        let cell = FamilyCell(&family);
        assert_eq!(cell.label(), "TreeTrie/LeapfrogTriejoin");
        assert_eq!(cell.group_segment(), "TreeTrie/LeapfrogTriejoin");
        let lines: Vec<(&str, String)> = cell
            .metadata()
            .into_iter()
            .map(|line| (line.label, line.value))
            .collect();
        assert_eq!(
            lines,
            [
                ("data structure", "TreeTrie".to_string()),
                ("algorithm", "LeapfrogTriejoin".to_string()),
            ]
        );
    }

    #[test]
    fn a_sorted_trie_cells_axes_are_its_identity_and_its_optimization_axes() {
        let family = TrieLftj::<TreeTrie>::new(Default::default());
        let axes = FamilyCell(&family).axes();
        assert_eq!(axes["data_structure"], "TreeTrie");
        assert_eq!(axes["algorithm"], "LeapfrogTriejoin");
        assert_eq!(axes["ds_layout_seek"], "galloping");
        assert_eq!(axes["ds_build_mode"], "serial");
    }

    #[test]
    fn a_hash_trie_cell_prepares_under_its_level_policy_and_says_so_in_its_axes() {
        let family = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion, KeyLevels>::new(
            HashTrieConfig::default(),
            HashTrieBuildMode::default(),
        );
        let cell = FamilyCell(&family);
        assert_eq!(cell.trie_levels(), TrieLevels::JoinKeys);
        let axes = cell.axes();
        assert_eq!(axes["data_structure"], "HashTrie");
        assert_eq!(axes["algorithm"], "HashTriejoin");
        assert_eq!(axes["ds_layout_levels"], "keys");
        assert_eq!(cell.label(), "HashTrie/HashTriejoin");

        let sorted = TrieLftj::<TreeTrie>::new(Default::default());
        assert_eq!(FamilyCell(&sorted).trie_levels(), TrieLevels::All);
    }

    #[test]
    fn a_family_cell_supports_every_metric_and_never_refuses_a_query() {
        let family = TrieLftj::<TreeTrie>::new(Default::default());
        let cell = FamilyCell(&family);
        for metric in [
            Metric::Build,
            Metric::Iteration,
            Metric::Space,
            Metric::EndToEnd,
        ] {
            assert!(cell.supports(metric), "{metric:?}");
        }
        let (catalog, prepared) = two_hops(TrieLevels::All);
        cell.preflight(&catalog, &prepared, true).unwrap();
        cell.preflight(&catalog, &prepared, false).unwrap();
    }

    #[test]
    fn a_family_cell_keeps_the_provided_subject_axes_and_per_index_bytes() {
        let family = TrieLftj::<TreeTrie>::new(Default::default());
        let cell = FamilyCell(&family);
        let (catalog, prepared) = two_hops(TrieLevels::All);
        let subject = cell.subject(&catalog, &prepared).unwrap();
        assert!(cell.subject_axes(&subject).is_empty());
        assert!(<FamilyCell<'_, TrieLftj<TreeTrie>> as Cell>::PER_INDEX_BYTES);
    }

    #[test]
    fn a_family_cells_subject_builds_the_plans_indexes_and_counts_the_answer() {
        let family = TrieLftj::<TreeTrie>::new(Default::default());
        let cell = FamilyCell(&family);
        let (catalog, prepared) = two_hops(TrieLevels::All);
        let subject = cell.subject(&catalog, &prepared).unwrap();
        let described = Described::new(&subject, true);
        assert_eq!(described.rows(), Some(1), "only 1 → 2 → 3");
        assert_eq!(described.bytes().len(), prepared.indexes().len());
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::cell`

Expected: the binary's tests do not compile:
`error[E0425]: cannot find function `FamilyCell` in this scope` (and the same for `Metric`,
`Catalog`, `Prepared`, `RelationHeader`, `Tuples`, `TrieLevels`), ending
`error: could not compile `kermit` (bin "kermit" test) due to N previous errors`.

- [ ] **Step 3: Implement**

Replace the whole of `kermit/src/bench/cell.rs` above its `#[cfg(test)]` block (the module doc,
the `use`, and `execution_segment`) with the following; leave the test block of Step 1 as it is.

```rust
//! The unit the generic runner measures: a [`Cell`]. `bench run`, `bench join`
//! and `bench probe` run one cell over a prepared workload, and the runner
//! (`run_benchmark`) asks a cell for exactly what differs between cells: its
//! labels and report axes, its Criterion group segment, the trie-level policy
//! its queries are prepared under, the metrics it supports, a fallible
//! preflight, and the [`Subject`] it builds for one prepared query. Everything
//! else (validation, `--verify`, `ReportSink`, group naming, the describe step
//! and `measure`) stays one copy in the runner.
//!
//! [`FamilyCell`] is the cell of kermit's own structures: it wraps an
//! [`ExecutionFamily`] and behaves exactly as the runner did when it was
//! generic over the family.

use {
    super::{
        measure::{QuerySubject, Subject},
        Metric,
    },
    crate::{
        bench_report::MetadataLine,
        execution::{Execution, ExecutionFamily},
    },
    kermit::db::{Catalog, Prepared},
    kermit_ds::{Relation, RelationHeader, TrieLevels, TupleView, Tuples},
    std::collections::BTreeMap,
};

/// What the per-query loop of `run_benchmark` needs from one cell.
pub(crate) trait Cell {
    /// What one prepared query is measured as on this cell.
    type Subject<'a>: Subject
    where
        Self: 'a;

    /// For stderr notes and metadata lines: `{ds}/{algo}`.
    fn label(&self) -> String;

    /// The cell's lines in the metadata block, after `benchmark` and `query`
    /// and before `allocator`: `data structure` and `algorithm`.
    fn metadata(&self) -> Vec<MetadataLine>;

    /// The report axes this cell sets: `data_structure`, `algorithm` and the
    /// `ds_*` optimization axes. The runner adds the axes that belong to the
    /// query.
    fn axes(&self) -> BTreeMap<String, serde_json::Value>;

    /// The Criterion group's last segments: `{ds}/{algo}`.
    fn group_segment(&self) -> String;

    /// The trie-level policy this cell's queries are prepared under (#116).
    fn trie_levels(&self) -> TrieLevels;

    /// Whether the cell can measure `metric`. The runner requests only the
    /// metrics a cell supports.
    fn supports(&self, metric: Metric) -> bool;

    /// Untimed and fallible, once per query, before the describe step. A no-op for kermit
    /// cells. `count` is false when the memory guard skips `materialize` for a non-streaming
    /// subject; the engine then runs `load` and `build` only.
    fn preflight(&self, catalog: &Catalog, prepared: &Prepared, count: bool) -> anyhow::Result<()>;

    /// The subject for one prepared query over `catalog`.
    fn subject<'a>(
        &'a self, catalog: &'a Catalog, prepared: &'a Prepared,
    ) -> anyhow::Result<Self::Subject<'a>>;

    /// Axes read from the query's subject, merged into that query's report axes after
    /// [`Cell::axes`]: `engine_version` (engines, from `E::version` of the subject's `Loaded`).
    /// Provided: empty, for kermit cells.
    fn subject_axes(&self, _subject: &Self::Subject<'_>) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::new()
    }

    /// Whether `Subject::heap_bytes` gives one figure per `IndexSpec`, so each `index` metadata
    /// line pairs a spec with its bytes. Provided: true. `EngineCell` sets false and gets no
    /// `index` lines.
    const PER_INDEX_BYTES: bool = true;
}

/// The last two segments of a kermit cell's Criterion group, and its label in
/// stderr notes: `{ds}/{algo}`. The strings are pinned by
/// `IndexStructure::axis_value` and `JoinAlgorithm::axis_value`; see the tests
/// there before changing one, since they name the on-disk
/// `target/criterion/{group}` directories.
pub(crate) fn execution_segment(cell: Execution) -> String {
    format!(
        "{}/{}",
        cell.index_structure().axis_value(),
        cell.algorithm().axis_value()
    )
}

/// A kermit structure under its join algorithm: a sorted trie under Leapfrog
/// Triejoin (`TrieLftj<R>`) or the hash trie under Hash Triejoin
/// (`HashHtj<H, P, E, L>`). The report's `data_structure` / `algorithm` axes
/// come from `family.execution()`, the same value that picked the code path,
/// so a report can never name an algorithm it did not run (issue #56).
pub(crate) struct FamilyCell<'f, F: ExecutionFamily>(pub(crate) &'f F);

impl<F: ExecutionFamily> Cell for FamilyCell<'_, F> {
    type Subject<'a>
        = QuerySubject<'a, F>
    where
        Self: 'a;

    fn label(&self) -> String { execution_segment(self.0.execution()) }

    fn metadata(&self) -> Vec<MetadataLine> {
        let execution = self.0.execution();
        vec![
            MetadataLine::new("data structure", execution.index_structure().axis_value()),
            MetadataLine::new("algorithm", execution.algorithm().axis_value()),
        ]
    }

    /// The identity axes, the dimensions the structure emits (the `ds_*`
    /// prefix convention, see `kermit_iters::HasOptimizationAxes`, guarantees
    /// no collision with the axes the runner assembles per query) and the
    /// build mode, which comes from the family because it leaves no trace in
    /// the structure. They are read off one empty relation, since they depend
    /// on the type, not on the data, and a query whose atoms are all constant
    /// checks builds no index to read them from.
    fn axes(&self) -> BTreeMap<String, serde_json::Value> {
        let execution = self.0.execution();
        let empty = Tuples::new(1);
        let probe = self.0.build_index(
            RelationHeader::new_positional("axes", 1),
            TupleView::all(&empty),
        );
        let mut axes = BTreeMap::from([
            (
                "data_structure".to_string(),
                serde_json::json!(execution.index_structure().axis_value()),
            ),
            (
                "algorithm".to_string(),
                serde_json::json!(execution.algorithm().axis_value()),
            ),
        ]);
        axes.extend(F::optimization_axes(&probe));
        axes.extend(self.0.build_mode_axes());
        axes
    }

    fn group_segment(&self) -> String { execution_segment(self.0.execution()) }

    fn trie_levels(&self) -> TrieLevels { <F::Rel as Relation>::TRIE_LEVELS }

    fn supports(&self, _metric: Metric) -> bool { true }

    fn preflight(&self, _catalog: &Catalog, _prepared: &Prepared, _count: bool) -> anyhow::Result<()> {
        Ok(())
    }

    fn subject<'a>(
        &'a self, catalog: &'a Catalog, prepared: &'a Prepared,
    ) -> anyhow::Result<QuerySubject<'a, F>> {
        Ok(QuerySubject {
            family: self.0,
            catalog,
            prepared,
        })
    }
}
```

Now `kermit/src/bench/run.rs`. Apply each edit below; each quotes the text it replaces.

**Edit A: imports.** The `super::{…}` block is now:

```rust
    super::{
        cell::execution_segment,
        criterion_directory_name,
        measure::{CriterionSettings, Described, MeasureRequest, QuerySubject, Subject},
```

Replace those three lines with:

```rust
    super::{
        cell::{execution_segment, Cell, FamilyCell},
        criterion_directory_name,
        measure::{CriterionSettings, Described, MeasureRequest, Subject},
```

In the `crate::{…}` block, replace

```rust
        execution::{Execution, ExecutionFamily, HashHtj, SortedTrie, Sweep, TrieLftj},
```

with

```rust
        execution::{Execution, HashHtj, SortedTrie, Sweep, TrieLftj},
```

and delete the line

```rust
    kermit_ds::{RelationHeader, TupleView, Tuples},
```

(the probe relation that read the optimization axes moved into `FamilyCell::axes`). If `cargo
build` reports any other import of this file unused, delete it too.

**Edit B: the doc comment of `run_benchmark`.** Replace

```rust
/// Generic over the [`ExecutionFamily`] so the sorted family
/// (`TrieLftj<R>`) and the hash family (`HashHtj<H, P, E>`) share one body:
/// metadata, Criterion group wiring and report assembly are identical, and
/// the report's `data_structure` / `algorithm` axes come from
/// `family.execution()` — the same value that picked the code path — so a
/// report can never name an algorithm it did not run (issue #56).
```

with

```rust
/// Generic over the [`Cell`] so every kind of cell shares one body: kermit's
/// sorted family (`TrieLftj<R>`) and hash family (`HashHtj<H, P, E>`), each
/// wrapped in a [`FamilyCell`], and any other cell. Metadata, Criterion group
/// wiring and report assembly are identical; what differs is asked of the
/// cell. For a `FamilyCell` the report's `data_structure` / `algorithm` axes
/// come from `family.execution()` — the same value that picked the code path
/// — so a report can never name an algorithm it did not run (issue #56).
```

and replace

```rust
/// function (issue #78). Per query, the cell measures a [`QuerySubject`]
/// through the measure layer. One describe build outside Criterion gives the
```

with

```rust
/// function (issue #78). Per query, the cell measures its subject
/// through the measure layer. One describe build outside Criterion gives the
```

**Edit C: the signature.** Replace

```rust
fn run_benchmark<F: ExecutionFamily>(
    family: &F, workload: &PreparedWorkload, settings: RunSettings<'_>, sink: &mut ReportSink,
) -> anyhow::Result<()> {
```

with

```rust
fn run_benchmark<C: Cell>(
    cell: &C, workload: &PreparedWorkload, settings: RunSettings<'_>, sink: &mut ReportSink,
) -> anyhow::Result<()> {
```

**Edit D: what the cell supplies before the loop.** Replace everything from the comment
`// The queries prepared under this family's trie levels (#116).` through the closing brace and
semicolon of the `let optimization_axes = { … };` block (Story 3 has added the two lines
`// The cell as notes name it.` and `let label = format!("{ds_name}/{algo_name}");` inside it, after
`let algo_name = …;`), that is, replace

```rust
    // The queries prepared under this family's trie levels (#116).
    let prepared_queries = workload.prepared(<F::Rel as kermit_ds::Relation>::TRIE_LEVELS);

    // `ds_name`/`algo_name` become the report's identity axes and the
    // on-disk `target/criterion/{group}` names (the group_name below embeds
    // them). The labels are pinned by `IndexStructure::axis_value` and
    // `JoinAlgorithm::axis_value`; see the tests there before changing one.
    let execution = family.execution();
    let ds_name = execution.index_structure().axis_value();
    let algo_name = execution.algorithm().axis_value();
    // The cell as notes name it.
    let label = format!("{ds_name}/{algo_name}");

    // Standard optimization axes: the dimensions the structure emits (the
    // `ds_*` prefix convention, see `kermit_iters::HasOptimizationAxes`,
    // guarantees no collision with the base axes assembled per query below)
    // plus the build mode, which comes from the family because it leaves no
    // trace in the structure. They are read off one empty relation, since
    // they depend on the type, not on the data, and a query whose atoms are
    // all constant checks builds no index to read them from.
    let optimization_axes = {
        let empty = Tuples::new(1);
        let probe = family.build_index(
            RelationHeader::new_positional("axes", 1),
            TupleView::all(&empty),
        );
        let mut axes = F::optimization_axes(&probe);
        axes.extend(family.build_mode_axes());
        axes
    };
```

with

```rust
    // The queries prepared under this cell's trie levels (#116).
    let prepared_queries = workload.prepared(cell.trie_levels());

    // The cell's label names it in the messages below; its axes (`data_structure`,
    // `algorithm` and the `ds_*` dimensions, for a kermit cell) are the base
    // of every query's report axes, and its group segment completes the
    // Criterion group (the on-disk `target/criterion/{group}` names embed it).
    let label = cell.label();
    let cell_axes = cell.axes();
    let group_segment = cell.group_segment();

    // A metric the cell cannot measure is not requested of it.
    let supported: Vec<Metric> = metrics
        .iter()
        .copied()
        .filter(|&metric| cell.supports(metric))
        .collect();
    let metrics = supported.as_slice();
```

**Edit E: the subject.** Replace

```rust
        let subject = QuerySubject {
            family,
            catalog,
            prepared,
        };
```

with

```rust
        // The cell's preflight counts unless Story 3's guard (bound at the top of this loop
        // body) has skipped an uncounted `materialize`: it is that guard's decision, not a
        // second copy of it.
        cell.preflight(catalog, prepared, guard.counts())?;
        let subject = cell.subject(catalog, prepared)?;
        let subject_axes = cell.subject_axes(&subject);
```

`guard` is the `MaterializeVerdict` Story 3 binds at the top of the loop body, before the
describe-step comment; the `preflight` call goes after that binding and before `cell.subject`.
Story 4 imports no `materialize_skip` and keeps no second copy of the decision.

The describe step (`Described::new(&subject, count)`) that follows is unchanged: it borrows the
subject, so `subject_axes` can be read first.

**Edit E2: `streams`.** Story 3 binds, at the top of the loop body, a line that still names the
family type `F`, which is gone once `run_benchmark<C: Cell>` lands. Replace

```rust
        let streams = <QuerySubject<'_, F> as Subject>::STREAMS;
```

with

```rust
        let streams = <C::Subject<'_> as Subject>::STREAMS;
```

`Subject` stays imported by Edit A for this line; Story 3 uses `streams` further down the loop,
and those uses are unchanged.

**Edit F: the metadata block.** Replace

```rust
        let mut metadata = vec![
            MetadataLine::new("benchmark", &definition.name),
            MetadataLine::new("query", &query_def.name),
            MetadataLine::new("data structure", ds_name),
            MetadataLine::new("algorithm", algo_name),
            MetadataLine::new("allocator", crate::ALLOCATOR),
        ];
```

with

```rust
        let mut metadata = vec![
            MetadataLine::new("benchmark", &definition.name),
            MetadataLine::new("query", &query_def.name),
        ];
        metadata.extend(cell.metadata());
        metadata.push(MetadataLine::new("allocator", crate::ALLOCATOR));
```

**Edit F2: the `index` lines.** A cell whose `Subject::heap_bytes` is not one figure per index
spec gets no `index` lines. Replace

```rust
        for (position, ((spec, &rows), &bytes)) in prepared
            .indexes()
            .iter()
            .zip(prepared.index_rows())
            .zip(described.bytes())
            .enumerate()
        {
            metadata.push(MetadataLine::new(
                "index",
                describe_index(position, spec, rows, bytes),
            ));
        }
```

with

```rust
        if C::PER_INDEX_BYTES {
            for (position, ((spec, &rows), &bytes)) in prepared
                .indexes()
                .iter()
                .zip(prepared.index_rows())
                .zip(described.bytes())
                .enumerate()
            {
                metadata.push(MetadataLine::new(
                    "index",
                    describe_index(position, spec, rows, bytes),
                ));
            }
        }
```

**Edit G: the verification message.** Replace

```rust
                        "verification failed: benchmark '{}' query '{}' on {}/{} returned {} \
                         tuples, expected {}",
                        definition.name,
                        query_def.name,
                        ds_name,
                        algo_name,
                        actual,
                        expected
```

with

```rust
                        "verification failed: benchmark '{}' query '{}' on {} returned {} \
                         tuples, expected {}",
                        definition.name,
                        query_def.name,
                        label,
                        actual,
                        expected
```

**Edit H: the group name and the axes.** Replace

```rust
        let group_name = criterion_group_name(prefix, &definition.name, &query_def.name, execution);
```

with

```rust
        let group_name =
            group_with_segment(prefix, &definition.name, &query_def.name, &group_segment);
```

Then, in the `BTreeMap::from([…])` of the query's axes, delete the two entries

```rust
            ("data_structure".to_string(), serde_json::json!(ds_name)),
            ("algorithm".to_string(), serde_json::json!(algo_name)),
```

and replace

```rust
        axes.extend(optimization_axes.clone());
```

with

```rust
        axes.extend(cell_axes.clone());
        axes.extend(subject_axes);
```

**Edit I: Story 3's code.** Story 3 wrote the `materialize` guard's note
(`note: skipping materialize for {benchmark}/{query} on {label}: …`), its metadata line and its
`--verify` message inside this function, naming the cell with `ds_name` and `algo_name` (or
`{ds_name}/{algo_name}`). Find every remaining use:

```bash
grep -n "ds_name\|algo_name\|family\|optimization_axes\|execution\|QuerySubject<'_, F>" kermit/src/bench/run.rs
```

In `run_benchmark` nothing may be left that names `family`, `execution`, `ds_name`, `algo_name`,
`optimization_axes` or `QuerySubject<'_, F>` (Edit E2 replaced the one Story 3 wrote).
Replace each `{ds_name}/{algo_name}` (or the pair `ds_name, algo_name` in
a `{}/{}` format) by the one `label` bound in Edit D, and leave the message text otherwise as it
is, so that the notes read as Story 3 pinned them (`on {label}` is `{ds}/{algo}` for a kermit
cell). The grep may still match `Execution` in `dispatch_run_bench`, `criterion_group_name`
and `resolve_sweep`: those are not `run_benchmark`.

**Edit J: `dispatch_run_bench`.** Replace its doc comment

```rust
/// Runs one `bench run` cell by monomorphising [`run_benchmark`] over the
/// [`ExecutionFamily`] the cell names, pushing each query's report to
/// `sink`. There is no separate algorithm parameter to ignore: the
/// `Execution` fixes both halves of the pair.
```

with

```rust
/// Runs one `bench run` cell by monomorphising [`run_benchmark`] over the
/// [`FamilyCell`] of the family the cell names, pushing each query's report to
/// `sink`. There is no separate algorithm parameter to ignore: the
/// `Execution` fixes both halves of the pair.
```

and its three call sites. Replace

```rust
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &TrieLftj::<kermit_ds::TreeTrie<S>>::new(build),
            workload,
            settings,
            sink,
        )),
```

with

```rust
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &FamilyCell(&TrieLftj::<kermit_ds::TreeTrie<S>>::new(build)),
            workload,
            settings,
            sink,
        )),
```

replace

```rust
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &TrieLftj::<kermit_ds::ColumnTrie<S>>::new(build),
            workload,
            settings,
            sink,
        )),
```

with

```rust
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &FamilyCell(&TrieLftj::<kermit_ds::ColumnTrie<S>>::new(build)),
            workload,
            settings,
            sink,
        )),
```

and replace

```rust
            run_benchmark(
                &HashHtj::<H, P, E, L>::new(config, build),
                workload,
                settings,
                sink,
            )
```

with

```rust
            run_benchmark(
                &FamilyCell(&HashHtj::<H, P, E, L>::new(config, build)),
                workload,
                settings,
                sink,
            )
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::`

Expected: the six new `bench::cell::tests::…` tests and `a_kermit_cells_segment_is_its_structure_and_algorithm`
print `ok`, with every other `bench::` test.

Then the refactor's real check, the report and CLI tests, which pin every stderr line, axis and
Criterion group:

Run: `nix develop --command cargo test -p kermit`

Expected: `test result: ok.` for the library, the binary and every integration test, with no
test changed.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git commit -m "refactor(bench): run_benchmark over a Cell, FamilyCell wrapping the families" -- kermit/src/bench/cell.rs kermit/src/bench/run.rs
```

## Story 5: The catalog shares its tuples.

- **Developer:** junior-dev
- **Files:** `kermit/src/db/catalog.rs`
- **Interfaces:** exposes `Catalog::shared_tuples` (Names contract: `Catalog`) to Story 8; uses nothing
- **Verify:** `cargo test -p kermit --lib`
- **Review:** light (one file, an `Arc` behind an unchanged accessor)

`Entry::tuples` becomes `Arc<Tuples>` and `Catalog::shared_tuples` hands out a clone of the `Arc`, so Nemo's `TableProvider` (a `'static` box) can stream from the catalog with one copy of the rows. `Catalog::tuples` keeps returning `Option<&Tuples>`, the statistics are still gathered from the unwrapped batch before it is shared, and kermit's builds still borrow through `TupleView`, so nothing that is measured changes.

### Task 5.1: `Entry::tuples` behind an `Arc`, and `shared_tuples`

**Files:**
- Modify: `kermit/src/db/catalog.rs:3-16` (imports and `Entry`), `:90-94` (the `Entry` construction), `:109-112` (the accessor), `:134-end` (tests)
- Test: `kermit/src/db/catalog.rs` (the inline `tests` module)

- [ ] **Step 1: Write the failing test**

In the `tests` module of `kermit/src/db/catalog.rs`, replace the last test and the module's closing brace

```rust
        assert_eq!(catalog.relation_names(), vec![
            "edge".to_string(),
            "r".to_string()
        ]);
    }
}
```

with

```rust
        assert_eq!(catalog.relation_names(), vec![
            "edge".to_string(),
            "r".to_string()
        ]);
    }

    /// `shared_tuples` is a handle on the catalog's own batch, not a copy: the
    /// pointer is the one `tuples` lends, and each call adds one strong count.
    #[test]
    fn shared_tuples_hands_out_the_catalogs_own_rows() {
        let catalog = Catalog::new(vec![edge(), r()], StatisticsLevel::TupleCounts);
        let shared = catalog.shared_tuples("edge").unwrap();
        assert!(std::ptr::eq(&*shared, catalog.tuples("edge").unwrap()));
        assert_eq!(shared.to_vecs(), vec![
            vec![1, 10],
            vec![1, 20],
            vec![2, 10],
            vec![3, 30]
        ]);
        assert_eq!(Arc::strong_count(&shared), 2);
        let again = catalog.shared_tuples("edge").unwrap();
        assert!(Arc::ptr_eq(&shared, &again));
        assert_eq!(Arc::strong_count(&shared), 3);
        // Another relation is another batch.
        assert!(!Arc::ptr_eq(&shared, &catalog.shared_tuples("r").unwrap()));
        assert!(catalog.shared_tuples("nope").is_none());
    }

    /// A provider holds its handle after the catalog is gone, which is why
    /// the handle is an `Arc` and not a borrow.
    #[test]
    fn a_shared_handle_outlives_the_catalog() {
        let catalog = Catalog::new(vec![edge()], StatisticsLevel::TupleCounts);
        let shared = catalog.shared_tuples("edge").unwrap();
        drop(catalog);
        assert_eq!(Arc::strong_count(&shared), 1);
        assert_eq!(shared.len(), 4);
        assert_eq!(shared.arity(), 2);
    }
}
```

- [ ] **Step 2: Run it to see it fail**

```bash
nix develop --command cargo test -p kermit --lib db::catalog
```

Expected: the build fails with `error[E0599]: no method named `shared_tuples` found for struct `Catalog``, once per use, and `error[E0433]` or `E0425` for `Arc` (not yet imported).

- [ ] **Step 3: Implement**

In `kermit/src/db/catalog.rs`, replace the imports and `Entry`

```rust
    kermit_ds::{RelationHeader, Tuples},
    std::collections::BTreeMap,
};

/// One relation: its header, its rows, and the statistics gathered from
/// them when the catalog was created.
struct Entry {
    header: RelationHeader,
    tuples: Tuples,
    statistics: RelationStats,
}
```

with

```rust
    kermit_ds::{RelationHeader, Tuples},
    std::{collections::BTreeMap, sync::Arc},
};

/// One relation: its header, its rows (shared, see
/// [`Catalog::shared_tuples`]), and the statistics gathered from them when
/// the catalog was created.
struct Entry {
    header: RelationHeader,
    tuples: Arc<Tuples>,
    statistics: RelationStats,
}
```

Replace the construction in `new`

```rust
                let entry = Entry {
                    header,
                    tuples,
                    statistics,
                };
```

with

```rust
                let entry = Entry {
                    header,
                    tuples: Arc::new(tuples),
                    statistics,
                };
```

Replace the accessor

```rust
    /// The rows of the relation called `name`, if the catalog holds one.
    pub fn tuples(&self, name: &str) -> Option<&Tuples> {
        self.relations.get(name).map(|entry| &entry.tuples)
    }
```

with

```rust
    /// The rows of the relation called `name`, if the catalog holds one.
    pub fn tuples(&self, name: &str) -> Option<&Tuples> {
        self.relations.get(name).map(|entry| Arc::as_ref(&entry.tuples))
    }

    /// A shared handle on the rows of the relation called `name`, if the
    /// catalog holds one: the catalog's own batch, not a copy. A consumer
    /// that must own its source (an engine's `'static` table provider) keeps
    /// the handle and streams from it, so the rows exist once.
    pub fn shared_tuples(&self, name: &str) -> Option<Arc<Tuples>> {
        self.relations.get(name).map(|entry| Arc::clone(&entry.tuples))
    }
```

- [ ] **Step 4: Run it to see it pass**

```bash
nix develop --command cargo test -p kermit --lib db::catalog
nix develop --command cargo test -p kermit --lib
nix develop --command cargo fmt --all -- --check
nix develop --command bash -c 'RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets'
```

Expected: the catalog tests, including `shared_tuples_hands_out_the_catalogs_own_rows` and `a_shared_handle_outlives_the_catalog`, print `ok`; the whole `kermit` lib suite passes with `0 failed`; the format check and clippy print nothing.

- [ ] **Step 5: Commit**

```bash
git commit -m "refactor(db): the catalog shares its tuples through an Arc" -- kermit/src/db/catalog.rs
```

## Story 6: `kermit::engine`: the trait and the SQL translator

- **Developer:** intermediate-dev
- **Files:** `kermit/src/engine/mod.rs`, `kermit/src/engine/sql.rs`, `kermit/src/lib.rs`
- **Interfaces:** exposes `Engine`, `EngineError`, `Unsupported` and the `sql` functions (Names contract: `kermit::engine`) to Stories 7, 8 and 10; uses `Prepared`, `IndexSpec`, `Filter`, `Catalog`
- **Verify:** `cargo test -p kermit --lib`
- **Review:** deep

Adds the new library module `kermit::engine`. `mod.rs` holds the `Engine` trait exactly as the Names contract gives it, plus `EngineError` and `Unsupported`. `sql.rs` is the translator every SQL engine shares: quoting, the table names, `create_spec_table` (one `CREATE TEMP TABLE` per `IndexSpec`, columns renamed to `c0…` in output order) and `join_select` (the join over the spec tables, every variable selected once in the executed query's canonical order (`canonical_names(prepared.query())`), later bindings as equalities, `CROSS JOIN` for an atom that shares no variable). No engine exists yet and there is no cargo feature, so a default build gains only plain Rust. The code holds no landmine marker.

Three points the Names contract leaves open:
- **Two helpers beyond the contract's list**, both in `sql.rs` and both used by Story 7: `relation_table_name(relation) -> String` (the unquoted `rel_{relation}`, because DuckDB's appender takes a raw table name, and `relation_table` is its quoted form) and `column(index) -> String` (the quoted `"c{index}"`, the one spelling of a positional column that `create_spec_table`, `join_select` and an engine's base-table definition share).
- **`create_spec_table` qualifies every source column by its table** (`SELECT "rel_r"."c1" AS "c0", "rel_r"."c0" AS "c1" FROM "rel_r"`). The spec's sketch writes bare names, but a reordered spec renames a column to a name another source column already has, and a qualified reference does not depend on how an engine ranks columns against aliases. The contract fixes only that the output columns are `c0…` in output order, which holds.
- **The plan is not the canonical order.** Every optimiser plans join keys first, then payload (#116), so a variable carried by one atom comes after a later variable that two atoms carry, and an index's columns follow the plan. The golden tests therefore assert the executed query's text (`prepared.query().to_string()`) before the SQL, which makes each fixture's shape explicit; the SQL expectations are derived from those texts.

### Task 6.1: The `Engine` trait, `EngineError` and `Unsupported`

**Files:**
- Create: `kermit/src/engine/mod.rs`
- Modify: `kermit/src/lib.rs:20`
- Test: `kermit/src/engine/mod.rs`

- [ ] **Step 1: Write the failing test**

In `kermit/src/lib.rs`, the module list currently reads

```rust
pub mod db;

use {
```

Replace those three lines with

```rust
pub mod db;
pub mod engine;

use {
```

Create `kermit/src/engine/mod.rs` holding only a header and the tests. The toy engine in them borrows what it loaded from its built state (as DuckDB's will) and refuses to materialize, so the tests reach both the lifetime shape of the trait and the error path.

```rust
//! External engines: other systems' joins, run on the queries kermit prepares.

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::db::prepare,
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{RelationHeader, TrieLevels, Tuples},
    };

    /// Two binary relations, `r` and `s`, of one row each.
    fn catalog() -> Catalog {
        let relation = |name: &str| {
            let mut tuples = Tuples::new(2);
            tuples.push(&[1, 2]);
            (RelationHeader::new_positional(name, 2), tuples)
        };
        Catalog::new(
            vec![relation("r"), relation("s")],
            StatisticsLevel::TupleCounts,
        )
    }

    fn prepared(catalog: &Catalog) -> Prepared {
        let query: JoinQuery = "Q(X, Y, Z) :- r(X, Y), s(Y, Z)."
            .parse()
            .expect("a well-formed query");
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        prepare(catalog, &query, &planner, TrieLevels::All).expect("a valid query")
    }

    /// An engine whose built state borrows what it loaded, as DuckDB's does,
    /// and which cannot materialize.
    struct RelationNames;

    impl Engine for RelationNames {
        type Built<'l> = &'l [String];
        type Loaded = Vec<String>;
        type Settings = ();

        const HAS_SPACE: bool = false;
        const KEEPS_DUPLICATES: bool = false;
        const MATERIALIZE_MUTATES: bool = false;
        const NAME: &'static str = "relation-names";
        const STREAMS: bool = true;

        fn label(_settings: &()) -> String { "relation-names".to_string() }

        fn threads(_settings: &()) -> Option<usize> { None }

        fn version(loaded: &Vec<String>) -> String { format!("{} relations", loaded.len()) }

        fn supports(_prepared: &Prepared) -> Result<(), Unsupported> { Ok(()) }

        fn load(
            _settings: &(), _catalog: &Catalog, prepared: &Prepared,
        ) -> Result<Vec<String>, EngineError> {
            Ok(prepared
                .relations()
                .into_iter()
                .map(str::to_string)
                .collect())
        }

        fn build<'l>(
            loaded: &'l Vec<String>, _catalog: &Catalog, _prepared: &Prepared,
        ) -> Result<&'l [String], EngineError> {
            Ok(loaded)
        }

        fn stream(built: &&[String]) -> Result<u64, EngineError> { Ok(built.len() as u64) }

        fn materialize(_built: &&[String]) -> Result<(u64, Option<Tuples>), EngineError> {
            Err(EngineError {
                engine: Self::NAME,
                message: "cannot materialize".to_string(),
            })
        }

        fn space(_built: &&[String]) -> Option<usize> { None }

        fn rows(built: &&[String]) -> Result<Vec<Vec<usize>>, EngineError> {
            Ok(vec![vec![built.len()]])
        }
    }

    #[test]
    fn a_built_state_may_borrow_what_the_engine_loaded() {
        let catalog = catalog();
        let prepared = prepared(&catalog);
        let loaded = RelationNames::load(&(), &catalog, &prepared).expect("loads");
        let built = RelationNames::build(&loaded, &catalog, &prepared).expect("builds");
        assert_eq!(RelationNames::version(&loaded), "2 relations");
        assert_eq!(RelationNames::stream(&built).expect("streams"), 2);
        assert_eq!(RelationNames::rows(&built).expect("has rows"), vec![vec![2]]);
        assert_eq!(RelationNames::label(&()), "relation-names");
        assert_eq!(RelationNames::threads(&()), None);
        assert_eq!(RelationNames::space(&built), None);
        assert!(RelationNames::supports(&prepared).is_ok());
    }

    #[test]
    fn an_engine_error_names_its_engine() {
        let catalog = catalog();
        let prepared = prepared(&catalog);
        let loaded = RelationNames::load(&(), &catalog, &prepared).expect("loads");
        let built = RelationNames::build(&loaded, &catalog, &prepared).expect("builds");
        let error = RelationNames::materialize(&built).expect_err("this engine cannot materialize");
        assert_eq!(error.engine, "relation-names");
        assert_eq!(error.to_string(), "relation-names: cannot materialize");
        let boxed: Box<dyn std::error::Error> = Box::new(error);
        assert!(boxed.source().is_none());
    }

    #[test]
    fn an_unsupported_query_gives_its_reason() {
        let unsupported = Unsupported {
            reason: "a ten-way join".to_string(),
        };
        assert_eq!(unsupported.to_string(), "unsupported: a ten-way join");
        assert_eq!(unsupported.clone(), unsupported);
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --lib engine::`

Expected: the library's test build fails to compile. The errors say that `Engine`, `EngineError`, `Unsupported`, `Catalog` and `Prepared` are not found in this scope (`error[E0405]`, `error[E0412]`, `error[E0422]`, `error[E0433]`), then `error: could not compile `kermit` (lib test)`.

- [ ] **Step 3: Implement**

In `kermit/src/engine/mod.rs`, replace the single header line

```rust
//! External engines: other systems' joins, run on the queries kermit prepares.
```

with the header, the imports, the trait and the two error types below. The file then reads: this text, a blank line, and the `#[cfg(test)] mod tests` of Step 1.

```rust
//! External engines: other systems' joins, run on the queries kermit prepares.
//!
//! An [`Engine`] answers the query that [`prepare`](crate::db::prepare) decided. It reads the
//! executed query (one atom `#k` per index, over the atom's variables in index-column order),
//! each index's [`IndexSpec`](kermit_algos::IndexSpec) and the plan, and it never parses
//! Datalog. Constants and repeated variables are already filters on the specs, so an engine sees
//! plain equi-joins. It answers the *full* conjunctive query: every variable of the executed
//! query, in the executed query's canonical order (`canonical_names(prepared.query())`), so that on duplicate-free inputs its count equals kermit's bag
//! count and the benchmark's `expected`.
//!
//! An all-constant atom is not an atom of the executed query. It is a check, counted once by the
//! lifecycle's `check_multiplicity`, so an engine never sees one.
//!
//! `sql` is the translator that SQL engines share. Each engine lives in its own module, behind
//! its own cargo feature (off by default), so that a default build compiles none of them.

use {
    crate::db::{Catalog, Prepared},
    kermit_ds::Tuples,
    std::{error::Error, fmt},
};

/// One external system, in kermit's phases.
///
/// `load` is untimed and puts the query's relations inside the engine. `build` is timed and
/// leaves what the join needs. `stream` and `materialize` are the two timed joins. A single
/// `Settings` value (a thread count, say) selects the engine's configuration.
///
/// Every method is fallible, but a timed body cannot return an error. So the binary runs one
/// fallible preflight per query (`load`, `build`, then one count), untimed, and the timed bodies
/// then `expect`: an error that first appears after a clean preflight is a broken invariant.
pub trait Engine {
    /// The report's `system` axis: `"duckdb"`, `"nemo"`.
    const NAME: &'static str;
    /// Whether [`stream`](Engine::stream) is implemented, which `iteration` and `end_to_end`
    /// need.
    const STREAMS: bool;
    /// Whether an all-constant atom's row count multiplies the results (bag) or is clamped to 1
    /// (set), as `Relation::KEEPS_DUPLICATES` says for kermit's structures. DuckDB: true. Nemo:
    /// false.
    const KEEPS_DUPLICATES: bool;
    /// Whether [`materialize`](Engine::materialize) changes the built state (Nemo stores its
    /// result), so that every timed sample needs a fresh build.
    const MATERIALIZE_MUTATES: bool;
    /// Whether [`space`](Engine::space) reports anything. DuckDB keeps no index and reports
    /// none.
    const HAS_SPACE: bool;

    /// The engine's configuration. DuckDB: its thread count (default 1). Nemo: `()`.
    type Settings: Default + Clone + fmt::Debug;
    /// The query's relations inside the engine: what `load` leaves, untimed.
    type Loaded;
    /// What one timed `build` leaves. It may borrow the [`Loaded`](Engine::Loaded) state it was
    /// built from.
    type Built<'l>
    where
        Self: 'l;

    /// The Criterion group segment and stderr label: `"duckdb-t1"`, `"nemo"`.
    fn label(settings: &Self::Settings) -> String;
    /// The `engine_threads` axis: `Some(n)` for DuckDB, `None` for Nemo.
    fn threads(settings: &Self::Settings) -> Option<usize>;
    /// The `engine_version` axis, read from the running engine (DuckDB: `pragma_version()`).
    fn version(loaded: &Self::Loaded) -> String;
    /// Whether the engine can run `prepared`, checked before anything is loaded.
    ///
    /// # Errors
    ///
    /// The [`Unsupported`] reason, when it cannot.
    fn supports(prepared: &Prepared) -> Result<(), Unsupported>;
    /// Puts the relations `prepared` reads inside the engine. Untimed.
    ///
    /// # Errors
    ///
    /// The engine's failure to set up or to load a relation.
    fn load(
        settings: &Self::Settings, catalog: &Catalog, prepared: &Prepared,
    ) -> Result<Self::Loaded, EngineError>;
    /// The timed build: one filtered copy per `IndexSpec`, and whatever the engine builds from
    /// it. It takes the catalog because Nemo's providers stream from
    /// `Catalog::shared_tuples`.
    ///
    /// # Errors
    ///
    /// The engine's failure to build.
    fn build<'l>(
        loaded: &'l Self::Loaded, catalog: &Catalog, prepared: &Prepared,
    ) -> Result<Self::Built<'l>, EngineError>
    where
        Self: 'l;
    /// The streamed, counted join: the `iteration` metric. Only called when
    /// [`STREAMS`](Engine::STREAMS) is true.
    ///
    /// # Errors
    ///
    /// The engine's failure to run the join.
    fn stream(built: &Self::Built<'_>) -> Result<u64, EngineError>;
    /// The join with its result stored, then the count of distinct full assignments: the
    /// `materialize` metric. The count comes with the sorted batch when the engine copies its rows
    /// into `Tuples` (DuckDB: `Some`, from `materialize_count`; Nemo: `None`), returned so that
    /// Criterion frees it after the timer stops. `EngineSubject` maps it onto
    /// `Materialized { rows, kept }`.
    ///
    /// # Errors
    ///
    /// The engine's failure to run the join.
    fn materialize(built: &Self::Built<'_>) -> Result<(u64, Option<Tuples>), EngineError>;
    /// The exact space of what `build` left, in bytes. `Some` only if
    /// [`HAS_SPACE`](Engine::HAS_SPACE).
    fn space(built: &Self::Built<'_>) -> Option<usize>;
    /// The full assignments, in the executed query's canonical order, for tests; never timed. Nemo runs its plan
    /// first if `materialize` has not.
    ///
    /// # Errors
    ///
    /// The engine's failure to run the join.
    fn rows(built: &Self::Built<'_>) -> Result<Vec<Vec<usize>>, EngineError>;
}

/// Why an engine cannot run a prepared query, as [`Engine::supports`] reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    /// What the engine cannot do with the query.
    pub reason: String,
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported: {}", self.reason)
    }
}

impl Error for Unsupported {}

/// An engine's failure to load, build or run a query.
#[derive(Debug)]
pub struct EngineError {
    /// The failing engine's [`Engine::NAME`].
    pub engine: &'static str,
    /// What went wrong, in the engine's own words.
    pub message: String,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.engine, self.message)
    }
}

impl Error for EngineError {}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --lib engine::`

Expected: `running 3 tests`, the three tests `engine::tests::a_built_state_may_borrow_what_the_engine_loaded`, `engine::tests::an_engine_error_names_its_engine` and `engine::tests::an_unsupported_query_gives_its_reason` pass, and `test result: ok. 3 passed`.

Then run `nix develop --command cargo fmt --all` and `nix develop --command cargo clippy -p kermit --all-targets -- -D warnings`; both finish without a diagnostic.

- [ ] **Step 5: Commit**

```bash
git add -N kermit/src/engine/mod.rs
git commit -m "feat(engine): the Engine trait, EngineError and Unsupported" -- kermit/src/engine/mod.rs kermit/src/lib.rs
```

### Task 6.2: The SQL translator

**Files:**
- Create: `kermit/src/engine/sql.rs`
- Modify: `kermit/src/engine/mod.rs` (the `sql` module declaration, before the `use` block)
- Test: `kermit/src/engine/sql.rs`

- [ ] **Step 1: Write the failing test**

In `kermit/src/engine/mod.rs`, the non-test code now begins (after the `//!` header) with

```rust
use {
    crate::db::{Catalog, Prepared},
    kermit_ds::Tuples,
    std::{error::Error, fmt},
};
```

Replace those five lines with

```rust
pub mod sql;

use {
    crate::db::{Catalog, Prepared},
    kermit_ds::Tuples,
    std::{error::Error, fmt},
};
```

Create `kermit/src/engine/sql.rs` holding only a header and the tests. The relations in the fixtures are empty, since the translator reads the plan and never the data. Every expectation below is derived from the executed query that its test asserts first.

```rust
//! The SQL translator that every SQL engine shares.

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::db::{prepare, Catalog},
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{Filter, RelationHeader, TrieLevels, Tuples},
    };

    /// A catalog of empty relations, each `(name, arity)`.
    fn catalog(relations: &[(&str, usize)]) -> Catalog {
        Catalog::new(
            relations
                .iter()
                .map(|&(name, arity)| {
                    (
                        RelationHeader::new_positional(name, arity),
                        Tuples::new(arity),
                    )
                })
                .collect(),
            StatisticsLevel::TupleCounts,
        )
    }

    fn prepared(catalog: &Catalog, text: &str) -> Prepared {
        let query: JoinQuery = text.parse().expect("a well-formed query");
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        prepare(catalog, &query, &planner, TrieLevels::All).expect("a valid query")
    }

    #[test]
    fn identifiers_are_double_quoted_with_embedded_quotes_doubled() {
        assert_eq!(quote_identifier("edge"), r#""edge""#);
        assert_eq!(quote_identifier(r#"a"b"#), r#""a""b""#);
        assert_eq!(quote_identifier(""), r#""""#);
    }

    #[test]
    fn tables_and_columns_have_fixed_names() {
        assert_eq!(relation_table_name("edge"), "rel_edge");
        assert_eq!(relation_table("edge"), r#""rel_edge""#);
        assert_eq!(spec_table(3, 12), r#""idx_3_12""#);
        assert_eq!(column(2), r#""c2""#);
    }

    #[test]
    fn an_unfiltered_spec_copies_its_columns_under_positional_names() {
        let spec = IndexSpec {
            relation: "e".to_string(),
            filter: Filter::none(),
            columns: vec![0, 1],
            levels: 2,
        };
        assert_eq!(
            create_spec_table(&spec, 0, 9),
            r#"CREATE TEMP TABLE "idx_0_9" AS SELECT "rel_e"."c0" AS "c0", "rel_e"."c1" AS "c1" FROM "rel_e""#
        );
    }

    /// `r(Y, X, c42)` under the plan `X, Y`: the index keeps columns 1 and 0, in that order, under
    /// the names `c0` and `c1`, and only the rows whose column 2 holds 42.
    #[test]
    fn a_reordered_spec_with_a_constant_renames_its_columns_into_output_order() {
        let catalog = catalog(&[("r", 3)]);
        let prepared = prepared(&catalog, "Q(X, Y) :- r(Y, X, c42).");
        assert_eq!(prepared.query().to_string(), "Q(X, Y) :- #0(X, Y).");
        assert_eq!(
            create_spec_table(&prepared.indexes()[0], 0, 5),
            r#"CREATE TEMP TABLE "idx_0_5" AS SELECT "rel_r"."c1" AS "c0", "rel_r"."c0" AS "c1" FROM "rel_r" WHERE "rel_r"."c2" = 42"#
        );
    }

    #[test]
    fn a_repeated_variable_is_an_equality_and_beside_a_constant_follows_it() {
        let catalog = catalog(&[("r", 2), ("t", 3)]);
        let diagonal = prepared(&catalog, "Q(X) :- r(X, X).");
        assert_eq!(diagonal.query().to_string(), "Q(X) :- #0(X).");
        assert_eq!(
            create_spec_table(&diagonal.indexes()[0], 0, 1),
            r#"CREATE TEMP TABLE "idx_0_1" AS SELECT "rel_r"."c0" AS "c0" FROM "rel_r" WHERE "rel_r"."c0" = "rel_r"."c1""#
        );
        let both = prepared(&catalog, "Q(X) :- t(X, c5, X).");
        assert_eq!(both.query().to_string(), "Q(X) :- #0(X).");
        assert_eq!(
            create_spec_table(&both.indexes()[0], 0, 1),
            r#"CREATE TEMP TABLE "idx_0_1" AS SELECT "rel_t"."c0" AS "c0" FROM "rel_t" WHERE "rel_t"."c1" = 5 AND "rel_t"."c0" = "rel_t"."c2""#
        );
    }

    #[test]
    fn every_condition_of_a_filter_is_joined_with_and() {
        let spec = IndexSpec {
            relation: "t".to_string(),
            filter: Filter::new(vec![(1, 5), (3, 7)], vec![(0, 2)]),
            columns: vec![2, 0],
            levels: 2,
        };
        assert_eq!(
            create_spec_table(&spec, 4, 1),
            r#"CREATE TEMP TABLE "idx_4_1" AS SELECT "rel_t"."c2" AS "c0", "rel_t"."c0" AS "c1" FROM "rel_t" WHERE "rel_t"."c1" = 5 AND "rel_t"."c3" = 7 AND "rel_t"."c0" = "rel_t"."c2""#
        );
    }

    /// All three atoms read one merged spec, through three aliases. Each later binding of a
    /// variable is an equality with its first.
    #[test]
    fn a_self_join_reads_one_merged_spec_through_several_aliases() {
        let catalog = catalog(&[("e", 2)]);
        let prepared = prepared(&catalog, "Q(X, Y, Z) :- e(X, Y), e(Y, Z), e(X, Z).");
        assert_eq!(
            prepared.query().to_string(),
            "Q(X, Y, Z) :- #0(X, Y), #0(Y, Z), #0(X, Z)."
        );
        assert_eq!(prepared.indexes().len(), 1);
        assert_eq!(
            join_select(&prepared, 7),
            [
                r#"SELECT a0."c0" AS "X", a0."c1" AS "Y", a1."c1" AS "Z""#,
                r#"FROM "idx_0_7" AS a0"#,
                r#"JOIN "idx_0_7" AS a1 ON a1."c0" = a0."c1""#,
                r#"JOIN "idx_0_7" AS a2 ON a2."c0" = a0."c0" AND a2."c1" = a1."c1""#,
            ]
            .join("\n")
        );
    }

    #[test]
    fn an_atom_that_shares_no_variable_cross_joins() {
        let catalog = catalog(&[("r", 1), ("s", 1)]);
        let prepared = prepared(&catalog, "Q(X, Y) :- r(X), s(Y).");
        assert_eq!(prepared.query().to_string(), "Q(X, Y) :- #0(X), #1(Y).");
        assert_eq!(
            join_select(&prepared, 3),
            [
                r#"SELECT a0."c0" AS "X", a1."c0" AS "Y""#,
                r#"FROM "idx_0_3" AS a0"#,
                r#"CROSS JOIN "idx_1_3" AS a1"#,
            ]
            .join("\n")
        );
    }

    /// `Y` is carried by both atoms and `X` by one, so the plan binds `Y` first and the first
    /// index reads its columns reversed. The select list still follows the canonical order of the
    /// executed query (`canonical_names(prepared.query())`): the head's `X`, then the body-only
    /// `Y`. The constant is a filter on the second spec.
    #[test]
    fn a_body_only_variable_is_selected_after_the_head_in_canonical_order() {
        let catalog = catalog(&[("r", 2), ("s", 2)]);
        let prepared = prepared(&catalog, "Q(X) :- r(X, Y), s(Y, c5).");
        assert_eq!(prepared.query().to_string(), "Q(X) :- #0(Y, X), #1(Y).");
        assert_eq!(
            create_spec_table(&prepared.indexes()[0], 0, 2),
            r#"CREATE TEMP TABLE "idx_0_2" AS SELECT "rel_r"."c1" AS "c0", "rel_r"."c0" AS "c1" FROM "rel_r""#
        );
        assert_eq!(
            create_spec_table(&prepared.indexes()[1], 1, 2),
            r#"CREATE TEMP TABLE "idx_1_2" AS SELECT "rel_s"."c0" AS "c0" FROM "rel_s" WHERE "rel_s"."c1" = 5"#
        );
        assert_eq!(
            join_select(&prepared, 2),
            [
                r#"SELECT a0."c1" AS "X", a0."c0" AS "Y""#,
                r#"FROM "idx_0_2" AS a0"#,
                r#"JOIN "idx_1_2" AS a1 ON a1."c0" = a0."c0""#,
            ]
            .join("\n")
        );
    }

    /// The join must read exactly the tables the specs create, under the same build suffix.
    #[test]
    fn the_join_reads_the_tables_the_specs_create() {
        let catalog = catalog(&[("r", 2), ("s", 2)]);
        let prepared = prepared(&catalog, "Q(X) :- r(X, Y), s(Y, c5).");
        let join = join_select(&prepared, 11);
        assert_eq!(prepared.indexes().len(), 2);
        for (index, spec) in prepared.indexes().iter().enumerate() {
            let table = spec_table(index, 11);
            assert!(join.contains(&table), "{join} does not read {table}");
            assert!(
                create_spec_table(spec, index, 11)
                    .starts_with(&format!("CREATE TEMP TABLE {table} AS ")),
                "spec {index} creates another table than {table}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "a query of checks alone has no join to translate")]
    fn a_query_of_checks_alone_has_nothing_to_translate() {
        let catalog = catalog(&[("s", 2)]);
        let mut query: JoinQuery = "Q(X) :- s(c7, c8).".parse().expect("a well-formed query");
        query.head.terms.clear();
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared =
            prepare(&catalog, &query, &planner, TrieLevels::All).expect("a query of checks");
        join_select(&prepared, 0);
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --lib engine::sql`

Expected: the library's test build fails to compile, with errors that `Prepared`, `IndexSpec`, `quote_identifier`, `relation_table_name`, `relation_table`, `spec_table`, `column`, `create_spec_table` and `join_select` are not found in this scope, then `error: could not compile `kermit` (lib test)`.

- [ ] **Step 3: Implement**

In `kermit/src/engine/sql.rs`, replace the single header line

```rust
//! The SQL translator that every SQL engine shares.
```

with the header and code below, and keep `#[cfg(test)] mod tests` after it.

```rust
//! The SQL translator that every SQL engine shares.
//!
//! It reads the executed query that `prepare` leaves and its index specs. `extract_accesses` and
//! `plan_indexes` guarantee the shape: every atom names an index id `#k`, holds one variable per
//! column in index-column order, and holds no constant, placeholder or repeated variable. Those
//! became filters on the specs.
//!
//! The query becomes two kinds of statement:
//! - [`create_spec_table`], one per spec: a temporary table `idx_{index}_{build}` of the spec's
//!   filtered, projected rows, its columns named `c0…` in output order. The base relations are
//!   tables `rel_{relation}` with columns `c0…`, which the engine loads before it builds.
//! - [`join_select`], one per query: the join over the spec tables. Atom `i` becomes the alias
//!   `a{i}` and its `j`-th variable the column `c{j}` of it. Every variable is selected once, at
//!   its first binding, in the executed query's canonical order
//!   (`canonical_names(prepared.query())`, the order kermit's executors emit); every later
//!   binding is an equality with the first; an atom that shares no variable with the atoms
//!   before it joins by `CROSS JOIN`.
//!
//! The build suffix keeps the tables of one build apart from another's, so the describe step's
//! set and a timed build can coexist in one database. Identifiers are double-quoted. DuckDB
//! (like SQL generally) compares even quoted identifiers without regard to case, so two
//! relations whose names differ only in case would share a table: the load then fails with the
//! database's "already exists" error rather than reading wrong rows.

use {
    crate::db::Prepared,
    kermit_algos::{canonical_names, IndexSpec, INDEX_ID_PREFIX},
    kermit_parser::{Predicate, Term},
    std::collections::HashMap,
};

/// `name` as a double-quoted SQL identifier, any `"` inside it doubled.
pub fn quote_identifier(name: &str) -> String { format!("\"{}\"", name.replace('"', "\"\"")) }

/// The unquoted name of the base table that holds `relation`: `rel_{relation}`. An appender takes
/// a table name raw, so it needs this form; a statement needs [`relation_table`].
pub fn relation_table_name(relation: &str) -> String { format!("rel_{relation}") }

/// The quoted base table that holds `relation`.
pub fn relation_table(relation: &str) -> String {
    quote_identifier(&relation_table_name(relation))
}

/// The quoted table of spec `index` in build `build`: `idx_{index}_{build}`.
pub fn spec_table(index: usize, build: u64) -> String {
    quote_identifier(&format!("idx_{index}_{build}"))
}

/// The quoted name of positional column `index`: `c{index}`. The base tables, the spec tables and
/// the join all name their columns this way.
pub fn column(index: usize) -> String { quote_identifier(&format!("c{index}")) }

/// `CREATE TEMP TABLE` for spec number `index` of build `build`: the spec's columns, renamed to
/// `c0…` in output order, of the rows of its relation that its filter admits. Constants become
/// `c{c} = {value}` and equalities `c{a} = c{b}`, joined by `AND`; an empty filter has no `WHERE`.
/// Every source column is qualified by its table.
///
/// `spec.levels` is ignored: engines prepare under `TrieLevels::All`, where it equals the number
/// of columns.
pub fn create_spec_table(spec: &IndexSpec, index: usize, build: u64) -> String {
    // A source column is qualified by its table, since an output name can equal another source
    // column's name (`"c1" AS "c0"`): the reference then never depends on an engine's alias rules.
    let table = relation_table(&spec.relation);
    let source = |position: usize| format!("{table}.{}", column(position));
    let columns: Vec<String> = spec
        .columns
        .iter()
        .enumerate()
        .map(|(output, &from)| format!("{} AS {}", source(from), column(output)))
        .collect();
    let constants = spec
        .filter
        .constants()
        .iter()
        .map(|&(from, value)| format!("{} = {value}", source(from)));
    let equalities = spec
        .filter
        .equalities()
        .iter()
        .map(|&(earlier, later)| format!("{} = {}", source(earlier), source(later)));
    let conditions: Vec<String> = constants.chain(equalities).collect();
    let filter = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    format!(
        "CREATE TEMP TABLE {} AS SELECT {} FROM {table}{filter}",
        spec_table(index, build),
        columns.join(", "),
    )
}

/// The index an executed atom `#k` reads.
fn index_of(atom: &Predicate) -> usize {
    atom.name
        .strip_prefix(INDEX_ID_PREFIX)
        .and_then(|digits| digits.parse().ok())
        .unwrap_or_else(|| panic!("join_select: `{}` does not name an index", atom.name))
}

/// The join of `prepared`'s executed query over the spec tables of build `build`, as one `SELECT`
/// (clauses on separate lines, no trailing semicolon).
///
/// Every variable of the executed query is selected once, in its canonical order
/// (`canonical_names(prepared.query())`), as
/// `a{alias}."c{column}" AS "{variable}"` at its first binding, so a row is one full assignment.
/// Each later binding of a variable is an equality with the first, and an atom with no equality
/// to the atoms before it is a `CROSS JOIN`.
///
/// # Panics
///
/// If the executed query has no atom (a query of checks alone never reaches an engine), or an
/// atom holds anything but distinct variables (`prepare` guarantees it does not).
pub fn join_select(prepared: &Prepared, build: u64) -> String {
    let query = prepared.query();
    assert!(
        !query.body.is_empty(),
        "join_select: a query of checks alone has no join to translate"
    );

    // Where each variable is first bound: (alias, column).
    let mut first: HashMap<&str, (usize, usize)> = HashMap::new();
    let mut clauses: Vec<String> = Vec::with_capacity(query.body.len());
    for (alias, atom) in query.body.iter().enumerate() {
        let table = spec_table(index_of(atom), build);
        let mut conditions: Vec<String> = Vec::new();
        for (position, term) in atom.terms.iter().enumerate() {
            let Term::Var(name) = term else {
                panic!("join_select: `{atom}` holds a term that is not a variable");
            };
            match first.get(name.as_str()) {
                | Some(&(earlier, source)) => conditions.push(format!(
                    "a{alias}.{} = a{earlier}.{}",
                    column(position),
                    column(source)
                )),
                | None => {
                    first.insert(name.as_str(), (alias, position));
                },
            }
        }
        assert!(
            alias > 0 || conditions.is_empty(),
            "join_select: `{atom}` repeats a variable"
        );
        clauses.push(match (alias, conditions.is_empty()) {
            | (0, _) => format!("FROM {table} AS a0"),
            | (_, true) => format!("CROSS JOIN {table} AS a{alias}"),
            | (_, false) => format!(
                "JOIN {table} AS a{alias} ON {}",
                conditions.join(" AND ")
            ),
        });
    }

    let select: Vec<String> = canonical_names(query)
        .iter()
        .map(|name| {
            let &(alias, position) = first
                .get(name.as_str())
                .expect("every variable of the executed query is bound by an atom");
            format!("a{alias}.{} AS {}", column(position), quote_identifier(name))
        })
        .collect();
    format!("SELECT {}\n{}", select.join(", "), clauses.join("\n"))
}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --lib engine::`

Expected: `running 14 tests` (the 3 of Task 6.1 and the 11 of `engine::sql::tests`), all `ok`, and `test result: ok. 14 passed`.

Then run `nix develop --command cargo fmt --all`, `nix develop --command bash -c 'RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets'` and `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit'`; each finishes without a diagnostic. Finally run `nix develop --command cargo test -p kermit --lib` to see the whole library's tests pass.

- [ ] **Step 5: Commit**

```bash
git add -N kermit/src/engine/sql.rs
git commit -m "feat(engine): the SQL translator shared by SQL engines" -- kermit/src/engine/sql.rs kermit/src/engine/mod.rs
```

## Story 7: DuckDB

- **Developer:** intermediate-dev
- **Files:** `kermit/src/engine/duckdb.rs`, `kermit/src/engine/mod.rs`, `kermit/Cargo.toml`, `kermit/build.rs`, `Cargo.toml`, `kermit/tests/engine_oracle.rs`, `.github/workflows/pr.yml`
- **Interfaces:** exposes `DuckDb`, `DuckDbSettings` and the features `engine-duckdb` / `engine-duckdb-bundled` (Names contract: the engines) to Stories 10 and 12; uses Stories 2 and 6
- **Verify:** `bash -c 'cargo test -p kermit --features engine-duckdb --lib && cargo test -p kermit --features engine-duckdb --test engine_oracle && RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets --features engine-duckdb && cargo test -p kermit --lib'`
- **Review:** deep (the `temp_directory` landmine: a private spill directory per connection, as the JOB generator keeps one per call)

Adds `kermit::engine::duckdb` behind the cargo feature `engine-duckdb`, which links Nix's `libduckdb` through `DUCKDB_LIB_DIR` (the dev shell sets it) and compiles no C++; `engine-duckdb-bundled` compiles DuckDB's amalgamation instead, for CI. A new `kermit/build.rs` adds the rpath so that a dev-shell binary finds `libduckdb.so` outside the shell, and fails a build that has neither the variable nor bundling. `DuckDb` loads each relation an index reads through the appender (`UBIGINT` columns), builds one temporary table per `IndexSpec` and prepares the join statement as the last step of `build` (decision 11), streams Arrow batches for `stream`, and copies them into `Tuples` for `materialize` and `rows`, which go through Story 2's `materialize_count`. Every connection sets `memory_limit = '8GB'` and a private `temp_directory`, `kermit-duckdb-<pid>-<n>` under `std::env::temp_dir()`, removed after the connection closes. The story also creates `kermit/tests/engine_oracle.rs` with DuckDB's cells and adds the `engines` CI job.

The code holds a landmine marker: `SET temp_directory`. DuckDB deletes every spill file in its temp directory when a connection closes, so two connections sharing a directory would corrupt each other's spills. The supervisor should dispatch this story's developer with `effort: xhigh`.

Points the Names contract leaves open:
- **`Engine::materialize` returns `(count, Option<Tuples>)`** (Names contract, amendment 3), so `DuckDb::materialize` returns `Ok((count, Some(sorted)))` from `materialize_count`: the sorted batch leaves the timed call, and Criterion frees it after the timer stops, as it does for kermit's cells. `EngineSubject` (Story 10) maps the pair onto `Materialized { rows, kept }`.
- **The Arrow schema is built by hand** (`width` columns of `UInt64`). `Statement::schema` panics until a non-streaming execution has run, and `stream_arrow` needs a schema before it executes, so the schema cannot be read from the prepared statement. Every selected column is a `UBIGINT` (the base tables are, and the spec tables copy them).
- **`RefCell<Statement>`.** `stream_arrow` takes `&mut Statement` and `Engine::stream` takes `&Built`, so the prepared statement sits in a `RefCell` for the borrow only. Nothing observable changes between runs, so `MATERIALIZE_MUTATES` stays false, and decision 11's point (no lazily initialised cell) stands.
- **`Cargo.lock` is untracked** (`.gitignore:8`): it is not in this story's **Files** line, it changes by building, and no commit names it.
- **`engine_oracle.rs` is gated on `engine-duckdb` alone**, because `engine-nemo` does not exist yet (`unexpected_cfgs` would warn). Story 8 widens the gate to `any(...)`; the DuckDB-only code is already in one module, `duckdb_cells`, for that edit.
- **Loaded relations are those of the specs**, not of the checks: an all-constant atom is counted by `check_multiplicity` from the catalog, so DuckDB never reads its relation.

### Task 7.1: The features, the dependency and the build script

**Files:**
- Modify: `kermit/Cargo.toml:13-18` (the features) and `kermit/Cargo.toml:35-36` (the dependency)
- Create: `kermit/build.rs`
- Modify: `Cargo.toml:42-43`

- [ ] **Step 1: Write the failing check**

There is no Rust test yet: this task's checks are cargo's, and a run of the build script by hand. First see that the feature and the script do not exist.

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo check -p kermit --features engine-duckdb`

Expected: `error: the package `kermit` does not contain this feature: engine-duckdb`.

Run: `nix develop --command rustc --edition 2021 -o /dev/null kermit/build.rs`

Expected: `error: couldn't read `kermit/build.rs``.

- [ ] **Step 3: Implement**

In `kermit/Cargo.toml`, the features currently end with

```toml
jemalloc = ["dep:tikv-jemallocator"]

[dependencies]
```

Replace those three lines with

```toml
jemalloc = ["dep:tikv-jemallocator"]
# External engines (docs/specs/2026-11-02-external-engines-design.md), off by default.
# `engine-duckdb` links the system libduckdb, found through DUCKDB_LIB_DIR and
# DUCKDB_INCLUDE_DIR, which the nix dev shell sets (build.rs adds the rpath). A dev-shell
# build compiles no C++. `engine-duckdb-bundled` compiles DuckDB's amalgamation instead,
# for CI, which has no Nix.
engine-duckdb = ["dep:duckdb"]
engine-duckdb-bundled = ["engine-duckdb", "duckdb/bundled"]

[dependencies]
```

In the same file, the last dependency currently reads

```toml
# Bundles jemalloc 5.3.1, the version #112 measured.
tikv-jemallocator = { version = "0.7", optional = true }
```

Replace those two lines with

```toml
# Bundles jemalloc 5.3.1, the version #112 measured.
tikv-jemallocator = { version = "0.7", optional = true }
# The `~1.4` line is DuckDB 1.4.x, which matches the flake's DuckDB (the one that computes
# JOB's `expected`). The newest crate is DuckDB 2.0.
duckdb = { version = "~1.4", default-features = false, optional = true }
```

Create `kermit/build.rs`:

```rust
//! Build script for the `kermit` package.
//!
//! The `engine-duckdb` feature links the system `libduckdb`, which the `libduckdb-sys` build
//! script finds through `DUCKDB_LIB_DIR`. A dependency's link arguments do not reach this
//! package's executables, so this script adds the rpath: a binary built in the nix dev shell then
//! finds Nix's `libduckdb.so` when it runs outside the shell, as the sweeps' binaries do. With
//! `engine-duckdb-bundled`, DuckDB is compiled into the executable and there is nothing to find.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DUCKDB_LIB_DIR");

    let links_system_library = env::var_os("CARGO_FEATURE_ENGINE_DUCKDB").is_some()
        && env::var_os("CARGO_FEATURE_ENGINE_DUCKDB_BUNDLED").is_none();
    if !links_system_library {
        return;
    }
    match env::var("DUCKDB_LIB_DIR") {
        | Ok(directory) if !directory.is_empty() => {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{directory}");
        },
        | _ => panic!(
            "engine-duckdb needs DUCKDB_LIB_DIR (the nix dev shell sets it) or the \
             engine-duckdb-bundled feature"
        ),
    }
}
```

In the root `Cargo.toml`, the file currently ends with

```toml
[profile.dev.package.tikv-jemalloc-sys]
opt-level = 3
```

Append after it

```toml

# `engine-duckdb-bundled` compiles DuckDB's C++ amalgamation through libduckdb-sys; at the dev
# profile's -O0 a debug test of the engine would crawl. The system-library build
# (`engine-duckdb`) compiles no C++, so this entry costs it nothing.
[profile.dev.package.libduckdb-sys]
opt-level = 3
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo check -p kermit --features engine-duckdb`

Expected: cargo resolves `duckdb`, `libduckdb-sys` and their dependencies (the untracked `Cargo.lock` changes by this build), compiles them, and prints `Finished`. The `libduckdb-sys` build script reads `DUCKDB_LIB_DIR` and compiles no C++.

Run the build script by hand under the environments cargo gives it, which is deterministic where a failing `cargo check` is not (the `libduckdb-sys` script may fail first):

```bash
nix develop --command bash -c '
d=$(mktemp -d)
rustc --edition 2021 -o "$d/build-script" kermit/build.rs
echo "--- linked, DUCKDB_LIB_DIR unset"
env -u DUCKDB_LIB_DIR CARGO_FEATURE_ENGINE_DUCKDB=1 "$d/build-script" 2>&1 | grep "engine-duckdb needs"
echo "--- linked, DUCKDB_LIB_DIR set"
env DUCKDB_LIB_DIR=/opt/duckdb/lib CARGO_FEATURE_ENGINE_DUCKDB=1 "$d/build-script"
echo "--- bundled"
env -u DUCKDB_LIB_DIR CARGO_FEATURE_ENGINE_DUCKDB=1 CARGO_FEATURE_ENGINE_DUCKDB_BUNDLED=1 "$d/build-script"
echo "--- no engine feature"
env -u DUCKDB_LIB_DIR "$d/build-script"
rm -r "$d"'
```

Expected output:
- under `--- linked, DUCKDB_LIB_DIR unset`, a line containing `engine-duckdb needs DUCKDB_LIB_DIR (the nix dev shell sets it) or the engine-duckdb-bundled feature`;
- under `--- linked, DUCKDB_LIB_DIR set`, the three lines `cargo:rerun-if-changed=build.rs`, `cargo:rerun-if-env-changed=DUCKDB_LIB_DIR` and `cargo:rustc-link-arg=-Wl,-rpath,/opt/duckdb/lib`;
- under `--- bundled` and under `--- no engine feature`, only the first two of those lines.

Run: `nix develop --command cargo check -p kermit` and then `nix develop --command bash -c '! cargo tree -p kermit | grep -q duckdb'`

Expected: the default build finishes as before, and the second command exits 0, which is to say `duckdb` is not in the default dependency tree.

- [ ] **Step 5: Commit**

```bash
git add -N kermit/build.rs
git commit -m "build(kermit): engine-duckdb and engine-duckdb-bundled features, with an rpath build script" -- kermit/Cargo.toml kermit/build.rs Cargo.toml
```

### Task 7.2: The DuckDB engine

**Files:**
- Create: `kermit/src/engine/duckdb.rs`
- Modify: `kermit/src/engine/mod.rs` (the module declaration)
- Test: `kermit/src/engine/duckdb.rs`

- [ ] **Step 1: Write the failing test**

In `kermit/src/engine/mod.rs`, the non-test code now begins (after the `//!` header) with

```rust
pub mod sql;

use {
    crate::db::{Catalog, Prepared},
```

Replace those four lines with

```rust
#[cfg(feature = "engine-duckdb")]
pub mod duckdb;
pub mod sql;

use {
    crate::db::{Catalog, Prepared},
```

Create `kermit/src/engine/duckdb.rs` holding only a header and the tests. The fixture is a directed graph whose edge `1 -> 2` is stored twice, so its one triangle walk `1 2 3` has two derivations: a bag of two rows, one distinct row.

```rust
//! DuckDB, run in-process through the `duckdb` crate (feature `engine-duckdb`).

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::db::prepare,
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{RelationHeader, TrieLevels},
    };

    const TRIANGLE: &str = "Q(X, Y, Z) :- e(X, Y), e(Y, Z), e(X, Z).";

    /// A catalog of relations, each `(name, arity, rows)`.
    fn catalog(relations: Vec<(&str, usize, Vec<Vec<usize>>)>) -> Catalog {
        let relations = relations
            .into_iter()
            .map(|(name, arity, rows)| {
                let mut tuples = Tuples::new(arity);
                for row in &rows {
                    tuples.push(row);
                }
                (RelationHeader::new_positional(name, arity), tuples)
            })
            .collect();
        Catalog::new(relations, StatisticsLevel::TupleCounts)
    }

    /// The edge `1 -> 2` twice, then `2 -> 3` and `1 -> 3`.
    fn graph() -> Catalog {
        catalog(vec![("e", 2, vec![
            vec![1, 2],
            vec![2, 3],
            vec![1, 3],
            vec![1, 2],
        ])])
    }

    fn prepared(catalog: &Catalog, text: &str) -> Prepared {
        let query: JoinQuery = text.parse().expect("a well-formed query");
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        prepare(catalog, &query, &planner, TrieLevels::All).expect("a valid query")
    }

    fn load(catalog: &Catalog, prepared: &Prepared) -> DuckDbLoaded {
        DuckDb::load(&DuckDbSettings::default(), catalog, prepared).expect("DuckDB loads")
    }

    /// The value of one of the connection's settings, as text.
    fn setting(loaded: &DuckDbLoaded, name: &str) -> String {
        loaded
            .connection
            .query_row(
                &format!("SELECT CAST(current_setting('{name}') AS VARCHAR)"),
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("a setting")
    }

    /// How many spec tables the connection holds.
    fn spec_tables(loaded: &DuckDbLoaded) -> i64 {
        loaded
            .connection
            .query_row(
                "SELECT count(*) FROM duckdb_tables() WHERE table_name LIKE 'idx%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("a count")
    }

    #[test]
    fn the_engine_is_named_by_its_settings() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        assert_eq!(DuckDbSettings::default().threads, 1);
        assert_eq!(DuckDb::label(&DuckDbSettings::default()), "duckdb-t1");
        assert_eq!(DuckDb::label(&DuckDbSettings { threads: 4 }), "duckdb-t4");
        assert_eq!(DuckDb::threads(&DuckDbSettings { threads: 4 }), Some(4));
        assert!(DuckDb::supports(&prepared).is_ok());
    }

    #[test]
    fn a_zero_thread_count_is_refused() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        let Err(error) = DuckDb::load(&DuckDbSettings { threads: 0 }, &catalog, &prepared) else {
            panic!("a thread count of zero must be refused");
        };
        assert_eq!(error.engine, "duckdb");
        assert!(error.message.contains("at least 1"), "{error}");
    }

    #[test]
    fn a_catalog_without_the_relation_is_an_engine_error() {
        let with = graph();
        let prepared = prepared(&with, TRIANGLE);
        let without = catalog(vec![("other", 1, vec![vec![1]])]);
        let Err(error) = DuckDb::load(&DuckDbSettings::default(), &without, &prepared) else {
            panic!("a catalog without the relation must be refused");
        };
        assert_eq!(error.engine, "duckdb");
        assert!(error.message.contains("`e`"), "{error}");
    }

    #[test]
    fn the_library_version_is_the_one_that_runs() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        let loaded = load(&catalog, &prepared);
        let version = DuckDb::version(&loaded);
        assert!(version.starts_with("v1.4"), "unexpected DuckDB version {version}");
    }

    #[test]
    fn a_connection_gets_its_threads_a_memory_limit_and_a_private_spill_directory() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        let loaded = DuckDb::load(&DuckDbSettings { threads: 3 }, &catalog, &prepared)
            .expect("DuckDB loads");
        assert_eq!(setting(&loaded, "threads"), "3");
        // 8GB is about 7.5 GiB, which DuckDB prints in binary units.
        let memory_limit = setting(&loaded, "memory_limit");
        assert!(
            memory_limit.starts_with("7.") && memory_limit.ends_with("GiB"),
            "memory_limit is {memory_limit}"
        );
        let directory = loaded
            ._spill
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a UTF-8 directory name")
            .to_string();
        assert!(
            directory.starts_with(&format!("kermit-duckdb-{}-", std::process::id())),
            "unexpected spill directory {directory}"
        );
        assert!(
            setting(&loaded, "temp_directory").contains(&directory),
            "temp_directory does not name {directory}"
        );
    }

    #[test]
    fn each_load_has_its_own_spill_directory_and_removes_it_on_drop() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        let first = load(&catalog, &prepared);
        let second = load(&catalog, &prepared);
        let (first_path, second_path) = (first._spill.path.clone(), second._spill.path.clone());
        assert_ne!(first_path, second_path);
        assert!(first_path.is_dir() && second_path.is_dir());
        drop(first);
        assert!(!first_path.exists(), "the first directory outlived its load");
        assert!(second_path.is_dir(), "dropping one load removed the other's directory");
        drop(second);
        assert!(!second_path.exists(), "the second directory outlived its load");
    }

    #[test]
    fn stream_counts_the_bag_and_materialize_the_distinct_rows() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        let loaded = load(&catalog, &prepared);
        let built = DuckDb::build(&loaded, &catalog, &prepared).expect("DuckDB builds");
        assert_eq!(DuckDb::stream(&built).expect("streams"), 2);
        assert_eq!(
            DuckDb::stream(&built).expect("streams again"),
            2,
            "the prepared statement runs again"
        );
        let (distinct, kept) = DuckDb::materialize(&built).expect("materializes");
        assert_eq!(distinct, 1);
        assert_eq!(
            kept.expect("DuckDB returns its sorted batch").to_vecs(),
            vec![vec![1, 2, 3], vec![1, 2, 3]],
            "the batch is the sorted bag, not the distinct rows"
        );
        let mut rows = DuckDb::rows(&built).expect("returns rows");
        rows.sort();
        assert_eq!(rows, vec![vec![1, 2, 3], vec![1, 2, 3]]);
        assert_eq!(DuckDb::space(&built), None);
    }

    #[test]
    fn builds_of_one_load_coexist_and_a_dropped_build_takes_its_tables_with_it() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        let loaded = load(&catalog, &prepared);
        assert_eq!(spec_tables(&loaded), 0, "loading builds no spec table");
        let first = DuckDb::build(&loaded, &catalog, &prepared).expect("DuckDB builds");
        let second = DuckDb::build(&loaded, &catalog, &prepared).expect("DuckDB builds again");
        assert_eq!(spec_tables(&loaded), 2);
        drop(first);
        assert_eq!(spec_tables(&loaded), 1);
        assert_eq!(DuckDb::stream(&second).expect("the second build still runs"), 2);
        drop(second);
        assert_eq!(spec_tables(&loaded), 0);
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --features engine-duckdb --lib engine::duckdb`

Expected: the library's test build fails to compile, with errors that `DuckDb`, `DuckDbSettings`, `DuckDbLoaded`, `Catalog`, `Prepared` and `Tuples` are not found in this scope, then `error: could not compile `kermit` (lib test)`.

- [ ] **Step 3: Implement**

In `kermit/src/engine/duckdb.rs`, replace the single header line

```rust
//! DuckDB, run in-process through the `duckdb` crate (feature `engine-duckdb`).
```

with the code below. The file then reads: this text, a blank line, and the `#[cfg(test)] mod tests` of Step 1.

```rust
//! DuckDB, run in-process through the `duckdb` crate (feature `engine-duckdb`).
//!
//! DuckDB answers the executed query as SQL (see [`sql`](super::sql)) over one temporary table
//! per index spec, and sits in kermit's phases like this:
//! - `load` (untimed): an in-memory database with `PRAGMA threads`, a memory limit and a private
//!   spill directory. Each relation an index reads is appended from the catalog as `UBIGINT`
//!   columns.
//! - `build` (timed): one `CREATE TEMP TABLE` per spec, holding its filtered, projected rows (one
//!   copy, and no index), then the join statement prepared.
//! - `stream`: the prepared statement streamed as Arrow batches, whose rows are counted. DuckDB
//!   builds its hash tables here.
//! - `materialize`: the same statement, every batch copied into kermit `Tuples`, then
//!   [`materialize_count`](crate::db::materialize_count); the count and the sorted batch are
//!   both returned, so the batch is freed after the timer.
//! - `space`: not reported, since DuckDB keeps no index.
//!
//! DuckDB's index work sits in `stream` while kermit's sits in `build`, so the two compare on
//! `end_to_end`. The statement is prepared at the end of `build` (decision 11 of the design), so
//! `iteration` never includes DuckDB's planning, and `build` and each `end_to_end` sample include
//! it once.
//!
//! A relation whose name differs from another's only in case would share a table with it, since
//! DuckDB compares quoted identifiers without regard to case: the load then fails with the
//! database's "already exists" error.

use {
    super::{
        sql::{
            column, create_spec_table, join_select, relation_table, relation_table_name,
            spec_table,
        },
        Engine, EngineError, Unsupported,
    },
    crate::db::{materialize_count, Catalog, Prepared},
    kermit_ds::Tuples,
    std::{
        cell::{Cell, RefCell},
        fmt::Display,
        fs,
        path::PathBuf,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    },
    ::duckdb::{
        appender_params_from_iter,
        arrow::{
            array::{Array, UInt64Array},
            datatypes::{DataType, Field, Schema, SchemaRef},
        },
        Connection, Statement,
    },
};

/// DuckDB's `memory_limit` on every connection. It sits under the sweeps' 14 GiB cap, where
/// DuckDB's own default is 80% of the host's RAM.
const MEMORY_LIMIT: &str = "8GB";

/// The DuckDB engine.
pub struct DuckDb;

/// The settings of a [`DuckDb`] engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuckDbSettings {
    /// DuckDB's `PRAGMA threads`. One thread gives the parallelism of kermit's joins and serial
    /// builds.
    pub threads: usize,
}

impl Default for DuckDbSettings {
    fn default() -> Self { DuckDbSettings { threads: 1 } }
}

/// One connection's private spill directory, `kermit-duckdb-<process id>-<sequence>` under the
/// system's temporary directory, removed when the guard drops.
///
/// DuckDB deletes every spill file in its `temp_directory` when a connection closes, so two
/// connections that shared a directory would corrupt each other's spills (a run could finish
/// with wrong rows). The process id keeps two processes apart and the sequence keeps two
/// connections of this process apart, as the JOB generator's `SpillDir` does for its calls.
struct SpillDirectory {
    path: PathBuf,
}

impl SpillDirectory {
    /// Creates a fresh, empty directory.
    fn create() -> Result<SpillDirectory, EngineError> {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "kermit-duckdb-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).map_err(|e| {
            failure(
                &format!("creating the spill directory {}", path.display()),
                e,
            )
        })?;
        Ok(SpillDirectory {
            path,
        })
    }

    /// The path as the text of a SQL statement, which must be valid UTF-8.
    fn text(&self) -> Result<&str, EngineError> {
        self.path.to_str().ok_or_else(|| {
            refusal(format!(
                "the spill directory {} is not valid UTF-8",
                self.path.display()
            ))
        })
    }
}

impl Drop for SpillDirectory {
    fn drop(&mut self) {
        // Already gone is fine, and a directory left behind in the temporary directory is
        // harmless: nothing can be done about either from a destructor.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// What [`DuckDb`]'s `load` leaves: an in-memory database holding the relations the query's
/// indexes read.
pub struct DuckDbLoaded {
    connection: Connection,
    /// The suffix of the next build's tables, so that builds of one load can coexist.
    builds: Cell<u64>,
    version: String,
    /// Held for its `Drop` and never read. Declared last, so that the connection closes the
    /// database before the directory goes.
    _spill: SpillDirectory,
}

/// What one timed `build` leaves: the spec tables, dropped with it, and the prepared join
/// statement.
pub struct DuckDbBuilt<'l> {
    /// `stream_arrow` needs `&mut Statement` and `Engine::stream` has `&Built`, so the statement
    /// is borrowed through a `RefCell`; nothing observable changes between runs. Declared first,
    /// so that it drops before the tables it reads.
    statement: RefCell<Statement<'l>>,
    /// The schema `stream_arrow` decodes batches with: `width` `UInt64` columns.
    schema: SchemaRef,
    /// The executed query's variable count: the width of a row.
    width: usize,
    /// Held for its `Drop` and never read: it drops the spec tables after the statement.
    _tables: Tables<'l>,
}

/// The spec tables of one build, dropped with it, even when the build fails half way.
struct Tables<'l> {
    connection: &'l Connection,
    /// Quoted names, pushed before each `CREATE` so that a failed one is still cleaned up.
    names: Vec<String>,
}

impl Drop for Tables<'_> {
    fn drop(&mut self) {
        if self.names.is_empty() {
            return;
        }
        let drops: Vec<String> = self
            .names
            .iter()
            .map(|name| format!("DROP TABLE IF EXISTS {name};"))
            .collect();
        // A failure leaves the tables to the connection's own drop.
        let _ = self.connection.execute_batch(&drops.join(" "));
    }
}

/// An error of this engine, with the step that failed.
fn failure(context: &str, source: impl Display) -> EngineError {
    refusal(format!("{context}: {source}"))
}

/// An error of this engine with its message as written.
fn refusal(message: String) -> EngineError {
    EngineError {
        engine: DuckDb::NAME,
        message,
    }
}

/// `text` as a single-quoted SQL string literal.
fn string_literal(text: &str) -> String { format!("'{}'", text.replace('\'', "''")) }

/// The relations the query's indexes read, each once, in index order. A relation only an
/// all-constant atom reads is counted from the catalog by `check_multiplicity`, never by DuckDB.
fn indexed_relations(prepared: &Prepared) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for spec in prepared.indexes() {
        if !names.contains(&spec.relation.as_str()) {
            names.push(&spec.relation);
        }
    }
    names
}

/// Creates `relation`'s base table, `UBIGINT` columns `c0…`, and appends the catalog's rows.
fn load_relation(
    connection: &Connection, relation: &str, tuples: &Tuples,
) -> Result<(), EngineError> {
    let context = |step: &str| format!("{step} `{relation}`");
    let columns: Vec<String> = (0..tuples.arity())
        .map(|index| format!("{} UBIGINT", column(index)))
        .collect();
    connection
        .execute_batch(&format!(
            "CREATE TABLE {} ({})",
            relation_table(relation),
            columns.join(", ")
        ))
        .map_err(|e| failure(&context("creating the table of"), e))?;
    let mut appender = connection
        .appender(&relation_table_name(relation))
        .map_err(|e| failure(&context("opening an appender on"), e))?;
    for row in tuples.rows() {
        appender
            .append_row(appender_params_from_iter(row.iter()))
            .map_err(|e| failure(&context("loading"), e))?;
    }
    appender
        .flush()
        .map_err(|e| failure(&context("flushing"), e))
}

/// The schema of a join's result: `width` `UInt64` columns.
fn result_schema(width: usize) -> SchemaRef {
    let fields: Vec<Field> = (0..width)
        .map(|index| Field::new(format!("v{index}"), DataType::UInt64, true))
        .collect();
    Arc::new(Schema::new(fields))
}

/// Runs the prepared join and copies every row into a flat batch, in the statement's order.
fn collect_rows(built: &DuckDbBuilt<'_>) -> Result<Tuples, EngineError> {
    let mut statement = built.statement.borrow_mut();
    let batches = statement
        .stream_arrow([], built.schema.clone())
        .map_err(|e| failure("running the join", e))?;
    let mut rows = Tuples::new(built.width);
    let mut row = vec![0usize; built.width];
    for batch in batches {
        if batch.num_columns() != built.width {
            return Err(refusal(format!(
                "the join returned {} columns, not {}",
                batch.num_columns(),
                built.width
            )));
        }
        let mut columns: Vec<&UInt64Array> = Vec::with_capacity(built.width);
        for index in 0..built.width {
            let array = batch
                .column(index)
                .as_any()
                .downcast_ref::<UInt64Array>()
                .ok_or_else(|| refusal(format!("result column {index} is not a UBIGINT")))?;
            columns.push(array);
        }
        for at in 0..batch.num_rows() {
            for (slot, array) in row.iter_mut().zip(&columns) {
                *slot = array.value(at) as usize;
            }
            rows.push(&row);
        }
    }
    Ok(rows)
}

impl Engine for DuckDb {
    type Settings = DuckDbSettings;
    type Loaded = DuckDbLoaded;
    type Built<'l> = DuckDbBuilt<'l>;

    const NAME: &'static str = "duckdb";
    const STREAMS: bool = true;
    const KEEPS_DUPLICATES: bool = true;
    const MATERIALIZE_MUTATES: bool = false;
    const HAS_SPACE: bool = false;

    fn label(settings: &DuckDbSettings) -> String { format!("duckdb-t{}", settings.threads) }

    fn threads(settings: &DuckDbSettings) -> Option<usize> { Some(settings.threads) }

    fn version(loaded: &DuckDbLoaded) -> String { loaded.version.clone() }

    fn supports(_prepared: &Prepared) -> Result<(), Unsupported> { Ok(()) }

    fn load(
        settings: &DuckDbSettings, catalog: &Catalog, prepared: &Prepared,
    ) -> Result<DuckDbLoaded, EngineError> {
        if settings.threads == 0 {
            return Err(refusal("the thread count must be at least 1".to_string()));
        }
        // Declared before the connection, so that on every return the connection drops first
        // and closes the database before its spill directory is removed.
        let spill = SpillDirectory::create()?;
        let connection = Connection::open_in_memory()
            .map_err(|e| failure("opening an in-memory database", e))?;
        connection
            .execute_batch(&format!(
                "PRAGMA threads = {}; SET memory_limit = '{MEMORY_LIMIT}'; SET temp_directory = {};",
                settings.threads,
                string_literal(spill.text()?),
            ))
            .map_err(|e| failure("configuring the connection", e))?;
        for relation in indexed_relations(prepared) {
            let tuples = catalog
                .tuples(relation)
                .ok_or_else(|| refusal(format!("the catalog holds no relation `{relation}`")))?;
            load_relation(&connection, relation, tuples)?;
        }
        let version = connection
            .query_row("SELECT library_version FROM pragma_version()", [], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| failure("reading the library version", e))?;
        Ok(DuckDbLoaded {
            connection,
            builds: Cell::new(0),
            version,
            _spill: spill,
        })
    }

    fn build<'l>(
        loaded: &'l DuckDbLoaded, _catalog: &Catalog, prepared: &Prepared,
    ) -> Result<DuckDbBuilt<'l>, EngineError>
    where
        Self: 'l,
    {
        let build = loaded.builds.get();
        loaded.builds.set(build + 1);
        let mut tables = Tables {
            connection: &loaded.connection,
            names: Vec::new(),
        };
        for (index, spec) in prepared.indexes().iter().enumerate() {
            tables.names.push(spec_table(index, build));
            loaded
                .connection
                .execute_batch(&create_spec_table(spec, index, build))
                .map_err(|e| failure(&format!("building the table of index #{index}"), e))?;
        }
        // The last step of the build (decision 11): `stream` then never includes planning.
        let statement = loaded
            .connection
            .prepare(&join_select(prepared, build))
            .map_err(|e| failure("preparing the join", e))?;
        let width = prepared.assignment_width();
        Ok(DuckDbBuilt {
            statement: RefCell::new(statement),
            schema: result_schema(width),
            width,
            _tables: tables,
        })
    }

    fn stream(built: &DuckDbBuilt<'_>) -> Result<u64, EngineError> {
        let mut statement = built.statement.borrow_mut();
        let batches = statement
            .stream_arrow([], built.schema.clone())
            .map_err(|e| failure("running the join", e))?;
        Ok(batches.map(|batch| batch.num_rows() as u64).sum())
    }

    fn materialize(built: &DuckDbBuilt<'_>) -> Result<(u64, Option<Tuples>), EngineError> {
        // The sorted batch is returned with the count, so it is freed after the timer stops.
        let (sorted, distinct) = materialize_count(collect_rows(built)?);
        Ok((distinct, Some(sorted)))
    }

    fn space(_built: &DuckDbBuilt<'_>) -> Option<usize> { None }

    fn rows(built: &DuckDbBuilt<'_>) -> Result<Vec<Vec<usize>>, EngineError> {
        Ok(collect_rows(built)?.to_vecs())
    }
}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --features engine-duckdb --lib engine::duckdb`

Expected: `running 8 tests`; `engine::duckdb::tests::the_engine_is_named_by_its_settings`, `a_zero_thread_count_is_refused`, `a_catalog_without_the_relation_is_an_engine_error`, `the_library_version_is_the_one_that_runs`, `a_connection_gets_its_threads_a_memory_limit_and_a_private_spill_directory`, `each_load_has_its_own_spill_directory_and_removes_it_on_drop`, `stream_counts_the_bag_and_materialize_the_distinct_rows` and `builds_of_one_load_coexist_and_a_dropped_build_takes_its_tables_with_it` all pass; `test result: ok. 8 passed`.

Then run `nix develop --command cargo fmt --all`, `nix develop --command bash -c 'RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets --features engine-duckdb'` and `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit --features engine-duckdb'`; each finishes without a diagnostic. Finally run `nix develop --command cargo test -p kermit --lib` (no features): the library's tests pass and DuckDB is not compiled.

- [ ] **Step 5: Commit**

```bash
git add -N kermit/src/engine/duckdb.rs
git commit -m "feat(engine): DuckDB behind engine-duckdb" -- kermit/src/engine/duckdb.rs kermit/src/engine/mod.rs
```

### Task 7.3: The engine oracle, with DuckDB's cells

**Files:**
- Create: `kermit/tests/engine_oracle.rs`
- Test: `kermit/tests/engine_oracle.rs`

- [ ] **Step 1: Write the failing test**

The oracle runs the shared shapes (Story 2's `common::shapes`: the 16 standard patterns plus the constant, repeated-variable, placeholder and all-constant cases) over seeded random relations, under two planners, and compares DuckDB with the nested-loop oracle on contents as well as counts: on small fixtures a wrong join condition can still produce the right count. The oracle's rows are the full assignments with bag semantics (`full_query(query, &prepared)` puts every variable in the head, in the executed query's canonical order, so the rows compare as they come, with no permutation); the oracle therefore runs once per planner. DuckDB's `rows` and `stream` compare with them, and its `materialize` count compares with their distinct rows. An all-constant atom is not in DuckDB's query, so the tests repeat DuckDB's rows once per unit of `check_multiplicity`, as `EngineSubject` will.

Create `kermit/tests/engine_oracle.rs`:

```rust
//! The engines against the nested-loop oracle: the shared shapes over seeded random relations,
//! compared on contents as well as counts. Built only with an engine feature.
#![cfg(feature = "engine-duckdb")]

mod common;

use {
    common::{
        oracle::{oracle, Lcg},
        shapes::{catalog_for, full_query, random_data, shapes, SEEDS},
    },
    kermit::db::{prepare, Catalog, Prepared},
    kermit_algos::{CardinalityOptimiser, JoinQuery, LexicographicOptimiser, Planner},
    kermit_ds::{HashTrie, TrieLevels},
    kermit_iters::SipHashStrategy,
};

type HashTrieSip = HashTrie<SipHashStrategy>;

/// One shape over one seeded database under one planner: what an engine is checked against.
struct Case {
    shape: &'static str,
    label: String,
    catalog: Catalog,
    prepared: Prepared,
    /// The oracle's full assignments in the executed query's canonical order, sorted, with bag
    /// semantics.
    bag: Vec<Vec<usize>>,
}

/// The two planners the cases run under: the index specs' column orders, and so the SQL, differ.
fn planners() -> Vec<(&'static str, Planner)> {
    vec![
        (
            "lexicographic",
            Planner::new(Box::new(LexicographicOptimiser)),
        ),
        ("cardinality", Planner::new(Box::new(CardinalityOptimiser))),
    ]
}

/// Every shape, over every seed, under every planner.
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (index, shape) in shapes().iter().enumerate() {
        let query: JoinQuery = shape.query.parse().expect("a shape parses");
        for seed in SEEDS {
            let mut rng = Lcg(1000 * index as u64 + seed);
            let relations = random_data(&mut rng, shape.arities);
            for (planner_name, planner) in planners() {
                let label = format!("{} seed {seed} under {planner_name}", shape.name);
                let catalog = catalog_for(&relations);
                let prepared = prepare(&catalog, &query, &planner, TrieLevels::All)
                    .unwrap_or_else(|e| panic!("{label}: {e}"));
                // The full query's head is the executed query's canonical order, which the plan
                // can change for a body-only variable, so the oracle runs once per planner.
                let bag = oracle::<HashTrieSip>(&relations, &full_query(&query, &prepared));
                cases.push(Case {
                    shape: shape.name,
                    label,
                    catalog,
                    prepared,
                    bag,
                });
            }
        }
    }
    cases
}

/// How many distinct rows `rows` holds.
fn distinct_rows(rows: &[Vec<usize>]) -> usize {
    let mut distinct = rows.to_vec();
    distinct.sort();
    distinct.dedup();
    distinct.len()
}

/// The final bounds catch a generator that stops producing answers, which would turn every
/// comparison into `[] == []`, and fixtures with no duplicate derivation, which would make bag
/// and set counts the same.
#[test]
fn the_cases_have_answers_and_duplicate_derivations() {
    let cases = cases();
    let with_rows = cases.iter().filter(|case| !case.bag.is_empty()).count();
    assert!(
        with_rows * 4 >= cases.len(),
        "only {with_rows} of {} cases returned a row",
        cases.len()
    );
    assert!(
        cases
            .iter()
            .any(|case| distinct_rows(&case.bag) < case.bag.len()),
        "no case derives a row twice, so the bag and set counts never differ"
    );
}

mod duckdb_cells {
    use {
        super::{cases, distinct_rows, Case},
        kermit::{
            db::check_multiplicity,
            engine::{
                duckdb::{DuckDb, DuckDbBuilt, DuckDbSettings},
                Engine,
            },
        },
    };

    /// Loads and builds `case` on DuckDB and hands `check` the built state and the multiplicity of
    /// the case's all-constant atoms (DuckDB keeps duplicates, so each one multiplies).
    fn on_duckdb(
        case: &Case, settings: &DuckDbSettings, check: impl FnOnce(&DuckDbBuilt<'_>, usize),
    ) {
        let label = &case.label;
        let loaded = DuckDb::load(settings, &case.catalog, &case.prepared)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        let built = DuckDb::build(&loaded, &case.catalog, &case.prepared)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        let multiplicity =
            check_multiplicity(&case.catalog, &case.prepared, DuckDb::KEEPS_DUPLICATES);
        check(&built, multiplicity);
    }

    /// DuckDB's rows, each repeated once per unit of multiplicity (a check that holds nowhere
    /// leaves none) and sorted: the oracle's bag.
    fn rows_of(built: &DuckDbBuilt<'_>, multiplicity: usize, label: &str) -> Vec<Vec<usize>> {
        let mut rows = Vec::new();
        for row in DuckDb::rows(built).unwrap_or_else(|e| panic!("{label}: {e}")) {
            for _ in 0..multiplicity {
                rows.push(row.clone());
            }
        }
        rows.sort();
        rows
    }

    #[test]
    fn duckdb_returns_the_oracles_full_assignments() {
        for case in cases() {
            on_duckdb(&case, &DuckDbSettings::default(), |built, multiplicity| {
                assert_eq!(
                    rows_of(built, multiplicity, &case.label),
                    case.bag,
                    "{}",
                    case.label
                );
            });
        }
    }

    #[test]
    fn duckdb_streams_the_oracles_bag_count() {
        for case in cases() {
            on_duckdb(&case, &DuckDbSettings::default(), |built, multiplicity| {
                let streamed =
                    DuckDb::stream(built).unwrap_or_else(|e| panic!("{}: {e}", case.label));
                assert_eq!(
                    streamed * multiplicity as u64,
                    case.bag.len() as u64,
                    "{}",
                    case.label
                );
                let again = DuckDb::stream(built).unwrap_or_else(|e| panic!("{}: {e}", case.label));
                assert_eq!(
                    again, streamed,
                    "{}: the prepared statement changed its answer",
                    case.label
                );
            });
        }
    }

    #[test]
    fn duckdb_materializes_the_oracles_distinct_count() {
        for case in cases() {
            on_duckdb(&case, &DuckDbSettings::default(), |built, multiplicity| {
                // A check that holds nowhere means no rows, and the lifecycle never asks the
                // engine then.
                let materialized = if multiplicity == 0 {
                    0
                } else {
                    let (count, kept) = DuckDb::materialize(built)
                        .unwrap_or_else(|e| panic!("{}: {e}", case.label));
                    assert!(kept.is_some(), "{}: DuckDB returns its sorted batch", case.label);
                    count
                };
                assert_eq!(
                    materialized,
                    distinct_rows(&case.bag) as u64,
                    "{}",
                    case.label
                );
            });
        }
    }

    #[test]
    fn duckdb_agrees_with_the_oracle_on_two_threads() {
        let shapes = ["triangle", "chain", "selfjoin", "mixed-constants-and-repeats"];
        for case in cases().iter().filter(|case| shapes.contains(&case.shape)) {
            on_duckdb(case, &DuckDbSettings { threads: 2 }, |built, multiplicity| {
                assert_eq!(
                    rows_of(built, multiplicity, &case.label),
                    case.bag,
                    "{} on two threads",
                    case.label
                );
            });
        }
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --features engine-duckdb --test engine_oracle`

Expected: the oracle's engine already exists (Task 7.2), so the test build compiles and the five tests pass. A test that cannot fail proves nothing, so first apply a mutant. In `rows_of`, change `rows.sort();` to `rows.reverse();`, and check that the mutant applied: `grep -n 'rows.reverse' kermit/tests/engine_oracle.rs` prints that line. Run the command again: `duckdb_returns_the_oracles_full_assignments` now fails with a left/right row mismatch naming a case such as `unary seed 0 under lexicographic`, because the unary shape's answers are many and reversed rows differ from sorted ones. Change `rows.reverse();` back to `rows.sort();` before Step 3.

- [ ] **Step 3: Implement**

Nothing to add: the oracle exercises Task 7.2's engine. If a case fails with the mutant restored, the failure is a real disagreement between DuckDB's translation and the oracle, and the label names the shape, seed and planner.

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --features engine-duckdb --test engine_oracle`

Expected: `running 5 tests`; `the_cases_have_answers_and_duplicate_derivations`, `duckdb_cells::duckdb_returns_the_oracles_full_assignments`, `duckdb_cells::duckdb_streams_the_oracles_bag_count`, `duckdb_cells::duckdb_materializes_the_oracles_distinct_count` and `duckdb_cells::duckdb_agrees_with_the_oracle_on_two_threads` pass; `test result: ok. 5 passed`.

Run: `nix develop --command cargo test -p kermit --test engine_oracle`

Expected: `running 0 tests` (the file is empty without an engine feature), so a default `cargo test` is unchanged.

Run: `nix develop --command cargo fmt --all` and `nix develop --command bash -c 'RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets --features engine-duckdb'`; both finish without a diagnostic.

- [ ] **Step 5: Commit**

```bash
git add -N kermit/tests/engine_oracle.rs
git commit -m "test(engine): the engine oracle, with DuckDB's cells" -- kermit/tests/engine_oracle.rs
```

### Task 7.4: The `engines` CI job

**Files:**
- Modify: `.github/workflows/pr.yml` (append a job)

- [ ] **Step 1: Write the failing check**

The job does not exist yet.

- [ ] **Step 2: Run it to see it fail**

Run: `grep -c '^  engines:' .github/workflows/pr.yml`

Expected: `0`.

- [ ] **Step 3: Implement**

The workflow currently ends with the `python` job:

```yaml
      - run: uv run pytest
        working-directory: python/kermit-lab
        env:
          KERMIT_BIN: ${{ github.workspace }}/target/debug/kermit
```

Append after it

```yaml

  engines:
    name: Engines (DuckDB bundled)
    runs-on: ubuntu-latest
    # CI has no Nix, so `engine-duckdb-bundled` compiles DuckDB's amalgamation, one large C++
    # translation unit: expect a long cold build.
    timeout-minutes: 60
    env:
      RUSTFLAGS: -Dwarnings
      RUSTDOCFLAGS: -Dwarnings
      # The one place the job names its engines; the Nemo engine's feature joins this list.
      ENGINE_FEATURES: engine-duckdb-bundled
    steps:
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4
      - uses: dtolnay/rust-toolchain@efa25f7f19611383d5b0ccf2d1c8914531636bf9 # master
        with:
          toolchain: nightly
          components: clippy
      - uses: Swatinem/rust-cache@ad397744b0d591a723ab90405b7247fac0e6b8db # v2
      - run: cargo clippy -p kermit --all-targets --features "$ENGINE_FEATURES"
      - run: cargo doc -p kermit --no-deps --features "$ENGINE_FEATURES"
      - run: cargo test -p kermit --features "$ENGINE_FEATURES" --lib
      - run: cargo test -p kermit --features "$ENGINE_FEATURES" --test engine_oracle
```

The existing jobs build without the features and stay as they are.

- [ ] **Step 4: Run it to see it pass**

Run: `grep -c '^  engines:' .github/workflows/pr.yml`

Expected: `1`.

Run the job's four commands in the dev shell, with the system `libduckdb` standing in for the bundled one (the bundled amalgamation is never built on this host):

```bash
nix develop --command bash -c 'ENGINE_FEATURES=engine-duckdb && cargo clippy -p kermit --all-targets --features "$ENGINE_FEATURES" && RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit --no-deps --features "$ENGINE_FEATURES" && cargo test -p kermit --features "$ENGINE_FEATURES" --lib && cargo test -p kermit --features "$ENGINE_FEATURES" --test engine_oracle'
```

Expected: all four succeed. No local YAML parser is available, so GitHub parses the file on the first run; keep the indentation of the new job at two spaces for the key and four for its fields, as the other jobs have it.

- [ ] **Step 5: Commit**

```bash
git commit -m "ci: the engines job, DuckDB bundled" -- .github/workflows/pr.yml
```

## Story 8: Nemo.

- **Developer:** intermediate-dev
- **Files:** `kermit/src/engine/nemo.rs`, `kermit/src/engine/mod.rs`, `kermit/Cargo.toml`, `kermit/tests/engine_oracle.rs`, `.github/workflows/pr.yml`
- **Interfaces:** exposes `Nemo`, `NEMO_REV`, `NEMO_PHYSICAL_VERSION` and the feature `engine-nemo` (Names contract: the engines) to Stories 10 and 12; uses Stories 2, 5 and 6
- **Verify:** `bash -c 'cargo test -p kermit --features engine-duckdb,engine-nemo --lib && cargo test -p kermit --features engine-duckdb,engine-nemo --test engine_oracle && RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets --features engine-duckdb,engine-nemo'`
- **Review:** deep

`kermit::engine::nemo` is Nemo behind the `engine-nemo` feature, through `nemo-physical`'s public API at the pinned revision. `load` keeps `Arc` handles on the catalog's tuples (Story 5's `Catalog::shared_tuples`). `build` makes a fresh `DatabaseInstance`, registers one table per index spec with a `ViewProvider` that writes the spec's filtered, projected rows into Nemo's loader as integer `AnyDataValue`s, loads each trie with `load_trie_into_memory` under `futures::executor::block_on` (the one copy, plus Nemo's sort and dedup), and builds the join plan in kermit's variable order: a marker is a variable's position in the plan, so Nemo, which orders a join's variables by the order its markers are pushed, runs kermit's plan. `materialize` runs `execute_plan` and returns `count_rows_in_memory` of the stored result; `space` sums `table_size_bytes` over the spec tables, read after loading; `rows` permutes the plan-ordered result columns into canonical order; `stream` is refused (`STREAMS` is false). The code holds none of the profile's landmine markers, but `NemoBuilt` keeps `RefCell` and `Cell` state because Nemo stores its result when a plan runs (`MATERIALIZE_MUTATES = true`, so a second `materialize` on one build panics): that is the shape of the interior-mutability landmine note, so the supervisor should dispatch this story's developer with `effort: xhigh`. Nemo joins the engine oracle, which Story 7 created gated on `engine-duckdb`: the file's gate widens to either engine, and Nemo's cells compare rows and the `materialize` count with the nested-loop oracle's distinct full assignments, on Story 7's shared cases (the lexicographic and the cardinality planner) and on every shape under every variable order. Nemo also joins the `engines` CI job, through its `ENGINE_FEATURES` variable. Unit tests pin `NEMO_REV` against `kermit/Cargo.toml` and check that the join's markers follow kermit's plan order. `Cargo.lock` is untracked (`.gitignore:8`), so no task here edits or commits it: the first build with `engine-nemo` changes the local copy, and no commit carries it.

### Task 8.1: The feature, the pinned revision and the plan's markers

**Files:**
- Modify: `kermit/Cargo.toml` (the `[features]` table and the `[dependencies]` table)
- Modify: `kermit/src/engine/mod.rs` (the module list)
- Create: `kermit/src/engine/nemo.rs`
- Test: `kermit/src/engine/nemo.rs` (its inline `tests` module)

This task adds everything in the module that needs no Nemo type: the two constants, the markers of the join (`plan_markers`) and the permutation back to canonical order (`to_canonical`). Task 8.2 adds the `Engine` impl on top. Until then nothing calls them, so the file carries a temporary `allow(dead_code)` that Task 8.2 deletes.

The first build with `--features engine-nemo` compiles `nemo-physical` and its dependencies, which took 5 min 57 s at two jobs in the premise check. Run each command below in the foreground with a ten-minute timeout, start nothing else beside it, and prefix the very first one with `env CARGO_BUILD_JOBS=2` as shown.

- [ ] **Step 1: Write the failing tests**

In `kermit/Cargo.toml` (Story 7 added the DuckDB features and dependency), the features currently end with

```toml
engine-duckdb-bundled = ["engine-duckdb", "duckdb/bundled"]

[dependencies]
```

Replace those three lines with

```toml
engine-duckdb-bundled = ["engine-duckdb", "duckdb/bundled"]
# Nemo, in-process (`kermit::engine::nemo`). Off by default: a default build compiles no engine.
engine-nemo = ["dep:nemo-physical", "dep:futures", "dep:async-trait"]

[dependencies]
```

In the same file, the last dependency currently reads

```toml
duckdb = { version = "~1.4", default-features = false, optional = true }
```

Replace that line with

```toml
duckdb = { version = "~1.4", default-features = false, optional = true }
# `engine-nemo`: `nemo-physical` is pinned to one revision of knowsys/nemo, which
# `engine::nemo::NEMO_REV` repeats and a unit test checks. `futures` drives its async loader and
# executor with `block_on`; `async-trait` implements its `TableProvider`.
nemo-physical = { git = "https://github.com/knowsys/nemo", rev = "04f0962b91edc1bbe73e6f65f08d41d1ee491980", optional = true }
futures = { version = "0.3", optional = true }
async-trait = { version = "0.1", optional = true }
```

The pin test finds the dependency by the line that starts with `nemo-physical`, so keep it on one line.

In `kermit/src/engine/mod.rs`, the module declarations currently read (Stories 6 and 7)

```rust
#[cfg(feature = "engine-duckdb")]
pub mod duckdb;
pub mod sql;
```

Replace those three lines with

```rust
#[cfg(feature = "engine-duckdb")]
pub mod duckdb;
#[cfg(feature = "engine-nemo")]
pub mod nemo;
pub mod sql;
```

Create `kermit/src/engine/nemo.rs`. It holds the module documentation, the imports, the constants, `failure`, and the tests. `plan_markers` and `to_canonical`, which the tests call, do not exist yet.

```rust
//! Nemo, in-process: the [`Engine`](super::Engine) behind the `engine-nemo` feature.
//!
//! [Nemo](https://github.com/knowsys/nemo) answers Datalog with leapfrog triejoins over
//! column-oriented tries, the family `ColumnTrie` belongs to. This module drives its
//! `nemo-physical` crate through the public API only, at the pinned revision [`NEMO_REV`]. How
//! kermit's phases map onto it:
//!
//! - **`load`** (untimed): shared handles on the tuples of each relation the query reads
//!   (`Catalog::shared_tuples`), so Nemo's loader streams from the catalog's one copy.
//! - **`build`** (timed): a fresh `DatabaseInstance`. For each index spec a `ViewProvider`
//!   streams the spec's view into Nemo's loader as integer `AnyDataValue`s, and
//!   `load_trie_into_memory` sorts, deduplicates and builds its trie (the one copy). The join is
//!   then planned, in kermit's variable order.
//! - **`materialize`** (timed, opt-in): `execute_plan` stores the result as a deduplicated trie
//!   and `count_rows_in_memory` counts it. Nemo stores its result, so one build can be
//!   materialised once (`MATERIALIZE_MUTATES`).
//! - **`space`**: Nemo's own `table_size_bytes` over the spec tables, read after loading.
//! - **No `stream`**: upstream's streaming join (`evaluate_operation` with `RowScan`) is
//!   private, and the public `execute_plan` stores its result, so `iteration` and `end_to_end`
//!   are not offered.
//!
//! Nemo has set semantics (its loader and its result trie deduplicate), so `KEEPS_DUPLICATES` is
//! false. It answers the full conjunctive query, whose distinct rows are the same under set and
//! bag semantics. It runs on one thread and reports no thread count.

// Task 8.2's `Engine` impl uses everything below; it deletes this allow.
#![allow(dead_code)]

use {
    super::EngineError,
    crate::db::Prepared,
    kermit_algos::INDEX_ID_PREFIX,
    kermit_parser::Term,
    std::fmt,
};

/// The revision of `nemo-physical` that `kermit/Cargo.toml` pins. A unit test compares the two,
/// so a move of the pin changes both or fails.
pub const NEMO_REV: &str = "04f0962b91edc1bbe73e6f65f08d41d1ee491980";

/// The workspace version of `nemo-physical` at [`NEMO_REV`]. Moving the pin means reading the
/// new revision's version here.
pub const NEMO_PHYSICAL_VERSION: &str = "0.10.2-dev";

/// The engine's name: the report's `system` axis, and the `engine` of its errors.
const ENGINE: &str = "nemo";

/// An error of this engine.
fn failure(message: impl fmt::Display) -> EngineError {
    EngineError {
        engine: ENGINE,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::db::{prepare, Catalog},
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{RelationHeader, TrieLevels, Tuples},
    };

    /// A catalog of positional relations, each `(name, arity, rows)`.
    fn catalog(relations: Vec<(&str, usize, Vec<Vec<usize>>)>) -> Catalog {
        let relations = relations
            .into_iter()
            .map(|(name, arity, rows)| {
                let mut tuples = Tuples::new(arity);
                for row in &rows {
                    tuples.push(row);
                }
                (RelationHeader::new_positional(name, arity), tuples)
            })
            .collect();
        Catalog::new(relations, StatisticsLevel::TupleCounts)
    }

    /// `text` prepared over `catalog` in the lexicographic plan, under the levels engines use.
    fn prepared_for(catalog: &Catalog, text: &str) -> Prepared {
        let query: JoinQuery = text.parse().expect("a well-formed query");
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        prepare(catalog, &query, &planner, TrieLevels::All).expect("a valid query")
    }

    /// `NEMO_REV` is a copy of the `rev` in `kermit/Cargo.toml`. The precedent is the Python
    /// `SCHEMA_VERSION` pin: the two move together, and this fails when only one does.
    #[test]
    fn the_pinned_revision_is_the_manifests() {
        let manifest = include_str!("../../Cargo.toml");
        let dependency = manifest
            .lines()
            .find(|line| line.trim_start().starts_with("nemo-physical"))
            .expect("kermit/Cargo.toml declares the nemo-physical dependency");
        assert!(
            dependency.contains(&format!("rev = \"{NEMO_REV}\"")),
            "NEMO_REV ({NEMO_REV}) is not the rev of `{dependency}`; move both together"
        );
    }

    /// R(X, P), S(X, Y), T(Y): X and Y are join keys and P is payload, so the lexicographic plan
    /// binds X, Y, P while the query's canonical order is X, P, Y. A marker is a plan position,
    /// so R's markers are X's 0 and P's 2, where canonical numbering would give 0 and 1.
    #[test]
    fn markers_follow_the_plans_variable_order() {
        let c = catalog(vec![
            ("R", 2, vec![vec![1, 7]]),
            ("S", 2, vec![vec![1, 5]]),
            ("T", 1, vec![vec![5]]),
        ]);
        let prepared = prepared_for(&c, "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).");
        assert_eq!(prepared.variable_order(), vec!["X", "Y", "P"]);
        let markers = plan_markers(&prepared).expect("every atom lists its variables in plan order");
        assert_eq!(markers.result, vec![0, 1, 2]);
        assert_eq!(markers.atoms, vec![
            (0, vec![0, 2]),
            (1, vec![0, 1]),
            (2, vec![1])
        ]);
    }

    /// The three atoms of a triangle over one relation read one merged index: Nemo fetches the
    /// one table three times, with different markers.
    #[test]
    fn atoms_over_one_merged_index_share_its_table() {
        let c = catalog(vec![("E", 2, vec![vec![1, 2]])]);
        let prepared = prepared_for(&c, "Q(X, Y, Z) :- E(X, Y), E(Y, Z), E(X, Z).");
        assert_eq!(prepared.indexes().len(), 1);
        let markers = plan_markers(&prepared).expect("every atom lists its variables in plan order");
        assert_eq!(markers.result, vec![0, 1, 2]);
        assert_eq!(markers.atoms, vec![
            (0, vec![0, 1]),
            (0, vec![1, 2]),
            (0, vec![0, 2])
        ]);
    }

    /// The result's column `p` holds the variable the plan binds `p`-th, whose canonical index is
    /// `ordering[p]`.
    #[test]
    fn to_canonical_puts_each_plan_column_at_its_variable() {
        // The plan binds canonical variables 0, 2, 1: plan column 1 holds variable 2.
        assert_eq!(to_canonical(&[10, 30, 20], &[0, 2, 1]), vec![10, 20, 30]);
        // The plan binds 2, 0, 1: the plan row [c, a, b] is the canonical row [a, b, c].
        assert_eq!(to_canonical(&[3, 1, 2], &[2, 0, 1]), vec![1, 2, 3]);
        assert_eq!(to_canonical(&[7], &[0]), vec![7]);
        assert_eq!(to_canonical(&[], &[]), Vec::<usize>::new());
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --features engine-nemo --lib engine::nemo`

Cargo first compiles `nemo-physical` and its dependencies (minutes), then fails to compile the `kermit` lib test target:

```
error[E0425]: cannot find function `plan_markers` in this scope
error[E0425]: cannot find function `to_canonical` in this scope
error: could not compile `kermit` (lib test) due to N previous errors
```

(`N` counts the calls in the tests. Unused-import warnings for `Prepared`, `INDEX_ID_PREFIX` and `Term` come with it.)

- [ ] **Step 3: Implement**

In `kermit/src/engine/nemo.rs`, insert this between `failure` and the line `#[cfg(test)]`:

```rust
/// The markers of one join, as the numbers Nemo's `OperationColumnMarker` wraps.
///
/// A marker is a variable's position in kermit's plan, so the result's markers are `0, 1, …` in
/// the order the plan binds the variables. Nemo orders a join's variables by the order its
/// markers are pushed, so it runs kermit's plan. An atom lists its variables' markers in
/// increasing order, as Nemo's join requires of every input (it panics otherwise): `plan_indexes`
/// gives each atom its variables in plan order, and `plan_markers` checks it.
#[derive(Debug, PartialEq, Eq)]
struct PlanMarkers {
    /// The result table's markers, one per variable of the executed query.
    result: Vec<usize>,
    /// One entry per atom of the executed query, in body order: the index (`#k`) it reads, whose
    /// table the atom fetches, and its variables' markers.
    atoms: Vec<(usize, Vec<usize>)>,
}

/// The markers of `prepared`'s join.
///
/// # Errors
///
/// An atom of the executed query that does not name one of its indexes, holds anything but a
/// variable, or lists its variables out of plan order: `prepare` guarantees none of these, so
/// each is a broken invariant reported rather than passed on to Nemo.
fn plan_markers(prepared: &Prepared) -> Result<PlanMarkers, EngineError> {
    let order = prepared.variable_order();
    let mut atoms = Vec::with_capacity(prepared.query().body.len());
    for atom in &prepared.query().body {
        let index = atom
            .name
            .strip_prefix(INDEX_ID_PREFIX)
            .and_then(|digits| digits.parse::<usize>().ok())
            .filter(|&index| index < prepared.indexes().len())
            .ok_or_else(|| failure(format!("atom {atom} does not name one of the query's indexes")))?;
        let mut markers = Vec::with_capacity(atom.terms.len());
        for term in &atom.terms {
            let Term::Var(name) = term else {
                return Err(failure(format!("atom {atom} holds {term}, which is not a variable")));
            };
            let position = order
                .iter()
                .position(|bound| bound == name)
                .ok_or_else(|| failure(format!("variable {name} of atom {atom} is not in the plan")))?;
            markers.push(position);
        }
        let columns = prepared.indexes()[index].columns.len();
        if markers.len() != columns || !markers.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(failure(format!(
                "atom {atom} does not list its {columns} variables in plan order"
            )));
        }
        atoms.push((index, markers));
    }
    Ok(PlanMarkers {
        result: (0..order.len()).collect(),
        atoms,
    })
}

/// `plan_row`, whose column `p` holds the variable the plan binds `p`-th, as the canonical row:
/// column `v` holds canonical variable `v`, in the order of `canonical_names(prepared.query())`
/// (the executed query's order, head variables first). `ordering` is the plan's `variable_ordering`
/// (canonical indices in descent order), a permutation of `0..plan_row.len()`.
fn to_canonical(plan_row: &[usize], ordering: &[usize]) -> Vec<usize> {
    debug_assert_eq!(plan_row.len(), ordering.len());
    let mut row = vec![0; plan_row.len()];
    for (position, &value) in plan_row.iter().enumerate() {
        row[ordering[position]] = value;
    }
    row
}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --features engine-nemo --lib engine::nemo`

Expected:

```
running 4 tests
test engine::nemo::tests::atoms_over_one_merged_index_share_its_table ... ok
test engine::nemo::tests::markers_follow_the_plans_variable_order ... ok
test engine::nemo::tests::the_pinned_revision_is_the_manifests ... ok
test engine::nemo::tests::to_canonical_puts_each_plan_column_at_its_variable ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; ... filtered out
```

Then `nix develop --command bash -c 'RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets --features engine-nemo'` prints no warning (the `allow(dead_code)` covers the helpers nothing calls yet), and `nix develop --command cargo test -p kermit --lib` still passes without the feature, where `nemo.rs` is not compiled.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git add -N kermit/src/engine/nemo.rs
git commit -m "feat(engine): the engine-nemo feature, its pinned revision and the plan's markers" -- kermit/Cargo.toml kermit/src/engine/mod.rs kermit/src/engine/nemo.rs
```

### Task 8.2: Nemo as an `Engine`

**Files:**
- Modify: `kermit/src/engine/nemo.rs` (the imports, the types, the `Engine` impl, the provider, the tests)
- Test: `kermit/src/engine/nemo.rs` (its inline `tests` module)

- [ ] **Step 1: Write the failing tests**

In the `tests` module of `kermit/src/engine/nemo.rs`, append these tests after `to_canonical_puts_each_plan_column_at_its_variable` (the last test of Task 8.1), before the module's closing `}`. They call `Nemo`, `NemoBuilt` and the `Engine` methods, none of which exist yet; the `catalog` and `prepared_for` helpers are Task 8.1's.

```rust
    /// Builds `text` over `relations` on Nemo and returns the build, before anything has run on
    /// it.
    fn built_for(relations: Vec<(&str, usize, Vec<Vec<usize>>)>, text: &str) -> NemoBuilt {
        let c = catalog(relations);
        let prepared = prepared_for(&c, text);
        Nemo::supports(&prepared).expect("Nemo runs the query");
        let loaded = Nemo::load(&(), &c, &prepared).expect("the relations load");
        Nemo::build(&loaded, &c, &prepared).expect("the build succeeds")
    }

    /// The `materialize` count and the sorted `rows` of `text` over `relations` on Nemo.
    fn answer(
        relations: Vec<(&str, usize, Vec<Vec<usize>>)>, text: &str,
    ) -> (u64, Vec<Vec<usize>>) {
        let built = built_for(relations, text);
        let (count, kept) = Nemo::materialize(&built).expect("the plan runs");
        assert!(kept.is_none(), "Nemo's result stays in its database");
        let mut rows = Nemo::rows(&built).expect("the result reads back");
        rows.sort();
        (count, rows)
    }

    /// The edges of the transitive tournament on `1..=n`: `i -> j` for `i < j`.
    fn tournament(n: usize) -> Vec<Vec<usize>> {
        (1..=n)
            .flat_map(|i| (i + 1..=n).map(move |j| vec![i, j]))
            .collect()
    }

    #[test]
    fn its_labels_and_version_are_fixed() {
        assert_eq!(Nemo::NAME, "nemo");
        assert_eq!(Nemo::label(&()), "nemo");
        assert_eq!(Nemo::threads(&()), None);
        let loaded = NemoLoaded {
            relations: BTreeMap::new(),
        };
        assert_eq!(Nemo::version(&loaded), "nemo-physical 0.10.2-dev @ 04f0962b91ed");
    }

    /// The plan binds X, Y, P and Nemo's result columns follow it; `rows` returns the canonical
    /// X, P, Y. The values differ, so a wrong permutation cannot pass. R(2, 9) and S(3, 5) find
    /// no partner.
    #[test]
    fn a_join_returns_full_assignments_in_canonical_order() {
        let (count, rows) = answer(
            vec![
                ("R", 2, vec![vec![1, 7], vec![2, 9]]),
                ("S", 2, vec![vec![1, 5], vec![3, 5]]),
                ("T", 1, vec![vec![5]]),
            ],
            "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).",
        );
        assert_eq!((count, rows), (1, vec![vec![1, 7, 5]]));
    }

    /// Three atoms over one relation: one table, fetched three times. The triangles of the
    /// transitive tournament on 1..=4 are its four increasing triples.
    #[test]
    fn a_self_join_fetches_one_table_for_each_atom() {
        let (count, rows) = answer(
            vec![("E", 2, tournament(4))],
            "Q(X, Y, Z) :- E(X, Y), E(Y, Z), E(X, Z).",
        );
        assert_eq!(count, 4);
        assert_eq!(rows, vec![
            vec![1, 2, 3],
            vec![1, 2, 4],
            vec![1, 3, 4],
            vec![2, 3, 4]
        ]);
    }

    /// `execute_plan` registers no table for an empty result, so `materialize` and `rows` must
    /// read that as no rows.
    #[test]
    fn a_join_with_no_answer_has_no_rows() {
        let (count, rows) = answer(
            vec![
                ("R", 2, vec![vec![1, 7]]),
                ("S", 2, vec![vec![2, 5]]),
                ("T", 1, vec![vec![5]]),
            ],
            "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).",
        );
        assert_eq!((count, rows), (0, Vec::new()));
    }

    /// The constant admits no row of `r`, so the spec's table is empty.
    #[test]
    fn a_view_that_admits_no_row_has_no_rows() {
        let (count, rows) = answer(
            vec![("r", 2, vec![vec![1, 2], vec![3, 4]])],
            "Q(X) :- r(X, c9).",
        );
        assert_eq!((count, rows), (0, Vec::new()));
    }

    #[test]
    fn an_empty_relation_has_no_rows() {
        let (count, rows) = answer(vec![("r", 1, vec![])], "Q(X) :- r(X).");
        assert_eq!((count, rows), (0, Vec::new()));
    }

    /// The provider writes the rows the spec's filter admits, in the spec's columns.
    #[test]
    fn the_provider_applies_the_views_constants_and_equalities() {
        let r = || vec![("r", 2, vec![vec![1, 2], vec![3, 4], vec![5, 2], vec![6, 6]])];
        assert_eq!(answer(r(), "Q(X) :- r(X, c2).").1, vec![vec![1], vec![5]]);
        assert_eq!(answer(r(), "Q(X) :- r(X, X).").1, vec![vec![6]]);
    }

    /// Nemo's loader deduplicates: a bag engine would count the repeated row twice.
    #[test]
    fn duplicate_rows_count_once() {
        let (count, rows) = answer(
            vec![("r", 2, vec![vec![1, 2], vec![1, 2], vec![3, 4]])],
            "Q(X, Y) :- r(X, Y).",
        );
        assert_eq!((count, rows), (2, vec![vec![1, 2], vec![3, 4]]));
    }

    /// `space` is Nemo's figure for the loaded spec tables, and grows with them.
    #[test]
    fn space_counts_the_loaded_tables() {
        let space_of = |n: usize| {
            let built = built_for(vec![("E", 2, tournament(n))], "Q(X, Y) :- E(X, Y).");
            Nemo::space(&built).expect("Nemo reports space")
        };
        let (small, large) = (space_of(3), space_of(11));
        assert!(small > 0, "the loaded trie has a size");
        assert!(large > small, "3 rows take {small} bytes, 55 rows {large}");
    }

    /// Nemo stores its result, so a second `materialize` on one build would time nothing: it
    /// panics, and the runner builds afresh for every sample.
    #[test]
    #[should_panic(expected = "runs once per build")]
    fn a_second_materialize_on_one_build_panics() {
        let built = built_for(vec![("r", 1, vec![vec![1]])], "Q(X) :- r(X).");
        let (count, kept) = Nemo::materialize(&built).expect("the plan runs");
        assert_eq!(count, 1);
        assert!(kept.is_none());
        let _ = Nemo::materialize(&built);
    }

    /// `rows` runs the plan itself when `materialize` has not.
    #[test]
    fn rows_run_the_plan_when_materialize_has_not() {
        let built = built_for(
            vec![("E", 2, tournament(4))],
            "Q(X, Y, Z) :- E(X, Y), E(Y, Z), E(X, Z).",
        );
        let mut rows = Nemo::rows(&built).expect("the plan runs");
        rows.sort();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], vec![1, 2, 3]);
    }

    #[test]
    fn stream_is_refused() {
        let built = built_for(vec![("r", 1, vec![vec![1]])], "Q(X) :- r(X).");
        let error = Nemo::stream(&built).expect_err("Nemo has no streamed join");
        assert_eq!(error.engine, "nemo");
        assert!(error.message.contains("no streamed join"), "{error}");
    }

    /// A query of constant checks has no index, so no join: the runner never builds it, and an
    /// attempt to is an error rather than a Nemo panic. The parser cannot write an empty head,
    /// so the query is built by clearing one.
    #[test]
    fn a_query_of_checks_is_refused_at_build() {
        let c = catalog(vec![("s", 2, vec![vec![7, 8]])]);
        let mut query: JoinQuery = "Q(X) :- s(c7, c8).".parse().expect("a well-formed query");
        query.head.terms.clear();
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared = prepare(&c, &query, &planner, TrieLevels::All).expect("a valid query");
        assert!(prepared.indexes().is_empty());
        let loaded = Nemo::load(&(), &c, &prepared).expect("nothing to load");
        let Err(error) = Nemo::build(&loaded, &c, &prepared) else {
            panic!("a query of checks has no join for Nemo to run");
        };
        assert_eq!(error.engine, "nemo");
        assert!(error.message.contains("constant checks"), "{error}");
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --features engine-nemo --lib engine::nemo`

The tests do not compile:

```
error[E0412]: cannot find type `NemoBuilt` in this scope
error[E0433]: failed to resolve: use of undeclared type `Nemo`
error[E0433]: failed to resolve: use of undeclared type `NemoLoaded`
error: could not compile `kermit` (lib test) due to N previous errors
```

(`N` counts the mentions of the missing names.)

- [ ] **Step 3: Implement**

All of this goes in `kermit/src/engine/nemo.rs`.

First, delete the two lines Task 8.1 marked as temporary (the comment and the attribute under it):

```rust
// Task 8.2's `Engine` impl uses everything below; it deletes this allow.
#![allow(dead_code)]
```

Then replace the file's `use { … };` block (the one under the module documentation) with:

```rust
use {
    super::{Engine, EngineError, Unsupported},
    crate::db::{Catalog, Prepared},
    kermit_algos::{IndexSpec, INDEX_ID_PREFIX},
    kermit_ds::{Filter, TupleView, Tuples},
    kermit_parser::Term,
    nemo_physical::{
        datasources::{table_providers::TableProvider, tuple_writer::TupleWriter},
        datavalues::{AnyDataValue, DataValue},
        error::ReadingError,
        management::{
            bytesized::ByteSized,
            database::{
                id::{ExecutionId, PermanentTableId},
                DatabaseInstance,
            },
            execution_plan::{ColumnOrder, ExecutionPlan},
        },
        tabular::operations::{OperationColumnMarker, OperationTable},
    },
    std::{
        cell::{Cell, RefCell},
        collections::{btree_map::Entry, BTreeMap},
        fmt,
        sync::Arc,
    },
};
```

Insert the types and the `Engine` impl between `failure` and the doc comment of `PlanMarkers` (Task 8.1 put them next to each other):

```rust
/// Nemo as an [`Engine`]. It has no settings: it runs on one thread.
pub struct Nemo;

/// What `load` leaves: shared handles on the tuples of each relation the query's indexes read.
/// Nothing is copied.
pub struct NemoLoaded {
    relations: BTreeMap<String, Arc<Tuples>>,
}

/// What one timed `build` leaves: a database whose spec tables are loaded, and the join plan
/// that has not run.
///
/// Running the plan changes the database (Nemo stores its result), so a build can be
/// materialised once, and `Nemo::MATERIALIZE_MUTATES` is true. The database and the plan sit
/// behind `RefCell`s and the outcome in a `Cell` because `Engine::materialize` and
/// `Engine::rows` take `&Self::Built`.
pub struct NemoBuilt {
    database: RefCell<DatabaseInstance>,
    /// The join plan, until it runs.
    plan: RefCell<Option<ExecutionPlan>>,
    /// The plan's output node, whose table `execute_plan` names in its result.
    output: ExecutionId,
    /// What running the plan produced.
    outcome: Cell<Outcome>,
    /// One table per index spec, in index order.
    tables: Vec<PermanentTableId>,
    /// The plan's `variable_ordering`, which `rows` permutes the result's columns by.
    ordering: Vec<usize>,
}

/// Whether the plan has run, and where its result is.
#[derive(Clone, Copy)]
enum Outcome {
    /// The plan has not run.
    NotRun,
    /// The plan ran and the join is empty: `execute_plan` registers no table for it.
    Empty,
    /// The plan ran and stored its result here.
    Table(PermanentTableId),
}

impl Engine for Nemo {
    type Built<'l>
        = NemoBuilt
    where
        Self: 'l;
    type Loaded = NemoLoaded;
    type Settings = ();

    const HAS_SPACE: bool = true;
    const KEEPS_DUPLICATES: bool = false;
    const MATERIALIZE_MUTATES: bool = true;
    const NAME: &'static str = ENGINE;
    const STREAMS: bool = false;

    fn label(_settings: &()) -> String { ENGINE.to_string() }

    fn threads(_settings: &()) -> Option<usize> { None }

    fn version(_loaded: &NemoLoaded) -> String {
        format!("nemo-physical {NEMO_PHYSICAL_VERSION} @ {}", &NEMO_REV[..12])
    }

    /// Nemo joins every query `prepare` produces: each index is one table, an atom fetches it
    /// with its variables as markers, and a constant or a repeated variable was already
    /// compiled into a spec's filter.
    fn supports(_prepared: &Prepared) -> Result<(), Unsupported> { Ok(()) }

    fn load(
        _settings: &(), catalog: &Catalog, prepared: &Prepared,
    ) -> Result<NemoLoaded, EngineError> {
        let mut relations = BTreeMap::new();
        for spec in prepared.indexes() {
            if let Entry::Vacant(slot) = relations.entry(spec.relation.clone()) {
                let tuples = catalog.shared_tuples(&spec.relation).ok_or_else(|| {
                    failure(format!("the catalog holds no relation {}", spec.relation))
                })?;
                slot.insert(tuples);
            }
        }
        Ok(NemoLoaded {
            relations,
        })
    }

    /// The timed build: a fresh database, one table per spec loaded through a `ViewProvider`,
    /// and the join plan.
    fn build<'l>(
        loaded: &'l NemoLoaded, _catalog: &Catalog, prepared: &Prepared,
    ) -> Result<NemoBuilt, EngineError>
    where
        Self: 'l,
    {
        if prepared.indexes().is_empty() {
            return Err(failure("a query of constant checks has no join to run"));
        }
        let markers = plan_markers(prepared)?;
        let mut database = DatabaseInstance::default();
        let mut tables = Vec::with_capacity(prepared.indexes().len());
        for (index, spec) in prepared.indexes().iter().enumerate() {
            let tuples = loaded
                .relations
                .get(&spec.relation)
                .ok_or_else(|| failure(format!("relation {} was not loaded", spec.relation)))?;
            let table = database.register_table(&format!("idx_{index}"), spec.columns.len());
            let provider = ViewProvider::new(Arc::clone(tuples), spec);
            database.add_source_external(table, ColumnOrder::default(), Box::new(provider));
            tables.push(table);
        }
        // Loading is the one copy of the rows, plus Nemo's sort, deduplication and trie.
        for &table in &tables {
            futures::executor::block_on(
                database.load_trie_into_memory(table, ColumnOrder::default()),
            )
            .map_err(failure)?;
        }
        let (plan, output) = join_plan(&markers, &tables);
        Ok(NemoBuilt {
            database: RefCell::new(database),
            plan: RefCell::new(Some(plan)),
            output,
            outcome: Cell::new(Outcome::NotRun),
            tables,
            ordering: prepared.plan().variable_ordering.clone(),
        })
    }

    /// Nemo's streaming join is private upstream, so there is none to offer.
    fn stream(_built: &NemoBuilt) -> Result<u64, EngineError> {
        Err(failure("Nemo has no streamed join (STREAMS is false); use materialize"))
    }

    /// Runs the plan: Nemo stores the join as a deduplicated trie, and the count is its rows.
    /// The batch is `None`: the result lives in Nemo's database and drops with the build, so
    /// there is nothing for the caller to free after the timer.
    ///
    /// # Panics
    ///
    /// If the plan has already run on this build.
    fn materialize(built: &NemoBuilt) -> Result<(u64, Option<Tuples>), EngineError> {
        assert!(
            matches!(built.outcome.get(), Outcome::NotRun),
            "Nemo stores its result, so `materialize` runs once per build, on a fresh `build`"
        );
        let count = match built.result()? {
            | Some(table) => built.database.borrow().count_rows_in_memory(table) as u64,
            | None => 0,
        };
        Ok((count, None))
    }

    /// The sum of Nemo's `table_size_bytes` over the spec tables. Read it before the plan runs:
    /// the figure is Nemo's own accounting of what loading left, and running a plan may add
    /// orders of a table.
    fn space(built: &NemoBuilt) -> Option<usize> {
        let database = built.database.borrow();
        let bytes: u64 = built
            .tables
            .iter()
            .map(|&table| database.table_size_bytes(table))
            .sum();
        Some(usize::try_from(bytes).unwrap_or(usize::MAX))
    }

    /// The result's rows in canonical order, running the plan first if it has not run.
    fn rows(built: &NemoBuilt) -> Result<Vec<Vec<usize>>, EngineError> {
        let Some(table) = built.result()? else {
            return Ok(Vec::new());
        };
        let database = built.database.borrow();
        let stored = database.table_row_iterator_inmemory(table).map_err(failure)?;
        stored
            .map(|values| -> Result<Vec<usize>, EngineError> {
                let plan_row = values
                    .iter()
                    .map(integer)
                    .collect::<Result<Vec<usize>, EngineError>>()?;
                Ok(to_canonical(&plan_row, &built.ordering))
            })
            .collect()
    }
}

impl NemoBuilt {
    /// Where the plan's result is stored, running the plan if it has not run: `None` for an
    /// empty join.
    fn result(&self) -> Result<Option<PermanentTableId>, EngineError> {
        match self.outcome.get() {
            | Outcome::Table(table) => return Ok(Some(table)),
            | Outcome::Empty => return Ok(None),
            | Outcome::NotRun => {},
        }
        let plan = self
            .plan
            .borrow_mut()
            .take()
            .expect("the plan is present until it has run once");
        let results =
            futures::executor::block_on(self.database.borrow_mut().execute_plan(plan))
                .map_err(failure)?;
        let outcome = match results.get(&self.output) {
            | Some(&table) => Outcome::Table(table),
            | None => Outcome::Empty,
        };
        self.outcome.set(outcome);
        Ok(match outcome {
            | Outcome::Table(table) => Some(table),
            | Outcome::Empty | Outcome::NotRun => None,
        })
    }
}
```

Insert the join plan, the id conversion, and the provider after `to_canonical` (and before the line `#[cfg(test)]`):

```rust
/// The join over the spec tables: the result's markers in kermit's plan order, and one fetch of
/// the atom's table per atom (merged specs are one table fetched by several atoms). Returns the
/// plan and its output node.
fn join_plan(markers: &PlanMarkers, tables: &[PermanentTableId]) -> (ExecutionPlan, ExecutionId) {
    let table_of = |numbers: &[usize]| {
        let mut table = OperationTable::default();
        for &number in numbers {
            table.push(OperationColumnMarker(number));
        }
        table
    };
    let mut plan = ExecutionPlan::default();
    let mut join = plan.join_empty(table_of(markers.result.as_slice()));
    for (index, atom) in &markers.atoms {
        let fetch = plan.fetch_table(table_of(atom.as_slice()), tables[*index]);
        join.add_subnode(fetch);
    }
    let output = plan.write_permanent(join, "join", "result");
    (plan, output)
}

/// A stored value as kermit's `usize` id. `ViewProvider` writes every value as an unsigned
/// integer, so anything else in a result is a broken invariant.
fn integer(value: &AnyDataValue) -> Result<usize, EngineError> {
    value
        .to_u64()
        .and_then(|id| usize::try_from(id).ok())
        .ok_or_else(|| failure(format!("result value {value:?} is not an id")))
}

/// One index spec's view of a relation, written into Nemo's loader as unsigned integers.
///
/// Nemo's `Box<dyn TableProvider>` is `'static`, so the provider owns an `Arc` handle on the
/// relation's tuples and rebuilds the `TupleView` when Nemo asks for the data. The rows are read
/// once, by the loader, which sorts and deduplicates them into a trie.
struct ViewProvider {
    tuples: Arc<Tuples>,
    filter: Filter,
    columns: Vec<usize>,
}

impl ViewProvider {
    fn new(tuples: Arc<Tuples>, spec: &IndexSpec) -> Self {
        Self {
            tuples,
            filter: spec.filter.clone(),
            columns: spec.columns.clone(),
        }
    }
}

impl fmt::Debug for ViewProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewProvider")
            .field("source_rows", &self.tuples.len())
            .field("filter", &self.filter)
            .field("columns", &self.columns)
            .finish()
    }
}

impl fmt::Display for ViewProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a view of {} rows through {} over columns {:?}",
            self.tuples.len(),
            self.filter,
            self.columns
        )
    }
}

impl ByteSized for ViewProvider {
    /// An upper bound that costs no scan: every source row, in the view's columns, as `u64`s.
    /// Nemo reads it only before it loads the table.
    fn size_bytes(&self) -> u64 {
        (self.tuples.len() * self.columns.len() * std::mem::size_of::<u64>()) as u64
    }
}

#[async_trait::async_trait(?Send)]
impl TableProvider for ViewProvider {
    async fn provide_table_data(
        self: Box<Self>, tuple_writer: &mut TupleWriter,
    ) -> Result<(), ReadingError> {
        let view = TupleView::new(&self.tuples, self.filter.clone(), self.columns.clone());
        view.for_each(|row| {
            for &value in row {
                tuple_writer.add_tuple_value(AnyDataValue::new_integer_from_u64(value as u64));
            }
        });
        Ok(())
    }

    fn output_arity(&self) -> usize { self.columns.len() }

    fn input_arity(&self) -> usize { self.columns.len() }
}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --features engine-nemo --lib engine::nemo`

Expected: `running 17 tests`, every one `ok` (Task 8.1's four and these thirteen), and

```
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; ... filtered out
```

Then run `nix develop --command bash -c 'RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets --features engine-nemo'`, which prints no warning (the `'l` of `build` is the trait's, with its `where Self: 'l`, and stays), and `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit --features engine-nemo'`, which prints no warning.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git commit -m "feat(engine): Nemo as an Engine (provider, load, build, materialize, space, rows)" -- kermit/src/engine/nemo.rs
```

### Task 8.3: Nemo joins the engine oracle

**Files:**
- Modify: `kermit/tests/engine_oracle.rs`
- Test: `kermit/tests/engine_oracle.rs`

`engine_oracle.rs` is Story 7's file: it is gated on `engine-duckdb` alone, declares `mod common;`, holds the shared `Case`, `cases()` and `distinct_rows` at its top level, and keeps DuckDB's cells in `mod duckdb_cells`. This task widens the gate, gives the DuckDB module its own gate, and appends Nemo's cells: the shared cases (the shapes under the lexicographic and the cardinality planner) compared on rows and on the `materialize` count, and every shape under every variable order, which checks `rows`' permutation of the result's plan-ordered columns against every plan.

- [ ] **Step 1: Write the failing test**

In `kermit/tests/engine_oracle.rs`, replace the file's gate

```rust
#![cfg(feature = "engine-duckdb")]
```

with

```rust
#![cfg(any(feature = "engine-duckdb", feature = "engine-nemo"))]
```

Replace the head of `Case`

```rust
/// One shape over one seeded database under one planner: what an engine is checked against.
struct Case {
```

with (its `shape` field is read only by DuckDB's two-thread test, so a build with Nemo alone would warn that it is dead)

```rust
/// One shape over one seeded database under one planner: what an engine is checked against.
#[cfg_attr(not(feature = "engine-duckdb"), allow(dead_code))]
struct Case {
```

Replace the head of DuckDB's module

```rust
mod duckdb_cells {
```

with

```rust
#[cfg(feature = "engine-duckdb")]
mod duckdb_cells {
```

Append this at the end of the file:

```rust
/// Nemo against the nested-loop oracle (`engine-nemo`).
///
/// Nemo answers the full conjunctive query as a set, so its rows and its `materialize` count are
/// compared with the oracle's *distinct* full assignments: the same set under set and bag
/// semantics, while the fixtures hold duplicate rows. The shared cases run the planners
/// `bench run` has; the every-order test forces each variable order, so `rows`' permutation of
/// the result's plan-ordered columns into canonical order is checked against every plan, and a
/// mis-ordered join would show in the contents, which a count alone could miss on these small
/// fixtures. Canonical order is `canonical_names(prepared.query())` of the `TrieLevels::All`
/// query Nemo ran, so the oracle's query is `full_query(query, &prepared)` for that same
/// `prepared`: the order can change with the plan, and so the oracle runs once per plan.
#[cfg(feature = "engine-nemo")]
mod nemo_cells {
    use {
        super::{cases, distinct_rows, Case, HashTrieSip},
        crate::common::{
            oracle::{oracle, Lcg, Relations},
            orders::{every_order, FixedOrder},
            shapes::{catalog_for, full_query, random_data, shapes, SEEDS},
        },
        kermit::{
            db::{check_multiplicity, prepare, Catalog},
            engine::{
                nemo::{Nemo, NemoBuilt},
                Engine,
            },
        },
        kermit_algos::{canonical_names, rewrite_placeholders, JoinQuery, Planner},
        kermit_ds::TrieLevels,
    };

    /// Loads and builds `case` on Nemo and hands `check` the built state and the multiplicity of
    /// the case's all-constant atoms (Nemo has set semantics, so each one clamps to one).
    fn on_nemo(case: &Case, check: impl FnOnce(&NemoBuilt, usize)) {
        let label = &case.label;
        Nemo::supports(&case.prepared).unwrap_or_else(|e| panic!("{label}: {e}"));
        let loaded = Nemo::load(&(), &case.catalog, &case.prepared)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        let built = Nemo::build(&loaded, &case.catalog, &case.prepared)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        let multiplicity =
            check_multiplicity(&case.catalog, &case.prepared, Nemo::KEEPS_DUPLICATES);
        check(&built, multiplicity);
    }

    #[test]
    fn nemo_returns_the_oracles_distinct_full_assignments() {
        for case in cases() {
            on_nemo(&case, |built, multiplicity| {
                // A check that holds nowhere means no rows, and the lifecycle never asks the
                // engine then. Otherwise Nemo's set is the answer, whatever the check's count.
                // `case.bag` is the oracle's for `full_query(query, &case.prepared)`, so its
                // rows are in the same canonical order as `rows`.
                let mut rows = if multiplicity == 0 {
                    Vec::new()
                } else {
                    Nemo::rows(built).unwrap_or_else(|e| panic!("{}: {e}", case.label))
                };
                rows.sort();
                // The oracle's bag is sorted, so its distinct rows are its deduplication.
                let mut want = case.bag.clone();
                want.dedup();
                assert_eq!(rows, want, "{}", case.label);
            });
        }
    }

    #[test]
    fn nemo_materializes_the_oracles_distinct_count() {
        for case in cases() {
            on_nemo(&case, |built, multiplicity| {
                let materialized = if multiplicity == 0 {
                    0
                } else {
                    let (count, kept) = Nemo::materialize(built)
                        .unwrap_or_else(|e| panic!("{}: {e}", case.label));
                    assert!(kept.is_none(), "{}: Nemo keeps no batch", case.label);
                    count
                };
                assert_eq!(
                    materialized,
                    distinct_rows(&case.bag) as u64,
                    "{}",
                    case.label
                );
            });
        }
    }

    /// Nemo's `materialize` count and sorted `rows` for `query` over `catalog`, with the plan
    /// binding `order` first, and the oracle's distinct rows for `full_query(query, &prepared)`
    /// over `relations`, where `prepared` is the `TrieLevels::All` query Nemo ran. Asserts the
    /// plan binds `order` first, so a planner that stopped honouring `order` fails here instead
    /// of running one order many times. The all-constant atoms are applied as the runner applies
    /// them, through `check_multiplicity`.
    fn nemo_in_order(
        relations: &Relations, catalog: &Catalog, query: &JoinQuery, order: &[String],
        label: &str,
    ) -> (u64, Vec<Vec<usize>>, Vec<Vec<usize>>) {
        let planner = Planner::new(Box::new(FixedOrder(order.to_vec())));
        let prepared = prepare(catalog, query, &planner, TrieLevels::All)
            .unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
        let names = canonical_names(prepared.query());
        let bound: Vec<String> = prepared
            .plan()
            .variable_ordering
            .iter()
            .map(|&variable| names[variable].clone())
            .collect();
        assert_eq!(
            &bound[..order.len()],
            order,
            "{label}: the plan does not bind the forced variables first"
        );
        assert!(
            !prepared.indexes().is_empty(),
            "{label}: every shape reads at least one index"
        );
        let mut want = oracle::<HashTrieSip>(relations, &full_query(query, &prepared));
        want.dedup();
        Nemo::supports(&prepared)
            .unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
        if check_multiplicity(catalog, &prepared, Nemo::KEEPS_DUPLICATES) == 0 {
            return (0, Vec::new(), want);
        }
        let loaded = Nemo::load(&(), catalog, &prepared)
            .unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
        let built = Nemo::build(&loaded, catalog, &prepared)
            .unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
        let (count, kept) = Nemo::materialize(&built)
            .unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
        assert!(kept.is_none(), "{label} in order {order:?}: Nemo keeps no batch");
        let mut rows = Nemo::rows(&built)
            .unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
        rows.sort();
        (count, rows, want)
    }

    /// Every shape over `SEEDS` random relations, under every order of its variables. The final
    /// bound catches a generator that stops producing queries with answers, which would turn
    /// every comparison into `[] == []`.
    #[test]
    fn nemo_returns_the_distinct_full_assignments_in_every_order() {
        let (mut with_rows, mut total) = (0, 0);
        for (index, shape) in shapes().iter().enumerate() {
            for seed in SEEDS {
                let mut rng = Lcg(1000 * index as u64 + seed);
                let relations = random_data(&mut rng, shape.arities);
                let label = format!("{} seed {seed}", shape.name);
                let query: JoinQuery = shape
                    .query
                    .parse()
                    .unwrap_or_else(|e| panic!("{label}: unparsable: {e}"));
                let catalog = catalog_for(&relations);
                let variables = canonical_names(&rewrite_placeholders(query.clone()));
                let mut answered = false;
                for order in every_order(&variables) {
                    let (count, rows, want) =
                        nemo_in_order(&relations, &catalog, &query, &order, &label);
                    assert_eq!(rows, want, "{label} in order {order:?}: rows");
                    assert_eq!(
                        count,
                        want.len() as u64,
                        "{label} in order {order:?}: materialize"
                    );
                    answered = !want.is_empty();
                }
                total += 1;
                with_rows += usize::from(answered);
            }
        }
        assert!(
            with_rows * 4 >= total,
            "only {with_rows} of {total} queries had an answer"
        );
    }
}
```

- [ ] **Step 2: Run it to see it fail**

The oracle needs a failing run to show it can fail. Break `to_canonical` on purpose, in a working tree where `kermit/src/engine/nemo.rs` has no other uncommitted change (Task 8.2 committed it), and check that the mutant applied:

```bash
sed -i 's/row\[ordering\[position\]\] = value;/row[position] = value;/' kermit/src/engine/nemo.rs
grep -n 'row\[position\] = value;' kermit/src/engine/nemo.rs
```

The `grep` must print one line. Then:

Run: `nix develop --command cargo test -p kermit --features engine-duckdb,engine-nemo --test engine_oracle nemo_cells`

Expected: `nemo_returns_the_oracles_distinct_full_assignments` and `nemo_returns_the_distinct_full_assignments_in_every_order` fail, on the first case whose plan differs from the canonical order, with a left/right row mismatch naming it (the shape, seed and order or planner vary):

```
test nemo_cells::nemo_returns_the_distinct_full_assignments_in_every_order ... FAILED
assertion `left == right` failed: triangle seed 0 in order ["V0", "V2", "V1"]: rows
```

`nemo_materializes_the_oracles_distinct_count` still passes: the count does not depend on the column order.

- [ ] **Step 3: Implement**

There is nothing to implement: the engine is Task 8.2's. Undo the mutant, which restores the committed file exactly, and check it:

```bash
git checkout -- kermit/src/engine/nemo.rs
grep -c 'row\[ordering\[position\]\] = value;' kermit/src/engine/nemo.rs
```

The `grep -c` prints `1`.

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --features engine-duckdb,engine-nemo --test engine_oracle`

Expected: Story 7's cases test and DuckDB's cells and Nemo's three tests are all `ok`, and `test result: ok`.

Then run `nix develop --command cargo test -p kermit --features engine-nemo --test engine_oracle`, which checks the widened gate and the `Case` attribute: with Nemo alone the file compiles without a warning, and the shared cases test and Nemo's three tests are `ok`.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git commit -m "test(engine): Nemo joins the engine oracle, on the shared cases and under every variable order" -- kermit/tests/engine_oracle.rs
```

### Task 8.4: Nemo joins the `engines` CI job

**Files:**
- Modify: `.github/workflows/pr.yml` (the `engines` job Story 7 added)

CI has no Nix, so the job builds with `engine-duckdb-bundled`. Story 7 made `ENGINE_FEATURES` the one place the job names its engines, and every command reads it: adding `engine-nemo` there puts Nemo under the job's clippy, rustdoc, unit tests and oracle. The existing jobs build no feature and are unchanged.

- [ ] **Step 1: Write the failing check**

The job is right when `ENGINE_FEATURES` lists both engines:

```bash
grep -c 'ENGINE_FEATURES: engine-duckdb-bundled,engine-nemo' .github/workflows/pr.yml
```

- [ ] **Step 2: Run it to see it fail**

Run the command from Step 1. Expected output: `0`, since the job lists `engine-duckdb-bundled` alone.

- [ ] **Step 3: Implement**

In `.github/workflows/pr.yml`, the `engines` job's `env` currently has the line

```yaml
      ENGINE_FEATURES: engine-duckdb-bundled
```

Replace that line (leave the comment above it as Story 7 wrote it) with

```yaml
      ENGINE_FEATURES: engine-duckdb-bundled,engine-nemo
```

and the job's name

```yaml
    name: Engines (DuckDB bundled)
```

with

```yaml
    name: Engines (DuckDB bundled, Nemo)
```

Nemo adds `nemo-physical` and its dependencies to a cold build (about six minutes at two jobs on the dev host); the job's `timeout-minutes` is Story 7's, so check it against the job's first run.

- [ ] **Step 4: Run it to see it pass**

Run the command from Step 1 again: it prints `1`. Then run the job's four commands in the dev shell, with the system `libduckdb` standing in for the bundled one (the bundled amalgamation is never built on this host) and Nemo added, which is the story's Verify plus the rustdoc run:

```bash
nix develop --command bash -c 'ENGINE_FEATURES=engine-duckdb,engine-nemo && cargo clippy -p kermit --all-targets --features "$ENGINE_FEATURES" && RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit --no-deps --features "$ENGINE_FEATURES" && cargo test -p kermit --features "$ENGINE_FEATURES" --lib && cargo test -p kermit --features "$ENGINE_FEATURES" --test engine_oracle'
nix develop --command bash -c 'RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets --features engine-duckdb,engine-nemo'
nix develop --command cargo fmt --all -- --check
```

All pass. The default build is untouched: `nix develop --command cargo test -p kermit --lib` passes without the features, and does not compile either engine. No local YAML parser is available, so GitHub parses the file on its first run; keep the indentation as it is.

- [ ] **Step 5: Commit**

```bash
git commit -m "ci: Nemo joins the engines job" -- .github/workflows/pr.yml
```

## Story 9: Report schema v7 and the `system` axis

- **Developer:** intermediate-dev
- **Files:** `kermit/src/bench_report.rs`, `kermit/src/bench/cell.rs`, `kermit/src/bench/ds.rs`, `python/kermit-lab/kermit_lab/__init__.py`, `kermit/tests/cli_bench_probe.rs`, `kermit/tests/cli_join_tests.rs`, `python/kermit-lab/tests/test_contract.py`
- **Interfaces:** exposes report v7 and the `system` axis (Names contract: Report v7) to Stories 10–12; uses Story 4's `Cell`
- **Verify:** `bash -c 'cargo test -p kermit && cd python/kermit-lab && uv run --locked --group test pytest -q'`
- **Review:** deep

Every report now says which system measured it. `FamilyCell::axes` and `bench ds` set
`system: "kermit"` (one shared constant, `KERMIT_SYSTEM`), and `REPORT_SCHEMA_VERSION` and
kermit-lab's `SCHEMA_VERSION` move to 7 together. The bump is not for a metric: none changes
meaning, so v6 and v7 reports load together. It is because Story 10 adds engine rows that omit
`data_structure`, `algorithm` and every `ds_*` axis, and an older kermit-lab would misread them
(one of its back-fills keys on `algorithm` being present). No engine row exists yet. The
`Files` line above cannot hold: the tests that pin `schema_version` as `6` live in
`kermit/tests/cli_bench_probe.rs`, `kermit/tests/cli_join_tests.rs` and
`python/kermit-lab/tests/test_contract.py`, and the new `system` assertions go beside them, so
Task 9.1 and Task 9.2 edit those three files as well.

### Task 9.1: the `system` axis

**Files:**
- Modify: `kermit/src/bench/cell.rs`
- Modify: `kermit/src/bench/ds.rs`
- Test: `kermit/src/bench/cell.rs`, `kermit/tests/cli_join_tests.rs`, `kermit/tests/cli_bench_probe.rs`

- [ ] **Step 1: Write the failing tests**

Append this test to the end of `mod tests` in `kermit/src/bench/cell.rs` (before its closing
brace). It uses the imports that module already has:

```rust
    #[test]
    fn every_kermit_cell_sets_the_kermit_system_axis() {
        let sorted = TrieLftj::<TreeTrie>::new(Default::default());
        assert_eq!(FamilyCell(&sorted).axes()["system"], "kermit");
        let hash = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion, kermit_ds::AllLevels>::new(
            HashTrieConfig::default(),
            HashTrieBuildMode::default(),
        );
        assert_eq!(FamilyCell(&hash).axes()["system"], "kermit");
    }
```

In `kermit/tests/cli_join_tests.rs`, `cli_bench_ds_writes_json_report` checks the `bench ds`
axes. Its current lines are:

```rust
    assert_eq!(axes["data_structure"], "TreeTrie");
    assert!(
        axes["relation_bytes"].is_number(),
```

Replace them with:

```rust
    assert_eq!(axes["data_structure"], "TreeTrie");
    assert_eq!(axes["system"], "kermit");
    assert!(
        axes["relation_bytes"].is_number(),
```

In `cli_bench_run_writes_json_report`, the current lines are:

```rust
    assert_eq!(axes["benchmark"], "triangle");
    assert_eq!(axes["data_structure"], "TreeTrie");
    assert_eq!(axes["algorithm"], "LeapfrogTriejoin");
```

Replace them with:

```rust
    assert_eq!(axes["benchmark"], "triangle");
    assert_eq!(axes["system"], "kermit");
    assert_eq!(axes["data_structure"], "TreeTrie");
    assert_eq!(axes["algorithm"], "LeapfrogTriejoin");
```

In `kermit/tests/cli_bench_probe.rs`, in `a_probe_verifies_each_query_and_times_one_build_and_one_run`,
the current line is:

```rust
        assert_eq!(report["kind"], "probe");
```

Replace it with:

```rust
        assert_eq!(report["kind"], "probe");
        assert_eq!(report["axes"]["system"], "kermit");
```

- [ ] **Step 2: Run them to see them fail**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::cell`

Expected: `every_kermit_cell_sets_the_kermit_system_axis` fails, panicking with
`no entry found for key` (the axes map has no `system` key yet).

Run: `nix develop --command cargo test -p kermit --test cli_join_tests --test cli_bench_probe`

Expected: `cli_bench_ds_writes_json_report`, `cli_bench_run_writes_json_report` and
`a_probe_verifies_each_query_and_times_one_build_and_one_run` fail with
`left: Null`, `right: "kermit"`.

- [ ] **Step 3: Implement**

In `kermit/src/bench/cell.rs`, add the constant above the trait. The current text is:

```rust
/// What the per-query loop of `run_benchmark` needs from one cell.
pub(crate) trait Cell {
```

Replace it with:

```rust
/// The `system` axis of every report that kermit's own structures produce:
/// `bench run`, `bench join`, `bench probe` through a [`FamilyCell`], and
/// `bench ds`. An external engine names itself instead (`duckdb`, `nemo`).
pub(crate) const KERMIT_SYSTEM: &str = "kermit";

/// What the per-query loop of `run_benchmark` needs from one cell.
pub(crate) trait Cell {
```

Update the trait's `axes` doc. The current text is:

```rust
    /// The report axes this cell sets: `data_structure`, `algorithm` and the
    /// `ds_*` optimization axes. The runner adds the axes that belong to the
    /// query.
```

Replace it with:

```rust
    /// The report axes this cell sets: `system`, and for a kermit cell
    /// `data_structure`, `algorithm` and the `ds_*` optimization axes. The
    /// runner adds the axes that belong to the query.
```

In `FamilyCell::axes`, the current text is:

```rust
        let mut axes = BTreeMap::from([
            (
                "data_structure".to_string(),
                serde_json::json!(execution.index_structure().axis_value()),
            ),
            (
                "algorithm".to_string(),
                serde_json::json!(execution.algorithm().axis_value()),
            ),
        ]);
```

Replace it with:

```rust
        let mut axes = BTreeMap::from([
            ("system".to_string(), serde_json::json!(KERMIT_SYSTEM)),
            (
                "data_structure".to_string(),
                serde_json::json!(execution.index_structure().axis_value()),
            ),
            (
                "algorithm".to_string(),
                serde_json::json!(execution.algorithm().axis_value()),
            ),
        ]);
```

In `kermit/src/bench/ds.rs`, import the constant. The file's import block begins:

```rust
use {
    super::{
```

Replace those two lines with:

```rust
use {
    super::{
        cell::KERMIT_SYSTEM,
```

(the `measure::{…}` and `Metric` entries that follow stay as they are, in that order). In
`run_ds_bench`, the current text is:

```rust
    let mut axes = BTreeMap::from([
        ("data_structure".to_string(), serde_json::json!(ds_name)),
        (
            "relation_path".to_string(),
```

Replace it with:

```rust
    let mut axes = BTreeMap::from([
        ("system".to_string(), serde_json::json!(KERMIT_SYSTEM)),
        ("data_structure".to_string(), serde_json::json!(ds_name)),
        (
            "relation_path".to_string(),
```

- [ ] **Step 4: Run them to see them pass**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::cell`

Expected: `every_kermit_cell_sets_the_kermit_system_axis ... ok` and the other `bench::cell` tests `ok`.

Run: `nix develop --command cargo test -p kermit --test cli_join_tests --test cli_bench_probe`

Expected: `test result: ok.` for both test binaries.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git commit -m "feat(bench): every kermit report sets the system axis to kermit" -- kermit/src/bench/cell.rs kermit/src/bench/ds.rs kermit/tests/cli_join_tests.rs kermit/tests/cli_bench_probe.rs
```

### Task 9.2: schema v7

**Files:**
- Modify: `kermit/src/bench_report.rs:107-127`, `kermit/src/bench_report.rs:457`
- Modify: `python/kermit-lab/kermit_lab/__init__.py:8`
- Test: `kermit/tests/cli_bench_probe.rs`, `kermit/tests/cli_join_tests.rs`, `python/kermit-lab/tests/test_contract.py`

- [ ] **Step 1: Write the failing tests**

In `kermit/src/bench_report.rs`, in the unit test `json_report_round_trips`, replace

```rust
        assert_eq!(json[0]["schema_version"], 6);
```

with

```rust
        assert_eq!(json[0]["schema_version"], 7);
```

In `kermit/tests/cli_bench_probe.rs`, in
`a_probe_verifies_each_query_and_times_one_build_and_one_run`, replace

```rust
        assert_eq!(report["schema_version"], 6);
```

with

```rust
        assert_eq!(report["schema_version"], 7);
```

In `kermit/tests/cli_join_tests.rs` there are three assertions. In `cli_bench_ds_writes_json_report`
replace

```rust
    assert_eq!(report["schema_version"], 6);
    assert_eq!(report["kind"], "ds");
```

with

```rust
    assert_eq!(report["schema_version"], 7);
    assert_eq!(report["kind"], "ds");
```

In `cli_bench_ds_writes_default_report_when_path_omitted` replace

```rust
    assert_eq!(json[0]["schema_version"], 6);
```

with

```rust
    assert_eq!(json[0]["schema_version"], 7);
```

and in `cli_bench_run_writes_json_report` replace

```rust
    assert_eq!(report["schema_version"], 6);
    assert_eq!(report["kind"], "run");
```

with

```rust
    assert_eq!(report["schema_version"], 7);
    assert_eq!(report["kind"], "run");
```

In `python/kermit-lab/tests/test_contract.py`, the three contract tests that read the real
binary's reports pin the version. In `test_bench_ds_reports_the_build_phase` replace

```python
    assert set(df["schema_version"]) == {6}
    assert set(df.loc[df["metric"] == "time", "phase"]) == {"build"}
    assert set(df.loc[df["metric"] == "time", "criterion_function"]) == {"TreeTrie/build"}
```

with

```python
    assert set(df["schema_version"]) == {7}
    assert set(df.loc[df["metric"] == "time", "phase"]) == {"build"}
    assert set(df.loc[df["metric"] == "time", "criterion_function"]) == {"TreeTrie/build"}
```

in `test_bench_join_reports_a_per_query_build_and_space` replace

```python
    assert set(raw["schema_version"]) == {6}
```

with

```python
    assert set(raw["schema_version"]) == {7}
```

and in `test_bench_probe_report_loads` replace

```python
    assert set(df["schema_version"]) == {6}
    assert set(df["estimator"]) == {"exact", "single"}
```

with

```python
    assert set(df["schema_version"]) == {7}
    assert set(df["estimator"]) == {"exact", "single"}
```

- [ ] **Step 2: Run them to see them fail**

Run: `nix develop --command cargo test -p kermit --test cli_join_tests --test cli_bench_probe`

Expected: `cli_bench_ds_writes_json_report`, `cli_bench_ds_writes_default_report_when_path_omitted`,
`cli_bench_run_writes_json_report` and `a_probe_verifies_each_query_and_times_one_build_and_one_run`
fail with `left: Number(6)`, `right: 7`.

- [ ] **Step 3: Implement**

Both constants change in this one step, because `python_schema_version_matches_rust` fails if
only one does.

In `kermit/src/bench_report.rs`, the current lines are:

```rust
/// (`kind: probe`) add single-run `build` / `iteration` times, and the `plan`
/// and `answer_rows` axes are new. No metric changes what it measures.
pub const REPORT_SCHEMA_VERSION: u32 = 6;
```

Replace them with:

```rust
/// (`kind: probe`) add single-run `build` / `iteration` times, and the `plan`
/// and `answer_rows` axes are new. No metric changes what it measures.
/// 7: every report carries the `system` axis (`kermit`, `duckdb`, `nemo`), and
/// the rows of an external engine, which omit `data_structure`, `algorithm`
/// and every `ds_*` axis, add `engine_version` and `engine_threads` and a
/// `materialize` function. No metric changes what it measures, so v6 and v7
/// reports load together; the bump is for a reader that expects every run
/// report to carry `data_structure` and `algorithm`.
pub const REPORT_SCHEMA_VERSION: u32 = 7;
```

In `python/kermit-lab/kermit_lab/__init__.py`, the current line is:

```python
SCHEMA_VERSION = 6
```

Replace it with:

```python
SCHEMA_VERSION = 7
```

- [ ] **Step 4: Run them to see them pass**

Run: `nix develop --command cargo test -p kermit`

Expected: `test result: ok.` for the library, the binary and every integration test, including
`python_schema_version_matches_rust`.

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q'`

Expected: the suite passes unchanged (v5 and v6 reports still load together; the contract tests
are skipped without `KERMIT_BIN`).

Run the contract tests against the real binary:

Run: `nix develop --command bash -c 'cargo build -p kermit && cd python/kermit-lab && KERMIT_BIN="$PWD/../../target/debug/kermit" uv run --locked --group test pytest -q tests/test_contract.py'`

Expected: every test in `test_contract.py` passes, with `schema_version` read as `{7}`.

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(bench): report schema v7, the system axis (kermit-lab reads v6 and v7)" -- kermit/src/bench_report.rs python/kermit-lab/kermit_lab/__init__.py kermit/tests/cli_bench_probe.rs kermit/tests/cli_join_tests.rs python/kermit-lab/tests/test_contract.py
```

## Story 10: Engine cells in the binary

- **Developer:** intermediate-dev
- **Files:** `kermit/src/bench/engine.rs`, `kermit/src/bench/mod.rs`, `kermit/src/bench/run.rs`, `kermit/src/options.rs`, `kermit/src/main.rs`, `kermit/tests/cli_engines.rs`
- **Interfaces:** exposes `EngineCell`, `EngineChoices`, `SweepCell` and the engine CLI (Names contract: engine cells) to Story 12; uses Stories 3, 4, 6, 7, 8 and 9
- **Verify:** `bash -c 'cargo test -p kermit --features engine-duckdb,engine-nemo --bin kermit && cargo test -p kermit --features engine-duckdb,engine-nemo --test cli_engines && cargo test -p kermit && RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets --features engine-duckdb,engine-nemo && cargo run -p kermit --features engine-duckdb,engine-nemo -- bench --sample-size 10 --measurement-time 1 --warm-up-time 1 run triangle -i all -a all --engine duckdb,nemo --metrics build iteration space materialize end-to-end --verify'`
- **Review:** deep

This story puts the engines of Stories 6 to 8 into the binary's runner. `bench/engine.rs` gives
`EngineSubject<E>` (the `Subject` of Story 3) and `EngineCell<E>` (the `Cell` of Story 4) for any
`Engine`, so DuckDB and Nemo share one body of code. `options.rs` gains `--engine` and
`--engine-threads` (`EngineSelector`, `EngineChoices`, `EngineChoice`). `run.rs` gains `SweepCell`,
which is a kermit `Execution` or an `EngineChoice`; the sweep, its Criterion group names and the
64-byte directory clash check run over `SweepCell`s. `main.rs` flattens the flags into `bench run`,
`bench probe` and `bench join`; on run and probe `-i` and `-a` become optional when `--engine`
is given. Engine cells prepare under `TrieLevels::All`, so they share the sorted cells' prepared
queries. An engine cell runs a fallible preflight per query (support, load, build, one count), calls
`kermit::db::check_multiplicity` inside its timed `build`, and never calls the engine for a query
made only of checks. The dispatch prints the skip notes once per engine cell and benchmark and does
not filter; `run_benchmark` (Story 4) filters the requested metrics through `Cell::supports`. The story ends by
running spec Acceptance 2 on `triangle`. None of the code holds a landmine marker. Engine features
are off in the profile's gates, so every engine-only item is either compiled under its feature or
under `cfg_attr(…, allow(dead_code))`, and the gates stay warning-free.

### Task 10.1: `EngineSubject` and `EngineCell`

**Files:**
- Create: `kermit/src/bench/engine.rs`
- Modify: `kermit/src/bench/mod.rs` (add `mod engine;` after the line `pub mod ds;`)
- Test: `kermit/src/bench/engine.rs`

- [ ] **Step 1: Write the failing test**

Register the module. In `kermit/src/bench/mod.rs`, add these lines directly after the line
`pub mod ds;` (Story 4 has added `mod cell;` in the same block; leave it):

```rust
// Engine cells exist only when an engine feature compiles one in; the gates build none.
#[cfg_attr(
    not(any(feature = "engine-duckdb", feature = "engine-nemo")),
    allow(dead_code)
)]
mod engine;
```

Create `kermit/src/bench/engine.rs` with the test support and the tests. Step 3 adds the
implementation above them.

```rust
// ---- the file's first lines are Step 3's; the test support starts here ----

/// A scripted [`Engine`] for the unit tests: no database, fixed answers. `S` is whether it streams,
/// `P` whether it reports space and `K` whether it keeps duplicates, so `Fake<true, false, true>`
/// is shaped like DuckDB and `Fake<false, true, false>` like Nemo.
#[cfg(test)]
pub(crate) mod test_support {
    use kermit::{
        db::{Catalog, Prepared},
        engine::{Engine, EngineError, Unsupported},
    };
    use kermit_ds::Tuples;

    thread_local! {
        static COUNTED: std::cell::RefCell<Vec<&'static str>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    fn counted(call: &'static str) { COUNTED.with(|counted| counted.borrow_mut().push(call)); }

    /// The counting calls (`stream`, `materialize`) the fakes of this test's thread have made since
    /// the last call, in order.
    pub(crate) fn take_counted() -> Vec<&'static str> {
        COUNTED.with(|counted| std::mem::take(&mut *counted.borrow_mut()))
    }

    /// `stream` counts [`Fake::STREAM_ROWS`], `materialize` counts [`Fake::MATERIALIZE_ROWS`] and,
    /// when `S` (DuckDB-shaped), returns an empty sorted batch beside the count, as DuckDB does.
    pub(crate) struct Fake<const S: bool, const P: bool, const K: bool>;

    impl<const S: bool, const P: bool, const K: bool> Fake<S, P, K> {
        pub(crate) const STREAM_ROWS: u64 = 5;
        pub(crate) const MATERIALIZE_ROWS: u64 = 3;
        pub(crate) const SPACE_BYTES: usize = 100;
    }

    impl<const S: bool, const P: bool, const K: bool> Engine for Fake<S, P, K> {
        const NAME: &'static str = "fake";
        const STREAMS: bool = S;
        const KEEPS_DUPLICATES: bool = K;
        const MATERIALIZE_MUTATES: bool = !S;
        const HAS_SPACE: bool = P;

        type Settings = ();
        type Loaded = ();
        type Built<'l>
            = u64
        where
            Self: 'l;

        fn label(_: &()) -> String { "fake".to_string() }

        fn threads(_: &()) -> Option<usize> { None }

        fn version(_: &()) -> String { "fake 1.0".to_string() }

        /// Refuses a query that reads a relation named `banned`.
        fn supports(prepared: &Prepared) -> Result<(), Unsupported> {
            if prepared.relations().contains(&"banned") {
                return Err(Unsupported {
                    reason: "relation banned".to_string(),
                });
            }
            Ok(())
        }

        fn load(_: &(), _: &Catalog, _: &Prepared) -> Result<(), EngineError> { Ok(()) }

        fn build<'l>(_: &'l (), _: &Catalog, _: &Prepared) -> Result<u64, EngineError>
        where
            Self: 'l,
        {
            Ok(Self::STREAM_ROWS)
        }

        fn stream(built: &u64) -> Result<u64, EngineError> {
            counted("stream");
            Ok(*built)
        }

        fn materialize(_: &u64) -> Result<(u64, Option<Tuples>), EngineError> {
            counted("materialize");
            Ok((Self::MATERIALIZE_ROWS, S.then(|| Tuples::new(0))))
        }

        fn space(_: &u64) -> Option<usize> { P.then_some(Self::SPACE_BYTES) }

        fn rows(_: &u64) -> Result<Vec<Vec<usize>>, EngineError> { Ok(Vec::new()) }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{
            test_support::{take_counted, Fake},
            *,
        },
        kermit::db::prepare,
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{RelationHeader, Tuples},
    };

    /// DuckDB-shaped: streams, no space, bag semantics.
    type Bag = Fake<true, false, true>;
    /// Streams, no space, set semantics.
    type Set = Fake<true, false, false>;
    /// Nemo-shaped: stores its result, so it neither streams nor keeps duplicates, and has space.
    type Storing = Fake<false, true, false>;

    fn catalog(relations: &[(&str, usize, &[&[usize]])]) -> Catalog {
        let relations = relations
            .iter()
            .map(|&(name, arity, rows)| {
                let mut tuples = Tuples::new(arity);
                for row in rows {
                    tuples.push(row);
                }
                (RelationHeader::new_positional(name, arity), tuples)
            })
            .collect();
        Catalog::new(relations, StatisticsLevel::TupleCounts)
    }

    fn planner() -> Planner { Planner::new(Box::new(LexicographicOptimiser)) }

    fn prepared(catalog: &Catalog, text: &str) -> Prepared {
        let query: JoinQuery = text.parse().expect("a well-formed query");
        prepare(catalog, &query, &planner(), TrieLevels::All).expect("a valid query")
    }

    /// The parser cannot write an empty head, so the query is built by clearing one: its atoms
    /// are all constants, hence all checks.
    fn checks_only(catalog: &Catalog, text: &str) -> Prepared {
        let mut query: JoinQuery = text.parse().expect("a well-formed query");
        query.head.terms.clear();
        prepare(catalog, &query, &planner(), TrieLevels::All).expect("a query of checks")
    }

    fn cell<E: Engine>() -> EngineCell<E> {
        EngineCell {
            settings: E::Settings::default(),
        }
    }

    /// `r` has two rows, `s` holds the row `(7, 8)` twice.
    fn duplicated_checks() -> Catalog {
        catalog(&[
            ("r", 1, &[&[1], &[2]]),
            ("s", 2, &[&[7, 8], &[7, 8]]),
            ("banned", 1, &[&[1]]),
        ])
    }

    #[test]
    fn an_all_constant_atoms_count_multiplies_a_bag_engines_stream() {
        let catalog = duplicated_checks();
        let prepared = prepared(&catalog, "Q(X) :- r(X), s(c7, c8).");
        let cell = cell::<Bag>();
        let subject = cell.subject(&catalog, &prepared).unwrap();
        let built = subject.build();
        assert_eq!(subject.exercise(&built), Bag::STREAM_ROWS * 2);
        // `materialize` counts distinct rows, so the multiplicity does not reach it.
        let materialized = subject.materialize(&built).unwrap();
        assert_eq!(materialized.rows, Bag::MATERIALIZE_ROWS);
        // The engine's sorted batch comes back as `kept`, for Criterion to free after the timer.
        assert!(materialized.kept.is_some());
    }

    #[test]
    fn a_set_engine_clamps_the_multiplicity_to_one() {
        let catalog = duplicated_checks();
        let prepared = prepared(&catalog, "Q(X) :- r(X), s(c7, c8).");
        let cell = cell::<Set>();
        let subject = cell.subject(&catalog, &prepared).unwrap();
        let built = subject.build();
        assert_eq!(subject.exercise(&built), Set::STREAM_ROWS);
    }

    #[test]
    fn a_check_that_counts_no_row_means_no_answers() {
        let catalog = catalog(&[("r", 1, &[&[1], &[2]]), ("s", 2, &[&[9, 9]])]);
        let prepared = prepared(&catalog, "Q(X) :- r(X), s(c7, c8).");
        let cell = cell::<Bag>();
        let subject = cell.subject(&catalog, &prepared).unwrap();
        let built = subject.build();
        assert_eq!(subject.exercise(&built), 0);
        assert_eq!(subject.materialize(&built).unwrap().rows, 0);
    }

    #[test]
    fn a_query_of_checks_never_calls_the_engine() {
        let catalog = duplicated_checks();
        let prepared = checks_only(&catalog, "Q(X) :- s(c7, c8).");
        assert!(prepared.indexes().is_empty());
        let cell = cell::<Bag>();
        // `Fake::supports` refuses any query that reads `banned`, checks included, so this
        // preflight passes only because a query of checks never reaches the engine.
        let banned = checks_only(&catalog, "Q(X) :- banned(c1).");
        assert_eq!(banned.relations(), vec!["banned"]);
        cell.preflight(&catalog, &banned, true).unwrap();
        cell.preflight(&catalog, &prepared, true).unwrap();
        let subject = cell.subject(&catalog, &prepared).unwrap();
        let built = subject.build();
        // The empty row, once per duplicate of the check's row.
        assert_eq!(subject.exercise(&built), 2);
        assert_eq!(subject.materialize(&built).unwrap().rows, 1);
        assert!(subject.heap_bytes(&built).is_empty());
        assert!(cell.subject_axes(&subject).is_empty());
    }

    #[test]
    fn the_subject_reports_the_engines_version_and_space() {
        let catalog = duplicated_checks();
        let prepared = prepared(&catalog, "Q(X) :- r(X).");

        let storing = cell::<Storing>();
        let subject = storing.subject(&catalog, &prepared).unwrap();
        let built = subject.build();
        assert_eq!(subject.heap_bytes(&built), vec![Storing::SPACE_BYTES]);
        let axes = storing.subject_axes(&subject);
        assert_eq!(axes["engine_version"], serde_json::json!("fake 1.0"));
        assert_eq!(axes.len(), 1);
        // Nemo-shaped: the count alone, with no batch to keep.
        assert!(subject.materialize(&built).unwrap().kept.is_none());

        let bag = cell::<Bag>();
        let subject = bag.subject(&catalog, &prepared).unwrap();
        let built = subject.build();
        assert!(subject.heap_bytes(&built).is_empty());
    }

    #[test]
    #[should_panic(expected = "fake does not stream")]
    fn exercising_a_storing_engine_is_a_broken_invariant() {
        let catalog = duplicated_checks();
        let prepared = prepared(&catalog, "Q(X) :- r(X).");
        let cell = cell::<Storing>();
        let subject = cell.subject(&catalog, &prepared).unwrap();
        let built = subject.build();
        let _ = subject.exercise(&built);
    }

    #[test]
    fn the_subjects_flags_follow_the_engines() {
        type StoringSubject<'a> = EngineSubject<'a, Storing>;
        type BagSubject<'a> = EngineSubject<'a, Bag>;
        assert!(!<StoringSubject<'_> as Subject>::STREAMS);
        assert!(<StoringSubject<'_> as Subject>::MATERIALIZE_MUTATES);
        assert!(<BagSubject<'_> as Subject>::STREAMS);
        assert!(!<BagSubject<'_> as Subject>::MATERIALIZE_MUTATES);
        assert!(!<BagSubject<'_> as Subject>::EXERCISE_MUTATES);
    }

    #[test]
    fn preflight_refuses_a_query_the_engine_does_not_support() {
        let catalog = duplicated_checks();
        let supported = prepared(&catalog, "Q(X) :- r(X).");
        let banned = prepared(&catalog, "Q(X) :- banned(X).");
        let cell = cell::<Bag>();
        cell.preflight(&catalog, &supported, true).unwrap();
        let error = cell.preflight(&catalog, &banned, true).unwrap_err().to_string();
        assert!(error.contains("fake cannot run query"), "{error}");
        assert!(error.contains("banned(X)"), "{error}");
        assert!(error.contains("relation banned"), "{error}");
    }

    #[test]
    fn preflight_runs_the_one_count_only_when_asked() {
        let catalog = duplicated_checks();
        let prepared = prepared(&catalog, "Q(X) :- r(X).");

        // A non-streaming engine counts through `materialize`: once when asked, and not at all
        // when the memory guard skips `materialize` (`count` false), which still loads and builds.
        let storing = cell::<Storing>();
        let _ = take_counted();
        storing.preflight(&catalog, &prepared, false).unwrap();
        assert!(take_counted().is_empty());
        storing.preflight(&catalog, &prepared, true).unwrap();
        assert_eq!(take_counted(), vec!["materialize"]);

        // A streaming engine counts through `stream`; `run_benchmark` always passes true for it.
        let bag = cell::<Bag>();
        bag.preflight(&catalog, &prepared, true).unwrap();
        assert_eq!(take_counted(), vec!["stream"]);
    }

    #[test]
    fn preflight_without_the_count_still_checks_support() {
        let catalog = duplicated_checks();
        let banned = prepared(&catalog, "Q(X) :- banned(X).");
        let error = cell::<Storing>()
            .preflight(&catalog, &banned, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("fake cannot run query"), "{error}");
        assert!(take_counted().is_empty());
    }

    #[test]
    fn a_cells_names_axes_and_levels() {
        let cell = cell::<Bag>();
        assert_eq!(cell.label(), "fake");
        assert_eq!(cell.group_segment(), "fake");
        assert_eq!(cell.trie_levels(), TrieLevels::All);
        let axes = cell.axes();
        assert_eq!(axes["system"], serde_json::json!("fake"));
        // The fake has no thread count, and `engine_version` comes from the subject.
        assert_eq!(axes.len(), 1);
        let per_index_bytes = std::hint::black_box(<EngineCell<Bag> as Cell>::PER_INDEX_BYTES);
        assert!(!per_index_bytes);
    }

    #[test]
    fn a_cell_supports_the_metrics_its_engine_can_measure() {
        let storing = cell::<Storing>();
        assert!(storing.supports(Metric::Build));
        assert!(storing.supports(Metric::Materialize));
        assert!(storing.supports(Metric::Space));
        assert!(!storing.supports(Metric::Iteration));
        assert!(!storing.supports(Metric::EndToEnd));

        let bag = cell::<Bag>();
        assert!(bag.supports(Metric::Build));
        assert!(bag.supports(Metric::Materialize));
        assert!(bag.supports(Metric::Iteration));
        assert!(bag.supports(Metric::EndToEnd));
        assert!(!bag.supports(Metric::Space));
    }

    #[test]
    fn the_skip_notes_name_each_metric_asked_for_and_not_measurable() {
        use Metric::{Build, EndToEnd, Iteration, Space};
        let storing = cell::<Storing>();
        assert_eq!(skip_notes(&storing, &[Build, Iteration, Space, EndToEnd], false), vec![
            "note: fake does not stream, so it skips iteration and end-to-end".to_string()
        ]);
        // A probe asks for a single `iteration` run without naming it.
        assert_eq!(skip_notes(&storing, &[Space], true), vec![
            "note: fake does not stream, so it skips iteration and end-to-end".to_string()
        ]);
        assert!(skip_notes(&storing, &[Build, Space], false).is_empty());

        let bag = cell::<Bag>();
        assert_eq!(skip_notes(&bag, &[Build, Iteration, Space, EndToEnd], false), vec![
            "note: fake reports no space".to_string()
        ]);
        assert!(skip_notes(&bag, &[Build, Iteration], false).is_empty());
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::engine`

Expected: the build fails. The file has no implementation yet, so the tests' imports and types are
unresolved: `error[E0432]: unresolved import` or `error[E0412]: cannot find type `EngineCell` in
this scope`, and `cannot find function `skip_notes` in this scope`. No test runs.

- [ ] **Step 3: Implement**

Insert this at the top of `kermit/src/bench/engine.rs`, above the test support from Step 1:

```rust
//! Engine cells: an external [`Engine`] (DuckDB, Nemo) run as one more cell of a `bench run`,
//! `bench join` or `bench probe` sweep.
//!
//! [`EngineSubject`] is the [`Subject`] the measure layer times: `build` is one timed engine
//! build, `exercise` one streamed count, `materialize` one stored result. [`EngineCell`] is the
//! [`Cell`] the per-query loop drives. Both are generic in the engine, so adding an engine adds
//! no code here. The dispatch in `run.rs` prints [`skip_notes`] once per cell and benchmark;
//! `run_benchmark` filters the requested metrics through [`Cell::supports`].
//!
//! Every engine method is fallible, but a timed body cannot return an error. So
//! [`EngineCell::preflight`] runs the whole path once (the count only when asked), untimed, and
//! turns any error into the cell's error; the timed bodies then `expect`, because an error that
//! first appears after a clean preflight is a broken invariant.

use {
    super::{
        cell::Cell,
        measure::{Materialized, Subject},
        Metric,
    },
    crate::bench_report::MetadataLine,
    anyhow::Context,
    kermit::{
        db::{check_multiplicity, Catalog, Prepared},
        engine::{Engine, EngineError},
    },
    kermit_ds::TrieLevels,
    std::collections::BTreeMap,
};

/// Unwraps an engine result that a clean preflight has already seen succeed.
fn expect_clean<T>(result: Result<T, EngineError>, step: &str) -> T {
    result.unwrap_or_else(|error| panic!("{step} failed after a clean preflight: {error}"))
}

/// A prepared query on one engine: the query's relations loaded into the engine, untimed.
///
/// `loaded` is `None` for a query made only of checks (all-constant atoms), which never reaches
/// the engine.
pub(crate) struct EngineSubject<'a, E: Engine + 'static> {
    loaded: Option<E::Loaded>,
    catalog: &'a Catalog,
    prepared: &'a Prepared,
}

impl<E: Engine + 'static> EngineSubject<'_, E> {
    /// The running engine's version, for the report's `engine_version` axis. `None` when the
    /// query never loaded the engine.
    fn version(&self) -> Option<String> { self.loaded.as_ref().map(E::version) }
}

/// What one timed build leaves: the engine's built state, and the product of the all-constant
/// atoms' counts (`kermit::db::check_multiplicity`), the lifecycle's rule.
pub(crate) struct EngineBuilt<'s, E: Engine + 'static> {
    /// `None` for a query of checks only, and for a zero multiplicity (no answers).
    engine: Option<E::Built<'s>>,
    multiplicity: usize,
}

impl<E: Engine + 'static> Subject for EngineSubject<'_, E> {
    type Built<'s>
        = EngineBuilt<'s, E>
    where
        Self: 's;

    /// `stream` does not change the built state; Nemo, which would change it, has no `stream`.
    const EXERCISE_MUTATES: bool = false;
    const STREAMS: bool = E::STREAMS;
    const MATERIALIZE_MUTATES: bool = E::MATERIALIZE_MUTATES;

    /// One timed build. The all-constant atoms' multiplicity is computed here, inside the timed
    /// body, as `kermit::db::build_with` computes it inside the timed build of a kermit cell.
    fn build(&self) -> EngineBuilt<'_, E> {
        let multiplicity = check_multiplicity(self.catalog, self.prepared, E::KEEPS_DUPLICATES);
        let engine = match &self.loaded {
            | Some(loaded) if multiplicity > 0 => Some(expect_clean(
                E::build(loaded, self.catalog, self.prepared),
                "build",
            )),
            | _ => None,
        };
        EngineBuilt {
            engine,
            multiplicity,
        }
    }

    /// The engine's own space as one figure, or none: an engine's bytes are not per index.
    fn heap_bytes(&self, built: &EngineBuilt<'_, E>) -> Vec<usize> {
        built
            .engine
            .as_ref()
            .and_then(E::space)
            .map(|bytes| vec![bytes])
            .unwrap_or_default()
    }

    /// The streamed row count, each row repeated `multiplicity` times as `kermit::db::execute`
    /// repeats it. A query of checks has the one empty row, `multiplicity` times; a zero
    /// multiplicity has none.
    fn exercise(&self, built: &EngineBuilt<'_, E>) -> u64 {
        assert!(
            E::STREAMS,
            "{} does not stream: exercise is only called on a streaming engine",
            E::NAME
        );
        let multiplicity = u64::try_from(built.multiplicity).unwrap_or(u64::MAX);
        match &built.engine {
            | Some(engine) => expect_clean(E::stream(engine), "stream").saturating_mul(multiplicity),
            | None => multiplicity,
        }
    }

    /// The distinct full assignments, and the engine's sorted batch as `kept` when it copies its
    /// rows into `Tuples` (DuckDB), which Criterion frees after the timer. The multiplicity
    /// repeats equal rows, which deduplication removes, so it only decides whether there are
    /// any; a query of checks has the one empty row.
    fn materialize(&self, built: &EngineBuilt<'_, E>) -> Option<Materialized> {
        let (rows, kept) = match (&built.engine, built.multiplicity) {
            | (_, 0) => (0, None),
            | (Some(engine), _) => expect_clean(E::materialize(engine), "materialize"),
            | (None, _) => (1, None),
        };
        Some(Materialized {
            rows,
            kept,
        })
    }
}

/// One engine, with its settings, as a cell of a sweep.
pub(crate) struct EngineCell<E: Engine> {
    pub(crate) settings: E::Settings,
}

impl<E: Engine + 'static> Cell for EngineCell<E> {
    type Subject<'a>
        = EngineSubject<'a, E>
    where
        Self: 'a;

    /// An engine's bytes are one figure for the whole query, so the report has no `index` lines.
    const PER_INDEX_BYTES: bool = false;

    fn label(&self) -> String { E::label(&self.settings) }

    fn metadata(&self) -> Vec<MetadataLine> {
        let mut lines = vec![MetadataLine::new("system", E::NAME)];
        if let Some(threads) = E::threads(&self.settings) {
            lines.push(MetadataLine::new("engine threads", threads));
        }
        lines
    }

    /// `system` and, for an engine with a thread count, `engine_threads`. The engine row omits
    /// `data_structure`, `algorithm` and every `ds_*` axis; `engine_version` comes from the
    /// subject ([`Cell::subject_axes`]).
    fn axes(&self) -> BTreeMap<String, serde_json::Value> {
        let mut axes = BTreeMap::from([("system".to_string(), serde_json::json!(E::NAME))]);
        if let Some(threads) = E::threads(&self.settings) {
            axes.insert("engine_threads".to_string(), serde_json::json!(threads));
        }
        axes
    }

    fn group_segment(&self) -> String { E::label(&self.settings) }

    /// Engines read the flat tuples through filters, so they share the sorted cells' prepared
    /// queries.
    fn trie_levels(&self) -> TrieLevels { TrieLevels::All }

    fn supports(&self, metric: Metric) -> bool {
        match metric {
            | Metric::Build | Metric::Materialize => true,
            | Metric::Iteration | Metric::EndToEnd => E::STREAMS,
            | Metric::Space => E::HAS_SPACE,
        }
    }

    /// Support, then load and build, then, when `count` is true, one count (`stream`, or else
    /// `materialize`), untimed. `count` is false only when the memory guard skips `materialize`
    /// for an engine that does not stream; `run_benchmark` always passes true for a streaming one.
    /// A query of checks never reaches the engine, so it needs no check.
    fn preflight(
        &self, catalog: &Catalog, prepared: &Prepared, count: bool,
    ) -> anyhow::Result<()> {
        if prepared.indexes().is_empty() {
            return Ok(());
        }
        let label = self.label();
        E::supports(prepared).map_err(|unsupported| {
            anyhow::anyhow!(
                "{label} cannot run query '{}': {}",
                prepared.query(),
                unsupported.reason
            )
        })?;
        let context = || format!("{label} failed on query '{}'", prepared.query());
        let loaded = E::load(&self.settings, catalog, prepared).with_context(context)?;
        let built = E::build(&loaded, catalog, prepared).with_context(context)?;
        if count {
            if E::STREAMS {
                E::stream(&built).with_context(context)?;
            } else {
                E::materialize(&built).with_context(context)?;
            }
        }
        Ok(())
    }

    fn subject<'a>(
        &'a self, catalog: &'a Catalog, prepared: &'a Prepared,
    ) -> anyhow::Result<EngineSubject<'a, E>> {
        let loaded = if prepared.indexes().is_empty() {
            None
        } else {
            let label = self.label();
            Some(
                E::load(&self.settings, catalog, prepared)
                    .with_context(|| format!("{label} failed on query '{}'", prepared.query()))?,
            )
        };
        Ok(EngineSubject {
            loaded,
            catalog,
            prepared,
        })
    }

    fn subject_axes(&self, subject: &EngineSubject<'_, E>) -> BTreeMap<String, serde_json::Value> {
        subject
            .version()
            .map(|version| BTreeMap::from([("engine_version".to_string(), serde_json::json!(version))]))
            .unwrap_or_default()
    }
}

/// The stderr notes for the metrics a cell cannot measure and the run asked for: `iteration` and
/// `end-to-end` of an engine that does not stream, and `space` of one that reports none. A
/// probe (`single_run`) asks for a single `iteration` run without naming it. The measure layer
/// (Story 3) acts on the same fact: `Described::measure` skips `iteration` and `end_to_end` for
/// a non-streaming subject, and `Described::materialize_once` / `Described::materialize_time`
/// give it its single-run and timed `materialize`.
pub(crate) fn skip_notes<C: Cell>(cell: &C, metrics: &[Metric], single_run: bool) -> Vec<String> {
    let asked = |metric: Metric| {
        metrics.contains(&metric) || (single_run && metric == Metric::Iteration)
    };
    let label = cell.label();
    let mut notes = Vec::new();
    if (asked(Metric::Iteration) || asked(Metric::EndToEnd))
        && !(cell.supports(Metric::Iteration) && cell.supports(Metric::EndToEnd))
    {
        notes.push(format!(
            "note: {label} does not stream, so it skips iteration and end-to-end"
        ));
    }
    if asked(Metric::Space) && !cell.supports(Metric::Space) {
        notes.push(format!("note: {label} reports no space"));
    }
    notes
}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::engine`

Expected: `test result: ok.` with the thirteen `bench::engine::tests::…` tests passing and none
failed. Then run the gates that build no features:
`nix develop --command cargo fmt --all` and
`nix develop --command cargo clippy -p kermit --all-targets -- -D warnings` (no warnings: the
module's `cfg_attr(…, allow(dead_code))` covers the engine items that only a feature uses).

- [ ] **Step 5: Commit**

```bash
git add -N kermit/src/bench/engine.rs
git commit -m "feat(bench): EngineSubject and EngineCell for any Engine" -- kermit/src/bench/engine.rs kermit/src/bench/mod.rs
```

### Task 10.2: `--engine` and `--engine-threads`

**Files:**
- Modify: `kermit/src/options.rs` (before the line `#[cfg(test)]` that opens `mod tests`, and inside that module)
- Test: `kermit/src/options.rs`

- [ ] **Step 1: Write the failing test**

In the `mod tests` of `kermit/src/options.rs`, insert these tests directly before the doc comment
line ``    /// Every `--ds-layout-hasher` choice names exactly one `HashStrategy` marker and is recovered from it. The table is explicit so a new``
(the line the test `hasher_choices_round_trip_through_layout_names` starts with; insert above its
doc comment):

```rust
    #[derive(Parser)]
    struct EngineCli {
        #[command(flatten)]
        engines: EngineChoices,
    }

    fn engine_choices(args: &[&str]) -> EngineChoices {
        EngineCli::try_parse_from(std::iter::once("kermit").chain(args.iter().copied()))
            .expect("the engine flags parse")
            .engines
    }

    #[test]
    fn engine_flags_parse_a_comma_list_and_default_to_one_thread() {
        let none = engine_choices(&[]);
        assert!(none.engine.is_empty());
        assert_eq!(none.engine_threads, 1);

        let both = engine_choices(&["--engine", "duckdb,nemo", "--engine-threads", "2"]);
        assert_eq!(both.engine, vec![EngineSelector::Duckdb, EngineSelector::Nemo]);
        assert_eq!(both.engine_threads, 2);

        let repeated = engine_choices(&["--engine", "nemo", "--engine", "duckdb"]);
        assert_eq!(repeated.engine, vec![EngineSelector::Nemo, EngineSelector::Duckdb]);

        assert!(EngineCli::try_parse_from(["kermit", "--engine", "umbra"]).is_err());
    }

    #[test]
    fn no_engine_resolves_to_no_cells_whatever_the_thread_flag_holds() {
        // `Default` leaves `engine_threads` at 0; with no engine it is never read.
        assert_eq!(EngineChoices::default().resolve_within(4).unwrap(), vec![]);
    }

    #[test]
    fn engine_threads_outside_one_to_the_available_parallelism_is_refused() {
        for threads in [0, 5] {
            let choices = EngineChoices {
                engine: vec![EngineSelector::Duckdb],
                engine_threads: threads,
            };
            assert_eq!(
                choices.resolve_within(4).unwrap_err().to_string(),
                "--engine-threads must be between 1 and 4"
            );
        }
    }

    #[test]
    fn an_engine_whose_feature_is_not_compiled_in_is_a_usage_error() {
        let choices = |engine| EngineChoices {
            engine: vec![engine],
            engine_threads: 1,
        };
        for engine in [EngineSelector::Duckdb, EngineSelector::Nemo] {
            let name = engine.name();
            let resolved = choices(engine).resolve_within(4);
            if engine.compiled() {
                assert_eq!(resolved.unwrap(), vec![EngineChoice {
                    engine,
                    threads: 1,
                }]);
            } else {
                assert_eq!(
                    resolved.unwrap_err().to_string(),
                    format!(
                        "--engine {name} needs the engine-{name} feature (build with --features \
                         engine-{name})"
                    )
                );
            }
        }
    }

    #[test]
    fn a_repeated_engine_resolves_once_in_first_given_order() {
        let engine = [EngineSelector::Nemo, EngineSelector::Duckdb]
            .into_iter()
            .find(|engine| engine.compiled());
        // Without an engine feature there is nothing to resolve here.
        let Some(engine) = engine else { return };
        let choices = EngineChoices {
            engine: vec![engine, engine],
            engine_threads: 2,
        };
        assert_eq!(choices.resolve_within(4).unwrap(), vec![EngineChoice {
            engine,
            threads: 2,
        }]);
    }

    #[test]
    fn a_choice_names_its_criterion_segment_and_stderr_label() {
        let duckdb = EngineChoice {
            engine: EngineSelector::Duckdb,
            threads: 4,
        };
        let nemo = EngineChoice {
            engine: EngineSelector::Nemo,
            threads: 4,
        };
        assert_eq!(duckdb.label(), "duckdb-t4");
        assert_eq!(nemo.label(), "nemo");
    }

    /// The label is spelled twice, here (so a build without the feature can name a group) and by
    /// `Engine::label`; they must agree.
    #[cfg(feature = "engine-duckdb")]
    #[test]
    fn the_duckdb_choice_label_is_the_engines_label() {
        use kermit::engine::{
            duckdb::{DuckDb, DuckDbSettings},
            Engine,
        };
        let choice = EngineChoice {
            engine: EngineSelector::Duckdb,
            threads: 3,
        };
        assert_eq!(
            choice.label(),
            DuckDb::label(&DuckDbSettings {
                threads: 3
            })
        );
    }

    #[cfg(feature = "engine-nemo")]
    #[test]
    fn the_nemo_choice_label_is_the_engines_label() {
        use kermit::engine::{nemo::Nemo, Engine};
        let choice = EngineChoice {
            engine: EngineSelector::Nemo,
            threads: 1,
        };
        assert_eq!(choice.label(), Nemo::label(&()));
    }

```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --bin kermit options::tests`

Expected: the build fails with `error[E0412]: cannot find type `EngineChoices` in this scope`,
and the same for `EngineSelector` and `EngineChoice`.

- [ ] **Step 3: Implement**

In `kermit/src/options.rs`, replace these lines, which end `DsChoices::resolve` and open the test
module:

```rust
        Ok(choices)
    }
}

#[cfg(test)]
mod tests {
```

with:

```rust
        Ok(choices)
    }
}

/// An external engine `--engine` can add to a `bench run`, `bench probe` or `bench join`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
// Used from `bench/run.rs` (Task 10.3), which removes this allow.
#[allow(dead_code)]
pub(crate) enum EngineSelector {
    /// DuckDB, in-process (feature `engine-duckdb`).
    Duckdb,
    /// Nemo, in-process (feature `engine-nemo`).
    Nemo,
}

#[allow(dead_code)]
impl EngineSelector {
    /// The flag value, which is also the cargo feature's suffix (`engine-{name}`).
    pub(crate) fn name(self) -> &'static str {
        match self {
            | Self::Duckdb => "duckdb",
            | Self::Nemo => "nemo",
        }
    }

    /// Whether this build compiled the engine in.
    pub(crate) fn compiled(self) -> bool {
        match self {
            | Self::Duckdb => cfg!(feature = "engine-duckdb"),
            | Self::Nemo => cfg!(feature = "engine-nemo"),
        }
    }

    /// The usage error for an engine whose feature this build lacks.
    pub(crate) fn missing_feature(self) -> anyhow::Error {
        let name = self.name();
        anyhow::anyhow!(
            "--engine {name} needs the engine-{name} feature (build with --features engine-{name})"
        )
    }
}

/// `--engine` and `--engine-threads`: the engines a run adds beside its `-i` / `-a` cells.
/// Flattened into `bench run` and `bench probe` (where `-i` and `-a` are then optional) and
/// into `bench join` (where they stay required). The `--ds-*` flags never reach an engine.
#[derive(Args, Clone, Debug, Default)]
#[allow(dead_code)]
pub(crate) struct EngineChoices {
    /// External engines to run beside the kermit cells, comma-separated or repeated (`duckdb`,
    /// `nemo`). An engine whose cargo feature is not compiled in is a usage error.
    #[arg(long, value_name = "ENGINE", value_enum, value_delimiter = ',')]
    pub(crate) engine: Vec<EngineSelector>,

    /// DuckDB's thread count: 1 up to the available parallelism. Nemo is single-threaded and
    /// ignores it.
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) engine_threads: usize,
}

/// One resolved `--engine` cell.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct EngineChoice {
    pub(crate) engine: EngineSelector,
    /// `--engine-threads`; only DuckDB reads it.
    pub(crate) threads: usize,
}

#[allow(dead_code)]
impl EngineChoice {
    /// The Criterion group's last segment and the stderr label, as `Engine::label` spells them:
    /// `duckdb-t{threads}`, `nemo`. Written out here so that a build without the engine's feature
    /// can still name its group; a feature-gated test pins the two together.
    pub(crate) fn label(self) -> String {
        match self.engine {
            | EngineSelector::Duckdb => format!("duckdb-t{}", self.threads),
            | EngineSelector::Nemo => "nemo".to_string(),
        }
    }
}

#[allow(dead_code)]
impl EngineChoices {
    /// The cells `--engine` names, in the order given and without repeats.
    ///
    /// # Errors
    ///
    /// `--engine-threads` outside `1..=available_parallelism()`, or an engine whose cargo feature
    /// this build lacks.
    pub(crate) fn resolve(&self) -> anyhow::Result<Vec<EngineChoice>> {
        let available = std::thread::available_parallelism().map_or(1, usize::from);
        self.resolve_within(available)
    }

    /// [`resolve`](Self::resolve) for a machine with `available` hardware threads.
    fn resolve_within(&self, available: usize) -> anyhow::Result<Vec<EngineChoice>> {
        if self.engine.is_empty() {
            return Ok(Vec::new());
        }
        if !(1..=available).contains(&self.engine_threads) {
            anyhow::bail!("--engine-threads must be between 1 and {available}");
        }
        let mut selected: Vec<EngineSelector> = Vec::new();
        for &engine in &self.engine {
            if !selected.contains(&engine) {
                selected.push(engine);
            }
        }
        if let Some(missing) = selected.iter().find(|engine| !engine.compiled()) {
            return Err(missing.missing_feature());
        }
        Ok(selected
            .into_iter()
            .map(|engine| EngineChoice {
                engine,
                threads: self.engine_threads,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --bin kermit options::tests`

Expected: `test result: ok.` for every `options::tests` test, six of them new (the two
feature-gated label tests run only under their features). Then
`nix develop --command cargo test -p kermit --features engine-duckdb,engine-nemo --bin kermit options::tests`
passes all eight. Then `nix develop --command cargo fmt --all` and
`nix develop --command cargo clippy -p kermit --all-targets -- -D warnings` (no warnings: the
temporary `allow(dead_code)` attributes cover the types until Task 10.3).

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(options): --engine and --engine-threads" -- kermit/src/options.rs
```

### Task 10.3: `SweepCell`, the sweep and `bench run` / `bench probe`

**Files:**
- Modify: `kermit/src/options.rs` (remove the temporary `allow(dead_code)` of Task 10.2)
- Modify: `kermit/src/bench/run.rs` (new items after `dispatch_run_bench`; `criterion_group_name`; `check_sweep_group_directories`; tests)
- Modify: `kermit/src/main.rs` (`RunSelection`; `run_bench_run_command`)
- Test: `kermit/src/bench/run.rs`

- [ ] **Step 1: Write the failing test**

In `kermit/src/bench/run.rs`, inside `mod tests`, first move Story 4's group-name test onto
`SweepCell`, since `criterion_group_name` takes one after Step 3. Replace its body, currently:

```rust
        let cell =
            Execution::for_structure(kermit_ds::IndexStructure::ColumnTrie, DsChoices::default());
        assert_eq!(
            criterion_group_name("run", "triangle", "q0000", cell),
            "run/triangle/q0000/ColumnTrie/LeapfrogTriejoin"
        );
        let cell =
            Execution::for_structure(kermit_ds::IndexStructure::HashTrie, DsChoices::default());
        assert_eq!(
            criterion_group_name("join", "adhoc", "q", cell),
            "join/adhoc/q/HashTrie/HashTriejoin"
        );
```

with:

```rust
        let cell = SweepCell::Kermit(Execution::for_structure(
            kermit_ds::IndexStructure::ColumnTrie,
            DsChoices::default(),
        ));
        assert_eq!(
            criterion_group_name("run", "triangle", "q0000", cell),
            "run/triangle/q0000/ColumnTrie/LeapfrogTriejoin"
        );
        let cell = SweepCell::Kermit(Execution::for_structure(
            kermit_ds::IndexStructure::HashTrie,
            DsChoices::default(),
        ));
        assert_eq!(
            criterion_group_name("join", "adhoc", "q", cell),
            "join/adhoc/q/HashTrie/HashTriejoin"
        );
```

Then insert these tests directly before
```rust
    #[test]
    fn groups_sharing_a_truncated_directory_are_rejected() {
```

```rust
    fn duckdb(threads: usize) -> SweepCell {
        SweepCell::Engine(crate::options::EngineChoice {
            engine: crate::options::EngineSelector::Duckdb,
            threads,
        })
    }

    fn nemo() -> SweepCell {
        SweepCell::Engine(crate::options::EngineChoice {
            engine: crate::options::EngineSelector::Nemo,
            threads: 1,
        })
    }

    fn tree_trie() -> SweepCell {
        SweepCell::Kermit(Execution::for_structure(
            kermit_ds::IndexStructure::TreeTrie,
            DsChoices::default(),
        ))
    }

    #[test]
    fn a_group_name_ends_in_the_cells_segment() {
        assert_eq!(
            criterion_group_name("run", "triangle", "triangle", tree_trie()),
            "run/triangle/triangle/TreeTrie/LeapfrogTriejoin"
        );
        assert_eq!(
            criterion_group_name("run", "triangle", "triangle", duckdb(4)),
            "run/triangle/triangle/duckdb-t4"
        );
        assert_eq!(
            criterion_group_name("run", "triangle", "triangle", nemo()),
            "run/triangle/triangle/nemo"
        );
    }

    /// The 64-byte clash check covers engine cells: past byte 64 the engines' segments are
    /// truncated away, so two of them (or one and a kermit cell) share a directory.
    #[test]
    fn engine_groups_share_a_criterion_directory_past_byte_64() {
        let prefix = format!("run-{}", "long-prefix-".repeat(6));
        let groups = |cells: &[SweepCell]| {
            cells
                .iter()
                .map(|&cell| criterion_group_name(&prefix, "triangle", "triangle", cell))
                .collect::<Vec<_>>()
        };
        let clash = check_group_directories_distinct(groups(&[duckdb(1), nemo()]))
            .unwrap_err()
            .to_string();
        assert!(clash.contains("/duckdb-t1"), "{clash}");
        assert!(clash.contains("/nemo"), "{clash}");
        assert!(clash.contains("64"), "{clash}");
        check_group_directories_distinct(groups(&[duckdb(1)])).unwrap();
    }

    #[test]
    fn a_cells_trie_levels_follow_its_kind() {
        assert_eq!(nemo().trie_levels(), kermit_ds::TrieLevels::All);
        assert_eq!(duckdb(2).trie_levels(), kermit_ds::TrieLevels::All);
        let keys = DsChoices {
            levels: crate::options::LevelsChoice::Keys,
            ..DsChoices::default()
        };
        let hash_trie = Execution::for_structure(kermit_ds::IndexStructure::HashTrie, keys);
        assert_eq!(
            SweepCell::Kermit(hash_trie).trie_levels(),
            kermit_ds::TrieLevels::JoinKeys
        );
    }

    #[test]
    fn the_sweep_lists_the_kermit_cells_then_the_engines() {
        let choice = |engine| crate::options::EngineChoice {
            engine,
            threads: 1,
        };
        let engines = vec![
            choice(crate::options::EngineSelector::Duckdb),
            choice(crate::options::EngineSelector::Nemo),
        ];
        let kermit = vec![Execution::for_structure(
            kermit_ds::IndexStructure::TreeTrie,
            DsChoices::default(),
        )];
        let cells = sweep_cells(kermit.clone(), &[], engines.clone()).unwrap();
        assert_eq!(cells, vec![
            SweepCell::Kermit(kermit[0]),
            SweepCell::Engine(engines[0]),
            SweepCell::Engine(engines[1]),
        ]);
        // Engines alone are a sweep too: `-i` and `-a` are optional beside `--engine`.
        let alone = sweep_cells(Vec::new(), &[], engines.clone()).unwrap();
        assert_eq!(alone.len(), 2);
    }

    #[test]
    fn a_sweep_with_no_cell_is_refused() {
        let error = sweep_cells(Vec::new(), &[], Vec::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("nothing to run"), "{error}");
        assert!(error.contains("-i and -a"), "{error}");
        assert!(error.contains("--engine"), "{error}");
    }

    #[test]
    fn a_ds_flag_needs_a_kermit_cell_to_reach() {
        let engines = vec![crate::options::EngineChoice {
            engine: crate::options::EngineSelector::Nemo,
            threads: 1,
        }];
        let error = sweep_cells(Vec::new(), &[DsFlag::LayoutSeek], engines.clone())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("--ds-layout-seek applies only to tree-trie or column-trie cells"),
            "{error}"
        );
        assert!(error.contains("--engine cells take no --ds-* flags"), "{error}");
        // With a kermit cell the flag is the sweep's business (`resolve_sweep`), not this check's.
        let kermit = vec![Execution::for_structure(
            kermit_ds::IndexStructure::TreeTrie,
            DsChoices::default(),
        )];
        sweep_cells(kermit, &[DsFlag::LayoutSeek], engines).unwrap();
    }

```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::run::tests`

Expected: the build fails: `error[E0433]: failed to resolve: use of undeclared type `SweepCell``
(and `cannot find function `sweep_cells` in this scope`).

- [ ] **Step 3: Implement**

**3a. `kermit/src/options.rs`.** Remove the five temporary `#[allow(dead_code)]` attributes added
in Task 10.2, and the comment line `// Used from `bench/run.rs` (Task 10.3), which removes this
allow.` above the first one. The types are now used by `run.rs` and `main.rs`.

**3b. `kermit/src/bench/run.rs`, imports and items.** Insert this directly before the doc comment
line `/// Everything a measured join needs besides the cell and the workload:`:

```rust
use {
    super::engine::{skip_notes, EngineCell},
    kermit::engine::Engine,
};

/// One cell of a `bench run`, `bench probe` or `bench join` sweep: a kermit
/// (structure, algorithm) cell, or an external engine.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum SweepCell {
    Kermit(Execution),
    Engine(crate::options::EngineChoice),
}

impl SweepCell {
    /// The trie-level policy this cell's queries are prepared under. Engines
    /// read no trie, and share the sorted cells' `All`.
    pub(crate) fn trie_levels(self) -> kermit_ds::TrieLevels {
        match self {
            | Self::Kermit(execution) => execution.trie_levels(),
            | Self::Engine(_) => kermit_ds::TrieLevels::All,
        }
    }

    /// The Criterion group's last segments: `{ds}/{algo}` (Story 4's
    /// `execution_segment`), or the engine's label.
    fn group_segment(self) -> String {
        match self {
            | Self::Kermit(execution) => execution_segment(execution),
            | Self::Engine(choice) => choice.label(),
        }
    }
}

/// The sweep a run covers: its kermit cells (from `-i` / `-a`, already
/// expanded by [`resolve_sweep`], empty without them), then its engines.
///
/// `given` are the `--ds-*` flags the user passed. They reach kermit cells
/// only, so one given with no kermit cell would be silently ignored, which is
/// a usage error (#86).
///
/// # Errors
///
/// No cell at all, or a `--ds-*` flag with no kermit cell to reach.
pub(crate) fn sweep_cells(
    kermit: Vec<Execution>, given: &[DsFlag], engines: Vec<crate::options::EngineChoice>,
) -> anyhow::Result<Vec<SweepCell>> {
    if kermit.is_empty() && engines.is_empty() {
        anyhow::bail!("nothing to run: give -i and -a, or --engine");
    }
    if kermit.is_empty() {
        if let Some(flag) = given.first() {
            anyhow::bail!(
                "{flag} applies only to {} cells, but this run names no -i and -a; --engine cells \
                 take no --ds-* flags",
                flag.structures_label()
            );
        }
    }
    Ok(kermit
        .into_iter()
        .map(SweepCell::Kermit)
        .chain(engines.into_iter().map(SweepCell::Engine))
        .collect())
}

/// Runs one sweep cell over `workload`: a kermit cell through
/// [`dispatch_run_bench`], an engine through [`dispatch_engine_bench`].
pub(crate) fn dispatch_sweep_cell(
    cell: SweepCell, workload: &PreparedWorkload, settings: RunSettings<'_>, sink: &mut ReportSink,
) -> anyhow::Result<()> {
    match cell {
        | SweepCell::Kermit(execution) => dispatch_run_bench(execution, workload, settings, sink),
        | SweepCell::Engine(choice) => dispatch_engine_bench(choice, workload, settings, sink),
    }
}

/// Monomorphises [`run_engine_cell`] over the engine `choice` names. An engine
/// whose feature this build lacks is refused before any cell runs
/// (`EngineChoices::resolve`), so its arm here only keeps the match total.
fn dispatch_engine_bench(
    choice: crate::options::EngineChoice, workload: &PreparedWorkload, settings: RunSettings<'_>,
    sink: &mut ReportSink,
) -> anyhow::Result<()> {
    match choice.engine {
        | crate::options::EngineSelector::Duckdb => {
            run_duckdb(choice.threads, workload, settings, sink)
        },
        | crate::options::EngineSelector::Nemo => run_nemo(workload, settings, sink),
    }
}

#[cfg(feature = "engine-duckdb")]
fn run_duckdb(
    threads: usize, workload: &PreparedWorkload, settings: RunSettings<'_>, sink: &mut ReportSink,
) -> anyhow::Result<()> {
    run_engine_cell(
        &EngineCell::<kermit::engine::duckdb::DuckDb> {
            settings: kermit::engine::duckdb::DuckDbSettings {
                threads,
            },
        },
        workload,
        settings,
        sink,
    )
}

#[cfg(not(feature = "engine-duckdb"))]
fn run_duckdb(
    _threads: usize, _workload: &PreparedWorkload, _settings: RunSettings<'_>,
    _sink: &mut ReportSink,
) -> anyhow::Result<()> {
    Err(crate::options::EngineSelector::Duckdb.missing_feature())
}

#[cfg(feature = "engine-nemo")]
fn run_nemo(
    workload: &PreparedWorkload, settings: RunSettings<'_>, sink: &mut ReportSink,
) -> anyhow::Result<()> {
    run_engine_cell(
        &EngineCell::<kermit::engine::nemo::Nemo> {
            settings: (),
        },
        workload,
        settings,
        sink,
    )
}

#[cfg(not(feature = "engine-nemo"))]
fn run_nemo(
    _workload: &PreparedWorkload, _settings: RunSettings<'_>, _sink: &mut ReportSink,
) -> anyhow::Result<()> {
    Err(crate::options::EngineSelector::Nemo.missing_feature())
}

/// Runs one engine cell over every query of `workload`: the skip notes for
/// the metrics it cannot measure, once per cell and benchmark, then
/// [`run_benchmark`] over the requested metrics. `run_benchmark` filters them
/// through `Cell::supports` and prints no note; this function filters nothing.
// Only a feature that compiles an engine in calls it.
#[cfg_attr(
    not(any(feature = "engine-duckdb", feature = "engine-nemo")),
    allow(dead_code)
)]
fn run_engine_cell<E: Engine + 'static>(
    cell: &EngineCell<E>, workload: &PreparedWorkload, settings: RunSettings<'_>,
    sink: &mut ReportSink,
) -> anyhow::Result<()> {
    for note in skip_notes(cell, settings.metrics, settings.single_run) {
        eprintln!("{note}");
    }
    run_benchmark(cell, workload, settings, sink)
}

```

**3c. `kermit/src/bench/run.rs`, group names and the clash check.** Story 4 split the group name
into `group_with_segment` (which `run_benchmark` calls with `cell.group_segment()`, and which this
story leaves alone) and a thin `criterion_group_name` over an `Execution`. Replace
`criterion_group_name` (Story 4's text):

```rust
/// [`group_with_segment`] for an [`Execution`] cell, which is how the sweep's clash
/// check names the groups before any cell runs.
fn criterion_group_name(prefix: &str, workload: &str, query: &str, cell: Execution) -> String {
    group_with_segment(prefix, workload, query, &execution_segment(cell))
}
```

with:

```rust
/// [`group_with_segment`] for a [`SweepCell`], which is how the sweep's clash
/// check names the groups before any cell runs: `{ds}/{algo}` for a kermit
/// cell, the engine's label for an engine.
fn criterion_group_name(prefix: &str, workload: &str, query: &str, cell: SweepCell) -> String {
    group_with_segment(prefix, workload, query, &cell.group_segment())
}
```

In `check_sweep_group_directories`, change the parameter. Replace:

```rust
    prefix: &str, benchmarks: &[BenchmarkDefinition], query_filter: Option<&str>,
    cells: &[Execution],
) -> anyhow::Result<()> {
```

with:

```rust
    prefix: &str, benchmarks: &[BenchmarkDefinition], query_filter: Option<&str>,
    cells: &[SweepCell],
) -> anyhow::Result<()> {
```

`run_benchmark` no longer calls `criterion_group_name`: Story 4 names its group through
`group_with_segment(…, &group_segment)`, with the segment from `Cell::group_segment`, which is the
same name for a kermit cell and the engine's label for an engine. Run
`grep -n 'criterion_group_name(' kermit/src/bench/run.rs`: the only calls must be the one in
`check_sweep_group_directories` and those in the tests.

**3d. `kermit/src/main.rs`, `RunSelection`.** Replace, in `RunSelection`:

```rust
    /// Data structure (`all` sweeps every available structure)
    #[arg(
        short,
        long,
        value_name = "INDEXSTRUCTURE",
        required = true,
        value_enum
    )]
    indexstructure: IndexStructureSelector,

    /// Join algorithm (`all` sweeps every available algorithm)
    #[arg(short, long, value_name = "ALGORITHM", required = true, value_enum)]
    algorithm: JoinAlgorithmSelector,

    #[command(flatten)]
    planner: PlannerArgs,

    /// Regenerate generator-driven benchmarks if their cached
```

with:

```rust
    /// Data structure (`all` sweeps every available structure). Required
    /// unless `--engine` names the cells, and then only together with `-a`.
    #[arg(
        short,
        long,
        value_name = "INDEXSTRUCTURE",
        required_unless_present = "engine",
        requires = "algorithm",
        value_enum
    )]
    indexstructure: Option<IndexStructureSelector>,

    /// Join algorithm (`all` sweeps every available algorithm). Required
    /// unless `--engine` names the cells, and then only together with `-i`.
    #[arg(
        short,
        long,
        value_name = "ALGORITHM",
        required_unless_present = "engine",
        requires = "indexstructure",
        value_enum
    )]
    algorithm: Option<JoinAlgorithmSelector>,

    #[command(flatten)]
    planner: PlannerArgs,

    /// Regenerate generator-driven benchmarks if their cached
```

and, directly after the `force` field (`    #[arg(long)]\n    force: bool,`), add:

```rust
    #[command(flatten)]
    engines: EngineChoices,
```

At the top of `kermit/src/main.rs`, extend the `options::{…}` list in the `use { … }` block that
begins `bench::{`, adding `EngineChoices`: the `options::{ with_hash_trie_layout,
with_sorted_trie_layout, BuildChoices, ConfigChoices, DsChoices, DsFlag, LayoutChoices,
PlannerArgs, }` list becomes `… DsFlag, EngineChoices, LayoutChoices, PlannerArgs, }`.

**3e. `kermit/src/main.rs`, `run_bench_run_command`.** In its destructuring of `selection`,
replace the two lines

```rust
        force,
        layout,
```

with

```rust
        force,
        engines,
        layout,
```

Then replace the three lines

```rust
    let choices = DsChoices::resolve(indexstructure, &layout, &config, &build)?;
    let given = DsFlag::given(&layout, &config, &build);
    let cells = resolve_sweep(indexstructure, algorithm, choices, &given, kind)?;
```

with:

```rust
    // Without `-i` / `-a` (engines only) every `--ds-*` flag is checked against all structures
    // here, and `sweep_cells` then refuses one that has no kermit cell to reach.
    let choices = DsChoices::resolve(
        indexstructure.unwrap_or(IndexStructureSelector::All),
        &layout,
        &config,
        &build,
    )?;
    let given = DsFlag::given(&layout, &config, &build);
    let kermit_cells = match (indexstructure, algorithm) {
        | (Some(indexstructure), Some(algorithm)) => {
            resolve_sweep(indexstructure, algorithm, choices, &given, kind)?
        },
        // clap's `requires` rules out one selector without the other.
        | _ => Vec::new(),
    };
    let cells = bench::run::sweep_cells(kermit_cells, &given, engines.resolve()?)?;
```

and, in the loop over cells, replace

```rust
        for &cell in &cells {
            dispatch_run_bench(cell, &workload, settings, &mut sink).with_context(|| {
```

with

```rust
        for &cell in &cells {
            bench::run::dispatch_sweep_cell(cell, &workload, settings, &mut sink).with_context(|| {
```

(`policies`, which maps each cell through `cell.trie_levels()`, and the call to
`check_sweep_group_directories(prefix, &materialized, query.as_deref(), &cells)` need no change:
both now take `SweepCell`s. Story 3's guard around that call in `main.rs`, the condition
`!single_run && any(is_timed)`, stays as it is: a probe runs no Criterion, so an engine-only probe
skips the clash check as a kermit probe does.)

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --bin kermit bench::run::tests`

Expected: `test result: ok.` with the six new tests and the existing `bench::run::tests`
(Story 4's group-name test, now on `SweepCell`, among them) passing. Then the whole binary and the gates that build no features:
`nix develop --command cargo test -p kermit --bin kermit`,
`nix develop --command cargo fmt --all` and
`nix develop --command cargo clippy -p kermit --all-targets -- -D warnings` (no warnings: the
non-feature `run_duckdb` / `run_nemo` and `run_engine_cell`'s `cfg_attr` keep every item used).
Then `nix develop --command cargo test -p kermit --features engine-duckdb,engine-nemo --bin kermit`.

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(bench): engine cells join the bench run and probe sweep" -- kermit/src/options.rs kermit/src/bench/run.rs kermit/src/main.rs
```

### Task 10.4: `bench join --engine`, the CLI tests and the acceptance run

**Files:**
- Modify: `kermit/src/main.rs` (`BenchSubcommand::Join`, `prepare_query`, `run_bench_join`, `main`)
- Create: `kermit/tests/cli_engines.rs`
- Test: `kermit/tests/cli_engines.rs`

- [ ] **Step 1: Write the failing test**

Create `kermit/tests/cli_engines.rs`:

```rust
//! The engine CLI: `--engine` and `--engine-threads` on `bench run`, `bench probe` and
//! `bench join`. The usage errors hold in every build; the report tests run only with both
//! engine features (`--features engine-duckdb,engine-nemo`), on the committed `triangle`
//! benchmark, so they need no network and no cache.

use {
    std::{
        fs,
        path::PathBuf,
        process::{Command, Output},
    },
    tempfile::TempDir,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

/// Runs `kermit bench --sample-size 10 --measurement-time 1 --warm-up-time 1 --report-json
/// <tmp>/report.json <subcommand and args>` with Criterion and the cache redirected into `tmp`.
/// Returns the output and the parsed report array (empty when no report was written).
fn bench(tmp: &TempDir, name: Option<&str>, args: &[&str]) -> (Output, Vec<serde_json::Value>) {
    let report = tmp.path().join("report.json");
    let mut command = Command::new(kermit_bin());
    command
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LIB_BACKTRACE", "0")
        .env("CRITERION_HOME", tmp.path().join("criterion"))
        .env("XDG_CACHE_HOME", tmp.path().join("cache"))
        .arg("bench");
    if let Some(name) = name {
        command.args(["--name", name]);
    }
    let output = command
        .args([
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
            "--report-json",
        ])
        .arg(&report)
        .args(args)
        .output()
        .expect("failed to spawn kermit");
    let reports = fs::read_to_string(&report)
        .ok()
        .map(|text| serde_json::from_str(&text).expect("the report is a JSON array"))
        .unwrap_or_default();
    (output, reports)
}

fn stderr(output: &Output) -> String { String::from_utf8_lossy(&output.stderr).into_owned() }

#[test]
fn without_engine_the_selectors_stay_required() {
    let tmp = TempDir::new().unwrap();
    let (output, reports) = bench(&tmp, None, &["run", "triangle"]);
    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("--indexstructure"), "{stderr}");
    assert!(reports.is_empty());
}

#[test]
fn one_selector_without_the_other_is_refused() {
    let tmp = TempDir::new().unwrap();
    let (output, reports) = bench(&tmp, None, &["run", "triangle", "-i", "all", "--engine", "nemo"]);
    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("--algorithm"), "{stderr}");
    assert!(reports.is_empty());
}

#[test]
fn engine_threads_outside_its_range_is_a_usage_error() {
    let tmp = TempDir::new().unwrap();
    for threads in ["0", "999999"] {
        let (output, reports) = bench(&tmp, None, &[
            "probe",
            "triangle",
            "--engine",
            "duckdb",
            "--engine-threads",
            threads,
        ]);
        assert!(!output.status.success(), "{threads}");
        let stderr = stderr(&output);
        assert!(
            stderr.contains("--engine-threads must be between 1 and "),
            "{threads}: {stderr}"
        );
        assert!(reports.is_empty());
    }
}

#[cfg(not(feature = "engine-nemo"))]
#[test]
fn an_engine_without_its_feature_is_a_usage_error() {
    let tmp = TempDir::new().unwrap();
    let (output, reports) = bench(&tmp, None, &["probe", "triangle", "--engine", "nemo"]);
    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(
        stderr.contains(
            "--engine nemo needs the engine-nemo feature (build with --features engine-nemo)"
        ),
        "{stderr}"
    );
    assert!(reports.is_empty());
}

#[cfg(feature = "engine-nemo")]
#[test]
fn a_ds_flag_with_no_kermit_cell_is_refused() {
    let tmp = TempDir::new().unwrap();
    let (output, reports) = bench(&tmp, None, &[
        "probe",
        "triangle",
        "--engine",
        "nemo",
        "--ds-layout-seek",
        "galloping",
    ]);
    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(
        stderr.contains("--ds-layout-seek applies only to tree-trie or column-trie cells"),
        "{stderr}"
    );
    assert!(reports.is_empty());
}

#[cfg(all(feature = "engine-duckdb", feature = "engine-nemo"))]
mod with_both_engines {
    use super::*;

    /// The report's `criterion_groups` functions, sorted.
    fn functions(report: &serde_json::Value) -> Vec<String> {
        let mut functions: Vec<String> = report["criterion_groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|group| group["function"].as_str().unwrap().to_string())
            .collect();
        functions.sort();
        functions
    }

    fn value<'r>(report: &'r serde_json::Value, function: &str) -> Option<&'r serde_json::Value> {
        report["values"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["function"] == function)
    }

    fn by_system<'r>(
        reports: &'r [serde_json::Value], system: &str,
    ) -> Vec<&'r serde_json::Value> {
        reports
            .iter()
            .filter(|report| report["axes"]["system"] == system)
            .collect()
    }

    /// Spec Acceptance 2: three kermit cells and two engine cells in one v7 report.
    #[test]
    fn a_sweep_beside_the_engines_writes_a_v7_report() {
        let tmp = TempDir::new().unwrap();
        let (output, reports) = bench(&tmp, None, &[
            "run",
            "triangle",
            "-i",
            "all",
            "-a",
            "all",
            "--engine",
            "duckdb,nemo",
            "--metrics",
            "build",
            "iteration",
            "space",
            "materialize",
            "end-to-end",
            "--verify",
        ]);
        let stderr = stderr(&output);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(reports.len(), 5, "one query on three kermit cells and two engines");
        for report in &reports {
            assert_eq!(report["schema_version"], 7);
            assert_eq!(report["axes"]["verified"], true, "{report}");
            assert_eq!(report["axes"]["answer_rows"], 4, "{report}");
        }
        assert_eq!(by_system(&reports, "kermit").len(), 3);

        let duckdb = by_system(&reports, "duckdb");
        let nemo = by_system(&reports, "nemo");
        assert_eq!((duckdb.len(), nemo.len()), (1, 1));
        let (duckdb, nemo) = (duckdb[0], nemo[0]);
        for engine in [duckdb, nemo] {
            for absent in ["data_structure", "algorithm", "ds_layout_seek"] {
                assert!(engine["axes"].get(absent).is_none(), "{absent}: {engine}");
            }
            assert!(engine["axes"]["engine_version"].is_string(), "{engine}");
        }
        assert_eq!(duckdb["axes"]["engine_threads"], 1);
        assert!(duckdb["axes"]["engine_version"].as_str().unwrap().starts_with('v'));
        assert!(nemo["axes"].get("engine_threads").is_none());
        assert!(nemo["axes"]["engine_version"].as_str().unwrap().starts_with("nemo-physical"));

        // DuckDB streams and reports no space; Nemo stores its result, so it skips the streamed
        // metrics, and reports its trie's space.
        assert_eq!(functions(duckdb), ["build", "end_to_end", "iteration", "materialize"]);
        assert!(value(duckdb, "space").is_none());
        assert_eq!(functions(nemo), ["build", "materialize"]);
        assert_eq!(value(nemo, "space").unwrap()["estimator"], "exact");
        for (report, segment) in [(duckdb, "/duckdb-t1"), (nemo, "/nemo")] {
            for group in report["criterion_groups"].as_array().unwrap() {
                let name = group["group"].as_str().unwrap();
                assert!(name.ends_with(segment), "{name}");
            }
        }
        assert!(
            stderr.contains("note: nemo does not stream, so it skips iteration and end-to-end"),
            "{stderr}"
        );
        assert!(stderr.contains("note: duckdb-t1 reports no space"), "{stderr}");
        assert!(!stderr.contains("note: duckdb-t1 does not stream"), "{stderr}");
    }

    #[test]
    fn engines_alone_need_no_selectors_and_a_probe_times_one_build() {
        let tmp = TempDir::new().unwrap();
        let (output, reports) = bench(&tmp, None, &[
            "probe",
            "triangle",
            "--engine",
            "duckdb,nemo",
            "--materialize",
        ]);
        let stderr = stderr(&output);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().all(|report| report["kind"] == "probe"));
        let duckdb = by_system(&reports, "duckdb")[0];
        let nemo = by_system(&reports, "nemo")[0];
        for phase in ["build", "iteration", "materialize"] {
            assert_eq!(value(duckdb, phase).unwrap()["estimator"], "single", "{phase}");
        }
        assert_eq!(value(nemo, "build").unwrap()["estimator"], "single");
        assert_eq!(value(nemo, "materialize").unwrap()["estimator"], "single");
        assert!(value(nemo, "iteration").is_none(), "Nemo does not stream: {nemo}");
        assert!(
            stderr.contains("note: nemo does not stream, so it skips iteration and end-to-end"),
            "{stderr}"
        );
    }

    #[test]
    fn the_group_clash_check_covers_engine_cells() {
        let tmp = TempDir::new().unwrap();
        // `{prefix}/triangle/triangle/{segment}`: the engine segments start past byte 64.
        let prefix = "run-with-a-prefix-long-enough-to-push-the-engine-segment-past-byte-64";
        let (output, reports) = bench(&tmp, Some(prefix), &[
            "run",
            "triangle",
            "--engine",
            "duckdb,nemo",
            "-m",
            "iteration",
        ]);
        let stderr = stderr(&output);
        assert!(!output.status.success(), "{stderr}");
        assert!(stderr.contains(&format!("{prefix}/triangle/triangle/duckdb-t1")), "{stderr}");
        assert!(stderr.contains(&format!("{prefix}/triangle/triangle/nemo")), "{stderr}");
        assert!(stderr.contains("64"), "{stderr}");
        assert!(!tmp.path().join("criterion").exists(), "nothing is timed before the refusal");
        assert!(reports.is_empty());
    }

    /// `bench join` takes `-i` / `-a` as before and adds the engines beside them.
    #[test]
    fn bench_join_adds_engines_beside_its_one_kermit_cell() {
        let tmp = TempDir::new().unwrap();
        let query = tmp.path().join("triangle.dl");
        fs::write(&query, "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).").unwrap();
        let edge = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../benchmarks/data/triangle/edge.csv");
        let (output, reports) = bench(&tmp, None, &[
            "join",
            "-r",
            edge.to_str().unwrap(),
            "-q",
            query.to_str().unwrap(),
            "-i",
            "tree-trie",
            "-a",
            "leapfrog-triejoin",
            "--engine",
            "duckdb,nemo",
            "--metrics",
            "build",
            "space",
        ]);
        let stderr = stderr(&output);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(reports.len(), 3, "{stderr}");
        let systems: Vec<&str> = reports
            .iter()
            .map(|report| report["axes"]["system"].as_str().unwrap())
            .collect();
        assert_eq!(systems, ["kermit", "duckdb", "nemo"]);
        assert!(reports.iter().all(|report| report["kind"] == "join"));
    }
}

/// The memory guard on an engine that does not stream. A 400-row relation `e` gives the `star`
/// query 160000 answers of 3 variables (3840000 bytes) and the `plain` query 400.
#[cfg(feature = "engine-nemo")]
mod nemo_materialize_guard {
    use super::*;

    fn workspace() -> (TempDir, TempDir) {
        let workspace = TempDir::new().unwrap();
        let cache = TempDir::new().unwrap();
        let data = workspace.path().join("data");
        fs::create_dir_all(&data).unwrap();
        let mut rows = String::from("src,dst\n");
        for i in 0..400 {
            rows.push_str(&format!("0,{i}\n"));
        }
        fs::write(data.join("e.csv"), rows).unwrap();
        let benchmarks = workspace.path().join("benchmarks");
        fs::create_dir_all(&benchmarks).unwrap();
        fs::write(
            benchmarks.join("guard.yml"),
            r#"name: guard
description: "the materialize guard on an engine that does not stream"
relations:
  - name: e
    path: "data/e.csv"
queries:
  - name: star
    description: "160000 answers of 3 variables, with expected"
    query: "Q(A, B, C) :- e(A, B), e(A, C)."
    expected: 160000
  - name: plain
    description: "400 answers, no expected"
    query: "Q(A, B) :- e(A, B)."
"#,
        )
        .unwrap();
        (workspace, cache)
    }

    /// `kermit bench <Criterion settings> --report-json <report> run guard -q <query> --engine
    /// nemo -m build materialize` plus `extra`, in `workspace`.
    fn run(
        workspace: &TempDir, cache: &TempDir, query: &str, extra: &[&str],
    ) -> (Output, Vec<serde_json::Value>) {
        let report = workspace.path().join("report.json");
        let output = Command::new(kermit_bin())
            .env("RUST_BACKTRACE", "0")
            .env("RUST_LIB_BACKTRACE", "0")
            .env("KERMIT_WORKSPACE", workspace.path())
            .env("XDG_CACHE_HOME", cache.path())
            .env("CRITERION_HOME", workspace.path().join("criterion"))
            .current_dir(workspace.path())
            .args([
                "bench",
                "--sample-size",
                "10",
                "--measurement-time",
                "1",
                "--warm-up-time",
                "1",
                "--report-json",
            ])
            .arg(&report)
            .args(["run", "guard", "-q", query, "--engine", "nemo", "-m", "build", "materialize"])
            .args(extra)
            .output()
            .expect("failed to spawn kermit");
        let reports = fs::read_to_string(&report)
            .ok()
            .map(|text| serde_json::from_str(&text).expect("the report is a JSON array"))
            .unwrap_or_default();
        (output, reports)
    }

    fn functions(report: &serde_json::Value) -> Vec<String> {
        report["criterion_groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|group| group["function"].as_str().unwrap().to_string())
            .collect()
    }

    /// Over the limit, nothing materialises: the note and the metadata line are printed, the
    /// report has no `materialize` function and no counted `answer_rows`, because neither the
    /// preflight nor the describe step counted (Nemo can count only through `materialize`).
    #[test]
    fn a_result_over_the_limit_is_never_materialised() {
        let (workspace, cache) = workspace();
        let (output, reports) =
            run(&workspace, &cache, "star", &["--materialize-max-bytes", "1048576"]);
        let err = stderr(&output);
        assert!(output.status.success(), "{err}");
        assert!(
            err.contains(
                "note: skipping materialize for guard/star on nemo: 3840000 bytes over \
                 --materialize-max-bytes 1048576"
            ),
            "{err}"
        );
        assert!(err.contains("skipped: 3840000 bytes over 1048576"), "{err}");
        assert_eq!(reports.len(), 1, "{err}");
        assert_eq!(functions(&reports[0]), ["build"]);
        assert!(reports[0]["axes"].get("answer_rows").is_none(), "{}", reports[0]["axes"]);
    }

    /// With no `expected` the guard cannot decide: the warning is printed once and
    /// `materialize` runs, its count being the describe step's.
    #[test]
    fn without_expected_the_warning_is_printed_once_and_materialize_runs() {
        let (workspace, cache) = workspace();
        let (output, reports) = run(&workspace, &cache, "plain", &[]);
        let err = stderr(&output);
        assert!(output.status.success(), "{err}");
        let warning = "warning: nemo does not stream and guard/plain has no expected count, so \
                       materialize runs without the --materialize-max-bytes guard";
        assert_eq!(err.matches(warning).count(), 1, "{err}");
        assert!(!err.contains("skipping materialize"), "{err}");
        assert_eq!(reports.len(), 1, "{err}");
        let mut functions = functions(&reports[0]);
        functions.sort();
        assert_eq!(functions, ["build", "materialize"]);
        assert_eq!(reports[0]["axes"]["answer_rows"], 400);
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --features engine-duckdb,engine-nemo --test cli_engines`

Expected: `without_engine_the_selectors_stay_required`, `one_selector_without_the_other_is_refused`,
`engine_threads_outside_its_range_is_a_usage_error`, `a_ds_flag_with_no_kermit_cell_is_refused`,
`a_sweep_beside_the_engines_writes_a_v7_report`,
`engines_alone_need_no_selectors_and_a_probe_times_one_build`,
`the_group_clash_check_covers_engine_cells`,
`a_result_over_the_limit_is_never_materialised` and
`without_expected_the_warning_is_printed_once_and_materialize_runs` already pass (Task 10.3 wired
them; the guard is Story 3's and the preflight's `count` Task 10.1's).
`bench_join_adds_engines_beside_its_one_kermit_cell` fails: clap rejects the flag with
`error: unexpected argument '--engine' found` on `bench join`, so the test panics on
`assert!(output.status.success(), …)`.

- [ ] **Step 3: Implement**

All in `kermit/src/main.rs`.

**3a. The flag.** In `BenchSubcommand::Join`, replace

```rust
    Join {
        #[command(flatten)]
        query_args: QueryArgs,

        /// Output file for one run's results (optional)
```

with

```rust
    Join {
        #[command(flatten)]
        query_args: QueryArgs,

        /// External engines to run beside the `-i` / `-a` cell (additive:
        /// `-i` and `-a` stay required, since `QueryArgs` is shared with
        /// `kermit join`).
        #[command(flatten)]
        engines: EngineChoices,

        /// Output file for one run's results (optional)
```

(The `///` on a flattened field is ignored by clap; keep it as a note for readers.)

**3b. Preparing under the engines' policy.** Replace `prepare_query`:

```rust
fn prepare_query(args: &QueryArgs, cell: Execution) -> anyhow::Result<PreparedWorkload> {
    Workload::adhoc(args.relations.clone(), &args.query)?
        .prepare(&args.planner.instantiate(), &[cell.trie_levels()])
}
```

with:

```rust
fn prepare_query(args: &QueryArgs, cell: Execution) -> anyhow::Result<PreparedWorkload> {
    prepare_query_under(args, &[cell.trie_levels()])
}

/// [`prepare_query`] for every trie-level policy in `levels`: `bench join`
/// adds `TrieLevels::All` for its engine cells.
fn prepare_query_under(
    args: &QueryArgs, levels: &[kermit_ds::TrieLevels],
) -> anyhow::Result<PreparedWorkload> {
    Workload::adhoc(args.relations.clone(), &args.query)?.prepare(&args.planner.instantiate(), levels)
}
```

**3c. The handler.** Add `engines: EngineChoices` as the last parameter of `run_bench_join`, after
Story 3's `materialize_max_bytes: u64` (which stays, with every parameter before it), and replace
its first lines

```rust
    let selector = IndexStructureSelector::of(query_args.indexstructure);
    let choices = DsChoices::resolve(selector, &query_args.layout, &config, &build)?;
    let cell = query_cell(&query_args, choices)?;
    let workload = prepare_query(&query_args, cell)?;
```

with

```rust
    // Resolved first, so a bad `--engine` fails before anything is read.
    let engine_cells = engines.resolve()?;
    let selector = IndexStructureSelector::of(query_args.indexstructure);
    let choices = DsChoices::resolve(selector, &query_args.layout, &config, &build)?;
    let cell = query_cell(&query_args, choices)?;
    // Engines prepare under `TrieLevels::All`; the kermit cell under its own policy.
    let levels: Vec<kermit_ds::TrieLevels> = std::iter::once(cell.trie_levels())
        .chain(engine_cells.iter().map(|_| kermit_ds::TrieLevels::All))
        .collect();
    let workload = prepare_query_under(&query_args, &levels)?;
```

and replace the line

```rust
    dispatch_run_bench(cell, &workload, settings, &mut sink)?;
```

with

```rust
    dispatch_run_bench(cell, &workload, settings, &mut sink)?;
    for engine in engine_cells {
        bench::run::dispatch_sweep_cell(
            bench::run::SweepCell::Engine(engine),
            &workload,
            settings,
            &mut sink,
        )?;
    }
```

**3d. `main`.** In `main`, the `BenchSubcommand::Join { … }` arm: add `engines,` to the pattern
(after `query_args,`) and `engines` as the last argument of the `run_bench_join(…)` call, after
Story 3's `materialize.materialize_max_bytes,`:

```rust
                build,
                materialize.materialize_max_bytes,
                engines,
            )?,
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo test -p kermit --features engine-duckdb,engine-nemo --test cli_engines`

Expected: `test result: ok. 10 passed; 0 failed`. Then the story's gates:
`nix develop --command cargo fmt --all -- --check`,
`nix develop --command cargo clippy -p kermit --all-targets -- -D warnings` and
`nix develop --command cargo test -p kermit` (no features: the four feature-free tests of
`cli_engines` run, including `an_engine_without_its_feature_is_a_usage_error`),
`bash -c 'RUSTDOCFLAGS=-Dwarnings nix develop --command cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings'`
(the binary's docs are not covered by `cargo doc`; two pre-existing errors may appear, and any
other is new).

Then spec Acceptance 2:

```bash
nix develop --command cargo run -p kermit --features engine-duckdb,engine-nemo -- bench --sample-size 10 --measurement-time 1 --warm-up-time 1 run triangle -i all -a all --engine duckdb,nemo --metrics build iteration space materialize end-to-end --verify
```

Expected: exit 0; stderr carries `note: nemo does not stream, so it skips iteration and end-to-end`
and `note: duckdb-t1 reports no space`; the report `bench-runs/run-<millis>.json` holds five
objects (three with `"system": "kermit"`, one `duckdb`, one `nemo`), each with
`"schema_version": 7`, `"verified": true` and `"answer_rows": 4`.

- [ ] **Step 5: Commit**

```bash
git add -N kermit/tests/cli_engines.rs
git commit -m "feat(bench): bench join --engine and the engine CLI tests" -- kermit/src/main.rs kermit/tests/cli_engines.rs
```

## Story 11: kermit-lab reads engine rows

- **Developer:** intermediate-dev
- **Files:** `python/kermit-lab/kermit_lab/defaults.py`, `python/kermit-lab/kermit_lab/presets.py`, `python/kermit-lab/kermit_lab/analysis.py`, `python/kermit-lab/kermit_lab/__init__.py`, `python/kermit-lab/kermit_lab/frame.py`, `python/kermit-lab/kermit_lab/loader.py`, `python/kermit-lab/tests/test_loader.py`, `python/kermit-lab/tests/test_presets.py`, `python/kermit-lab/tests/test_public_api.py`, `python/kermit-lab/tests/test_engines.py`
- **Interfaces:** exposes `kermit_lab.engines` (Names contract: kermit-lab) to Story 12; uses Story 9's v7 axes
- **Verify:** `bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q'`
- **Review:** deep

kermit-lab learns the v7 rows that Stories 9 and 10 write. The loader keeps the three new axes
(`system`, `engine_version`, `engine_threads`), recognises `materialize` as a time phase, and
back-fills `system = "kermit"` on every row that lacks it, so v6 and v7 reports load together
(no `MEANING_CHANGES` entry is added: no metric changes meaning). `analysis.cell_labels` names a
row's cell (`system`, plus the thread count, for an engine; `data_structure/algorithm` for a kermit
cell), and `speedup_table` is pinned to ignore engine rows, which have no build mode. The new
preset `kl.engines()` draws only the like-for-like pairs of the spec's Summary: DuckDB against
kermit on `end_to_end`, Nemo against kermit on `build`, `materialize` and `space`. Three files
carry what the axes and the phase need to reach kermit-lab: `kermit_lab/frame.py` (an explicit
include-list of axis keys silently drops unknown ones), `kermit_lab/loader.py` (`TIME_PHASES`) and
`tests/test_loader.py` (a test that pins `TIME_PHASES` exactly). The seek axis's scope in
`presets.AXIS_PHASES` gains `materialize`, since a seek strategy moves a join that materializes,
and the six existing figures that name kermit's structures (`scaling`, `bar_time`, `bar_space`,
`tradeoff`, `dist`, `bar_queries`) draw kermit's rows only, because an engine row has no
`data_structure` and would otherwise break `render-all`; `kl.engines()` is where engines are
drawn. It relies on Story 9 having raised `SCHEMA_VERSION` to 7, or the v7 fixtures below fail to
load. No landmine marker appears in this story's code.

### Task 11.1: Keep the engine axes, the `materialize` phase and the `system` back-fill

**Files:**
- Create: `python/kermit-lab/tests/test_engines.py`
- Modify: `python/kermit-lab/kermit_lab/loader.py` (`TIME_PHASES` and the `phase_of` docstring)
- Modify: `python/kermit-lab/kermit_lab/frame.py` (`_AXIS_STR_KEYS`, `_AXIS_INT_KEYS`)
- Modify: `python/kermit-lab/kermit_lab/defaults.py` (`BINARY_AXIS_DEFAULTS`)
- Test: `python/kermit-lab/tests/test_loader.py` (the `TIME_PHASES` pin)

- [ ] **Step 1: Write the failing tests**

Create `python/kermit-lab/tests/test_engines.py`. The fixture writes one kermit report per schema
version beside one report per engine, with the helpers `conftest.py` already has:

```python
"""kermit-lab reads engine rows: the v7 axes, the `materialize` phase, the `system`
back-fill, the cell label, and the `kl.engines()` preset (external engines, Story 11)."""
from __future__ import annotations

from pathlib import Path

import pandas as pd
import pytest

from kermit_lab.defaults import BINARY_AXIS_DEFAULTS, apply_axis_defaults
from kermit_lab.frame import load
from kermit_lab.loader import TIME_PHASES, load_reports, phase_of

from .conftest import _add_values, _FunctionSpec, _write_function_dir, _write_report

NEMO_VERSION = "nemo-physical 0.10.2-dev @ 04f0962b91ed"


def _write_time_functions(
    criterion_root: Path, group: str, points: dict[str, float]
) -> list[tuple[str, str, str]]:
    """One Criterion time function per entry of ``points`` (function -> mean), all in ``group``."""
    refs: list[tuple[str, str, str]] = []
    for function, point in points.items():
        samples = [(i + 1, point * (i + 1)) for i in range(10)]
        _write_function_dir(
            criterion_root, _FunctionSpec(group, function, "time", point, samples)
        )
        refs.append((group, function, "time"))
    return refs


@pytest.fixture
def engine_tree(tmp_path: Path) -> dict:
    """Four reports of one query, `triangle`: a v6 TreeTrie report (no `system`), a v7 HashTrie
    report, a v7 DuckDB report (no `space`, no `materialize`) and a v7 Nemo report (no
    `iteration` or `end_to_end`, but a `materialize` and a `space`)."""
    criterion_root = tmp_path / "target" / "criterion"
    reports_dir = tmp_path / "reports"
    criterion_root.mkdir(parents=True)
    reports_dir.mkdir()
    base = {
        "benchmark": "triangle", "query": "triangle", "tuples": 12,
        "optimiser": "lexicographic", "allocator": "jemalloc",
    }

    tree = _write_report(
        reports_dir, "tree-v6", kind="run",
        axes={**base, "data_structure": "TreeTrie", "algorithm": "LeapfrogTriejoin"},
        metadata=[],
        groups=_write_time_functions(
            criterion_root, "run/tri/TreeTrie", {"build": 300.0, "end_to_end": 900.0}
        ),
        schema_version=6,
    )
    _add_values(tree, [{"function": "space", "metric": "space", "estimator": "exact", "value": 6400}])

    hash_trie = _write_report(
        reports_dir, "hash-v7", kind="run",
        axes={
            **base, "system": "kermit", "data_structure": "HashTrie", "algorithm": "HashTriejoin",
        },
        metadata=[],
        groups=_write_time_functions(
            criterion_root, "run/tri/HashTrie",
            {"build": 500.0, "end_to_end": 700.0, "materialize": 2000.0},
        ),
        schema_version=7,
    )
    _add_values(hash_trie, [{"function": "space", "metric": "space", "estimator": "exact", "value": 9000}])

    _write_report(
        reports_dir, "duckdb-v7", kind="run",
        axes={**base, "system": "duckdb", "engine_version": "v1.4.4", "engine_threads": 1},
        metadata=[],
        groups=_write_time_functions(
            criterion_root, "run/tri/duckdb-t1",
            {"build": 400.0, "iteration": 150.0, "end_to_end": 800.0},
        ),
        schema_version=7,
    )

    nemo = _write_report(
        reports_dir, "nemo-v7", kind="run",
        axes={**base, "system": "nemo", "engine_version": NEMO_VERSION},
        metadata=[],
        groups=_write_time_functions(
            criterion_root, "run/tri/nemo", {"build": 600.0, "materialize": 1500.0}
        ),
        schema_version=7,
    )
    _add_values(nemo, [{"function": "space", "metric": "space", "estimator": "exact", "value": 3000}])

    return {
        "criterion_root": criterion_root,
        "reports_dir": reports_dir,
        "paths": sorted(reports_dir.glob("*.json")),
    }


def test_v6_and_v7_reports_load_together(engine_tree) -> None:
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    assert sorted(set(df["schema_version"])) == [6, 7]
    assert len(load_reports(engine_tree["paths"])) == 4


def test_a_report_without_system_is_back_filled_as_kermit(engine_tree) -> None:
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    v6 = df[df["data_structure"].isin(["TreeTrie"])]
    assert not v6.empty
    assert (v6["system"] == "kermit").all()
    assert sorted(set(df["system"])) == ["duckdb", "kermit", "nemo"]

    raw = load(engine_tree["paths"], engine_tree["criterion_root"], apply_defaults=False)
    raw_v6 = raw[raw["data_structure"].isin(["TreeTrie"])]
    assert raw_v6["system"].isna().all()


def test_engine_rows_carry_system_version_and_threads(engine_tree) -> None:
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    duck = df[df["system"] == "duckdb"]
    assert set(duck["engine_version"]) == {"v1.4.4"}
    assert set(duck["engine_threads"]) == {1}
    assert str(df["engine_threads"].dtype) == "Int64"
    nemo = df[df["system"] == "nemo"]
    assert set(nemo["engine_version"]) == {NEMO_VERSION}
    assert nemo["engine_threads"].isna().all()
    # An engine row has no structure, and the structure-scoped back-fills leave it so.
    engines = df[df["system"].isin(["duckdb", "nemo"])]
    assert engines["data_structure"].isna().all()
    assert engines["algorithm"].isna().all()


def test_materialize_is_a_time_phase(engine_tree) -> None:
    assert "materialize" in TIME_PHASES
    assert phase_of("materialize") == "materialize"
    assert phase_of("TreeTrie/materialize") == "materialize"
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    nemo_time = df[(df["system"] == "nemo") & (df["metric"] == "time")]
    assert set(nemo_time["phase"]) == {"build", "materialize"}


def test_system_default_fills_rows_without_it_and_keeps_the_rest() -> None:
    df = pd.DataFrame({
        "system": [pd.NA, "duckdb", pd.NA],
        "data_structure": ["TreeTrie", pd.NA, pd.NA],
    })
    out = apply_axis_defaults(df)
    assert out["system"].tolist() == ["kermit", "duckdb", "kermit"]
    assert BINARY_AXIS_DEFAULTS["system"] == "kermit"
    # The caller's frame is unchanged.
    assert df["system"].isna().tolist() == [True, False, True]
```

Edit the pin in `python/kermit-lab/tests/test_loader.py`. Its current line is:

```python
    assert TIME_PHASES == ("build", "insertion", "copies", "iteration", "end_to_end")
```

Replace it with:

```python
    # `materialize` is the opt-in metric of report v7 (external engines).
    assert TIME_PHASES == (
        "build", "insertion", "copies", "iteration", "end_to_end", "materialize",
    )
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q tests/test_engines.py tests/test_loader.py'`

Expected: `test_a_report_without_system_is_back_filled_as_kermit` and
`test_engine_rows_carry_system_version_and_threads` fail with `KeyError: 'system'` (the loader
drops axes it does not list), `test_materialize_is_a_time_phase` fails on
`assert 'materialize' in TIME_PHASES`, `test_system_default_fills_rows_without_it_and_keeps_the_rest`
fails with `KeyError: 'system'` on `BINARY_AXIS_DEFAULTS["system"]`, and `test_loader.py`'s
`test_phase_of_recognises_build` fails on the new `TIME_PHASES` tuple.
`test_v6_and_v7_reports_load_together` passes already when Story 9 has landed.

- [ ] **Step 3: Implement**

In `python/kermit-lab/kermit_lab/loader.py`, replace the line

```python
TIME_PHASES: tuple[str, ...] = ("build", "insertion", "copies", "iteration", "end_to_end")
```

with

```python
# `materialize` is v7's opt-in metric: join, collect every full assignment, sort and count the
# distinct rows (kermit, DuckDB) or store the result trie (Nemo). It is a time phase like the rest.
TIME_PHASES: tuple[str, ...] = (
    "build", "insertion", "copies", "iteration", "end_to_end", "materialize",
)
```

and replace the two docstring lines of `phase_of`

```python
    """Return the time phase (``"build"`` / ``"insertion"`` / ``"copies"`` /
    ``"iteration"`` / ``"end_to_end"``) if ``function_id`` encodes one.
```

with

```python
    """Return the time phase (``"build"`` / ``"insertion"`` / ``"copies"`` /
    ``"iteration"`` / ``"end_to_end"`` / ``"materialize"``) if ``function_id``
    encodes one.
```

In `python/kermit-lab/kermit_lab/frame.py`, replace

```python
    "column_orders",
    "allocator",
    "query",
```

with

```python
    "column_orders",
    "allocator",
    "system",
    "engine_version",
    "query",
```

and replace

```python
    "index_rows",
    "answer_rows",
)
```

with

```python
    "index_rows",
    "answer_rows",
    "engine_threads",
)
```

In `python/kermit-lab/kermit_lab/defaults.py`, replace the registry's last entry

```python
    "allocator": "system",
}
```

with

```python
    "allocator": "system",
    # Every report before v7 came from one of kermit's own cells; the external engines
    # (DuckDB, Nemo) arrived with the `system` axis and always carry it.
    "system": "kermit",
}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q tests/test_engines.py tests/test_loader.py tests/test_defaults.py tests/test_frame.py'`

Expected: every test passes (the last line reads `<n> passed`). `test_defaults.py`'s
`test_the_registries_are_scoped` still holds: the new entry has no `ds_` or `algo_` prefix.

- [ ] **Step 5: Commit**

```bash
git add -N python/kermit-lab/tests/test_engines.py
git commit -m "feat(lab): read the v7 engine axes, the materialize phase and a system back-fill" -- python/kermit-lab/kermit_lab/loader.py python/kermit-lab/kermit_lab/frame.py python/kermit-lab/kermit_lab/defaults.py python/kermit-lab/tests/test_loader.py python/kermit-lab/tests/test_engines.py
```

### Task 11.2: Label cells, and keep engine rows out of `speedup_table`

**Files:**
- Modify: `python/kermit-lab/kermit_lab/analysis.py` (new `KERMIT_SYSTEM`, `cell_labels`; `speedup_table`'s docstring)
- Test: `python/kermit-lab/tests/test_engines.py`

- [ ] **Step 1: Write the failing tests**

In `python/kermit-lab/tests/test_engines.py`, replace the import line

```python
from kermit_lab.defaults import BINARY_AXIS_DEFAULTS, apply_axis_defaults
```

with

```python
from kermit_lab.analysis import cell_labels, speedup_table
from kermit_lab.defaults import BINARY_AXIS_DEFAULTS, apply_axis_defaults
```

and append:

```python
def test_cell_labels_name_engines_by_system_and_kermit_cells_by_structure_and_algorithm() -> None:
    df = pd.DataFrame(
        {
            "system": ["kermit", "kermit", "duckdb", "nemo", pd.NA],
            "data_structure": ["TreeTrie", "HashTrie", pd.NA, pd.NA, "ColumnTrie"],
            "algorithm": ["LeapfrogTriejoin", "HashTriejoin", pd.NA, pd.NA, pd.NA],
            "engine_threads": pd.array([pd.NA, pd.NA, 4, pd.NA, pd.NA], dtype="Int64"),
        },
        index=[10, 11, 12, 13, 14],
    )
    labels = cell_labels(df)
    assert labels.tolist() == [
        "TreeTrie/LeapfrogTriejoin", "HashTrie/HashTriejoin", "duckdb-t4", "nemo", "ColumnTrie",
    ]
    # Aligned with the frame, so `df.assign(cell=cell_labels(df))` is safe.
    assert labels.index.tolist() == [10, 11, 12, 13, 14]


def test_cell_labels_of_a_frame_without_the_v7_columns() -> None:
    df = pd.DataFrame({"data_structure": ["TreeTrie"], "algorithm": ["LeapfrogTriejoin"]})
    assert cell_labels(df).tolist() == ["TreeTrie/LeapfrogTriejoin"]
    assert cell_labels(pd.DataFrame({"metric": ["time"]})).tolist() == ["kermit"]


def test_speedup_table_ignores_engine_rows() -> None:
    """An engine row has no build mode, so it cannot be a baseline or an arm: a frame that
    mixes engines in gives the table it gives without them."""
    kermit = []
    for mode, mean in (("serial", 100.0), ("parallel:2", 50.0)):
        kermit.append({
            "kind": "ds", "metric": "time", "phase": "build", "estimator": "criterion",
            "system": "kermit", "data_structure": "TreeTrie", "relation_path": "r.parquet",
            "ds_build_mode": mode, "threads": 2 if mode == "parallel:2" else pd.NA,
            "mean_ns": mean, "mean_lo": mean * 0.99, "mean_hi": mean * 1.01,
            "source_path": f"{mode}.json", "criterion_group": mode,
            "criterion_function": "TreeTrie/build",
        })
    engine = {
        "kind": "run", "metric": "time", "phase": "build", "estimator": "criterion",
        "system": "nemo", "data_structure": pd.NA, "relation_path": pd.NA,
        "ds_build_mode": pd.NA, "threads": pd.NA,
        "mean_ns": 10.0, "mean_lo": 9.0, "mean_hi": 11.0,
        "source_path": "nemo.json", "criterion_group": "nemo",
        "criterion_function": "build",
    }
    plain = pd.DataFrame(kermit)
    plain["threads"] = plain["threads"].astype("Int64")
    mixed = pd.DataFrame([*kermit, engine])
    mixed["threads"] = mixed["threads"].astype("Int64")

    expected = speedup_table(plain)
    got = speedup_table(mixed)
    assert got["speedup"].tolist() == pytest.approx([2.0])
    assert got["speedup"].tolist() == pytest.approx(expected["speedup"].tolist())
    assert got["threads"].tolist() == expected["threads"].tolist()
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q tests/test_engines.py'`

Expected: collection fails with
`ImportError: cannot import name 'cell_labels' from 'kermit_lab.analysis'`, so every test in the
file errors.

- [ ] **Step 3: Implement**

In `python/kermit-lab/kermit_lab/analysis.py`, after the `_DERIVED_COLS` definition

```python
_DERIVED_COLS: frozenset[str] = frozenset({"threads"})
```

insert:

```python


#: The `system` axis of kermit's own cells. Every report before v7, and every v7 report of a
#: structure cell, carries it (or is back-filled with it, `defaults.BINARY_AXIS_DEFAULTS`).
KERMIT_SYSTEM = "kermit"


def _cell_label(system: object, data_structure: object, algorithm: object, threads: object) -> str:
    """One row's cell: an engine by its ``system`` (``duckdb-t4``: the engine's group segment,
    with its thread count when it reports one), a kermit cell by ``data_structure/algorithm``
    (just the structure for a ``bench ds`` row)."""
    if not pd.isna(system) and system != KERMIT_SYSTEM:
        return str(system) if pd.isna(threads) else f"{system}-t{int(threads)}"
    parts = [str(p) for p in (data_structure, algorithm) if not pd.isna(p)]
    return "/".join(parts) if parts else KERMIT_SYSTEM


def cell_labels(df: pd.DataFrame) -> pd.Series:
    """The cell label of every row of ``df``, aligned with its index: ``system`` (plus
    ``-t{engine_threads}``) for an engine row, ``data_structure/algorithm`` for a kermit row.

    The plot defaults colour and facet by it, so one figure can hold kermit's cells beside the
    engines'. A column that is absent (a frame of v6 reports has no ``system`` or
    ``engine_threads``) counts as missing in every row.
    """
    def column(name: str) -> pd.Series:
        if name in df.columns:
            return df[name]
        return pd.Series(pd.NA, index=df.index, dtype="object")

    labels = [
        _cell_label(system, data_structure, algorithm, threads)
        for system, data_structure, algorithm, threads in zip(
            column("system"), column("data_structure"), column("algorithm"),
            column("engine_threads"),
        )
    ]
    return pd.Series(labels, index=df.index, dtype="object")
```

In the same file, extend `speedup_table`'s docstring: replace its closing paragraph

```
    ``presized:N`` rows whose case has no baseline row, and leaves them out.
    """
    value_lo, value_hi = _ci_columns_for(value)
    needed = [
        "metric", "phase", "ds_build_mode", "threads", "criterion_group",
```

with

```
    ``presized:N`` rows whose case has no baseline row, and leaves them out.

    Engine rows (``system`` other than kermit, report v7) have no ``ds_build_mode``, so they
    are neither a baseline nor an arm and never enter a case; label cells with
    :func:`cell_labels` to plot them beside the structures.
    """
    value_lo, value_hi = _ci_columns_for(value)
    needed = [
        "metric", "phase", "ds_build_mode", "threads", "criterion_group",
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q tests/test_engines.py tests/test_analysis.py'`

Expected: every test passes (`<n> passed`). The speedup test passes without a change to
`speedup_table`'s code: `rows[... & df["ds_build_mode"].notna()]` already drops the engine row;
the test pins that.

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(lab): label cells by system, and pin that speedup_table ignores engine rows" -- python/kermit-lab/kermit_lab/analysis.py python/kermit-lab/tests/test_engines.py
```

### Task 11.3: `kl.engines()`, the like-for-like preset

**Files:**
- Modify: `python/kermit-lab/kermit_lab/presets.py` (imports; `ENGINE_PAIRS`, `engines`)
- Modify: `python/kermit-lab/kermit_lab/__init__.py` (export)
- Test: `python/kermit-lab/tests/test_engines.py`, `python/kermit-lab/tests/test_public_api.py`

- [ ] **Step 1: Write the failing tests**

In `python/kermit-lab/tests/test_engines.py`, replace the import block

```python
from pathlib import Path

import pandas as pd
import pytest
```

with

```python
from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
import pandas as pd
import pytest
from matplotlib.figure import Figure

import kermit_lab as kl
from kermit_lab import presets
from kermit_lab.plots_errors import InsufficientAxesError
```

and append:

```python
def _panels(fig: Figure) -> dict[str, list[float]]:
    """Each visible panel's title -> its sorted bar heights."""
    return {
        ax.get_title(): sorted(p.get_height() for p in ax.patches)
        for ax in fig.axes
        if ax.get_visible()
    }


def test_engines_draws_only_the_like_for_like_pairs(engine_tree) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    fig = kl.engines(df)
    assert isinstance(fig, Figure)
    panels = _panels(fig)
    # DuckDB against kermit on end_to_end only: its iteration (150) is not drawn.
    assert panels["duckdb vs kermit: end_to_end"] == [700.0, 800.0, 900.0]
    # Nemo against kermit on build, materialize and space. The v6 TreeTrie report has no
    # materialize function, so that panel holds the HashTrie cell and Nemo.
    assert panels["nemo vs kermit: build"] == [300.0, 500.0, 600.0]
    assert panels["nemo vs kermit: materialize"] == [1500.0, 2000.0]
    assert panels["nemo vs kermit: space"] == [3000.0, 6400.0, 9000.0]
    assert len(panels) == 4
    plt.close("all")


def test_engines_labels_cells_by_system_and_by_structure_and_algorithm(engine_tree) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    fig = kl.engines(df)
    ax = next(a for a in fig.axes if a.get_title() == "nemo vs kermit: build")
    labels = set(ax.get_legend_handles_labels()[1])
    assert labels == {"HashTrie/HashTriejoin", "TreeTrie/LeapfrogTriejoin", "nemo"}
    ax = next(a for a in fig.axes if a.get_title() == "duckdb vs kermit: end_to_end")
    assert "duckdb-t1" in set(ax.get_legend_handles_labels()[1])
    plt.close("all")


def test_engines_skips_the_pairs_of_an_engine_that_is_absent(engine_tree) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    only_duckdb = df[~df["system"].isin(["nemo"])]
    assert list(_panels(kl.engines(only_duckdb))) == ["duckdb vs kermit: end_to_end"]
    plt.close("all")


def test_engines_needs_an_engine_row(fixture_tree) -> None:
    plt.close("all")
    kermit_only = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    with pytest.raises(InsufficientAxesError, match="no engine rows"):
        kl.engines(kermit_only)
    with pytest.raises(InsufficientAxesError, match="'system' column"):
        kl.engines(pd.DataFrame({"metric": ["time"]}))
    plt.close("all")


def test_engines_saves_to_out(engine_tree, tmp_path: Path) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    out = tmp_path / "engines.pdf"
    kl.engines(df, out=out)
    assert out.is_file() and out.stat().st_size > 0
    plt.close("all")


def test_engines_is_public() -> None:
    assert kl.engines is presets.engines
    assert "engines" in kl.__all__
```

In `python/kermit-lab/tests/test_public_api.py`, replace

```python
                 "speedup", "speedup_table"):
```

with

```python
                 "speedup", "speedup_table", "engines"):
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q tests/test_engines.py tests/test_public_api.py'`

Expected: the six new tests fail with
`AttributeError: module 'kermit_lab' has no attribute 'engines'` (the final one fails on
`presets.engines`); the earlier tests still pass. `tests/test_public_api.py`'s
`test_public_names` fails on `AssertionError: engines` once Step 1 has edited it.

- [ ] **Step 3: Implement**

In `python/kermit-lab/kermit_lab/presets.py`, replace the imports

```python
from .analysis import SPEEDUP_MEASURES, baseline_build_mode, speedup_table
from .facet import finish, make_grid
from .frame import build_of
from .loader import TIME_PHASES
```

with

```python
from .analysis import (
    KERMIT_SYSTEM,
    SPEEDUP_MEASURES,
    baseline_build_mode,
    cell_labels,
    speedup_table,
)
from .encoding import AestheticMap, resolve_scalar, select
from .facet import finish, make_grid
from .frame import build_of
from .geoms import draw_bar
from .loader import TIME_PHASES
```

and append at the end of the file:

```python


#: The like-for-like pairs (the external engines' design, Summary), as ``(engine, y, phase)``:
#: an engine is drawn against kermit's own cells only where both do the same work. DuckDB's
#: index work (its hash tables) sits in its ``iteration`` while kermit's sits in ``build``, so
#: only ``end_to_end`` is the same work; Nemo builds its tries in ``build`` and stores its
#: result in ``materialize``, as kermit's structures and their ``materialize`` do, and its
#: exact ``space`` is a trie's, as theirs are.
ENGINE_PAIRS: tuple[tuple[str, str, str], ...] = (
    ("duckdb", "time", "end_to_end"),
    ("nemo", "time", "build"),
    ("nemo", "time", "materialize"),
    ("nemo", "space", "space"),
)


def engines(df: pd.DataFrame, *, out: Optional[Path] = None) -> Figure:
    """Each engine against kermit's cells on the pairs that compare like with like
    (:data:`ENGINE_PAIRS`): DuckDB on ``end_to_end``; Nemo on ``build``, ``materialize`` and
    ``space``. One panel per pair that has engine rows, bars per query coloured by cell
    (:func:`~kermit_lab.analysis.cell_labels`), Criterion estimates for time and exact bytes for
    space. A row without ``system`` counts as kermit's.

    Raises :class:`InsufficientAxesError` when ``df`` has no ``system`` column (load v7
    reports) or no engine row on any pair.
    """
    if "system" not in df.columns:
        raise InsufficientAxesError(
            "engines needs the 'system' column that report v7 adds; load v7 reports"
        )
    labelled = df.assign(
        cell=cell_labels(df),
        system=df["system"].mask(df["system"].isna(), KERMIT_SYSTEM),
    )
    panels = []
    for engine, y, phase in ENGINE_PAIRS:
        pair = labelled[labelled["system"].isin([engine, KERMIT_SYSTEM])]
        if not (pair["system"] == engine).any():
            continue
        amap = AestheticMap(kind="bar", x="query", y=y, colour="cell", phase=phase)
        try:
            rows = select(pair, amap)
        except InsufficientAxesError:
            continue
        if not (rows["system"] == engine).any():
            continue
        title = f"{engine} vs kermit: {phase}"
        ylabel = "space (bytes)" if y == "space" else "time (ns)"
        panels.append((title, ylabel, resolve_scalar(rows, amap)))
    if not panels:
        raise InsufficientAxesError(
            "no engine rows on a like-for-like pair (duckdb on end_to_end; nemo on build, "
            "materialize and space)"
        )

    apply_style()
    fig, axes = make_grid(len(panels))
    for ax, (title, ylabel, series) in zip(axes, panels):
        draw_bar(ax, series, xlabel="query", ylabel=ylabel)
        ax.set_title(title)
    finish(fig, axes, title="Engines against kermit, like-for-like pairs", out=out)
    return fig
```

In `python/kermit-lab/kermit_lab/__init__.py`, replace

```python
    bar_time,
    dist,
    scaling,
```

(inside the `from .presets import (` block) with

```python
    bar_time,
    dist,
    engines,
    scaling,
```

and in `__all__` replace

```python
    "dist",
    "linestyle_for",
```

with

```python
    "dist",
    "engines",
    "linestyle_for",
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q'`

Expected: the whole kermit-lab suite passes (`<n> passed`, plus the contract test skipped when
`KERMIT_BIN` is unset), including all of `tests/test_engines.py`.

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(lab): kl.engines, the like-for-like preset for DuckDB and Nemo" -- python/kermit-lab/kermit_lab/presets.py python/kermit-lab/kermit_lab/__init__.py python/kermit-lab/tests/test_engines.py python/kermit-lab/tests/test_public_api.py
```

### Task 11.4: A seek strategy moves `materialize` too

**Files:**
- Modify: `python/kermit-lab/kermit_lab/presets.py:90` (the `ds_layout_seek` entry of `AXIS_PHASES`)
- Test: `python/kermit-lab/tests/test_presets.py`

- [ ] **Step 1: Write the failing test**

In `python/kermit-lab/tests/test_presets.py`, after

```python
    with pytest.raises(InsufficientAxesError, match="no time rows"):
        presets.ablation(df, axis="ds_layout_seek", phase="end_to_end")
```

insert:

```python


def test_ablation_draws_seek_on_materialize(tmp_path) -> None:
    """`materialize` runs the join, which seeks, so the seek ablation is drawn on it."""
    from .conftest import _FunctionSpec, _write_function_dir, _write_report

    criterion_root = tmp_path / "target" / "criterion"
    reports_dir = tmp_path / "reports"
    criterion_root.mkdir(parents=True)
    reports_dir.mkdir()
    paths = []
    for seek, point in (("binary", 2000.0), ("galloping", 1500.0)):
        function = f"TreeTrie/{seek}/materialize"
        samples = [(i + 1, point * (i + 1)) for i in range(10)]
        _write_function_dir(
            criterion_root, _FunctionSpec("run", function, "time", point, samples)
        )
        axes = {
            "benchmark": "triangle",
            "query": "triangle",
            "data_structure": "TreeTrie",
            "algorithm": "LeapfrogTriejoin",
            "tuples": 100,
            "ds_layout_seek": seek,
        }
        paths.append(
            _write_report(
                reports_dir, f"run-TreeTrie-{seek}", kind="run", axes=axes, metadata=[],
                groups=[("run", function, "time")], schema_version=7,
            )
        )
    df = kl.load(paths, criterion_root=criterion_root)
    assert set(df["phase"].dropna()) == {"materialize"}
    assert isinstance(
        presets.ablation(df, axis="ds_layout_seek", phase="materialize"), Figure
    )
    plt.close("all")
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q tests/test_presets.py::test_ablation_draws_seek_on_materialize'`

Expected: FAIL with `InsufficientAxesError: ds_layout_seek cannot affect phase 'materialize': it
changes only how a built trie is searched; plot it on 'end_to_end' or 'iteration'`.

- [ ] **Step 3: Implement**

In `python/kermit-lab/kermit_lab/presets.py`, replace

```python
    # A seek strategy changes how a built trie is searched; no build calls
    # seek (#80).
    "ds_layout_seek": AxisScope(
        frozenset({"iteration", "end_to_end"}),
        "it changes only how a built trie is searched",
    ),
```

with

```python
    # A seek strategy changes how a built trie is searched; no build calls
    # seek (#80). A `materialize` runs the join, so it searches too.
    "ds_layout_seek": AxisScope(
        frozenset({"iteration", "end_to_end", "materialize"}),
        "it changes only how a built trie is searched, as a join does",
    ),
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q tests/test_presets.py'`

Expected: every test passes (`<n> passed`), `test_ablation_refuses_seek_outside_search_phases`
included: its `match="searched"` still holds for `insertion`.

- [ ] **Step 5: Commit**

```bash
git commit -m "fix(lab): the seek ablation is drawn on materialize too" -- python/kermit-lab/kermit_lab/presets.py python/kermit-lab/tests/test_presets.py
```

### Task 11.5: Engine rows in the other entry points

**Files:**
- Modify: `python/kermit-lab/kermit_lab/presets.py` (`_kermit_rows`; the six figures that name kermit's structures)
- Test: `python/kermit-lab/tests/test_engines.py`

An engine row has no `data_structure` or `algorithm`. `summary` (a `pivot_table`) and `compare`
(masks on `group_by`, merges on the other columns) take such a frame without change, and the tests
below pin that. `render-all` does not: `bar_space` and `bar_time` put `data_structure` on the x
axis, and `geoms._categories` tests each value with `x not in cats`, which raises `TypeError`
("boolean value of NA is ambiguous") on a `pd.NA` beside strings. The six figures that name
kermit's structures therefore draw kermit's rows only (a row without `system` is kermit's);
`kl.engines()` is where engines are drawn.

- [ ] **Step 1: Write the failing tests**

In `python/kermit-lab/tests/test_engines.py`, replace the two import lines

```python
from kermit_lab.analysis import cell_labels, speedup_table
```

and

```python
from kermit_lab import presets
from kermit_lab.plots_errors import InsufficientAxesError
```

with

```python
from kermit_lab.analysis import cell_labels, compare, speedup_table, summary
```

and

```python
from kermit_lab import presets
from kermit_lab.drivers.render_all import render_all
from kermit_lab.plots_errors import InsufficientAxesError
```

and append:

```python
def test_summary_pivots_engine_rows_by_system(engine_tree) -> None:
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    table = summary(df[df["metric"] == "time"], rows="system", cols="phase")
    assert list(table.index) == ["duckdb", "kermit", "nemo"]
    assert table.loc["duckdb", "iteration"] == 150.0
    assert table.loc["nemo", "materialize"] == 1500.0
    assert table.loc["kermit", "materialize"] == 2000.0


def test_compare_pairs_kermit_cells_and_ignores_engine_rows() -> None:
    df = pd.DataFrame(
        {
            "system": ["kermit", "kermit", "duckdb"],
            "data_structure": ["TreeTrie", "HashTrie", pd.NA],
            "query": ["triangle", "triangle", "triangle"],
            "mean_ns": [300.0, 150.0, 100.0],
            "mean_lo": [290.0, 140.0, 90.0],
            "mean_hi": [310.0, 160.0, 110.0],
        }
    )
    out = compare(df, baseline="TreeTrie", target="HashTrie")
    assert len(out) == 1
    assert out["speedup"].iloc[0] == pytest.approx(2.0)
    assert out["system"].tolist() == ["kermit"]


def _bar_heights(fig: Figure) -> list[float]:
    return sorted(p.get_height() for ax in fig.axes if ax.get_visible() for p in ax.patches)


def test_the_structure_figures_draw_kermit_rows_only(engine_tree) -> None:
    plt.close("all")
    df = load(engine_tree["paths"], engine_tree["criterion_root"])
    # Nemo's 3000 bytes are not drawn: only the TreeTrie (v6, no `system`) and HashTrie rows are.
    assert _bar_heights(presets.bar_space(df)) == [6400.0, 9000.0]
    # DuckDB's end_to_end (800) is not drawn either.
    fig = presets.bar_time(df, query="triangle", phase="end_to_end")
    assert _bar_heights(fig) == [700.0, 900.0]
    plt.close("all")


def test_render_all_takes_a_frame_holding_engine_rows(engine_tree, tmp_path: Path) -> None:
    plt.close("all")
    reports = load_reports(engine_tree["paths"])
    render_all(reports, tmp_path, engine_tree["criterion_root"], "pdf", phase="end_to_end")
    names = {p.name for p in tmp_path.iterdir()}
    assert {"bar-space.pdf", "bar-time-triangle.pdf"} <= names
    assert plt.get_fignums() == []
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q tests/test_engines.py'`

Expected: `test_summary_pivots_engine_rows_by_system` and
`test_compare_pairs_kermit_cells_and_ignores_engine_rows` pass already (they pin behaviour).
`test_the_structure_figures_draw_kermit_rows_only` fails with `TypeError: boolean value of NA is
ambiguous` (the engine row's `data_structure`), and `test_render_all_takes_a_frame_holding_engine_rows`
fails with the same `TypeError`.

- [ ] **Step 3: Implement**

In `python/kermit-lab/kermit_lab/presets.py`, replace

```python
from .styles import apply as apply_style
```

with

```python
from .styles import apply as apply_style


def _kermit_rows(df: pd.DataFrame) -> pd.DataFrame:
    """``df`` without the external engines' rows. An engine row has no ``data_structure`` or
    ``algorithm``, which the figures below put on their axes and in their legends; they draw
    kermit's cells, and :func:`engines` draws the engines beside them. A row without ``system``
    (a v6 report, or a frame loaded without defaults) is kermit's."""
    if "system" not in df.columns:
        return df
    is_engine = df["system"].notna() & (df["system"] != KERMIT_SYSTEM)
    return df[~is_engine]
```

Then replace each of these six lines, which open the figures' `plot` calls:

```python
    return plot(df, kind="line", x="tuples", y="time", colour="data_structure",
```

with

```python
    return plot(_kermit_rows(df), kind="line", x="tuples", y="time", colour="data_structure",
```

```python
    return plot(df, kind="bar", x="data_structure", y="time", colour="data_structure",
```

with

```python
    return plot(_kermit_rows(df), kind="bar", x="data_structure", y="time",
                colour="data_structure",
```

```python
    return plot(df, kind="bar", x="data_structure", y="space", colour="data_structure",
```

with

```python
    return plot(_kermit_rows(df), kind="bar", x="data_structure", y="space",
                colour="data_structure",
```

```python
    return plot(df, kind="scatter", x="space", y="time", colour="data_structure",
```

with

```python
    return plot(_kermit_rows(df), kind="scatter", x="space", y="time", colour="data_structure",
```

```python
    return plot(df, kind="violin", x="data_structure", y="time", style="algorithm",
```

with

```python
    return plot(_kermit_rows(df), kind="violin", x="data_structure", y="time",
                style="algorithm",
```

```python
    return plot(df, kind="bar", x="query", y="time", colour="data_structure",
```

with

```python
    return plot(_kermit_rows(df), kind="bar", x="query", y="time", colour="data_structure",
```

(`ablation` already drops the rows that lack its axis, which every engine row does.)

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q'`

Expected: the whole kermit-lab suite passes (`<n> passed`, plus the contract test skipped when
`KERMIT_BIN` is unset).

- [ ] **Step 5: Commit**

```bash
git commit -m "fix(lab): the structure figures and render-all take frames holding engine rows" -- python/kermit-lab/kermit_lab/presets.py python/kermit-lab/tests/test_engines.py
```

## Story 12: Docs, and the acceptance probes

- **Developer:** intermediate-dev
- **Files:** `docs/engines/TEMPLATE.md`, `docs/engines/duckdb.md`, `docs/engines/nemo.md`, `CLAUDE.md`, `ARCHITECTURE.md`, `BENCHMARKING.md`, `docs/specs/bench-report-schema.md`, `README.md`, `USAGE.md`, `kermit/README.md`, `python/kermit-lab/README.md`
- **Interfaces:** exposes nothing; uses every earlier story
- **Verify:** `bash -c 'test -f docs/engines/TEMPLATE.md && test -f docs/engines/duckdb.md && test -f docs/engines/nemo.md && grep -q "Adding an external engine" CLAUDE.md && grep -q "engine_version" docs/specs/bench-report-schema.md && cargo build --release -p kermit --features engine-duckdb,engine-nemo && for b in "triangle" "lubm-reference -q q1" "watdiv-stress-100-test-1 -q q0001" "oxford-uniform-s3 -q triangle"; do target/release/kermit bench probe $b -i all -a all --engine duckdb,nemo --verify --materialize || exit 1; done && for b in "sw-snap-amazon-pre -q triangle" "sw-h2o-pre -q q1" "sw-job-1-pre -q 1a"; do XDG_CACHE_HOME=/tb/Source/Academia/kermit-bench-runs/full-sweep-2026-10-24/cache target/release/kermit bench probe $b -i all -a all --engine duckdb,nemo --verify --materialize || exit 1; done'`
- **Review:** light (docs, and probes that change nothing)

This story writes the documentation of the external-engines change and then runs its acceptance
probes. It writes the engine pages (a template, then DuckDB and Nemo, each with its phase mapping,
fairness caveats, version pin and how to move it), the report schema's v7 catalogue, the CLAUDE.md
recipe "Adding an external engine" with the module and its gotchas, ARCHITECTURE.md's
`kermit::engine` section, and BENCHMARKING.md's like-for-like pairs, then corrects the text that the engines, the `materialize`
metric and v7 leave stale in CLAUDE.md, ARCHITECTURE.md, BENCHMARKING.md, README.md, USAGE.md,
`kermit/README.md` and kermit-lab's README. The last task builds the release
binary with both engine features and runs spec Acceptance 3's seven probes: the four on the shared
cache and the three on the 2026-10-24 sweep's private cache, each with every cell (three kermit
cells, DuckDB, Nemo), `--verify` and `--materialize`. The probes only read caches (no `--force`, no
`bench gen`). A probe that exits non-zero stops the task and is reported, not worked around. The
outcome of each is recorded as a table on the engine pages, generated from the probes' own reports
so that no number is typed by hand. The code is documentation and shell; no landmine marker
appears in it. The docs describe the engines as Stories 6 to 10 define them (the names contract),
and the probes need the release build to finish, which compiles `nemo-physical`: build in the dev
shell, never `engine-duckdb-bundled`.

### Task 12.1: The engine pages

**Files:**
- Create: `docs/engines/TEMPLATE.md`
- Create: `docs/engines/duckdb.md`
- Create: `docs/engines/nemo.md`

- [ ] **Step 1: Write the failing check**

The check is the existence of the three pages and of the sections the template requires:

```bash
test -f docs/engines/TEMPLATE.md && test -f docs/engines/duckdb.md && test -f docs/engines/nemo.md \
  && for f in docs/engines/duckdb.md docs/engines/nemo.md; do
       for h in '## Phase mapping' '## Translation' '## Fairness caveats' '## Version pin' '## Settings and limits' '## Tests'; do
         grep -q "^$h" "$f" || { echo "$f lacks $h"; exit 1; }
       done
     done
```

- [ ] **Step 2: Run it to see it fail**

Run: `bash -c 'test -f docs/engines/TEMPLATE.md'; echo "exit $?"`

Expected: `exit 1` (the directory does not exist yet).

- [ ] **Step 3: Implement**

Create `docs/engines/TEMPLATE.md`:

````markdown
# `<EngineName>`

> **Status:** [stable | experimental] · **CLI:** `--engine <name>` · **Feature:** `engine-<name>` · **Implementation:** [`kermit::engine::<module>`](../../kermit/src/engine/<module>.rs)

One paragraph: what the engine is, what it is compared against and why (the thesis question it serves,
RQ1.4: a bounded sanity baseline against an established peer engine, not a head-to-head claim).

## Phase mapping

How the engine fills kermit's phases. Every engine answers the full conjunctive query (every
variable of the executed query), from the same `Prepared` the structure cells run.

| Phase | What the engine does | Timed | Notes |
|---|---|---|---|
| `load` | … | no | … |
| `build` | … | yes | what is copied, and how many times (kermit's cells copy once) |
| `iteration` | … or "not offered" | yes | … |
| `materialize` | … | yes | … |
| `space` | … or "not reported" | exact | … |

State the `Engine` constants: `NAME` (the `system` axis), `STREAMS`, `KEEPS_DUPLICATES`,
`MATERIALIZE_MUTATES`, `HAS_SPACE`.

## Translation

How `Prepared` (the executed query over index ids `#k`, each index's `IndexSpec`, the plan) becomes
the engine's input: one statement or plan, with a worked example for the triangle under the plan
`X, Y, Z`.

## Fairness caveats

Where the engine's work sits differently from kermit's, so that a reader knows which pairs compare
like with like and which do not (see `BENCHMARKING.md`, "Comparing against external engines"):
- which phase holds the engine's index work;
- set or bag semantics, and what that does to counts on inputs with duplicate rows;
- who chooses the join order;
- threads, memory limits and anything the engine does outside kermit's timers.

## Version pin

The version that runs, where it is pinned, what `engine_version` records, and **how to move the
pin** (every file and constant to change, and the commands that prove the move).

## Settings and limits

The engine's `Settings`, its CLI flags and their bounds, and the memory or disk limits kermit sets
on it.

## Tests

The engine's cells in `kermit/tests/engine_oracle.rs`, its unit tests, and the CI job that runs
them.

## Acceptance probes

Appended by the story that runs spec Acceptance 3: one table of the engine's single-run times per
probed query.

## See also

- Sibling pages in `docs/engines/`.
- The design: [`docs/specs/2026-11-02-external-engines-design.md`](../specs/2026-11-02-external-engines-design.md).
- The report schema's v7 axes: [`docs/specs/bench-report-schema.md`](../specs/bench-report-schema.md).
````

Create `docs/engines/duckdb.md`:

````markdown
# `DuckDB`

> **Status:** experimental · **CLI:** `--engine duckdb` · **Feature:** `engine-duckdb` (dev shell), `engine-duckdb-bundled` (CI) · **Implementation:** [`kermit::engine::duckdb`](../../kermit/src/engine/duckdb.rs)

DuckDB, run in process through the `duckdb` crate, is the production binary-join engine kermit's
structures are compared against. It is already kermit's JOB oracle (a JOB query's `expected` is its
`COUNT(*)`), and the engine that Free Join and the Linear-Chained work measure against. It runs
exactly the query kermit's cells run: it reads `Prepared` after `prepare` has validated, rewritten,
extracted accesses and planned, and never parses Datalog. The thesis question it serves is RQ1.4: a
bounded sanity baseline against an established peer, not a head-to-head claim.

## Phase mapping

| Phase | What DuckDB does | Timed | Notes |
|---|---|---|---|
| `load` | An in-memory database, `PRAGMA threads = N`, and one table `rel_<name>` (columns `c0…`, `UBIGINT`) per relation the query reads, appended from the catalog's flat tuples | no | |
| `build` | One `CREATE TEMP TABLE` per `IndexSpec`: the spec's filtered, projected rows from the relation table (one copy; no index exists). Then the join statement is prepared | yes | The preparation (planning, well under a millisecond on these queries) is the last step of `build`, so `iteration` excludes it and each `end_to_end` sample includes it once (decision 11) |
| `iteration` | The prepared join over the spec tables, streamed as Arrow batches whose rows are counted | yes | DuckDB builds its hash tables here, inside the query |
| `materialize` | The same statement, each batch's columns copied into kermit `Tuples` (row-major), then the sort and distinct count that kermit's own cells use (`kermit::db::materialize_count`) | yes, opt-in | Every sample builds nothing new: `MATERIALIZE_MUTATES` is false |
| `space` | Not reported | | DuckDB keeps no index; its hash tables exist only inside a query. The note `{label} reports no space` is printed |

`Engine` constants: `NAME` is `"duckdb"`, `STREAMS` true, `KEEPS_DUPLICATES` true,
`MATERIALIZE_MUTATES` false, `HAS_SPACE` false.

## Translation

The shared SQL translator (`kermit/src/engine/sql.rs`) reads the executed query and its specs, whose
shape `extract_accesses` and `plan_indexes` guarantee: every atom names an index id, has one variable
per column in index-column order, repeats no variable and holds no constant. Each spec `#k` becomes
a table `idx_k` followed by a per-build suffix; each atom becomes an alias `a{i}` over its spec's
table, with column `c{j}` bound to the atom's `j`-th variable; every variable is selected once, at
its first binding, in canonical order; every later binding is an equality with the first; an atom
that shares no variable with the atoms before it joins by `CROSS JOIN`; identifiers are
double-quoted. For the triangle under the plan `X, Y, Z`, where all three atoms read the merged
spec `#0`:

```sql
SELECT a0."c0" AS "X", a0."c1" AS "Y", a1."c1" AS "Z"
FROM "idx_0" AS a0
JOIN "idx_0" AS a1 ON a1."c0" = a0."c1"
JOIN "idx_0" AS a2 ON a2."c0" = a0."c0" AND a2."c1" = a1."c1";
```

A spec's build statement selects the spec's columns from the base relation under its `Filter`
(constants as `c{c} = v`, equalities as `c{a} = c{b}`). For a ternary `r` read as `r(Y, X, c42)`
under the plan `X, Y`:
`CREATE TEMP TABLE "idx_0_<n>" AS SELECT "c1", "c0" FROM "rel_r" WHERE "c2" = 42`.

## Fairness caveats

- **Compare DuckDB with kermit on `end_to_end` only.** DuckDB's index work (its hash tables) sits
  in its `iteration`, while kermit's sits in `build`, so only the sum is the same work. `kl.engines()`
  draws exactly that pair.
- **DuckDB chooses its own join order.** The statement fixes the query, not the order: kermit's
  variable order is the plan of its own cells, and DuckDB's optimiser plans the joins itself.
- **Bag semantics.** DuckDB counts duplicate rows, as `HashTrie` does (`KEEPS_DUPLICATES`), so on
  duplicate-free inputs its count equals kermit's and the benchmark's `expected`.
- **One thread by default** (`PRAGMA threads = 1`), the same parallelism as kermit's joins and serial
  builds. `--engine-threads N` raises it; reports carry `engine_threads`.
- **Memory.** Every connection sets `memory_limit = '8GB'`, under the sweep's 14 GiB cap (DuckDB's
  default is 80% of RAM), and a private spill directory.
- **Probes are single cold runs.** A probe's `build`, `iteration` and `materialize` are one run each:
  compare them with other single runs, never with a Criterion estimate.

## Version pin

- The crate is `duckdb = "~1.4"` in `kermit/Cargo.toml`: DuckDB 1.4.x, matching the flake's DuckDB
  1.4.4, which computes JOB's `expected`. The newest crate line (1.20000.x) is DuckDB 2.0.
- `engine-duckdb` links the system `libduckdb`, found through `DUCKDB_LIB_DIR` and
  `DUCKDB_INCLUDE_DIR`, which the dev shell exports from Nix's `duckdb.lib` and `duckdb.dev`
  outputs (`flake.nix`). A dev-shell build compiles no C++, and `kermit/build.rs` adds an rpath to
  `DUCKDB_LIB_DIR` so a binary built in the shell also runs outside it. Without `DUCKDB_LIB_DIR` and
  without bundling, the build fails with `engine-duckdb needs DUCKDB_LIB_DIR (the nix dev shell sets
  it) or the engine-duckdb-bundled feature`.
- `engine-duckdb-bundled` adds `duckdb/bundled`, which compiles DuckDB's C++ amalgamation. It is for
  CI (no Nix). **Never build it on a host that cannot spare several GB for the compiler process.**
- Every report's `engine_version` records the library that actually ran (`pragma_version()`), so a
  report says which DuckDB it measured whatever the pin.

To move the pin:
1. Change the `duckdb` requirement in `kermit/Cargo.toml` (and `cargo update -p duckdb`, which
   rewrites `Cargo.lock`), and `[profile.dev.package.libduckdb-sys]` in the root `Cargo.toml` if the
   `-sys` crate is renamed.
2. Make the dev shell's `duckdb` the same minor line: `nix flake update nixpkgs` moves it, and
   `duckdb --version` in `nix develop` shows it. The JOB generator drives the same library
   (`kermit-generate/src/job/duckdb.rs`), so a move that changes any JOB `expected` count is a
   regeneration question for the JOB caches, not only for this page.
3. Run `cargo test -p kermit --features engine-duckdb --test engine_oracle` and the acceptance
   probes below, and record the new version here.

## Settings and limits

- `DuckDbSettings { threads }`: `--engine-threads N`, default 1, range `1..=available_parallelism()`;
  outside it the error reads `--engine-threads must be between 1 and {available_parallelism}`.
- `memory_limit = '8GB'` and the private spill directory `kermit-duckdb-<pid>-<n>` under the system
  temporary directory, removed when the connection drops. DuckDB deletes every spill file in its
  directory on exit, so two connections sharing one would corrupt each other's spills; the JOB
  generator guards against the same hazard.
- kermit sets no timeout of its own; the sweep harness's per-step timeout and memory cap apply. An
  engine that fails ends its cell with an error, and nothing is retried.

## Tests

- `kermit/tests/engine_oracle.rs` checks DuckDB's counts and contents against the nested-loop oracle
  over the shared query shapes (`kermit/tests/common/shapes.rs`), on bag semantics.
- Unit tests in `kermit/src/engine/sql.rs` pin the translator's statements.
- The `engines` job of `.github/workflows/pr.yml` runs both (bundled, since CI has no Nix). The
  profile's gates build no cargo features, so a story that touches the engine names
  `--features engine-duckdb` in its own Verify line.

## See also

- [`nemo.md`](nemo.md) and [`TEMPLATE.md`](TEMPLATE.md).
- The design: [`docs/specs/2026-11-02-external-engines-design.md`](../specs/2026-11-02-external-engines-design.md).
- The report schema's v7 axes: [`docs/specs/bench-report-schema.md`](../specs/bench-report-schema.md).
````

Create `docs/engines/nemo.md`:

````markdown
# `Nemo`

> **Status:** experimental · **CLI:** `--engine nemo` · **Feature:** `engine-nemo` · **Implementation:** [`kermit::engine::nemo`](../../kermit/src/engine/nemo.rs)

Nemo (`knowsys/nemo`, Apache-2.0) is a rule engine whose join is a leapfrog triejoin over
column-oriented tries, the same family as kermit's `ColumnTrie` under `LeapfrogTriejoin`. It is the
peer engine of thesis RQ1.4: a bounded sanity baseline with explicit threats to validity, not a
head-to-head claim. kermit runs it in process through the public API of `nemo-physical` at a pinned
git revision, with no fork, on the same `Prepared` query the structure cells run.

## Phase mapping

| Phase | What Nemo does | Timed | Notes |
|---|---|---|---|
| `load` | Shared `Arc` handles on the catalog's tuples of each relation the query reads | no | |
| `build` | A fresh `DatabaseInstance`; per `IndexSpec`, `register_table` and `add_source_external` with a `ViewProvider` that streams the spec's matching rows, output columns only, as integer values; then `load_trie_into_memory` per spec, which sorts, deduplicates and builds the trie. The join plan is built here, in kermit's variable order | yes | One copy of the rows, plus Nemo's sort, deduplication and trie: the same kind of work as `ColumnTrie`'s build |
| `iteration` | Not offered | | Upstream's streaming join is private; the public `execute_plan` stores its result as a deduplicated trie. The note `{label} does not stream, so it skips iteration and end-to-end` is printed |
| `materialize` | `execute_plan`, then `count_rows_in_memory` of the stored result | yes, opt-in | Nemo stores the result, so every sample runs on a fresh build in untimed setup (`MATERIALIZE_MUTATES`), as a lazy `HashTrie` does; a second `materialize` on one build panics |
| `space` | The sum of `table_size_bytes` over the spec tables | exact | Read in the describe step, before anything runs |

`Engine` constants: `NAME` is `"nemo"`, `STREAMS` false, `KEEPS_DUPLICATES` false,
`MATERIALIZE_MUTATES` true, `HAS_SPACE` true.

## Translation

Nemo's translator (`kermit/src/engine/nemo.rs`) builds one public `ExecutionPlan`:
- one `OperationTable` of result markers, pushed in **kermit's plan order** (`Prepared::plan()`), so
  Nemo runs kermit's variable order and not one of its own;
- one `fetch_table(markers, id)` per atom, over its spec's Nemo table (merged specs are one Nemo
  table, fetched by several atoms);
- `join_empty(result_markers)`, then `add_subnode` per atom, then `write_permanent`.

The result's columns follow the plan order, so `Engine::rows` (used by tests only) permutes them into
canonical order.

## Fairness caveats

- **Compare Nemo with kermit on `build`, `materialize` and `space`.** Nemo has no streamed
  `iteration`, so it is never compared on `iteration` or `end_to_end`; `kl.engines()` draws exactly
  those three pairs.
- **Set semantics.** Nemo's loader deduplicates the input and its result is a deduplicated trie, so
  its count equals the bag count only on duplicate-free inputs. kermit's `materialize` is defined to
  match: it collects the full assignments and counts distinct rows.
- **`materialize` is not a streamed join.** Nemo stores its result trie; kermit's cells and DuckDB
  copy rows into a `Tuples`, sort and count. The costs are alike in kind (a stored, ordered result),
  not equal.
- **Join order.** Nemo runs kermit's plan, so a difference is not a join-order difference.
- **Probes are single cold runs.** Compare them with other single runs, never with a Criterion
  estimate.
- **Memory.** Nemo does not stream, so its `--materialize-max-bytes` guard reads `expected` alone
  and runs before the describe step. Over the limit, nothing materialises: no preflight count, no
  describe-step count, no `materialize`, only the stderr note and the `materialize` metadata line.
  A `--verify`-only run (no `materialize` metric) over the limit materialises nothing either, and
  skips verification for that cell with the same note.
  A query without `expected` prints this warning once and runs unguarded:

  `warning: {label} does not stream and {benchmark}/{query} has no expected count, so materialize runs without the --materialize-max-bytes guard`

## Version pin

- `nemo-physical` is a git dependency in `kermit/Cargo.toml` at
  `rev = "04f0962b91edc1bbe73e6f65f08d41d1ee491980"` (workspace version 0.10.2-dev, `main` on
  2026-10-18), with `futures` (`executor::block_on`) and `async-trait` (`TableProvider`). None is in
  `default`; the feature is `engine-nemo`.
- `kermit::engine::nemo` exports `NEMO_REV` and `NEMO_PHYSICAL_VERSION`, and `Engine::version`
  returns `nemo-physical {NEMO_PHYSICAL_VERSION} @ {first 12 of NEMO_REV}`, which every Nemo report
  records as `engine_version`. A unit test pins `NEMO_REV` against the `rev` in `kermit/Cargo.toml`.
- Upstream is nightly-only, and at this revision it needs a rustc of 2026-11-02 or later:
  on the 2026-10-31 nightly it fails with `E0391` (rust-lang/rust#165263, fixed 2026-11-01). The
  flake's `rust-overlay` is locked past that.

To move the pin:
1. Change `rev` in the `nemo-physical` dependency of `kermit/Cargo.toml`, then `cargo update -p
   nemo-physical` to rewrite `Cargo.lock`.
2. Change `NEMO_REV` and `NEMO_PHYSICAL_VERSION` in `kermit/src/engine/nemo.rs` to the new revision
   and its workspace version. The unit test fails until they agree with `Cargo.toml`.
3. Re-check the public API the engine uses (`DatabaseInstance::register_table`,
   `add_source_external`, `load_trie_into_memory`, `ExecutionPlan`, `execute_plan`,
   `count_rows_in_memory`, `table_size_bytes`, `TableProvider`, `TupleWriter`): any of them going
   private or changing signature is a code change, not a pin move.
4. Run `cargo test -p kermit --features engine-duckdb,engine-nemo --lib`, the engine oracle and the
   acceptance probes below, and record the new revision here.

## Settings and limits

- `Settings` is `()`: Nemo has no kermit-side setting, and `--engine-threads` does not apply to it
  (its report carries no `engine_threads`).
- `--materialize-max-bytes` (default 4 GiB, range 1 MiB to 1 TiB) guards `materialize`. For Nemo,
  which does not stream, the row count is `expected` alone and the guard runs before the describe
  step, and a `--verify`-only run over the limit skips verification for the cell with the note (see
  Fairness caveats, Memory).
- kermit sets no timeout of its own, and an engine that fails ends its cell with an error; nothing is
  retried. No engine calls an outside service at run time: Nemo is a build-time git dependency.

## Tests

- `kermit/tests/engine_oracle.rs` checks Nemo's counts and contents against the **distinct** rows of
  the nested-loop oracle (distinct full assignments are the same set under set and bag semantics).
- Unit tests in `kermit/src/engine/nemo.rs` pin `NEMO_REV` against `kermit/Cargo.toml` and check that
  the join's markers follow kermit's plan order.
- The `engines` job of `.github/workflows/pr.yml` runs them with `engine-nemo`.

## See also

- [`duckdb.md`](duckdb.md) and [`TEMPLATE.md`](TEMPLATE.md).
- [`docs/data-structures/column-trie.md`](../data-structures/column-trie.md), Nemo's own family inside kermit.
- The design: [`docs/specs/2026-11-02-external-engines-design.md`](../specs/2026-11-02-external-engines-design.md).
````

- [ ] **Step 4: Run it to see it pass**

Run the check of Step 1 again.

Expected: no output and exit status 0.

- [ ] **Step 5: Commit**

```bash
git add -N docs/engines/TEMPLATE.md docs/engines/duckdb.md docs/engines/nemo.md
git commit -m "docs(engines): the engine template and the DuckDB and Nemo pages" -- docs/engines/TEMPLATE.md docs/engines/duckdb.md docs/engines/nemo.md
```

### Task 12.2: The report schema's v7 catalogue

**Files:**
- Modify: `docs/specs/bench-report-schema.md`

- [ ] **Step 1: Write the failing check**

```bash
grep -q '^\*\*Current schema version:\*\* `7`' docs/specs/bench-report-schema.md \
  && grep -q '`engine_version`' docs/specs/bench-report-schema.md \
  && grep -q '`engine_threads`' docs/specs/bench-report-schema.md \
  && grep -q '^| 7 ' docs/specs/bench-report-schema.md
```

- [ ] **Step 2: Run it to see it fail**

Run: `bash -c "grep -q '^\*\*Current schema version:\*\* \`7\`' docs/specs/bench-report-schema.md"; echo "exit $?"`

Expected: `exit 1` (the page says `6`).

- [ ] **Step 3: Implement**

Apply these edits to `docs/specs/bench-report-schema.md`, each replacing the quoted current text.

1. The version line. Replace

```
**Current schema version:** `6`
```

with

```
**Current schema version:** `7`
```

2. In the JSON example under `## BenchReport`, replace `"schema_version": 6,` with
`"schema_version": 7,` and replace the axis lines

```
      "relation_path": "kermit/tests/fixtures/edge.csv",
      "tuples": 4
```

with

```
      "relation_path": "kermit/tests/fixtures/edge.csv",
      "system": "kermit",
      "tuples": 4
```

3. In the field catalogue, replace the `schema_version` row's description

```
Currently `6`. Consumers should refuse unknown majors.
```

with

```
Currently `7`. Consumers should refuse unknown majors.
```

4. In `ValueEntry (v6)`, replace

```
or `build` / `iteration` (a probe's single runs).
```

with

```
or `build` / `iteration` / `materialize` (a probe's single runs; `materialize` only under `--materialize`).
```

5. Insert this paragraph immediately before the paragraph that starts `In v5 the time functions ran as`:

```
From v7, `materialize` is a further opt-in Criterion time function (`--metrics materialize` on
`bench run` and `bench join`; `bench probe --materialize` writes a single-run value of the same
name): the join collecting every full assignment of the executed query, sorted and counted
distinct (kermit's cells and DuckDB), or Nemo's `execute_plan` and row count. `bench ds` has no
join and rejects it. An engine's rows (below) use the same bare function ids under the group
segment `duckdb-t{N}` or `nemo` in place of `{ds}/{algo}`: `build`, `materialize`, and, for DuckDB
only (it streams), `iteration` and `end_to_end`; `space` is an exact `values` entry for Nemo only
(DuckDB keeps no index).
```

6. In the conventional axes table, replace the tail of the `data_structure` row

```
The `IndexStructure::axis_value` string. |
```

with

```
The `IndexStructure::axis_value` string. Absent from engine rows (v7). |
```

and the tail of the `algorithm` row

```
The `JoinAlgorithm::axis_value` string. |
```

with

```
The `JoinAlgorithm::axis_value` string. Absent from engine rows (v7). |
```

7. Insert these three rows directly after the `allocator` row of the same table:

```
| `system`         | `join`, `ds`, `run`, `probe` | string       | v7: what ran the query. `"kermit"` for kermit's own cells (and `bench ds`), `"duckdb"` or `"nemo"` for an external engine cell (`--engine`). On every report from v7; kermit-lab back-fills `"kermit"` on earlier rows, which could only be kermit's. |
| `engine_version` | `run`, `probe` (engine rows) | string  | v7: the version of the engine library that actually ran. DuckDB: `SELECT library_version FROM pragma_version()` (for example `v1.4.4`). Nemo: `nemo-physical {version} @ {first 12 characters of the git revision}`. Absent from kermit rows. |
| `engine_threads` | `run`, `probe` (DuckDB rows) | number (usize) | v7: DuckDB's `PRAGMA threads` (`--engine-threads`, default 1). Absent from Nemo rows, which have no thread setting, and from kermit rows. |
```

An engine row sets `system`, `engine_version` and, for DuckDB, `engine_threads`; it omits
`data_structure`, `algorithm` and every `ds_*` axis. The rest of its axes (`benchmark`, `query`,
`optimiser`, `plan`, `answer_rows`, `verified`, `allocator`) are the run's, as for a kermit cell.

8. In `## Versioning policy`, replace

```
- v6 is a structural bump with no meaning change: `space` moved from
  `criterion_groups` into `values`. kermit-lab loads v5 and v6 reports
  together.
```

with

```
- v6 is a structural bump with no meaning change: `space` moved from
  `criterion_groups` into `values`. kermit-lab loads v5 and v6 reports
  together.
- v7 adds the `system` axis and engine rows, which omit `data_structure` and
  `algorithm`. No metric changes meaning (`materialize` is new and opt-in), so
  kermit-lab loads v6 and v7 reports together. The bump is there because every
  v6 run report carries `data_structure` and `algorithm`, kermit-lab relies on
  that (one back-fill keys on `algorithm` being present), and an older
  kermit-lab would misread an engine row.
```

9. Append this row to the end of the change log table:

```
| 7       | 2026-11-02 | External engines (DuckDB, Nemo, in process; `--engine`) and the opt-in `materialize` metric. New axes: `system` (on every report: `kermit`, `duckdb`, `nemo`), `engine_version` and `engine_threads` (engine rows only). Engine rows omit `data_structure`, `algorithm` and every `ds_*` axis. New Criterion function `materialize`, and a probe's single-run `materialize` value under `--materialize`. No metric changes what it measures, so kermit-lab loads v6 and v7 reports together and back-fills `system = "kermit"`. |
```

- [ ] **Step 4: Run it to see it pass**

Run the check of Step 1 again.

Expected: no output and exit status 0.

- [ ] **Step 5: Commit**

```bash
git commit -m "docs(schema): report v7, the system axis and engine rows" -- docs/specs/bench-report-schema.md
```

### Task 12.3: CLAUDE.md, ARCHITECTURE.md and BENCHMARKING.md

**Files:**
- Modify: `CLAUDE.md` (Build Commands; the `kermit` crate's architecture paragraph; "Extending the System"; Component Reference Docs; Gotchas)
- Modify: `ARCHITECTURE.md` (schema version sentence; a new section; "Adding New Components")
- Modify: `BENCHMARKING.md` (a new section; "Where to look next")

- [ ] **Step 1: Write the failing check**

```bash
grep -q '^### Adding an external engine' CLAUDE.md \
  && grep -q 'docs/engines/duckdb.md' CLAUDE.md \
  && grep -q '^### External engines (`kermit::engine`)' ARCHITECTURE.md \
  && grep -q '^### New External Engine' ARCHITECTURE.md \
  && grep -q '^## Comparing against external engines' BENCHMARKING.md
```

- [ ] **Step 2: Run it to see it fail**

Run: `bash -c "grep -q '^### Adding an external engine' CLAUDE.md"; echo "exit $?"`

Expected: `exit 1`.

- [ ] **Step 3: Implement**

**CLAUDE.md**, five edits.

1. Build Commands. Directly after the line

```
cargo run -- bench probe triangle -i all -a all                     # Screen a benchmark without Criterion: verify, one timed build and run, exact space
```

insert:

```
cargo run -p kermit --features engine-duckdb,engine-nemo -- bench run triangle -i all -a all --engine duckdb,nemo --metrics build iteration space materialize end-to-end --verify  # External engines beside the three cells (features off by default; the dev shell links Nix's libduckdb)
cargo run -p kermit --features engine-duckdb,engine-nemo -- bench probe triangle -i all -a all --engine duckdb,nemo --verify --materialize  # Screen engines too, and the opt-in materialize metric (single run)
```

2. Workspace Architecture. Replace the last line of the `kermit` crate's paragraph

```
                  All Criterion execution lives here (in `src/bench/`; including SpaceMeasurement).
```

with

```
                  All Criterion execution lives here (in `src/bench/`; including SpaceMeasurement).
                  `kermit::engine` (library; spec
                  `docs/specs/2026-11-02-external-engines-design.md`) runs other systems' joins
                  on kermit's own prepared queries: the `Engine` trait, the shared SQL translator
                  (`engine/sql.rs`), DuckDB (`engine/duckdb.rs`, feature `engine-duckdb`) and
                  Nemo (`engine/nemo.rs`, feature `engine-nemo`). Both features are off by
                  default; `bench/engine.rs` wraps an engine as a `Cell` beside the three
                  structure cells (`--engine`).
```

3. Extending the System. Insert this section immediately before the heading
`### Adding an optimization to a data structure or algorithm`:

```
### Adding an external engine

An engine runs another system's join on kermit's own prepared query, measured by kermit's own measure layer. It is not an index structure and not an `ExecutionFamily`: it reads `Prepared` (the executed query over index ids `#k`, each index's `IndexSpec`, the plan) after `prepare` has validated, rewritten, extracted accesses and planned, and never parses Datalog.

1. **Module.** Create `kermit/src/engine/<name>.rs` behind the cargo feature `engine-<name>`, registered in `kermit/src/engine/mod.rs` as `#[cfg(feature = "engine-<name>")] pub mod <name>;`. Precedents: `duckdb.rs` (SQL, through the shared `engine/sql.rs`) and `nemo.rs` (a plan over tries). Engines live in the library beside `kermit::db`, so `kermit/tests` can check them against the nested-loop oracle.
2. **Implement `Engine`** (`kermit/src/engine/mod.rs`): the constants `NAME` (the report's `system` axis), `STREAMS`, `KEEPS_DUPLICATES`, `MATERIALIZE_MUTATES` and `HAS_SPACE`; the `Settings`, `Loaded` and `Built` types; and `label`, `threads`, `version`, `supports`, `load` (untimed), `build` (the timed copy: one filtered copy per `IndexSpec`, so the engine pays the one copy every structure pays), `stream`, `materialize`, `space` and `rows` (full assignments in canonical order, for tests, never timed). Every engine answers the full conjunctive query, so a count equals kermit's bag count on duplicate-free inputs. All-constant atoms are handled once by `kermit::db::check_multiplicity`, so a translator never sees a check. Every method is fallible; `EngineCell` runs one fallible preflight per query, untimed and before the describe step, so the timed bodies `expect`.
3. **Feature.** Add `engine-<name>` to `kermit/Cargo.toml` with optional `dep:` entries, never in `default`. An engine that links a native library says how the dev shell and CI find it (DuckDB: `DUCKDB_LIB_DIR`, `kermit/build.rs`, and the CI-only `engine-duckdb-bundled`).
4. **CLI value.** Add a variant to `EngineSelector` (`kermit/src/options.rs`), its feature-missing error in `EngineChoices::resolve`, and the arm that turns an `EngineChoice` into an `EngineCell<YourEngine>` in the sweep (`kermit/src/bench/run.rs`), so `bench run`, `bench probe` and `bench join` reach it.
5. **Wire the tests.** Add the engine's cells to `kermit/tests/engine_oracle.rs` (counts and contents over `common::shapes`: against `oracle::<HashTrieSip>` on bag semantics, or its distinct rows on set semantics), unit tests for its translation, and the engine's feature to the `engines` job of `.github/workflows/pr.yml`. The profile's gates build no cargo features, so the engine story's Verify line names `--features`.
6. **Write the doc.** Create `docs/engines/<name>.md` from `docs/engines/TEMPLATE.md`: phase mapping, fairness caveats, version pin and how to move it.
7. **kermit-lab.** If the engine has a like-for-like pair, add it to `ENGINE_PAIRS` in `python/kermit-lab/kermit_lab/presets.py`.

Do **not** modify kermit's structures, algorithms, optimisers or existing metrics for an engine (Priorities item 6).

```

4. Component Reference Docs. Directly after the line

```
- `docs/optimisers/TEMPLATE.md` — skeleton for new optimiser docs.
```

insert:

```
- `docs/engines/duckdb.md` — DuckDB in process (`--engine duckdb`, feature `engine-duckdb`): phase mapping, fairness caveats, version pin.
- `docs/engines/nemo.md` — Nemo in process (`--engine nemo`, feature `engine-nemo`): phase mapping, fairness caveats, version pin.
- `docs/engines/TEMPLATE.md` — skeleton for new engine docs.
```

5. Gotchas. Directly before the line

```
- **CI env vars**: All CI jobs set `RUST_BACKTRACE=1`.
```

insert:

```
- **External engines are off by default**: `engine-duckdb`, `engine-duckdb-bundled` and `engine-nemo` are cargo features outside `default`, so a plain `cargo build` / `cargo test` compiles neither engine and the existing CI jobs are unchanged. The dev shell's `engine-duckdb` links Nix's `libduckdb` through `DUCKDB_LIB_DIR` and `DUCKDB_INCLUDE_DIR` and compiles no C++; **never build `engine-duckdb-bundled` on this host** (its compiler process needs several GB, and `earlyoom` prefers to kill `cc1plus`); CI builds it in the separate `engines` job. The flow profile's gates build no features, so an engine story's Verify line passes `--features`. `nemo-physical` needs a nightly of 2026-11-02 or later (`E0391`, rust-lang/rust#165263 before).
- **`materialize` is the full assignment**: the opt-in metric (`--metrics materialize`, probe `--materialize`) joins, collects every variable of the executed query (`kermit::db::execute_assignments`, not the head prefix), sorts (`Tuples::sort`) and counts adjacent-distinct rows (`materialize_count`), because deduplicating head rows would collapse duplicates the bag `expected` counts. On duplicate-free inputs it equals `expected` on every cell, Nemo's stored result trie included. It is skipped, with a stderr note and a `materialize` metadata line, when rows × variables × 8 B exceeds `--materialize-max-bytes` (default 4 GiB, range 1 MiB to 1 TiB); for a streaming cell the row count is `expected`, else the describe step's streamed count, before any `materialize`; an engine that cannot stream (Nemo) is guarded from `expected` alone, before its describe step, and over the limit nothing materialises (no preflight count, no describe-step count, no `materialize`; a `--verify`-only run over the limit also materialises nothing and skips verification for that cell, with the note); without `expected` it prints `warning: {label} does not stream and {benchmark}/{query} has no expected count, so materialize runs without the --materialize-max-bytes guard` and runs unguarded. `bench ds` rejects the metric. A cell whose `materialize` changes its built state (`MATERIALIZE_MUTATES`: Nemo) runs every sample on a fresh build.
- **Report v7 and the `system` axis**: every report carries `system` (`kermit`, `duckdb`, `nemo`); engine rows add `engine_version` and, for DuckDB, `engine_threads`, and omit `data_structure`, `algorithm` and every `ds_*` axis. v6 and v7 load together because no metric changed meaning; the bump is there because kermit-lab relies on `data_structure` and `algorithm` being on every run report. kermit-lab back-fills `system = "kermit"` and labels cells with `analysis.cell_labels`.
- **Compare engines on like-for-like pairs only**: DuckDB against kermit on `end_to_end` (its hash tables are built inside its `iteration`, kermit's inside `build`), Nemo against kermit on `build`, `materialize` and `space` (it has no streamed `iteration` or `end_to_end`, and the stderr notes say so). `kl.engines()` draws only these pairs. DuckDB runs one thread by default (`--engine-threads`) and plans its own join order; Nemo runs kermit's plan. Probe numbers are single cold runs.
```

**ARCHITECTURE.md**, three edits.

1. In "JSON bench reports", replace

```
(currently `5`)
```

with

```
(currently `7`)
```

and, in the same paragraph, replace the paragraph's last clause

```
and each index adds an `index` metadata line (id, relation, filter, column order, rows, bytes).
```

with

```
and each index adds an `index` metadata line (id, relation, filter, column order, rows, bytes). Since v7 every report carries `system` (`kermit`, `duckdb`, `nemo`), engine rows add `engine_version` (and `engine_threads` for DuckDB) and omit `data_structure` and `algorithm`, and `materialize` is a further opt-in metric.
```

2. Insert this section immediately before the heading `### Space measurement`:

```
### External engines (`kermit::engine`)

`kermit/src/engine/` runs other systems' joins on kermit's own prepared queries (spec `docs/specs/2026-11-02-external-engines-design.md`). It is a library module beside `kermit::db`, so the nested-loop oracle can check it from `kermit/tests`:

- `mod.rs` holds the `Engine` trait (a family of associated constants and types, so it is deliberately not object-safe, like `ExecutionFamily`), `EngineError` and `Unsupported`. An engine reads `Prepared` (the executed query over index ids, each index's `IndexSpec`, the plan) and never parses Datalog.
- `sql.rs` is the translator every SQL engine shares: a `CREATE TEMP TABLE` per spec, from the base relation under the spec's `Filter`, and one `SELECT` over aliases `a{i}` of the spec tables that selects every variable once, in canonical order.
- `duckdb.rs` (feature `engine-duckdb`, or `engine-duckdb-bundled` in CI) and `nemo.rs` (feature `engine-nemo`) implement `Engine`. Both features are off by default.

The binary wraps an engine as a cell. `run_benchmark` is generic over `bench::cell::Cell`; `FamilyCell` wraps today's `ExecutionFamily` cells unchanged, and `bench::engine::EngineCell<E>` wraps an engine, with `EngineSubject<E>` as its `Subject` (the engine's loaded database, its built state, and the same `Subject` methods the structure cells have, including `materialize`). Engine cells join the `bench run` / `bench probe` / `bench join` sweep through `--engine` and share the prepared queries of the sorted cells (`TrieLevels::All`). The timed `build` includes `kermit::db::check_multiplicity` with the engine's `KEEPS_DUPLICATES`, so an all-constant atom behaves as on every structure. Engines answer the full conjunctive query: `materialize` counts distinct full assignments on every cell (`kermit::db::execute_assignments`, `materialize_count`), and `Catalog` holds each relation's tuples behind an `Arc` so Nemo's table providers can stream from it with one copy.

Which phases an engine offers differs (DuckDB: `build`, `iteration`, `materialize`, no `space`; Nemo: `build`, `materialize`, `space`, no `iteration`), so only like-for-like pairs compare: DuckDB with kermit on `end_to_end`, Nemo with kermit on `build`, `materialize` and `space` (`BENCHMARKING.md`; `kl.engines()`). Per-engine pages are in `docs/engines/`.

```

3. In "Adding New Components", insert this section immediately before the heading `### New Benchmark`:

```
### New External Engine

1. Create `kermit/src/engine/<name>.rs` behind a new cargo feature `engine-<name>` (never in `default`) and implement `Engine`: the constants, the `Settings` / `Loaded` / `Built` types and the phase methods. The engine reads `Prepared` and answers the full conjunctive query; a SQL engine reuses `engine/sql.rs`.
2. Add a variant to `EngineSelector` in `kermit/src/options.rs`, with its feature-missing error in `EngineChoices::resolve`, and the arm that builds its `EngineCell<E>` in the sweep (`kermit/src/bench/run.rs`).
3. **Wire the tests.** Add its cells to `kermit/tests/engine_oracle.rs` and its feature to the `engines` job of `.github/workflows/pr.yml`.
4. **Write the doc** at `docs/engines/<name>.md` from the template, and add its like-for-like pair, if it has one, to `ENGINE_PAIRS` in `python/kermit-lab/kermit_lab/presets.py`.

```

**BENCHMARKING.md**, two edits.

1. Insert this section immediately before the heading `## Statistical tooling`:

````
## Comparing against external engines

kermit can run DuckDB and Nemo in process on the very queries its own cells run, measured by the same
measure layer: `--engine duckdb,nemo` adds engine cells beside the `-i`/`-a` cells of `bench run`,
`bench probe` and `bench join`. The engines are cargo features that are off by default, so the binary
must be built with them (in the dev shell, `--features engine-duckdb,engine-nemo`):

```sh
cargo run -p kermit --features engine-duckdb,engine-nemo -- bench run triangle -i all -a all \
  --engine duckdb,nemo --metrics build iteration space materialize end-to-end --verify
```

This is the thesis's RQ1.4: a **bounded sanity baseline** against an established peer engine, with
explicit threats to validity, not a head-to-head claim. An engine reads the same `Prepared` query, so
all of them answer the full conjunctive query, and on duplicate-free inputs every count equals the
benchmark's `expected` (`--verify` checks it for each cell).

**The phases differ, so only some pairs compare.** Where an engine does its index work decides which
phase is the same work as kermit's:

| Engine | Compare with kermit on | Why | Not offered |
| --- | --- | --- | --- |
| DuckDB (`duckdb-t{N}`) | `end_to_end` | its hash tables are built inside its `iteration`, while kermit's index work is in `build`, so only the sum is the same work | `space` (it keeps no index) |
| Nemo (`nemo`) | `build`, `materialize`, `space` | it builds sorted tries as `ColumnTrie` does and stores its result | `iteration`, `end_to_end` (its streaming join is private upstream); stderr says so |

`kl.engines()` draws exactly these pairs and no others. kermit-lab's six structure figures
(`scaling`, `bar_time`, `bar_space`, `tradeoff`, `dist` and `bar_queries`) draw kermit rows only,
because a generic figure would put an engine beside kermit on a metric that is not a like-for-like
pair; `kl.plot` draws engines too, through the cell label, for a reader who picks the metric
knowingly. Read the fairness caveats before quoting a
ratio: [`docs/engines/duckdb.md`](docs/engines/duckdb.md) and
[`docs/engines/nemo.md`](docs/engines/nemo.md) (DuckDB plans its own join order, Nemo runs kermit's
plan; DuckDB counts duplicate rows and Nemo does not).

**`materialize` (opt-in, `--metrics materialize`; probes: `--materialize`).** The join, collecting
every full assignment of the executed query, sorted and counted distinct, on kermit's cells and
DuckDB; Nemo stores its result trie and counts it. It is not `iteration`: kermit keeps streaming for
that, because storing results adds a cost roughly equal for every structure and a result such as
SNAP's `path3` does not fit in memory. A query whose `materialize` would exceed
`--materialize-max-bytes` (default 4 GiB) is skipped with a stderr note and a metadata line. Nemo
does not stream, so its guard reads `expected` alone; over the limit nothing materialises, and a run
asked only for `--verify` skips verification for that cell with the same note.

**Threads and variance.** DuckDB runs one thread (`--engine-threads N` for more; every report records
`engine_threads`), the parallelism of kermit's joins and serial builds. Probe numbers are single cold
runs: compare them with other single runs, not with Criterion estimates. Every report records the
engine's version (`engine_version`) and the allocator; do not mix reports across an engine pin move.

````

2. In "Where to look next", directly after the row

```
| Benchmark catalogue, WatDiv, LUBM | [`docs/benchmarks/README.md`](docs/benchmarks/README.md) |
```

insert:

```
| External engines (DuckDB, Nemo): phases, fairness, version pins | [`docs/engines/duckdb.md`](docs/engines/duckdb.md), [`docs/engines/nemo.md`](docs/engines/nemo.md) |
```

- [ ] **Step 4: Run it to see it pass**

Run the stricter check of Step 1 again.

Expected: no output and exit status 0.

- [ ] **Step 5: Commit**

```bash
git commit -m "docs: the external-engine recipe, module and like-for-like pairs" -- CLAUDE.md ARCHITECTURE.md BENCHMARKING.md
```

### Task 12.4: The docs that the engines, the metric and v7 leave stale

**Files:**
- Modify: `CLAUDE.md` (the JSON bench reports gotcha; the `bench run` sweeps gotcha; the load, prepare, build, execute gotcha; the defaulted Layout parameters gotcha)
- Modify: `ARCHITECTURE.md:94`, `ARCHITECTURE.md:444-445`
- Modify: `BENCHMARKING.md:75-78`, `BENCHMARKING.md:94-98`, `BENCHMARKING.md:158-159`
- Modify: `README.md:82`, `README.md:123`, `README.md:137`
- Modify: `USAGE.md:165-168`, `USAGE.md` ("Screen a benchmark" section, after the paragraph ending "estimate.")
- Modify: `kermit/README.md:16-17`
- Modify: `python/kermit-lab/README.md:200-202`

Task 12.3 adds the new text; this task corrects the text that stops being true. Each edit quotes the
current text of the tree. Edit with the Edit tool, and quote the line numbers only to find the text.

- [ ] **Step 1: Write the failing check**

```bash
grep -q 'schema_version` (currently `7`)' CLAUDE.md \
  && grep -q 'run_benchmark<C: Cell>' CLAUDE.md \
  && grep -q 'run_benchmark<C: Cell>' ARCHITECTURE.md \
  && grep -q 'opt-in `end-to-end` and `materialize`' CLAUDE.md \
  && grep -q '^| \*\*Materialize\*\* |' BENCHMARKING.md \
  && grep -q -- '--engine-threads' USAGE.md \
  && grep -q '`engine`' kermit/README.md \
  && grep -q 'kl.engines()' python/kermit-lab/README.md
```

- [ ] **Step 2: Run it to see it fail**

Run: `bash -c "grep -q 'run_benchmark<C: Cell>' CLAUDE.md"; echo "exit $?"`

Expected: `exit 1`.

- [ ] **Step 3: Implement**

**CLAUDE.md**, four edits, each replacing the quoted text with the text after it.

1. In the JSON bench reports gotcha, replace

```
Schema is versioned via `schema_version` (currently `6`) and lives in
```

with

```
Schema is versioned via `schema_version` (currently `7`) and lives in
```

and replace

```
`answer_rows` whenever the describe step counted, and `allocator` on every report)
```

with

```
`answer_rows` whenever the describe step counted, `allocator` and `system` (`kermit`, `duckdb`, `nemo`) on every report, and `engine_version` (plus `engine_threads` for DuckDB) on an engine's, which omits `data_structure`, `algorithm` and every `ds_*` axis)
```

2. In the `bench run` sweeps gotcha, replace

```
One generic `run_benchmark<F: ExecutionFamily>` in `kermit/src/bench/run.rs` serves both trait families and `bench run`,
```

with

```
One generic `run_benchmark<C: Cell>` in `kermit/src/bench/run.rs` (`Cell`, `kermit/src/bench/cell.rs`) serves both trait families, each wrapped in a `FamilyCell`, the engine cells (`EngineCell<E>`, `kermit/src/bench/engine.rs`, added by `--engine`) and `bench run`,
```

3. In the load, prepare, build, execute gotcha, replace

```
`--metrics` takes `build`, `iteration`, `space` (the default) and the opt-in `end-to-end`;
```

with

```
`--metrics` takes `build`, `iteration`, `space` (the default) and the opt-in `end-to-end` and `materialize`;
```

4. In the defaulted Layout parameters gotcha, replace

```
kermit-lab draws a seek ablation only for `iteration` / `end_to_end` (`AXIS_PHASES` in `presets.py`).
```

with

```
kermit-lab draws a seek ablation only for `iteration` / `end_to_end` / `materialize` (`AXIS_PHASES` in `presets.py`; a seek strategy moves every phase that runs the join, and `materialize` does).
```

**ARCHITECTURE.md**, two edits.

1. In the row of the sorted/hash comparison table, replace

```
| `bench run` dispatch | one generic `run_benchmark<F: ExecutionFamily>` | the same `run_benchmark<F>` |
```

with

```
| `bench run` dispatch | one generic `run_benchmark<C: Cell>`, the structure cell wrapped in a `FamilyCell` | the same `run_benchmark<C>` |
```

2. In the `kermit bench` subcommand list, replace

```
  - `bench run [<NAME> | --all]` — run YAML-defined benchmarks; `--metrics` selects from `build`, `iteration`, `space`, plus the opt-in `end-to-end`. `--force` opts into regenerating a generator-driven benchmark when its spec hash drifts.
  - `bench probe [<NAME> | --all]` — screen YAML-defined benchmarks without Criterion: per query and cell, verify against `expected`, time one build and one execution, record the exact `space`, `answer_rows` and `plan`.
```

with

```
  - `bench run [<NAME> | --all]` — run YAML-defined benchmarks; `--metrics` selects from `build`, `iteration`, `space`, plus the opt-in `end-to-end` and `materialize`. `--engine duckdb,nemo` adds external-engine cells beside the structure cells. `--force` opts into regenerating a generator-driven benchmark when its spec hash drifts.
  - `bench probe [<NAME> | --all]` — screen YAML-defined benchmarks without Criterion: per query and cell, verify against `expected`, time one build and one execution, record the exact `space`, `answer_rows` and `plan`. `--materialize` adds a single-run `materialize`, and `--engine` adds engine cells.
```

**BENCHMARKING.md**, three edits.

1. Replace

```
`bench ds` and `bench run` accept `--metrics`, defaulting to `build`,
`iteration`, and `space` (the opt-in `end-to-end` phase is *not* in the
default set). They fall into two families.
```

with

```
`bench ds` and `bench run` accept `--metrics`, defaulting to `build`,
`iteration`, and `space` (the opt-in `end-to-end` and `materialize` phases
are *not* in the default set, and `bench ds` rejects `materialize`, since it
has no join). They fall into two families.
```

2. In the phase table, directly after the row that begins `| **End-to-end** | \`end-to-end\` |`, insert

```
| **Materialize** | `materialize` | Opt-in. The join collecting every full assignment of the executed query into one flat batch, sorting it (`Tuples::sort`) and counting adjacent-distinct rows; Nemo stores its result trie and counts that. A cell whose `materialize` changes its built state (a lazy HashTrie, Nemo) runs every sample on a fresh build | Comparing against engines that store their results; it is not a replacement for Iteration, which stays streaming |
```

3. Directly after the paragraph ending

```
So compare TreeTrie `build` across v5 and v6 only within that bound, as any
two binaries already are.
```

insert

```

Reports with `schema_version` 7 measure what v6 measured: every report gains the `system`
axis (`kermit`, `duckdb`, `nemo`), engine rows add `engine_version` (and `engine_threads` for
DuckDB) and omit `data_structure`, `algorithm` and the `ds_*` axes, and `materialize` is a new opt-in
phase. `kermit-lab` loads v6 and v7 together and back-fills `system = "kermit"`. See
[Comparing against external engines](#comparing-against-external-engines).
```

**README.md**, three edits.

1. Replace

```
`T = build + K × query`); it is never in the default set.
```

with

```
`T = build + K × query`); it is never in the default set. The opt-in `materialize` metric times the join collecting every full assignment, sorted and counted distinct; it is not in the default set either. `--engine duckdb,nemo` adds DuckDB and Nemo cells beside the kermit cells (cargo features `engine-duckdb` and `engine-nemo`, off by default; see `BENCHMARKING.md`).
```

2. Replace

```
times one build plus K full-trie iterations per Criterion sample.
```

with

```
times one build plus K full-trie iterations per Criterion sample, and the opt-in `materialize` metric times the join collecting every full assignment.
```

3. Replace

```
Supported metrics (on `bench ds`/`bench run`): `build`, `iteration`, `space`, plus the opt-in `end-to-end`.
```

with

```
Supported metrics (on `bench run`; `bench ds` takes all but `materialize`): `build`, `iteration`, `space`, plus the opt-in `end-to-end` and `materialize`.
```

**USAGE.md**, two edits.

1. Replace

```
`build`, `iteration` and `space` metrics by default (`-m`
selects; `end-to-end` is opt-in, with `--queries-per-build K` setting its K,
default 1), accepts
```

with

```
`build`, `iteration` and `space` metrics by default (`-m`
selects; `end-to-end` and `materialize` are opt-in, with `--queries-per-build K` setting the K
of `end-to-end`, default 1), accepts
```

2. In "Screen a benchmark (`bench probe`)", directly after the paragraph ending

```
run is cold, so compare it with other single runs, never with a Criterion
estimate.
```

insert

````

Probe also runs external engines and the opt-in `materialize` metric. The engines are cargo
features that are off by default, so build with them (in the dev shell,
`--features engine-duckdb,engine-nemo`):

```sh
cargo run -p kermit --features engine-duckdb,engine-nemo -- bench probe triangle -i all -a all --engine duckdb,nemo --verify --materialize
```

- `--engine duckdb,nemo` adds an engine cell per name beside the `-i`/`-a` cells; `-i`/`-a` are
  required only without it. Naming an engine whose feature is not compiled in is a usage error that
  names the feature.
- `--engine-threads N` sets DuckDB's threads (default 1, range `1..=available_parallelism()`);
  Nemo ignores it.
- `--materialize` adds a single-run `materialize` value to every cell. `--materialize-max-bytes B`
  (default 4294967296, range 1 MiB to 1 TiB) skips a `materialize` whose collected rows would
  exceed `B`, with a stderr note and a `materialize` metadata line.
- An engine cell gets the single-run `build`, `iteration` (DuckDB only: Nemo does not stream, and
  stderr says so) and, with `--materialize`, `materialize`. DuckDB reports no `space`.
````

**kermit/README.md**: in "Library surface", directly after the line that begins ``- [`compute_join`](src/lib.rs) — helper that builds`` insert

```
- [`engine`](src/engine/mod.rs) — external engines (DuckDB, Nemo) that run a join on a prepared query: the `Engine` trait, the shared SQL translator, and one module per engine behind the off-by-default cargo features `engine-duckdb` and `engine-nemo`. The binary's `--engine` flag wraps each as a benchmark cell.
```

**python/kermit-lab/README.md**: directly after the paragraph ending

```
columns. No metric changed meaning at v6, so v5 and v6 reports load
together.
```

insert

```

A v7 report adds the `system` axis (`kermit`, `duckdb`, `nemo`) to every report, the
`engine_version` axis (and DuckDB's `engine_threads`) to engine rows, which have no
`data_structure`, `algorithm` or `ds_*` axis, and the `materialize` time phase.
`kl.load` back-fills `system = "kermit"` on rows that lack it, so v6 and v7 reports load
together (no metric changed meaning at v7), and `kermit_lab.analysis.cell_labels(df)` names a
row's cell for plots (`system`, plus the thread count, for an engine). `kl.engines(df)` draws only
the like-for-like pairs: DuckDB against kermit on `end_to_end`, Nemo against kermit on `build`,
`materialize` and `space`. `speedup_table` ignores engine rows.

The six structure figures (`scaling`, `bar_time`, `bar_space`, `tradeoff`, `dist` and `bar_queries`)
draw kermit rows only: they name kermit's data structures, and a generic figure never puts an engine
beside kermit on a metric that is not a like-for-like pair. Engines are drawn by `kl.engines()`, and
by `kl.plot` through the cell label.
```

- [ ] **Step 4: Run it to see it pass**

Run the check of Step 1 again.

Expected: no output and exit status 0.

- [ ] **Step 5: Commit**

```bash
git commit -m "docs: correct the stale metric, schema and cell text for external engines" -- CLAUDE.md ARCHITECTURE.md BENCHMARKING.md README.md USAGE.md kermit/README.md python/kermit-lab/README.md
```

### Task 12.5: Run the acceptance probes and record them

**Files:**
- Modify: `docs/engines/duckdb.md` (append the probe table)
- Modify: `docs/engines/nemo.md` (append the probe table)

The probes read the shared cache (`~/.cache/kermit/benchmarks/`) and the 2026-10-24 sweep's private
cache; they run no `--force` and no `bench gen`. Run every command in the dev shell, and never build
`engine-duckdb-bundled`.

- [ ] **Step 1: Write the failing check**

```bash
grep -q '^## Acceptance probes' docs/engines/duckdb.md && grep -q '^## Acceptance probes' docs/engines/nemo.md
```

Note that `docs/engines/TEMPLATE.md` already carries a heading of that name, as guidance, and the two
engine pages do not yet.

- [ ] **Step 2: Run it to see it fail**

Run: `bash -c "grep -q '^## Acceptance probes' docs/engines/duckdb.md"; echo "exit $?"`

Expected: `exit 1`.

- [ ] **Step 3: Implement**

Build the release binary with both engines, then write the probe runner and the table generator into
a scratch directory outside the tree.

```bash
nix develop --command cargo build --release -p kermit --features engine-duckdb,engine-nemo
OUT=$(mktemp -d)
echo "$OUT"
```

Write `$OUT/probes.sh` (spec Acceptance 3: one query of every benchmark family, five cells each):

```bash
cat > "$OUT/probes.sh" <<'EOF'
#!/usr/bin/env bash
# usage: probes.sh OUT_DIR   (run from the workspace root, inside `nix develop`)
set -euo pipefail
out=$1
n=0
shared() {
  n=$((n + 1))
  target/release/kermit bench --report-json "$out/probe-$n.json" probe "$@" \
    -i all -a all --engine duckdb,nemo --verify --materialize
}
sweep() {
  n=$((n + 1))
  XDG_CACHE_HOME=/tb/Source/Academia/kermit-bench-runs/full-sweep-2026-10-24/cache \
    target/release/kermit bench --report-json "$out/probe-$n.json" probe "$@" \
    -i all -a all --engine duckdb,nemo --verify --materialize
}
shared triangle
shared lubm-reference -q q1
shared watdiv-stress-100-test-1 -q q0001
shared oxford-uniform-s3 -q triangle
sweep sw-snap-amazon-pre -q triangle
sweep sw-h2o-pre -q q1
sweep sw-job-1-pre -q 1a
EOF
chmod +x "$OUT/probes.sh"
```

Write `$OUT/table.py`, which reads the probes' reports and prints one engine's table (a Markdown
section; no number is typed by hand):

```bash
cat > "$OUT/table.py" <<'EOF'
"""Print the acceptance-probe table of one engine from the probes' reports.

usage: table.py OUT_DIR ENGINE DATE   (ENGINE is the report's `system`: duckdb or nemo)
"""
import json
import sys
from pathlib import Path

out, engine, date = Path(sys.argv[1]), sys.argv[2], sys.argv[3]


def time_ms(values, function):
    for v in values:
        if v["function"] == function and v["metric"] == "time":
            return f"{v['value'] / 1e6:.3f}"
    return "-"


def space_bytes(values):
    for v in values:
        if v["function"] == "space" and v["metric"] == "space":
            return str(int(v["value"]))
    return "-"


probes = sorted(out.glob("probe-*.json"), key=lambda p: int(p.stem.split("-")[1]))
print(f"## Acceptance probes ({date})")
print()
print(
    "`bench probe <benchmark> [-q <query>] -i all -a all --engine duckdb,nemo --verify "
    "--materialize` on one query of every benchmark family: three kermit cells and both "
    "engines, every count verified (against `expected`, else compared across the cells). "
    "Times are single cold runs in milliseconds: compare them with other single runs, never "
    "with Criterion estimates. `-` means the engine does not offer the phase."
)
print()
print(
    "| Benchmark | Query | Cells | `answer_rows` | Cells agree "
    "| `build` ms | `iteration` ms | `materialize` ms | `space` bytes | `engine_version` |"
)
print("|---|---|---|---|---|---|---|---|---|---|")
for path in probes:
    reports = json.loads(path.read_text())
    first = reports[0]["axes"]
    answers = {r["axes"].get("answer_rows") for r in reports}
    agree = "yes" if len(answers) == 1 and None not in answers else "NO"
    shown = ", ".join(sorted(str(a) for a in answers))
    for r in reports:
        axes = r["axes"]
        if axes.get("system") != engine:
            continue
        values = r.get("values", [])
        print(
            f"| {first.get('benchmark')} | {first.get('query')} | {len(reports)} | {shown} "
            f"| {agree} | {time_ms(values, 'build')} | {time_ms(values, 'iteration')} "
            f"| {time_ms(values, 'materialize')} | {space_bytes(values)} "
            f"| {axes.get('engine_version')} |"
        )
EOF
```

Run the probes. A non-zero exit stops the task: report the failing probe and its stderr instead of
changing anything.

```bash
nix develop --command bash "$OUT/probes.sh" "$OUT"
```

Expected: seven probes run to completion with exit status 0, each writing `$OUT/probe-<n>.json`
and printing the cells' stderr blocks (five cells per probe: three kermit, `duckdb-t1`, `nemo`;
Nemo's `iteration` and `end_to_end` skipped with the note
`note: nemo does not stream, so it skips iteration and end-to-end`, and DuckDB with
`note: duckdb-t1 reports no space`).

Write `$OUT/insert.py`, which puts a table into a page just before its `## See also` section (the
template's order is `## Tests`, `## Acceptance probes`, `## See also`):

```bash
cat > "$OUT/insert.py" <<'EOF'
import sys
from pathlib import Path

page, table = Path(sys.argv[1]), Path(sys.argv[2]).read_text()
head, marker, tail = page.read_text().partition("## See also")
assert marker, f"{page} has no '## See also' section"
page.write_text(head + table.rstrip("\n") + "\n\n" + marker + tail)
EOF
```

Generate each engine's table and insert it into its page:

```bash
DATE=$(date +%F)
for engine in duckdb nemo; do
  python3 -I "$OUT/table.py" "$OUT" "$engine" "$DATE" > "$OUT/$engine.md"
  python3 -I "$OUT/insert.py" "docs/engines/$engine.md" "$OUT/$engine.md"
done
```

- [ ] **Step 4: Run it to see it pass**

Run the check of Step 1 again, then count each page's probe rows (every row of the table after
its header starts with a lower-case benchmark name):

```bash
for engine in duckdb nemo; do
  sed -n '/^## Acceptance probes/,/^## See also/p' "docs/engines/$engine.md" | grep -c '^| [a-z]'
done
grep -c '| NO |' docs/engines/duckdb.md docs/engines/nemo.md
```

Expected: the check exits 0, the loop prints `7` twice (one row per probe), and the last command
prints `docs/engines/duckdb.md:0` and `docs/engines/nemo.md:0`. A `NO` means the five cells'
`answer_rows` disagreed: that is a finding to report, not to edit away.

- [ ] **Step 5: Commit**

```bash
git commit -m "docs(engines): the acceptance probes' outcomes" -- docs/engines/duckdb.md docs/engines/nemo.md
```
