# Column Orders: `--column-orders stored|any`

**Date:** 2026-10-05
**Status:** Approved design (2026-10-05); no code yet
**Scope:** Issue #93. A planner policy, `ColumnOrderPolicy`, selected with
`--column-orders stored|any` on `join`, `bench join` and `bench run`. Under
`any` the optimiser may choose any variable order; after planning, an
*orientation* rewrite renames each atom whose plan disagrees with its stored
column order to `Index_<π>_<base>` and permutes its terms, and the catalog
holds a per-query copy of that relation with its columns permuted, built
before the timed join and dropped after the query. `stored` is today's
behaviour and stays the default.

**Sequencing.** Lands after #81 (which provides `Database<R>`, the shared
`Precedence` and `cost-based`) and before #82, which is rescoped to a memory
policy that keeps selected copies across queries and depends on this issue.
The default stays `stored` until the next sweep has data on `any`.

## Motivation

Tries are built once, in each relation's stored column order, so every plan
must bind an atom's columns left to right. That constraint is why #82's 10
index-bound WatDiv templates cost 3.7e7 to 1.7e11 bindings under every
optimiser, and why `edge(X, Y), edge(Y, X)` is rejected with
`CyclicAttributeOrder`. Leapfrog Triejoin's own setting (Veldhuizen 2014)
runs the other way: choose the variable order, then use tries whose column
order matches it. `any` provides that, measured against `stored`: the
copies are charged to their own metric, the join over them to `iteration`.

## Decisions

| Question | Decision |
|---|---|
| Name | `--column-orders stored\|any`; report axis `column_orders`; type `ColumnOrderPolicy`. |
| Kind | Neither Layout, Config nor BuildMode: a planner policy, a peer of `--optimiser`, applying to both iterator families. |
| Where the policy lives | On the planner. A `Planner` value in `kermit-algos` bundles the optimiser and the policy and replaces `&dyn QueryOptimiser` at the entry points, in the families and in the tests (user, 2026-10-05, after weighing catalog state and a second per-call argument). The catalog is policy-free: it stores copies. |
| Pinning model | A relation is *pinned* to its stored order or *free*. Only pinned atoms add column-order edges to `Precedence`. `stored` pins everything, `any` pins nothing. #82's third policy pins selectively with no new mechanism. |
| Copy source | The base relation's file-order tuples, kept in memory for the workload under `any` (user, 2026-10-05). Never `Projectable::project`: it rebuilds via `from_tuples` (dropping ColumnTrie's build mode), feeds tuples in the structure's iteration order (#66 showed input order changes build cost) and yields a nameless header. |
| Copy builder | `RelationFamily::build_relation` in the binary, `R::from_tuples` in the library and tests. Both keep config and build mode (`Configured`/`BuiltWith` forward to `from_tuples_with_config`/`_build_mode`). |
| Copy lifetime | One query's copies at a time: create before the query's group, drop after. Peak memory is the base relations, their tuples, and one query's copies. |
| Measurement | `iteration` times the join over base relations plus prebuilt copies; `insertion` is unchanged (base only); a new `copies` time function, emitted beside `insertion` when the plan needs copies, times permuting and building the query's copies; `end_to_end`'s fresh build includes the copies; `space` gains one `space/Index_<π>_<base>` entry per copy. |
| Schema version | Stays at 3. `column_orders` and `copies` are additive; no existing metric changes what it measures. kermit-lab back-fills `stored`. |
| Validation | `validate_query` takes the policy and checks `CyclicAttributeOrder` only under `stored`. |
| Reserved prefix | `Index_` joins `Const_` and `Select_`. |
| Default | `stored`. |

## Design: `kermit-algos`

### Policy and planner (`optimiser/column_orders.rs`, `optimiser/planner.rs`)

