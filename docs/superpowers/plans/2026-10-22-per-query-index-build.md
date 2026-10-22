# Per-Query Index Builds Implementation Plan

> **Supervised plan.** `/flow-draft` writes the stories below the marker from this skeleton,
> then `/flow-supervise` runs them story by story. Steps use checkbox (`- [ ]`) syntax for
> tracking.

**Goal:** Relations load into a flat `Catalog`, untimed. Each query then builds, and times,
only the indexes its plan reads. Each index is a relation filtered by the query's constants
and repeated variables, read through a borrowed `TupleView` in plan order, and copied once.
The `stored` policy, the const and selection views and the copy protocol are retired, and
reports move to schema v5.

**Architecture:**
- **Views (Stories 1–7).** `Filter` and `TupleView` live in kermit-iters. Every kermit-ds
  build mode reads a view and copies its rows once. The `from_view*` constructors become
  the required ones, and `from_tuples*` become conveniences over `TupleView::all`.
- **The new pipeline, beside the old (Stories 8–14).** Two renames free `IndexSpec`,
  `prepare` and `Prepared`. kermit-algos gains access extraction and index planning, and
  `kermit::db` gains `Catalog` plus `prepare` / `build` / `execute`. The test harness moves
  to them, and an all-orders test joins it.
- **The binary, then deletion (Stories 15–24).**
  - The families gain `build_index`, `build_indexes`, `execute` and `count_rows`.
  - The commands move over one at a time: `bench ds`, then `bench run` / `bench join`, then
    `kermit join`. `--column-orders` goes with the last.
  - Then come the deletions: the old family API, `Database<R>` with its copy protocol, the
    harness's policy, the views and rewrites, and the `stored` policy.
  - Report schema v5 and kermit-lab close the code.
- **Docs (Stories 25–27).**

**Tech Stack:** Rust nightly in the nix dev shell (`rust-toolchain.toml`; the profile's
prefix is `nix develop --command`); Criterion 0.8.2; kermit-lab on Python 3.13 under uv
(pandas, pytest).

**Spec:** `docs/specs/2026-10-22-per-query-index-build-design.md`, including Amendment 1.
Every decision it records is settled.

**Branch:** loom branch `aidanb/flat-vec-in-out`. Never switch branches, amend or push. The
change lands on master once, through the owner, after the final gate.

## File structure

| File | Change | Stories |
|---|---|---|
| `kermit-iters/src/tuple_view.rs` | create: `Filter`, `TupleView` | 1 |
| `kermit-iters/src/lib.rs` | export them | 1 |
| `kermit-ds/src/relation.rs` | view constructors, `KEEPS_DUPLICATES`; later the flip | 2, 7 |
| `kermit-ds/src/configured.rs`, `kermit-ds/src/built_with.rs` | forward views | 2, 7 |
| `kermit-ds/src/lib.rs` | re-export `Filter`, `TupleView` | 2 |
| `kermit-ds/src/morsel.rs` | `scatter_rows` and then `partition_rows` read views | 3, 6 |
| `kermit-ds/src/ds/tree_trie/implementation.rs` | builds from views | 3, 7 |
| `kermit-ds/src/ds/column_trie/implementation.rs` | builds from views | 4, 7 |
| `kermit-ds/src/ds/hash_trie/implementation.rs` | `KEEPS_DUPLICATES`; builds from views | 2, 5, 6, 7 |
| `kermit-ds/src/ds/hash_trie/radix.rs`, `kermit-ds/src/ds/hash_trie/parallel.rs` | partitioning builds read views | 6 |
| `kermit-algos/src/access.rs` | create: access extraction; later takes `RewriteError` | 9, 22 |
| `kermit-algos/src/index_plan.rs` | create: index planning | 10 |
| `kermit-algos/src/lib.rs` | facade | 8, 9, 10, 22, 23 |
| `kermit-algos/src/orient.rs` | `IndexSpec` renamed `CopySpec`, then deleted | 8, 22 |
| `kermit-algos/src/const_rewrite.rs`, `kermit-algos/src/selection_rewrite.rs` | delete | 22 |
| `kermit-algos/src/{sorted,hash}/{singleton,selection,iter_kind}.rs` | delete | 22 |
| `kermit-algos/src/{sorted,hash}/mod.rs` | drop those modules | 22 |
| `kermit-algos/src/optimiser/stats.rs` | drop `Const_` entries, then the policy | 22, 23 |
| `kermit-algos/src/optimiser/column_orders.rs` | delete | 23 |
| `kermit-algos/src/optimiser/{mod,planner,ordering,lexicographic,cardinality,cost_based}.rs` | plan freely | 23 |
| `kermit-algos/tests/cost_based_watdiv_plans.rs` | plan through accesses | 22, 23 |
| `kermit/src/db.rs` | renames; new modules; old pipeline deleted | 8, 11, 12, 20 |
| `kermit/src/db/catalog.rs` | create: `Catalog` | 11 |
| `kermit/src/db/lifecycle.rs` | create: `prepare` / `build` / `execute` | 12, 20, 23 |
| `kermit/src/db/database.rs` | rename, then delete | 8, 20 |
| `kermit/src/db/validation.rs` | rename; final checks | 8, 20 |
| `kermit/src/lib.rs` | `compute_join` over the conveniences | 20 |
| `kermit/src/execution.rs` | families: new API, then old API deleted | 8, 15, 19, 23 |
| `kermit/src/bench/run.rs` | per-query metrics | 8, 16, 17, 19 |
| `kermit/src/bench/ds.rs`, `kermit/src/bench/mod.rs` | `bench ds` from views; `Metric::Build`; space over a slice | 16 |
| `kermit/src/bench/workload.rs` | `PreparedWorkload` | 17 |
| `kermit/src/main.rs` | commands move over | 16, 17, 18, 19 |
| `kermit/src/options.rs` | `--column-orders` removed; one-argument `Planner::new` | 18, 23 |
| `kermit/src/bench_report.rs` | schema 5 | 24 |
| `kermit/tests/common/utils.rs` | harness on the new lifecycle; policy removed | 8, 14, 21, 23 |
| `kermit/tests/common/macros.rs` | policy parameter removed | 21 |
| `kermit/tests/common/mod.rs`, `kermit/tests/common/orders.rs`, `kermit/tests/common/oracle.rs` | create the helpers | 13 |
| `kermit/tests/all_orders.rs` | create | 13, 23 |
| `kermit/tests/join_tests.rs` | `AnyOrders` blocks removed | 21 |
| `kermit/tests/result_allocation.rs` | streamed `execute` | 14, 23 |
| `kermit/tests/tuple_allocation.rs` | view cells; `CopySpec` rename and removal | 7, 8, 22 |
| `kermit/tests/column_orders_any.rs` | rename, then delete | 8, 14 |
| `kermit/tests/column_orders_equivalence.rs` | helpers moved out, then delete | 13, 14 |
| `kermit/tests/cli_column_orders.rs` | delete | 17 |
| `kermit/tests/cli_bench_join_axes.rs` | v5 axes and functions | 17 |
| `kermit/tests/cli_join_tests.rs` | `{ds}/build`; schema 5 | 16, 24 |
| `kermit/tests/cli_query_errors.rs` | cyclic and reserved-name shapes removed | 18, 20 |
| `kermit/tests/translator_round_trip.rs`, `kermit/tests/job_translate.rs` | two-argument `validate_query` | 20 |
| `kermit/tests/{lubm_mini_oracle,lubm_cardinalities,job_generate,subject_position_constant,snap_generate,h2o_generate,watdiv_correctness}.rs` | the new harness; then one-argument `Planner::new` | 14, 23 |
| `python/kermit-lab/kermit_lab/{__init__,loader,defaults,presets,analysis}.py`, `python/kermit-lab/kermit_lab/drivers/main.py`, `python/kermit-lab/README.md` | v5 | 24 |
| `python/kermit-lab/tests/{test_loader,test_cli,test_frame,test_defaults,conftest,test_presets,test_render_all,test_analysis,test_contract}.py` | v5 | 24 |
| `CLAUDE.md`, `ARCHITECTURE.md`, `README.md`, `benchmarks/README.md` | docs | 25 |
| `docs/algorithms/`, `docs/data-structures/`, `docs/optimisers/` pages | docs | 26 |
| `docs/specs/{bench-report-schema,optimization-standard,benchmarking-architecture}.md`, `docs/benchmarks/*.md` | docs | 27 |

## Names contract

