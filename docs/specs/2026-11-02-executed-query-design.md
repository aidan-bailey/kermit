# The Executed Query as a Type

**Date:** 2026-11-02
**Status:** Design approved in conversation, decision by decision and then section by section
(2026-11-02).
**Scope:** `JoinAlgo::join_for_each` takes an `ExecutedQuery`, a type that only a checked
constructor makes, and its indexes as a slice in id order. Constants, placeholders, repeated
variables, unbound variables and mismatched plans can no longer reach a join. The work the type
does once, untimed, at `prepare` leaves the timed `iteration`, so the report schema goes to v7.
**Related:**
- The LFTJ sweep of 2026-11-02 (766a88c4, 9dd7c2f9, ac7ae0ed), which found this.
- The per-query index builds (`docs/specs/2026-10-22-per-query-index-build-design.md`), which made
  `kermit::db::prepare` produce the executed query this type now names.
- Report v5, whose `iteration` already "excludes planning"; v7 finishes that.
- #99, the next sweep, which will be the first v7 measurement.

## Summary

Both joins are correct only on the *executed form* of a query: one distinct variable per column,
no constant, no `_`, every variable carried by some atom, and a plan valid for it.
`kermit::db::prepare` always produces that form, but the joins' public `join_for_each` accepts any
`JoinQuery`, and nothing checks it. `ExecutedQuery` makes the form a type, built once by a total
constructor that holds the query, its analysis and its checked plan. The joins read the analysis
and plan from it instead of recomputing them on every call. Atoms name their index by id, so
`execute` passes the indexes it already holds as a slice instead of building a map.

## Motivation

The joins, called directly on four queries that `prepare` would have rewritten (a probe, since
deleted). The relations were `{(1,2),(3,3)}`, `{(1,2,1),(1,2,5)}`, `{(1,5),(2,6)}` and
`{(1,5),(1,6),(2,7)}`.

| Query | Right answer | LeapfrogTriejoin | HashTriejoin |
|---|---|---|---|
| `R(X, X)` | `[[3]]` | `[[1], [3]]`, silently | panics: "every iterator is at its last level…" |
| `R(X, Y, X)` | `[[1, 2]]` | rejected, by the wrong rule ("requires variable 1 bound before 0") | the same |
| `R(X, c5)` | `[[1]]` | `[[1], [2]]`, silently | panics: "range end index 2 out of range…" |
| `R(X, _)` | `[[1], [1], [2]]` | `[[1], [2]]`, silently | panics: the same |

The cause: `analyse` skips constants and `_` and lists a repeated variable twice, and
`QueryPlan::validate` compares the plan with that analysis only. The `earlier != later` guard in
`validate` is dead code, since `position[x] > position[x]` is never true. Nothing in the CLI,
`bench` or `kermit::db` reaches these cases. A direct caller of the public trait does, and LFTJ
then answers wrongly without an error.

Separately, every timed `iteration` sample pays for work whose result is fixed at `prepare`:
`execute` clones the query, both joins run `analyse` and `validate`, and `execute` builds a
`HashMap<String, &R>` with one string per atom. The first three cost about 0.46 µs per join on a
3-atom triangle and 1.5 µs on an 8-atom chain (release build, measured 2026-11-02); the map was
not timed separately.

## Decisions

