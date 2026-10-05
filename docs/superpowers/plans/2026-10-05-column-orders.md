# Column Orders (`--column-orders stored|any`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `--column-orders stored|any` (issue #93): under `any` the
planner may bind an atom's columns in any order, an orientation rewrite
renames each atom whose plan disagrees with its stored column order to
`Index_<π>_<base>` with permuted terms, and the catalog holds a per-query
copy of that relation with its columns permuted, built before the timed
join and dropped after the query.

**Architecture:** Four layers, built bottom-up.

- **Planner policy** (Tasks 1–4, `kermit-algos`). `ColumnOrderPolicy` and
  the `Planner` bundle; pinning through `CatalogStats::is_pinned` into a
  public `Precedence::for_query`; the cost-based estimate over bound column
  *sets*; and `orient`, the fourth rewrite. Nothing touches a structure or
  an executor.
- **Catalog and entry points** (Tasks 5–6, `kermit::db`). `Database<R>`
  gains a copy store (`add_index` / `clear_indexes` / `index` / `indexes`)
  plus `required_indexes`; the four entry points take `&Planner`;
  `validate_query` takes the policy; the shared body orients the planned
  query and resolves `Index_*` names in the store (`MissingIndex` if
  absent).
- **Binary and kermit-lab** (Tasks 7–10). `ExecutionFamily` index methods,
  `PlannerArgs` (`--optimiser` + `--column-orders`), `kermit join` under
  `any`, `bench run`'s `copies` / `space/Index_*` / `index:` metadata /
  `column_orders` axis, and kermit-lab's axis, phase and back-fill.
- **Coverage** (Tasks 11–12). The 16 patterns under `any` for every alias
  × optimiser, five `any`-only patterns, a seeded equivalence test, the
  oracle and allocation tests under both policies.

The controller owns the verification beyond CI (Task 13), the docs (Task
14) and the gate (Task 15).

**Tech Stack:** Rust nightly workspace (clap, Criterion), the Nix dev
shell, Python with uv for kermit-lab, DuckDB ground truth from the
bench-runs directory.

**Spec:** [`docs/specs/2026-10-05-column-orders-design.md`](../../specs/2026-10-05-column-orders-design.md)

---

## Ground rules for every task

- **Paths.** `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab`.
  `SCRATCH` is the executing session's scratchpad directory.
  `RUNS=/tb/Source/Academia/kermit-bench-runs` holds the DuckDB ground
  truth (`thesis-rebench-2026-10-02/groundtruth/duckdb-prelim.json`).
  **Never `cd`** in a Bash call; use absolute paths, `env -C <dir>`,
  `git -C $WT` or `uv --directory`. Each Bash call starts a fresh shell
  (zsh), so define `WT`, `SCRATCH` and `RUNS` at the top of every call
  that uses them.
- **Cargo.** Run it in the **foreground** through the flake with
  `CARGO_BUILD_JOBS=2`, e.g. `CARGO_BUILD_JOBS=2 nix develop $WT --command
  cargo test -p kermit-algos`. A background memory monitor kills
  `run_in_background` cargo jobs. Run anything over 10 minutes (miri, the
  WatDiv verification) fully detached with `setsid nohup … & disown`,
  writing to a log under `$SCRATCH`.
- **Formatting.** Format only with `nix develop $WT --command cargo fmt
  --all`; stable rustfmt rewrites dozens of files. `rustfmt.toml` puts a
  leading `|` on every match arm, and the snippets below already do.
- **Doc comments.** clippy's `doc_markdown` lints them under `-Dwarnings`,
  so backtick every identifier. `kermit-algos` and `kermit` set
  `#![deny(missing_docs)]`, so every new public item needs a doc comment.
  Public docs must not link to private items
  (`rustdoc::private_intra_doc_links`).
- **Commits.** Plain conventional commits ending `(#93)`. Never amend, never
  push. Every message ends with:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
  ```
  (An executing session substitutes its own `Claude-Session` line.) Stage
  files by name and check `git -C $WT show --name-only HEAD` for stray
  files after each commit.
- **Mutation checks.**
  1. Commit first.
  2. Apply the mutant with an exact edit.
  3. Confirm that the named test fails **and** that `git -C $WT diff` shows
     the mutant.
  4. Revert by reversing the exact edit, never with `git checkout`.
  5. Re-run the test and see it pass. `git -C $WT status --porcelain` must
     then be empty.
- **Scope (Priority 6).**
  - Do not change any data structure (`kermit-ds`), either join algorithm
    (`kermit-algos/src/{sorted,hash}/`), or `bench ds`.
  - The three optimisers change in exactly one way each: they build their
    `Precedence` from the query and statistics (Task 2). `cost_based.rs`
    additionally estimates over column sets (Task 3).
  - Under `stored`, every plan, every answer and every report key except
    the new `column_orders` axis is unchanged.
- **Reports.** Keep each implementer's final report under about 40 lines.
  Cover commit SHAs, test counts, mutation-check outcomes, and any
  deviation from this plan with its reason.

## Work packages

| Package | Tasks | Scope | Commits | Done when |
|---|---|---|---|---|
| *Controller* | 0 | Baseline | — | Baseline test counts recorded |
| **P1 — planner policy** | 1, 2, 3, 4 | `kermit-algos/src/optimiser/{column_orders,planner,stats,ordering,lexicographic,cardinality,cost_based,mod}.rs`, `analysis.rs`, `orient.rs`, `lib.rs`; one line in `kermit/src/db.rs`; one in `kermit-algos/tests/cost_based_watdiv_plans.rs` | 4 | `cargo test -p kermit-algos` and `cargo check --workspace --all-targets` green; mutation checks recorded |
| **P2 — catalog and entry points** | 5, 6 | `kermit/src/db.rs`, `kermit/src/db/{database,validation}.rs`, `kermit/src/execution.rs`, `kermit/src/bench/{run,workload}.rs`, `kermit/src/main.rs`, 7 test files | 2 | `cargo test -p kermit` green; mutation checks recorded |
| *Controller* | — | **Checkpoint 1 with the user**: the planner bundle and the store, no CLI yet | — | — |
| **P3 — binary** | 7, 8, 9 | `kermit/src/execution.rs`, `kermit/src/options.rs`, `kermit/src/main.rs`, `kermit/src/bench/run.rs`, new `kermit/tests/cli_column_orders.rs`, 2 CLI tests | 3 | `cargo test -p kermit` green; `kermit join … --column-orders any` answers the cyclic query on all six cells |
| **P4 — kermit-lab** | 10 | `python/kermit-lab/kermit_lab/{frame,loader,defaults}.py`, 4 test files, `docs/specs/bench-report-schema.md` | 1 | `uv run pytest` green, including the contract test with `KERMIT_BIN` |
| **P5 — coverage** | 11, 12 | `kermit/tests/common/{macros,utils}.rs`, `join_tests.rs`, new `column_orders_any.rs` and `column_orders_equivalence.rs`, `lubm_mini_oracle.rs`, `lubm_cardinalities.rs`, `watdiv_correctness.rs`, `subject_position_constant.rs`, `result_allocation.rs` | 2 | `cargo test -p kermit` green with `lubm_cardinalities` run in `nix develop`; mutation checks recorded |
| *Controller* | 13 | The 10 WatDiv templates against DuckDB, the budget count, the `stored` sanity run | — | `$SCRATCH/verification.md` written |
| *Controller* | 14 | Docs | 1 | `cargo doc` clean |
| *Controller* | 15 | Gate; **checkpoint 2 with the user** | — | All CI checks green locally |

Run one implementer at a time, because every package commits to the same
branch. Give each implementer the ground rules plus its tasks' full text.

---

## Task 0 [controller]: Baseline

**Files:** none.

- [ ] **Step 1: Confirm the branch is current**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
git -C $WT fetch origin
git -C $WT log --oneline HEAD..origin/master
git -C $WT log --oneline -1
```

Expected: no output from the second command, and the third shows the spec
commit (`400f347`) or later. If origin/master has moved, `git -C $WT merge
origin/master` (never rebase). Note: local `master` holds #81 but
origin/master does not yet; this branch already includes #81.

- [ ] **Step 2: Green baseline**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
SCRATCH=<scratchpad>
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace 2>&1 | tee $SCRATCH/baseline-test.log | grep -E '^test result:' | awk '{p+=$4; f+=$6} END {print "passed", p, "failed", f}'
```

Expected: `failed 0`. Record the passed count in `$SCRATCH/baseline.txt`.
If `e2e_watdiv` fails, re-run it alone before treating the failure as real;
it has a known clock-seeding flake.

---

# P1 — planner policy (`kermit-algos`)

## Task 1 [P1]: `ColumnOrderPolicy` and the `Planner` bundle

**Files:**
- Create: `kermit-algos/src/optimiser/column_orders.rs`
- Create: `kermit-algos/src/optimiser/planner.rs`
- Modify: `kermit-algos/src/optimiser/mod.rs` (module list and re-exports, lines 17–31)
- Modify: `kermit-algos/src/lib.rs` (re-exports, lines 51–55)

- [ ] **Step 1: Write the policy with its failing tests**

Create `kermit-algos/src/optimiser/column_orders.rs`:

```rust
//! The column-order policy: how free the planner is to bind an atom's
//! columns.
//!
//! Tries are built once, in each relation's stored column order, so a
//! trie-descending join binds an atom's columns left to right. That is a
//! constraint on the plan, not on the query: Leapfrog Triejoin's own
//! setting (Veldhuizen 2014) chooses the variable order first and uses
//! tries whose column order matches it. [`ColumnOrderPolicy`] says which
//! of the two the planner does. It is a planner setting, a peer of the
//! optimiser, carried by [`Planner`](super::Planner); it is neither a
//! Layout, a Config nor a BuildMode, and it applies to both iterator
//! families.

use clap::ValueEnum;

/// Which column orders the planner may bind an atom's columns in.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, ValueEnum)]
pub enum ColumnOrderPolicy {
    /// Each relation is read in its stored column order, so a plan binds
    /// every atom's columns left to right. Today's behaviour, and the
    /// default: the zero-copy point every `any` measurement is compared
    /// against.
    #[default]
    Stored,
    /// The planner binds an atom's columns in any order. An atom whose
    /// plan disagrees with its stored order runs over a per-query copy of
    /// its relation with the columns permuted (see `crate::orient`).
    Any,
}

impl ColumnOrderPolicy {
    /// The bench-report axis value for this policy (the `column_orders`
    /// key). Pinned to clap's value names by a test, as
    /// [`Optimiser::axis_value`](super::Optimiser::axis_value) is.
    pub fn axis_value(self) -> &'static str {
        match self {
            | Self::Stored => "stored",
            | Self::Any => "any",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A variant rename cannot desync the CLI value from the report axis.
    #[test]
    fn axis_values_match_clap_value_names() {
        for v in ColumnOrderPolicy::value_variants() {
            assert_eq!(v.axis_value(), v.to_possible_value().unwrap().get_name());
        }
    }

    /// `stored` is today's behaviour and stays the default until the next
    /// sweep has data on `any` (issue #93).
    #[test]
    fn stored_is_the_default() {
        assert_eq!(ColumnOrderPolicy::default(), ColumnOrderPolicy::Stored);
        assert_eq!(ColumnOrderPolicy::Stored.axis_value(), "stored");
        assert_eq!(ColumnOrderPolicy::Any.axis_value(), "any");
    }
}
```

- [ ] **Step 2: Write the planner bundle with its failing tests**

Create `kermit-algos/src/optimiser/planner.rs`:

```rust
//! The planner: an optimiser plus the column-order policy it plans under.

use {
    super::{CatalogStats, ColumnOrderPolicy, QueryOptimiser, QueryPlan, StatisticsLevel},
    kermit_parser::JoinQuery,
    std::fmt,
};

/// What plans a join: a [`QueryOptimiser`] and the [`ColumnOrderPolicy`]
/// it may plan under.
///
/// The join entry points (`kermit::db`), the CLI's families and the test
/// harness all pass one of these where they used to pass a bare
/// `&dyn QueryOptimiser`, so the two planner settings travel together and
/// cannot disagree between the call that computes a query's copies and
/// the join itself. The catalog stays policy-free: it only stores the
/// copies the planner asks for.
pub struct Planner {
    optimiser: Box<dyn QueryOptimiser>,
    column_orders: ColumnOrderPolicy,
}

impl Planner {
    /// A planner running `optimiser` under `column_orders`.
    pub fn new(optimiser: Box<dyn QueryOptimiser>, column_orders: ColumnOrderPolicy) -> Self {
        Self {
            optimiser,
            column_orders,
        }
    }

    /// `optimiser` under [`ColumnOrderPolicy::Stored`]: today's planning,
    /// and the fixtures' one-liner.
    pub fn stored(optimiser: impl QueryOptimiser + 'static) -> Self {
        Self::new(Box::new(optimiser), ColumnOrderPolicy::Stored)
    }

    /// The column orders this planner may bind an atom's columns in.
    pub fn column_orders(&self) -> ColumnOrderPolicy { self.column_orders }

    /// The statistics the optimiser reads
    /// ([`QueryOptimiser::required_statistics`]).
    pub fn required_statistics(&self) -> StatisticsLevel { self.optimiser.required_statistics() }

    /// Plans `query` from `stats` ([`QueryOptimiser::plan`]). `stats` must
    /// have been built under this planner's policy
    /// ([`CatalogStats::for_query`]), which is how the policy reaches the
    /// optimiser's precedence graph.
    pub fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan {
        debug_assert_eq!(
            stats.column_orders(),
            self.column_orders,
            "the statistics were built under another column-order policy"
        );
        self.optimiser.plan(query, stats)
    }
}

impl fmt::Debug for Planner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Planner")
            .field("column_orders", &self.column_orders)
            .field("required_statistics", &self.required_statistics())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::optimiser::{CardinalityOptimiser, LexicographicOptimiser, RelationStats},
    };

    /// Plans like `lexicographic` but declares that it reads per-column
    /// distinct counts.
    struct NeedsColumns;

    impl QueryOptimiser for NeedsColumns {
        fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan {
            LexicographicOptimiser.plan(query, stats)
        }

        fn required_statistics(&self) -> StatisticsLevel { StatisticsLevel::ColumnDistinct }
    }

    #[test]
    fn stored_wraps_the_optimiser_under_the_stored_policy() {
        let planner = Planner::stored(NeedsColumns);
        assert_eq!(planner.column_orders(), ColumnOrderPolicy::Stored);
        assert_eq!(planner.required_statistics(), StatisticsLevel::ColumnDistinct);
    }

    #[test]
    fn the_planner_plans_with_its_optimiser() {
        let q: JoinQuery = "Q(A, B, C) :- R(A, B), S(A, C).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, ColumnOrderPolicy::Stored, |name| {
            let tuples = if name == "R" { 1000 } else { 3 };
            Some(RelationStats::new(tuples, 2))
        });
        let planner = Planner::stored(CardinalityOptimiser);
        assert_eq!(planner.plan(&q, &stats), CardinalityOptimiser.plan(&q, &stats));
        assert_eq!(planner.plan(&q, &stats).variable_ordering, vec![0, 2, 1]);
    }

    #[test]
    fn debug_names_the_policy_and_statistics() {
        let shown = format!("{:?}", Planner::new(Box::new(NeedsColumns), ColumnOrderPolicy::Any));
        assert!(shown.contains("Any"), "{shown}");
        assert!(shown.contains("ColumnDistinct"), "{shown}");
    }
}
```

`CatalogStats::for_query` gains its policy parameter and
`CatalogStats::column_orders()` in Task 2; until then the planner test
module does not compile. Register the modules now and write Task 2
before running the tests.

- [ ] **Step 3: Register the modules and re-export**

In `kermit-algos/src/optimiser/mod.rs`, the module list becomes:

```rust
mod cardinality;
mod column_orders;
mod cost_based;
mod lexicographic;
mod ordering;
mod plan;
mod planner;
mod stats;

pub use {
    cardinality::CardinalityOptimiser,
    column_orders::ColumnOrderPolicy,
    cost_based::CostBasedOptimiser,
    lexicographic::LexicographicOptimiser,
    ordering::{check_attribute_order, topological_order, CyclicAttributeOrder, Precedence},
    plan::{PlanError, QueryPlan},
    planner::Planner,
    stats::{distinct_per_column, CatalogStats, RelationStats, StatisticsLevel},
};
```

(`Precedence` becomes public in Task 2.) In `kermit-algos/src/lib.rs`, the
`optimiser::{…}` re-export line gains `ColumnOrderPolicy`, `Planner` and
`Precedence`, keeping the alphabetical order rustfmt enforces:

```rust
    optimiser::{
        check_attribute_order, distinct_per_column, topological_order, CardinalityOptimiser,
        CatalogStats, ColumnOrderPolicy, CostBasedOptimiser, CyclicAttributeOrder,
        LexicographicOptimiser, Optimiser, PlanError, Planner, Precedence, QueryOptimiser,
        QueryPlan, RelationStats, StatisticsLevel,
    },
```

Also update the `optimiser/mod.rs` module doc's first paragraph to name
the bundle: after "produces a [`QueryPlan`]" add "A [`Planner`] bundles an
optimiser with the [`ColumnOrderPolicy`] it plans under; `kermit::db`
passes one where it used to pass a bare optimiser."

- [ ] **Step 4: Continue with Task 2; run both tasks' tests together**

Task 2 changes `CatalogStats::for_query`, which this task's tests call.
Commit after Task 2's Step 7 compiles: `git -C $WT add kermit-algos/src/optimiser/column_orders.rs kermit-algos/src/optimiser/planner.rs kermit-algos/src/optimiser/mod.rs kermit-algos/src/lib.rs` and

```
feat(algos): ColumnOrderPolicy and the Planner bundle (#93)
```

(commit this before Task 2's own commit, once the workspace compiles with
both tasks applied; the two commits may land back to back).

---

## Task 2 [P1]: Pinning: `CatalogStats::is_pinned`, `Precedence::for_query`, the new `topological_order`

**Files:**
- Modify: `kermit-algos/src/optimiser/stats.rs` (`CatalogStats`, lines 103–156; tests from 158)
- Modify: `kermit-algos/src/optimiser/ordering.rs` (whole file: `topological_order`, `Precedence`, tests)
- Modify: `kermit-algos/src/optimiser/lexicographic.rs:19-30`, `cardinality.rs:22-52`
- Modify: `kermit-algos/src/optimiser/cost_based.rs:59-95` (two lines) and its `stats_for` test helper (line 310)
- Modify: `kermit-algos/tests/cost_based_watdiv_plans.rs:58` (one call)
- Modify: `kermit/src/db.rs:264` (one call)

- [ ] **Step 1: Statistics carry the policy**

In `stats.rs`, change the struct and `for_query`, and add two methods:

```rust
/// Per-relation statistics for the predicates of one query, plus the
/// column-order policy the plan is made under.
///
/// Plain data: planners never touch data structures, so they stay
/// data-structure-agnostic and unit-testable with literal maps. Callers
/// build one per join via [`CatalogStats::for_query`]. The policy rides
/// along because it decides which atoms *pin* the plan to their stored
/// column order ([`is_pinned`](Self::is_pinned)), which every optimiser
/// reads through [`Precedence::for_query`](super::Precedence::for_query).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogStats {
    relations: BTreeMap<String, RelationStats>,
    column_orders: ColumnOrderPolicy,
}
```

Add the import `super::ColumnOrderPolicy` to the `use` block. Replace
`for_query`:

```rust
    /// Builds stats for every body predicate of `query`, under
    /// `column_orders`.
    ///
    /// `stats_of` supplies a relation's statistics (in `kermit::db`, those
    /// its `Database` gathered when it was built). Synthetic `Const_*`
    /// predicates (introduced by the const-view rewrite) are recorded as
    /// single-tuple unary relations whose one column has one value.
    /// Predicates the lookup does not know get no entry — planners treat
    /// missing stats as "assume large".
    pub fn for_query(
        query: &JoinQuery, column_orders: ColumnOrderPolicy,
        stats_of: impl Fn(&str) -> Option<RelationStats>,
    ) -> Self {
        let mut stats = CatalogStats {
            column_orders,
            ..CatalogStats::default()
        };
        for pred in &query.body {
            if stats.relations.contains_key(&pred.name) {
                continue;
            }
            if is_const_predicate(&pred.name) {
                stats.insert(
                    pred.name.clone(),
                    RelationStats::new(1, 1).with_column_distinct(vec![1]),
                );
            } else if let Some(relation) = stats_of(&pred.name) {
                stats.insert(pred.name.clone(), relation);
            }
        }
        stats
    }

    /// The column-order policy these statistics were built under.
    pub fn column_orders(&self) -> ColumnOrderPolicy { self.column_orders }

    /// Whether the atom named `name` must bind its columns left to right,
    /// i.e. adds its column-order edges to the plan's precedence graph.
    ///
    /// `stored` pins every relation and `any` pins none, so today the
    /// name is not consulted; it is the seam for a selective policy
    /// (issue #82: a relation with a stored reordered copy is free). A
    /// `Select_<n>_<base>` view follows its base, and a `Const_*`
    /// singleton is unary, so pinning it changes nothing.
    pub fn is_pinned(&self, _name: &str) -> bool {
        match self.column_orders {
            | ColumnOrderPolicy::Stored => true,
            | ColumnOrderPolicy::Any => false,
        }
    }
```

Fix the existing test `for_query_records_relations_and_const_singletons`
(and any other `for_query` call in the file) to pass
`ColumnOrderPolicy::Stored` as the second argument, then add:

```rust
    #[test]
    fn stored_pins_every_atom_and_any_pins_none() {
        let q: JoinQuery = "Q(X) :- R(X, K0), Select_0_R(X, K1), Const_c5(K0).".parse().unwrap();
        let stored = CatalogStats::for_query(&q, ColumnOrderPolicy::Stored, |_| None);
        assert_eq!(stored.column_orders(), ColumnOrderPolicy::Stored);
        for atom in &q.body {
            assert!(stored.is_pinned(&atom.name), "{}", atom.name);
        }
        let any = CatalogStats::for_query(&q, ColumnOrderPolicy::Any, |_| None);
        assert_eq!(any.column_orders(), ColumnOrderPolicy::Any);
        for atom in &q.body {
            assert!(!any.is_pinned(&atom.name), "{}", atom.name);
        }
    }

    #[test]
    fn default_statistics_are_stored() {
        assert_eq!(CatalogStats::default().column_orders(), ColumnOrderPolicy::Stored);
    }
```

- [ ] **Step 2: `Precedence` becomes public, built per query from the pinned atoms**

Replace the top of `ordering.rs` (everything before `/// Body atoms whose
column orders together form a cycle`) with:

```rust
//! Constraint-respecting topological ordering shared by all optimisers.

use {
    crate::{analyse, optimiser::CatalogStats},
    kermit_parser::JoinQuery,
    std::{
        cmp::Reverse,
        collections::{BinaryHeap, HashSet},
    },
};

/// Computes a *global attribute order* (GAO): a permutation of the
/// query's variables in which every *pinned* relation's variables appear
/// in physical column order.
///
/// Trie-descending joins bind each relation one physical column per depth,
/// so a relation `r(K, Y)` read in its stored order participates correctly
/// only if its first column `K` is bound before its second column `Y`.
/// `precedence` holds those edges ([`Precedence::for_query`]: one
/// `col[i] -> col[i+1]` edge per adjacent pair of each pinned atom's
/// variables; under `--column-orders any` no atom is pinned and the graph
/// is empty). Kahn's algorithm yields a valid order.
///
/// `rank` maps a candidate variable to an [`Ord`] key; among the variables
/// whose constraints are currently satisfied (the *ready set*), the
/// smallest key is emitted next, with the canonical index as the final
/// deterministic tie-break. Because candidates are restricted to the ready
/// set, **any** rank function yields a valid order — an ordering policy
/// can be slow, never wrong. `rank` is evaluated once per variable, as it
/// becomes ready — cheap enough for expensive statistics-driven policies.
///
/// # Panics
///
/// Panics if the constraints are cyclic (e.g. `r(X, Y), s(Y, X)` under
/// `stored`): answering such a query would require a relation sorted in
/// two different column orders at once, which a single fixed trie order
/// cannot provide. `kermit::db` rejects such a query before planning
/// ([`check_attribute_order`]), and under `any` the graph has no cycle.
pub fn topological_order<K: Ord>(precedence: &Precedence, rank: impl Fn(usize) -> K) -> Vec<usize> {
    let order = precedence.kahn(rank);
    assert_eq!(
        order.len(),
        precedence.num_vars(),
        "query imposes a cyclic global attribute order; the join cannot answer it with a single \
         trie column order per relation"
    );
    order
}

/// The column-order constraint graph: one edge `col[i] -> col[i+1]` per
/// adjacent pair of a pinned relation's variables. The one place those
/// edges are built, shared by [`topological_order`], which orders it,
/// [`check_attribute_order`], which reports a cycle in it, and the
/// cost-based search, which reads its [`predecessor_masks`](Self::predecessor_masks),
/// so none of them can disagree about which plans are valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Precedence {
    /// `adjacency[v]`: the variables that must be bound after `v`.
    adjacency: Vec<HashSet<usize>>,
    /// Number of distinct variables that must be bound before each one.
    in_degree: Vec<usize>,
}