```rust
/// Which column orders the planner may bind an atom's columns in.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, ValueEnum)]
pub enum ColumnOrderPolicy {
    /// Each relation is read in its stored column order, so a plan binds
    /// every atom's columns left to right (today's behaviour).
    #[default]
    Stored,
    /// The planner binds an atom's columns in any order; atoms whose plan
    /// disagrees with the stored order run over a per-query permuted copy.
    Any,
}

impl ColumnOrderPolicy {
    /// The `column_orders` report axis value; pinned to the clap names by a
    /// test, as `Optimiser::axis_value` is.
    pub fn axis_value(self) -> &'static str;   // "stored" | "any"
}

/// The planner: an optimiser plus the column-order policy it plans under.
pub struct Planner {
    optimiser: Box<dyn QueryOptimiser>,
    column_orders: ColumnOrderPolicy,
}

impl Planner {
    pub fn new(optimiser: Box<dyn QueryOptimiser>, column_orders: ColumnOrderPolicy) -> Self;
    /// `stored` with the given optimiser: the fixtures' one-liner.
    pub fn stored(optimiser: impl QueryOptimiser + 'static) -> Self;
    pub fn column_orders(&self) -> ColumnOrderPolicy;
    pub fn required_statistics(&self) -> StatisticsLevel;                 // delegates
    pub fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan;  // delegates
}
```

`QueryOptimiser` and its three implementations keep their signatures. The
policy reaches them through the statistics they already receive.

### Pinning (`optimiser/stats.rs`, `optimiser/ordering.rs`)

```rust
impl CatalogStats {
    /// One entry per body predicate, as today, plus the policy the plan is
    /// made under.
    pub fn for_query(query: &JoinQuery, column_orders: ColumnOrderPolicy,
                     stats_of: impl Fn(&str) -> Option<RelationStats>) -> Self;
    pub fn column_orders(&self) -> ColumnOrderPolicy;
    /// Whether the atom named `name` must bind its columns left to right.
    /// `Select_<n>_<base>` follows its base; `Const_*` is unary, so pinning
    /// it changes nothing.
    pub fn is_pinned(&self, name: &str) -> bool;
}

/// The column-order precedence DAG: an edge u → v for every pinned atom in
/// which u's column precedes v's. The one place those edges are built.
pub struct Precedence { /* adjacency, in_degree */ }

impl Precedence {
    pub fn for_query(query: &JoinQuery, stats: &CatalogStats) -> Self;
    pub fn predecessor_masks(&self) -> Option<Vec<u64>>;
}

pub fn topological_order<K: Ord>(num_vars: usize, precedence: &Precedence,
                                 rank: impl Fn(usize) -> K) -> Vec<usize>;
```

`Precedence::new(num_vars, predicate_variables)` (all atoms pinned) stays as
a `pub(crate)` constructor for `check_attribute_order`, which keeps its
stored semantics; validation decides whether to call it. The three
optimisers change one line each: build the precedence from the query and
statistics, then rank as before. Under `stored` every plan is byte-identical
to today's.

### Cost-based under `any` (`optimiser/cost_based.rs`)

Predecessor masks come from `Precedence::for_query`. With no edges every
variable subset is a DP state, so the `1 << 14` budget falls back to
`cardinality`'s plan above 14 variables (2¹⁴ − 1 = 16,383 states for 14
variables fits; 15 does not). `CostModel::estimate(bound)` reads the
distinct counts of each atom's *bound columns as a set*: the projection size
of an atom onto a column set is the product of those columns' distinct
counts, capped at the tuple count, with one column giving its own distinct
count. Under `stored` the bound set of every atom is a stored-order prefix,
so the arithmetic is unchanged and the 124-template parity fixture still
holds; the prefix `debug_assert!` becomes a `stored`-only check.

### Orientation (`orient.rs`)

A fourth rewrite, beside `const_rewrite.rs`, `placeholder_rewrite.rs` and
`selection_rewrite.rs`, run after planning:

```rust
pub const INDEX_PREDICATE_PREFIX: &str = "Index_";
pub fn is_index_predicate(name: &str) -> bool;

/// A relation copy with its columns permuted: column `i` of the copy is
/// column `permutation[i]` of `base`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexSpec {
    pub name: String,            // "Index_1_0_edge"
    pub base: String,            // "edge"
    pub permutation: Vec<usize>, // [1, 0]
}

impl IndexSpec {
    pub fn new(base: &str, permutation: Vec<usize>) -> Self;   // names itself
    pub fn permute(&self, tuple: &[usize]) -> Vec<usize>;
    pub fn permute_all(&self, tuples: &[Vec<usize>]) -> Vec<Vec<usize>>;
}

/// The executor-ready form of a planned query.
pub struct Oriented {
    pub query: JoinQuery,
    pub plan: QueryPlan,
    pub index_specs: Vec<IndexSpec>,
    pub selection_specs: Vec<SelectionSpec>,
}

pub fn orient(policy: ColumnOrderPolicy, query: JoinQuery, plan: QueryPlan,
              selection_specs: Vec<SelectionSpec>) -> Oriented;
```

