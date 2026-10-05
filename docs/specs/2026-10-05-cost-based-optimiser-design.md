# Cost-Based Query Optimiser

**Date:** 2026-10-05
**Status:** Approved design (2026-10-05); no code yet
**Scope:** Issue #81. A third `QueryOptimiser`, `CostBasedOptimiser`
(`--optimiser cost-based`), which ports the estimate-based dynamic programme
that #68 simulated (`plan_est_dp` in `planners.py`). It needs a statistic the
planner does not have today, the number of distinct values in each column. A
catalog type in `kermit::db`, `Database<R>`, gathers it once, when the
optimiser asks for it, and holds it.

**Sequencing.** #81 waited for #65 (the streamed join rewrote `run_join`);
#65 is closed. #81 lands before #82 (object-first tries, which only pay off
with this planner) and before the next authoritative sweep (issue text).

## Motivation

#68 diagnosed the 15 WatDiv stress templates that take over 10 s. At depth
k, LFTJ binds exactly |⋈ atoms projected onto Sₖ| keys, where Sₖ is the set
of the first k variables of the plan. So a plan's work is
`Σₖ |⋈ π_{Sₖ}(atoms)|`, and that sum depends only on the sets Sₖ, not on the
order inside them. #68 computed these counts exactly for all 124 templates
of `watdiv-stress-100-test-1` and scored six candidate planners on them
(`kermit-bench-runs/watdiv-stress-100-prelim-2026-09-10/diagnostics/issue-68/`).

Under today's subject-first tries, the estimate-based DP, against
`cardinality`:

- fixes q0020 (9.68e10 → 1.81e4 bindings; both current optimisers cost
  9.68e10) and q0034 (2.27e9 → 9.67e5; `cardinality` is 187× worse than
  `lexicographic` there);
- improves 32 templates and worsens 3: q0008 (3.81e7 → 8.85e7, 2.32×), q0360
  (1.25×) and q0070 (1.01×);
- drops the workload total from 3.85e11 to 2.64e11 bindings.

A greedy planner over the same statistics (`plan_fanout_greedy`) worsens 75
templates, q0131 by 144×. The search, not only the statistics, is what fixes
the plans.

## Decisions