impl Precedence {
    /// The graph of `query` under the policy `stats` carry: only atoms
    /// [`CatalogStats::is_pinned`] says are pinned add edges. `query` is
    /// the rewritten query, as [`QueryOptimiser::plan`](super::QueryOptimiser::plan)
    /// receives it.
    pub fn for_query(query: &JoinQuery, stats: &CatalogStats) -> Self {
        let analysis = analyse(query);
        let pinned = query
            .body
            .iter()
            .zip(&analysis.predicate_variables)
            .filter(|(atom, _)| stats.is_pinned(&atom.name))
            .map(|(_, vars)| vars);
        Self::build(analysis.num_vars, pinned)
    }

    /// The graph with every atom pinned: what `stored` plans under, and
    /// what [`check_attribute_order`] reports cycles in.
    pub(crate) fn new(num_vars: usize, predicate_variables: &[Vec<usize>]) -> Self {
        Self::build(num_vars, predicate_variables)
    }

    fn build<'v>(num_vars: usize, atoms: impl IntoIterator<Item = &'v Vec<usize>>) -> Self {
        let mut adjacency: Vec<HashSet<usize>> = vec![HashSet::new(); num_vars];
        let mut in_degree: Vec<usize> = vec![0; num_vars];
        for vars in atoms {
            for pair in vars.windows(2) {
                let (earlier, later) = (pair[0], pair[1]);
                if earlier != later && adjacency[earlier].insert(later) {
                    in_degree[later] += 1;
                }
            }
        }
        Self {
            adjacency,
            in_degree,
        }
    }

    /// The number of variables the graph orders.
    pub fn num_vars(&self) -> usize { self.in_degree.len() }

    /// Whether the graph has no edge at all, so every order is valid.
    pub fn is_free(&self) -> bool { self.in_degree.iter().all(|&d| d == 0) }

    /// The variables each variable must follow, as one bitmask per variable:
    /// bit `u` of entry `v` is set when `u -> v` is an edge. A set of bound
    /// variables `S` can bind `v` next exactly when `masks[v] & !S == 0`.
    /// `None` past 64 variables, which a `u64` cannot index.
    pub fn predecessor_masks(&self) -> Option<Vec<u64>> {
        let num_vars = self.num_vars();
        if num_vars > 64 {
            return None;
        }
        let mut masks = vec![0u64; num_vars];
        for (earlier, laters) in self.adjacency.iter().enumerate() {
            for &later in laters {
                masks[later] |= 1u64 << earlier;
            }
        }
        Some(masks)
    }

    /// Kahn's algorithm, emitting the smallest-`rank` ready variable first.
    /// The result is shorter than `num_vars` exactly when the constraints
    /// are cyclic: the variables on and after a cycle never become ready.
    fn kahn<K: Ord>(&self, rank: impl Fn(usize) -> K) -> Vec<usize> {
        // unchanged body
    }
}
```

Keep `kahn`'s body exactly as it is. `check_attribute_order` keeps its
body; it still calls `Precedence::new(analysis.num_vars,
&analysis.predicate_variables)` (every atom pinned, which is what `stored`
means) and reads `precedence.adjacency` directly. Update its doc comment's
first paragraph to: "Checks that `query` admits a global attribute order
under `stored`, i.e. that [`topological_order`] over every atom's
column-order edges would not panic on it, and otherwise names the body
atoms on one cycle of those constraints. Under `--column-orders any` no
atom is pinned, so there is nothing to check and `kermit::db` skips this."

- [ ] **Step 3: Update the ordering tests and add the pinning ones**

In `ordering.rs`'s `mod tests`, every `topological_order(n, &preds, rank)`
becomes `topological_order(&Precedence::new(n, &preds), rank)`:

```rust
    #[test]
    fn identity_rank_reproduces_lexicographic_order() {
        // Triangle: R(0,1), S(1,2), T(0,2) — edges 0->1, 1->2, 0->2.
        let preds = vec![vec![0, 1], vec![1, 2], vec![0, 2]];
        assert_eq!(topological_order(&Precedence::new(3, &preds), |v| v), vec![0, 1, 2]);
    }
```

(likewise `constraint_forces_late_canonical_variable_first`,
`rank_breaks_ties_among_ready_variables` and `cyclic_constraints_panic`).
Add, with `use crate::optimiser::ColumnOrderPolicy;` in the test imports:

```rust
    fn stats(q: &JoinQuery, policy: ColumnOrderPolicy) -> CatalogStats {
        CatalogStats::for_query(q, policy, |_| None)
    }

    /// Under `stored`, `for_query` builds the same graph as `new` over
    /// every atom.
    #[test]
    fn under_stored_every_atom_adds_its_edges() {
        let q: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        let analysis = analyse(&q);
        let from_query = Precedence::for_query(&q, &stats(&q, ColumnOrderPolicy::Stored));
        assert_eq!(
            from_query,
            Precedence::new(analysis.num_vars, &analysis.predicate_variables)
        );
        assert!(!from_query.is_free());
        assert_eq!(from_query.predecessor_masks().unwrap(), vec![0b000, 0b001, 0b011]);
    }

    /// Under `any` nothing is pinned: no edges, every order valid, and a
    /// query that is cyclic under `stored` orders fine.
    #[test]
    fn under_any_no_atom_adds_an_edge() {
        let q: JoinQuery = "Q(X, Y) :- edge(X, Y), edge(Y, X).".parse().unwrap();
        let precedence = Precedence::for_query(&q, &stats(&q, ColumnOrderPolicy::Any));
        assert!(precedence.is_free());
        assert_eq!(precedence.num_vars(), 2);
        assert_eq!(precedence.predecessor_masks().unwrap(), vec![0, 0]);
        assert_eq!(topological_order(&precedence, |v| v), vec![0, 1]);
        // The rank alone decides.
        assert_eq!(topological_order(&precedence, |v| 1 - v), vec![1, 0]);
    }
```

- [ ] **Step 4: The three optimisers build their precedence from the query and statistics**

`lexicographic.rs`:

```rust
use crate::optimiser::{
    ordering::{topological_order, Precedence},
    CatalogStats, QueryOptimiser, QueryPlan,
};

impl QueryOptimiser for LexicographicOptimiser {
    fn plan(&self, query: &kermit_parser::JoinQuery, stats: &CatalogStats) -> QueryPlan {
        QueryPlan {
            variable_ordering: topological_order(&Precedence::for_query(query, stats), |v| v),
        }
    }
}
```

(drop the now-unused `analysis::analyse` import). Update its doc comment:
"keeps head variables early when unconstrained" stays; add "Under
`--column-orders any` nothing constrains it, so the plan is the canonical
order itself."

`cardinality.rs`:

```rust
use crate::{
    analysis::analyse,
    optimiser::{
        ordering::{topological_order, Precedence},
        CatalogStats, QueryOptimiser, QueryPlan,
    },
};

impl QueryOptimiser for CardinalityOptimiser {
    fn plan(&self, query: &kermit_parser::JoinQuery, stats: &CatalogStats) -> QueryPlan {
        let analysis = analyse(query);
        let mut min_size = vec![usize::MAX; analysis.num_vars];
        for (pred, vars) in query.body.iter().zip(&analysis.predicate_variables) {
            let size = stats.tuples(&pred.name).unwrap_or(usize::MAX);
            for &v in vars {
                min_size[v] = min_size[v].min(size);
            }
        }
        QueryPlan {
            variable_ordering: topological_order(&Precedence::for_query(query, stats), |v| {
                min_size[v]
            }),
        }
    }
}
```

`cost_based.rs`, in `plan`, only the two `Precedence`/`topological_order`
lines change in this task (Task 3 replaces `Precedence::new` with
`for_query` once the estimate can handle a free graph):

```rust
        let precedence = Precedence::new(analysis.num_vars, &analysis.predicate_variables);
        let cheapest = precedence
            .predecessor_masks()
            .and_then(|predecessors| cheapest_order(&model, &predecessors, self.state_budget));
        …
        let variable_ordering = topological_order(&precedence, |v| position[v]);
```

- [ ] **Step 5: Migrate every `for_query` caller**

Each test helper `stats_for` in `lexicographic.rs`, `cardinality.rs` and
`cost_based.rs` gains the policy argument, defaulting to `Stored`:

```rust
    fn stats_for(q: &JoinQuery, sizes: &[(&str, usize)]) -> CatalogStats {
        CatalogStats::for_query(q, ColumnOrderPolicy::Stored, |name| { /* unchanged */ })
    }
```

(import `crate::optimiser::ColumnOrderPolicy` in each test module; in
`cost_based.rs` the direct `CatalogStats::for_query(&q, |_| Some(…))` in
`a_column_without_a_distinct_count_is_assumed_a_key` also gains
`ColumnOrderPolicy::Stored`). `kermit-algos/tests/cost_based_watdiv_plans.rs`'s
`catalog` likewise:

```rust
    CatalogStats::for_query(query, ColumnOrderPolicy::Stored, |name| { /* unchanged */ })
