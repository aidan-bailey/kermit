# Query Optimiser Design

**Status:** Approved design, pre-implementation.
**Date:** 2026-07-08

## Problem

Kermit has no query optimiser. For the LFTJ family, the variable ordering
(global attribute order, GAO) *is* the query plan, and it is currently
computed inside `kermit-algos/src/leapfrog_triejoin.rs::global_attribute_order`
with one hardcoded heuristic: Kahn's topological sort over the column-order
constraint DAG, popping the smallest canonical variable index first. There is
no way to plug in a different ordering policy, no way for an ordering to
consult relation statistics (e.g. tuple counts), and no benchmark axis for
comparing orderings — even though ordering choice is one of the largest
performance levers for worst-case-optimal joins and a natural thesis
experiment.

Two proto-optimiser seams already exist but are not named as such:

- `rewrite_atoms` (`kermit-algos/src/const_rewrite.rs`) is a query→query
  rewrite pass.
- `global_attribute_order` defines the space of *valid* plans: exactly the
  topological orders of the column-order DAG.

## Goals

1. A first-class, DS-agnostic **`QueryOptimiser`** component with a classic
   optimizer/executor separation: the optimiser produces a **`QueryPlan`**,
   algorithms consume it.
2. One new policy: **cardinality (smallest-relation-first)** — prefer
   variables appearing in the smallest relations, driven by tuple counts.
3. The current behaviour preserved bit-for-bit as the default policy
   (**lexicographic**), so existing benchmark measurements remain valid and
   ablations have a control arm.
4. A benchable third dimension: (data structure × algorithm × optimiser),
   surfaced through the CLI and bench-report axes so `kermit-lab` can pivot
   on it with no new tooling.

## Non-goals (v1)

- Atom/body-predicate reordering in the plan.
- Per-column statistics (distinct counts, histograms) or cost *models* —
  v1 policies are heuristic rankings, not cost estimation.
- Any change to how algorithms execute once given an ordering.
- Runtime plan caching.

## Architecture

### Components

New module `kermit-algos/src/optimiser/` (no new crate):

| File | Contents |
|---|---|
| `mod.rs` | `QueryOptimiser` trait, `CatalogStats`/`RelationStats`, re-exports |
| `analysis.rs` | `QueryAnalysis` + `analyse(&JoinQuery)` — canonical variable indexing (head-vars-first), `predicate_variables`, column-order constraint edges. Hoisted from passes 1–3 of `build_variable_index`. |
| `plan.rs` | `QueryPlan { variable_ordering: Vec<usize> }` + `validate(&QueryAnalysis)` |
| `ordering.rs` | shared `topological_order(analysis, rank_fn)` — Kahn's loop with pluggable ready-set priority |
| `lexicographic.rs` | `LexicographicOptimiser` (default; reproduces current behaviour) |
| `cardinality.rs` | `CardinalityOptimiser` (smallest-relation-first) |

### The trait

```rust
pub trait QueryOptimiser {
    fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan;
}
```

Object-safe (`&self`), held as `Box<dyn QueryOptimiser>` by
`DatabaseEngine` — runtime-selectable, no monomorphization of the engine per
optimiser and no growth in `instantiate_database`'s match.

### Canonical indexing is shared, not serialised

Planner and executor must agree on variable numbering. Both call the same
deterministic `analyse(&JoinQuery)`; the plan carries canonical indices.
This is a *move* of existing logic, not a rewrite — LFTJ and HashTriejoin
already share `build_variable_index`/`global_attribute_order`.

### Validity

