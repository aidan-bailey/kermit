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
| `kermit/src/bench/cell.rs` | create: `Cell`, `FamilyCell`; then the `system` axis | 4, 9 |
| `kermit/src/db/catalog.rs` | `Arc<Tuples>` per entry; `shared_tuples` | 5 |
| `kermit/src/engine/mod.rs` | create: `Engine`, `EngineError`, `Unsupported` | 6 |
| `kermit/src/engine/sql.rs` | create: the SQL translator | 6 |
| `kermit/src/lib.rs` | `pub mod engine;` | 6 |
| `kermit/src/engine/duckdb.rs` | create: `DuckDb` | 7 |
| `kermit/build.rs` | create: rpath to `DUCKDB_LIB_DIR` | 7 |
| `kermit/Cargo.toml` | features `engine-duckdb`, `engine-duckdb-bundled`, then `engine-nemo`, and their dependencies | 7, 8 |
| `Cargo.toml` | `[profile.dev.package.libduckdb-sys] opt-level = 3` | 7 |
| `Cargo.lock` | the new dependencies | 7, 8 |
| `kermit/tests/engine_oracle.rs` | create with DuckDB's cells; then Nemo's | 7, 8 |
| `.github/workflows/pr.yml` | the `engines` job; then Nemo joins it | 7, 8 |
| `kermit/src/engine/nemo.rs` | create: `Nemo`, `ViewProvider` | 8 |
| `kermit/src/bench_report.rs` | `REPORT_SCHEMA_VERSION = 7` | 9 |
| `python/kermit-lab/kermit_lab/__init__.py` | `SCHEMA_VERSION = 7` | 9 |
| `kermit/src/bench/engine.rs` | create: `EngineSubject`, `EngineCell` | 10 |
| `kermit/src/options.rs` | `EngineSelector`, `EngineChoices`, `EngineChoice` | 10 |
| `kermit/tests/cli_engines.rs` | create: engine CLI behaviour | 10 |
| `python/kermit-lab/kermit_lab/defaults.py` | back-fill `system` | 11 |
| `python/kermit-lab/kermit_lab/presets.py` | `materialize` phase; `engines()` | 11 |
| `python/kermit-lab/kermit_lab/analysis.py` | cell label | 11 |
| `python/kermit-lab/tests/test_engines.py` | create | 11 |
| `docs/engines/TEMPLATE.md`, `docs/engines/duckdb.md`, `docs/engines/nemo.md` | create | 12 |
| `CLAUDE.md`, `ARCHITECTURE.md`, `BENCHMARKING.md`, `docs/specs/bench-report-schema.md` | the recipe, the module, v7, the like-for-like pairs | 12 |

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
/// `query` after `rewrite_placeholders`, with every variable in the head, in canonical order
/// (`canonical_names(&analyse(..))`): the full conjunctive query, whose rows are the full
/// assignments that `execute_assignments` and every engine return.
pub fn full_query(query: &JoinQuery) -> JoinQuery;
```

How the oracles compare (the fixtures hold duplicate rows):
- DuckDB's `stream` rows compare with `oracle::<HashTrieSip>(&relations, &full_query(q))`, which
  has bag semantics.
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
- `bench ds has no join, so it cannot measure materialize`.

`{label}` is `{ds}/{algo}` for kermit cells and `Engine::label` for engines.

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
    fn preflight(&self, catalog: &Catalog, prepared: &Prepared) -> anyhow::Result<()>;
    fn subject<'a>(&'a self, catalog: &'a Catalog, prepared: &'a Prepared) -> anyhow::Result<Self::Subject<'a>>;
}
pub(crate) struct FamilyCell<'f, F: ExecutionFamily>(pub(crate) &'f F);
// run.rs: run_benchmark<F: ExecutionFamily> becomes
fn run_benchmark<C: Cell>(cell: &C, workload: &PreparedWorkload, settings: RunSettings<'_>, sink: &mut ReportSink) -> anyhow::Result<()>;
```