```

with `ColumnOrderPolicy` added to its `kermit_algos::{…}` import. In
`kermit/src/db.rs:264`:

```rust
    let stats = CatalogStats::for_query(&rewritten, ColumnOrderPolicy::Stored, |name| {
```

with `ColumnOrderPolicy` added to the `kermit_algos::{…}` import at the top
of `db.rs` (Task 6 replaces the literal with the planner's policy).

- [ ] **Step 6: Policy-sensitive ranking tests**

In `cardinality.rs`'s tests, add (the existing
`column_order_constraints_still_bind` is the `stored` half):

```rust
    /// Under `any` the ranking alone decides: `S` is tiny, so its `Y`
    /// goes first even though `R(X, Y)` stores `X` first.
    #[test]
    fn under_any_the_smallest_relation_goes_first_regardless_of_column_order() {
        let q: JoinQuery = "Q(X, Y) :- R(X, Y), S(Y).".parse().unwrap();
        let sizes = [("R", 1000), ("S", 1)];
        let stored = CardinalityOptimiser.plan(&q, &stats_for(&q, &sizes));
        assert_eq!(stored.variable_ordering, vec![0, 1]);
        let any = CatalogStats::for_query(&q, ColumnOrderPolicy::Any, |name| {
            let arity = q.body.iter().find(|p| p.name == name)?.terms.len();
            let &(_, tuples) = sizes.iter().find(|(n, _)| *n == name)?;
            Some(RelationStats::new(tuples, arity))
        });
        assert_eq!(CardinalityOptimiser.plan(&q, &any).variable_ordering, vec![1, 0]);
    }
```

In `lexicographic.rs`'s tests:

```rust
    /// Under `any` the plan is the canonical order itself, even when the
    /// stored column order would have forced `Y` first.
    #[test]
    fn under_any_the_canonical_order_is_the_plan() {
        let q: JoinQuery = "Q(X, Y) :- r(Y, X).".parse().unwrap();
        let stored = CatalogStats::for_query(&q, ColumnOrderPolicy::Stored, |_| None);
        assert_eq!(LexicographicOptimiser.plan(&q, &stored).variable_ordering, vec![1, 0]);
        let any = CatalogStats::for_query(&q, ColumnOrderPolicy::Any, |_| None);
        assert_eq!(LexicographicOptimiser.plan(&q, &any).variable_ordering, vec![0, 1]);
    }
```

- [ ] **Step 7: Run the tests**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos 2>&1 | grep -E '^test result|FAILED|panicked'
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo check --workspace --all-targets 2>&1 | tail -3
```

Expected: all `kermit-algos` tests pass (the previous count plus 8: two
in `column_orders.rs`, three in `planner.rs`, two in `stats.rs`, two in
`ordering.rs`, one each in `cardinality.rs` and `lexicographic.rs`, minus
nothing), and the workspace checks.

- [ ] **Step 8: Format and commit (Task 1 first, then this)**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
nix develop $WT --command cargo fmt --all
git -C $WT add kermit-algos/src/optimiser/column_orders.rs kermit-algos/src/optimiser/planner.rs kermit-algos/src/optimiser/mod.rs kermit-algos/src/lib.rs
git -C $WT commit -q -F - <<'MSG'
feat(algos): ColumnOrderPolicy and the Planner bundle (#93)

`--column-orders stored|any` is a planner setting, a peer of the
optimiser: `Planner` carries both so the call that computes a query's
copies and the join cannot disagree.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
git -C $WT add kermit-algos/src/optimiser/stats.rs kermit-algos/src/optimiser/ordering.rs kermit-algos/src/optimiser/lexicographic.rs kermit-algos/src/optimiser/cardinality.rs kermit-algos/src/optimiser/cost_based.rs kermit-algos/tests/cost_based_watdiv_plans.rs kermit/src/db.rs
git -C $WT commit -q -F - <<'MSG'
feat(algos): pinned atoms alone add column-order edges (#93)

`CatalogStats` carries the column-order policy and answers `is_pinned`;
`Precedence::for_query` adds an atom's left-to-right edges only when it
is pinned, and `topological_order` takes the precedence. `stored` pins
everything, so every plan is unchanged; `any` pins nothing.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
```

Note the first commit alone does not compile (its tests call the new
`for_query`); that is acceptable because the two land back to back and
the gate runs on the branch tip. If the implementer prefers one commit,
merge the two messages.

- [ ] **Step 9: Mutation check: pinning is read**

Mutant: in `stats.rs`, make `is_pinned` return `true` unconditionally.
Expected to fail: `under_any_no_atom_adds_an_edge` (ordering.rs),
`under_any_the_smallest_relation_goes_first_regardless_of_column_order`,
`under_any_the_canonical_order_is_the_plan`, `stored_pins_every_atom_and_any_pins_none`.
Revert by reversing the edit.

---

## Task 3 [P1]: Cost-based under `any`: a free search over column sets

**Files:**
- Modify: `kermit-algos/src/optimiser/cost_based.rs` (`plan` lines 59–98; `AtomStats::projection_size` 128–143; `CostModel` 145–232; tests)

- [ ] **Step 1: Failing tests first**

Add to `cost_based.rs`'s `mod tests` (with `ColumnOrderPolicy` imported):

```rust
    fn stats_under(
        q: &JoinQuery, policy: ColumnOrderPolicy, relations: &[(&str, usize, &[usize])],
    ) -> CatalogStats {
        CatalogStats::for_query(q, policy, |name| {
            let &(_, tuples, distinct) = relations.iter().find(|(n, ..)| *n == name)?;
            Some(RelationStats::new(tuples, distinct.len()).with_column_distinct(distinct.to_vec()))
        })
    }

    /// Under `any` a bound set need not be a stored-order prefix of an
    /// atom: `{Y}` binds `R`'s second column alone, and the estimate reads
    /// that column's distinct count.
    #[test]
    fn under_any_the_bound_columns_need_not_be_a_prefix() {
        let q: JoinQuery = "Q(X, Y) :- R(X, Y).".parse().unwrap();
        let stats = stats_under(&q, ColumnOrderPolicy::Any, &[("R", 100, &[10, 20])]);
        assert_eq!(estimate(&q, &stats, &[1]), 20.0);
        assert_eq!(estimate(&q, &stats, &[0]), 10.0);
        assert_eq!(estimate(&q, &stats, &[0, 1]), 100.0);
    }

    /// Under `any` a non-prefix pair of a ternary atom multiplies those
    /// columns' distinct counts, capped at the tuples, like a prefix does.
    #[test]
    fn under_any_a_column_pair_assumes_independent_columns_capped_by_the_tuples() {
        let q: JoinQuery = "Q(X, Y, Z) :- T(X, Y, Z).".parse().unwrap();
        let stats = stats_under(&q, ColumnOrderPolicy::Any, &[("T", 1000, &[10, 5, 7])]);
        assert_eq!(estimate(&q, &stats, &[0, 2]), 70.0);
        let stats = stats_under(&q, ColumnOrderPolicy::Any, &[("T", 1000, &[100, 50, 70])]);
        assert_eq!(estimate(&q, &stats, &[1, 2]), 1000.0);
    }

    /// `column_order_constraints_bind` is the `stored` half: `R(X, Y)`
    /// forces `X` first although `S` is tiny. Under `any` the cheap side
    /// goes first.
    #[test]
    fn under_any_the_cheap_side_goes_first() {
        let q: JoinQuery = "Q(X, Y) :- R(X, Y), S(Y).".parse().unwrap();
        let relations: &[(&str, usize, &[usize])] = &[("R", 1000, &[1000, 1000]), ("S", 1, &[1])];
        let stored = stats_under(&q, ColumnOrderPolicy::Stored, relations);
        assert_eq!(CostBasedOptimiser::default().plan(&q, &stored).variable_ordering, vec![0, 1]);
        let any = stats_under(&q, ColumnOrderPolicy::Any, relations);
        assert_eq!(CostBasedOptimiser::default().plan(&q, &any).variable_ordering, vec![1, 0]);
    }

    /// With no edges every subset is a state: three variables reach seven
    /// sets, so a budget of six falls back to `cardinality` under `any`
    /// while `stored` reaches only three.
    #[test]
    fn a_free_search_reaches_every_subset() {
        let q: JoinQuery = "Q(X, Y, Z) :- T(X, Y, Z).".parse().unwrap();
        let relations: &[(&str, usize, &[usize])] = &[("T", 1000, &[10, 5, 7])];
        let any = stats_under(&q, ColumnOrderPolicy::Any, relations);
        let seven = CostBasedOptimiser {
            state_budget: 7,
        }
        .plan(&q, &any);
        // Cheapest first: Y (5), then Z (5 · 7 = 35 ≥ 5 · 10 = 50? no: 35), then X.
        assert_eq!(seven.variable_ordering, vec![1, 2, 0]);
        assert_eq!(
            CostBasedOptimiser {
                state_budget: 6
            }
            .plan(&q, &any),
            CardinalityOptimiser.plan(&q, &any)
        );
        let stored = stats_under(&q, ColumnOrderPolicy::Stored, relations);
        assert_eq!(
            CostBasedOptimiser {
                state_budget: 3
            }
            .plan(&q, &stored)
            .variable_ordering,
            vec![0, 1, 2]
        );
    }
```

(Check the arithmetic in `a_free_search_reaches_every_subset` when it
runs: costs are `est({Y}) = 5`, `est({Y, Z}) = 35`, `est({X, Y, Z}) =
1000`; the alternative `{Y}, {X, Y}` costs `5 + 50`, so `Y, Z, X` wins.
If the DP tie-breaks differently, fix the expected order from the
printed plan and record why in the test's comment.)

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p
kermit-algos under_any a_free_search` — expected: the first two panic on
the prefix `debug_assert!` in `estimate`, the plan tests fail on their
assertions.

- [ ] **Step 2: Estimate over bound column sets**

Replace `AtomStats::projection_size`:

```rust
    /// The size of the atom's projection onto `columns` (ascending,
    /// non-empty): its tuples when they are all its columns, one column's
    /// distinct count on its own, and otherwise the product of the
    /// columns' distinct counts capped at the tuples, since no statistic
    /// covers a multi-column projection (independent columns assumed).
    /// Under `stored` the bound columns are always a prefix; under `any`
    /// any subset.
    fn projection_size(&self, columns: &[usize]) -> f64 {
        if columns.len() == self.distinct.len() {
            self.tuples
        } else if let [column] = columns {
            self.distinct[*column]
        } else {
            columns
                .iter()
                .map(|&column| self.distinct[column])
                .product::<f64>()
                .min(self.tuples)
        }
    }
```

`CostModel` records the policy so the `stored` invariant stays checked:

```rust
struct CostModel<'q> {
    atoms: Vec<AtomStats>,
    /// Each atom's variables in column order, as `analyse` numbers them.
    predicate_variables: &'q [Vec<usize>],
    num_vars: usize,
    column_orders: ColumnOrderPolicy,
}
```

(`CostModel::new` sets `column_orders: stats.column_orders()`; import
`ColumnOrderPolicy` from `crate::optimiser`.) In `estimate`, replace the
prefix computation:

```rust
        for (atom, vars) in self.atoms.iter().zip(self.predicate_variables) {
            let bound_columns: Vec<usize> =
                (0..vars.len()).filter(|&c| contains(bound, vars[c])).collect();
            debug_assert!(
                self.column_orders == ColumnOrderPolicy::Any
                    || bound_columns.iter().enumerate().all(|(i, &c)| i == c),
                "under `stored`, a set closed under column order binds a prefix of every atom"
            );
            if bound_columns.is_empty() {
                continue;
            }
            numerator *= atom.projection_size(&bound_columns);
            let mut columns = bound_columns;
            columns.sort_by_key(|&column| vars[column]);
            for column in columns {
                let v = vars[column];
                if distinct_of[v].is_empty() {
                    met.push(v);
                }
                distinct_of[v].push(atom.distinct[column]);
            }
        }
```

Update `estimate`'s doc: "`bound` is any set of variables; under `stored`
it is closed under the column-order constraints, so each atom's bound
columns form a prefix, and under `any` they are any subset." Under
`stored` the prefix path produces exactly the old numbers, so every
existing estimate test and the WatDiv parity fixture are unchanged.

- [ ] **Step 3: The search reads the policy's graph**

In `plan`, replace `Precedence::new(analysis.num_vars,
&analysis.predicate_variables)` with `Precedence::for_query(query, stats)`.
Update the type doc: after "those closed under the column-order
constraints, so its plan is valid by construction." add "Under
`--column-orders any` there are no constraints, so every subset of the
variables is a state: 2ⁿ − 1 for n variables, which fits the default
budget up to 14 variables and falls back to [`CardinalityOptimiser`]
above."

- [ ] **Step 4: Run, format, commit**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos 2>&1 | grep -E '^test result|FAILED|panicked'
nix develop $WT --command cargo fmt --all
git -C $WT add kermit-algos/src/optimiser/cost_based.rs
git -C $WT commit -q -F - <<'MSG'
feat(algos): the cost-based search estimates bound column sets (#93)

Under `any` an atom's bound columns are any subset, not a stored-order
prefix; the projection size multiplies the bound columns' distinct
counts. Under `stored` the subset is always a prefix, so the arithmetic
and the 124-template parity fixture are unchanged.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
```

Expected: every `kermit-algos` test passes, including
`cost_based_watdiv_plans` (q0020, q0034, q0008 orders unchanged).

- [ ] **Step 5: Mutation check: the set-based projection**

Mutant: in `projection_size`, replace `self.distinct[*column]` with
`self.distinct[0]`. Expected to fail:
`under_any_the_bound_columns_need_not_be_a_prefix` (`est({Y})` would be
10). Revert.

---

## Task 4 [P1]: The orientation rewrite

**Files:**
- Create: `kermit-algos/src/orient.rs`
- Modify: `kermit-algos/src/analysis.rs` (add `canonical_names`)
- Modify: `kermit-algos/src/lib.rs` (module list, re-exports, module doc)

- [ ] **Step 1: `canonical_names` beside `analyse`**

Append to `analysis.rs` after `analyse`:

```rust
/// The variables of `query` by canonical index: `names[i]` is the variable
/// [`analyse`] numbers `i`. The same two passes, head then body in first
/// appearance, so the two always agree. The orientation rewrite uses it to
/// carry a plan across a renumbering.
pub fn canonical_names(query: &JoinQuery) -> Vec<String> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut names: Vec<String> = Vec::new();
    let terms = query
        .head
        .terms
        .iter()
        .chain(query.body.iter().flat_map(|pred| pred.terms.iter()));
    for term in terms {
        if let Term::Var(name) = term {
            if seen.insert(name) {
                names.push(name.clone());
            }
        }
    }
    names
}
```

(add `HashSet` to the `std::collections` import.) Add to its test module:

```rust
    #[test]
    fn canonical_names_agree_with_analyse() {
        let q: JoinQuery = "Q(Y, X) :- r(X, K0, Y), s(Y, Z), Const_c5(K0).".parse().unwrap();
        let names = canonical_names(&q);
        assert_eq!(names, vec!["Y", "X", "K0", "Z"]);
        let analysis = analyse(&q);
        assert_eq!(names.len(), analysis.num_vars);
        // `r(X, K0, Y)` in canonical indices is [1, 2, 0].
        assert_eq!(analysis.predicate_variables[0], vec![1, 2, 0]);
    }
```

- [ ] **Step 2: Write `orient.rs` with its tests**

```rust
//! The orientation rewrite: makes `--column-orders any` look like `stored`
//! to the executors.
//!
//! Leapfrog Triejoin opens an atom's k-th trie level for its k-th variable
//! in descent order, and Hash Triejoin maps leaf tuples positionally, so
//! both need every atom's term order to equal its trie's column order
//! (`QueryPlan::validate` rejects a plan that would break that). Under
//! [`ColumnOrderPolicy::Any`] the planner ignores stored column orders, so
//! after planning this pass permutes each disagreeing atom's terms into
//! plan order and points the atom at a *copy* of its relation with the
//! columns permuted the same way, `Index_<π>_<base>`. The catalog
//! (`kermit::db::Database`) builds and holds those copies per query; the
//! executors and the data structures never learn about the policy. Runs
//! after the const, placeholder and selection rewrites and after
//! planning, immediately before `JoinAlgo::join_for_each`.

use {
    crate::{
        analysis::{analyse, canonical_names},
        const_rewrite::is_const_predicate,
        optimiser::{ColumnOrderPolicy, QueryPlan},
        selection_rewrite::SelectionSpec,
    },
    kermit_parser::JoinQuery,
    std::collections::HashMap,
};

/// Prefix of the synthetic predicate names this rewrite introduces (e.g.
/// `Index_1_0_edge`). Load-bearing beyond this module: `kermit::db`
/// resolves such a name in its copy store rather than among the base
/// relations, and rejects it in a user's query.
pub const INDEX_PREDICATE_PREFIX: &str = "Index_";

/// Returns `true` iff `name` names a reordered copy introduced by
/// [`orient`].
pub fn is_index_predicate(name: &str) -> bool { name.starts_with(INDEX_PREDICATE_PREFIX) }

/// A relation copy with its columns permuted: column `i` of the copy is
/// column `permutation[i]` of `base`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexSpec {
    /// The synthetic predicate name, `Index_<π joined by _>_<base>`, e.g.
    /// `Index_1_0_edge`.
    pub name: String,
    /// The relation the copy is built from, e.g. `edge`.
    pub base: String,
    /// `permutation[i]` is the base column that becomes column `i`.
    pub permutation: Vec<usize>,
}

impl IndexSpec {
    /// The copy of `base` under `permutation`, named by both.
    pub fn new(base: &str, permutation: Vec<usize>) -> Self {
        let digits: Vec<String> = permutation.iter().map(usize::to_string).collect();
        Self {
            name: format!("{INDEX_PREDICATE_PREFIX}{}_{base}", digits.join("_")),
            base: base.to_string(),
            permutation,
        }
    }

    /// `tuple` of the base relation, reordered into the copy's columns.
    pub fn permute(&self, tuple: &[usize]) -> Vec<usize> {
        self.permutation.iter().map(|&column| tuple[column]).collect()
    }

    /// Every tuple of the base relation, reordered, in the same order.
    pub fn permute_all(&self, tuples: &[Vec<usize>]) -> Vec<Vec<usize>> {
        tuples.iter().map(|tuple| self.permute(tuple)).collect()
    }

    /// The copy for a human: `edge (1, 0)`. The `bench run` metadata line.
    pub fn describe(&self) -> String {
        let digits: Vec<String> = self.permutation.iter().map(usize::to_string).collect();
        format!("{} ({})", self.base, digits.join(", "))
    }
}

/// The executor-ready form of a planned query: the query the executors
/// run, its plan over that query's numbering, the copies it needs and the
/// selection views, which now point at a copy where their atom was
/// reoriented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Oriented {
    /// The query to execute.
    pub query: JoinQuery,
    /// `plan`, renumbered for `query`.
    pub plan: QueryPlan,
    /// The copies `query` reads, one per distinct `(base, permutation)`,
    /// in body order of first use. Empty under `stored`.
    pub index_specs: Vec<IndexSpec>,
    /// The selection views, with `relation` and `equalities` moved to the
    /// copy where the view's atom was reoriented.
    pub selection_specs: Vec<SelectionSpec>,
}

/// Orients `query` to `plan` under `policy`.
///
/// Under [`ColumnOrderPolicy::Stored`] everything is returned unchanged
/// with no copies: the plan already binds every atom left to right.
/// Under [`ColumnOrderPolicy::Any`], for each body atom other than a
/// `Const_*` singleton, π is its columns sorted by plan position (every
/// atom's variables are distinct after the selection rewrite, so the
/// positions are). An identity π leaves the atom alone. Otherwise the
/// atom's terms are permuted; a plain atom is renamed to the copy, and a
/// `Select_<n>_<base>` atom keeps its name while its [`SelectionSpec`]
/// points at the copy with every equality mapped through π and re-sorted
/// so that `source < repeat` still holds (the selection views are
/// positional). Atoms with equal `(base, π)` share one [`IndexSpec`].
///
/// `analyse` numbers body-only variables by first appearance, so
/// permuting terms can renumber them: the plan is translated by variable
/// name through a fresh analysis of the oriented query, and in debug
/// builds checked to validate against it.
///
/// `query` must be the rewritten query (one variable per column), and
/// `plan` its plan.
pub fn orient(
    policy: ColumnOrderPolicy, mut query: JoinQuery, plan: QueryPlan,
    mut selection_specs: Vec<SelectionSpec>,
) -> Oriented {
    if policy == ColumnOrderPolicy::Stored {
        return Oriented {
            query,
            plan,
            index_specs: Vec::new(),
            selection_specs,
        };
    }

    let analysis = analyse(&query);
    let names = canonical_names(&query);
    let mut position = vec![usize::MAX; analysis.num_vars];
    for (pos, &v) in plan.variable_ordering.iter().enumerate() {
        position[v] = pos;
    }
    let plan_names: Vec<&str> = plan
        .variable_ordering
        .iter()
        .map(|&v| names[v].as_str())
        .collect();
    let selection_of: HashMap<String, usize> = selection_specs
        .iter()
        .enumerate()
        .map(|(i, spec)| (spec.name.clone(), i))
        .collect();

    let mut index_specs: Vec<IndexSpec> = Vec::new();
    for (atom, vars) in query.body.iter_mut().zip(&analysis.predicate_variables) {
        if is_const_predicate(&atom.name) {
            continue;
        }
        debug_assert_eq!(
            vars.len(),
            atom.terms.len(),
            "orient reads the rewritten query: one variable per column"
        );
        let mut pi: Vec<usize> = (0..vars.len()).collect();
        pi.sort_by_key(|&column| position[vars[column]]);
        if pi.iter().enumerate().all(|(i, &column)| i == column) {
            continue;
        }
        atom.terms = pi.iter().map(|&column| atom.terms[column].clone()).collect();

        let selection = selection_of.get(&atom.name).copied();
        let base = match selection {
            | Some(i) => selection_specs[i].relation.clone(),
            | None => atom.name.clone(),
        };
        let spec = IndexSpec::new(&base, pi.clone());
        if !index_specs.iter().any(|s| s.name == spec.name) {
            index_specs.push(spec.clone());
        }
        match selection {
            | Some(i) => {
                // `inverse[old] = new`: where each base column went.
                let mut inverse = vec![0; pi.len()];
                for (new, &old) in pi.iter().enumerate() {
                    inverse[old] = new;
                }
                let view = &mut selection_specs[i];
                view.relation = spec.name;
                for equality in &mut view.equalities {
                    let (a, b) = (inverse[equality.source], inverse[equality.repeat]);
                    equality.source = a.min(b);
                    equality.repeat = a.max(b);
                }
                view.equalities.sort_by_key(|e| e.repeat);
            },
            | None => atom.name = spec.name,
        }
    }

    // Translate the plan by name: the oriented query may number its
    // body-only variables differently.
    let renumbered = canonical_names(&query);
    let index_of: HashMap<&str, usize> = renumbered
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), i))
        .collect();
    let plan = QueryPlan {
        variable_ordering: plan_names.iter().map(|name| index_of[name]).collect(),
    };
    debug_assert_eq!(
        plan.validate(&analyse(&query)),
        Ok(()),
        "an oriented query's plan binds every atom left to right"
    );

    Oriented {
        query,
        plan,
        index_specs,
        selection_specs,
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            rewrite_atoms, rewrite_placeholders, rewrite_repeated_variables, ColumnEquality,
            LexicographicOptimiser, QueryOptimiser,
        },
        crate::optimiser::CatalogStats,
    };

    /// `text` after the three rewrites, with its selection views.
    fn rewritten(text: &str) -> (JoinQuery, Vec<SelectionSpec>) {
        let (query, _) = rewrite_atoms(text.parse().expect("parse")).expect("const rewrite");
        rewrite_repeated_variables(rewrite_placeholders(query))
    }

    /// `plan` as canonical indices of `query`, from variable names.
    fn plan_of(query: &JoinQuery, order: &[&str]) -> QueryPlan {
        let names = canonical_names(query);
        QueryPlan {
            variable_ordering: order
                .iter()
                .map(|name| names.iter().position(|n| n == name).expect("a query variable"))
                .collect(),
        }
    }

    fn body_text(query: &JoinQuery) -> Vec<String> {
        query.body.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn stored_returns_its_input_with_no_copies() {
        let (query, specs) = rewritten("Q(X, Y) :- edge(Y, X).");
        let plan = plan_of(&query, &["X", "Y"]);
        let out = orient(ColumnOrderPolicy::Stored, query.clone(), plan.clone(), specs.clone());
        assert_eq!(out, Oriented {
            query,
            plan,
            index_specs: vec![],
            selection_specs: specs,
        });
    }

    #[test]
    fn an_atom_whose_plan_disagrees_is_renamed_and_permuted() {
        let (query, specs) = rewritten("Q(X, Y) :- edge(Y, X).");
        let out = orient(ColumnOrderPolicy::Any, query.clone(), plan_of(&query, &["X", "Y"]), specs);
        assert_eq!(body_text(&out.query), vec!["Index_1_0_edge(X, Y)"]);
        assert_eq!(out.index_specs, vec![IndexSpec::new("edge", vec![1, 0])]);
        assert_eq!(out.index_specs[0].name, "Index_1_0_edge");
        assert_eq!(out.plan.variable_ordering, vec![0, 1]);
    }

    #[test]
    fn an_atom_that_agrees_is_untouched() {
        let (query, specs) = rewritten("Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).");
        let out = orient(ColumnOrderPolicy::Any, query.clone(), plan_of(&query, &["X", "Y", "Z"]), specs);
        assert_eq!(out.query, query);
        assert!(out.index_specs.is_empty());
    }

    #[test]
    fn equal_base_and_permutation_share_one_copy() {
        let (query, specs) = rewritten("Q(X, Y, Z) :- edge(Y, X), edge(Z, X).");
        let out = orient(ColumnOrderPolicy::Any, query.clone(), plan_of(&query, &["X", "Y", "Z"]), specs);
        assert_eq!(body_text(&out.query), vec!["Index_1_0_edge(X, Y)", "Index_1_0_edge(X, Z)"]);
        assert_eq!(out.index_specs, vec![IndexSpec::new("edge", vec![1, 0])]);
    }

    #[test]
    fn two_permutations_of_one_base_are_two_copies() {
        let (query, specs) = rewritten("Q(X, Y, Z) :- r(Y, X, Z), r(Z, X, Y).");
        let out = orient(ColumnOrderPolicy::Any, query.clone(), plan_of(&query, &["X", "Y", "Z"]), specs);
        assert_eq!(body_text(&out.query), vec![
            "Index_1_0_2_r(X, Y, Z)",
            "Index_1_2_0_r(X, Y, Z)"
        ]);
        assert_eq!(out.index_specs, vec![
            IndexSpec::new("r", vec![1, 0, 2]),
            IndexSpec::new("r", vec![1, 2, 0]),
        ]);
    }

    #[test]
    fn const_singletons_are_never_oriented() {
        // `edge(c5, X)` rewrites to `edge(K0, X), Const_c5(K0)`; the plan
        // binds X first.
        let (query, specs) = rewritten("Q(X) :- edge(c5, X).");
        let out = orient(ColumnOrderPolicy::Any, query.clone(), plan_of(&query, &["X", "K0"]), specs);
        assert_eq!(body_text(&out.query), vec!["Index_1_0_edge(X, K0)", "Const_c5(K0)"]);
        assert_eq!(out.index_specs.len(), 1);
    }

    /// `r(X, X, Y)` is `Select_0_r(X, K0, Y)` with the equality (0, 1).
    /// Binding Y first permutes the atom to (Y, X, K0): the view points
    /// at `Index_2_0_1_r` and the equality becomes (1, 2).
    #[test]
    fn a_selection_view_points_at_its_copy_and_remaps_its_equality() {
        let (query, specs) = rewritten("Q(X, Y) :- r(X, X, Y).");
        assert_eq!(specs[0].equalities, vec![ColumnEquality {
            source: 0,
            repeat: 1
        }]);
        let out = orient(ColumnOrderPolicy::Any, query.clone(), plan_of(&query, &["Y", "X", "K0"]), specs);
        assert_eq!(body_text(&out.query), vec!["Select_0_r(Y, X, K0)"]);
        assert_eq!(out.index_specs, vec![IndexSpec::new("r", vec![2, 0, 1])]);
        assert_eq!(out.selection_specs[0].name, "Select_0_r");
        assert_eq!(out.selection_specs[0].relation, "Index_2_0_1_r");
        assert_eq!(out.selection_specs[0].equalities, vec![ColumnEquality {
            source: 1,
            repeat: 2
        }]);
    }

    /// When the repeat column comes first in the plan, the remapped pair is
    /// re-sorted so the source is still the earlier column.
    #[test]
    fn a_remapped_equality_keeps_its_source_first() {
        let (query, specs) = rewritten("Q(X, Y) :- r(X, Y, X).");
        // Select_0_r(X, Y, K0), equality (0, 2). Plan K0, Y, X: π = [2, 1, 0].
        let out = orient(ColumnOrderPolicy::Any, query.clone(), plan_of(&query, &["K0", "Y", "X"]), specs);
        assert_eq!(body_text(&out.query), vec!["Select_0_r(K0, Y, X)"]);
        assert_eq!(out.selection_specs[0].equalities, vec![ColumnEquality {
            source: 0,
            repeat: 2
        }]);
    }

    /// `r(X, A, B)` reoriented to `(X, B, A)` makes `B` the first body-only
    /// variable, so its canonical index changes; the plan follows the
    /// names, not the old indices.
    #[test]
    fn the_plan_is_translated_by_name() {
        let (query, specs) = rewritten("Q(X) :- r(X, A, B), s(B, A).");
        // Canonical: X = 0, A = 1, B = 2. Plan X, B, A = [0, 2, 1].
        let plan = plan_of(&query, &["X", "B", "A"]);
        assert_eq!(plan.variable_ordering, vec![0, 2, 1]);
        let out = orient(ColumnOrderPolicy::Any, query, plan, specs);
        assert_eq!(body_text(&out.query), vec!["Index_0_2_1_r(X, B, A)", "s(B, A)"]);
        // Now B = 1, A = 2, and the plan still reads X, B, A.
        assert_eq!(canonical_names(&out.query), vec!["X", "B", "A"]);
        assert_eq!(out.plan.variable_ordering, vec![0, 1, 2]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    /// End to end with a real optimiser: the cyclic-under-`stored` query
    /// orients to one copy and a plan the executors accept.
    #[test]
    fn a_cyclic_query_orients_to_a_valid_plan() {
        let (query, specs) = rewritten("Q(X, Y) :- edge(X, Y), edge(Y, X).");
        let stats = CatalogStats::for_query(&query, ColumnOrderPolicy::Any, |_| None);
        let plan = LexicographicOptimiser.plan(&query, &stats);
        let out = orient(ColumnOrderPolicy::Any, query, plan, specs);
        assert_eq!(body_text(&out.query), vec!["edge(X, Y)", "Index_1_0_edge(X, Y)"]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    #[test]
    fn specs_permute_tuples_and_describe_themselves() {
        let spec = IndexSpec::new("edge", vec![1, 0]);
        assert_eq!(spec.permute(&[7, 9]), vec![9, 7]);
        assert_eq!(spec.permute_all(&[vec![1, 2], vec![3, 4]]), vec![vec![2, 1], vec![4, 3]]);
        assert_eq!(spec.describe(), "edge (1, 0)");
        assert!(is_index_predicate(&spec.name));
        assert!(!is_index_predicate("edge"));
    }
}
```

- [ ] **Step 3: Register and re-export**

In `lib.rs`: add `mod orient;` to the module list (alphabetical, after
`optimiser`), and to the `pub use` block:

```rust
    analysis::{analyse, canonical_names, QueryAnalysis},
    …
    orient::{is_index_predicate, orient, IndexSpec, Oriented, INDEX_PREDICATE_PREFIX},
```

Extend the crate doc's layout paragraph: "`orient`, which runs after
planning and renames each atom whose plan disagrees with its stored
column order to a reordered copy (`--column-orders any`)".

- [ ] **Step 4: Run, format, commit**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos orient canonical 2>&1 | grep -E '^test result|FAILED|panicked'
nix develop $WT --command cargo fmt --all
git -C $WT add kermit-algos/src/orient.rs kermit-algos/src/analysis.rs kermit-algos/src/lib.rs
git -C $WT commit -q -F - <<'MSG'
feat(algos): the orientation rewrite (#93)

After planning under `any`, each atom whose plan disagrees with its
stored column order has its terms permuted and reads a reordered copy,
`Index_<π>_<base>`; a selection view points at the copy with its
equalities remapped; the plan is translated by variable name. The
executors see a query whose term orders equal its tries' column orders.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
```

- [ ] **Step 5: Mutation checks**

1. Drop the equality remap: comment out the `for equality in &mut
   view.equalities { … }` loop. Expected to fail:
   `a_selection_view_points_at_its_copy_and_remaps_its_equality`,
   `a_remapped_equality_keeps_its_source_first`. Revert.
2. Drop the plan translation: replace the translated `plan` with the
   input `plan`. Expected to fail: `the_plan_is_translated_by_name`
   (the debug assertion fires, or the ordering differs). Revert.

---

# P2 — catalog and entry points (`kermit::db`)

## Task 5 [P2]: The catalog holds a query's copies

**Files:**
- Modify: `kermit/src/db/database.rs` (struct lines 22–26, `new` 37–48, `From` 68–83, accessors 51–63, tests)
- Modify: `kermit/src/db.rs` (re-exports, lines 23–26)

No behaviour change: the store is empty until Task 6 fills it.

- [ ] **Step 1: Failing tests**

Append to `database.rs`'s `mod tests` (imports: add `kermit_algos::IndexSpec`,
`kermit_ds::{ColumnTrieBuildMode, ConfigProvider, Configured, HashTrieConfig, LoadFactor, BuildModeProvider, BuiltWith, ConfigurableRelation, RelationHeader}`,
`std::cell::Cell`, and `super::{build_index, index_header}`):

```rust
    #[test]
    fn an_added_copy_is_found_by_its_name_and_cleared() {
        let mut database = Database::from(store::<TreeTrie>(edges()));
        assert!(database.index("Index_1_0_edge").is_none());
        assert_eq!(database.indexes().count(), 0);

        let spec = IndexSpec::new("edge", vec![1, 0]);
        let header = RelationHeader::new_positional("edge", 2);
        let copy: TreeTrie = build_index(&spec, &header, &edges());
        database.add_index(spec.clone(), copy);
        assert!(database.index("Index_1_0_edge").is_some());
        assert!(database.get("Index_1_0_edge").is_none(), "copies are not base relations");
        let held: Vec<&IndexSpec> = database.indexes().map(|(spec, _)| spec).collect();
        assert_eq!(held, vec![&spec]);
        // The statistics are read-only and know nothing of copies.
        assert_eq!(database.statistics("Index_1_0_edge"), None);

        database.clear_indexes();
        assert!(database.index("Index_1_0_edge").is_none());
        assert_eq!(database.indexes().count(), 0);
        assert!(database.get("edge").is_some(), "clearing copies keeps the base relations");
    }

    #[test]
    fn index_header_permutes_named_attributes_and_keeps_positional_arity() {
        let spec = IndexSpec::new("edge", vec![1, 0]);
        let named = RelationHeader::new("edge", vec!["src".into(), "dst".into()]);
        let header = index_header(&spec, &named);
        assert_eq!(header.name(), "Index_1_0_edge");
        assert_eq!(header.attrs(), ["dst".to_string(), "src".to_string()]);
        assert_eq!(header.arity(), 2);
        let positional = index_header(&spec, &RelationHeader::new_positional("edge", 2));
        assert_eq!(positional.name(), "Index_1_0_edge");
        assert!(positional.attrs().is_empty());
        assert_eq!(positional.arity(), 2);
    }

    #[test]
    fn build_index_permutes_the_tuples_in_file_order() {
        let spec = IndexSpec::new("edge", vec![1, 0]);
        let header = RelationHeader::new_positional("edge", 2);
        let copy: ColumnTrie = build_index(&spec, &header, &edges());
        assert_eq!(copy.header().name(), "Index_1_0_edge");
        let mut stored = Vec::new();
        SortedFamily::for_each_tuple(&copy, |t| stored.push(t.to_vec()));
        assert_eq!(stored, vec![vec![10, 1], vec![10, 2], vec![20, 1], vec![30, 3]]);
    }

    kermit_ds::define_config_provider!(HalfFull, HashTrieConfig, HashTrieConfig {
        load_factor: LoadFactor::percent(50).unwrap(),
    });

    thread_local! {
        static BUILD_CALLS: Cell<usize> = const { Cell::new(0) };
    }

    /// A build-mode provider that counts how often the build asks for it.
    struct CountedIncremental;

    impl BuildModeProvider<ColumnTrieBuildMode> for CountedIncremental {
        fn build_mode() -> ColumnTrieBuildMode {
            BUILD_CALLS.with(|calls| calls.set(calls.get() + 1));
            ColumnTrieBuildMode::Incremental
        }
    }

    /// A copy is built the way its relation type builds, so a configured
    /// hash trie keeps its load factor and a `BuiltWith` column trie asks
    /// its provider for the mode — which `Projectable::project` would not
    /// do (it rebuilds through `from_tuples` with a nameless header).
    #[test]
    fn build_index_keeps_config_and_build_mode() {
        let spec = IndexSpec::new("edge", vec![1, 0]);
        let header = RelationHeader::new_positional("edge", 2);

        let hashed: Configured<HashTrie<SipHashStrategy>, HalfFull> =
            build_index(&spec, &header, &edges());
        assert_eq!(hashed.config().load_factor, LoadFactor::percent(50).unwrap());
        assert_eq!(hashed.header().name(), "Index_1_0_edge");

        BUILD_CALLS.with(|calls| calls.set(0));
        let column: BuiltWith<ColumnTrie, CountedIncremental> = build_index(&spec, &header, &edges());
        assert_eq!(BUILD_CALLS.with(Cell::get), 1, "the copy asked the provider for its mode");
        assert_eq!(column.header().name(), "Index_1_0_edge");
    }
```

(`Configured<R, P>` derefs to `R`, and `HashTrie` implements
`ConfigurableRelation::config()`, so `hashed.config()` resolves through
`Deref`; if it does not, call `(*hashed).config()`.)

Run `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit
--lib database` — expected: compile errors (`index`, `indexes`,
`add_index`, `clear_indexes`, `build_index`, `index_header` missing).

- [ ] **Step 2: The store**

In `database.rs`, add `kermit_algos::IndexSpec` and
`kermit_ds::RelationHeader` to the imports, and extend the type:

```rust
/// Relations by name, plus the statistics a query optimiser reads, plus
/// the reordered copies the current query needs.
///
/// A database is a catalog in the textbook sense: planners read its
/// statistics, never its relations. The statistics are gathered once, when
/// the database is built ("ANALYZE"), up to the [`StatisticsLevel`] asked
/// for, and nothing after that can change them, so they cannot drift from
/// the relations. Build it at the level the optimiser declares
/// ([`Planner::required_statistics`]). The join entry points return
/// [`JoinError::MissingStatistics`] rather than plan from less.
///
/// Under `--column-orders any` a plan may read an atom through a copy of
/// its relation with the columns permuted (`Index_<π>_<base>`, see
/// [`kermit_algos::orient`]). The catalog *stores* those copies and never
/// builds them: [`required_indexes`](Self::required_indexes) says which
/// copies a query needs, the caller builds each from the base relation's
/// file-order tuples ([`build_index`] in the library, the bench family's
/// `build_relation` in the binary) and [`add_index`](Self::add_index)es
/// it, and [`clear_indexes`](Self::clear_indexes) drops them after the
/// query, so one query's copies are held at a time. Statistics stay
/// those of the base relations: the planner sees only base names.
///
/// [`Planner::required_statistics`]: kermit_algos::Planner::required_statistics
/// [`JoinError::MissingStatistics`]: super::JoinError::MissingStatistics
/// [`build_index`]: super::build_index
pub struct Database<R> {
    relations: BTreeMap<String, R>,
    statistics: BTreeMap<String, RelationStats>,
    level: StatisticsLevel,
    /// The current query's copies, by their `Index_*` name.
    indexes: BTreeMap<String, (IndexSpec, R)>,
}
```

Both constructors set `indexes: BTreeMap::new()` (`From` in its struct
literal; `new` inherits it through `Self::from`). Add to the `impl<R>
Database<R>` block:

```rust
    /// The reordered copy queries call `name`, if the store holds one.
    pub fn index(&self, name: &str) -> Option<&R> { self.indexes.get(name).map(|(_, copy)| copy) }

    /// Every copy the store holds, with its spec, in name order.
    pub fn indexes(&self) -> impl Iterator<Item = (&IndexSpec, &R)> {
        self.indexes.values().map(|(spec, copy)| (spec, copy))
    }

    /// Drops every copy. The base relations and statistics stay.
    pub fn clear_indexes(&mut self) { self.indexes.clear(); }
```

and a new block:

```rust
impl<R: Relation> Database<R> {
    /// Stores `copy` as the relation `spec` names, replacing any copy of
    /// that name. `copy` is `spec.base` with its columns permuted by
    /// `spec.permutation`; see [`build_index`](super::build_index).
    ///
    /// # Panics
    ///
    /// In debug builds, if `copy`'s arity is not the permutation's length
    /// or the base relation's arity.
    pub fn add_index(&mut self, spec: IndexSpec, copy: R) {
        debug_assert_eq!(
            copy.header().arity(),
            spec.permutation.len(),
            "a copy has one column per permutation entry"
        );
        if let Some(base) = self.relations.get(&spec.base) {
            debug_assert_eq!(
                copy.header().arity(),
                base.header().arity(),
                "a copy has its base relation's arity"
            );
        }
        self.indexes.insert(spec.name.clone(), (spec, copy));
    }
}
```

Add the two free functions at the end of `database.rs` (before the
tests):

```rust
/// The header of `spec`'s copy: named `spec.name`, with the base's
/// attribute names permuted (or positional, if the base is).
pub fn index_header(spec: &IndexSpec, base: &RelationHeader) -> RelationHeader {
    if base.attrs().is_empty() {
        RelationHeader::new_positional(&spec.name, base.arity())
    } else {
        let attrs = spec
            .permutation
            .iter()
            .map(|&column| base.attrs()[column].clone())
            .collect();
        RelationHeader::new(&spec.name, attrs)
    }
}

/// Builds `spec`'s copy from its base relation's `tuples`, in the order
/// given (file order, for the same reason `insertion` rebuilds from it),
/// through `R::from_tuples`, so a `Configured` or `BuiltWith` relation
/// keeps its config or build mode. Never through `Projectable::project`,
/// which rebuilds in the structure's own iteration order with a nameless
/// header and drops the build mode.
pub fn build_index<R: Relation>(
    spec: &IndexSpec, base: &RelationHeader, tuples: &[Vec<usize>],
) -> R {
    R::from_tuples(index_header(spec, base), spec.permute_all(tuples))
}
```

In `db.rs`'s `pub use`, export them: `database::{build_index, index_header,
Database}`.

- [ ] **Step 3: Run, format, commit**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --lib database 2>&1 | grep -E '^test result|FAILED|panicked'
nix develop $WT --command cargo fmt --all
git -C $WT add kermit/src/db/database.rs kermit/src/db.rs
git -C $WT commit -q -F - <<'MSG'
feat(db): the catalog holds a query's reordered copies (#93)

`Database` gains a copy store with explicit add, lookup and clear, and
`build_index` builds a copy from its base's file-order tuples through
`from_tuples`, so config and build mode carry over. Nothing fills the
store yet.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
```

- [ ] **Step 4: Mutation check: copies are not built via `project`**

Mutant: build the copy the way a careless implementer would, through
`Projectable::project`. Replace `build_index`'s body with:

```rust
    let whole: R = R::from_tuples(index_header(spec, base), tuples.to_vec());
    whole.project(spec.permutation.clone())
```

Expected to fail: `build_index_permutes_the_tuples_in_file_order` and
`build_index_keeps_config_and_build_mode` (`project` returns a nameless
header, so the `Index_1_0_edge` name assertions fail; the `BuiltWith`
provider is also asked twice). Revert.

---

## Task 6 [P2]: The entry points plan under a `Planner` and run over copies

**Files:**
- Modify: `kermit/src/db/validation.rs` (imports 19–31; `JoinError` 40–106 and its `Display` 108–181; `prepare` 253–283; `validate_query` 302–306; `check_body_atoms` 327–335; tests)
- Modify: `kermit/src/db.rs` (imports 27–40; `run_join` 185–278; the four entry points 280–369; tests 371–1087)
- Modify: `kermit/src/execution.rs` (`TrieLftj` 608–680, `HashHtj` 682–758, tests)
- Modify: `kermit/src/bench/run.rs:89` and `:337-367`, `kermit/src/bench/workload.rs:108-120`, `kermit/src/main.rs` (`validate_query_files` 616–624, `load_query_runner` 648–676)
- Modify tests: `kermit/tests/common/utils.rs`, `lubm_mini_oracle.rs`, `lubm_cardinalities.rs`, `result_allocation.rs`, `watdiv_correctness.rs`, `subject_position_constant.rs`

Under `stored` nothing observable changes. The CLI still only offers
`--optimiser`; it builds `Planner::new(optimiser.instantiate(),
ColumnOrderPolicy::Stored)` until Task 8.

- [ ] **Step 1: Validation takes the policy; `Index_` is reserved; `MissingIndex`**

In `validation.rs`, import `ColumnOrderPolicy`, `is_index_predicate` and
`INDEX_PREDICATE_PREFIX` from `kermit_algos`. Extend `JoinError`:

```rust
    /// A body atom names a relation with a prefix reserved for the
    /// predicates the join synthesises (`Const_`, `Select_`, `Index_`).
    ReservedRelationName {
        /// The relation name as written.
        relation: String,
    },
    …
    /// The body atoms need their relations' columns in contradictory
    /// orders under `--column-orders stored`. A limitation of reading each
    /// relation in one column order, not malformed input: each atom is
    /// matched one column at a time, left to right, so
    /// `edge(X, Y), edge(Y, X)` would need `edge` sorted both ways at
    /// once. `--column-orders any` lifts it by reading one of the atoms
    /// through a reordered copy.
    CyclicAttributeOrder {
        /// The atoms on one such cycle, as written, in body order.
        atoms: Vec<String>,
    },
    …
    /// The plan reads an atom through a reordered copy the database does
    /// not hold. A library-usage error: build the copies
    /// [`Database::required_indexes`](super::Database::required_indexes)
    /// names and [`add_index`](super::Database::add_index) them before
    /// joining, as the CLI and the test harness do.
    MissingIndex {
        /// The copy's name, e.g. `Index_1_0_edge`.
        index: String,
        /// The relation it would be built from.
        base: String,
    },
```

Display arms:

```rust
            | JoinError::ReservedRelationName {
                relation,
            } => write!(
                f,
                "relation name {relation:?} is reserved: the prefixes {CONST_PREDICATE_PREFIX:?}, \
                 {SELECTION_PREDICATE_PREFIX:?} and {INDEX_PREDICATE_PREFIX:?} name predicates the \
                 join synthesises from constants, repeated variables and reordered copies"
            ),
            …
            | JoinError::CyclicAttributeOrder {
                atoms,
            } => write!(
                f,
                "unsupported query under --column-orders stored: atoms {} need their relations' \
                 columns in contradictory orders. Each body atom is matched one column at a time, \
                 left to right (subject before object for an RDF triple), so it fixes the order \
                 its variables are bound in, and these atoms fix opposite orders. This is a \
                 limitation of reading each relation in its stored column order, not an error in \
                 the query; `--column-orders any` answers it over a reordered copy",
                atoms
                    .iter()
                    .map(|a| format!("`{a}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            …
            | JoinError::MissingIndex {
                index,
                base,
            } => write!(
                f,
                "the plan reads {base:?} through the reordered copy {index:?}, which the database \
                 does not hold; build the copies `required_indexes` names before joining"
            ),
```

`check_body_atoms` rejects the third prefix:

```rust
        if is_const_predicate(&atom.name)
            || is_selection_predicate(&atom.name)
            || is_index_predicate(&atom.name)
        {
```

`prepare` and `validate_query` take the policy; the cyclic check runs
under `stored` only:

```rust
pub(super) fn prepare(
    query: &JoinQuery, relations: &(impl RelationArities + ?Sized),
    column_orders: ColumnOrderPolicy,
) -> Result<Prepared, JoinError> {
    check_head_terms(query)?;
    check_body_atoms(query, relations)?;
    check_head_bound(query)?;

    let (rewritten, const_specs) = rewrite_atoms(query.clone())?;
    let rewritten = rewrite_placeholders(rewritten);
    let (rewritten, selection_specs) = rewrite_repeated_variables(rewritten);

    // Under `stored` every atom binds its columns left to right, so the
    // atoms must admit one order per relation; under `any` the planner
    // is free and the orientation rewrite reads a disagreeing atom
    // through a reordered copy, so there is nothing to check. The
    // rewrites decide which variables each atom carries, so the check
    // runs on their output; they keep body atoms in place, so the
    // indices it reports locate the atoms as the user wrote them.
    if column_orders == ColumnOrderPolicy::Stored {
        check_attribute_order(&rewritten).map_err(|cycle| JoinError::CyclicAttributeOrder {
            atoms: cycle
                .atoms
                .iter()
                .map(|&i| query.body[i].to_string())
                .collect(),
        })?;
    }

    Ok(Prepared {
        query: rewritten,
        const_specs,
        selection_specs,
    })
}

/// Checks that `query` can run over a store holding `relations` under
/// `column_orders`.
///
/// … (keep the numbered list; item 5 becomes "under `stored`, the atoms
/// admit one column order per relation") …
pub fn validate_query(
    query: &JoinQuery, relations: &(impl RelationArities + ?Sized),
    column_orders: ColumnOrderPolicy,
) -> Result<(), JoinError> {
    prepare(query, relations, column_orders).map(|_| ())
}
```

Tests in `validation.rs`: the local `validate(q)` helper passes
`ColumnOrderPolicy::Stored`; add `validate_any(q)` passing `Any`, add the
`Index_` row to `synthetic_prefixes_are_reserved`:

```rust
            ("Q(X, Y) :- Index_1_0_edge(X, Y).", "Index_1_0_edge"),
```

and the new tests:

```rust
    #[test]
    fn under_any_opposite_column_orders_are_accepted() {
        assert_eq!(validate_any("Q(X, Y) :- edge(X, Y), edge(Y, X)."), Ok(()));
        assert_eq!(validate_any("Q(X) :- r(X, _, Y), edge(Y, X)."), Ok(()));
        // Every other check still runs.
        assert!(matches!(
            validate_any("Q(X, Y) :- edge(X, Z)."),
            Err(JoinError::UnboundHeadVariable { .. })
        ));
    }

    #[test]
    fn the_cyclic_message_names_the_flag_that_lifts_it() {
        let err = validate("Q(X, Y) :- edge(X, Y), edge(Y, X).").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("--column-orders any"), "{message}");
        assert!(message.contains("limitation"), "{message}");
        let missing = JoinError::MissingIndex {
            index: "Index_1_0_edge".into(),
            base: "edge".into(),
        };
        assert!(missing.to_string().contains("required_indexes"));
    }
```

- [ ] **Step 2: The shared body plans, orients and resolves copies**

In `db.rs`, the imports become:

```rust
use {
    kermit_algos::{
        is_const_predicate, is_index_predicate, is_selection_predicate, orient, CatalogStats,
        ColumnEquality, ConstSpec, HashTrieIterKind, HashTriejoin, IndexSpec, JoinAlgo, JoinQuery,
        Oriented, Planner, QueryPlan, SelectionSpec, SingletonHashTrieIter, SingletonTrieIter,
        TrieIterKind,
    },
    …
```

Add, before `run_join`:

```rust
/// A query ready to execute: validated, rewritten, planned and oriented.
struct Planned {
    /// The query the executor runs.
    query: JoinQuery,
    plan: QueryPlan,
    const_specs: Vec<ConstSpec>,
    /// The copies the query reads (`Index_*` atoms and reoriented views).
    index_specs: Vec<IndexSpec>,
    selection_specs: Vec<SelectionSpec>,
}

/// Validation and the query rewrites ([`prepare`]), the statistics gate,
/// the plan, and the orientation: everything that decides *what* runs,
/// shared by the join itself and by [`Database::required_indexes`], so
/// the copies a caller builds are the copies the join will read.
fn plan_query<R: Relation + Cardinality>(
    database: &Database<R>, query: &JoinQuery, planner: &Planner,
) -> Result<Planned, JoinError> {
    let Prepared {
        query: rewritten,
        const_specs,
        selection_specs,
    } = prepare(query, database, planner.column_orders())?;

    // The plan comes from the database's statistics, so the optimiser must
    // not read more than were gathered when it was built.
    let required = planner.required_statistics();
    if required > database.level() {
        return Err(JoinError::MissingStatistics {
            required,
            available: database.level(),
        });
    }

    // Stats + planning run per join — inside benchmarks' measured region —
    // so this stays O(#predicates) reads of statistics the database
    // gathered when it was built. A selection view reports its base
    // relation's statistics: upper bounds, the conservative values for a
    // size-driven planner. The statistics are built *before* orientation,
    // so the planner sees only base names.
    let base_of: HashMap<&str, &str> = selection_specs
        .iter()
        .map(|s| (s.name.as_str(), s.relation.as_str()))
        .collect();
    let stats = CatalogStats::for_query(&rewritten, planner.column_orders(), |name| {
        let base = base_of.get(name).copied().unwrap_or(name);
        database.statistics(base).cloned()
    });
    let plan = planner.plan(&rewritten, &stats);

    let Oriented {
        query,
        plan,
        index_specs,
        selection_specs,
    } = orient(planner.column_orders(), rewritten, plan, selection_specs);
    Ok(Planned {
        query,
        plan,
        const_specs,
        index_specs,
        selection_specs,
    })
}

impl<R: Relation + Cardinality> Database<R> {
    /// The reordered copies `query` needs under `planner` that the store
    /// does not hold yet: validate, rewrite, plan and orient the query
    /// exactly as the join will, and keep each `Index_*` the plan reads
    /// that [`index`](Self::index) does not find. Build each from its
    /// base relation's file-order tuples ([`build_index`]) and
    /// [`add_index`](Self::add_index) it before joining. Empty under
    /// `stored`, and once the copies are held.
    ///
    /// # Errors
    ///
    /// As the join entry points: the [`JoinError`] [`validate_query`]
    /// would return, or [`JoinError::MissingStatistics`].
    pub fn required_indexes(
        &self, query: &JoinQuery, planner: &Planner,
    ) -> Result<Vec<IndexSpec>, JoinError> {
        let planned = plan_query(self, query, planner)?;
        Ok(planned
            .index_specs
            .into_iter()
            .filter(|spec| self.index(&spec.name).is_none())
            .collect())
    }
}
```

Replace `run_join`:

```rust
/// The one join body: [`plan_query`] (validation, rewrites, statistics,
/// plan, orientation), wrapper map, execute, project.
///
/// Each result tuple is passed to `emit` as a borrowed slice of exactly
/// the head's columns; the result is never materialised here.
///
/// # Errors
///
/// Returns the [`JoinError`] [`validate_query`] would,
/// [`JoinError::MissingStatistics`], or [`JoinError::MissingIndex`] when
/// the plan reads a copy the database does not hold, before any tuple is
/// emitted.
fn run_join<'a, R, F, JA, S>(
    database: &'a Database<R>, query: JoinQuery, planner: &Planner, mut emit: S,
) -> Result<(), JoinError>
where
    R: Relation + Cardinality + 'a,
    F: JoinFamily<R>,
    JA: JoinAlgo<F::Wrapper<'a>>,
    S: FnMut(&[usize]),
{
    let head_len = query.head.terms.len();
    let Planned {
        query: oriented,
        plan,
        const_specs,
        index_specs,
        selection_specs,
    } = plan_query(database, &query, planner)?;

    // `prepare` has checked that every base relation the body names is
    // present; a copy is present only if the caller built it.
    let lookup = |name: &str| -> Result<&'a R, JoinError> {
        if is_index_predicate(name) {
            database.index(name).ok_or_else(|| JoinError::MissingIndex {
                index: name.to_string(),
                base: index_specs
                    .iter()
                    .find(|spec| spec.name == name)
                    .map(|spec| spec.base.clone())
                    .unwrap_or_default(),
            })
        } else {
            Ok(database
                .get(name)
                .expect("`prepare` checked that the database holds every body relation"))
        }
    };

    let mut wrappers: HashMap<String, F::Wrapper<'a>> = HashMap::new();
    for pred in &oriented.body {
        if wrappers.contains_key(&pred.name) {
            continue;
        }
        // Const_* and Select_* predicates are synthetic — created by the
        // rewrites and materialised from their specs below. They aren't
        // expected to live in the database. Index_* predicates are copies
        // the caller added to it.
        if is_const_predicate(&pred.name) || is_selection_predicate(&pred.name) {
            continue;
        }
        wrappers.insert(pred.name.clone(), F::wrap_relation(lookup(&pred.name)?));
    }
    for (name, id) in const_specs {
        wrappers.entry(name).or_insert_with(|| F::wrap_const(id));
    }
    for spec in &selection_specs {
        // A view over a reoriented atom reads its copy.
        let base = lookup(&spec.relation)?;
        wrappers.insert(
            spec.name.clone(),
            F::wrap_selection(base, spec.equalities.clone()),
        );
    }

    let ds_map: HashMap<String, &F::Wrapper<'a>> =
        wrappers.iter().map(|(k, v)| (k.clone(), v)).collect();

    // Projection to the head (#71): unchanged — `analyse` numbers the
    // head's variables `0..head_len` in head order on the oriented query
    // too, since orientation touches body atoms only.
    JA::join_for_each(&plan, oriented, ds_map, |row| emit(&row[..head_len]));
    Ok(())
}
```

The four entry points replace `optimiser: &dyn QueryOptimiser` with
`planner: &Planner` in their signature, doc ("planned by `planner`") and
the `run_join` call. Update the module doc's first paragraph: "all over a
[`Database`] … plus the reordered copies the current query reads under
`--column-orders any`". Remove `ColumnOrderPolicy` and `QueryOptimiser`
from the imports if now unused.

- [ ] **Step 3: Migrate `db.rs`'s tests and add the `any` ones**

Mechanical rule for the existing tests: every `&LexicographicOptimiser`
argument becomes `&Planner::stored(LexicographicOptimiser)`, and
`&NeedsColumns` becomes `&Planner::stored(NeedsColumns)`. `StatsSpy` must
be shared with the planner that owns it, so it becomes clonable:

```rust
    /// Records the query and statistics `run_join` hands it, and plans like
    /// `lexicographic`.
    #[derive(Default, Clone)]
    struct StatsSpy(Rc<RefCell<Option<(JoinQuery, CatalogStats)>>>);
```

(import `std::rc::Rc`), and `the_planner_reads_the_databases_statistics`
passes `&Planner::stored(spy.clone())`. Add to the test imports
`kermit_algos::{ColumnOrderPolicy, Planner}`, `kermit_ds::{ColumnTrie, RelationHeader}`
and `crate::db::build_index`, then add:

```rust
    /// The mutual-edge graph: 1 ↔ 2, 2 ↔ 3 and the one-way 1 → 3.
    fn mutual_edges() -> Vec<Vec<usize>> {
        vec![vec![1, 2], vec![2, 1], vec![2, 3], vec![3, 2], vec![1, 3]]
    }

    fn any(optimiser: impl QueryOptimiser + 'static) -> Planner {
        Planner::new(Box::new(optimiser), ColumnOrderPolicy::Any)
    }

    /// Builds the copies `planner` needs for `query` into `database` from
    /// `tuples` (base name → file-order tuples), as the CLI and the test
    /// harness do.
    fn add_copies<R: Relation + Cardinality>(
        database: &mut Database<R>, query: &JoinQuery, planner: &Planner,
        tuples: &BTreeMap<&str, Vec<Vec<usize>>>,
    ) -> Vec<IndexSpec> {
        let specs = database.required_indexes(query, planner).unwrap();
        for spec in &specs {
            let base = database.get(&spec.base).unwrap().header().clone();
            database.add_index(spec.clone(), build_index(spec, &base, &tuples[spec.base.as_str()]));
        }
        specs
    }

    /// `edge(X, Y), edge(Y, X)` is a cycle under `stored` and a one-copy
    /// query under `any`, in both families.
    #[test]
    fn under_any_a_cyclic_query_runs_over_a_copy() {
        let query: JoinQuery = "Q(X, Y) :- edge(X, Y), edge(Y, X).".parse().unwrap();
        let tuples = BTreeMap::from([("edge", mutual_edges())]);
        let want = vec![vec![1, 2], vec![2, 1], vec![2, 3], vec![3, 2]];

        let mut sorted = rels(vec![("edge", 2, mutual_edges())]);
        assert!(matches!(
            lftj_join::<TreeTrie, LeapfrogTriejoin>(
                &sorted,
                query.clone(),
                &Planner::stored(LexicographicOptimiser)
            ),
            Err(JoinError::CyclicAttributeOrder { .. })
        ));
        let planner = any(LexicographicOptimiser);
        let specs = add_copies(&mut sorted, &query, &planner, &tuples);
        assert_eq!(specs, vec![IndexSpec::new("edge", vec![1, 0])]);
        let mut got = lftj_join::<TreeTrie, LeapfrogTriejoin>(&sorted, query.clone(), &planner).unwrap();
        got.sort();
        assert_eq!(got, want);
        assert!(
            sorted.required_indexes(&query, &planner).unwrap().is_empty(),
            "held copies are not required again"
        );

        let mut hashed: Database<HashTrie> = Database::from(BTreeMap::from([(
            "edge".to_string(),
            HashTrie::from_tuples(2.into(), mutual_edges()),
        )]));
        add_copies(&mut hashed, &query, &planner, &tuples);
        let mut got =
            hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(&hashed, query, &planner).unwrap();
        got.sort();
        assert_eq!(got, want);
    }

    /// A caller that skips `required_indexes` gets a typed error before
    /// any row.
    #[test]
    fn a_copy_the_store_lacks_is_a_missing_index() {
        let query: JoinQuery = "Q(X, Y) :- edge(X, Y), edge(Y, X).".parse().unwrap();
        let database = rels(vec![("edge", 2, mutual_edges())]);
        let mut rows = 0;
        let refused = lftj_join_for_each::<TreeTrie, LeapfrogTriejoin>(
            &database,
            query,
            &any(LexicographicOptimiser),
            |_| rows += 1,
        );
        assert_eq!(
            refused,
            Err(JoinError::MissingIndex {
                index: "Index_1_0_edge".into(),
                base: "edge".into(),
            })
        );
        assert_eq!(rows, 0);
    }

    /// Under `stored` a plan never needs a copy; under `any` a plan that
    /// agrees with every stored order needs none either.
    #[test]
    fn stored_and_agreeing_plans_need_no_copies() {
        let query: JoinQuery = "Q(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).".parse().unwrap();
        let database = rels(vec![("edge", 2, mutual_edges())]);
        assert!(database
            .required_indexes(&query, &Planner::stored(LexicographicOptimiser))
            .unwrap()
            .is_empty());
        assert!(database
            .required_indexes(&query, &any(LexicographicOptimiser))
            .unwrap()
            .is_empty());
    }

    /// The planner is handed the rewritten query *before* orientation:
    /// base names and base statistics, never an `Index_*`.
    #[test]
    fn the_planner_sees_base_names_under_any() {
        let query: JoinQuery = "Q(X, Y) :- edge(X, Y), edge(Y, X).".parse().unwrap();
        let tuples = BTreeMap::from([("edge", mutual_edges())]);
        let mut database = Database::new::<SortedFamily>(
            rel_map(vec![("edge", 2, mutual_edges())]),
            StatisticsLevel::ColumnDistinct,
        );
        let spy = StatsSpy::default();
        let planner = any(spy.clone());
        add_copies(&mut database, &query, &planner, &tuples);
        lftj_join::<TreeTrie, LeapfrogTriejoin>(&database, query, &planner).unwrap();
        let (planned, stats) = spy.0.borrow_mut().take().expect("the optimiser planned");
        assert!(planned.body.iter().all(|atom| atom.name == "edge"), "{planned}");
        assert_eq!(stats.column_orders(), ColumnOrderPolicy::Any);
        assert_eq!(stats.tuples("edge"), Some(5));
        assert_eq!(stats.tuples("Index_1_0_edge"), None);
    }

    /// `r(X, X, Y), s(Y, X)`: the selection on `r` and the reversed `s`
    /// are both reoriented, and the equality follows its columns.
    #[test]
    fn a_selection_on_a_reoriented_atom_keeps_its_equality() {
        let r = vec![vec![1, 1, 5], vec![1, 2, 5], vec![2, 2, 6], vec![3, 3, 7]];
        let s = vec![vec![5, 1], vec![6, 2], vec![7, 9]];
        let query: JoinQuery = "Q(X, Y) :- r(X, X, Y), s(Y, X).".parse().unwrap();
        let tuples = BTreeMap::from([("r", r.clone()), ("s", s.clone())]);
        let want = vec![vec![1, 5], vec![2, 6]];

        let mut sorted = rels(vec![("r", 3, r.clone()), ("s", 2, s.clone())]);
        assert!(matches!(
            lftj_join::<TreeTrie, LeapfrogTriejoin>(
                &sorted,
                query.clone(),
                &Planner::stored(LexicographicOptimiser)
            ),
            Err(JoinError::CyclicAttributeOrder { .. })
        ));
        let planner = any(LexicographicOptimiser);
        let specs = add_copies(&mut sorted, &query, &planner, &tuples);
        assert_eq!(specs, vec![
            IndexSpec::new("r", vec![0, 2, 1]),
            IndexSpec::new("s", vec![1, 0]),
        ]);
        let mut got = lftj_join::<TreeTrie, LeapfrogTriejoin>(&sorted, query.clone(), &planner).unwrap();
        got.sort();
        assert_eq!(got, want);

        let mut hashed: Database<HashTrie> = Database::from(BTreeMap::from([
            ("r".to_string(), HashTrie::from_tuples(3.into(), r)),
            ("s".to_string(), HashTrie::from_tuples(2.into(), s)),
        ]));
        add_copies(&mut hashed, &query, &planner, &tuples);
        let mut got =
            hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(&hashed, query, &planner).unwrap();
        got.sort();
        assert_eq!(got, want);
    }
```

(In `a_selection_on_a_reoriented_atom_keeps_its_equality`, canonical
indices are X = 0, Y = 1, K0 = 2 and `lexicographic` under `any` plans
X, Y, K0; `Select_0_r(X, K0, Y)` then has π = [0, 2, 1] and `s(Y, X)`
π = [1, 0], so two copies.)

- [ ] **Step 4: The families and the binary's call sites**

`execution.rs`: import `Planner` and `ColumnOrderPolicy` from
`kermit_algos` (drop `Optimiser` and `QueryOptimiser` if unused outside
tests). `TrieLftj` and `HashHtj` hold a `planner: Planner` instead of
`optimiser`:

```rust
/// Sorted family: `R` under Leapfrog Triejoin through [`lftj_join_for_each`].
pub struct TrieLftj<R: SortedTrieRelation> {
    structure: SortedTrieFamily<R>,
    planner: Planner,
}

impl<R: SortedTrieRelation> TrieLftj<R> {
    /// Creates the family building every relation by `build`, planned by
    /// `planner`.
    pub fn new(build: R::BuildMode, planner: Planner) -> Self {
        Self {
            structure: SortedTrieFamily::new(build),
            planner,
        }
    }
}
```

and in its `ExecutionFamily` impl `self.planner.required_statistics()`
replaces `self.optimiser.required_statistics()`, and
`lftj_join_for_each::<R, LeapfrogTriejoin>(engine, query, &self.planner, emit)`.
`HashHtj` likewise (`new(config: HashTrieConfig, planner: Planner)`).

Callers, this task only (Task 8 replaces them with `PlannerArgs`):
- `kermit/src/bench/run.rs` `dispatch_run_bench`: `let optimiser =
  settings.optimiser;` becomes `let planner = || Planner::new(settings.optimiser.instantiate(), ColumnOrderPolicy::Stored);`
  and each `::new(…, optimiser)` becomes `::new(…, planner())`.
  `workload.validate()?` becomes `workload.validate(ColumnOrderPolicy::Stored)?`.
- `kermit/src/bench/workload.rs`: `pub fn validate(&self, column_orders: ColumnOrderPolicy)`
  passing it to `validate_query(&q.query, headers.as_slice(), column_orders)`.
- `kermit/src/main.rs` `load_query_runner`: the same closure; and
  `validate_query_files`: `validate_query(query, headers.as_slice(), ColumnOrderPolicy::Stored)`.

`execution.rs` tests: every `Optimiser::Lexicographic` argument becomes
`Planner::stored(LexicographicOptimiser)`; in
`engines_gather_the_statistics_their_optimiser_reads` the loop body uses
`Planner::new(optimiser.instantiate(), ColumnOrderPolicy::Stored)` for
each of the three families (one planner per family, since `Planner` is
not `Clone`). Import `LexicographicOptimiser` in the test module.

- [ ] **Step 5: The test files**

`kermit/tests/common/utils.rs`: `JoinEntry::join` / `count` take
`planner: &Planner` (import `Planner`; `QueryOptimiser` stays for
`test_join`'s bound); every impl passes `planner` through. In
`test_join`, replace the two lines that create and use the optimiser:

```rust
    let planner = Planner::stored(O::default());
    let database = JA::database(relations, planner.required_statistics());
```

and the two calls pass `&planner`. (Task 11 extends this for `any`.)

`lubm_mini_oracle.rs`: `cardinality_mismatches` builds `let planner =
Planner::new(optimiser.instantiate(), ColumnOrderPolicy::Stored);`, uses
`planner.required_statistics()` and passes `&planner` to `lftj_join`.
`lubm_cardinalities.rs`: the `optimisers` vec holds `Planner`s:

```rust
    let optimisers: Vec<(&str, Planner)> = vec![
        ("lexicographic", Planner::stored(LexicographicOptimiser)),
        ("cardinality", Planner::stored(CardinalityOptimiser)),
        ("cost-based", Planner::stored(CostBasedOptimiser::default())),
    ];
```

and `cardinality_mismatches(…, optimiser_name: &str, planner: &Planner, …)`.
`result_allocation.rs`: `&LexicographicOptimiser` becomes `&planner` with
`let planner = Planner::stored(LexicographicOptimiser);` created **before**
`allocation_counter::measure` (the box is not the join's allocation).
`watdiv_correctness.rs` and `subject_position_constant.rs`:
`let planner = Planner::new(optimiser.instantiate(), ColumnOrderPolicy::Stored);`
in place of `optimiser.instantiate()`, passing `&planner`.

- [ ] **Step 6: Build, test, format, commit**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit 2>&1 | tee $SCRATCH/task6.log | grep -E '^test result|FAILED|panicked'
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets 2>&1 | grep -E '^(warning|error)' | sort | uniq -c
nix develop $WT --command cargo fmt --all
git -C $WT add kermit/src/db.rs kermit/src/db/validation.rs kermit/src/execution.rs kermit/src/bench/run.rs kermit/src/bench/workload.rs kermit/src/main.rs kermit/tests/common/utils.rs kermit/tests/lubm_mini_oracle.rs kermit/tests/lubm_cardinalities.rs kermit/tests/result_allocation.rs kermit/tests/watdiv_correctness.rs kermit/tests/subject_position_constant.rs
git -C $WT commit -q -F - <<'MSG'
refactor(db): the entry points plan under a Planner and run over copies (#93)

`lftj_join*` / `hash_join*` take a `Planner`; `validate_query` takes the
column-order policy and checks `CyclicAttributeOrder` under `stored`
only. The shared body orients the planned query and resolves `Index_*`
names in the catalog's copy store, failing with `MissingIndex` when a
copy was not built; `Database::required_indexes` runs the same pipeline
to say which copies a query needs. `Index_` is reserved. Every caller
still plans under `stored`, so nothing observable changes.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
```

Expected: every `kermit` test passes (the lib gains 6 in `db.rs` and 2 in
`validation.rs`); clippy prints nothing.

- [ ] **Step 7: Mutation checks**

1. In `prepare`, remove the `if column_orders == ColumnOrderPolicy::Stored`
   guard (run the cyclic check always). Expected to fail:
   `under_any_opposite_column_orders_are_accepted`,
   `under_any_a_cyclic_query_runs_over_a_copy`. Revert.
2. In `run_join`'s selection loop, replace `lookup(&spec.relation)?` with
   `database.get(&spec.relation).expect("base")` — the bug of resolving a
   view's relation among base relations only. Expected to fail:
   `a_selection_on_a_reoriented_atom_keeps_its_equality` (panics on the
   `Index_0_2_1_r` lookup). Revert.

---

## Controller checkpoint 1 (after Task 6)

Report to the user: the planner bundle, pinning, the set-based estimate,
`orient`, the copy store and the entry points are in; every existing test
passes under `stored`; the CLI does not yet expose the policy. Confirm
the test counts against `$SCRATCH/baseline.txt` (expected: baseline +
about 35) and that `git -C $WT log --oneline` shows six `(#93)` commits
after the spec. Ask whether to continue into P3.

---

# P3 — binary (`kermit`)

## Task 7 [P3]: `ExecutionFamily` builds, holds and drops copies

**Files:**
- Modify: `kermit/src/execution.rs` (`ExecutionFamily` trait 455–500; the two impls 649–680 and 727–758; tests)

- [ ] **Step 1: Failing test**

Add to `execution.rs`'s test module (it already has the `Spy` relation
whose `build_with` records `BUILT_WITH`, and `HalfFull`-style config
through `HashTrieConfig`):