Under `stored`, `orient` returns its input with no specs. Under `any`:

1. For each body atom other than a `Const_*` atom, π is its columns sorted
   by plan position (stable; after the selection rewrite every atom's
   variables are distinct, so positions are distinct). An identity π leaves
   the atom alone.
2. A non-identity π permutes the atom's terms. A plain atom is renamed
   `Index_<π joined by _>_<base>`. A `Select_<n>_<base>` atom keeps its
   name; its `SelectionSpec.relation` becomes the copy's name and each
   `ColumnEquality { source, repeat }` is mapped through π⁻¹ and re-sorted
   so that whichever column now comes first is the `source`
   (`EqualitySelectionTrieIter` is positional and asserts `source < repeat`).
3. Atoms with equal `(base, π)` share one `IndexSpec`; specs are emitted in
   body order.
4. The plan is translated by variable name: `analyse` numbers body-only
   variables by first appearance, so permuting terms can renumber them.
   `orient` maps each plan entry old index → name → new index through a
   fresh `analyse` of the oriented query, and `debug_assert!`s that the new
   plan validates against it.

The executors never learn about `any`. LFTJ opens an atom's k-th trie level
for its k-th variable in descent order, and `QueryPlan::validate` rejects a
plan that would break that; HashTriejoin maps leaf tuples positionally. By
the time the plan reaches `join_for_each`, every atom's term order equals
its (copied) trie's column order. The data structures do not change.

## Design: `kermit::db`

### The catalog

`Database<R>` stays relations plus statistics and gains a copy store:

```rust
pub struct Database<R> {
    relations: BTreeMap<String, R>,
    statistics: BTreeMap<String, RelationStats>,
    level: StatisticsLevel,
    indexes: BTreeMap<String, (IndexSpec, R)>,   // the current query's copies
}

impl<R: Relation + Cardinality> Database<R> {
    /// The copies `query` needs under `planner` that the store does not
    /// hold yet: validate, rewrite, gate on statistics, plan, orient.
    pub fn required_indexes(&self, query: &JoinQuery, planner: &Planner)
        -> Result<Vec<IndexSpec>, JoinError>;
    pub fn add_index(&mut self, spec: IndexSpec, copy: R);   // debug-asserts arity == base arity
    pub fn clear_indexes(&mut self);
    pub fn index(&self, name: &str) -> Option<&R>;
    pub fn indexes(&self) -> impl Iterator<Item = (&IndexSpec, &R)>;   // for space/Index_*
}

/// The copy's header: `spec.name`, the base's attributes permuted.
pub fn index_header(spec: &IndexSpec, base: &RelationHeader) -> RelationHeader;

/// A copy built the library way: `R::from_tuples(index_header, permuted)`.
pub fn build_index<R: Relation>(spec: &IndexSpec, base: &RelationHeader,
                                tuples: &[Vec<usize>]) -> R;
```

The join never builds a copy itself. Statistics stay read-only and are
built from the rewritten query *before* orientation, so the planner sees
only base names (`Select_` names already map to their base through
`base_of`).

### Entry points and the shared body

All four entry points take `planner: &Planner` where they took
`optimiser: &dyn QueryOptimiser`:

```rust
pub fn lftj_join_for_each<R, JA>(database: &Database<R>, query: JoinQuery,
                                 planner: &Planner, emit: impl FnMut(&[usize]))
    -> Result<(), JoinError>;
// lftj_join, hash_join_for_each, hash_join likewise.

pub fn validate_query(query: &JoinQuery, relations: &(impl RelationArities + ?Sized),
                      column_orders: ColumnOrderPolicy) -> Result<(), JoinError>;
```

`prepare` takes the policy too and runs `check_attribute_order` only under
`stored`. `run_join` becomes:

1. `prepare` (validate, const, placeholder and selection rewrites).
2. Statistics gate (`MissingStatistics`), as today.
3. `CatalogStats::for_query(&rewritten, planner.column_orders(), …)`,
   `planner.plan`, then `orient`.
4. Wrappers over the *oriented* query: a plain atom resolves in
   `relations`; an `Index_*` name, as an atom or as a selection's
   `relation`, resolves in `indexes` or fails with
   `JoinError::MissingIndex { index, base }`; `Const_*` as today.
5. `JA::join_for_each` over the oriented plan; head projection unchanged
   (head variables keep their numbering through orientation).