`dispatch_run_bench` wraps each family in `FamilyCell`. Every existing report, stderr line and
Criterion group is byte-for-byte unchanged.

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
    fn build<'l>(loaded: &'l Self::Loaded, catalog: &Catalog, prepared: &Prepared) -> Result<Self::Built<'l>, EngineError>;
    fn stream(built: &Self::Built<'_>) -> Result<u64, EngineError>;         // only if STREAMS
    fn materialize(built: &Self::Built<'_>) -> Result<u64, EngineError>;
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

`build` takes the catalog as well, because Nemo's providers stream from `Catalog::shared_tuples`.
The spec's sketch is otherwise unchanged.

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
// check_sweep_group_directories takes `cells: &[SweepCell]`; criterion_group_name takes a SweepCell
// kermit/src/bench/engine.rs
pub(crate) struct EngineSubject<'a, E: Engine> { /* loaded: E::Loaded, catalog, prepared */ }
pub(crate) struct EngineCell<E: Engine> { pub(crate) settings: E::Settings }
impl<E: Engine> Cell for EngineCell<E> { /* … */ }
```

- **Where the flags go.** `EngineChoices` is flattened into `RunSelection`, used by `bench run` and
  `bench probe`; there `-i` and `-a` become `required_unless_present = "engine"`. It is also
  flattened into `bench join`, where `-i` and `-a` stay required, because `QueryArgs` is shared
  with `kermit join` (spec § CLI).
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
  - **Files:** `kermit/src/bench/mod.rs`, `kermit/src/bench/measure.rs`, `kermit/src/bench/run.rs`, `kermit/src/bench/ds.rs`, `kermit/src/main.rs`, `kermit/tests/cli_materialize.rs`
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
  - **Files:** `kermit/src/engine/duckdb.rs`, `kermit/src/engine/mod.rs`, `kermit/Cargo.toml`, `kermit/build.rs`, `Cargo.toml`, `Cargo.lock`, `kermit/tests/engine_oracle.rs`, `.github/workflows/pr.yml`
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
  - **Files:** `kermit/src/engine/nemo.rs`, `kermit/src/engine/mod.rs`, `kermit/Cargo.toml`, `Cargo.lock`, `kermit/tests/engine_oracle.rs`, `.github/workflows/pr.yml`
  - **Interfaces:** exposes `Nemo`, `NEMO_REV`, `NEMO_PHYSICAL_VERSION` and the feature `engine-nemo` (Names contract: the engines) to Stories 10 and 12; uses Stories 2, 5 and 6
  - **Verify:** `bash -c 'cargo test -p kermit --features engine-duckdb,engine-nemo --lib && cargo test -p kermit --features engine-duckdb,engine-nemo --test engine_oracle && RUSTFLAGS=-Dwarnings cargo clippy -p kermit --all-targets --features engine-duckdb,engine-nemo'`
  - **Review:** deep
  - **Reading:** spec § Nemo, § Background (Nemo's API), § Premises items 2–6; the premise check's Nemo module at `/tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-state-of-kermit-18e430b26aaa0f70/b3663c65-7a34-4d42-bcaa-8dc41bfcea9c/scratchpad/premise-check/src/main.rs` (may be gone after a reboot; the spec records what it proved)

- **Story 9** — Report schema v7 and the `system` axis.

  Bumps `REPORT_SCHEMA_VERSION` and kermit-lab's `SCHEMA_VERSION` to 7 together. Every kermit
  report gets `system: "kermit"`, through `FamilyCell::axes` and `bench ds`. No engine row exists
  yet. kermit-lab's existing tests must still pass on v7 reports.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/bench_report.rs`, `kermit/src/bench/cell.rs`, `kermit/src/bench/ds.rs`, `python/kermit-lab/kermit_lab/__init__.py`
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
  - **Files:** `python/kermit-lab/kermit_lab/defaults.py`, `python/kermit-lab/kermit_lab/presets.py`, `python/kermit-lab/kermit_lab/analysis.py`, `python/kermit-lab/kermit_lab/__init__.py`, `python/kermit-lab/tests/test_engines.py`
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
  - **Files:** `docs/engines/TEMPLATE.md`, `docs/engines/duckdb.md`, `docs/engines/nemo.md`, `CLAUDE.md`, `ARCHITECTURE.md`, `BENCHMARKING.md`, `docs/specs/bench-report-schema.md`
  - **Interfaces:** exposes nothing; uses every earlier story
  - **Verify:** `bash -c 'test -f docs/engines/TEMPLATE.md && test -f docs/engines/duckdb.md && test -f docs/engines/nemo.md && grep -q "Adding an external engine" CLAUDE.md && grep -q "engine_version" docs/specs/bench-report-schema.md && cargo build --release -p kermit --features engine-duckdb,engine-nemo && for b in "triangle" "lubm-reference -q q1" "watdiv-stress-100-test-1 -q q0001" "oxford-uniform-s3 -q triangle"; do target/release/kermit bench probe $b -i all -a all --engine duckdb,nemo --verify --materialize || exit 1; done && for b in "sw-snap-amazon-pre -q triangle" "sw-h2o-pre -q q1" "sw-job-1-pre -q 1a"; do XDG_CACHE_HOME=/tb/Source/Academia/kermit-bench-runs/full-sweep-2026-10-24/cache target/release/kermit bench probe $b -i all -a all --engine duckdb,nemo --verify --materialize || exit 1; done'`
  - **Review:** light (docs, and probes that change nothing)
  - **Reading:** spec § Documentation, § Acceptance item 3, § Summary; `CLAUDE.md` "Extending the System"; `docs/data-structures/TEMPLATE.md`

<!-- plan_join: the stories below are generated from the drafts; edit the drafts, then re-join -->