1. **The type, not a check (option C).** A runtime check at the joins (option B) was the smaller
   change. The type makes the bad shapes impossible to express, following Priority 5 ("encode
   invariants in types").
2. **The analysis and plan leave the timed region, and the schema goes to v7 (option a).**
   CLAUDE.md's rule is that changing what a metric measures needs a bump, and the change is made
   before #99, the next sweep.
3. **Atoms name their index by id, and `join_for_each` takes `&[DS]` (option i).** `execute` then
   passes the `Vec<R>` it holds, with no map, no string clones and no `"#k"` parsing.
4. **An empty body yields one empty row.** That is the answer to an empty conjunction, and what
   `kermit::db` already emits per unit of multiplicity. Both joins agree on it, and `execute` drops
   its special case.
5. **A slice of the wrong length panics; it is not an error value.** It is the one input the type
   cannot check. `kermit::db` cannot produce it, and an invalid plan panics today.

## Design

### `ExecutedQuery` (`kermit-algos/src/executed.rs`, re-exported at the crate root)

```rust
#[derive(Clone, Debug)]
pub struct ExecutedQuery {
    query: JoinQuery,         // as given: for Display and introspection
    analysis: QueryAnalysis,  // analyse(&query)
    plan: QueryPlan,          // valid for `analysis`
    atom_indexes: Vec<usize>, // atom j reads index atom_indexes[j]
    index_count: usize,       // distinct atom names
}

impl ExecutedQuery {
    pub fn new(query: JoinQuery, plan: QueryPlan) -> Result<Self, ExecutedQueryError>;
    pub fn query(&self) -> &JoinQuery;
    pub fn analysis(&self) -> &QueryAnalysis;
    pub fn plan(&self) -> &QueryPlan;
    pub fn atom_indexes(&self) -> &[usize];
    pub fn index_count(&self) -> usize;
}
impl fmt::Display for ExecutedQuery; // the query's own Display
```

`atom_indexes` numbers the distinct atom names by first appearance, so a `plan_indexes` query
(`#0`, `#1`, … by first appearance) gets id `k` for `#k`, `R(Y, X)` gets `[0]`, and the self-join
`R(X, Y), R(Y, Z)` gets `[0, 0]`. `index_count` is the number of distinct names.

`new` checks, in this order, and returns the first failure:
- each body atom, column by column:
  - `Constant { atom, column }` for a `Term::Atom`;
  - `Placeholder { atom, column }` for `_`;
  - `RepeatedVariable { atom, variable }` for a variable the atom already carries;
- `UnboundVariable { variable }`: a head variable no atom carries, in head order;
- `Plan(PlanError)`: `plan.validate(&analysis)`.

`atom` and `column` are 0-based positions, and `variable` is the variable's name. The error
derives `Debug, Clone, PartialEq, Eq`, has a `Display` per variant and implements
`std::error::Error`, with `source` for `Plan`. The head's own shape (distinct variables, no
constant) is not checked here: the joins do not read the head, and `kermit::db`'s validation keeps
that rule, which its head-prefix slice relies on. A query with no atoms is valid.

`QueryPlan::validate` stays public and loses the dead `earlier != later` guard. `R(X, Y, X)` now
fails as `RepeatedVariable` before the plan is checked.

### `JoinAlgo` and the two joins

```rust
fn join_for_each<S: FnMut(&[usize])>(query: &ExecutedQuery, indexes: &[DS], emit: S);
```

The contract: `indexes[id]` is the index that `atom_indexes` names. The join emits every variable
of the query in canonical order (head first), and an empty body emits one empty row. It panics if
`indexes.len() != query.index_count()`, with the message
`<Algo>::join_for_each: {len} indexes for a query that reads {index_count}`. Neither join calls
`analyse` or `validate` any more.

- **LeapfrogTriejoin:** one trie iterator per atom, from `indexes[atom_indexes[j]]`, so a
  self-join opens two iterators over one index. It reads `analysis().predicate_variables` and
  `plan().variable_ordering`, and gains `if atom_indexes is empty { emit(&[]); return }`.
- **HashTriejoin:** collects its per-atom relations the same way. It drops its early return for a
  variable no atom carries, which `UnboundVariable` now rules out. A test confirms that its empty
  body yields one empty row; if it does not, it gains the same branch as LFTJ.

Each join still builds its own per-atom iterators inside the call.

### `plan_indexes` and `kermit::db`

- `IndexPlan` becomes `{ query: ExecutedQuery, indexes: Vec<IndexSpec> }`. `plan_indexes` builds
  the `#k` query and the translated plan as today, then
  `ExecutedQuery::new(query, plan).expect("a rewritten, validated query is in executed form")`,
  which replaces its `debug_assert` on `validate`. A `debug_assert` checks that each atom's id is
  the `k` of its `#k` name.
- `Prepared` holds the `ExecutedQuery` in place of its query and plan. `query() -> &JoinQuery` and
  `plan() -> &QueryPlan` stay, as views of it, and `executed() -> &ExecutedQuery` is added.
- `execute` keeps its early return for multiplicity 0 and its `debug_assert` on the specs, then
  calls `JA::join_for_each(prepared.executed(), &indexes.relations, …)`, lending each row's head
  `multiplicity` times. Its map and its empty-body branch go. An all-constant query now runs
  through the join and gets its one empty row, multiplied as before.