`required_indexes` runs steps 1–3 and filters out the specs already held,
so it and the join cannot disagree: same planner, same statistics, same
deterministic plan. `check_body_atoms` rejects `Index_` under
`ReservedRelationName`.

### Errors

`JoinError` gains `MissingIndex { index: String, base: String }`: "query
needs a copy of `edge` with columns (1, 0) (`Index_1_0_edge`) that the
database does not hold; build it with `required_indexes` first". It can
only arise from a caller that skipped `required_indexes`; the binary and
the test harness never do.

## Design: `kermit` binary

### CLI

A clap-flattened `PlannerArgs` replaces the bare `--optimiser` on `join`,
`bench join` (via `QueryArgs`) and `bench run`:

```rust
#[derive(Args, Copy, Clone)]
pub struct PlannerArgs {
    #[arg(long, value_enum, default_value_t = Optimiser::Lexicographic)]
    pub optimiser: Optimiser,
    #[arg(long, value_enum, default_value_t = ColumnOrderPolicy::Stored)]
    pub column_orders: ColumnOrderPolicy,
}
impl PlannerArgs { pub fn instantiate(self) -> Planner; }
```

`RunSettings.planner: PlannerArgs`; `TrieLftj::new(build, planner)` and
`HashHtj::new(config, planner)` take a `Planner`. `bench ds` has no join and
no planner flags.

### `ExecutionFamily`

Four methods over the engine, all defaulted on `Engine = Database<Rel>`:

```rust
fn required_indexes(&self, engine: &Self::Engine, query: &JoinQuery)
    -> Result<Vec<IndexSpec>, JoinError>;
/// Permutes `tuples` and builds the copy through `build_relation`, so
/// config and build mode carry over; then `engine.add_index`.
fn add_index(&self, engine: &mut Self::Engine, spec: IndexSpec,
             base: &RelationHeader, tuples: &[Vec<usize>]);
fn clear_indexes(engine: &mut Self::Engine);
fn indexes(engine: &Self::Engine) -> Vec<(&IndexSpec, &Self::Rel)>;
```

### `kermit join` and `bench join --output`

`build_join_runner` loads with `load_with_tuples` under `any` and keeps the
`(header, tuples)` per relation. `JoinRunner` becomes `FnMut`: required,
add, join, clear. Under `stored` nothing is kept and nothing changes.

### `bench run`

Relations are loaded with tuples whenever the policy is `any` or the
metrics need them (`insertion`/`end_to_end`, as today). Per query:

1. `family.required_indexes(&engine, q)`; one metadata line per copy,
   `index: edge (1, 0)`.
2. `copies`, emitted only when the list is non-empty: an `iter_batched`
   whose routine permutes and builds every copy of the query through
   `build_relation` (one function for the query, like `insertion` is one
   function for the workload). The permutation is inside the timed region:
   it is the price of a copy.
3. `family.add_index` for each spec. `iteration` is the unchanged closure
   over `&engine`. `end_to_end`'s fresh build is `build_from_tuples` plus
   the copies, then K counted joins. `space` adds one
   `space/Index_<π>_<base>` function per copy, beside the base relations'.
4. `clear_indexes`.

Report `axes` gain `column_orders` (always, from `planner.column_orders.axis_value()`).
As with `--optimiser`, the Criterion group name omits the policy, so give
each policy its own `--name`.

### kermit-lab

`column_orders` joins `_AXIS_STR_KEYS` with an unscoped `stored` back-fill
for pre-#93 reports; `copies` joins `TIME_PHASES` and `phase_of`; the
real-binary contract test asserts the axis is present. `AXIS_PHASES`
(ablation scopes) is for `ds_*` axes and does not change; `column_orders`
is faceted or coloured like `optimiser`.

## Effect on measurements

Under `stored`: no metric changes what it measures; plans are identical;
the shared body gains a no-op `orient` call and one enum compare, within
the codegen precision bound (±3 %, documented). Under `any`: `iteration`
is comparable with `stored` (both time a join over prebuilt structures);
`copies`, `space/Index_*` and `end_to_end` show what `any` costs.

## Out of scope

