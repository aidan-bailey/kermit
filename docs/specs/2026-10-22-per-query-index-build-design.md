# Per-Query Index Builds: Flat Catalog, Filtered Views, One Timed Build

**Date:** 2026-10-22
**Status:** Design approved in conversation (2026-10-22), decision by decision and then
section by section.
**Scope:** Relations are read into flat `Tuples`, untimed, and no structure is built at load.
For each query, kermit plans from statistics of the flat data, then builds exactly the
indexes the plan reads, each one a relation filtered by the query's constants and repeated
variables and laid out in plan order, and times that build. Every structure, build mode and
command moves to this lifecycle. The `stored` column-order policy, the const and selection
views and the reordered-copy protocol are retired. The report schema becomes v5.
**Related:**
- Freitag et al., VLDB 2020: §2.2 (indexes "have to be built on-the-fly during query
  processing"), §3.2 (separate build and probe phases), §3.3.2 (tuples materialised once,
  partitioned while materialised), §4 (the attribute order is optimised freely);
- Veldhuizen 2014 (Leapfrog Triejoin), §3.4: the rewrites kermit's const, selection and
  orientation steps implement today, with indexes kept for later queries;
- #93 (column orders: the `stored` policy and the copies this design replaces);
- #82 (keep copies across queries; rescoped by this design);
- #105 (paper convergence), #100 (lazy expansion on selective queries), #99 (the next sweep);
- #111 (`Tuples`), #101 (partitioned materialisation), #91, #94 and #107 (the build modes);
- kraken (`aannleax/kraken`), whose trie systems build per join but in an untimed `load`.

## Summary

Today:
- `bench run` builds every relation a benchmark declares once per cell, in file column
  order, timed as `insertion`, whether a query reads it or not.
- Under `--column-orders any`, each query's reordered copies are built from retained
  file-order tuples, timed as `copies`. Under the default, `stored`, none are.
- Constants and repeated variables are handled during the join, by one-value relations
  (`Const_*`) and equality views (`Select_*`), in both join families.
- Every timed join re-plans.

This design splits a query's life into four steps:
- **Load (untimed):** read each relation the queries name into a `Catalog`: flat `Tuples`
  plus statistics. No structure is built.
- **Prepare (untimed, once per query):** validate, rewrite `_`, extract one access per atom
  (relation, filter, kept columns), gather exact statistics for filtered accesses, plan
  freely, and turn the plan into index specs (relation, filter, column order), merging
  identical ones.
- **Build (timed):** each spec reads the flat area through a borrowed `TupleView` (filter
  plus column order) and copies its rows exactly once, in the structure's own way. An atom
  with only constants becomes a row count.
- **Execute (timed):** the unchanged `JoinAlgo` runs over plain indexes; the entry point
  projects to the head and applies the all-constant multiplicity.

Freeing what a timed step built is never timed.

## Motivation

1. **It is the paper's model.** Freitag et al. (§2.2): indexes "are required on different
   permutations of attributes from the input relations. The number of such permutations is
   usually much too large to store the corresponding indexes persistently. Therefore, they
   have to be built on-the-fly during query processing." With prebuilt base structures,
   kermit cannot show the trade the paper is about: structures that probe fast but build
   slowly against ones that build cheaply. kraken's README names the same asymmetry: its
   tries are built in an untimed `load` while the hash-join engines build inside the timed
   query.
2. **Build cost depends on the policy and the catalogue.** `insertion` builds every declared
   relation per cell; `copies` builds only `any`'s reordered relations, per query.
3. **Plans depend on the structure.** Statistics come from the built structures
   (`kermit/src/db/database.rs:124`). The sorted tries count distinct rows and HashTrie
   counts duplicates, so on a file with duplicate rows `cardinality` and `cost-based` can
   plan differently per structure.