```rust
    /// A copy is built through `build_relation`, so it carries the
    /// family's build mode (seen by the spy) and config (seen on the
    /// copy), and the engine holds it until cleared.
    #[test]
    fn families_build_copies_through_build_relation() {
        let header = RelationHeader::new_positional("edge", 2);
        let edges = vec![vec![1, 2], vec![1, 3], vec![2, 3]];
        let spec = || IndexSpec::new("edge", vec![1, 0]);

        let join = TrieLftj::<Spy>::new(
            ColumnTrieBuildMode::Incremental,
            Planner::stored(LexicographicOptimiser),
        );
        let mut engine = join.build(vec![join.build_relation(header.clone(), edges.clone())]);
        BUILT_WITH.take();
        join.add_index(&mut engine, spec(), &header, &edges);
        assert_eq!(BUILT_WITH.take(), Some(ColumnTrieBuildMode::Incremental));
        let held: Vec<&IndexSpec> = TrieLftj::<Spy>::indexes(&engine)
            .into_iter()
            .map(|(spec, _)| spec)
            .collect();
        assert_eq!(held, vec![&spec()]);
        assert_eq!(engine.index("Index_1_0_edge").unwrap().header().arity(), 2);
        TrieLftj::<Spy>::clear_indexes(&mut engine);
        assert!(TrieLftj::<Spy>::indexes(&engine).is_empty());

        let config = HashTrieConfig {
            load_factor: LoadFactor::percent(50).unwrap(),
        };
        let hash = HashHtj::<SipHashStrategy, NoPruning>::new(
            config,
            Planner::stored(LexicographicOptimiser),
        );
        let mut engine = hash.build(vec![hash.build_relation(header.clone(), edges.clone())]);
        hash.add_index(&mut engine, spec(), &header, &edges);
        let (_, copy) = HashHtj::<SipHashStrategy, NoPruning>::indexes(&engine)[0];
        assert_eq!(copy.config(), config);
        assert_eq!(copy.header().name(), "Index_1_0_edge");
    }

    /// `required_indexes` goes through the engine's planner: the cyclic
    /// query needs one copy under `any` and is rejected under `stored`.
    #[test]
    fn required_indexes_follow_the_familys_planner() {
        let header = || RelationHeader::new_positional("edge", 2);
        let edges = || vec![vec![1, 2], vec![2, 1]];
        let query: JoinQuery = "Q(X, Y) :- edge(X, Y), edge(Y, X).".parse().unwrap();

        let stored = TrieLftj::<TreeTrie>::new((), Planner::stored(LexicographicOptimiser));
        let engine = stored.build(vec![stored.build_relation(header(), edges())]);
        assert!(matches!(
            stored.required_indexes(&engine, &query),
            Err(JoinError::CyclicAttributeOrder { .. })
        ));

        let any = TrieLftj::<TreeTrie>::new(
            (),
            Planner::new(Box::new(LexicographicOptimiser), ColumnOrderPolicy::Any),
        );
        let mut engine = any.build(vec![any.build_relation(header(), edges())]);
        let specs = any.required_indexes(&engine, &query).unwrap();
        assert_eq!(specs, vec![IndexSpec::new("edge", vec![1, 0])]);
        for spec in specs {
            any.add_index(&mut engine, spec, &header(), &edges());
        }
        assert_eq!(any.count(&engine, query.clone()).unwrap(), 2);
        assert!(any.required_indexes(&engine, &query).unwrap().is_empty());
    }
```