| Question | Decision |
|---|---|
| Name | `cost-based`: CLI value, `optimiser` report axis value, `docs/optimisers/cost-based.md`. Type `CostBasedOptimiser`. |
| Cost function | `cost(v₁…vₙ) = Σₖ est(Sₖ)`, #68's counting model with the exact count replaced by an estimate. |
| Estimate | The System R estimate from `planners.py`'s `est_f`: the product of the atoms' bound projections, divided, per variable, by every column distinct count except the smallest. |
| Search | Forward DP over the downward-closed variable sets of the column-order precedence DAG, as `plan_est_dp` does. Ties break on `(cost, order)`, lexicographically. |
| Validity | The DP only adds a variable whose predecessors are all bound. Its order then passes through `topological_order(n, pv, \|v\| position[v])`, which returns it unchanged; a `debug_assert` checks that. |
| Budget | A named constant, 2¹⁴ = 16,384 states (WatDiv's largest template has 820). A query with more than 64 variables, or more states than the budget, gets `CardinalityOptimiser`'s plan. |
| New statistic | Per-column distinct counts, `RelationStats::column_distinct`. |
| Where statistics live | A catalog, `kermit::db::Database<R>`: the relations plus one `RelationStats` per relation, gathered when it is built, read-only afterwards. The entry points take it instead of `&BTreeMap<String, R>`. The data structures do not change. |
| When they are gathered | Only when the optimiser asks. `QueryOptimiser::required_statistics()` returns a `StatisticsLevel`; the default is `TupleCounts`, and `CostBasedOptimiser` returns `ColumnDistinct`. The engine builds its `Database` at that level. |
| Insufficient statistics | `JoinError::MissingStatistics { required, available }`, returned after validation and before planning. |
| Default optimiser | Unchanged: `lexicographic`. |
| Schema version | Stays at 3. `"cost-based"` is a new value of an existing axis, and no existing metric changes what it measures. |

## Design: `kermit-algos`

### Statistics (`optimiser/stats.rs`)

```rust
/// The statistics a planner reads, cheapest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatisticsLevel {
    /// Tuple counts only: O(1) per relation through `kermit_ds::Cardinality`.
    TupleCounts,
    /// Tuple counts plus the number of distinct values in every column:
    /// one O(N) walk per relation.
    ColumnDistinct,
}

pub struct RelationStats {
    pub tuples: usize,
    pub arity: usize,
    /// Distinct values per column, in column order; empty when the
    /// statistics were gathered at `TupleCounts`.
    pub column_distinct: Vec<usize>,
}
```

`RelationStats` loses `Copy`, since it now holds a `Vec`. A new free
function, `distinct_per_column(arity, walk) -> Vec<usize>`, counts distinct
values with one `HashSet` per column over the tuples `walk` lends. It takes
a closure, not a data structure, so the module keeps its rule: nothing here
touches a data structure. Tuple counts still come from
`kermit_ds::Cardinality`, so they agree with what the join iterates.

`CatalogStats::for_query(query, stats_of)` keeps its shape. `stats_of` now
returns `Option<RelationStats>` instead of a tuple count. A `Const_*`
predicate gets `{ tuples: 1, arity: 1, column_distinct: [1] }`.
`CatalogStats` gains `distinct(name, column) -> Option<usize>`.

### The trait (`optimiser/mod.rs`)

```rust
pub trait QueryOptimiser {
    fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan;

    /// The statistics `plan` reads. The engine gathers exactly these, so an
    /// optimiser that reads only tuple counts never pays for a column walk.
    fn required_statistics(&self) -> StatisticsLevel { StatisticsLevel::TupleCounts }
}
```

`Optimiser` gains `CostBased`. `instantiate()` returns
`CostBasedOptimiser::default()` and `axis_value()` returns `"cost-based"`,
which is also clap's derived name; `axis_values_match_clap_value_names`
already pins the two together. `CostBasedOptimiser` and `StatisticsLevel` are
re-exported from `optimiser` and from the crate root.

### Shared precedence (`optimiser/ordering.rs`)

`Precedence` becomes `pub(crate)` and gains `predecessor_masks() ->
Option<Vec<u64>>`: bit u of entry v is set when u must be bound before v. It
returns `None` for more than 64 variables. The DP reads readiness from it,
so `topological_order`, `check_attribute_order` and the DP all build the
constraint edges in one place.

### The optimiser (`optimiser/cost_based.rs`)

**Estimate.** For a downward-closed set S (a bitmask), with each atom's
statistics `(n, d[0..arity])`:

```
est(∅) = 1
est(S) = numerator / denominator, where
  numerator   = Π over atoms with p ≥ 1 bound columns of
                  n                         if p = arity
                  d[0]                      if p = 1 < arity
                  min(n, d[0] · … · d[p-1]) otherwise (independence)
  denominator = Π over bound variables v of
                  the product of v's column distinct counts, over every
                  atom where v is bound, except the smallest
```

Because S respects column order, an atom's bound columns are always a
prefix, so p is well defined. After the selection rewrite no variable
repeats within an atom. WatDiv and LUBM relations are unary or binary, so
the independence branch never runs on them.

Guards: an atom with `n = 0` makes `est(S) = 0` for every S that binds one
of its columns. Distinct counts are clamped to at least 1 in the
denominator. The prototype never met an empty relation.

The arithmetic is `f64` and runs in the prototype's order: atoms in body
order, then each atom's bound variables in ascending index, with each
variable's distinct counts sorted ascending. Equal inputs then give
bit-identical estimates, which the parity check relies on.

**Search.**

```
best[∅] = (0, [])
for layer in 1..=n:
    for each S in the previous layer, v ∉ S with predecessors(v) ⊆ S:
        T = S ∪ {v}
        candidate = (best[S].cost + est(T), best[S].order ++ [v])
        keep candidate if T is new or candidate < best[T]   // (cost, order)
return best[all].order
```

`est(T)` is memoised per state. Costs compare with `f64::total_cmp`, and
ties break on the order, lexicographically, as `plan_est_dp`'s
`key = (c, order + (v,))` does.

**Validity.** The DP's order is already a topological order. It passes
through `topological_order(n, pv, |v| position[v])` anyway, so every
optimiser's plan still comes out of `topological_order` as the recipe says.
A `debug_assert_eq!` checks that the pass changes nothing, so the safety net
cannot hide a DP bug.

**Budget.** `CostBasedOptimiser { state_budget: usize }`, where `Default`
uses `DEFAULT_STATE_BUDGET = 1 << 14`. When `predecessor_masks()` returns
`None` (more than 64 variables) or the number of states passes the budget,
`plan` returns `CardinalityOptimiser.plan(query, stats)`. That fallback is
measured on every #68 template; an estimate-driven greedy is not, and the
greedy #68 did measure was 144× worse on q0131. Tests set the budget to
force the fallback.

**Missing statistics.** When `CostBasedOptimiser` is called directly with
literal `CatalogStats`: a predicate with no statistics is "large"
(`n = d = usize::MAX as f64`, the `cardinality` convention), and a missing
column distinct count is taken to equal `n` (the column is a key). Through
`kermit::db` this cannot happen, because `MissingStatistics` is checked
first.

## Design: `kermit::db`

### The catalog

```rust
/// Relations plus the statistics a planner reads, gathered once when the
/// database is built ("ANALYZE") and read-only afterwards, so they cannot
/// drift from the relations.
pub struct Database<R> {
    relations: BTreeMap<String, R>,
    statistics: BTreeMap<String, RelationStats>,
    level: StatisticsLevel,
}

impl<R: Relation + Cardinality> Database<R> {
    pub fn new<F: JoinFamily<R>>(relations: BTreeMap<String, R>, level: StatisticsLevel) -> Self;
}

impl<R> Database<R> {
    pub fn get(&self, name: &str) -> Option<&R>;
    pub fn relations(&self) -> impl Iterator<Item = &R>;
    pub fn statistics(&self, name: &str) -> Option<&RelationStats>;
    pub fn level(&self) -> StatisticsLevel;
}

/// Tuple counts only: no walk, so no family needed.
impl<R: Relation + Cardinality> From<BTreeMap<String, R>> for Database<R> { /* … */ }

impl<R: Relation> RelationArities for Database<R> { /* as for the BTreeMap */ }
```

Relations stay keyed by the names queries use: the caller's map, as
today. `ExecutionFamily::build` keys them by `header().name()`, and test
fixtures by their own names (their headers are nameless). At `TupleCounts`,
statistics come from `Cardinality::tuple_count` and the header's arity,
which costs nothing; `From<BTreeMap<String, R>>` builds exactly that. At
`ColumnDistinct`, each relation is also walked once through
`F::for_each_tuple` into `distinct_per_column`.

### The walk (`JoinFamily::for_each_tuple`)

`JoinFamily<R>` gains `fn for_each_tuple(relation: &R, visit: impl
FnMut(&[usize]))`:

- **`SortedFamily`** walks `TrieIteratorWrapper::new(relation.trie_iter())`
  with `advance()`, which lends each tuple and allocates nothing per tuple.
  `SortedTrieFamily::for_each_tuple` walks the same way.
- **`HashFamily<H>`** walks the `HashTrieIterator` contract: `open`, `next`
  and `leaf_tuples` at the last level. That is the contract `HashTriejoin`
  already relies on, including for pruned singletons, and it works for any
  `R: HashTrieIterable`, `Configured<HashTrie, _>` included.

The walk lives on the family, not on a new `kermit-ds` trait, so the
structures are untouched. It also avoids pairing a blanket `impl<T:
TrieIterable>` with a concrete `impl for HashTrie`, which Rust's overlap
check can reject because `TrieIterable` lives upstream in `kermit-iters`.

### Entry points

`lftj_join_for_each`, `lftj_join`, `hash_join_for_each` and `hash_join` take
`&Database<R>` instead of `&BTreeMap<String, R>`. `run_join`:

1. `prepare` (validation and the three rewrites), unchanged;
2. **new:** if `optimiser.required_statistics() > database.level()`, return
   `JoinError::MissingStatistics { required, available }`;
3. builds the wrappers, unchanged;
4. builds `CatalogStats::for_query(&rewritten, …)` from
   `database.statistics(base)`, where `base` resolves a `Select_*` name to
   its base relation as today (a selection view reports its base's counts:
   upper bounds);
5. plans and executes, unchanged.

Planning stays O(#predicates) reads, plus the optimiser's own search.

`JoinError::MissingStatistics` names both levels, e.g. `the optimiser
needs per-column distinct counts, but the database was built with tuple
counts only`. It is a library-misuse error;
the CLI cannot produce it, because the engine is always built at the chosen
optimiser's level.

## Design: `kermit` binary

`TrieLftj<R>` and `HashHtj<H, P>` get `type Engine = Database<R>`.
`build` and `build_from_tuples` call `Database::new::<SortedFamily>` (or
`HashFamily<H>`) at `self.optimiser.required_statistics()`.
`relations(engine)` collects `engine.relations()`. Nothing in `bench/run.rs`,
`bench/ds.rs` or `main.rs` changes, because every join goes through
`ExecutionFamily::join_for_each`.

`kermit::compute_join` bypasses `kermit::db` and is unchanged.

## Effect on measurements

| Metric | Lexicographic / cardinality | Cost-based |
|---|---|---|
| `insertion`, `space` | unchanged (structures untouched) | the same |
| `iteration` | unchanged: O(1) statistics reads, same planner | statistics reads plus the DP; the walk ran at engine build, outside timing |
| `end_to_end` | unchanged apart from O(#relations) catalog bookkeeping (a name clone and a map entry per relation, no walk), immeasurable beside the build | includes the walk, its real price |
| `kermit join` wall time | unchanged | includes the walk |

Planning time for q0034, the largest WatDiv DP (820 states), is measured and
recorded in `docs/optimisers/cost-based.md`.

## Out of scope

- Object-first tries (#82). This planner respects today's subject-first
  constraint. #68's 10 index-bound templates stay slow until #82.
- Changing the default optimiser. That waits for the next sweep's data.
- Statistics for selection views (a view reports its base's counts),
  histograms, multi-column distinct counts.
- `kermit/tests/result_allocation.rs`. It checks per-row allocation; the
  planner allocates per join.
- `kermit::compute_join`.

## Testing

**Breadth (CLAUDE.md recipe).**

- `kermit/tests/join_tests.rs`: 13 invocations with `CostBasedOptimiser`,
  mirroring `CardinalityOptimiser`'s: the 10 Layout aliases (`TreeTrie` and
  `ColumnTrie` × linear/binary/galloping, `HashTrie` × Sip/Fx ×
  pruned/unpruned) plus the `HalfFull` config suite and the two `Incremental`
  build-mode suites.
- `kermit/tests/common/utils.rs`: `JoinEntry` gains
  `database(relations, level)`, which picks the family, and its `join` /
  `count` take `&Database<R>`. `test_join<R, JA, O>` builds the database at
  `O::default().required_statistics()`, so each optimiser plans at exactly
  the level it declares, and all 16 patterns plan from real statistics.
- `kermit/tests/lubm_mini_oracle.rs`: covered automatically through
  `Optimiser::value_variants()`; it builds the database at the instantiated
  planner's level.
- `kermit/tests/lubm_cardinalities.rs`: a `cost-based` row. It needs `java`,
  so it runs inside `nix develop`.
- `kermit/tests/cli_optimiser_choice.rs`: `--optimiser cost-based` records
  the axis.
- `kermit/tests/watdiv_correctness.rs` and
  `kermit/tests/subject_position_constant.rs`: parameterised over
  `Optimiser::value_variants()`. Both run only `lexicographic` today, a known
  gap. Subject-position constants are where cost-based plans differ most.

**Unit tests.**

- `cost_based.rs`: `est` against hand-computed values (two atoms give
  `nR·nS / max(dR, dS)`; a partial prefix uses `d[0]`; the independence
  branch for arity 3); the zero-tuple guard; a q0020-shaped mini query where
  the DP binds the constant's side first and `cardinality` does not; column
  constraints respected; deterministic tie-break; budget 0 returns
  `CardinalityOptimiser`'s plan; `required_statistics() == ColumnDistinct`.
- `stats.rs`: `distinct_per_column` on literal tuples, duplicates included;
  `for_query` statistics for `Const_*` and selection names.
- `ordering.rs`: `predecessor_masks` agrees with the edges `Precedence`
  builds, and returns `None` past 64 variables.
- `kermit/src/db.rs`: at `TupleCounts` no relation is walked (a spy
  family); at `ColumnDistinct` both families report identical statistics for
  the same tuples, pruned hash tries included; `MissingStatistics` is
  returned before any row is emitted; `RelationArities for Database` agrees
  with the `BTreeMap` implementation.
- `kermit/src/execution.rs`: each family builds its engine at its
  optimiser's level.

**WatDiv plan fixture (acceptance criterion 3, CI).**
`kermit-algos/tests/cost_based_watdiv_plans.rs` embeds, with `include_str!`,
`tests/fixtures/watdiv-stress-100-stats.tsv`: #68's `relstats.txt`, 61
relations with tuples, distinct subjects and distinct objects, about 2 KB,
with a provenance header. It runs q0020, q0034 and q0008 through the three
rewrites and the planner, and asserts the prototype's `estdp_sf` plans by
variable name:

| Query | Plan | Exact cost (#68) |
|---|---|---|
| q0020 | K0 V2 V3 V4 V5 V0 V6 V1 | 1.81e4 |
| q0034 | V12 V0 K0 V9 V10 V11 V4 V5 V6 V7 V8 V2 V1 | 9.67e5 |
| q0008 | V0 V1 V10 V11 V6 V8 V7 V2 K0 V3 V4 V5 | 8.85e7 (the documented regression) |

It needs no data and no Java, and runs under Miri.

**Mutation checks.** Each mutant is confirmed to have applied, against
committed fixes:

- drop the denominator: the plan fixture fails;
- drop the readiness test in the DP: the `debug_assert` and the plan fixture
  fail;
- flip the tie-break: the determinism test fails;
- remove the `MissingStatistics` check: the `Database` test fails.

## Verification beyond CI

1. **Parity over all 124 templates.** A scratch harness outside the repo
   reads `queries-124.yml` and `relstats.txt`, prints the Rust plans and
   diffs them against `planners.jsonl`'s `estdp_sf` plans. Expect identity,
   except possibly the templates with `Select_*` views: the prototype
   measured the filtered view, and the Rust planner uses its base's
   statistics. Every differing plan is scored with `bindings.py`'s exact
   counter, inside the README's `systemd-run` memory cap. The result goes in
   the doc and on the issue.
2. **Smoke run.** `kermit join` on q0020 with `--optimiser cost-based`,
   reading the existing prelim WatDiv cache without writing it, against
   `cardinality`. Both current optimisers took over 30 s; the expectation is
   milliseconds, with the row count matching DuckDB (0).
3. **Planning time** for q0034, recorded in the doc.

## Verification (gate)

Inside `nix develop`, with `CARGO_BUILD_JOBS=2`: `cargo test`, `cargo clippy
--all-targets` with `-Dwarnings`, `cargo fmt --all --check`, `cargo doc
--workspace` with `-Dwarnings`, `cargo miri test` (CI's exclusions and
flags), and kermit-lab's pytest with `KERMIT_BIN`. Before trusting a local
fmt run, compare the flake's nightly with CI's (CLAUDE.md, "The flake's
nightly must track CI's").

## Documentation

- `docs/optimisers/cost-based.md` from the template: the cost function and
  search (in place of "Ranking function"); statistics consumed and the
  `required_statistics` contract; complexity, O(states × (V + P)) bounded by
  the budget, with the measured planning time; a worked micro-example, a
  q0020-shaped three-atom query with numbers; **Regressions and
  limitations**: q0008 (2.32×), q0360 (1.25×), q0070 (1.01×) against
  `cardinality`, the subject-first ceiling that #82 lifts, selection views,
  the independence assumption, the fallback; and the parity result.
- `docs/optimisers/lexicographic.md`, `cardinality.md`: one-line cross-links.
- `CLAUDE.md`: the store is `Database<R>` (workspace architecture, entry
  points, gotchas); `QueryOptimiser` implementations and `StatisticsLevel`
  (key trait hierarchy); the optimiser recipe gains "declare
  `required_statistics()` if `plan` reads more than tuple counts"; the
  component docs list.
- `ARCHITECTURE.md`: the catalog in the data flow, and the optimiser list.

## Commit sequence

The implementation plan
(`docs/superpowers/plans/2026-10-05-cost-based-optimiser.md`) refines this
into one commit per task.

1. `docs(spec): cost-based query optimiser (#81)` (this file).
2. `feat(algos): per-column distinct statistics and StatisticsLevel` —
   `RelationStats`, `CatalogStats`, `distinct_per_column`, `required_statistics` with
   its default, `predecessor_masks`.
3. `refactor(db): Database<R> catalog` — the type, `JoinFamily::for_each_tuple`,
   the entry points, `MissingStatistics`, `Engine = Database<R>`, every
   caller migrated. No behaviour change: every existing optimiser runs at
   `TupleCounts`.
4. `feat(algos): CostBasedOptimiser (#81)` — the optimiser, its unit tests,
   the WatDiv plan fixture.
5. `feat: --optimiser cost-based (#81)` — the enum variant, re-exports, join
   suite invocations, the LUBM row, the CLI test.
6. `test: run WatDiv and subject-position tests under every optimiser`.
7. `docs: cost-based optimiser (#81)` — the component doc, cross-links,
   CLAUDE.md, ARCHITECTURE.md, parity results.

## Resolved questions

- **Where statistics live** (user, 2026-10-05): a `Database<R>` catalog in
  `kermit::db`, chosen after weighing its API cost (about 10 call sites)
  against per-structure statistics (moves `insertion`/`space`, Priority 6)
  and a lazy cache (interior mutability).
- **When they are gathered** (user, 2026-10-05): only when the optimiser
  declares it needs them, with a typed error for a mismatch.
- **Name** (user, 2026-10-05): `cost-based`.
- **Test scope** (user, 2026-10-05): parameterising `watdiv_correctness.rs`
  and `subject_position_constant.rs` over every optimiser is in scope.

## Acceptance (issue #81)

| Criterion | Where |
|---|---|
| `--optimiser cost-based` on `join`, `bench join`, `bench run` | `Optimiser::CostBased` through `ValueEnum`; `cli_optimiser_choice.rs` |
| Join-suite invocations for every valid pair; `lubm_mini_oracle.rs`; a `lubm_cardinalities.rs` row | Testing, breadth |
| Plans for q0020 and q0034 match #68's simulated costs; regressions documented | The WatDiv plan fixture (identical plans), the 124-template parity run, and the doc's regressions section |
| `docs/optimisers/cost-based.md` exists | Documentation |
| Lands before object-first tries and before the next sweep | Sequencing |