4. **Build modes are skewed by one copy.** `radix:K`, `parallel:N`, `presized:N` and
   TreeTrie's `parallel:N` copy rows inside the timer; `serial` and `bulk` do not. Every
   build-mode comparison so far (#91, #94, #101) carries that confound.
5. **Selections live in the join.** A one-value relation, an equality view and a wrapper
   enum per join family, plus `orient`'s equality-class remap (the #93 bug class), exist to
   adapt queries to prebuilt, unfiltered, file-ordered structures. Per-query filtered builds
   in plan order absorb all of it.

## Decisions

1. **The timed build starts at the borrowed flat area.** Each index copies its rows exactly
   once, in its own way, inside the timer. That is the paper's build phase and removes the
   build-mode skew (Motivation 4). Rejected: permuting outside the timer (today's
   `insertion`: keeps the skew and never measures the copy), and permuting inside it
   (today's `copies`: the partitioning modes copy twice).
2. **`stored` is dropped; the planner is always free.** It existed because base structures
   were prebuilt in file order, so that order was free and every other cost a copy. With
   every index built per query, it only constrains the planner. A test that runs every
   pattern under every variable order replaces the plan variety it gave the join suite.
3. **Planning is untimed,** once per query, before any timer. Plans no longer depend on the
   structure, so planning would add the same constant to every cell.
4. **Constants and repeated variables are applied while building,** and the views are
   deleted rather than kept beside the filter (Freitag: selections run below the join).
   The filter is closed to the two equalities the query language can express: a term is a
   variable, a constant or `_` (`kermit-parser/src/join_query.rs:9-15`).
5. **An atom with only constants is counted before the join.** No index is built for it:
   zero matches means no answers; otherwise its count multiplies each result on a structure
   that keeps duplicates.
6. **Freeing is never timed.** Every timed step returns what it built, and Criterion frees
   it after the timer stops. Today `bench run`'s `insertion` frees inside the timer
   (`kermit/src/bench/run.rs:298`) while `bench ds`'s does not (`kermit/src/bench/ds.rs:89`).
7. **An explicit lifecycle:** `Catalog`, `prepare`, `build`, `execute`. Rejected: extending
   `Database<R>`'s add-and-clear copy protocol to every atom (its joins re-plan inside the
   timer, and the store stays tied to one structure type), and indexes of row ids into the
   shared flat area (every structure and both algorithms would read rows through an
   indirection, and HashTrie would lose its own buffer).

## Background: what this replaces

- **Loading.** `RelationFamily::load` (`kermit/src/execution.rs:379`) builds a relation
  from a file; `load_with_tuples` (`:398`) also keeps the file-order batch for rebuilds.
  `bench run` loads every relation of the definition per cell (`kermit/src/bench/run.rs:113-121`,
  called per cell from `kermit/src/main.rs:922-924`).
- **The engine.** `Database<R>` (`kermit/src/db/database.rs:34`) holds built relations,
  statistics gathered from them, and the current query's copies.
- **Planning.** `plan_query` (`kermit/src/db.rs:158`) validates, rewrites constants
  (`kermit-algos/src/const_rewrite.rs`), placeholders and repeated variables
  (`selection_rewrite.rs`), plans, and orients (`orient.rs`). `run_join` (`db.rs:245`)
  calls it on every join, inside the timed region (`db.rs:177`).
- **The policy.** `ColumnOrderPolicy` (`kermit-algos/src/optimiser/column_orders.rs:17`),
  `stored` by default, pins atoms through `Precedence::for_query` and rejects cyclic orders.
- **The views.** `sorted/` and `hash/` each hold `singleton.rs`, `selection.rs` and
  `iter_kind.rs` (about 920 lines of non-test code), wrapped by `JoinFamily` (`db.rs:52`).

## Design

### Lifecycle

```
files ── read (untimed) ──► Catalog: name → (header, Tuples) + statistics
                                │
query ── prepare (untimed) ─────┤  validate, rewrite `_`, extract accesses,
                                │  filtered statistics, plan, index specs
                                ▼
                          Prepared { executed query, plan, index specs, constant checks }
                                │
       build (TIMED) ───────────┤  each spec → TupleView → the structure's view constructor
                                │  each constant check → a row count
                                ▼
                          Indexes<R> { index id → R, multiplicity }
                                │
       execute (TIMED) ─────────┤  JoinAlgo::join_for_each over plain indexes,
                                │  head projection, multiplicity
                                ▼
                              rows → sink          (indexes freed after the timers stop)
```

### `TupleView` and `Filter` (`kermit-iters`, beside `Tuples`)

- **`Filter`** is a closed type: `constants: Vec<(usize, usize)>` (column, value) and
  `equalities: Vec<(usize, usize)>` (an earlier column, a later column with the same
  variable). Nothing else can be expressed. `Filter::none()` matches every row. Both lists
  are kept sorted, so equal filters compare equal.
- **`TupleView<'a>`**: a borrowed `&'a Tuples`, a filter and `columns`, where output column
  `i` reads source column `columns[i]`. It offers:
  - `source_rows()`: the flat area's row count, for splitting morsels by row id;
  - `matches(row)`, and `write(row, out)`, which writes a row's output columns;
  - `for_each(visit)`: the matching rows, in source order, through one reused scratch row;
  - `count()`: the matching rows; free on an unfiltered view;
  - `materialize() -> Tuples`: the matching rows in source order, sized exactly (counting
    first when filtered).
- `TupleView::all(&tuples)` is the identity view. A view with no output columns is legal;
  its `count()` is the multiplicity of an all-constant atom.

### Building from views (`kermit-ds`)

- **Constructors.** `Relation::from_view(header, view)` becomes the required constructor.
  `from_view_with_config`, `from_view_with_build_mode` and
  `from_view_with_config_and_build_mode` join the traits that own the matching
  `from_tuples_with_*` today. Every `from_tuples*` becomes a provided method,
  `from_view(header, TupleView::all(&batch))`: one copy, after which the batch is dropped.
  There is one build path, so test fixtures run the code the benchmarks time.
  `Configured<R, P>` and `BuiltWith<R, P>` pass views through. `insert` and `insert_all`
  are unchanged, and so are the file loaders' signatures.
- **One copy per build mode:**

| structure / mode | reads the view by | its one copy |
|---|---|---|
| TreeTrie `serial`; ColumnTrie `bulk`, `incremental` | `materialize()`, then `Tuples::sort` in place¹ | scratch, freed at the end of the build |
| TreeTrie `parallel:N` | `matches` while scattering row ids into partitions; `write` while each worker gathers | each worker's gathered partition |
| HashTrie `bulk`, `incremental` | `materialize()` | the trie's buffer |
| HashTrie `radix:K`, `parallel:N`, `presized:N` | `partition_rows` checks `matches` in its counting pass and calls `write` in its scatter pass | the partition-ordered copy, which is the trie's buffer |
| HashTrie lazy expansion | (unchanged) | `resolve` reads the trie's own buffer |

  ¹ At arity 5 and above, `Tuples::sort` gathers into a new buffer, a second copy, so there
  the sorted builds sort row ids against the view and gather once.
- **Invariants that hold:**
  - every build mode builds an equivalent structure from the same view: the same contents
    and capacities (Amendment 2); the radix family stays content-equivalent;
  - no build allocates per input row (`kermit/tests/tuple_allocation.rs`);
  - HashTrie's `space` is equal across its modes by construction, because every buffer is
    created by its own build at exact size. The capacity inheritance in
    `kermit-ds/src/morsel.rs:352-353` and the readers' load-bearing `shrink_to_fit`
    (`kermit-ds/src/relation.rs:511-514`) are no longer needed;
  - `root-capacity=tuples` sizes by `view.count()`; `presized:N` counts before
    partitioning;
  - an index's header holds the relation's kept attributes in output order: today's
    `index_header`, extended to projection.

### Preparing a query (`kermit-algos`, `kermit::db`)

`prepare(&catalog, &query, &planner) -> Result<Prepared, JoinError>`:

1. **Validate** against the catalog's headers. `CyclicAttributeOrder` goes with `stored`.
   `ReservedRelationName` goes too: index ids live in their own namespace, and relation
   names are `[A-Za-z][A-Za-z0-9_]*` (`kermit-parser/src/lib.rs:95-98`).
2. **Rewrite `_`** into fresh variables, unchanged, keeping #78's bag semantics.
3. **Extract one access per atom,** column by column: a variable's first occurrence becomes
   a kept column; a later occurrence becomes an equality with the first; a constant becomes
   a constant filter. The result is `Access { relation, filter, kept columns, variables }`,
   with kept columns in source order. An access with no kept columns is a constant check.
4. **Statistics per access.** An unfiltered access reads its relation's catalog statistics.
   A filtered access scans the flat area through its filter for exact counts, and for
   per-column distinct counts when the optimiser asks for them
   (`QueryOptimiser::required_statistics`). The catalog gathers its own statistics once, at
   that level; a planner asking for more than the catalog holds is `MissingStatistics`.
5. **Plan.** The optimiser plans the query of accesses, with statistics keyed by access id.
   The optimisers lose their ordering constraints (`Precedence::for_query`'s pinning and
   `check_attribute_order`); otherwise they are unchanged. `Planner` stays the single
   planning parameter, without its policy.
6. **Index specs.** Each access's kept columns are sorted by the plan position of their
   variables, giving `IndexSpec { relation, filter, columns }`. Identical specs merge, so a
   self-join that reads one relation one way builds it once. Specs get internal ids (`#0`,
   `#1`, …), and the executed query's atoms name those ids, with their variables in index
   column order. The plan is translated by variable name, as `orient` does today, since
   `analyse` renumbers body-only variables.

`Prepared` holds the executed query, the plan, the index specs, the constant checks and the
head's length.

### Building and executing (`kermit::db`)

- **`build`** (timed) turns each spec into a `TupleView` over its relation's flat tuples and
  calls the structure's view constructor, and turns each constant check into
  `view.count()`. A zero count means the query has no answers. Otherwise the counts multiply,
  clamped to 1 on structures that do not keep duplicates. A new associated constant,
  `Relation::KEEPS_DUPLICATES`, is `true` for HashTrie and `false` for the sorted tries.
  `build` returns `Indexes<R>`: the indexes by id, the multiplicity, and the specs they
  were built from.
- **`execute`** (timed) maps ids to indexes, calls `JoinAlgo::join_for_each`, and emits
  each row's head prefix (`&row[..head_len]`, as today) once per unit of multiplicity.
  Executing with indexes built from another `Prepared` is a programming error, caught by a
  debug assertion that the `Indexes`' specs equal the `Prepared`'s. Ids alone cannot catch
  it, since every `Prepared` numbers its specs from `#0`.
- **Library surface:** `Catalog::new(relations, statistics_level)`, `prepare`, `build`,
  `execute`; the conveniences `lftj_join[_for_each]::<R, JA>` and
  `hash_join[_for_each]::<R>` run all three. The hash entry point no longer takes `H`,
  which existed only to hash constants. `compute_join` goes through the conveniences.
- **Errors.** `JoinError` drops `CyclicAttributeOrder`, `ReservedRelationName` and
  `MissingIndex`; it keeps the head, arity, unknown-relation, unbound-variable and
  malformed-constant variants and `MissingStatistics`.

### Retired

- `kermit-algos`: `const_rewrite.rs` and `selection_rewrite.rs` (replaced by access
  extraction); `orient.rs`'s equality-class remap (the rest becomes the index-spec step);
  `sorted/` and `hash/`'s `singleton.rs`, `selection.rs` and `iter_kind.rs`;
  `optimiser/column_orders.rs`; the `Const_` entries in `CatalogStats`.
- `kermit::db`: `Database<R>`, `required_indexes`, `add_index`, `clear_indexes`,
  `build_index`, `JoinFamily`.
- The binary: `--column-orders`; `RelationFamily::load` and `load_with_tuples`;
  `ExecutionFamily`'s engine and copy methods.

### The binary (`kermit/src`)

- **Families.** `RelationFamily::build_index(&self, header, view) -> Rel` replaces
  `build_relation` and is still each family's one config- and mode-aware construction
  site. `ExecutionFamily` loses `Engine`, `build`, `build_from_tuples`, `relations` and the
  copy methods, and gains `build_indexes(&catalog, &prepared) -> Indexes<Rel>` and
  `join_for_each(&prepared, &indexes, sink)`. `JOIN_MUTATES` stays.
- **`bench run`.** Once per benchmark, before the loop over cells: validate every query
  from headers, as today, then read every relation the queries name into one `Catalog` at
  the optimiser's statistics level. Once per query: `prepare`, shared by every cell. Then,
  per cell and query, the metrics below.
- **`bench join`** is the same flow through `Workload::adhoc`; `--output` and the benchmark
  share one catalog, which removes today's double read and build.
- **`bench ds`** reads one relation; its `build` is `from_view` over the identity view, its
  `iteration` the structure's scan (unchanged), its `space` the built relation's heap bytes
  (unchanged), and its `end_to_end` a build plus K scans. Its Criterion functions become
  `{ds}/build`, `{ds}/iteration`, `{ds}/space` and `{ds}/end_to_end`.
- **`kermit join`** reads the catalog for the query's relations, then prepares, builds and
  executes. Its CSV output is unchanged.

## Parity with the paper

| | Freitag et al. | kermit after this design |
|---|---|---|
| Index lifetime | built per query (§2.2, §3.2) | built per query |
| Selections | below the join, before the build | applied while building |
| Materialisation | once per trie, partitioned while materialised (§3.3.2) | once per index; the radix family partitions while copying |
| Attribute order | optimised freely (§4) | planned freely |
| Trie levels | join key attributes only; other columns are payload (§3.2.1) | every kept column is a level: a new #105 row |
| Order strategy | Tributary Join's cost model (§4) | kermit's optimisers, unchanged |

## Placement under the standard

This is not an optimisation with an axis. Like #111, it changes the lifecycle every
structure goes through, unconditionally, and nothing selects the old one.

- **Old and new are told apart** by the report's `schema_version` (5).
- **Priority 6.** It touches all three structures and both algorithms, as one declared
  cross-cutting change. No build algorithm changes; only its input becomes a view. The join
  algorithms' code is unchanged; they lose the wrappers around their inputs.
- **The BuildMode tests** (`define_multiway_join_test_suite_for_build_mode!` and the
  equivalence tests) build from views; the standard's equivalence rules are unchanged.

## CLI, reports and kermit-lab

- **CLI.** `--column-orders` is removed from `join`, `bench join` and `bench run`.
  `--metrics` renames `insertion` to `build` and drops `copies`; the defaults become
  `build, iteration, space`. Every other flag is unchanged.
- **Metrics, all per query:**
  - `build`: `build(catalog, prepared)`, every index the query needs plus its constant
    checks. Criterion `iter_batched` with a no-op setup and `BatchSize::PerIteration`: no
    setup copy, at most one set of indexes alive, freed outside the timer.
  - `iteration`: `execute` over indexes built once, outside Criterion. Under `JOIN_MUTATES`
    each iteration gets a fresh build in untimed setup, as today. Planning is excluded.
  - `end_to_end`: `build` plus K × `execute`, with K = `queries_per_build`; it equals
    `build` + K × `iteration`.
  - `space`: the heap bytes of the query's indexes, one Criterion `space` function per query
    through `SpaceMeasurement`, replacing the per-relation and per-copy functions. Lazy
    HashTrie keeps the check that the footprint is as built.
  - `--verify` counts with `execute`, on fresh indexes under `JOIN_MUTATES`.
- **Report schema v5.**
  - `insertion` and `copies` are replaced by `build`; `iteration` excludes planning;
    `end_to_end` and `space` are per query.
  - The `column_orders` axis is removed. A new `index_rows` axis holds the rows the build
    copied, so build time can be normalised per row. `tuples` becomes the flat row count of
    the relations the query reads, the same for every structure.
  - One `index` metadata line per index: its id, relation, filter, column order, rows and
    bytes.
  - `REPORT_SCHEMA_VERSION` and kermit-lab's `SCHEMA_VERSION` both become 5, pinned together
    by the existing `cargo test`.
  - Criterion group names (`{workload}/{query}/{ds}/{algo}`) and
    `check_sweep_group_directories` are unchanged.
- **kermit-lab.** It refuses to load v4 and v5 reports together (`allow_mixed_schema=True`
  overrides); its presets rename `insertion` to `build`; the real-binary contract test is
  updated.

## Testing

- **The join suite.** `define_multiway_join_test_suite!` and its config and build-mode
  variants keep their patterns and invocations. The harness
  (`kermit/tests/common/utils.rs`: `JoinEntry`, `test_join`) moves to `Catalog`, `prepare`,
  `build` and `execute`. `define_multiway_join_test_suite_with_column_orders!`, `AnyOrders`
  and `StoredOrders` go, so each alias × algorithm × optimiser runs once.
- **All orders (new).** Every suite pattern, plus constant cases (`R(X, c1)`,
  `R(X, X, c1)`, an atom with only constants, and a mix), under every variable order, forced
  by a test-only optimiser that returns a fixed order, on one alias per structure, against
  the nested-loop oracle in `kermit/tests/column_orders_equivalence.rs`. It replaces that
  file and `column_orders_any.rs`, whose six queries move into it.
- **Views and filters.**
  - `kermit-iters`: `TupleView` unit tests for filters, equalities, projection, permutation,
    views with no columns, exact `materialize` capacity and morsel row ranges.
  - `kermit-ds`: every structure × build mode, built from a filtered and permuted view, equals
    the same build from that view's materialised batch, in contents and capacities,
    extending the `identity::` equivalence helpers.
  - `kermit::db`: an all-constant atom multiplies results on HashTrie and not on the sorted
    tries, and a zero count yields no rows.
- **Expectations that must not move.** `lubm_mini_oracle.rs`, `lubm_cardinalities.rs`,
  `watdiv_correctness.rs` and `subject_position_constant.rs` run pushdown end to end, since
  every query in them carries constants.
- **Allocation.** `tuple_allocation.rs` covers builds from views; `result_allocation.rs`
  covers the streamed `execute`.
- **CLI and kermit-lab.** `cli_column_orders.rs` is deleted; the error shapes that no longer
  exist leave `cli_query_errors.rs`; tests that name metrics follow the rename; pytest
  follows v5.

## Measurement

There is no A/B against v4: every build metric changes its definition. The next
authoritative sweep (#99) is the first v5 measurement.

Expected effects of applying selections in the build, compared with building whole
relations per query (decision 4's rejected alternative). These are unmeasured predictions
from per-row costs (a filtered scan at about 1–2 ns per row against a full build at about
20–100 ns), for the sweep to confirm or refute:

| metric | LUBM, WatDiv (queries carry constants) | JOB, SNAP, H2O (no constants) |
|---|---|---|
| `build` | much lower for selective constants: a full build becomes a scan plus a small build. Higher where several unselective filters hit one large relation, each scanning and building it | no difference |
| `iteration` | modestly lower: smaller indexes, one fewer level per constant | at most a few percent from the removed wrappers, inside the ±3% rebuild bound |
| `space` | lower, by about the fraction of rows that match | no difference |
| `end_to_end` | much lower | no difference |

Lazy HashTrie loses its constant-driven wins, since a filtered index has little to defer,
and the parallel build modes may lose to `serial` and `bulk` on small filtered inputs.

## Out of scope

- Keeping indexes across queries (Veldhuizen's model). #82 is rescoped to an index cache over
  this design.
- Selections beyond the two equalities the query language can express.
- Join-key-only trie levels (Freitag §3.2.1): a new #105 row.
- An optimiser that counts index builds.
- Making set and bag semantics agree across structures.
- #103 (ColumnTrie's parallel build), unaffected; it builds on views when it resumes.

## Sequencing

One branch, landing in one push with one schema bump. Each phase leaves the CI gate green.

1. **Views.** `kermit-iters` and `kermit-ds`: `Filter`, `TupleView`, the `from_view*`
   constructors, every build mode reading views, and `from_tuples*` through identity views.
   Answers and the report schema are unchanged.
2. **The new pipeline, beside the old.** `kermit-algos` and `kermit::db`: access extraction,
   filtered statistics, index specs, `Catalog`, `prepare`, `build`, `execute` and constant
   checks. The test harness and the all-orders test move to it.
3. **The binary, then deletion.** Families, `bench run`, `bench join`, `bench ds`,
   `kermit join`, metrics, CLI flags, report v5 and kermit-lab v5. Then delete
   `Database<R>`, the copy protocol, the views and wrappers, `stored` and `column_orders`,
   with the tests that targeted them.
4. **Documentation** (below).

## Acceptance

1. No structure is built at load: `bench run` reads each relation once per benchmark, and
   only relations a query names.
2. Every index is built from a view and copies its rows once, in every build mode, pinned by
   tests.
3. The views, the wrappers, `stored`, `--column-orders` and the copy protocol are gone;
   `JoinAlgo` is unchanged.
4. Answers are unchanged: the join suite, the all-orders test and the LUBM and WatDiv
   oracles pass.
5. The allocation tests pass.
6. `REPORT_SCHEMA_VERSION` and kermit-lab's `SCHEMA_VERSION` are both 5, and the contract
   test passes.
7. The CI gate passes: test, clippy (`-Dwarnings`), fmt, doc (`-Dwarnings`) and python.
8. The documentation below is updated.

## Documentation

- `CLAUDE.md`: Workspace Architecture, Key Trait Hierarchy (`Relation`, `Planner`,
  `Tuples`), Testing Patterns, the recipes (Extending the System), and the Gotchas on the
  const, placeholder, selection and orientation rewrites, `CyclicAttributeOrder`, copies,
  "a `--name` per policy", JSON bench reports and query validation.
- `ARCHITECTURE.md`.
- `docs/algorithms/leapfrog-triejoin.md` and `hash-triejoin.md` (no views);
  `docs/data-structures/*.md` (builds from views); `docs/optimisers/*.md` (no column-order
  policy).
- `docs/specs/bench-report-schema.md` (v5), `optimization-standard.md` (equivalence from
  views), `benchmarking-architecture.md`.