The space of valid GAOs is exactly the set of topological orders of the
column-order DAG (each relation contributes `col[i] → col[i+1]` edges). The
two provided optimisers are valid **by construction** because they only
choose which ready vertex Kahn's algorithm pops next. Because a plan is
data and `QueryOptimiser` is an open trait, the executor additionally
asserts `plan.validate(&analysis)` — an internal-invariant panic on failure,
consistent with the LFTJ `open`-after-`at_end` discipline (Priorities #5).
The existing cyclic-constraint panic (e.g. `r(X, Y), s(Y, X)`) moves into
the shared ordering machinery and still fires.

### Statistics seam

New one-method trait in `kermit-iters` (precedent: `HeapSize`):

```rust
pub trait Cardinality {
    /// Number of distinct tuples stored.
    fn tuple_count(&self) -> usize;
}
```

Implemented by `TreeTrie`, `ColumnTrie`, `HashTrie` via an O(1) counter
maintained on insert (incremented only when the inserted tuple was actually
new — duplicate inserts must not inflate the count). Touching all three DSs
is in-scope for this change: the seam is the feature.

The optimiser never sees data structures. `DatabaseEngine::join` (and
`hash_join`) gather counts into plain data:

```rust
pub struct RelationStats { pub tuples: usize, pub arity: usize }
pub struct CatalogStats { /* BTreeMap<String, RelationStats> */ }
```

Synthetic `Const_*` predicates enter as `tuples: 1`, so the cardinality
policy binds constants first with zero special-casing. Plain-data stats keep
the optimiser DS-agnostic and unit-testable with literal maps.

### Data flow

`JoinAlgo::join_iter` gains the plan as an explicit input (breaking trait
change; both impls and all call sites updated in this work):

```rust
fn join_iter(plan: &QueryPlan, query: JoinQuery, ds: HashMap<String, &DS>)
    -> impl Iterator<Item = Vec<usize>>;
```

Deliberately **no** `Option<QueryPlan>` and no defaulted fallback method —
one entry point, no drift. Engine flow:

```
rewrite_atoms → gather CatalogStats (Const_* = 1)
             → optimiser.plan(rewritten, stats)
             → JA::join_iter(&plan, rewritten, ds_map)
```

Planning runs on the **rewritten** query because `Const_*` predicates impose
real column-order constraints the GAO must respect. Inside LFTJ,
`build_variable_index` shrinks to: `analyse()` + `plan.validate()` + use
`plan.variable_ordering`. The head-order output remapping is untouched, so
tuple *content* is identical under every valid plan; only enumeration order
may differ.

`DatabaseEngine<R, JA>` gains `optimiser: Box<dyn QueryOptimiser>`
(constructors default it to `LexicographicOptimiser`; a `with_optimiser`
constructor sets it). `instantiate_database` gains an optimiser argument,
passed through — not matched on. The `hash_join` free function gains an
`optimiser: &dyn QueryOptimiser` parameter and mirrors the same flow.

### CLI and bench surface

- New CLI enum in the style of `JoinAlgorithmSelector` but **without an
  `all` sweep variant** (the existing `-i all`/`-a all` sweep loop is a
  filed known-issue; the optimiser dimension stays out of it in v1):
  `--optimiser <lexicographic|cardinality>`, default `lexicographic`, on
  `join`, `bench join`, and `bench run`. Long-only: `-o` is already taken
  by `--output` on `join`/`bench join` (clap derives shorts from field
  names), and a short that exists on some subcommands but not others is
  worse than none.
- Bench reports gain a conventional top-level axis key **`optimiser`**
  alongside `data_structure`/`algorithm`. The axes map is open, so
  `schema_version` stays at 2; the key is documented in
  `docs/specs/bench-report-schema.md`. Optimiser ablation in `kermit-lab`
  becomes `kl.load()` + `groupby("optimiser")`.

## Testing

- **Suite coverage (Priorities #1):** `define_multiway_join_test_suite!`
  grows an optional optimiser argument (default: lexicographic).
  `kermit/tests/join_tests.rs` adds invocations running the 11 standard
  patterns with `CardinalityOptimiser` for every valid (DS, algorithm)
  pair. Under non-default optimisers the suite sorts both sides of result
  comparisons, because a different GAO may change result enumeration order
  (content is unchanged).
- **Unit tests:** planner determinism; cardinality ordering given literal
  `CatalogStats`; `plan.validate()` rejects a column-order-violating
  permutation; const-singletons rank as size 1; cyclic-GAO panic still
  fires; `tuple_count` unaffected by duplicate inserts (per DS).
- **CLI test:** `--optimiser cardinality` lands in the report JSON
  (mirroring `kermit/tests/cli_hash_trie_hasher_choice.rs`).

## Documentation

- New `docs/optimisers/` directory: `TEMPLATE.md`, `lexicographic.md`,
  `cardinality.md` — same skeleton discipline as algorithm/DS docs
  (ranking function, complexity, invariants, worked micro-example).
- New CLAUDE.md "Extending the System" recipe: **Adding a new query
  optimiser** (module file → trait impl → enum variant → CLI selector →
  suite invocation → doc), making the fourth extension axis as
  recognizable as the other three (Priorities #4).
- `ARCHITECTURE.md` data-flow update; `bench-report-schema.md` gains the
  `optimiser` key.

## Alternatives considered

- **Layout-style type parameter** (`LeapfrogTriejoin<P: VariableOrderPolicy>`,
  axis `algo_layout_optimiser`): zero-cost and valid-by-construction with no
  `JoinAlgo` churn, but compile-time-only selection multiplies
  `instantiate_database` arms and frames the optimiser as an algorithm knob
  rather than a component. Rejected in favour of the optimizer/executor
  separation with future plan headroom.
- **Runtime config enum inside `global_attribute_order`**: smallest diff,
  but `JoinAlgo::join_iter` is an associated function with no `self` to
  carry config — it needs a signature change anyway, at which point it is
  the chosen design with less payoff.