- `Indexes::get(&str)` stays as library API, off the join path. `ExecutionFamily`, `compute_join`
  and the bench runners all call `execute`, so nothing above it changes.

### Report schema v7

- `REPORT_SCHEMA_VERSION` (`kermit/src/bench_report.rs`) and kermit-lab's `SCHEMA_VERSION`
  (`python/kermit-lab/kermit_lab/__init__.py`) go from 6 to 7. The existing test pins them
  together.
- kermit-lab's `MEANING_CHANGES` (`loader.py`) gains a row: `EXECUTED_QUERY_SCHEMA = 7`, the
  change "`iteration` and `end_to_end` exclude the join's per-query setup (query clone, analysis,
  plan check, index map)", and the metrics comparable across it: `build`, `space`, and `bench
  ds`'s metrics. A load mixing v6 and v7 is refused unless `allow_mixed_schema=True`.
- `docs/specs/bench-report-schema.md` gains a v7 change-log row, and its versioning-policy bullet
  lists v7 among the meaning-change bumps.
- No field or axis changes, and no back-fill is needed.

## Testing

- `executed.rs` unit tests:
  - one test per error variant;
  - the four motivation shapes, each rejected (`R(X, Y, X)` as `RepeatedVariable`);
  - id numbering, including the self-join's `[0, 0]`;
  - a valid query's accessors, and its `Display` matching the source query.
- Each join: a `#[should_panic]` test on the length message, and an empty body yielding exactly
  one empty row.
- Migrated to the new API: LFTJ's `join_for_each_emits_head_order_under_a_reordering_plan`,
  HashTriejoin's `joined` helper and its other direct caller, and `index_plan.rs`'s plan
  assertions.
- kermit-lab: a pytest that a mixed v6/v7 load is refused, and allowed under
  `allow_mixed_schema=True`, mirroring v5's.
- Unchanged, and green: everything through `kermit::db`. That is the join suites, `all_orders`,
  `lubm_mini_oracle`, `lubm_cardinalities` (inside `nix develop`), `cli_query_errors` and the
  lifecycle tests, the all-constant ones above all. `result_allocation.rs` keeps its "nothing per
  result row" checks; an exact count that drops because the clone and map are gone is updated,
  and the change reported.

## Out of scope

The optimisers, the data structures, `kermit::db`'s query validation and its head-shape rules,
and the per-join iterator setup inside each algorithm.

## Sequencing

1. `ExecutedQuery`, its error and its tests (kermit-algos, additive).
2. The switch, in one commit, since a trait signature change spans the crates: `JoinAlgo`, both
   joins and their tests, `plan_indexes` / `IndexPlan`, the dead guard, and `kermit::db`.
3. Report v7: Rust, kermit-lab and the schema doc.
4. Documentation.

## Acceptance

- None of the four motivation shapes can reach a join; `ExecutedQuery::new` rejects each.
- `execute` passes a borrowed `ExecutedQuery` and a slice: no clone, map or name parsing per call.
- The gate is green: `cargo test --workspace` (with `lubm_cardinalities` run, not skipped),
  `cargo clippy --workspace --all-targets` with warnings as errors, `cargo fmt --all --check`,
  `cargo doc --workspace` with warnings as errors, and kermit-lab's pytest with `KERMIT_BIN`.
- kermit-lab refuses a mixed v6/v7 load.

## Documentation

- `kermit-algos/src/join_algo.rs`: the signature, the contract, the empty body and `# Panics`.
- Module docs of `index_plan.rs`, `lib.rs`, `sorted/mod.rs` and `hash/mod.rs`.
- `docs/algorithms/leapfrog-triejoin.md` and `hash-triejoin.md`: the plan check moves into the
  type.
- `ARCHITECTURE.md`: the planner-to-executor handoff.
- `CLAUDE.md`:
  - the `QueryOptimiser` bullet ("`QueryPlan` is the only type crossing planner → executor");
  - the "Adding a new join algorithm" recipe: read the analysis and plan from the type, do not
    validate, handle an empty body, take `&[DS]`;
  - the "Load, prepare, build, execute" gotcha;
  - the JSON-reports gotcha ("currently `7`", and what v7 changed).