(imports: `kermit_algos::{IndexSpec, LexicographicOptimiser, Planner, ColumnOrderPolicy, JoinQuery}`,
`kermit_ds::{ConfigurableRelation, LoadFactor}`.)

- [ ] **Step 2: The trait and both impls**

Add to `ExecutionFamily` (after `count`):

```rust
    /// The reordered copies `query` needs under this family's planner
    /// that `engine` does not hold yet
    /// ([`Database::required_indexes`]). Empty under
    /// `--column-orders stored`.
    ///
    /// # Errors
    ///
    /// As [`join_for_each`](Self::join_for_each).
    fn required_indexes(
        &self, engine: &Self::Engine, query: &JoinQuery,
    ) -> Result<Vec<IndexSpec>, JoinError>;

    /// Builds `spec`'s copy from its base relation's header and file-order
    /// `tuples` through [`build_relation`](RelationFamily::build_relation),
    /// so the copy carries the configuration and build mode the report's
    /// axes name, and adds it to `engine`.
    fn add_index(
        &self, engine: &mut Self::Engine, spec: IndexSpec, base: &RelationHeader,
        tuples: &[Vec<usize>],
    );

    /// Drops every copy `engine` holds.
    fn clear_indexes(engine: &mut Self::Engine);

    /// Every copy `engine` holds, with its spec: the `space/Index_*`
    /// functions.
    fn indexes(engine: &Self::Engine) -> Vec<(&IndexSpec, &Self::Rel)>;
```

Both impls (`TrieLftj<R>` and `HashHtj<H, P>`) are the same four
methods over `Database`:

```rust
    fn required_indexes(
        &self, engine: &Self::Engine, query: &JoinQuery,
    ) -> Result<Vec<IndexSpec>, JoinError> {
        engine.required_indexes(query, &self.planner)
    }

    fn add_index(
        &self, engine: &mut Self::Engine, spec: IndexSpec, base: &RelationHeader,
        tuples: &[Vec<usize>],
    ) {
        let copy = self.build_relation(index_header(&spec, base), spec.permute_all(tuples));
        engine.add_index(spec, copy);
    }

    fn clear_indexes(engine: &mut Self::Engine) { engine.clear_indexes(); }

    fn indexes(engine: &Self::Engine) -> Vec<(&IndexSpec, &Self::Rel)> {
        engine.indexes().collect()
    }
```

(import `kermit::db::index_header` and `kermit_algos::IndexSpec`.)

- [ ] **Step 3: Run, format, commit**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit execution 2>&1 | grep -E '^test result|FAILED|panicked'
nix develop $WT --command cargo fmt --all
git -C $WT add kermit/src/execution.rs
git -C $WT commit -q -F - <<'MSG'
feat(bench): families build, hold and drop a query's copies (#93)

`ExecutionFamily::add_index` builds a copy through `build_relation`, so
it carries the config and build mode the report names; `required_indexes`,
`clear_indexes` and `indexes` wrap the catalog's store.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
```

- [ ] **Step 4: Mutation check**

Mutant: in `TrieLftj::add_index`, build with
`R::from_tuples(index_header(&spec, base), spec.permute_all(tuples))`
instead of `self.build_relation(…)`. Expected to fail:
`families_build_copies_through_build_relation` (`BUILT_WITH` stays
`None`, since the spy's `from_tuples` is `ColumnTrie`'s own). Revert.

---

## Task 8 [P3]: `--column-orders` on `join`, `bench join` and `bench run`; `kermit join` under `any`

**Files:**
- Modify: `kermit/src/options.rs` (add `PlannerArgs`)
- Modify: `kermit/src/main.rs` (`QueryArgs` 75–106; `BenchSubcommand::Run` 303–306; `JoinRunner` 583–610; `validate_query_files` 616–624; `load_query_runner` 648–676; `run_join` 757–776; `run_bench_join` 855–887; `run_bench_run_command` 931–957; the `BenchSubcommand::Run` arm 1203–1232; `write_join` 564–571)
- Modify: `kermit/src/bench/run.rs` (`RunSettings` 39–57; `dispatch_run_bench` 337–367)
- Create: `kermit/tests/cli_column_orders.rs`
- Modify: `kermit/tests/cli_query_errors.rs:257-266`, `kermit/tests/cli_bench_join_axes.rs:32`

- [ ] **Step 1: Failing CLI tests**

Create `kermit/tests/cli_column_orders.rs`:

```rust
//! CLI smoke tests for `--column-orders` (issue #93): the flag lands in the
//! bench-report `column_orders` axis, defaulting to `stored`, and `kermit
//! join --column-orders any` answers a query that `stored` rejects, with
//! identical rows on every cell.

mod common;

use {
    common::cli::{axes_of, bench_join, bench_run, kermit_bin},
    std::{fs, path::Path, process::Command},
    tempfile::TempDir,
};