Every name, type and signature that one story defines for another. After this skeleton,
only `/flow-draft`'s manager edits this section.

**kermit-iters (Story 1; re-exported by `kermit_ds` from Story 2).** File
`kermit-iters/src/tuple_view.rs`. `kermit-iters/src/lib.rs` gains `mod tuple_view;` beside
its other private modules and `tuple_view::{Filter, TupleView}` in its one `pub use` block.

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Filter { /* private: constants: Vec<(usize, usize)>, equalities: Vec<(usize, usize)> */ }
impl Filter {
    pub fn none() -> Filter;
    /// Sorts and dedups both lists. Panics if an equality's first column is not below its second.
    pub fn new(constants: Vec<(usize, usize)>, equalities: Vec<(usize, usize)>) -> Filter;
    pub fn is_none(&self) -> bool;
    pub fn constants(&self) -> &[(usize, usize)];  // (column, value)
    pub fn equalities(&self) -> &[(usize, usize)]; // (earlier column, later column)
    pub fn matches(&self, row: &[usize]) -> bool;
}
impl std::fmt::Display for Filter {} // "-" when none; else "col{c}={value}" then "col{a}=col{b}", joined by ", "

#[derive(Clone, Debug)]
pub struct TupleView<'a> { /* private: source: &'a Tuples, filter: Filter, columns: Vec<usize> */ }
impl<'a> TupleView<'a> {
    /// Panics if a column, or a column the filter names, is not below `source.arity()`.
    pub fn new(source: &'a Tuples, filter: Filter, columns: Vec<usize>) -> TupleView<'a>;
    pub fn all(source: &'a Tuples) -> TupleView<'a>; // no filter, columns 0..source.arity()
    pub fn source(&self) -> &'a Tuples;
    pub fn filter(&self) -> &Filter;
    pub fn columns(&self) -> &[usize];
    pub fn arity(&self) -> usize;       // columns().len()
    pub fn source_rows(&self) -> usize; // source().len()
    pub fn is_identity(&self) -> bool;  // no filter and columns == 0..source.arity()
    pub fn matches(&self, row: RowId) -> bool;
    /// Writes row `row`'s output columns into `out`; panics if `out.len() != self.arity()`.
    pub fn write(&self, row: RowId, out: &mut [usize]);
    pub fn for_each(&self, visit: impl FnMut(&[usize])); // matching rows, source order, one scratch row
    pub fn count(&self) -> usize;                        // matching rows; source_rows() when unfiltered
    pub fn materialize(&self) -> Tuples;          // matching rows, source order, value_capacity == count * arity
    pub fn materialize_sorted(&self) -> Tuples;   // matching rows sorted lexicographically, exact capacity, one copy at every arity
    /// `rows` are matching source row ids: sorts them by output row (reordering the slice), then gathers once.
    pub fn gather_sorted(&self, rows: &mut [RowId]) -> Tuples;
}
```

A view with no columns is legal: `materialize()` returns `Tuples::from_flat(0, count, Vec::new())`.

**kermit-ds (Stories 2–7).**

```rust
// kermit-ds/src/relation.rs
pub trait Relation: JoinIterable + Projectable {
    const KEEPS_DUPLICATES: bool = false; // HashTrie: true; Configured<R, P> and BuiltWith<R, P>: R::KEEPS_DUPLICATES
    fn from_view(header: RelationHeader, view: TupleView<'_>) -> Self;
    // header, new, from_tuples, insert, insert_all: signatures unchanged
}
pub trait ConfigurableRelation: Relation {
    fn from_view_with_config(header: RelationHeader, config: Self::Config, view: TupleView<'_>) -> Self;
}
pub trait BuildModeRelation: Relation {
    fn from_view_with_build_mode(header: RelationHeader, mode: Self::BuildMode, view: TupleView<'_>) -> Self;
}
pub trait ConfiguredBuildModeRelation: ConfigurableRelation + BuildModeRelation {
    fn from_view_with_config_and_build_mode(
        header: RelationHeader, config: Self::Config, mode: Self::BuildMode, view: TupleView<'_>,
    ) -> Self;
}
```

- **Story 2 (bridge).** Each `from_view*` gets a default body that calls the matching
  `from_tuples*` with `view.materialize()`. `Configured` and `BuiltWith` override the
  defaults and pass the view on: `R::from_view_with_config`, `R::from_view_with_build_mode`
  and `R::from_view_with_config_and_build_mode`.
- **Story 7 (flip).** Every `from_view*` becomes required. Every `from_tuples*` becomes a
  provided method: `let tuples: Tuples = tuples.into();` then the matching `from_view*`
  with `TupleView::all(&tuples)`.
- **The arity panic,** on every path: `from_tuples: tuple arity {view.arity()} does not
  match header arity {header.arity()}`. It fires only when `view.source_rows() > 0`.
- **`kermit-ds/src/lib.rs`** re-exports `kermit_iters::{Filter, TupleView}` (Story 2).
- **`kermit-ds/src/morsel.rs`** stays crate-private. `scatter_rows` (Story 3) and
  `partition_rows` (Story 6) keep every other parameter and replace `tuples: &Tuples`
  with `view: &TupleView<'_>`.
  - Morsels are ranges of source row ids, and rows the filter rejects are skipped.
  - The `partition_of` closure sees output rows.
  - `RowPartition` ids and `Partitioned::origin` hold source row ids.
  - `Partitioned::tuples` is the matching output rows at exact capacity, so the copy no
    longer inherits `value_capacity`.

**kermit-algos (Stories 8–10, 22–23).**

- **Rename (Story 8).** `orient::IndexSpec` becomes `CopySpec`, and the facade exports
  `CopySpec`. Story 22 deletes it with `orient.rs`.

```rust
// kermit-algos/src/access.rs (Story 9)
pub const ACCESS_PREFIX: &str = "@"; // access ids "@0", "@1", …: the position in AccessQuery::accesses
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Access {
    pub relation: String,
    pub filter: Filter,
    pub columns: Vec<usize>,    // kept source columns, in source order
    pub variables: Vec<String>, // the variable each kept column binds, parallel to `columns`
}
#[derive(Clone, Debug)]
pub struct AccessQuery {
    pub query: JoinQuery,       // head unchanged; body[i] is atom "@{i}" over accesses[i].variables
    pub accesses: Vec<Access>,  // body atoms with at least one kept column, in body order
    pub checks: Vec<Access>,    // body atoms with none (only constants), in body order
}
/// Expects `rewrite_placeholders` to have run; constants must be `c<digits>`.
pub fn extract_accesses(query: &JoinQuery) -> Result<AccessQuery, RewriteError>;
```

- **`RewriteError`.** Until Story 22 it is `crate::const_rewrite::RewriteError`. Story 22
  moves it, unchanged (variant `BadAtom(String)`, Display text, `Error` impl), into
  `access.rs`. The facade path `kermit_algos::RewriteError` never changes.

```rust
// kermit-algos/src/index_plan.rs (Story 10)
pub const INDEX_ID_PREFIX: &str = "#"; // index ids "#0", "#1", …: first appearance in the executed query
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IndexSpec {
    pub relation: String,
    pub filter: Filter,
    pub columns: Vec<usize>, // source columns, in plan order
}
impl IndexSpec {
    pub fn view<'a>(&self, tuples: &'a Tuples) -> TupleView<'a>;
    /// Named `relation`, with `base`'s attributes at `columns`; positional of arity `columns.len()` when `base` is.
    pub fn header(&self, base: &RelationHeader) -> RelationHeader;
}
#[derive(Clone, Debug)]
pub struct IndexPlan {
    pub query: JoinQuery,        // head unchanged; body[j] is atom "#{id}" over its variables in plan order
    pub plan: QueryPlan,         // translated by variable name; plan.validate(&analyse(&query)) is Ok
    pub indexes: Vec<IndexSpec>, // indexes[id]; identical specs merged
}
pub fn plan_indexes(accesses: &AccessQuery, plan: &QueryPlan) -> IndexPlan; // `plan` was made for accesses.query
```

- **Facade.** Story 9 adds `access::{extract_accesses, Access, AccessQuery, ACCESS_PREFIX}`;
  Story 10 adds `index_plan::{plan_indexes, IndexPlan, IndexSpec, INDEX_ID_PREFIX}`.
- **Removed from the facade in Story 22:** `rewrite_atoms`, `ConstSpec`,
  `is_const_predicate`, `CONST_PREDICATE_PREFIX`, `rewrite_repeated_variables`,
  `SelectionSpec`, `ColumnEquality`, `is_selection_predicate`,
  `SELECTION_PREDICATE_PREFIX`, `orient`, `Oriented`, `CopySpec`, `is_index_predicate`,
  `INDEX_PREDICATE_PREFIX`, `TrieIterKind`, `HashTrieIterKind`, `SingletonTrieIter`,
  `SingletonHashTrieIter`, `EqualitySelectionTrieIter` and
  `EqualitySelectionHashTrieIter`. `CatalogStats::for_query` stops recording `Const_*`
  entries.
- **Removed in Story 23:** `ColumnOrderPolicy`, `check_attribute_order`,
  `CyclicAttributeOrder`, `Precedence::for_query`, `Planner::stored`,
  `Planner::column_orders`, `CatalogStats::column_orders` and `CatalogStats::is_pinned`.
  Story 23 also changes two signatures and adds one constructor:

```rust
impl Planner { pub fn new(optimiser: Box<dyn QueryOptimiser>) -> Planner; }
impl CatalogStats {
    pub fn for_query(query: &JoinQuery, stats_of: impl Fn(&str) -> Option<RelationStats>) -> CatalogStats;
}
impl Precedence { pub fn free(num_vars: usize) -> Precedence; } // no edges; the three optimisers call it where they called for_query
```

**kermit::db (Stories 8, 11, 12, 20).**

- **Rename (Story 8).** The private `validation::{prepare, Prepared}` become
  `{check_and_rewrite, Rewritten}`. Story 20 deletes them.

```rust
// kermit/src/db/catalog.rs (Story 11); db.rs: `mod catalog; pub use catalog::Catalog;`
pub struct Catalog { /* private */ }
impl Catalog {
    /// Keyed by header name; a later duplicate name replaces an earlier one.
    /// Statistics: tuple counts are row counts; column-distinct counts only at ColumnDistinct.
    pub fn new(relations: Vec<(RelationHeader, Tuples)>, level: StatisticsLevel) -> Catalog;
    pub fn header(&self, name: &str) -> Option<&RelationHeader>;
    pub fn tuples(&self, name: &str) -> Option<&Tuples>;
    pub fn statistics(&self, name: &str) -> Option<&RelationStats>;
    pub fn level(&self) -> StatisticsLevel;
    pub fn names(&self) -> impl Iterator<Item = &str>;
}
impl RelationArities for Catalog {}

// kermit/src/db/lifecycle.rs (Story 12); db.rs: `mod lifecycle; pub use lifecycle::{build, build_with, execute, prepare, Indexes, Prepared};`
pub struct Prepared { /* private */ }
impl Prepared {
    pub fn query(&self) -> &JoinQuery;      // the executed query (IndexPlan::query)
    pub fn plan(&self) -> &QueryPlan;       // IndexPlan::plan
    pub fn indexes(&self) -> &[IndexSpec];  // IndexPlan::indexes
    pub fn checks(&self) -> &[Access];      // AccessQuery::checks
    pub fn head_len(&self) -> usize;
    pub fn relations(&self) -> Vec<&str>;   // distinct relations read (indexes and checks), first appearance
}
pub fn prepare(catalog: &Catalog, query: &JoinQuery, planner: &Planner) -> Result<Prepared, JoinError>;
pub struct Indexes<R> { /* private */ }
impl<R> Indexes<R> {
    pub fn get(&self, id: &str) -> Option<&R>;
    pub fn iter(&self) -> impl Iterator<Item = (&IndexSpec, &R)>; // in id order
    pub fn multiplicity(&self) -> usize;                          // product of the checks' counts, each clamped to 1 unless R::KEEPS_DUPLICATES
    pub fn rows(&self) -> usize;                                  // rows the build copied: the sum of the index views' counts
}
pub fn build_with<R: Relation>(
    catalog: &Catalog, prepared: &Prepared, construct: impl FnMut(RelationHeader, TupleView<'_>) -> R,
) -> Indexes<R>;
pub fn build<R: Relation>(catalog: &Catalog, prepared: &Prepared) -> Indexes<R>; // build_with(…, R::from_view)
/// Debug-asserts that `indexes` were built from `prepared` (their specs equal `prepared.indexes()`).
/// Emits nothing when the multiplicity is 0; emits the empty head row `multiplicity` times when no atom is left.
pub fn execute<R, JA>(prepared: &Prepared, indexes: &Indexes<R>, emit: impl FnMut(&[usize]))
where R: JoinIterable, JA: JoinAlgo<R>;
```

- **`prepare` until Stories 20 and 23.** It calls `validate_query(query, catalog,
  ColumnOrderPolicy::Any)` until Story 20, and `CatalogStats::for_query(…,
  ColumnOrderPolicy::Any, …)` until Story 23.
- **Story 20's final library surface:**

```rust
pub fn validate_query(query: &JoinQuery, relations: &(impl RelationArities + ?Sized)) -> Result<(), JoinError>;
pub fn lftj_join_for_each<R, JA>(catalog: &Catalog, query: JoinQuery, planner: &Planner, emit: impl FnMut(&[usize])) -> Result<(), JoinError>
where R: Relation + TrieIterable, JA: JoinAlgo<R>;
pub fn lftj_join<R, JA>(catalog: &Catalog, query: JoinQuery, planner: &Planner) -> Result<Vec<Vec<usize>>, JoinError>
where R: Relation + TrieIterable, JA: JoinAlgo<R>;
pub fn hash_join_for_each<R>(catalog: &Catalog, query: JoinQuery, planner: &Planner, emit: impl FnMut(&[usize])) -> Result<(), JoinError>
where R: Relation + HashTrieIterable;
pub fn hash_join<R>(catalog: &Catalog, query: JoinQuery, planner: &Planner) -> Result<Vec<Vec<usize>>, JoinError>
where R: Relation + HashTrieIterable;
```

- **`JoinError` after Story 20** keeps `NonVariableHeadTerm`, `RepeatedHeadVariable`,
  `UnknownRelation`, `ArityMismatch`, `MalformedConstant(RewriteError)`,
  `UnboundHeadVariable` and `MissingStatistics`, with their Display texts unchanged. It
  drops `ReservedRelationName`, `CyclicAttributeOrder` and `MissingIndex`.
- **Story 20 deletes** `Database`, `JoinFamily`, `SortedFamily`, `HashFamily`,
  `build_index` and `index_header`.

**The binary (Stories 15–19, 24).**

```rust
// kermit/src/execution.rs
pub trait RelationFamily {
    fn build_index(&self, header: RelationHeader, view: TupleView<'_>) -> Self::Rel; // Story 15: the one construction site
}
pub trait SortedTrieRelation {
    fn build_with(header: RelationHeader, build: Self::BuildMode, view: TupleView<'_>) -> Self; // Story 15: was Tuples
}
pub trait ExecutionFamily: RelationFamily {
    const JOIN_MUTATES: bool; // unchanged
    fn build_indexes(&self, catalog: &Catalog, prepared: &Prepared) -> Indexes<Self::Rel>; // Story 15, provided: build_with over build_index
    fn execute<S: FnMut(&[usize])>(&self, prepared: &Prepared, indexes: &Indexes<Self::Rel>, emit: S); // Story 15
    fn count_rows(&self, prepared: &Prepared, indexes: &Indexes<Self::Rel>) -> u64; // Story 15, provided: counts execute's rows through black_box
}
pub(crate) fn read_relation(path: &Path) -> anyhow::Result<(RelationHeader, Tuples)>; // Story 15: was private
```

- **Removed in Story 19:** `RelationFamily::{build_relation, load, load_with_tuples}` and
  `ExecutionFamily::{Engine, build, build_from_tuples, relations, join_for_each, count,
  required_indexes, add_index, clear_indexes, indexes}`. The constructors become
  `TrieLftj::new(build)` and `HashHtj::new(config, build)`, with no `Planner`.

```rust
// kermit/src/bench/workload.rs (Story 17)
pub struct PreparedWorkload {
    pub workload: Workload,
    pub catalog: Catalog,
    pub prepared: Vec<Prepared>, // parallel to workload.queries
}
impl Workload {
    /// Validates every query from headers, reads only the relations the queries name into one
    /// Catalog at planner.required_statistics(), and prepares each query once.
    pub fn prepare(self, planner: &Planner) -> anyhow::Result<PreparedWorkload>;
}
```

- **`Workload::validate`** is removed (Story 17). `dispatch_run_bench` and `run_benchmark`
  take `workload: &PreparedWorkload` in place of `&Workload`, with the other parameters
  and the return types unchanged (Story 17).
- **`kermit/src/bench/mod.rs` (Story 16).**
  - `Metric::Insertion` becomes `Metric::Build`, with clap value `build`. The default
    `--metrics` of `bench join`, `bench ds` and `bench run` becomes
    `[Metric::Build, Metric::Iteration, Metric::Space]`.
  - `add_space_bench` takes `relations: &[&R]` in place of one `&R` and sums their
    `heap_size_bytes()` inside the `iter_custom` loop; its other parameters are unchanged.
- **Criterion function ids.** `bench run` and `bench join` emit, per query, `build`,
  `iteration`, `end_to_end` and `space` (Story 17). `bench ds` emits `{ds}/build`,
  `{ds}/iteration`, `{ds}/end_to_end` and `{ds}/space` (Story 16).
- **Report (Story 17).**
  - The `column_orders` axis is removed. A new `index_rows` axis (an integer) holds
    `Indexes::rows()`.
  - `tuples` becomes the sum of `Catalog::tuples(name).len()` over `Prepared::relations()`.
  - There is one metadata line per index, labelled `index`, with the value
    `{id} {relation} filter {filter} columns {columns:?}: {rows} rows, {bytes} bytes`.
  - Story 24 sets `REPORT_SCHEMA_VERSION = 5`.
- **CLI (Story 18).** `--column-orders` is removed from `PlannerArgs`. `instantiate`
  returns `Planner::new(optimiser.instantiate(), ColumnOrderPolicy::Any)` until Story 23,
  then `Planner::new(optimiser.instantiate())`.

**Tests (Stories 13, 14, 21).**

```rust
// kermit/tests/common/orders.rs (Story 13)
pub struct FixedOrder(pub Vec<String>); // binds variables in this order (names); panics on a name the query lacks
impl QueryOptimiser for FixedOrder {}   // plan(): positions through canonical_names(query)
pub fn every_order(variables: &[String]) -> Vec<Vec<String>>; // every permutation
// kermit/tests/common/oracle.rs (Story 13): Lcg, random_relations, oracle and match_atoms,
// moved verbatim from column_orders_equivalence.rs with their signatures unchanged

// kermit/tests/common/utils.rs (Story 14)
pub type Inputs = BTreeMap<String, (RelationHeader, Tuples)>; // unchanged
/// Replaces load_parquet_relations: reads each `<name>.parquet` in `dir` once, flat.
pub fn load_parquet_inputs<'n>(dir: &Path, names: impl IntoIterator<Item = &'n str>) -> Inputs;
pub fn catalog_of(inputs: &Inputs, level: StatisticsLevel) -> Catalog;
pub trait JoinEntry<R> {
    fn join(catalog: &Catalog, query: JoinQuery, planner: &Planner) -> Result<Vec<Vec<usize>>, JoinError>;
    fn count(catalog: &Catalog, query: JoinQuery, planner: &Planner) -> Result<u64, JoinError>;
}
/// Joins through `JA` collected and counted through the streamed path, asserting they agree;
/// returns the rows and the index specs the query built.
pub fn join_under_planner<R: Relation, JA: JoinEntry<R>>(
    catalog: &Catalog, query: JoinQuery, planner: &Planner,
) -> Result<(Vec<Vec<usize>>, Vec<IndexSpec>), JoinError>;
```

- **`kermit/tests/common/mod.rs`** gains `pub mod orders; pub mod oracle;` (Story 13).
- **`JoinEntry::database`** and **`load_parquet_relations`** are removed in Story 14, which
  updates their seven callers.
- **`test_join`** keeps its `P: ColumnOrderProvider` parameter, ignored, until Story 21
  removes it, together with the `policy` argument of `define_multiway_join_test!` and of
  every suite macro.

**kermit-lab (Story 24).** All of this follows the spec's Amendment 1 §10.
- `SCHEMA_VERSION = 5` (`kermit_lab/__init__.py`).
- `PER_QUERY_BUILD_SCHEMA = 5` (`loader.py`).
- `TIME_PHASES = ("build", "insertion", "copies", "iteration", "end_to_end")`.
- Every default phase becomes `"build"`, in `presets.py`, `analysis.py` and
  `drivers/main.py`.
- `JOIN_AXIS_DEFAULTS` applies only to rows whose `schema_version` is below 5
  (`defaults.py`).

**Messages tests match.**
- The arity panic above.
- `RewriteError`'s Display text.
- The Display texts of every `JoinError` variant that stays.

None of these change.

## Stories

- **Story 1** — `Filter` and `TupleView` in kermit-iters.

  Adds the borrowed view every later build reads: a flat `Tuples`, a closed filter of
  constant and column equalities, and output columns. That includes `materialize` (one
  exact-size copy) and the sorted helpers that let the sorted tries copy once at every
  arity. No caller changes yet.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-iters/src/tuple_view.rs`, `kermit-iters/src/lib.rs`
  - **Interfaces:** exposes `Filter` and `TupleView` (Names contract: kermit-iters) to Stories 2–7 and 9–12; uses `Tuples`, `RowId`
  - **Verify:** `cargo test -p kermit-iters`
  - **Review:** deep
  - **Reading:** spec "`TupleView` and `Filter`", Amendment 1 §9 and §14; `kermit-iters/src/tuples.rs:1-255`; `kermit-iters/src/lib.rs:40-70`

- **Story 2** — View constructors on the relation traits, bridged.

  Adds `from_view` and the three `from_view_with_*` constructors to the relation traits.
  Their default bodies materialise the view and call today's tuple constructors, so every
  structure works unchanged. `Configured` and `BuiltWith` pass views through. Also adds
  `KEEPS_DUPLICATES` (`true` on HashTrie) and re-exports `Filter` and `TupleView` from
  kermit-ds.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-ds/src/relation.rs`, `kermit-ds/src/configured.rs`, `kermit-ds/src/built_with.rs`, `kermit-ds/src/lib.rs`, `kermit-ds/src/ds/hash_trie/implementation.rs`
  - **Interfaces:** exposes `Relation::{from_view, KEEPS_DUPLICATES}`, the `from_view_with_*` constructors and `kermit_ds::{Filter, TupleView}` to Stories 3–7, 12 and 15; uses Story 1's `TupleView::materialize`
  - **Verify:** `cargo test -p kermit-ds`
  - **Review:** deep
  - **Reading:** spec "Building from views (`kermit-ds`)", Amendment 1 §6; `kermit-ds/src/relation.rs:178-376`; `kermit-ds/src/configured.rs:80-160`; `kermit-ds/src/built_with.rs:70-130`; `kermit-ds/src/lib.rs:20-64`; `kermit-ds/src/ds/hash_trie/implementation.rs:448-495`

- **Story 3** — TreeTrie builds from views.

  `serial` builds from `materialize_sorted`. `parallel:N` scatters the matching source rows
  by their first output key, and each worker calls `gather_sorted` on its partition; that
  is the one copy at every arity. `morsel::scatter_rows` and `first_key_splitters` read the
  view, and `from_tuples*` delegates through `TupleView::all`. Tests: builds from a
  filtered, permuted view equal the build from that view's materialised batch, array for
  array, and `parallel:N` still equals `serial`.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-ds/src/ds/tree_trie/implementation.rs`, `kermit-ds/src/morsel.rs`
  - **Interfaces:** uses Story 1's `TupleView` and Story 2's view constructors; changes `morsel::scatter_rows` to take a view (Names contract: kermit-ds), which Story 6 keeps
  - **Verify:** `cargo test -p kermit-ds --lib tree_trie`
  - **Review:** deep
  - **Reading:** spec "Building from views (`kermit-ds`)", Amendment 1 §14; `kermit-ds/src/ds/tree_trie/implementation.rs:140-380`; `kermit-ds/src/ds/tree_trie/implementation.rs:500-560`; `kermit-ds/src/ds/tree_trie/implementation.rs:650-670`; `kermit-ds/src/morsel.rs:60-175`; `kermit-ds/src/morsel.rs:426-560`

- **Story 4** — ColumnTrie builds from views.

  Both modes build from `materialize_sorted`: `bulk` through `from_sorted`, `incremental`
  through `from_sorted_by_insertion`. `from_tuples*` delegates through `TupleView::all`.
  Tests: a filtered, permuted view builds the same arrays as its materialised batch, in
  both modes.

  - **Developer:** junior-dev
  - **Files:** `kermit-ds/src/ds/column_trie/implementation.rs`
  - **Interfaces:** uses Story 1's `materialize_sorted` and Story 2's `from_view_with_build_mode`; exposes nothing
  - **Verify:** `cargo test -p kermit-ds --lib column_trie`
  - **Review:** light (one file, no landmine)
  - **Reading:** spec "Building from views (`kermit-ds`)"; `kermit-ds/src/ds/column_trie/implementation.rs:280-520`; `kermit-ds/src/ds/column_trie/implementation.rs:740-860`

- **Story 5** — HashTrie `bulk` and `incremental` build from views.

  `bulk` and `incremental` materialise the view as the trie's own buffer, then run
  Algorithm 2 or `insert_at` as today. The root is sized by `view.count()`. An empty source
  becomes `Tuples::new(header.arity())`. The arity panic keeps its text. The partitioning
  arms still materialise and call the tuple path, which Story 6 replaces. Tests: views
  against materialised batches through `identity::assert_same_trie`.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-ds/src/ds/hash_trie/implementation.rs`
  - **Interfaces:** uses Stories 1–2; exposes nothing
  - **Verify:** `cargo test -p kermit-ds --lib hash_trie`
  - **Review:** deep
  - **Reading:** spec "Building from views (`kermit-ds`)", Amendment 1 §6; `kermit-ds/src/ds/hash_trie/implementation.rs:130-260`; `kermit-ds/src/ds/hash_trie/implementation.rs:440-740`; `kermit-ds/src/ds/hash_trie/implementation.rs:1030-1050`; `kermit-ds/src/ds/hash_trie/implementation.rs:1250-1280`; `kermit-ds/src/ds/hash_trie/implementation.rs:1515-1525`; `kermit-ds/src/ds/hash_trie/identity.rs:110-330`

- **Story 6** — HashTrie's partitioning builds read views.

  `morsel::partition_rows` reads the view: the counting pass checks `matches`, the scatter
  pass writes output rows, `origin` holds source row ids, and the copy is exact-size. The
  `value_capacity` inheritance is dropped. `radix::fill_root`, `parallel::fill_root` and
  `parallel::fill_presized_root` take a view rather than consuming a batch, and the links
  are sized by `view.count()`. The capacity tests now expect exact sizes, and
  mode-equivalence tests are added over filtered views.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-ds/src/morsel.rs`, `kermit-ds/src/ds/hash_trie/radix.rs`, `kermit-ds/src/ds/hash_trie/parallel.rs`, `kermit-ds/src/ds/hash_trie/implementation.rs`
  - **Interfaces:** uses Stories 1–3 and 5; changes `morsel::partition_rows` to take a view (Names contract: kermit-ds); exposes nothing
  - **Verify:** `cargo test -p kermit-ds --lib`
  - **Review:** deep
  - **Reading:** spec "Building from views (`kermit-ds`)"; `kermit-ds/src/morsel.rs:170-420`; `kermit-ds/src/morsel.rs:600-841`; `kermit-ds/src/ds/hash_trie/radix.rs:100-240`; `kermit-ds/src/ds/hash_trie/radix.rs:380-460`; `kermit-ds/src/ds/hash_trie/parallel.rs:130-420`; `kermit-ds/src/ds/hash_trie/parallel.rs:460-570`; `kermit-ds/src/ds/hash_trie/parallel.rs:700-730`; `kermit-ds/src/ds/hash_trie/parallel.rs:820-830`; `kermit-ds/src/ds/hash_trie/parallel.rs:1150-1190`; `kermit-ds/src/ds/hash_trie/implementation.rs:540-740`

- **Story 7** — Views become the one build path.

  `from_view*` becomes required and every `from_tuples*` a provided method over
  `TupleView::all`, which removes the bridge bodies and each structure's own
  `from_tuples*`. `read_csv`'s comment stops claiming that HashTrie keeps the reader's
  buffer. `tuple_allocation.rs` gains view-build cells: a filtered view builds every
  structure and mode without allocating per input row.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-ds/src/relation.rs`, `kermit-ds/src/configured.rs`, `kermit-ds/src/built_with.rs`, `kermit-ds/src/ds/tree_trie/implementation.rs`, `kermit-ds/src/ds/column_trie/implementation.rs`, `kermit-ds/src/ds/hash_trie/implementation.rs`, `kermit/tests/tuple_allocation.rs`
  - **Interfaces:** makes Story 2's `from_view*` the required constructors and every `from_tuples*` a provided one; every later build goes through them
  - **Verify:** `cargo test -p kermit --test tuple_allocation`
  - **Review:** deep
  - **Reading:** spec "Building from views (`kermit-ds`)", "Testing"; `kermit-ds/src/relation.rs:230-376`; `kermit-ds/src/relation.rs:475-520`; `kermit-ds/src/relation.rs:600-627`; `kermit/tests/tuple_allocation.rs:1-503`

- **Story 8** — Free the names the new pipeline takes.

  This is a pure rename with no change in behaviour. `kermit_algos::IndexSpec` becomes
  `CopySpec` in `orient.rs`, in the facade and at every use. The private
  `validation::{prepare, Prepared}` become `{check_and_rewrite, Rewritten}`.

  - **Developer:** junior-dev
  - **Files:** `kermit-algos/src/orient.rs`, `kermit-algos/src/lib.rs`, `kermit/src/db.rs`, `kermit/src/db/database.rs`, `kermit/src/db/validation.rs`, `kermit/src/execution.rs`, `kermit/src/bench/run.rs`, `kermit/tests/common/utils.rs`, `kermit/tests/column_orders_any.rs`, `kermit/tests/tuple_allocation.rs`
  - **Interfaces:** frees `IndexSpec` for Story 10 and `prepare` / `Prepared` for Story 12 (Names contract: kermit-algos, kermit::db)
  - **Verify:** `cargo test -p kermit-algos -p kermit --no-run`
  - **Review:** light (mechanical rename, no landmine)
  - **Reading:** spec Amendment 1 §2; `kermit-algos/src/orient.rs:30-80`; `kermit-algos/src/lib.rs:44-70`; `kermit/src/db/validation.rs:260-345`; `kermit/src/db.rs:20-45`

- **Story 9** — Access extraction.

  `extract_accesses` walks each body atom's terms after the placeholder rewrite. A
  variable's first occurrence is a kept column, a repeat becomes an equality with that
  column, and a constant becomes a constant filter. Atoms with no kept column become
  checks. The access query names its atoms `@<k>`. Unit tests cover constants, repeats,
  mixes, triple repeats, all-constant atoms and malformed constants.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-algos/src/access.rs`, `kermit-algos/src/lib.rs`
  - **Interfaces:** exposes `Access`, `AccessQuery`, `extract_accesses` and `ACCESS_PREFIX` to Stories 10, 12, 20 and 22; uses Story 1's `Filter` and `const_rewrite::RewriteError`
  - **Verify:** `cargo test -p kermit-algos --lib access`
  - **Review:** deep
  - **Reading:** spec "Preparing a query" steps 2–3, Amendment 1 §1 and §5; `kermit-algos/src/const_rewrite.rs:1-125`; `kermit-algos/src/selection_rewrite.rs:1-140`; `kermit-algos/src/placeholder_rewrite.rs:1-60`; `kermit-algos/src/analysis.rs:1-110`

- **Story 10** — Index planning.

  `plan_indexes` puts each access's kept columns in plan order, merges identical specs, and
  numbers them `#<k>` by first appearance. It rebuilds the query over those ids, with each
  atom's variables in plan order, and translates the plan by variable name, as
  `orient` does today. Unit tests: self-joins merge, a reversed order gets its own index,
  and the translated plan validates against the new query.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-algos/src/index_plan.rs`, `kermit-algos/src/lib.rs`
  - **Interfaces:** exposes `IndexSpec`, `IndexPlan`, `plan_indexes` and `INDEX_ID_PREFIX` to Stories 12 and 22; uses Story 9's `AccessQuery`, `analyse`, `canonical_names`, `QueryPlan`
  - **Verify:** `cargo test -p kermit-algos --lib index_plan`
  - **Review:** deep
  - **Reading:** spec "Preparing a query" step 6, Amendment 1 §1; `kermit-algos/src/orient.rs:85-230`; `kermit-algos/src/analysis.rs:1-110`; `kermit-algos/src/optimiser/plan.rs:1-110`

- **Story 11** — The catalog.

  `Catalog` holds each relation's header and flat `Tuples`, keyed by name. It gathers
  statistics once: row counts always, and column-distinct counts at `ColumnDistinct`
  through `distinct_per_column`. It implements `RelationArities`, so `validate_query` reads
  it. Unit tests: lookups, statistics at both levels, and a duplicate name replacing the
  earlier relation.

  - **Developer:** junior-dev
  - **Files:** `kermit/src/db/catalog.rs`, `kermit/src/db.rs`
  - **Interfaces:** exposes `Catalog` to Stories 12–20; uses `RelationStats`, `StatisticsLevel`, `distinct_per_column`, `RelationArities`
  - **Verify:** `cargo test -p kermit --lib db::catalog`
  - **Review:** light (a new type over existing statistics helpers, no landmine)
  - **Reading:** spec "Preparing a query" step 4, Amendment 1 §12; `kermit/src/db/database.rs:1-140`; `kermit-algos/src/optimiser/stats.rs:25-110`; `kermit/src/db/validation.rs:220-260`

- **Story 12** — Prepare, build and execute.

  `prepare` validates, rewrites placeholders, extracts accesses, gates on the statistics
  level, gathers statistics (filtered accesses scan through their view), plans and calls
  `plan_indexes`. `build_with` builds each spec through its view and counts each check into
  the multiplicity. `execute` runs `JoinAlgo::join_for_each`, emits the head prefix once per
  unit of multiplicity, and handles a query with no atoms left. Unit tests run constants,
  repeats, placeholders and all-constant atoms (multiplicity on HashTrie, not on the
  sorted tries) against hand-computed answers, plus errors and `MissingStatistics`.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/db/lifecycle.rs`, `kermit/src/db.rs`
  - **Interfaces:** exposes `Prepared`, `prepare`, `Indexes`, `build_with`, `build` and `execute` to Stories 13–20; uses Stories 2, 9, 10 and 11
  - **Verify:** `cargo test -p kermit --lib db::lifecycle`
  - **Review:** deep
  - **Reading:** spec "Preparing a query", "Building and executing (`kermit::db`)", Amendment 1 §1, §6 and §7; `kermit/src/db.rs:143-330`; `kermit/src/db/validation.rs:320-345`; `kermit-algos/src/optimiser/stats.rs:110-190`; `kermit-algos/src/optimiser/planner.rs:1-60`

- **Story 13** — The all-orders test.

  `all_orders.rs` runs each of the 16 patterns' query shapes, the constant cases and the
  six queries of `column_orders_any.rs` over seeded random relations.
  - Each query runs under every variable order (at most 120, `every_order`), forced by
    `FixedOrder`.
  - It drives `kermit::db::{prepare, build, execute}` directly, not the harness, on
    `TreeTrie` and `ColumnTrie` under LFTJ and `HashTrie` under HashTriejoin.
  - Rows are compared with the nested-loop oracle.

  The oracle and its generators move to `common/oracle.rs`, and
  `column_orders_equivalence.rs` imports them until Story 14 deletes it.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/tests/all_orders.rs`, `kermit/tests/common/orders.rs`, `kermit/tests/common/oracle.rs`, `kermit/tests/common/mod.rs`, `kermit/tests/column_orders_equivalence.rs`
  - **Interfaces:** exposes `FixedOrder`, `every_order` and the moved oracle helpers (Names contract: tests) to Stories 14 and 23; uses Story 12
  - **Verify:** `cargo test -p kermit --test all_orders`
  - **Review:** deep
  - **Reading:** spec "Testing", Amendment 1 §11; `kermit/tests/column_orders_equivalence.rs:1-307`; `kermit/tests/column_orders_any.rs:1-210`; `kermit/tests/common/macros.rs:30-445`; `kermit/tests/common/mod.rs:1-20`

- **Story 14** — The test harness runs the new lifecycle.

  `JoinEntry` joins and counts over a `Catalog` through `prepare`, `build` and `execute`.
  `catalog_of` and `load_parquet_inputs` replace `JoinEntry::database` and
  `load_parquet_relations`, and `join_under_planner` takes a catalog (Names contract:
  tests). The seven callers move over: `h2o_generate`, `job_generate`,
  `lubm_cardinalities`, `lubm_mini_oracle`, `snap_generate`, `subject_position_constant`
  and `watdiv_correctness`. `test_join` keeps its policy parameter, ignored, so the macros
  compile unchanged. `result_allocation.rs` measures the streamed `execute` over prebuilt
  indexes. `column_orders_any.rs` and `column_orders_equivalence.rs` are deleted; Story 13
  carried their queries and oracle over.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/tests/common/utils.rs`, `kermit/tests/result_allocation.rs`, `kermit/tests/h2o_generate.rs`, `kermit/tests/job_generate.rs`, `kermit/tests/lubm_cardinalities.rs`, `kermit/tests/lubm_mini_oracle.rs`, `kermit/tests/snap_generate.rs`, `kermit/tests/subject_position_constant.rs`, `kermit/tests/watdiv_correctness.rs`, `kermit/tests/column_orders_any.rs`, `kermit/tests/column_orders_equivalence.rs`
  - **Interfaces:** exposes the reworked harness (Names contract: tests) to Stories 21 and 23; uses Stories 12–13
  - **Verify:** `cargo test -p kermit --test join_tests`
  - **Review:** deep
  - **Reading:** spec "Testing"; `kermit/tests/common/utils.rs:1-350`; `kermit/tests/common/macros.rs:1-30`; `kermit/tests/common/macros.rs:445-490`; `kermit/tests/result_allocation.rs:1-260`; `kermit/tests/lubm_mini_oracle.rs:100-260`; `kermit/tests/watdiv_correctness.rs:1-100`

- **Story 15** — Families build indexes and execute prepared queries.

  The new family API is added beside the old. `RelationFamily::build_index` is the one
  construction site, honouring Layout, Config and BuildMode. `SortedTrieRelation::build_with`
  takes a view. `ExecutionFamily` gains `build_indexes`, `execute` (LFTJ or HashTriejoin
  through `kermit::db::execute`) and `count_rows`. `read_relation` becomes `pub(crate)`. The
  old methods stay until Story 19. Tests: every family builds the same relation through
  `build_index` as through `build_relation`, and `count_rows` agrees with the old `count`.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/execution.rs`
  - **Interfaces:** exposes `RelationFamily::build_index`, `ExecutionFamily::{build_indexes, execute, count_rows}` and `read_relation` to Stories 16–19; uses Stories 2, 11 and 12
  - **Verify:** `cargo test -p kermit --bin kermit execution`
  - **Review:** deep
  - **Reading:** spec "The binary (`kermit/src`)", Amendment 1 §3; `kermit/src/execution.rs:90-170`; `kermit/src/execution.rs:340-900`; `kermit/src/execution.rs:1240-1620`

- **Story 16** — `bench ds` builds from views; the `build` metric.

  `bench ds` reads its relation with `read_relation`. Its `build` and `end_to_end` build
  through `build_index` over `TupleView::all`, and its functions become
  `{ds}/build` and so on. Every timed routine returns what it built, so `end_to_end` no
  longer frees inside the timer (decision 6). `Metric::Insertion` becomes `Metric::Build`,
  with value `build`, across the CLI defaults and `run.rs`'s checks. `add_space_bench` takes
  a slice, and `run.rs`'s calls pass one-element slices. `run.rs` keeps its old metric
  blocks until Story 17.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/bench/ds.rs`, `kermit/src/bench/mod.rs`, `kermit/src/bench/run.rs`, `kermit/src/main.rs`, `kermit/tests/cli_join_tests.rs`
  - **Interfaces:** exposes `Metric::Build` and the slice-taking `add_space_bench` (Names contract: the binary) to Story 17; uses Story 15
  - **Verify:** `cargo test -p kermit --test cli_join_tests`
  - **Review:** deep
  - **Reading:** spec "CLI, reports and kermit-lab", Amendment 1 §8; `kermit/src/bench/ds.rs:1-247`; `kermit/src/bench/mod.rs:30-120`; `kermit/src/main.rs:225-345`; `kermit/tests/cli_join_tests.rs:480-680`

- **Story 17** — `bench run` and `bench join` build per query.

  `Workload::prepare` returns a `PreparedWorkload`, which `bench run`'s handler calls once
  per benchmark, before its loop over cells, and `bench join` once for its query. In
  `run_benchmark`, per query:
  - `build` times `build_indexes` with a no-op setup and `BatchSize::PerIteration`.
  - `iteration` times `execute` over indexes built once, or over fresh builds in untimed
    setup under `JOIN_MUTATES`.
  - `end_to_end` times a build plus K executes.
  - `space` measures every index together, and `--verify` counts with `count_rows`.

  Every timed routine returns what it built, so freeing is never timed (decision 6). The
  axes drop `column_orders` and add `index_rows`; `tuples` is per query; and an `index`
  metadata line is written per index. `bench join --output` stays on the old runner until
  Story 18. `cli_column_orders.rs` is deleted.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/bench/run.rs`, `kermit/src/bench/workload.rs`, `kermit/src/main.rs`, `kermit/tests/cli_bench_join_axes.rs`, `kermit/tests/cli_column_orders.rs`
  - **Interfaces:** exposes `PreparedWorkload` and `Workload::prepare` (Names contract: the binary) to Story 18; uses Stories 15–16
  - **Verify:** `cargo test -p kermit --test cli_bench_join_axes`
  - **Review:** deep
  - **Reading:** spec "The binary (`kermit/src`)", "CLI, reports and kermit-lab", Amendment 1 §4, §8 and §9; `kermit/src/bench/run.rs:1-530`; `kermit/src/bench/workload.rs:1-248`; `kermit/src/main.rs:795-940`; `kermit/tests/cli_bench_join_axes.rs:1-80`

- **Story 18** — `kermit join` and `bench join --output` share the prepared workload, and `--column-orders` goes.

  `kermit join` and `bench join --output` go through `Workload::adhoc(…).prepare(…)`, then
  `build_indexes` and `execute` into the CSV sink. `bench join` reads its relations once
  for both the output and the benchmark. `validate_query_files` goes, since `prepare`
  validates. `--column-orders` leaves `PlannerArgs`, and `instantiate` passes
  `ColumnOrderPolicy::Any` until Story 23. `cli_query_errors.rs` loses its cyclic-order
  shape, a query that now runs.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/main.rs`, `kermit/src/options.rs`, `kermit/tests/cli_query_errors.rs`
  - **Interfaces:** uses Story 17's `PreparedWorkload`; removes `--column-orders` (Names contract: the binary)
  - **Verify:** `cargo test -p kermit --test cli_query_errors`
  - **Review:** deep
  - **Reading:** spec "The binary (`kermit/src`)", Amendment 1 §4; `kermit/src/main.rs:95-130`; `kermit/src/main.rs:415-725`; `kermit/src/main.rs:795-835`; `kermit/src/options.rs:1-45`; `kermit/src/options.rs:2045-2079`; `kermit/tests/cli_query_errors.rs:270-330`

- **Story 19** — Families lose the old engine API.

  Removes `build_relation`, `load` and `load_with_tuples`, plus the engine and copy methods
  of `ExecutionFamily`. `TrieLftj` and `HashHtj` stop holding a `Planner`. The dispatch
  arms in `run.rs` and `main.rs` construct them without one. `execution.rs`'s tests drop
  the old methods' cases.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/execution.rs`, `kermit/src/bench/run.rs`, `kermit/src/main.rs`
  - **Interfaces:** removes the old family API (Names contract: the binary); uses Stories 15–18
  - **Verify:** `cargo test -p kermit --bin kermit`
  - **Review:** deep
  - **Reading:** spec "Retired", Amendment 1 §3; `kermit/src/execution.rs:340-900`; `kermit/src/execution.rs:895-2199`; `kermit/src/bench/run.rs:488-530`; `kermit/src/main.rs:585-625`

- **Story 20** — The old `kermit::db` pipeline goes.

  Deletes `Database`, the copy protocol, `JoinFamily` and its families, `run_join`,
  `plan_query`, `build_index` and `index_header`. The conveniences take a `Catalog` and run
  `prepare`, `build` and `execute`.
  - `validate_query(query, relations)` keeps its check order and checks constants through
    `extract_accesses`.
  - `JoinError` drops three variants.
  - `compute_join` builds a `Catalog`.
  - `db.rs`'s tests are rewritten for the new conveniences.
  - The reserved-name shape leaves `cli_query_errors.rs`.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/db.rs`, `kermit/src/db/database.rs`, `kermit/src/db/validation.rs`, `kermit/src/db/lifecycle.rs`, `kermit/src/lib.rs`, `kermit/tests/translator_round_trip.rs`, `kermit/tests/job_translate.rs`, `kermit/tests/cli_query_errors.rs`
  - **Interfaces:** exposes the final `lftj_join*`, `hash_join*` and `validate_query` (Names contract: kermit::db) to Stories 22–23; uses Stories 9, 12 and 19
  - **Verify:** `cargo test -p kermit --lib db`
  - **Review:** deep (`compute_join` is a landmine marker; this story rebuilds it)
  - **Reading:** spec "Building and executing (`kermit::db`)", "Retired", Amendment 1 §5 and §13; `kermit/src/db.rs:1-420`; `kermit/src/db/validation.rs:1-420`; `kermit/src/lib.rs:30-60`; `kermit/tests/cli_query_errors.rs:270-330`

- **Story 21** — The harness and suites lose the column-order policy.

  Removes `ColumnOrderProvider`, `StoredOrders` and `AnyOrders`, and the `policy` argument
  of `test_join`, `define_multiway_join_test!` and every suite macro, including
  `_with_column_orders!`. It drops `join_tests.rs`'s two `AnyOrders` blocks, keeping any
  type alias another block uses.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/tests/common/macros.rs`, `kermit/tests/common/utils.rs`, `kermit/tests/join_tests.rs`
  - **Interfaces:** removes the harness's policy (Names contract: tests); uses Stories 13–14
  - **Verify:** `cargo test -p kermit --test join_tests`
  - **Review:** deep
  - **Reading:** spec "Testing"; `kermit/tests/common/macros.rs:1-609`; `kermit/tests/common/utils.rs:185-350`; `kermit/tests/join_tests.rs:675-1063`

- **Story 22** — The views and the old rewrites go.

  Deletes `const_rewrite.rs`, `selection_rewrite.rs`, `orient.rs` (`CopySpec`) and both
  families' `singleton.rs`, `selection.rs` and `iter_kind.rs`. `RewriteError` moves
  unchanged into `access.rs`. The facade drops the retired names, and
  `CatalogStats::for_query` stops recording `Const_*` entries. `cost_based_watdiv_plans.rs`
  plans through `extract_accesses`, and `tuple_allocation.rs` drops its `CopySpec` cell.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-algos/src/const_rewrite.rs`, `kermit-algos/src/selection_rewrite.rs`, `kermit-algos/src/orient.rs`, `kermit-algos/src/access.rs`, `kermit-algos/src/lib.rs`, `kermit-algos/src/sorted/mod.rs`, `kermit-algos/src/sorted/singleton.rs`, `kermit-algos/src/sorted/selection.rs`, `kermit-algos/src/sorted/iter_kind.rs`, `kermit-algos/src/hash/mod.rs`, `kermit-algos/src/hash/singleton.rs`, `kermit-algos/src/hash/selection.rs`, `kermit-algos/src/hash/iter_kind.rs`, `kermit-algos/src/optimiser/stats.rs`, `kermit-algos/tests/cost_based_watdiv_plans.rs`, `kermit/tests/tuple_allocation.rs`
  - **Interfaces:** moves `RewriteError` into `access.rs` and removes the retired names (Names contract: kermit-algos); uses Stories 9, 10 and 20
  - **Verify:** `cargo test -p kermit-algos`
  - **Review:** deep
  - **Reading:** spec "Retired", Amendment 1 §5; `kermit-algos/src/lib.rs:25-75`; `kermit-algos/src/sorted/mod.rs:1-60`; `kermit-algos/src/hash/mod.rs:1-60`; `kermit-algos/src/optimiser/stats.rs:140-170`; `kermit-algos/tests/cost_based_watdiv_plans.rs:1-240`; `kermit/tests/tuple_allocation.rs:170-205`

- **Story 23** — The optimisers plan freely, and the policy goes.

  Deletes `column_orders.rs`. `Planner::new` takes the optimiser alone. `CatalogStats` loses
  its policy, and `for_query` takes two arguments. `Precedence::free` replaces `for_query`
  in the three optimisers; `check_attribute_order` and `CyclicAttributeOrder` go; and the
  cost model drops its policy. Every caller updates, and loops over policies collapse to one
  run.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-algos/src/optimiser/column_orders.rs`, `kermit-algos/src/optimiser/mod.rs`, `kermit-algos/src/optimiser/planner.rs`, `kermit-algos/src/optimiser/stats.rs`, `kermit-algos/src/optimiser/ordering.rs`, `kermit-algos/src/optimiser/lexicographic.rs`, `kermit-algos/src/optimiser/cardinality.rs`, `kermit-algos/src/optimiser/cost_based.rs`, `kermit-algos/src/lib.rs`, `kermit-algos/tests/cost_based_watdiv_plans.rs`, `kermit/src/options.rs`, `kermit/src/db/lifecycle.rs`, `kermit/src/execution.rs`, `kermit/tests/common/utils.rs`, `kermit/tests/all_orders.rs`, `kermit/tests/result_allocation.rs`, `kermit/tests/lubm_mini_oracle.rs`, `kermit/tests/lubm_cardinalities.rs`, `kermit/tests/job_generate.rs`, `kermit/tests/subject_position_constant.rs`, `kermit/tests/snap_generate.rs`, `kermit/tests/h2o_generate.rs`, `kermit/tests/watdiv_correctness.rs`
  - **Interfaces:** changes `Planner::new` and `CatalogStats::for_query` and adds `Precedence::free` (Names contract: kermit-algos); uses Stories 21–22
  - **Verify:** `cargo test -p kermit-algos`
  - **Review:** deep
  - **Reading:** spec "Preparing a query" step 5, "Retired"; `kermit-algos/src/optimiser/planner.rs:1-130`; `kermit-algos/src/optimiser/stats.rs:110-190`; `kermit-algos/src/optimiser/ordering.rs:1-240`; `kermit-algos/src/optimiser/cost_based.rs:60-230`; `kermit-algos/src/optimiser/column_orders.rs:1-60`; `kermit/src/options.rs:20-45`

- **Story 24** — Report schema v5 and kermit-lab.

  `REPORT_SCHEMA_VERSION` becomes 5, with a history entry in `bench_report.rs`'s doc and
  the schema assertions updated. kermit-lab follows Amendment 1 §10:
  - `SCHEMA_VERSION` and `PER_QUERY_BUILD_SCHEMA` become 5, with a v5 `MEANING_CHANGES`
    entry.
  - `TIME_PHASES` and `phase_of` gain `build`.
  - The default phases become `build`.
  - The `column_orders` back-fill is limited to rows older than v5.
  - The README describes v5.
  - The pytest suite and `test_contract.py` move to v5: the column-orders and copies
    contract tests are replaced by v5 `build` / `space` checks.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/bench_report.rs`, `kermit/tests/cli_join_tests.rs`, `python/kermit-lab/kermit_lab/__init__.py`, `python/kermit-lab/kermit_lab/loader.py`, `python/kermit-lab/kermit_lab/defaults.py`, `python/kermit-lab/kermit_lab/presets.py`, `python/kermit-lab/kermit_lab/analysis.py`, `python/kermit-lab/kermit_lab/drivers/main.py`, `python/kermit-lab/README.md`, `python/kermit-lab/tests/test_loader.py`, `python/kermit-lab/tests/test_cli.py`, `python/kermit-lab/tests/test_frame.py`, `python/kermit-lab/tests/test_defaults.py`, `python/kermit-lab/tests/conftest.py`, `python/kermit-lab/tests/test_presets.py`, `python/kermit-lab/tests/test_render_all.py`, `python/kermit-lab/tests/test_analysis.py`, `python/kermit-lab/tests/test_contract.py`
  - **Interfaces:** uses the report keys of Stories 16–17; sets `SCHEMA_VERSION = 5` for the final gate's contract test
  - **Verify:** `cargo test -p kermit --bin kermit bench_report`
  - **Review:** deep
  - **Reading:** spec "CLI, reports and kermit-lab", Amendment 1 §10; `kermit/src/bench_report.rs:70-100`; `kermit/src/bench_report.rs:280-390`; `python/kermit-lab/kermit_lab/loader.py:1-200`; `python/kermit-lab/kermit_lab/defaults.py:55-130`; `python/kermit-lab/kermit_lab/presets.py:85-130`; `python/kermit-lab/tests/test_contract.py:1-190`

- **Story 25** — CLAUDE.md, ARCHITECTURE.md and the READMEs.

  Documents the new lifecycle in CLAUDE.md:
  - Workspace Architecture, and the Key Trait Hierarchy (`Relation`'s view constructors,
    `Planner`, `Tuples` and `TupleView`).
  - Testing Patterns: the all-orders test, and no column-order variant.
  - The recipes.
  - The gotchas: the rewrites are replaced by accesses and filtered builds, and the
    `--column-orders`, copies and "a `--name` per policy" entries are gone. The JSON
    reports gotcha moves to v5, and the query-validation gotcha changes.

  ARCHITECTURE.md, `README.md` and `benchmarks/README.md` follow.

  - **Developer:** junior-dev
  - **Files:** `CLAUDE.md`, `ARCHITECTURE.md`, `README.md`, `benchmarks/README.md`
  - **Interfaces:** none
  - **Verify:** `bash -c '! grep -n -E "required_indexes|Database<R>|JoinFamily|load_with_tuples|ColumnOrderPolicy" CLAUDE.md ARCHITECTURE.md README.md benchmarks/README.md'`
  - **Review:** light (documentation only; its text may quote the markers `unsafe {`, `unsafe impl`, `unsafe fn`, `AtomicU32`, `OnceCell`, `triejoin_open`, `iter_custom(`, `global_allocator`, `features = ["test-hooks"]`, `rand::`, `COPY (`, `temp_directory`, `compute_join`, `.bytes()?`, `var_name(`)
  - **Reading:** the whole spec; `CLAUDE.md`; `ARCHITECTURE.md`

- **Story 26** — Component pages.

  The algorithm pages drop the const and selection views. Each structure page's build
  reads a view and copies once per mode. The optimiser pages drop the column-order policy
  and describe planning from flat-data statistics. The templates follow.

  - **Developer:** junior-dev
  - **Files:** `docs/algorithms/hash-triejoin.md`, `docs/algorithms/leapfrog-triejoin.md`, `docs/data-structures/column-trie.md`, `docs/data-structures/hash-trie.md`, `docs/data-structures/parallel-build.md`, `docs/data-structures/tree-trie.md`, `docs/data-structures/TEMPLATE.md`, `docs/optimisers/cardinality.md`, `docs/optimisers/cost-based.md`, `docs/optimisers/lexicographic.md`, `docs/optimisers/TEMPLATE.md`
  - **Interfaces:** none
  - **Verify:** `bash -c '! grep -rn -E "SingletonTrieIter|EqualitySelection|iter_kind|ColumnOrderPolicy|required_indexes" docs/algorithms docs/data-structures docs/optimisers'`
  - **Review:** light (documentation only; its text may quote the markers `unsafe {`, `unsafe impl`, `unsafe fn`, `AtomicU32`, `OnceCell`, `triejoin_open`, `iter_custom(`, `global_allocator`, `features = ["test-hooks"]`, `rand::`, `COPY (`, `temp_directory`, `compute_join`, `.bytes()?`, `var_name(`)
  - **Reading:** spec "Design", "Parity with the paper"; the files named

- **Story 27** — The specs and benchmark docs.

  `bench-report-schema.md` gains its v5 row and loses `column_orders` from the current
  catalogue. `optimization-standard.md` takes BuildMode equivalence from views, and
  `benchmarking-architecture.md` takes the lifecycle. The benchmark pages drop
  `--column-orders` and describe per-query builds.

  - **Developer:** junior-dev
  - **Files:** `docs/specs/bench-report-schema.md`, `docs/specs/optimization-standard.md`, `docs/specs/benchmarking-architecture.md`, `docs/benchmarks/README.md`, `docs/benchmarks/H2O.md`, `docs/benchmarks/JOB.md`, `docs/benchmarks/LUBM.md`, `docs/benchmarks/SNAP.md`, `docs/benchmarks/WATDIV.md`
  - **Interfaces:** none
  - **Verify:** `bash -c 'grep -q -E "^\| 5 " docs/specs/bench-report-schema.md'`
  - **Review:** light (documentation only; its text may quote the markers `unsafe {`, `unsafe impl`, `unsafe fn`, `AtomicU32`, `OnceCell`, `triejoin_open`, `iter_custom(`, `global_allocator`, `features = ["test-hooks"]`, `rand::`, `COPY (`, `temp_directory`, `compute_join`, `.bytes()?`, `var_name(`)
  - **Reading:** spec "CLI, reports and kermit-lab", "Placement under the standard", Amendment 1 §10; the files named

<!-- plan_join: the stories below are generated from the drafts; edit the drafts, then re-join -->