- Keeping copies across queries (#82), choosing which copies to keep, and
  any third policy.
- Building copies from the structure's own iterator (`project`).
- Per-relation pinning on the CLI.
- An `ablation` preset over `column_orders`.
- A schema bump.

## Testing

**Breadth (Priority 1).** `define_multiway_join_test_suite_with_column_orders!(Relation, Algo, Optimiser, Policy)`
in `kermit/tests/common/macros.rs`, delegating to the base suite inside a
module as the Config and BuildMode macros do. The 16 pattern macros and
`define_multiway_join_test!` gain a fourth, policy argument; the
three-argument forms expand with `Stored`. `test_join` takes a fourth type
parameter `P: ColumnOrderProvider` (a test-only trait in
`kermit/tests/common/utils.rs`, `fn policy() -> ColumnOrderPolicy`, with the
unit structs `StoredOrders` and `AnyOrders`), builds
`Planner::new(Box::new(O::default()), P::policy())`, and before joining
calls `required_indexes` and `build_index::<R>` on the fixture's tuples,
adding each copy; `JoinEntry::database` is unchanged, `join`/`count` take
`&Planner`. Invocations in `kermit/tests/join_tests.rs`: every one of the 10
aliases × 3 optimisers under `Any` (30 suites), plus `Any` invocations of
the `Configured<HashTrieSip|Fx, HalfFull>` and `BuiltWith<ColumnTrie,
Incremental>` suites (the copies keep their config and build mode; a spy
provider counts the calls). Rows under `any` equal the rows under `stored`.

**`any`-only patterns.** `kermit/tests/column_orders_any.rs`, a macro over
the 10 aliases × 3 optimisers:

| Query | `stored` | `any` |
|---|---|---|
| `Q(X, Y) :- edge(X, Y), edge(Y, X).` | `CyclicAttributeOrder` | the symmetric edges |
| `Q(V0, V2, V7) :- includes(V2, V0), purchasefor(V7, V0).` (#82's incoming star; the head puts `V0` first, so `lexicographic` reorients both atoms) | accepted | same rows, two copies |
| `Q(X, Y, Z) :- r(X, Y, Z), s(Z, X).` | `CyclicAttributeOrder` | rows |
| `Q(X, Y) :- r(X, X, Y), s(Y, X).` (repeat on a reoriented atom) | `CyclicAttributeOrder` | rows; the equality remap |
| `Q(X, Y) :- r(X, Y), s(Y, c1, X).` (constant on a reoriented atom; the const rewrite gives `s` a fresh middle variable, so `X → Y → K → X` is a cycle) | `CyclicAttributeOrder` | rows |

**Equivalence.** `kermit/tests/column_orders_equivalence.rs`: a seeded
generator (an inline LCG; no new crate) draws random conjunctive queries
over random small relations; for every query `stored` accepts, `any` must
return the same multiset on every cell and optimiser. `lubm_mini_oracle.rs`
iterates `ColumnOrderPolicy::value_variants()` × `Optimiser::value_variants()`;
`lubm_cardinalities.rs` gains policy rows; `watdiv_correctness.rs` and
`subject_position_constant.rs` run under both policies.

**Unit.** `orient` (identity under `stored`; π; shared specs; equality
remap with `source < repeat` restored; plan translation by name; a
`Select_` atom pointing at its copy; `Const_` atoms untouched);
`Precedence::for_query` pinning and `CatalogStats::is_pinned` (incl.
`Select_` following its base); `CostModel::estimate` on a non-prefix bound
set under `any` and the parity fixture under `stored`; `MissingIndex`;
`Index_` under `ReservedRelationName`; `index_header`; `build_index`
keeping config and build mode; `required_indexes` filtering held copies;
`ColumnOrderPolicy::axis_value` pinned to clap names; `Planner`
delegation. `result_allocation.rs` gains `any` cells (copies are built
before measurement; the join still allocates nothing per row).
`cli_query_errors.rs`: `--column-orders any` accepts the cyclic query on all
six cells; the `stored` message now names the flag.

**Mutants.** A named test must fail when: the equality remap is dropped
(the repeat-on-reoriented-atom pattern); copies are built via `project`
(the `BuiltWith` spy, and the nameless header); the plan is not translated
by name (a pattern with a body-only variable that moves); `check_attribute_order`
runs under `any` (the cyclic pattern); `copies` is emitted with no specs
(a report test).

## Verification beyond CI

On the cached `watdiv-stress-100-test-1-prelim` data (`~/.cache/kermit/benchmarks/`,
read-only; see the shared-cache rule):

- The 10 templates moved from #82 (q0264, q0079, q0017, q0030, q0306,
  q0409, q0035, q0010, q0008, q0085) under `--column-orders any --optimiser
  cost-based` on every structure: each within 10 s per execution, counts
  matching `groundtruth/duckdb-prelim.json` in
  `kermit-bench-runs/thesis-rebench-2026-10-02/`.
- The number of the 124 templates whose DP exceeds the budget under `any`,
  recorded in `docs/optimisers/cost-based.md`.
- A `stored` sanity run on `oxford-uniform-s3` (TreeTrie as control, ≥5
  replicates): timings within the codegen precision bound.

## Verification (gate)

Inside `nix develop`, with `CARGO_BUILD_JOBS=2`: `cargo test`, `cargo clippy
--all-targets` with `-Dwarnings`, `cargo fmt --all --check`, `cargo doc
--workspace` with `-Dwarnings`, `cargo miri test` (CI's exclusions and
flags), and kermit-lab's pytest with `KERMIT_BIN`.

## Documentation

- `ARCHITECTURE.md`: validate, rewrite, plan, orient, execute; copies in
  the catalog; the `Planner`.
- `docs/algorithms/leapfrog-triejoin.md`, `hash-triejoin.md`: the
  orientation contract (term order equals trie column order) and that
  `CyclicAttributeOrder` is a `stored` limitation.
- `docs/optimisers/{lexicographic,cardinality,cost-based}.md`: behaviour
  per policy; the cost-based doc records the budget count.
- `docs/specs/bench-report-schema.md`: `column_orders`, `copies`,
  `space/Index_*`, the `index:` metadata line.
- `CLAUDE.md`: trait hierarchy (`Planner`, `ColumnOrderPolicy`), the
  optimiser recipe (`Precedence::for_query` + the new `topological_order`
  signature), gotchas (orientation rewrite, `Index_` reserved, copies via
  `build_relation` never `project`, a `--name` per policy,
  `CyclicAttributeOrder` under `stored` only, tuples kept under `any`).

## Commit sequence

The implementation plan refines this into one commit per task.

1. `docs(spec): column orders (#93)` (this file).
2. `feat(algos): ColumnOrderPolicy and Planner` — the enum, the bundle,
   `CatalogStats` pinning, `Precedence::for_query`, the new
   `topological_order`; optimisers migrated; no behaviour change.
3. `feat(algos): the orientation rewrite` — `orient.rs`, `IndexSpec`,
   `Oriented`, unit tests.
4. `feat(algos): cost-based estimates bound column sets` — the set-based
   projection size; parity fixture unchanged.
5. `refactor(db): the entry points take a Planner` — signatures, `validate_query`
   policy, every caller migrated under `stored`.
6. `feat(db): the catalog holds a query's copies` — the store,
   `required_indexes`, `MissingIndex`, `Index_` reserved, `index_header`,
   `build_index`; `run_join` orients and resolves copies.
7. `feat: --column-orders stored|any` — `PlannerArgs`, families, `kermit
   join`, `bench run` (`copies`, `space/Index_*`, metadata, axis), kermit-lab.
8. `test: the 16 patterns under any, any-only patterns, equivalence` — the
   macros, suites, oracle and CLI tests, allocation cells.
9. `docs: column orders` — the docs list above, plus the WatDiv results.

## Resolved questions

- **Scope** (user, 2026-10-05): all of #93 in this spec; the branch name
  (`93-planner`) is only where it started.
- **Policy site** (user, 2026-10-05): on the planner, as a `Planner`
  bundle, after weighing catalog state (no churn, agreement by
  construction, but a planner setting on the store) and a second per-call
  argument (two values that must agree at ~30 sites).
- **Copy source** (user, 2026-10-05): file-order tuples kept in memory for
  the workload; the default metrics already keep them for `insertion`.

## Acceptance (issue #93)

| Criterion | Where |
|---|---|
| `--column-orders stored\|any` on `join`, `bench join`, `bench run`; reports carry `column_orders`; `stored` unchanged | `PlannerArgs`; `cli_query_errors.rs`; report tests; the `stored` sanity run |
| Tests pass for every structure × algorithm × optimiser | Testing, breadth and `any`-only |
| Reports carry `copies` and `space/Index_*`; kermit-lab loads them | `bench run` step 2–3; loader and contract tests |
| The 10 moved templates within 10 s under `any` + `cost-based`, counts matching DuckDB | Verification beyond CI |
| The docs record how many templates exceed the DP budget under `any` | `docs/optimisers/cost-based.md` |