#[test]
fn bench_join_records_stored_by_default() {
    let (output, report) = bench_join("tree-trie", "leapfrog-triejoin", &["-m", "space"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let axes = axes_of(&report);
    assert_eq!(axes["column_orders"], "stored");
    assert_eq!(axes["optimiser"], "lexicographic");
}

#[test]
fn bench_join_with_any_records_the_axis() {
    let (output, report) = bench_join("hash-trie", "hash-triejoin", &[
        "-m",
        "space",
        "--column-orders",
        "any",
    ]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(axes_of(&report)["column_orders"], "any");
}

#[test]
fn bench_run_with_any_records_the_axis() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "column-trie",
        "-a",
        "leapfrog-triejoin",
        "-m",
        "space",
        "--column-orders",
        "any",
    ]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let axes = axes_of(&report);
    assert_eq!(axes["column_orders"], "any");
    assert_eq!(axes["data_structure"], "ColumnTrie");
}

/// The six (structure, algorithm, Layout) cells `kermit join` can run.
const CELLS: [&[&str]; 6] = [
    &["-i", "tree-trie", "-a", "leapfrog-triejoin"],
    &["-i", "column-trie", "-a", "leapfrog-triejoin"],
    &["-i", "hash-trie", "-a", "hash-triejoin", "--ds-layout-hasher", "sip", "--ds-layout-pruning", "off"],
    &["-i", "hash-trie", "-a", "hash-triejoin", "--ds-layout-hasher", "sip", "--ds-layout-pruning", "on"],
    &["-i", "hash-trie", "-a", "hash-triejoin", "--ds-layout-hasher", "fxhash", "--ds-layout-pruning", "off"],
    &["-i", "hash-trie", "-a", "hash-triejoin", "--ds-layout-hasher", "fxhash", "--ds-layout-pruning", "on"],
];

/// 1 ↔ 2, 2 → 3.
fn write_edges(dir: &Path) -> std::path::PathBuf {
    let edge = dir.join("edge.csv");
    fs::write(&edge, "src,dst\n1,2\n2,1\n2,3\n").unwrap();
    edge
}

/// `kermit join` on `query` under `--column-orders any`, once per cell:
/// the sorted rows of each cell's CSV (after the header line).
fn rows_on_every_cell(query: &str) -> Vec<Vec<String>> {
    let dir = TempDir::new().unwrap();
    let edge = write_edges(dir.path());
    let query_path = dir.path().join("q.dl");
    fs::write(&query_path, query).unwrap();
    CELLS
        .iter()
        .map(|cell| {
            let output = Command::new(kermit_bin())
                .env("RUST_BACKTRACE", "0")
                .args(["join", "-r"])
                .arg(&edge)
                .arg("-q")
                .arg(&query_path)
                .args(*cell)
                .args(["--column-orders", "any"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{query} on {cell:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut lines: Vec<String> = stdout.lines().skip(1).map(str::to_string).collect();
            lines.sort();
            lines
        })
        .collect()
}

#[test]
fn join_answers_a_cyclic_query_under_any_on_every_cell() {
    let rows = rows_on_every_cell("Q(X, Y) :- edge(X, Y), edge(Y, X).");
    for (cell, got) in CELLS.iter().zip(&rows) {
        assert_eq!(got, &["1,2".to_string(), "2,1".to_string()], "{cell:?}");
    }
}

#[test]
fn join_under_any_answers_an_agreeing_query_like_stored() {
    let rows = rows_on_every_cell("Q(X, Z) :- edge(X, Y), edge(Y, Z).");
    for (cell, got) in CELLS.iter().zip(&rows) {
        assert_eq!(got, &["1,1".to_string(), "1,3".to_string(), "2,2".to_string()], "{cell:?}");
    }
}
```

In `cli_query_errors.rs`, extend `cyclic_attribute_order_is_a_reported_limitation`:

```rust
    assert!(stderr.contains("limitation"), "{stderr}");
    assert!(stderr.contains("--column-orders any"), "{stderr}");
```

In `cli_bench_join_axes.rs` after the `optimiser` assertion:

```rust
    assert_eq!(axes["column_orders"], "stored");
```

Run `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit
--test cli_column_orders` — expected: clap rejects `--column-orders`.

- [ ] **Step 2: `PlannerArgs`**

In `kermit/src/options.rs` (which already derives `clap::Args` groups),
add:

```rust
/// How a join is planned: the optimiser and the column-order policy.
/// Flattened into `join`, `bench join` and `bench run`; `bench ds` joins
/// nothing and has neither flag.
#[derive(clap::Args, Copy, Clone, Debug)]
pub struct PlannerArgs {
    /// Query optimiser (plans the join's variable ordering). Long-only:
    /// `-o` belongs to `--output`.
    #[arg(long, value_enum, default_value_t = Optimiser::Lexicographic)]
    pub optimiser: Optimiser,

    /// Column orders the planner may bind an atom's columns in: `stored`
    /// reads each relation in its stored column order, so a plan binds
    /// every atom's columns left to right (today's behaviour); `any`
    /// lets the planner choose, and an atom whose plan disagrees with
    /// the stored order runs over a per-query copy with the columns
    /// permuted, built before the timed join (`copies`) and dropped
    /// after the query.
    #[arg(long, value_enum, default_value_t = ColumnOrderPolicy::Stored)]
    pub column_orders: ColumnOrderPolicy,
}

impl PlannerArgs {
    /// The planner these flags select.
    pub fn instantiate(self) -> Planner {
        Planner::new(self.optimiser.instantiate(), self.column_orders)
    }
}
```

(import `kermit_algos::{ColumnOrderPolicy, Optimiser, Planner}`). Add a
unit test beside the module's existing ones:

```rust
    #[test]
    fn planner_args_default_to_lexicographic_stored() {
        #[derive(clap::Parser)]
        struct Cli {
            #[command(flatten)]
            planner: PlannerArgs,
        }
        let cli = Cli::parse_from(["kermit"]);
        assert_eq!(cli.planner.optimiser, Optimiser::Lexicographic);
        assert_eq!(cli.planner.column_orders, ColumnOrderPolicy::Stored);
        let planner = cli.planner.instantiate();
        assert_eq!(planner.column_orders(), ColumnOrderPolicy::Stored);
        let cli = Cli::parse_from(["kermit", "--column-orders", "any", "--optimiser", "cost-based"]);
        assert_eq!(cli.planner.instantiate().column_orders(), ColumnOrderPolicy::Any);
        assert_eq!(cli.planner.optimiser, Optimiser::CostBased);
    }
```

- [ ] **Step 3: Wire `main.rs`**

`QueryArgs`: replace the `optimiser` field with

```rust
    #[command(flatten)]
    planner: PlannerArgs,
```

`BenchSubcommand::Run`: replace its `optimiser` field the same way
(`#[command(flatten)] planner: PlannerArgs,`), and in the `Run` match arm
and `run_bench_run_command`'s signature rename `optimiser: Optimiser` to
`planner: PlannerArgs`. `RunSettings` (run.rs) replaces `pub optimiser:
Optimiser` with `pub planner: PlannerArgs` (import it from
`crate::options`); both `RunSettings { … }` literals in `main.rs` set
`planner: query_args.planner` / `planner`. `dispatch_run_bench`:

```rust
    let planner = settings.planner;
    match cell {
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &TrieLftj::<kermit_ds::TreeTrie<S>>::new((), planner.instantiate()),
            …
```

(the three arms). In `run_benchmark`, `workload.validate(ColumnOrderPolicy::Stored)?`
becomes `workload.validate(settings.planner.column_orders)?`, so a cyclic
workload passes validation under `any`; the copies themselves arrive in
Task 9 (until then such a run fails inside `--verify` or timing with
`MissingIndex`, which Task 9, in the same package, removes).

`validate_query_files`: `validate_query(query, headers.as_slice(),
args.planner.column_orders)`. `load_query_runner`: `let planner = ||
args.planner.instantiate();` and each arm passes `planner()` plus the
policy to `build_join_runner`, e.g.

```rust
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
        }) => with_sorted_trie_layout!(seek, |S| build_join_runner(
            TrieLftj::<kermit_ds::TreeTrie<S>>::new((), planner()),
            cell,
            &args.relations,
            args.planner.column_orders,
        )),
```

The runner itself:

```rust
/// A built engine behind a closure: runs one query, passing each result
/// tuple to the sink as the join produces it. `FnMut`, because under
/// `--column-orders any` a run builds the query's copies into the engine
/// first and drops them after.
type JoinRunner = Box<dyn FnMut(JoinQuery, &mut dyn FnMut(&[usize])) -> Result<(), JoinError>>;

/// Loads `args.relations` into `family`'s engine and returns a runner over it.
/// `cell` is the cell the caller resolved, which `family` must implement.
/// Under `any` the file-order tuples are kept, so a query's copies can be
/// built from them (`RelationFamily::load_with_tuples`); under `stored`
/// nothing is kept.
fn build_join_runner<F: ExecutionFamily + 'static>(
    family: F, cell: Execution, paths: &[PathBuf], column_orders: ColumnOrderPolicy,
) -> anyhow::Result<JoinRunner> {
    debug_assert_eq!(
        family.execution(),
        cell,
        "dispatch built a different cell than the one resolved"
    );
    let mut relations = Vec::with_capacity(paths.len());
    let mut inputs: BTreeMap<String, (RelationHeader, Vec<Vec<usize>>)> = BTreeMap::new();
    for path in paths {
        if column_orders == ColumnOrderPolicy::Any {
            let (relation, tuples) = family.load_with_tuples(path)?;
            let header = relation.header().clone();
            inputs.insert(header.name().to_string(), (header, tuples));
            relations.push(relation);
        } else {
            relations.push(family.load(path)?);
        }
    }
    let mut engine = family.build(relations);
    Ok(Box::new(move |q, sink| {
        for spec in family.required_indexes(&engine, &q)? {
            let (header, tuples) = inputs
                .get(&spec.base)
                .expect("validation checked that every base relation was loaded");
            family.add_index(&mut engine, spec, header, tuples);
        }
        let result = family.join_for_each(&engine, q, sink);
        F::clear_indexes(&mut engine);
        result
    }))
}
```

(imports: `kermit_algos::ColumnOrderPolicy`, `kermit_ds::{Relation, RelationHeader}`,
`std::collections::BTreeMap`.) `write_join` takes `join: &mut JoinRunner`
and calls `join(query, &mut |tuple| …)`; `run_join` and `run_bench_join`
bind `let mut join = load_query_runner(…)?;` and pass `&mut join`.

- [ ] **Step 4: Run, format, commit**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_column_orders --test cli_query_errors --test cli_bench_join_axes --test cli_optimiser_choice 2>&1 | grep -E '^test result|FAILED|panicked'
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit 2>&1 | grep -E '^test result|FAILED|panicked'
nix develop $WT --command cargo fmt --all
git -C $WT add kermit/src/options.rs kermit/src/main.rs kermit/src/bench/run.rs kermit/tests/cli_column_orders.rs kermit/tests/cli_query_errors.rs kermit/tests/cli_bench_join_axes.rs
git -C $WT commit -q -F - <<'MSG'
feat(cli): --column-orders stored|any (#93)

`PlannerArgs` flattens `--optimiser` and `--column-orders` into `join`,
`bench join` and `bench run`. `kermit join` under `any` keeps each
relation's file-order tuples and builds a query's copies before the
join, dropping them after; the cyclic query answers on every cell.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
```

Expected: `bench_run_with_any_records_the_axis` passes (triangle needs
no copies under `lexicographic`), the two `join` tests pass on all six
cells, `cli_query_errors` and `cli_optimiser_choice` still pass.

---

## Task 9 [P3]: `bench run` under `any`: `copies`, `space/Index_*`, the `index:` line, the axis

**Files:**
- Modify: `kermit/src/bench/run.rs` (`run_benchmark` 74–332)
- Modify: `kermit/tests/cli_column_orders.rs` (add the `bench run` tests)

- [ ] **Step 1: Failing CLI tests**

Append to `cli_column_orders.rs`:

```rust
/// A workspace with `edge.csv` (1 ↔ 2, 2 → 3) and one benchmark whose
/// query is cyclic under `stored`.
fn mutual_workspace() -> (TempDir, TempDir) {
    let workspace = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let data = workspace.path().join("data");
    fs::create_dir_all(&data).unwrap();
    write_edges(&data);
    let benchmarks = workspace.path().join("benchmarks");
    fs::create_dir_all(&benchmarks).unwrap();
    fs::write(
        benchmarks.join("mutual.yml"),
        r#"name: mutual
description: "mutual edges: cyclic under stored, one copy under any"
relations:
  - name: edge
    path: "data/edge.csv"
queries:
  - name: mutual
    description: "edges present in both directions"
    query: "Q(X, Y) :- edge(X, Y), edge(Y, X)."
    expected: 2
"#,
    )
    .unwrap();
    (workspace, cache)
}

/// `bench run mutual` in `workspace`, returning the process output and
/// the parsed report array.
fn bench_run_mutual(
    workspace: &TempDir, cache: &TempDir, args: &[&str],
) -> (std::process::Output, Vec<serde_json::Value>) {
    let report = workspace.path().join("report.json");
    let output = Command::new(kermit_bin())
        .env("RUST_BACKTRACE", "0")
        .env("KERMIT_WORKSPACE", workspace.path())
        .env("XDG_CACHE_HOME", cache.path())
        .current_dir(workspace.path())
        .args(["bench", "--sample-size", "10", "--measurement-time", "1", "--warm-up-time", "1"])
        .args(["--report-json"])
        .arg(&report)
        .args(["run", "mutual"])
        .args(args)
        .output()
        .unwrap();
    let reports = fs::read_to_string(&report)
        .ok()
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap_or_default();
    (output, reports)
}

fn functions_of(report: &serde_json::Value) -> Vec<String> {
    report["criterion_groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["function"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn bench_run_under_any_times_the_copies_and_measures_their_space() {
    let (workspace, cache) = mutual_workspace();
    let (output, reports) = bench_run_mutual(&workspace, &cache, &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "--column-orders",
        "any",
        "--verify",
        "-m",
        "insertion",
        "iteration",
        "space",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(reports.len(), 1, "{reports:?}");
    let functions = functions_of(&reports[0]);
    for function in ["insertion", "copies", "iteration", "space/edge", "space/Index_1_0_edge"] {
        assert!(functions.contains(&function.to_string()), "{function} missing from {functions:?}");
    }
    assert_eq!(reports[0]["axes"]["column_orders"], "any");
    assert_eq!(reports[0]["axes"]["verified"], true);
    let metadata = reports[0]["metadata"].as_array().unwrap();
    assert!(
        metadata.iter().any(|m| m["label"] == "index" && m["value"] == "edge (1, 0)"),
        "{metadata:?}"
    );
    assert!(stderr.contains("index"), "{stderr}");
}

#[test]
fn bench_run_under_stored_still_rejects_the_cyclic_query() {
    let (workspace, cache) = mutual_workspace();
    let (output, reports) = bench_run_mutual(&workspace, &cache, &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "-m",
        "iteration",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("contradictory orders"), "{stderr}");
    assert!(reports.is_empty(), "{reports:?}");
}

#[test]
fn bench_run_under_any_emits_no_copies_when_the_plan_agrees() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "-m",
        "insertion",
        "space",
        "--column-orders",
        "any",
    ]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let reports = common::cli::reports_of(&report);
    let functions = functions_of(&reports[0]);
    assert!(!functions.iter().any(|f| f == "copies"), "{functions:?}");
    assert!(!functions.iter().any(|f| f.starts_with("space/Index_")), "{functions:?}");
    assert_eq!(reports[0]["axes"]["column_orders"], "any");
}
```

(Check the `metadata` entry field names against `ReportField` in
`bench_report.rs` — they serialise as `label` / `value`; adjust the
assertion if the names differ.) Run the `--test cli_column_orders`
binary — expected: the first test fails (no `copies`, and `--verify`
fails with `MissingIndex`).

- [ ] **Step 2: `run_benchmark` per query under `any`**

Rewrite the body of `run_benchmark` from the `rebuilds` line to the end
of the per-query loop as follows (unchanged parts are elided with `…`
but keep them):

```rust
    let RunSettings {
        kind,
        prefix,
        planner,
        metrics,
        queries_per_build,
        verify,
        bench_args,
    } = settings;
    let column_orders = planner.column_orders;
    workload.validate(column_orders)?;
    // … `insertion` and `end_to_end` rebuild from each relation's tuples
    // in file order, kept here, and under `--column-orders any` so does
    // every query's reordered copy. An `iteration`-only `stored` run
    // keeps none: it would be a dead copy of the whole workload.
    let rebuilds = metrics
        .iter()
        .any(|m| matches!(m, Metric::Insertion | Metric::EndToEnd))
        || column_orders == ColumnOrderPolicy::Any;
    let mut relations: Vec<F::Rel> = Vec::with_capacity(workload.relation_paths.len());
    let mut build_inputs: Vec<(RelationHeader, Vec<Vec<usize>>)> = Vec::new();
    for path in &workload.relation_paths {
        if rebuilds {
            let (relation, tuples) = family.load_with_tuples(path)?;
            build_inputs.push((relation.header().clone(), tuples));
            relations.push(relation);
        } else {
            relations.push(family.load(path)?);
        }
    }
    // The file-order input of `base`, for the copies.
    let input_of = |base: &str| -> &(RelationHeader, Vec<Vec<usize>>) {
        build_inputs
            .iter()
            .find(|(header, _)| header.name() == base)
            .expect("validation checked that every base relation was loaded")
    };
    let mut engine = family.build(relations);

    let execution = family.execution();
    let ds_name = execution.index_structure().axis_value();
    let algo_name = execution.algorithm().axis_value();
    let has_time_metrics = …;

    // Everything read from the base relations is taken once, here, so the
    // per-query loop can mutate the engine (adding and dropping copies)
    // without a borrow outliving it.
    let (total_tuples, optimization_axes, relation_lines) = {
        let relations = F::relations(&engine);
        let total_tuples: usize = relations.iter().map(|r| F::tuple_count(r)).sum();
        let mut optimization_axes = relations
            .first()
            .map(|r| F::optimization_axes(r))
            .unwrap_or_default();
        optimization_axes.extend(family.build_mode_axes());
        let relation_lines: Vec<String> = relations
            .iter()
            .map(|rel| {
                let h = rel.header();
                format!("{:?} (arity {})", h.name(), h.arity())
            })
            .collect();
        (total_tuples, optimization_axes, relation_lines)
    };

    let mut reports: Vec<BenchReport> = Vec::with_capacity(workload.queries.len());

    for query_def in &workload.queries {
        // The copies this query reads under `any` (none under `stored`):
        // built before anything is timed, from the same file-order tuples
        // `insertion` rebuilds from, through the same `build_relation`,
        // and dropped after the query, so one query's copies are held at
        // a time.
        let specs = family.required_indexes(&engine, &query_def.query)?;
        for spec in &specs {
            let (header, tuples) = input_of(&spec.base);
            family.add_index(&mut engine, spec.clone(), header, tuples);
        }

        let mut metadata = vec![ …benchmark, query, data structure, algorithm… ];
        for spec in &specs {
            metadata.push(MetadataLine::new("index", spec.describe()));
        }
        let verified = …unchanged…;
        if metrics.contains(&Metric::EndToEnd) { … }
        for line in &relation_lines {
            metadata.push(MetadataLine::new("relation", line));
        }
        write_metadata_block(&mut io::stderr(), "bench run metadata", &metadata)?;

        let group_name = …;
        let mut criterion_groups: Vec<CriterionGroupRef> = Vec::new();

        if has_time_metrics {
            let mut criterion = build_time_criterion(bench_args);
            let mut group = criterion.benchmark_group(&group_name);

            if metrics.contains(&Metric::Insertion) {
                group.bench_function("insertion", …unchanged…);
                criterion_groups.push(…"insertion"…);

                // The price of `any`: permuting and building this query's
                // copies, timed as one function like `insertion` is for the
                // base relations and through the same `build_relation`.
                // The permutation is inside the timed region, since a copy
                // cannot be built without it. Emitted only when the plan
                // needs a copy, so a `stored` report is unchanged.
                if !specs.is_empty() {
                    group.bench_function("copies", |b| {
                        b.iter_batched(
                            || specs.clone(),
                            |specs| {
                                for spec in specs {
                                    let (header, tuples) = input_of(&spec.base);
                                    let header = index_header(&spec, header);
                                    std::hint::black_box(
                                        family.build_relation(header, spec.permute_all(tuples)),
                                    );
                                }
                            },
                            criterion::BatchSize::SmallInput,
                        );
                    });
                    criterion_groups.push(CriterionGroupRef {
                        group: group_name.clone(),
                        function: "copies".to_string(),
                        metric: ReportMetric::Time,
                    });
                }
            }

            if metrics.contains(&Metric::Iteration) { …unchanged, over `&engine`… }

            if metrics.contains(&Metric::EndToEnd) {
                // … the fresh build includes this query's copies, as the
                // engine `iteration` reads does.
                group.bench_function("end_to_end", |b| {
                    b.iter_batched(
                        || (build_inputs.clone(), vec![query_def.query.clone(); queries_per_build as usize]),
                        |(inputs, queries)| {
                            let mut fresh = family.build_from_tuples(inputs);
                            for spec in specs.clone() {
                                let (header, tuples) = input_of(&spec.base);
                                family.add_index(&mut fresh, spec, header, tuples);
                            }
                            for q in queries {
                                std::hint::black_box(family.count(&fresh, q).expect(VALIDATED));
                            }
                        },
                        criterion::BatchSize::PerIteration,
                    );
                });
                criterion_groups.push(…"end_to_end"…);
            }

            group.finish();
            criterion.final_summary();
        }

        if metrics.contains(&Metric::Space) {
            let mut criterion = build_space_criterion(bench_args);
            let mut group = criterion.benchmark_group(&group_name);
            for rel in F::relations(&engine) {
                let function = format!("space/{}", rel.header().name());
                criterion_groups.push(add_space_bench(&mut group, &group_name, function, rel));
            }
            // One function per copy, beside the base relations'.
            for (spec, copy) in F::indexes(&engine) {
                let function = format!("space/{}", spec.name);
                criterion_groups.push(add_space_bench(&mut group, &group_name, function, copy));
            }
            group.finish();
            criterion.final_summary();
        }

        let mut axes = BTreeMap::from([
            …benchmark, query, data_structure, algorithm…
            ("optimiser".to_string(), serde_json::json!(planner.optimiser.axis_value())),
            ("column_orders".to_string(), serde_json::json!(column_orders.axis_value())),
            ("tuples".to_string(), serde_json::json!(total_tuples)),
        ]);
        …unchanged…
        reports.push(BenchReport::new(kind, &metadata, axes, criterion_groups));

        F::clear_indexes(&mut engine);
    }
```

(imports: `kermit::db::index_header`, `kermit_algos::ColumnOrderPolicy`,
`kermit_ds::RelationHeader`.) The `verify` block reads `&engine` after
the copies are added, so `--verify` works under `any`. Update the
`run_benchmark` doc comment with one sentence: "Under `--column-orders
any`, each query's reordered copies are built before it is verified or
timed and dropped after it (see `ExecutionFamily::add_index`)."

- [ ] **Step 3: Run, format, commit**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_column_orders --test cli_bench_run_sweep --test cli_end_to_end_metric --test cli_bench_run_verify 2>&1 | grep -E '^test result|FAILED|panicked'
nix develop $WT --command cargo fmt --all
git -C $WT add kermit/src/bench/run.rs kermit/tests/cli_column_orders.rs
git -C $WT commit -q -F - <<'MSG'
feat(bench): bench run under any builds, times and measures each query's copies (#93)

Per query: the copies the plan needs are built before verification and
timing and dropped after; `copies` times permuting and building them
beside `insertion`; `end_to_end`'s fresh build includes them; `space`
gains one `space/Index_<π>_<base>` function per copy; each copy gets
an `index:` metadata line; every report carries `column_orders`.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
```

- [ ] **Step 4: Mutation check**

Mutant: emit `copies` unconditionally (remove `if !specs.is_empty()`).
Expected to fail: `bench_run_under_any_emits_no_copies_when_the_plan_agrees`.
Revert.

---

# P4 — kermit-lab

## Task 10 [P4]: kermit-lab loads `column_orders` and `copies`; the schema doc

**Files:**
- Modify: `python/kermit-lab/kermit_lab/frame.py:31-38`, `loader.py:123-139`, `defaults.py`
- Modify: `python/kermit-lab/tests/{test_frame,test_loader,test_defaults,test_contract}.py`, `tests/conftest.py` (the `fixture_end_to_end_tree` axes)
- Modify: `docs/specs/bench-report-schema.md` (axes table 70–80, after the `CriterionGroupRef` table 55–61, change log 197+)

- [ ] **Step 1: Failing tests**

`tests/test_loader.py`:

```python
def test_phase_of_recognises_copies() -> None:
    from kermit_lab.loader import TIME_PHASES, phase_of

    # `bench run` under `--column-orders any` times a query's reordered
    # copies as one function beside `insertion` (#93).
    assert "copies" in TIME_PHASES
    assert phase_of("copies") == "copies"
    assert phase_of("space/Index_1_0_edge") is None
```

`tests/test_defaults.py` (and update `test_the_scoped_registry_is_the_only_one`
to expect `["JOIN_AXIS_DEFAULTS", "SCOPED_AXIS_DEFAULTS"]` with a docstring
noting the second registry is scoped to join rows, not structure-blind):

```python
def test_column_orders_backfills_join_rows_only() -> None:
    """Every join before #93 ran `stored`; a `bench ds` row joins nothing
    and keeps NaN."""
    df = pd.DataFrame({
        "data_structure": ["TreeTrie", "HashTrie", "ColumnTrie", "TreeTrie"],
        "algorithm": ["LeapfrogTriejoin", "HashTriejoin", pd.NA, "LeapfrogTriejoin"],
        "column_orders": [pd.NA, "any", pd.NA, pd.NA],
    })
    out = apply_axis_defaults(df)
    assert out["column_orders"].iloc[0] == "stored"
    assert out["column_orders"].iloc[1] == "any"
    assert pd.isna(out["column_orders"].iloc[2])
    assert out["column_orders"].iloc[3] == "stored"
    assert defaults.JOIN_AXIS_DEFAULTS == {"column_orders": "stored"}


def test_column_orders_backfill_needs_the_algorithm_column() -> None:
    df = pd.DataFrame({"data_structure": ["TreeTrie"], "column_orders": [pd.NA]})
    out = apply_axis_defaults(df)
    assert pd.isna(out["column_orders"].iloc[0])
```

`tests/test_frame.py`, after `test_optimiser_axis_reaches_frame`:

```python
def test_column_orders_axis_reaches_frame(fixture_end_to_end_tree) -> None:
    df = load(fixture_end_to_end_tree["paths"], fixture_end_to_end_tree["criterion_root"])
    assert "column_orders" in df.columns
    assert set(df.column_orders.dropna().unique()) == {"any"}


def test_column_orders_backfills_stored_on_legacy_join_reports(fixture_tree) -> None:
    # Reports from before #93 carry no `column_orders`; every join then ran
    # `stored`, so the loader fills it on join rows.
    df = load(fixture_tree["paths"], fixture_tree["criterion_root"])
    assert set(df.column_orders.dropna().unique()) == {"stored"}
```

In `tests/conftest.py`'s `fixture_end_to_end_tree`, add
`"column_orders": "any"` to each report's `axes` beside
`"optimiser": "lexicographic"` (read the fixture builder to find the
dict literal). `fixture_tree` must carry an `algorithm` axis on its join
rows (it does: `bench run`-shaped reports); if its rows have no
`algorithm`, the back-fill test above must use a fixture that does.

`tests/test_contract.py`:

```python
def test_bench_join_reports_its_column_orders(tmp_path: Path) -> None:
    report = tmp_path / "join.json"
    _run(
        tmp_path, report,
        "join", "--relations", str(FIXTURES / "first.csv"), str(FIXTURES / "second.csv"),
        "--query", str(FIXTURES / "intersect_query.dl"),
        "-i", "tree-trie", "-a", "leapfrog-triejoin", "-m", "space",
        "--column-orders", "any",
    )
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion")
    assert len(df) >= 1
    assert set(df["column_orders"]) == {"any"}
```

Run: `uv --directory $WT/python/kermit-lab run pytest -q` — expected:
the new tests fail (`copies` unknown, `column_orders` dropped,
`JOIN_AXIS_DEFAULTS` missing).

- [ ] **Step 2: Implement**

`frame.py`: add `"column_orders",` after `"optimiser",` in
`_AXIS_STR_KEYS`. `loader.py`:

```python
TIME_PHASES: tuple[str, ...] = ("insertion", "copies", "iteration", "end_to_end")
```

and in `phase_of`'s docstring list `copies` ("the reordered copies a
query needs under `--column-orders any`, emitted beside `insertion`
when the plan needs one"). `defaults.py`: add after `SCOPED_AXIS_DEFAULTS`:

```python
# Axis -> value to substitute for NaN on *join* rows only (rows with an
# `algorithm`): a `bench ds` row joins nothing and has no such axis.
# Unlike the scoped registry this is structure-blind by design, because
# every structure's joins ran the same policy before the axis existed.
JOIN_AXIS_DEFAULTS: dict[str, object] = {
    # Every join before #93 read relations in their stored column order.
    "column_orders": "stored",
}
```

and extend `apply_axis_defaults` (keeping its early return for a frame
without `data_structure`, but applying the join defaults before it when
`algorithm` is present):

```python
def apply_axis_defaults(df: pd.DataFrame) -> pd.DataFrame:
    """…(existing docstring)… Join-row defaults (:data:`JOIN_AXIS_DEFAULTS`)
    fill only rows whose ``algorithm`` is set."""
    out = df.copy()
    if "algorithm" in out.columns:
        joined = out["algorithm"].notna()
        for col, default in JOIN_AXIS_DEFAULTS.items():
            if col in out.columns:
                out[col] = out[col].mask(out[col].isna() & joined, default)
    if "data_structure" not in out.columns:
        return out
    …unchanged scoped loop…
```

Update the module docstring's "This registry records the *one*
exception" sentence to mention the join-row registry.

`docs/specs/bench-report-schema.md`: in the axes table, after the
`optimiser` row:

```
| `column_orders`  | `join`, `run`            | string           | Column-order policy the join was planned under (`--column-orders`). Values: `"stored"` (default; each relation read in its stored column order) and `"any"` (the planner is free; atoms whose plan disagrees with the stored order read a per-query reordered copy). Always emitted since #93; kermit-lab back-fills `"stored"` on earlier join reports. |
```

After the `CriterionGroupRef` table, add a short subsection:

```
### Time and space function ids

`bench run` / `bench join` write the time functions `insertion` (every
base relation built from its file-order tuples), `iteration` (the
streamed join over the prebuilt engine), `end_to_end` (opt-in: a fresh
build plus K joins) and, under `--column-orders any` when the plan needs
a copy, `copies` (every reordered copy the query needs, permuted and
built through the same `build_relation` as `insertion`; emitted beside
`insertion`). The space functions are `space/<relation>` per base
relation and `space/Index_<π>_<base>` per copy (e.g. `space/Index_1_0_edge`).
Each copy also gets an `index` metadata line, e.g. `edge (1, 0)`. Under
`stored` no `copies` or `space/Index_*` function is ever written.
```

Change log row:

```
| 3 (no bump) | 2026-10-05 | Added the `column_orders` conventional `axes` key (`--column-orders stored|any`, #93), the `copies` time function and `space/Index_<π>_<base>` space functions, all present only under `any`. Additive — under `stored` nothing changes — so `schema_version` stays `3`. kermit-lab back-fills `stored` on earlier join rows. |
```

- [ ] **Step 3: Run, commit**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit 2>&1 | tail -1
env KERMIT_BIN=$WT/target/debug/kermit nix develop $WT --command uv --directory $WT/python/kermit-lab run pytest -q 2>&1 | tail -3
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit schema 2>&1 | grep -E '^test result'
git -C $WT add python/kermit-lab/kermit_lab/frame.py python/kermit-lab/kermit_lab/loader.py python/kermit-lab/kermit_lab/defaults.py python/kermit-lab/tests/test_frame.py python/kermit-lab/tests/test_loader.py python/kermit-lab/tests/test_defaults.py python/kermit-lab/tests/test_contract.py python/kermit-lab/tests/conftest.py docs/specs/bench-report-schema.md
git -C $WT commit -q -F - <<'MSG'
feat(kermit-lab): the column_orders axis and the copies phase (#93)

`column_orders` is a string axis, back-filled to `stored` on join rows
that predate it; `copies` is a time phase. The schema doc records both
and the `space/Index_*` functions; no version bump, since `stored`
reports are unchanged.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
```

Expected: pytest green including the contract test; the Rust
`schema`-named tests (the Python `SCHEMA_VERSION` pin) still pass.

---

# P5 — coverage

## Task 11 [P5]: The 16 patterns under `any`, and the `any`-only patterns

**Files:**
- Modify: `kermit/tests/common/utils.rs` (`JoinEntry` users, `test_join` 131–198; add the provider and `join_under_planner`)
- Modify: `kermit/tests/common/macros.rs` (`define_multiway_join_test!` 1–28, the 16 pattern macros, `define_multiway_join_test_suite!` 428–456; add `define_multiway_join_test_suite_with_column_orders!`)
- Modify: `kermit/tests/join_tests.rs`
- Create: `kermit/tests/column_orders_any.rs`

- [ ] **Step 1: The provider and the copy-aware join helper**

Add to `utils.rs` (imports: `kermit::db::build_index`,
`kermit_algos::{ColumnOrderPolicy, Planner}`, `kermit_ds::RelationHeader`):

```rust
/// The column-order policy a macro-generated suite runs under, lifted to
/// a type so `define_multiway_join_test_suite!` can name it, as
/// `ConfigProvider` and `BuildModeProvider` lift a Config and a BuildMode.
pub trait ColumnOrderProvider {
    fn policy() -> ColumnOrderPolicy;
}

/// `--column-orders stored`: today's planning, the default.
pub struct StoredOrders;

/// `--column-orders any`: the planner is free and the join reads
/// reordered copies.
pub struct AnyOrders;

impl ColumnOrderProvider for StoredOrders {
    fn policy() -> ColumnOrderPolicy { ColumnOrderPolicy::Stored }
}

impl ColumnOrderProvider for AnyOrders {
    fn policy() -> ColumnOrderPolicy { ColumnOrderPolicy::Any }
}

/// A fixture's relations by name: the header the relation was built with
/// and its tuples in fixture (file) order, which copies are built from.
pub type Inputs = BTreeMap<String, (RelationHeader, Vec<Vec<usize>>)>;

/// Runs `query` over `database` under `planner` through `JA`'s entry
/// point the way the CLI does: builds the copies `required_indexes`
/// names from `inputs`, joins, and drops them. Returns the rows and the
/// specs it built (empty under `stored`).
pub fn join_under_planner<R: Relation + Cardinality, JA: JoinEntry<R>>(
    database: &mut Database<R>, inputs: &Inputs, query: JoinQuery, planner: &Planner,
) -> Result<(Vec<Vec<usize>>, Vec<kermit_algos::IndexSpec>), JoinError> {
    let specs = database.required_indexes(&query, planner)?;
    for spec in &specs {
        let (header, tuples) = &inputs[&spec.base];
        database.add_index(spec.clone(), build_index::<R>(spec, header, tuples));
    }
    assert!(
        database.required_indexes(&query, planner)?.is_empty(),
        "held copies must not be required again"
    );
    let rows = JA::join(database, query.clone(), planner);
    let streamed = JA::count(database, query, planner);
    database.clear_indexes();
    let rows = rows?;
    assert_eq!(streamed?, rows.len(), "streamed count disagrees with the collected rows");
    Ok((rows, specs))
}
```

Rewrite `test_join` to take the provider and go through the helper:

```rust
pub fn test_join<R, JA, O, P>(
    input: Vec<Vec<Vec<usize>>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
    result: Vec<Vec<usize>>,
) where
    R: Relation + Cardinality,
    JA: JoinEntry<R>,
    O: QueryOptimiser + Default,
    P: ColumnOrderProvider,
{
    let mut inputs: Inputs = BTreeMap::new();
    let relations: BTreeMap<String, R> = input
        .into_iter()
        .zip(&rel_variables)
        .enumerate()
        .map(|(i, (tuples, rv))| {
            assert!(
                tuples.iter().all(|t| t.len() == rv.len()),
                "fixture R{i}: every tuple must have one value per atom term"
            );
            let header = RelationHeader::new_positional(format!("R{i}"), rv.len());
            inputs.insert(format!("R{i}"), (header.clone(), tuples.clone()));
            (format!("R{i}"), R::from_tuples(header, tuples))
        })
        .collect();
    …build `query_str` and `query` as before…

    // The database is analysed to exactly what the optimiser reads, as the
    // CLI's engines are; under `any` the copies the plan needs are built
    // from the fixture's tuples before the join and dropped after, as the
    // CLI does.
    let planner = Planner::new(Box::new(O::default()), P::policy());
    let mut database = JA::database(relations, planner.required_statistics());
    let (mut actual, _) =
        join_under_planner::<R, JA>(&mut database, &inputs, query, &planner)
            .unwrap_or_else(|e| panic!("{query_str}: {e}"));
    actual.sort();
    let mut expected = result;
    expected.sort();
    assert_eq!(actual, expected);
}
```

(The streamed-count check moved into the helper. A named positional
header replaces the nameless one so the copies' base lookup works; the
join keys relations by the map's name, not the header's, so nothing else
changes.)

- [ ] **Step 2: The macros take a policy**

`define_multiway_join_test!` gains `$policy:ty` after `$optimiser:ty`
and passes it as the fourth type argument of `test_join`. Each of the 16
pattern macros (`define_unary_multiway_join_test!` … `define_middle_placeholder_multiway_join_test!`)
changes its matcher to `($relation_type:ident, $join_algorithm:ty,
$optimiser:ty, $policy:ty)` and forwards `$policy` after `$optimiser` in
its one `define_multiway_join_test!` call; the test names are unchanged
(the module a policy suite lives in keeps them unique). The suite macro
gets the policy arm first:

```rust
#[macro_export]
macro_rules! define_multiway_join_test_suite {
    (@policy $relation_type:ident, $join_algorithm:ty, $optimiser:ty, $policy:ty) => {
        $crate::define_unary_multiway_join_test!( $relation_type, $join_algorithm, $optimiser, $policy );
        $crate::define_triangle_multiway_join_test!( $relation_type, $join_algorithm, $optimiser, $policy );
        … all 16, each with `, $policy` appended …
        $crate::define_middle_placeholder_multiway_join_test!( $relation_type, $join_algorithm, $optimiser, $policy );
    };
    (
        $(
            $relation_type:ident,
            $join_algorithm:ty,
            $optimiser:ty
        ),+
    ) => {
        $(
            $crate::define_multiway_join_test_suite!(
                @policy $relation_type, $join_algorithm, $optimiser,
                $crate::common::utils::StoredOrders
            );
        )+
    };
}
```

Add the column-orders counterpart after the BuildMode one:

```rust
/// The column-order-policy counterpart of [`define_multiway_join_test_suite!`]
/// (issue #93).
///
/// Runs the standard join patterns against `Relation` under `Policy`
/// (`AnyOrders` / `StoredOrders` from `common::utils`) inside a uniquely
/// named module, delegating to [`define_multiway_join_test_suite!`] so a
/// pattern added there runs here too. Under `AnyOrders` every pattern's
/// rows must equal the `stored` rows the plain invocation checks.
///
/// ```ignore
/// define_multiway_join_test_suite_with_column_orders!(TreeTrieGalloping, LeapfrogTriejoin, CostBasedOptimiser, AnyOrders);
/// // → tests named e.g. `column_orders_treetriegalloping_leapfrogtriejoin_costbasedoptimiser_anyorders::triangle_treetriegalloping_leapfrogtriejoin_costbasedoptimiser`
/// ```
#[macro_export]
macro_rules! define_multiway_join_test_suite_with_column_orders {
    (
        $(
            $relation_type:ident,
            $join_algorithm:ident,
            $optimiser:ident,
            $policy:ident
        ),+
    ) => {
        $(
            paste::paste! {
                mod [<column_orders_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower _ $policy:lower>] {
                    use super::*;

                    $crate::define_multiway_join_test_suite!(
                        @policy $relation_type, $join_algorithm, $optimiser, $policy
                    );
                }
            }
        )+
    };
}
```

- [ ] **Step 3: Invocations**

In `join_tests.rs`, import `common::utils::AnyOrders` and
`kermit_ds::{BuiltWith, Configured}`, and append:

```rust
// ── Column orders: every alias × optimiser under `any` (issue #93) ─────
// The plain invocations above are the `stored` baseline; these run the
// same 16 patterns with the planner free and the join reading reordered
// copies, and must return the same rows.
define_multiway_join_test_suite_with_column_orders!(
    TreeTrieLinear, LeapfrogTriejoin, LexicographicOptimiser, AnyOrders,
    TreeTrieLinear, LeapfrogTriejoin, CardinalityOptimiser, AnyOrders,
    TreeTrieLinear, LeapfrogTriejoin, CostBasedOptimiser, AnyOrders,
    TreeTrieBinary, LeapfrogTriejoin, LexicographicOptimiser, AnyOrders,
    TreeTrieBinary, LeapfrogTriejoin, CardinalityOptimiser, AnyOrders,
    TreeTrieBinary, LeapfrogTriejoin, CostBasedOptimiser, AnyOrders,
    TreeTrieGalloping, LeapfrogTriejoin, LexicographicOptimiser, AnyOrders,
    TreeTrieGalloping, LeapfrogTriejoin, CardinalityOptimiser, AnyOrders,
    TreeTrieGalloping, LeapfrogTriejoin, CostBasedOptimiser, AnyOrders,
    ColumnTrieLinear, LeapfrogTriejoin, LexicographicOptimiser, AnyOrders,
    ColumnTrieLinear, LeapfrogTriejoin, CardinalityOptimiser, AnyOrders,
    ColumnTrieLinear, LeapfrogTriejoin, CostBasedOptimiser, AnyOrders,
    ColumnTrieBinary, LeapfrogTriejoin, LexicographicOptimiser, AnyOrders,
    ColumnTrieBinary, LeapfrogTriejoin, CardinalityOptimiser, AnyOrders,
    ColumnTrieBinary, LeapfrogTriejoin, CostBasedOptimiser, AnyOrders,
    ColumnTrieGalloping, LeapfrogTriejoin, LexicographicOptimiser, AnyOrders,
    ColumnTrieGalloping, LeapfrogTriejoin, CardinalityOptimiser, AnyOrders,
    ColumnTrieGalloping, LeapfrogTriejoin, CostBasedOptimiser, AnyOrders,
    HashTrieSip, HashTriejoin, LexicographicOptimiser, AnyOrders,
    HashTrieSip, HashTriejoin, CardinalityOptimiser, AnyOrders,
    HashTrieSip, HashTriejoin, CostBasedOptimiser, AnyOrders,
    HashTrieFx, HashTriejoin, LexicographicOptimiser, AnyOrders,
    HashTrieFx, HashTriejoin, CardinalityOptimiser, AnyOrders,
    HashTrieFx, HashTriejoin, CostBasedOptimiser, AnyOrders,
    HashTrieSipPruned, HashTriejoin, LexicographicOptimiser, AnyOrders,
    HashTrieSipPruned, HashTriejoin, CardinalityOptimiser, AnyOrders,
    HashTrieSipPruned, HashTriejoin, CostBasedOptimiser, AnyOrders,
    HashTrieFxPruned, HashTriejoin, LexicographicOptimiser, AnyOrders,
    HashTrieFxPruned, HashTriejoin, CardinalityOptimiser, AnyOrders,
    HashTrieFxPruned, HashTriejoin, CostBasedOptimiser, AnyOrders
);

// Copies keep their config and build mode: the Config and BuildMode
// aliases under `any`.
type HashTrieSipHalfFullAny = Configured<HashTrieSip, HalfFull>;
type HashTrieFxHalfFullAny = Configured<HashTrieFx, HalfFull>;
type ColumnTrieIncrementalAny = BuiltWith<ColumnTrie, Incremental>;

define_multiway_join_test_suite_with_column_orders!(
    HashTrieSipHalfFullAny, HashTriejoin, LexicographicOptimiser, AnyOrders,
    HashTrieSipHalfFullAny, HashTriejoin, CardinalityOptimiser, AnyOrders,
    HashTrieSipHalfFullAny, HashTriejoin, CostBasedOptimiser, AnyOrders,
    HashTrieFxHalfFullAny, HashTriejoin, LexicographicOptimiser, AnyOrders,
    HashTrieFxHalfFullAny, HashTriejoin, CardinalityOptimiser, AnyOrders,
    HashTrieFxHalfFullAny, HashTriejoin, CostBasedOptimiser, AnyOrders,
    ColumnTrieIncrementalAny, LeapfrogTriejoin, LexicographicOptimiser, AnyOrders,
    ColumnTrieIncrementalAny, LeapfrogTriejoin, CardinalityOptimiser, AnyOrders,
    ColumnTrieIncrementalAny, LeapfrogTriejoin, CostBasedOptimiser, AnyOrders
);
```

(The `JoinEntry` impl for `Configured<HashTrie<H, P>, C>` exists; the
blanket `TrieIterable` impl covers `BuiltWith<ColumnTrie, _>`. If the
`:lower` paste on a type alias ident produces a name clash, rename the
aliases.)

- [ ] **Step 4: The `any`-only patterns**

Create `kermit/tests/column_orders_any.rs`:

```rust
//! Queries `--column-orders stored` rejects with `CyclicAttributeOrder`
//! and `any` answers (issue #93), on every alias × algorithm × optimiser.
//! Each pattern pins the rows `any` returns and, where the query is
//! cyclic, that `stored` still rejects it.

mod common;

use {
    common::utils::{join_under_planner, Inputs, JoinEntry},
    kermit::db::JoinError,
    kermit_algos::{
        CardinalityOptimiser, ColumnOrderPolicy, CostBasedOptimiser, HashTriejoin, JoinQuery,
        LeapfrogTriejoin, LexicographicOptimiser, Planner, QueryOptimiser,
    },
    kermit_ds::{
        BinarySeek, Cardinality, ColumnTrie, GallopingSeek, HashTrie, LinearSeek, Relation,
        RelationHeader, SingletonPruning, TreeTrie,
    },
    kermit_iters::{FxHashStrategy, SipHashStrategy},
    std::collections::BTreeMap,
};

type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;
type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieFxPruned = HashTrie<FxHashStrategy, SingletonPruning>;
type TreeTrieLinear = TreeTrie<LinearSeek>;
type TreeTrieBinary = TreeTrie<BinarySeek>;
type TreeTrieGalloping = TreeTrie<GallopingSeek>;
type ColumnTrieLinear = ColumnTrie<LinearSeek>;
type ColumnTrieBinary = ColumnTrie<BinarySeek>;
type ColumnTrieGalloping = ColumnTrie<GallopingSeek>;

/// A pattern: relations as `(name, arity, tuples)`, the query, and the
/// sorted rows `any` must return.
struct Pattern {
    relations: Vec<(&'static str, usize, Vec<Vec<usize>>)>,
    query: &'static str,
    rows: Vec<Vec<usize>>,
    /// Whether `stored` rejects the query with `CyclicAttributeOrder`.
    cyclic_under_stored: bool,
}

fn mutual_edges() -> Pattern {
    Pattern {
        relations: vec![("edge", 2, vec![vec![1, 2], vec![2, 1], vec![2, 3], vec![3, 2], vec![1, 3]])],
        query: "Q(X, Y) :- edge(X, Y), edge(Y, X).",
        rows: vec![vec![1, 2], vec![2, 1], vec![2, 3], vec![3, 2]],
        cyclic_under_stored: true,
    }
}

/// #82's incoming star: acyclic under `stored`, and under `any` the head
/// puts `V0` first, so `lexicographic` reorients both atoms.
fn incoming_star() -> Pattern {
    Pattern {
        relations: vec![
            ("includes", 2, vec![vec![10, 1], vec![11, 1], vec![12, 2]]),
            ("purchasefor", 2, vec![vec![20, 1], vec![21, 2], vec![22, 3]]),
        ],
        query: "Q(V0, V2, V7) :- includes(V2, V0), purchasefor(V7, V0).",
        rows: vec![vec![1, 10, 20], vec![1, 11, 20], vec![2, 12, 21]],
        cyclic_under_stored: false,
    }
}

fn ternary_cycle() -> Pattern {
    Pattern {
        relations: vec![
            ("r", 3, vec![vec![1, 2, 3], vec![4, 5, 6], vec![1, 7, 9]]),
            ("s", 2, vec![vec![3, 1], vec![6, 4], vec![9, 2]]),
        ],
        query: "Q(X, Y, Z) :- r(X, Y, Z), s(Z, X).",
        rows: vec![vec![1, 2, 3], vec![4, 5, 6]],
        cyclic_under_stored: true,
    }
}

/// A repeated variable on a reoriented atom: the selection's equality
/// must follow its columns through the permutation.
fn repeat_on_reoriented_atom() -> Pattern {
    Pattern {
        relations: vec![
            ("r", 3, vec![vec![1, 1, 5], vec![1, 2, 5], vec![2, 2, 6], vec![3, 3, 7]]),
            ("s", 2, vec![vec![5, 1], vec![6, 2], vec![7, 9]]),
        ],
        query: "Q(X, Y) :- r(X, X, Y), s(Y, X).",
        rows: vec![vec![1, 5], vec![2, 6]],
        cyclic_under_stored: true,
    }
}

/// A constant on a reoriented atom: `s(Y, c7, X)` rewrites to
/// `s(Y, K0, X), Const_c7(K0)`, so `X → Y → K0 → X` is a cycle.
fn constant_on_reoriented_atom() -> Pattern {
    Pattern {
        relations: vec![
            ("r", 2, vec![vec![1, 2], vec![3, 4], vec![5, 6]]),
            ("s", 3, vec![vec![2, 7, 1], vec![4, 8, 3], vec![6, 7, 9]]),
        ],
        query: "Q(X, Y) :- r(X, Y), s(Y, c7, X).",
        rows: vec![vec![1, 2]],
        cyclic_under_stored: true,
    }
}

/// Runs `pattern` over `R` through `JA` under `optimiser`: `stored`
/// rejects or agrees, `any` returns the pinned rows. Returns the specs
/// `any` built.
fn check<R: Relation + Cardinality, JA: JoinEntry<R>, O: QueryOptimiser + Default>(
    pattern: &Pattern,
) -> Vec<kermit_algos::IndexSpec> {
    let inputs: Inputs = pattern
        .relations
        .iter()
        .map(|(name, arity, tuples)| {
            let header = RelationHeader::new_positional(*name, *arity);
            (name.to_string(), (header, tuples.clone()))
        })
        .collect();
    let query: JoinQuery = pattern.query.parse().unwrap();
    // One database per policy: a planner is not `Clone`, and a fresh
    // store per run keeps the two runs independent.
    let run = |policy: ColumnOrderPolicy| {
        let planner = Planner::new(Box::new(O::default()), policy);
        let store: BTreeMap<String, R> = inputs
            .iter()
            .map(|(name, (header, tuples))| {
                (name.clone(), R::from_tuples(header.clone(), tuples.clone()))
            })
            .collect();
        let mut database = JA::database(store, planner.required_statistics());
        join_under_planner::<R, JA>(&mut database, &inputs, query.clone(), &planner)
    };

    let stored = run(ColumnOrderPolicy::Stored);
    if pattern.cyclic_under_stored {
        assert!(
            matches!(stored, Err(JoinError::CyclicAttributeOrder { .. })),
            "{}: stored must reject a cyclic query, got {stored:?}",
            pattern.query
        );
    } else {
        let (mut rows, _) = stored.unwrap_or_else(|e| panic!("{}: {e}", pattern.query));
        rows.sort();
        assert_eq!(rows, pattern.rows, "{} under stored", pattern.query);
    }

    let (mut rows, specs) =
        run(ColumnOrderPolicy::Any).unwrap_or_else(|e| panic!("{}: {e}", pattern.query));
    rows.sort();
    assert_eq!(rows, pattern.rows, "{} under any", pattern.query);
    specs
}

macro_rules! any_only_patterns {
    ($( $relation:ident, $algo:ty, $optimiser:ty ),+ $(,)?) => {
        $(
            paste::paste! {
                mod [<any_ $relation:lower _ $algo:lower _ $optimiser:lower>] {
                    use super::*;

                    #[test]
                    fn mutual_edges() { check::<$relation, $algo, $optimiser>(&super::mutual_edges()); }

                    #[test]
                    fn incoming_star() { check::<$relation, $algo, $optimiser>(&super::incoming_star()); }

                    #[test]
                    fn ternary_cycle() { check::<$relation, $algo, $optimiser>(&super::ternary_cycle()); }

                    #[test]
                    fn repeat_on_reoriented_atom() {
                        check::<$relation, $algo, $optimiser>(&super::repeat_on_reoriented_atom());
                    }

                    #[test]
                    fn constant_on_reoriented_atom() {
                        check::<$relation, $algo, $optimiser>(&super::constant_on_reoriented_atom());
                    }
                }
            }
        )+
    };
}

any_only_patterns!(
    TreeTrieLinear, LeapfrogTriejoin, LexicographicOptimiser,
    TreeTrieLinear, LeapfrogTriejoin, CardinalityOptimiser,
    TreeTrieLinear, LeapfrogTriejoin, CostBasedOptimiser,
    TreeTrieBinary, LeapfrogTriejoin, LexicographicOptimiser,
    TreeTrieBinary, LeapfrogTriejoin, CardinalityOptimiser,
    TreeTrieBinary, LeapfrogTriejoin, CostBasedOptimiser,
    TreeTrieGalloping, LeapfrogTriejoin, LexicographicOptimiser,
    TreeTrieGalloping, LeapfrogTriejoin, CardinalityOptimiser,
    TreeTrieGalloping, LeapfrogTriejoin, CostBasedOptimiser,
    ColumnTrieLinear, LeapfrogTriejoin, LexicographicOptimiser,
    ColumnTrieLinear, LeapfrogTriejoin, CardinalityOptimiser,
    ColumnTrieLinear, LeapfrogTriejoin, CostBasedOptimiser,
    ColumnTrieBinary, LeapfrogTriejoin, LexicographicOptimiser,
    ColumnTrieBinary, LeapfrogTriejoin, CardinalityOptimiser,
    ColumnTrieBinary, LeapfrogTriejoin, CostBasedOptimiser,
    ColumnTrieGalloping, LeapfrogTriejoin, LexicographicOptimiser,
    ColumnTrieGalloping, LeapfrogTriejoin, CardinalityOptimiser,
    ColumnTrieGalloping, LeapfrogTriejoin, CostBasedOptimiser,
    HashTrieSip, HashTriejoin, LexicographicOptimiser,
    HashTrieSip, HashTriejoin, CardinalityOptimiser,
    HashTrieSip, HashTriejoin, CostBasedOptimiser,
    HashTrieFx, HashTriejoin, LexicographicOptimiser,
    HashTrieFx, HashTriejoin, CardinalityOptimiser,
    HashTrieFx, HashTriejoin, CostBasedOptimiser,
    HashTrieSipPruned, HashTriejoin, LexicographicOptimiser,
    HashTrieSipPruned, HashTriejoin, CardinalityOptimiser,
    HashTrieSipPruned, HashTriejoin, CostBasedOptimiser,
    HashTrieFxPruned, HashTriejoin, LexicographicOptimiser,
    HashTrieFxPruned, HashTriejoin, CardinalityOptimiser,
    HashTrieFxPruned, HashTriejoin, CostBasedOptimiser,
);

/// Under `lexicographic` the incoming star reorients both atoms (the
/// head puts `V0` first), so `any` builds two copies; the mutual-edge
/// query builds one.
#[test]
fn lexicographic_builds_the_expected_copies() {
    let specs = check::<TreeTrie, LeapfrogTriejoin, LexicographicOptimiser>(&incoming_star());
    let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Index_1_0_includes", "Index_1_0_purchasefor"]);
    let specs = check::<HashTrieSip, HashTriejoin, LexicographicOptimiser>(&mutual_edges());
    assert_eq!(specs.len(), 1);
}
```

(`paste` is already a dev-dependency of `kermit`; check `kermit/Cargo.toml`.)

- [ ] **Step 5: Run, format, commit**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests --test column_orders_any 2>&1 | grep -E '^test result|FAILED|panicked'
nix develop $WT --command cargo fmt --all
git -C $WT add kermit/tests/common/utils.rs kermit/tests/common/macros.rs kermit/tests/join_tests.rs kermit/tests/column_orders_any.rs
git -C $WT commit -q -F - <<'MSG'
test: the 16 join patterns under any, and the any-only patterns (#93)

`define_multiway_join_test_suite_with_column_orders!` runs the standard
suite under a `ColumnOrderProvider`; every alias × optimiser runs it
under `any`, including the Config and BuildMode aliases, so copies keep
their config and build mode. `column_orders_any.rs` pins five queries
`stored` rejects or reorients: mutual edges, #82's incoming star, a
ternary cycle, a repeat and a constant on a reoriented atom.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
```

Expected: `join_tests` grows from 624 to 1248 tests (39 + 39 suites ×
16); `column_orders_any` has 151.

- [ ] **Step 6: Mutation check (the equality remap, end to end)**

Mutant: in `kermit-algos/src/orient.rs`, comment out the equality remap
loop (as in Task 4's check). Expected to fail: every
`repeat_on_reoriented_atom` test in `column_orders_any` (wrong rows or a
`source < repeat` debug assertion), and `a_selection_on_a_reoriented_atom_keeps_its_equality`
in `db.rs`. Revert.

---

## Task 12 [P5]: Equivalence, the oracles and allocation under both policies

**Files:**
- Create: `kermit/tests/column_orders_equivalence.rs`
- Modify: `kermit/tests/lubm_mini_oracle.rs:201-300`, `lubm_cardinalities.rs:59-219`, `watdiv_correctness.rs:43-94`, `subject_position_constant.rs:39-104`, `result_allocation.rs`

- [ ] **Step 1: A seeded equivalence test**

Create `kermit/tests/column_orders_equivalence.rs`:

```rust
//! `any` returns the same multiset as `stored` for every query `stored`
//! accepts, and answers every query `stored` rejects only for its column
//! order (issue #93). Random small conjunctive queries over random small
//! relations, from a fixed seed, so a failure reproduces; no new crate.

mod common;

use {
    common::utils::{join_under_planner, Inputs, JoinEntry},
    kermit::db::JoinError,
    kermit_algos::{
        CardinalityOptimiser, ColumnOrderPolicy, CostBasedOptimiser, HashTriejoin, JoinQuery,
        LeapfrogTriejoin, LexicographicOptimiser, Planner, QueryOptimiser,
    },
    kermit_ds::{Cardinality, ColumnTrie, HashTrie, Relation, RelationHeader, SingletonPruning, TreeTrie},
    kermit_iters::SipHashStrategy,
    std::collections::BTreeMap,
};

/// A 64-bit linear congruential generator (Knuth's MMIX constants).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// A value in `0..n`.
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

const VALUES: usize = 4;
const VARIABLES: [&str; 4] = ["A", "B", "C", "D"];

/// Up to three relations of arity 1–3 with 0–6 tuples over `0..VALUES`.
fn random_relations(rng: &mut Lcg) -> Vec<(String, usize, Vec<Vec<usize>>)> {
    (0..1 + rng.below(3))
        .map(|i| {
            let arity = 1 + rng.below(3);
            let tuples = (0..rng.below(7))
                .map(|_| (0..arity).map(|_| rng.below(VALUES)).collect())
                .collect();
            (format!("r{i}"), arity, tuples)
        })
        .collect()
}

/// Two or three atoms over `relations`, each column a variable from the
/// pool or (one in eight, never the very first column) a constant; the
/// head is every variable used, so it is always bound.
fn random_query(rng: &mut Lcg, relations: &[(String, usize, Vec<Vec<usize>>)]) -> String {
    let mut used: Vec<&str> = Vec::new();
    let atoms: Vec<String> = (0..2 + rng.below(2))
        .enumerate()
        .map(|(atom, _)| {
            let (name, arity, _) = &relations[rng.below(relations.len())];
            let terms: Vec<String> = (0..*arity)
                .map(|column| {
                    let first = atom == 0 && column == 0;
                    if !first && rng.below(8) == 0 {
                        format!("c{}", rng.below(VALUES))
                    } else {
                        let v = VARIABLES[rng.below(VARIABLES.len())];
                        if !used.contains(&v) {
                            used.push(v);
                        }
                        v.to_string()
                    }
                })
                .collect();
            format!("{name}({})", terms.join(", "))
        })
        .collect();
    format!("Q({}) :- {}.", used.join(", "), atoms.join(", "))
}

/// Rows of `query` over `relations` as `R` through `JA` under `policy`,
/// sorted.
fn rows<R: Relation + Cardinality, JA: JoinEntry<R>, O: QueryOptimiser + Default>(
    relations: &[(String, usize, Vec<Vec<usize>>)], query: &JoinQuery, policy: ColumnOrderPolicy,
) -> Result<Vec<Vec<usize>>, JoinError> {
    let mut inputs: Inputs = BTreeMap::new();
    let mut store: BTreeMap<String, R> = BTreeMap::new();
    for (name, arity, tuples) in relations {
        let header = RelationHeader::new_positional(name, *arity);
        inputs.insert(name.clone(), (header.clone(), tuples.clone()));
        store.insert(name.clone(), R::from_tuples(header, tuples.clone()));
    }
    let planner = Planner::new(Box::new(O::default()), policy);
    let mut database = JA::database(store, planner.required_statistics());
    let (mut rows, _) = join_under_planner::<R, JA>(&mut database, &inputs, query.clone(), &planner)?;
    rows.sort();
    Ok(rows)
}

/// For `seeds` random cases: a query `stored` accepts gives the same rows
/// under `any`; a query `stored` rejects only for its column order runs
/// under `any`; any other rejection is identical under both.
fn equivalent<R: Relation + Cardinality, JA: JoinEntry<R>, O: QueryOptimiser + Default>(
    seeds: std::ops::Range<u64>,
) -> (usize, usize) {
    let (mut agreed, mut lifted) = (0, 0);
    for seed in seeds {
        let mut rng = Lcg(seed);
        let relations = random_relations(&mut rng);
        let text = random_query(&mut rng, &relations);
        let Ok(query) = text.parse::<JoinQuery>() else {
            panic!("seed {seed}: unparsable query {text}");
        };
        let stored = rows::<R, JA, O>(&relations, &query, ColumnOrderPolicy::Stored);
        let any = rows::<R, JA, O>(&relations, &query, ColumnOrderPolicy::Any);
        match stored {
            | Ok(stored) => {
                assert_eq!(any.as_ref().ok(), Some(&stored), "seed {seed}: {text}");
                agreed += 1;
            },
            | Err(JoinError::CyclicAttributeOrder { .. }) => {
                assert!(any.is_ok(), "seed {seed}: {text}: {any:?}");
                lifted += 1;
            },
            | Err(other) => assert_eq!(any, Err(other), "seed {seed}: {text}"),
        }
    }
    (agreed, lifted)
}

#[test]
fn stored_and_any_agree_on_every_accepted_query() {
    let (agreed, lifted) =
        equivalent::<TreeTrie, LeapfrogTriejoin, LexicographicOptimiser>(0..300);
    assert!(agreed >= 100 && lifted >= 10, "agreed {agreed}, lifted {lifted}");
    equivalent::<TreeTrie, LeapfrogTriejoin, CardinalityOptimiser>(0..300);
    equivalent::<TreeTrie, LeapfrogTriejoin, CostBasedOptimiser>(0..300);
    equivalent::<ColumnTrie, LeapfrogTriejoin, LexicographicOptimiser>(0..300);
    equivalent::<ColumnTrie, LeapfrogTriejoin, CostBasedOptimiser>(0..300);
    equivalent::<HashTrie<SipHashStrategy>, HashTriejoin, LexicographicOptimiser>(0..300);
    equivalent::<HashTrie<SipHashStrategy>, HashTriejoin, CostBasedOptimiser>(0..300);
    equivalent::<HashTrie<SipHashStrategy, SingletonPruning>, HashTriejoin, CardinalityOptimiser>(
        0..300,
    );
}
```

If `random_query`'s empty-variable fallback reads awkwardly, replace it
by re-drawing until at least one variable is used (loop on the seed's
rng). If the `agreed`/`lifted` lower bounds do not hold for the first
300 seeds, print the counts once, lower the bounds to what the generator
produces and note it in the test's comment; they exist to catch a
generator that never produces an interesting case.

- [ ] **Step 2: The oracles under both policies**

`lubm_mini_oracle.rs`: `cardinality_mismatches` takes `planner: &Planner`
and a `label: &str`, loads each relation with `kermit_ds::read_parquet`
to keep its `(header, tuples)` in an `Inputs` map beside the `R` built
from them, and runs each query through
`join_under_planner::<R, LeapfrogTriejoin>` (add `mod common;` and
import it). The driver loops both policies:

```rust
    for &policy in ColumnOrderPolicy::value_variants() {
        for &optimiser in Optimiser::value_variants() {
            let planner = Planner::new(optimiser.instantiate(), policy);
            let label = format!("{} / {}", optimiser.axis_value(), policy.axis_value());
            mismatches.extend(cardinality_mismatches::<TreeTrie<LinearSeek>>(&bench, out.path(), &planner, &label, &expected));
            … the six tries …
        }
    }
```

and the final message says "x 3 optimisers x 2 policies". The same
change to `lubm_cardinalities.rs` (its hand-written `optimisers` vec
becomes `(name, Optimiser)` pairs expanded over both policies) and to
`watdiv_correctness.rs` (`check_cardinalities::<R>(optimiser, policy)`
looping `ColumnOrderPolicy::value_variants()` inside the optimiser
loop). `subject_position_constant.rs`: `lftj(query, optimiser, policy)`
and `hash(…)` build their planner with the policy and go through
`join_under_planner`; each test loops both policies.

- [ ] **Step 3: Allocation under `any`**

In `result_allocation.rs`, add a reversed `R` fixture and two cells
(copies are built before the measured region; the join over them must
still allocate nothing per row):

```rust
/// `R = {(0, x)}`: the reversed `R`, so `Q(X, Y, Z) :- R(Y, X), S(X, Z).`
/// reads it through the copy `Index_1_0_R` under `any`.
fn reversed_relations<Rel: Relation>(fan_out: usize) -> BTreeMap<String, Rel> {
    let r: Vec<Vec<usize>> = (0..XS).map(|x| vec![0, x]).collect();
    BTreeMap::from([
        ("R".to_string(), Rel::from_tuples(RelationHeader::new_positional("R", 2), r)),
        (
            "S".to_string(),
            Rel::from_tuples(RelationHeader::new_positional("S", 2), s_tuples(fan_out)),
        ),
    ])
}

const REVERSED_QUERY: &str = "Q(X, Y, Z) :- R(Y, X), S(X, Z).";

/// Allocations of one streamed LFTJ join of `REVERSED_QUERY` under `any`,
/// with the copy built before measuring, after checking the row count.
fn lftj_any_allocations<Rel: TrieIterable + Cardinality + Relation>(fan_out: usize) -> u64 {
    let planner = Planner::new(Box::new(LexicographicOptimiser), ColumnOrderPolicy::Any);
    let mut database = Database::from(reversed_relations::<Rel>(fan_out));
    let query: JoinQuery = REVERSED_QUERY.parse().unwrap();
    let r: Vec<Vec<usize>> = (0..XS).map(|x| vec![0, x]).collect();
    for spec in database.required_indexes(&query, &planner).unwrap() {
        let header = RelationHeader::new_positional("R", 2);
        database.add_index(spec.clone(), build_index::<Rel>(&spec, &header, &r));
    }
    let mut produced = 0usize;
    let info = allocation_counter::measure(|| {
        lftj_join_for_each::<Rel, LeapfrogTriejoin>(&database, query, &planner, |tuple| {
            std::hint::black_box(tuple);
            produced += 1;
        })
        .unwrap();
    });
    assert_eq!(produced, XS * fan_out, "the join must produce every row");
    info.count_total
}

#[test]
fn tree_trie_lftj_under_any_allocates_independently_of_result_size() {
    lftj_any_allocations::<TreeTrie>(SMALL);
    assert_flat(
        "TreeTrie/LFTJ/any",
        lftj_any_allocations::<TreeTrie>(SMALL),
        lftj_any_allocations::<TreeTrie>(LARGE),
    );
}
```

and the HashTriejoin twin (`htj_any_allocations<H, P>` over
`hash_join_for_each`, test `hash_trie_sip_htj_under_any_allocates_independently_of_result_size`).
Import `kermit::db::build_index`, `kermit_algos::{ColumnOrderPolicy, Planner}`,
`kermit_ds::RelationHeader`.

- [ ] **Step 4: Run, format, commit**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test column_orders_equivalence --test lubm_mini_oracle --test lubm_cardinalities --test watdiv_correctness --test subject_position_constant --test result_allocation 2>&1 | grep -E '^test result|FAILED|panicked|skip'
nix develop $WT --command cargo fmt --all
git -C $WT add kermit/tests/column_orders_equivalence.rs kermit/tests/lubm_mini_oracle.rs kermit/tests/lubm_cardinalities.rs kermit/tests/watdiv_correctness.rs kermit/tests/subject_position_constant.rs kermit/tests/result_allocation.rs
git -C $WT commit -q -F - <<'MSG'
test: stored and any agree, on random queries and the oracles (#93)

A seeded generator checks that `any` returns `stored`'s rows for every
query `stored` accepts and answers every query it rejects only for its
column order; the LUBM, WatDiv and subject-position oracles run under
both policies; the streamed join over copies allocates nothing per row.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
```

`lubm_cardinalities` needs `java` (run inside `nix develop`); confirm in
its output that it ran rather than skipped.

- [ ] **Step 5: Mutation check: the plan translation, end to end**

Mutant: in `orient.rs`, replace the translated plan with the input plan
(Task 4's second mutant). Expected to fail:
`stored_and_any_agree_on_every_accepted_query` (a renumbered body-only
variable makes a plan the executor's `validate` panics on), and
`the_plan_is_translated_by_name`. Revert.

---

## Task 13 [controller]: Verification beyond CI

**Files:** `$SCRATCH/verification.md` (not committed); `$SCRATCH/templates.py`.

The cached `watdiv-stress-100-test-1-prelim` data under
`~/.cache/kermit/benchmarks/` is read-only here (never `--force`, `bench
clean` or `bench gen` against it; see the shared-cache rule in memory).
The DuckDB counts are in
`$RUNS/thesis-rebench-2026-10-02/groundtruth/duckdb-prelim.json`
(`{ "q0264": { "count": N, … }, … }`).

- [ ] **Step 1: Release binary**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build --release -p kermit 2>&1 | tail -1
ls -la $WT/target/release/kermit
```

- [ ] **Step 2: The 10 templates against DuckDB, within 10 s**

Write `$SCRATCH/templates.py`: read the cache's `benchmark.yml`
(`~/.cache/kermit/benchmarks/watdiv-stress-100-test-1-prelim/benchmark.yml`),
pick the queries named `q0264 q0079 q0017 q0030 q0306 q0409 q0035 q0010
q0008 q0085`, write each to `$SCRATCH/dl/<name>.dl`, and for each of the
three structures (`tree-trie`/`leapfrog-triejoin`,
`column-trie`/`leapfrog-triejoin`, `hash-trie`/`hash-triejoin`) run

```
timeout 300 $WT/target/release/kermit join -r <every relation the query names, as cache/<name>.parquet> -q $SCRATCH/dl/<name>.dl -i <ds> -a <algo> --optimiser cost-based --column-orders any -o /dev/null
```

under `/usr/bin/time -f %e`, recording wall seconds and, in a second
run with `-o $SCRATCH/out/<name>.<ds>.csv`, the row count (lines minus
the header). Also time one trivial unary query per structure over the
same relations as the load baseline, so "per execution" is wall minus
load. Compare each count with `duckdb-prelim.json[name].count`.

Expected: all 30 counts match DuckDB; each execution (wall − load) is
under 10 s. Record the table in `$SCRATCH/verification.md`. If a
template exceeds 10 s on one structure, record it; it becomes a
documented result, not a blocker, and goes to the user at checkpoint 2.

- [ ] **Step 3: How many templates exceed the DP budget under `any`**

Under `any` the search reaches every subset, 2ⁿ − 1 states for n
variables, so it falls back exactly when n ≥ 15 (2¹⁵ − 1 > 16,384), with
n counted after the rewrites: distinct variables plus one per constant
(each constant becomes a fresh variable). Count over all 124 templates of
`$RUNS/watdiv-stress-100-prelim-2026-09-10/diagnostics/issue-68/queries-124.yml`
with a few lines of Python (parse each query's body atoms, count distinct
uppercase variable names plus `c<digits>` terms). Record the count and
the template names in `$SCRATCH/verification.md`; Task 14 writes it into
`docs/optimisers/cost-based.md`.

- [ ] **Step 4: `stored` sanity run within the codegen precision bound**

With the baseline binary from `git archive` of the commit before the
spec (per the perf-comparison recipe in memory: a separate target dir,
distinct `--name`s, TreeTrie as the control), run
`bench run oxford-uniform-s3 -i tree-trie -a leapfrog-triejoin -m
iteration` five times each, alternating arms. Expected: the ratio of
medians within ±3 %. Record it.

---

## Task 14 [controller]: Documentation

**Files:**
- Modify: `ARCHITECTURE.md` ("Query validation" 338–342, "Query Planning" 360–363, "Database layer" 431–437)
- Modify: `docs/algorithms/leapfrog-triejoin.md` ("Valid global attribute order" invariant, line 52), `docs/algorithms/hash-triejoin.md` ("Variable ordering arrives as a plan", last invariant)
- Modify: `docs/optimisers/{lexicographic,cardinality,cost-based}.md` (add a "Column orders" section before "CLI")
- Modify: `CLAUDE.md` (Key Trait Hierarchy; the optimiser recipe step 2; Gotchas)
- (`docs/specs/bench-report-schema.md` was done in Task 10.)

- [ ] **Step 1: `ARCHITECTURE.md`**

"Query validation": the last check becomes "and, after the rewrites
below and under `--column-orders stored` only, the atoms admit one column
order per relation (`kermit_algos::check_attribute_order`)", and add one
sentence: "`--column-orders any` lifts that limitation: the planner
ignores stored column orders and the orientation rewrite (below) reads a
disagreeing atom through a reordered copy." Add an "Orientation rewrite"
subsection after "Query Planning":

> ### Orientation rewrite
>
> After planning, `kermit_algos::orient` (see `kermit-algos/src/orient.rs`) makes the plan executable under `--column-orders any`. For each body atom other than a `Const_*` singleton, π is its columns sorted by plan position; a non-identity π permutes the atom's terms and renames a plain atom to `Index_<π>_<base>` (e.g. `Index_1_0_edge`), while a `Select_<n>_<base>` view keeps its name and its `SelectionSpec` points at the copy with every column equality mapped through π. Atoms with equal `(base, π)` share one `IndexSpec`. Body-only variables are numbered by first appearance, so the plan is translated by variable name through a fresh `analyse` of the oriented query. Under `stored` the rewrite returns its input. The executors never learn about the policy: by the time the plan reaches `join_for_each`, every atom's term order equals its (copied) trie's column order.
>
> The copies live on the `Database` (see "Database layer"): `Database::required_indexes(query, planner)` runs validate, rewrite, plan and orient and names the copies the store lacks; the caller builds each from the base relation's file-order tuples (`kermit::db::build_index` in the library, `ExecutionFamily::add_index` through `build_relation` in the binary, so config and build mode carry over) and adds it, and clears them after the query. One query's copies are held at a time. `bench run` times them as `copies` beside `insertion`, measures each as `space/Index_<π>_<base>`, includes them in `end_to_end`'s fresh build, and leaves `iteration` a join over prebuilt structures. The planner sees only base names: statistics are built before orientation.

"Query Planning": "asks its `QueryOptimiser`" becomes "asks its `Planner`
(a `QueryOptimiser` plus the `ColumnOrderPolicy`, `--column-orders
stored|any`)", and "the set of topological orders of the column-order
constraint DAG" becomes "… of the pinned atoms' column-order constraint
DAG (`Precedence::for_query`; under `any` no atom is pinned and every
order is valid)". "Database layer": the entry points take `&Planner`;
add "plus the reordered copies the current query reads under `any`,
with `required_indexes` / `add_index` / `clear_indexes` / `index`; a copy
the plan reads that the store lacks is `JoinError::MissingIndex`".

- [ ] **Step 2: The algorithm and optimiser docs**

LFTJ "Valid global attribute order": after "`topological_order` itself
still panics on one, for callers that skip the entry points." add "Under
`--column-orders any` the cycle does not arise: the planner is free, and
the orientation rewrite (`kermit-algos/src/orient.rs`) reads one of the
atoms through a reordered copy, so the term order of every atom LFTJ
sees still equals its trie's column order. Nothing in this algorithm
changed for the policy." The HTJ doc's last invariant gets the same two
sentences (with "HTJ" and "its hash trie's column order").

Each optimiser doc gets, before "## CLI":

> ## Column orders
>
> `--column-orders stored` (the default) pins every atom to its stored
> column order: the plan binds each atom's columns left to right, as
> every plan did before issue #93. `--column-orders any` pins nothing, so
> the ranking alone decides and an atom whose plan disagrees with its
> stored order is read through a reordered copy (`docs/specs/2026-10-05-column-orders-design.md`).

with one policy-specific sentence each: lexicographic — "Under `any` the
plan is the canonical order itself: head variables in head order, then
body-only variables by first appearance."; cardinality — "Under `any`
the smallest relation's variable goes first even when its column is not
that relation's first."; cost-based — "Under `any` every subset of the
variables is a DP state (2ⁿ − 1), so the default budget covers up to 14
variables and the search falls back to `cardinality` above; of the 124
`watdiv-stress-100-test-1` templates, N exceed it (Task 13's count, with
their names). The estimate reads the distinct counts of an atom's bound
columns as a set, which under `stored` is always a prefix, so plans
under `stored` are unchanged." Also update cost-based's "Regressions and
limitations" subject-first ceiling sentence: `any` lifts it per query;
#82 keeps copies across queries.

- [ ] **Step 3: `CLAUDE.md`**

Key Trait Hierarchy: after the `QueryOptimiser` bullet add
"- **Planner**: a `QueryOptimiser` plus a `ColumnOrderPolicy`
(`--column-orders stored|any`, `kermit-algos/src/optimiser/{planner,column_orders}.rs`);
every join entry point, both bench families and the test harness take a
`&Planner`. The policy reaches the optimisers through
`CatalogStats::is_pinned` and `Precedence::for_query`: `stored` pins
every atom to its stored column order, `any` pins none." Optimiser
recipe step 2: "Build the ordering with
`topological_order(&Precedence::for_query(query, stats), rank)`"
(replacing the old signature) and "under `any` the precedence is empty,
so your ranking alone decides". Workspace architecture `kermit-algos`
line: add "`orient` (the orientation rewrite)". Gotchas, four new
bullets after the selection-rewrite one:

- **Orientation rewrite (`--column-orders any`)**: after planning, `kermit_algos::orient` permutes each atom whose plan disagrees with its stored column order and renames it `Index_<π>_<base>` (a `Select_` view keeps its name; its spec points at the copy with equalities remapped), translating the plan by variable name since `analyse` renumbers body-only variables. `Index_` is a reserved prefix beside `Const_` and `Select_`. The executors and data structures do not change. Under `stored` it is the identity.
- **Copies are built from file-order tuples through `build_relation`, never `project`**: `Database::required_indexes(query, &planner)` names the copies; `ExecutionFamily::add_index` (binary) and `kermit::db::build_index` (library, tests) build them from the base relation's `(header, tuples)` so config and build mode carry over; `project` rebuilds in the structure's iteration order with a nameless header and drops the build mode. `kermit join` and `bench run` therefore keep every relation's file-order tuples under `any` (the default metrics already keep them for `insertion`). A plan that reads a copy the store lacks is `JoinError::MissingIndex`.
- **`CyclicAttributeOrder` is a `stored` limitation**: `validate_query` takes the policy and runs `check_attribute_order` under `stored` only; `edge(X, Y), edge(Y, X)` runs under `any` over one copy. The error message names the flag.
- **A `--name` per policy**: like `--optimiser`, the Criterion group omits `column_orders`, so two policies under one `--name` overwrite each other's results. Under `any` a report gains `copies` (beside `insertion`, only when the plan needs a copy), `space/Index_<π>_<base>` per copy and an `index:` metadata line per copy; `iteration` stays a join over prebuilt structures; schema version stays 3 and kermit-lab back-fills `stored` on join rows that predate the axis.

- [ ] **Step 4: Check and commit**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo doc --workspace --no-deps 2>&1 | grep -E 'warning|error' ; echo "doc exit: $?"
git -C $WT add ARCHITECTURE.md CLAUDE.md docs/algorithms/leapfrog-triejoin.md docs/algorithms/hash-triejoin.md docs/optimisers/lexicographic.md docs/optimisers/cardinality.md docs/optimisers/cost-based.md
git -C $WT commit -q -F - <<'MSG'
docs: column orders (#93)

Validate, rewrite, plan, orient, execute; the copies on the catalog;
the Planner in the trait hierarchy and the optimiser recipe; each
optimiser's behaviour per policy and the WatDiv templates that exceed
the cost-based budget under `any`; the gotchas for the orientation
rewrite, copies via `build_relation`, `CyclicAttributeOrder` under
`stored` only and a `--name` per policy.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HghrsqcjkwJFhyVZh2zwgq
MSG
```

---

## Task 15 [controller]: Gate and hand-off

- [ ] **Step 1: Toolchain parity**

`nix flake update rust-overlay` only if `cargo fmt --check` and CI's
nightly could differ (see the rustfmt-drift memory); otherwise skip.

- [ ] **Step 2: The gate, foreground except miri**

```bash
WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/93-planner_18db980ce7f4cbab
SCRATCH=<scratchpad>
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace 2>&1 | tee $SCRATCH/gate-test.log | grep -E '^test result:' | awk '{p+=$4; f+=$6} END {print "passed", p, "failed", f}'
RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets 2>&1 | tail -2
nix develop $WT --command cargo fmt --all --check && echo fmt-ok
RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo doc --workspace --no-deps 2>&1 | tail -1
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit 2>&1 | tail -1
env KERMIT_BIN=$WT/target/debug/kermit nix develop $WT --command uv --directory $WT/python/kermit-lab run pytest -q 2>&1 | tail -2
```

Miri, detached (it excludes `kermit` and `kermit-bench`; `kermit-algos`'s
new tests run under it):

```bash
setsid nohup env MIRIFLAGS="-Zmiri-disable-isolation" CARGO_BUILD_JOBS=2 nix develop $WT --command cargo miri test --workspace --exclude kermit --exclude kermit-bench > $SCRATCH/miri.log 2>&1 & disown
```

Expected: `failed 0`, clippy clean, fmt clean, doc clean, pytest green,
miri green. Record the final test count against `$SCRATCH/baseline.txt`.

- [ ] **Step 3: Checkpoint 2 with the user**

Report: commit list, test counts, the verification table (10 templates
× 3 structures: count match and seconds), the budget count, the `stored`
sanity ratio, and anything that deviated from this plan. Landing is the
user's call (the landing memory: merge origin/master in, re-run the
gate, push `HEAD:master`; never rebase). Offer to post the results on
#93 and tick its acceptance boxes, and to update the #93 memory note.

---

## Deviations from the spec, by design

- `topological_order(&precedence, rank)` rather than the spec's
  `topological_order(num_vars, &precedence, rank)`: the precedence knows
  its variable count (`Precedence::num_vars`), so the argument was
  redundant.
- kermit-lab's back-fill of `stored` is a second, join-row-scoped
  registry (`JOIN_AXIS_DEFAULTS`) rather than three entries in the
  structure-scoped one: a `bench ds` row joins nothing and must stay NaN.
- `Task 2` keeps `Precedence::new` as a `pub(crate)` all-pinned
  constructor for `check_attribute_order` and the tests, beside the
  public `for_query`.
