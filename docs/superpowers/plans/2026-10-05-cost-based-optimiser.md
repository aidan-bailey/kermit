# Cost-Based Query Optimiser Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `CostBasedOptimiser` (`--optimiser cost-based`), a port of
#68's estimate-based dynamic programme, and the `Database<R>` catalog in
`kermit::db` that gathers the per-column distinct counts it reads, only
when an optimiser declares it needs them.

**Architecture:** Three layers, built bottom-up.

- **`kermit-algos` statistics** (Tasks 1–2). `StatisticsLevel`,
  `QueryOptimiser::required_statistics()`, `RelationStats::column_distinct`
  and `distinct_per_column`. These stay plain data; nothing touches a
  structure.
- **The `kermit::db` catalog** (Tasks 3–5). `JoinFamily::for_each_tuple`
  lends a relation's tuples. `Database<R>` holds the relations and their
  statistics, and the four entry points take it in place of
  `&BTreeMap<String, R>`. Every caller migrates, with no behaviour change.
- **The optimiser** (Tasks 6–9). A cost model plus a DP over the
  downward-closed variable sets, then a CI fixture with WatDiv statistics,
  the CLI variant and every test hook.

The controller owns the verification beyond CI (Task 10), the docs (Task
11) and the gate (Task 12).

**Tech Stack:** Rust nightly workspace (clap, Criterion), the Nix dev
shell, and Python with uv for the #68 evidence scripts.

**Spec:** [`docs/specs/2026-10-05-cost-based-optimiser-design.md`](../../specs/2026-10-05-cost-based-optimiser-design.md)

---

## Ground rules for every task

- **Paths.** `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/81_18db9218353440cf`.
  `SCRATCH` is the executing session's scratchpad directory.
  `EVID=/tb/Source/Academia/kermit-bench-runs/watdiv-stress-100-prelim-2026-09-10/diagnostics/issue-68`
  holds #68's evidence. **Never `cd`** in a Bash call; use absolute paths,
  `env -C <dir>`, `git -C $WT` or `uv --directory`. Each Bash call starts a
  fresh shell (zsh), so define `WT`, `SCRATCH` and `EVID` at the top of
  every call that uses them.
- **Cargo.** Run it in the **foreground** through the flake with
  `CARGO_BUILD_JOBS=2`, e.g. `CARGO_BUILD_JOBS=2 nix develop $WT --command
  cargo test -p kermit-algos`. A background memory monitor kills
  `run_in_background` cargo jobs. Run anything over 10 minutes (miri) fully
  detached with `setsid nohup … & disown`, writing to a log under
  `$SCRATCH`.
- **Formatting.** Format only with `nix develop $WT --command cargo fmt
  --all`; stable rustfmt rewrites dozens of files. `rustfmt.toml` puts a
  leading `|` on every match arm, and the snippets below already do.
- **Doc comments.** clippy's `doc_markdown` lints them under `-Dwarnings`,
  so backtick every identifier. Both `kermit-algos` and `kermit` set
  `#![deny(missing_docs)]`, so every new public item needs a doc comment.
  Public docs must not link to private items
  (`rustdoc::private_intra_doc_links`).
- **Commits.** Plain conventional commits ending `(#81)`. Never amend, never
  push. Every message ends with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01HYRXJE5mQn8ZJjRgxMur8D
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
  - Do not change any data structure (`kermit-ds`), any join algorithm, or
    `bench ds`'s scan.
  - The existing optimisers change in exactly two ways: they gain the
    defaulted `required_statistics()` without overriding it, and their
    *test* helpers follow the new `CatalogStats::for_query` signature
    (Task 2).
- **Reports.** Keep each implementer's final report under about 40 lines.
  Cover commit SHAs, test counts, mutation-check outcomes, and any
  deviation from this plan with its reason.

## Work packages

| Package | Tasks | Scope | Commits | Done when |
|---|---|---|---|---|
| *Controller* | 0 | Baseline | — | Baseline test counts recorded |
| **P1 — planner statistics** | 1, 2 | `kermit-algos` `optimiser/{stats,mod,cardinality,lexicographic}.rs`, `lib.rs`; one closure in `kermit/src/db.rs` | 2 | `cargo test -p kermit-algos` and `cargo check --workspace --all-targets` green |
| **P2 — the catalog** | 3, 4, 5 | `kermit/src/db.rs`, `kermit/src/db/{database,validation}.rs`, `kermit/src/execution.rs`, 6 test files | 3 | `cargo test -p kermit` green; mutation check recorded |
| *Controller* | — | **Checkpoint 1 with the user**: the catalog refactor, no behaviour change | — | — |
| **P3 — the optimiser** | 6, 7 | `kermit-algos/src/optimiser/{cost_based,ordering,mod}.rs`, `lib.rs`, a new integration test and its fixture | 2 | `cargo test -p kermit-algos` green; mutation checks recorded |
| **P4 — wiring and coverage** | 8, 9 | `Optimiser::CostBased`; `join_tests.rs`, `lubm_cardinalities.rs`, `cli_optimiser_choice.rs`, `watdiv_correctness.rs`, `subject_position_constant.rs` | 2 | `cargo test -p kermit` green, with `lubm_cardinalities` run in `nix develop` |
| *Controller* | 10 | Parity over 124 templates, smoke run, planning time | — | `$SCRATCH/verification.md` written |
| *Controller* | 11 | Docs | 1 | `cargo doc` clean |
| *Controller* | 12 | Gate; **checkpoint 2 with the user** | — | All CI checks green locally |

Run one implementer at a time, because every package commits to the same
branch. Give each implementer the ground rules plus its tasks' full text.

---

## Task 0 [controller]: Baseline

**Files:** none.

- [ ] **Step 1: Confirm the branch is current**

```bash
git -C $WT fetch origin
git -C $WT log --oneline HEAD..origin/master
```

Expected: no output. If origin/master has moved, `git -C $WT merge
origin/master` (never rebase).

- [ ] **Step 2: Green baseline**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace 2>&1 | tee $SCRATCH/baseline-test.log | grep -E '^test result:' | awk '{p+=$4; f+=$6} END {print "passed", p, "failed", f}'
```

Expected: `failed 0`. Record the passed count in `$SCRATCH/baseline.txt`.
If `e2e_watdiv` fails, re-run it alone before treating the failure as real;
it has a known clock-seeding flake.

---

# P1 — planner statistics (`kermit-algos`)

## Task 1 [P1]: Optimisers declare the statistics they read

**Files:**
- Modify: `kermit-algos/src/optimiser/stats.rs`
- Modify: `kermit-algos/src/optimiser/mod.rs`
- Modify: `kermit-algos/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module of `kermit-algos/src/optimiser/stats.rs`:

```rust
    #[test]
    fn statistics_levels_order_cheapest_first() {
        assert!(StatisticsLevel::TupleCounts < StatisticsLevel::ColumnDistinct);
        assert_eq!(StatisticsLevel::TupleCounts.to_string(), "tuple counts");
        assert_eq!(
            StatisticsLevel::ColumnDistinct.to_string(),
            "per-column distinct counts"
        );
    }
```

Add to `optimiser_enum_tests` in `kermit-algos/src/optimiser/mod.rs`. The
match is exhaustive on purpose: adding an `Optimiser` variant fails to
compile until its expected level is written down here.

```rust
    /// Each optimiser declares exactly the statistics it reads; the engine
    /// gathers that much and no more.
    #[test]
    fn each_optimiser_declares_the_statistics_it_reads() {
        for v in Optimiser::value_variants() {
            let want = match v {
                | Optimiser::Lexicographic | Optimiser::Cardinality => StatisticsLevel::TupleCounts,
            };
            assert_eq!(v.instantiate().required_statistics(), want, "{v:?}");
        }
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos statistics`
Expected: a compile error, ``cannot find type `StatisticsLevel` ``.

- [ ] **Step 3: Add `StatisticsLevel`**

In `kermit-algos/src/optimiser/stats.rs`, change the `use` block to:

```rust
use {
    crate::const_rewrite::is_const_predicate,
    kermit_parser::JoinQuery,
    std::{collections::BTreeMap, fmt},
};
```

and add, above `RelationStats`:

```rust
/// How much a planner needs to know about each relation, cheapest first.
///
/// A [`QueryOptimiser`](super::QueryOptimiser) declares its level through
/// `required_statistics`, and the join engine gathers exactly that much
/// when it builds its relation store, so an optimiser that reads only tuple
/// counts never pays for a walk over the data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatisticsLevel {
    /// The number of stored tuples per relation. Free: every structure
    /// keeps it (`kermit_ds::Cardinality`).
    TupleCounts,
    /// Tuple counts plus the number of distinct values in each column. One
    /// walk over every relation.
    ColumnDistinct,
}

impl fmt::Display for StatisticsLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            | StatisticsLevel::TupleCounts => "tuple counts",
            | StatisticsLevel::ColumnDistinct => "per-column distinct counts",
        })
    }
}
```

- [ ] **Step 4: Add the trait method and the re-exports**

In `kermit-algos/src/optimiser/mod.rs`, replace the trait with:

```rust
pub trait QueryOptimiser {
    /// Produces a plan for `query` given per-relation `stats`.
    fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan;

    /// The statistics [`plan`](Self::plan) reads. The join engine gathers
    /// exactly this much before planning, and refuses to plan from less,
    /// so the default (tuple counts, which cost nothing) is right for any
    /// optimiser that reads only [`CatalogStats::tuples`].
    fn required_statistics(&self) -> StatisticsLevel { StatisticsLevel::TupleCounts }
}
```

Change the re-export line `stats::{CatalogStats, RelationStats},` to
`stats::{CatalogStats, RelationStats, StatisticsLevel},`. In
`kermit-algos/src/lib.rs`, add `StatisticsLevel` to the `optimiser::{…}`
list after `RelationStats`.

- [ ] **Step 5: Run the tests to see them pass**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos`
Expected: all pass, including the two new tests.

- [ ] **Step 6: Format, check, commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo check --workspace --all-targets
git -C $WT add kermit-algos/src/optimiser/stats.rs kermit-algos/src/optimiser/mod.rs kermit-algos/src/lib.rs
git -C $WT commit -m "feat(algos): optimisers declare the statistics they read (#81)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HYRXJE5mQn8ZJjRgxMur8D"
```

## Task 2 [P1]: Per-column distinct counts in the planner's statistics

**Files:**
- Modify: `kermit-algos/src/optimiser/stats.rs`
- Modify: `kermit-algos/src/optimiser/mod.rs`, `kermit-algos/src/lib.rs` (re-export)
- Modify: `kermit-algos/src/optimiser/cardinality.rs` (test helper only)
- Modify: `kermit-algos/src/optimiser/lexicographic.rs` (one test only)
- Modify: `kermit/src/db.rs` (the `for_query` closure in `run_join`)

- [ ] **Step 1: Write the failing tests**

Replace the `tests` module of `kermit-algos/src/optimiser/stats.rs`, keeping
Task 1's test, with:

```rust
#[cfg(test)]
mod tests {
    use {super::*, kermit_parser::JoinQuery};

    #[test]
    fn statistics_levels_order_cheapest_first() {
        assert!(StatisticsLevel::TupleCounts < StatisticsLevel::ColumnDistinct);
        assert_eq!(StatisticsLevel::TupleCounts.to_string(), "tuple counts");
        assert_eq!(
            StatisticsLevel::ColumnDistinct.to_string(),
            "per-column distinct counts"
        );
    }

    #[test]
    fn for_query_records_relations_and_const_singletons() {
        let q: JoinQuery = "Q(X) :- R(X, K0), Const_c5(K0).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, |name| match name {
            | "R" => Some(RelationStats::new(42, 2).with_column_distinct(vec![7, 3])),
            | _ => None,
        });
        assert_eq!(stats.tuples("R"), Some(42));
        assert_eq!(stats.distinct("R", 0), Some(7));
        assert_eq!(stats.distinct("R", 1), Some(3));
        assert_eq!(stats.tuples("Const_c5"), Some(1));
        assert_eq!(stats.distinct("Const_c5", 0), Some(1));
        assert_eq!(stats.tuples("Unknown"), None);
    }

    #[test]
    fn distinct_counts_are_absent_at_tuple_counts() {
        let q: JoinQuery = "Q(X) :- R(X).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, |_| Some(RelationStats::new(9, 1)));
        assert_eq!(stats.tuples("R"), Some(9));
        assert_eq!(stats.distinct("R", 0), None);
    }

    #[test]
    fn for_query_skips_unknown_relations() {
        let q: JoinQuery = "Q(X) :- R(X), Mystery(X).".parse().unwrap();
        let stats =
            CatalogStats::for_query(&q, |name| (name == "R").then(|| RelationStats::new(7, 1)));
        assert_eq!(stats.tuples("Mystery"), None);
    }

    #[test]
    fn for_query_looks_up_each_relation_once() {
        use std::cell::Cell;
        let q: JoinQuery = "Q(X, Z) :- R(X, Y), R(Y, Z).".parse().unwrap();
        let lookups = Cell::new(0);
        let stats = CatalogStats::for_query(&q, |name| {
            lookups.set(lookups.get() + 1);
            (name == "R").then(|| RelationStats::new(9, 2))
        });
        assert_eq!(stats.tuples("R"), Some(9));
        assert_eq!(lookups.get(), 1);
    }

    #[test]
    fn distinct_per_column_counts_each_column_separately() {
        let tuples = [vec![1, 10], vec![1, 20], vec![2, 10], vec![3, 30], vec![3, 30]];
        let counts = distinct_per_column(2, |visit| {
            for tuple in &tuples {
                visit(tuple.as_slice());
            }
        });
        assert_eq!(counts, vec![3, 3]);
    }

    #[test]
    fn distinct_per_column_of_an_empty_walk_is_zero() {
        assert_eq!(distinct_per_column(2, |_| {}), vec![0, 0]);
    }

    #[test]
    #[should_panic(expected = "one distinct count per column")]
    fn with_column_distinct_needs_one_count_per_column() {
        let _ = RelationStats::new(4, 2).with_column_distinct(vec![1]);
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos stats`
Expected: compile errors: no function `new` on `RelationStats`, no method
`distinct`, and ``cannot find function `distinct_per_column` ``.

- [ ] **Step 3: Implement**

In `kermit-algos/src/optimiser/stats.rs`:

1. Set the `use` block to:

```rust
use {
    crate::const_rewrite::is_const_predicate,
    kermit_parser::JoinQuery,
    std::{
        collections::{BTreeMap, HashSet},
        fmt,
    },
};
```

2. Append one sentence to the module doc's last paragraph: ``[`distinct_per_column`] counts over a tuple walk the caller lends it, for the same reason.``

3. Replace `RelationStats` with:

```rust
/// Statistics for one relation, as visible to the planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationStats {
    /// Number of stored tuples (`kermit_ds::Cardinality` semantics: what a
    /// full iteration yields).
    pub tuples: usize,
    /// Number of attributes.
    pub arity: usize,
    /// Distinct values in each column, in column order. Empty unless the
    /// statistics were gathered at [`StatisticsLevel::ColumnDistinct`].
    pub column_distinct: Vec<usize>,
}

impl RelationStats {
    /// Tuple-count statistics, with no per-column counts.
    pub fn new(tuples: usize, arity: usize) -> Self {
        Self {
            tuples,
            arity,
            column_distinct: Vec::new(),
        }
    }

    /// These statistics with one distinct-value count per column.
    ///
    /// # Panics
    ///
    /// If `column_distinct` does not hold exactly one count per column.
    pub fn with_column_distinct(self, column_distinct: Vec<usize>) -> Self {
        assert_eq!(
            column_distinct.len(),
            self.arity,
            "one distinct count per column"
        );
        Self {
            column_distinct,
            ..self
        }
    }
}

/// Counts the distinct values in each of `arity` columns over the tuples
/// `walk` lends its visitor: the statistic
/// [`StatisticsLevel::ColumnDistinct`] adds. The caller supplies the walk
/// (typically a relation's own), so this needs no data structure. One
/// `HashSet` per column: O(N · arity) time, O(distinct values) space.
pub fn distinct_per_column(
    arity: usize, walk: impl FnOnce(&mut dyn FnMut(&[usize])),
) -> Vec<usize> {
    let mut seen: Vec<HashSet<usize>> = vec![HashSet::new(); arity];
    walk(&mut |tuple| {
        for (values, &value) in seen.iter_mut().zip(tuple) {
            values.insert(value);
        }
    });
    seen.iter().map(HashSet::len).collect()
}
```

4. In `impl CatalogStats`, add after `tuples`:

```rust
    /// Distinct values in column `column` of `name`, if they were gathered.
    pub fn distinct(&self, name: &str, column: usize) -> Option<usize> {
        self.relations.get(name)?.column_distinct.get(column).copied()
    }
```

and replace `for_query` with:

```rust
    /// Builds stats for every body predicate of `query`.
    ///
    /// `stats_of` supplies a relation's statistics (in `kermit::db`, those
    /// its `Database` gathered when it was built). Synthetic `Const_*`
    /// predicates (introduced by the const-view rewrite) are recorded as
    /// single-tuple unary relations whose one column has one value.
    /// Predicates the lookup does not know get no entry — planners treat
    /// missing stats as "assume large".
    pub fn for_query(
        query: &JoinQuery, stats_of: impl Fn(&str) -> Option<RelationStats>,
    ) -> Self {
        let mut stats = CatalogStats::default();
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
```

5. Re-export: in `kermit-algos/src/optimiser/mod.rs` the line becomes
`stats::{distinct_per_column, CatalogStats, RelationStats, StatisticsLevel},`.
In `kermit-algos/src/lib.rs` add `distinct_per_column` at the start of the
`optimiser::{…}` list.

- [ ] **Step 4: Follow the new signature in the two sibling optimisers' tests**

In `kermit-algos/src/optimiser/cardinality.rs`, change the test module's
`use` to `use {super::*, crate::optimiser::RelationStats, kermit_parser::JoinQuery};`
and replace `stats_for` with:

```rust
    fn stats_for(q: &JoinQuery, sizes: &[(&str, usize)]) -> CatalogStats {
        CatalogStats::for_query(q, |name| {
            let arity = q.body.iter().find(|p| p.name == name)?.terms.len();
            let &(_, tuples) = sizes.iter().find(|(n, _)| *n == name)?;
            Some(RelationStats::new(tuples, arity))
        })
    }
```

In `kermit-algos/src/optimiser/lexicographic.rs`, change the test module's
`use` the same way and the closure in `stats_are_ignored` to:

```rust
        let stats = CatalogStats::for_query(&q, |name| match name {
            | "R" => Some(RelationStats::new(1_000_000, 1)),
            | "S" => Some(RelationStats::new(1, 1)),
            | _ => None,
        });
```

- [ ] **Step 5: Keep `kermit` compiling**

In `kermit/src/db.rs`, add `RelationStats` to the `kermit_algos::{…}` import
and change the `for_query` call in `run_join` to:

```rust
    let stats = CatalogStats::for_query(&rewritten, |name| {
        let base = base_of.get(name).copied().unwrap_or(name);
        relations
            .get(base)
            .map(|r| RelationStats::new(r.tuple_count(), r.header().arity()))
    });
```

Task 5 replaces this.

- [ ] **Step 6: Run the tests**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --lib
```

Expected: all pass.

- [ ] **Step 7: Format, check, commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets -p kermit-algos -p kermit -- -Dwarnings
git -C $WT add kermit-algos/src/optimiser/stats.rs kermit-algos/src/optimiser/mod.rs kermit-algos/src/lib.rs kermit-algos/src/optimiser/cardinality.rs kermit-algos/src/optimiser/lexicographic.rs kermit/src/db.rs
git -C $WT commit -m "feat(algos): per-column distinct counts in the planner's statistics (#81)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HYRXJE5mQn8ZJjRgxMur8D"
```

---

# P2 — the catalog (`kermit::db`)

## Task 3 [P2]: Join families lend their relations' tuples

**Files:**
- Modify: `kermit/src/db.rs`

- [ ] **Step 1: Write the failing tests**

Append to `kermit/src/db.rs`:

```rust
#[cfg(test)]
mod family_walk_tests {
    use {
        super::*,
        kermit_ds::{ColumnTrie, HashTrie, NoPruning, PruningPolicy, SingletonPruning, TreeTrie},
        kermit_iters::SipHashStrategy,
    };

    fn walked<R, F: JoinFamily<R>>(relation: &R) -> Vec<Vec<usize>> {
        let mut tuples = Vec::new();
        F::for_each_tuple(relation, |tuple| tuples.push(tuple.to_vec()));
        tuples
    }

    /// Shared prefixes and single-tuple subtries, so the pruned hash trie
    /// stores some subtries as `Singleton`s.
    fn tuples() -> Vec<Vec<usize>> {
        vec![
            vec![1, 10, 100],
            vec![1, 10, 101],
            vec![1, 20, 100],
            vec![2, 30, 300],
            vec![3, 40, 400],
        ]
    }

    #[test]
    fn sorted_walk_visits_every_tuple_in_order() {
        let tree: TreeTrie = TreeTrie::from_tuples(3.into(), tuples());
        assert_eq!(walked::<_, SortedFamily>(&tree), tuples());
        let column: ColumnTrie = ColumnTrie::from_tuples(3.into(), tuples());
        assert_eq!(walked::<_, SortedFamily>(&column), tuples());
    }

    /// The family walk goes through `HashTrieIterator` and must visit
    /// exactly what the trie's own `for_each_tuple` visits.
    fn assert_hash_walk_matches_the_trie<P: PruningPolicy>(tuples: Vec<Vec<usize>>) {
        let trie: HashTrie<SipHashStrategy, P> = HashTrie::from_tuples(3.into(), tuples);
        let mut expected = Vec::new();
        trie.for_each_tuple(|tuple| expected.push(tuple.to_vec()));
        let mut got = walked::<_, HashFamily<SipHashStrategy>>(&trie);
        expected.sort();
        got.sort();
        assert_eq!(got, expected);
    }

    #[test]
    fn hash_walk_visits_what_the_trie_stores() {
        assert_hash_walk_matches_the_trie::<NoPruning>(tuples());
        assert_hash_walk_matches_the_trie::<SingletonPruning>(tuples());
        // The hash trie is a multiset: a duplicate is visited twice.
        let mut duplicated = tuples();
        duplicated.push(vec![1, 10, 100]);
        assert_hash_walk_matches_the_trie::<NoPruning>(duplicated.clone());
        assert_hash_walk_matches_the_trie::<SingletonPruning>(duplicated);
        assert_hash_walk_matches_the_trie::<NoPruning>(Vec::new());
        assert_hash_walk_matches_the_trie::<SingletonPruning>(Vec::new());
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --lib family_walk_tests`
Expected: a compile error, ``no function or associated item named `for_each_tuple` ``.

- [ ] **Step 3: Implement**

In `kermit/src/db.rs`, change the `kermit_iters` import to
`kermit_iters::{HashStrategy, HashTrieIterable, HashTrieIterator, JoinIterable, TrieIterable, TrieIteratorWrapper},`.

Add the last method to the `JoinFamily` trait:

```rust
    /// Lends every tuple stored in `relation` to `visit`, in the family's
    /// native order, without allocating per tuple. The catalog walks
    /// relations through it to count distinct values, so it needs no
    /// structure-specific code.
    fn for_each_tuple(relation: &R, visit: impl FnMut(&[usize]));
```

In `impl<R: TrieIterable> JoinFamily<R> for SortedFamily`:

```rust
    fn for_each_tuple(relation: &R, mut visit: impl FnMut(&[usize])) {
        let mut tuples = TrieIteratorWrapper::new(relation.trie_iter());
        while let Some(tuple) = tuples.advance() {
            visit(tuple);
        }
    }
```

In `impl<R: HashTrieIterable, H: HashStrategy> JoinFamily<R> for HashFamily<H>`:

```rust
    fn for_each_tuple(relation: &R, mut visit: impl FnMut(&[usize])) {
        for_each_hash_tuple(relation.hash_trie_iter(), &mut visit);
    }
```

and after that impl block, the walk:

```rust
/// Depth-first walk of a hash trie through its [`HashTrieIterator`]: the
/// `open` / `next` / `up` / `leaf_tuples` contract Hash Triejoin itself
/// relies on, so it serves every hash-family relation, pruned singletons
/// included.
fn for_each_hash_tuple(mut iter: impl HashTrieIterator, visit: &mut impl FnMut(&[usize])) {
    // From before the root, `open` enters the root level, and fails only on
    // an empty relation. `depth` counts the levels entered, so the walk is
    // over once it climbs back out of the root.
    if !iter.open() {
        return;
    }
    let mut depth = 1;
    loop {
        let at_leaf = match iter.leaf_tuples() {
            | Some(chain) => {
                for tuple in chain {
                    visit(tuple);
                }
                true
            },
            | None => false,
        };
        // A built trie has no empty inner node, so `open` succeeds on every
        // inner bucket.
        if !at_leaf && iter.open() {
            depth += 1;
            continue;
        }
        // Advance to the next bucket, climbing out of each exhausted level.
        while iter.next().is_none() {
            iter.up();
            depth -= 1;
            if depth == 0 {
                return;
            }
        }
    }
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --lib`
Expected: all pass, including the 2 new tests.

- [ ] **Step 5: Format, check, commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets -p kermit -- -Dwarnings
git -C $WT add kermit/src/db.rs
git -C $WT commit -m "feat(db): join families lend their relations' tuples (#81)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HYRXJE5mQn8ZJjRgxMur8D"
```

- [ ] **Step 6: Mutation check**

Mutant: in `for_each_hash_tuple`, replace `if !at_leaf && iter.open() {`
with `if iter.open() {`. Expected: `hash_walk_visits_what_the_trie_stores`
fails, with tuples visited twice or a panic. Follow the ground-rule
protocol.

## Task 4 [P2]: `Database<R>`, relations plus planner statistics

**Files:**
- Create: `kermit/src/db/database.rs`
- Modify: `kermit/src/db.rs` (module declaration and re-export only)

- [ ] **Step 1: Write the type's tests first**

Create `kermit/src/db/database.rs` containing only the test module, below,
and add `mod database;` + `pub use database::Database;` to `kermit/src/db.rs`
next to `mod validation;`.

```rust
#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::db::{HashFamily, SortedFamily},
        kermit_algos::{ColumnEquality, TrieIterKind},
        kermit_ds::{ColumnTrie, HashTrie, SingletonPruning, TreeTrie},
        kermit_iters::{SipHashStrategy, TrieIterable},
    };

    /// A sorted family whose walk panics, so a database built at
    /// `TupleCounts` provably never walks a relation.
    struct NoWalk;

    impl<R: TrieIterable> JoinFamily<R> for NoWalk {
        type Wrapper<'a>
            = TrieIterKind<'a, R>
        where
            R: 'a;

        fn wrap_relation(_: &R) -> Self::Wrapper<'_> { unreachable!() }

        fn wrap_const<'a>(_: usize) -> Self::Wrapper<'a>
        where
            R: 'a,
        {
            unreachable!()
        }

        fn wrap_selection(_: &R, _: Vec<ColumnEquality>) -> Self::Wrapper<'_> { unreachable!() }

        fn for_each_tuple(_: &R, _: impl FnMut(&[usize])) {
            panic!("a database built at TupleCounts walked a relation")
        }
    }

    fn edges() -> Vec<Vec<usize>> { vec![vec![1, 10], vec![1, 20], vec![2, 10], vec![3, 30]] }

    fn store<R: Relation>(tuples: Vec<Vec<usize>>) -> BTreeMap<String, R> {
        BTreeMap::from([("edge".to_string(), R::from_tuples(2.into(), tuples))])
    }

    #[test]
    fn tuple_counts_read_cardinality_and_walk_nothing() {
        let database =
            Database::new::<NoWalk>(store::<TreeTrie>(edges()), StatisticsLevel::TupleCounts);
        assert_eq!(database.level(), StatisticsLevel::TupleCounts);
        assert_eq!(database.statistics("edge"), Some(&RelationStats::new(4, 2)));
    }

    #[test]
    fn a_map_converts_to_a_tuple_count_database() {
        let database = Database::from(store::<TreeTrie>(edges()));
        assert_eq!(database.level(), StatisticsLevel::TupleCounts);
        assert_eq!(database.statistics("edge"), Some(&RelationStats::new(4, 2)));
    }

    /// Every family reports the same statistics for the same tuples. The
    /// hash trie is a multiset, so a duplicate tuple adds to its tuple
    /// count but not to any column's distinct count.
    #[test]
    fn column_distinct_counts_each_column_in_every_family() {
        let level = StatisticsLevel::ColumnDistinct;
        let want = RelationStats::new(4, 2).with_column_distinct(vec![3, 3]);

        let tree = Database::new::<SortedFamily>(store::<TreeTrie>(edges()), level);
        assert_eq!(tree.level(), level);
        assert_eq!(tree.statistics("edge"), Some(&want));
        let column = Database::new::<SortedFamily>(store::<ColumnTrie>(edges()), level);
        assert_eq!(column.statistics("edge"), Some(&want));
        let hash = Database::new::<HashFamily<SipHashStrategy>>(
            store::<HashTrie<SipHashStrategy>>(edges()),
            level,
        );
        assert_eq!(hash.statistics("edge"), Some(&want));
        let pruned = Database::new::<HashFamily<SipHashStrategy>>(
            store::<HashTrie<SipHashStrategy, SingletonPruning>>(edges()),
            level,
        );
        assert_eq!(pruned.statistics("edge"), Some(&want));

        let mut duplicated = edges();
        duplicated.push(vec![1, 10]);
        let multiset = Database::new::<HashFamily<SipHashStrategy>>(
            store::<HashTrie<SipHashStrategy>>(duplicated),
            level,
        );
        assert_eq!(
            multiset.statistics("edge"),
            Some(&RelationStats::new(5, 2).with_column_distinct(vec![3, 3]))
        );
    }

    #[test]
    fn relations_are_found_by_name() {
        let database = Database::from(store::<TreeTrie>(edges()));
        assert!(database.get("edge").is_some());
        assert!(database.get("nope").is_none());
        assert_eq!(database.relations().count(), 1);
        assert_eq!(database.statistics("nope"), None);
    }

    #[test]
    fn arities_agree_with_the_map_they_wrap() {
        let map = store::<TreeTrie>(edges());
        let (arity, names) = (map.arity("edge"), map.relation_names());
        let database = Database::from(map);
        assert_eq!(database.arity("edge"), arity);
        assert_eq!(database.arity("nope"), None);
        assert_eq!(database.relation_names(), names);
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --lib database`
Expected: a compile error, ``cannot find type `Database` ``.

- [ ] **Step 3: Implement**

Put above the test module in `kermit/src/db/database.rs`:

```rust
//! The relation store the join entry points read.

use {
    super::{JoinFamily, RelationArities},
    kermit_algos::{distinct_per_column, RelationStats, StatisticsLevel},
    kermit_ds::{Cardinality, Relation},
    std::collections::BTreeMap,
};

/// Relations by name, plus the statistics a query optimiser reads.
///
/// A database is a catalog in the textbook sense: planners read its
/// statistics, never its relations. The statistics are gathered once, when
/// the database is built ("ANALYZE"), up to the [`StatisticsLevel`] asked
/// for, and the database has no mutation API, so they cannot drift from
/// the relations. Build it at the level the optimiser declares
/// ([`QueryOptimiser::required_statistics`]). The join entry points return
/// [`JoinError::MissingStatistics`] rather than plan from less.
///
/// [`QueryOptimiser::required_statistics`]: kermit_algos::QueryOptimiser::required_statistics
/// [`JoinError::MissingStatistics`]: super::JoinError::MissingStatistics
pub struct Database<R> {
    relations: BTreeMap<String, R>,
    statistics: BTreeMap<String, RelationStats>,
    level: StatisticsLevel,
}

impl<R: Relation + Cardinality> Database<R> {
    /// Builds a database over `relations`, keyed by the names queries use,
    /// gathering statistics up to `level`.
    ///
    /// Tuple counts come from [`Cardinality::tuple_count`], which every
    /// structure keeps, so they cost nothing.
    /// [`StatisticsLevel::ColumnDistinct`] also walks each relation once
    /// through `F` ([`JoinFamily::for_each_tuple`]) to count each column's
    /// distinct values: O(N · arity) time, O(distinct values) space.
    pub fn new<F: JoinFamily<R>>(relations: BTreeMap<String, R>, level: StatisticsLevel) -> Self {
        let mut database = Self::from(relations);
        if level >= StatisticsLevel::ColumnDistinct {
            for (name, stats) in &mut database.statistics {
                let relation = &database.relations[name];
                stats.column_distinct = distinct_per_column(stats.arity, |visit| {
                    F::for_each_tuple(relation, visit)
                });
            }
        }
        database.level = level;
        database
    }
}

impl<R> Database<R> {
    /// The relation queries call `name`, if the database holds one.
    pub fn get(&self, name: &str) -> Option<&R> { self.relations.get(name) }

    /// Every relation, in name order.
    pub fn relations(&self) -> impl Iterator<Item = &R> { self.relations.values() }

    /// The statistics gathered for `name`, if the database holds it.
    pub fn statistics(&self, name: &str) -> Option<&RelationStats> { self.statistics.get(name) }

    /// How far the statistics go.
    pub fn level(&self) -> StatisticsLevel { self.level }
}

/// A database with tuple counts only ([`StatisticsLevel::TupleCounts`]):
/// free to build, and all that an optimiser with the default
/// `required_statistics` reads.
impl<R: Relation + Cardinality> From<BTreeMap<String, R>> for Database<R> {
    fn from(relations: BTreeMap<String, R>) -> Self {
        let statistics = relations
            .iter()
            .map(|(name, relation)| {
                let stats = RelationStats::new(relation.tuple_count(), relation.header().arity());
                (name.clone(), stats)
            })
            .collect();
        Self {
            relations,
            statistics,
            level: StatisticsLevel::TupleCounts,
        }
    }
}

impl<R: Relation> RelationArities for Database<R> {
    fn arity(&self, relation: &str) -> Option<usize> { self.relations.arity(relation) }

    fn relation_names(&self) -> Vec<String> { self.relations.relation_names() }
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --lib database`
Expected: 5 passed.

- [ ] **Step 5: Format, check, commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets -p kermit -- -Dwarnings
git -C $WT add kermit/src/db.rs kermit/src/db/database.rs
git -C $WT commit -m "feat(db): Database<R>, relations plus planner statistics (#81)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HYRXJE5mQn8ZJjRgxMur8D"
```

- [ ] **Step 6: Mutation check**

Mutant: in `Database::new`, change `if level >= StatisticsLevel::ColumnDistinct {`
to `if true {`. Expected: `tuple_counts_read_cardinality_and_walk_nothing`
panics with "walked a relation".

## Task 5 [P2]: The entry points read a `Database`

The one breaking change. Afterwards every existing optimiser runs exactly
as before (at `TupleCounts`). Use an opus implementer.

**Files:**
- Modify: `kermit/src/db/validation.rs` (`JoinError::MissingStatistics`)
- Modify: `kermit/src/db.rs` (module doc, `run_join`, the four entry points, tests)
- Modify: `kermit/src/execution.rs` (`Engine = Database<R>`, one test fix, one new test)
- Modify: `kermit/tests/common/utils.rs`, `kermit/tests/lubm_cardinalities.rs`,
  `kermit/tests/lubm_mini_oracle.rs`, `kermit/tests/watdiv_correctness.rs`,
  `kermit/tests/subject_position_constant.rs`, `kermit/tests/result_allocation.rs`

- [ ] **Step 1: Write the failing tests**

In `kermit/src/db.rs`'s `tests` module, extend the `use` to:

```rust
    use {
        super::*,
        kermit_algos::{
            CatalogStats, JoinQuery, LeapfrogTriejoin, LexicographicOptimiser, QueryPlan,
            StatisticsLevel,
        },
        kermit_ds::{Relation, TreeTrie},
        std::collections::BTreeMap,
    };
```

replace `rels` with:

```rust
    fn rel_map(entries: Vec<(&str, usize, Vec<Vec<usize>>)>) -> BTreeMap<String, TreeTrie> {
        entries
            .into_iter()
            .map(|(name, arity, tuples)| {
                (
                    name.to_string(),
                    TreeTrie::from_tuples(arity.into(), tuples),
                )
            })
            .collect()
    }

    fn rels(entries: Vec<(&str, usize, Vec<Vec<usize>>)>) -> Database<TreeTrie> {
        Database::from(rel_map(entries))
    }
```

and add:

```rust
    /// Plans like `lexicographic` but declares that it reads per-column
    /// distinct counts.
    struct NeedsColumns;

    impl QueryOptimiser for NeedsColumns {
        fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan {
            LexicographicOptimiser.plan(query, stats)
        }

        fn required_statistics(&self) -> StatisticsLevel { StatisticsLevel::ColumnDistinct }
    }

    /// An optimiser that reads more than the database gathered is refused
    /// before any row, and the same relations analysed to its level run.
    #[test]
    fn an_optimiser_reading_ungathered_statistics_is_refused() {
        let entries = || {
            vec![
                ("first", 1, vec![vec![1], vec![2], vec![3]]),
                ("second", 1, vec![vec![2], vec![3], vec![4]]),
            ]
        };
        let query: JoinQuery = "Q(X) :- first(X), second(X).".parse().unwrap();
        let mut rows = 0;
        let refused = lftj_join_for_each::<TreeTrie, LeapfrogTriejoin>(
            &rels(entries()),
            query.clone(),
            &NeedsColumns,
            |_| rows += 1,
        );
        assert_eq!(
            refused,
            Err(JoinError::MissingStatistics {
                required: StatisticsLevel::ColumnDistinct,
                available: StatisticsLevel::TupleCounts,
            })
        );
        assert_eq!(rows, 0);

        let analysed =
            Database::new::<SortedFamily>(rel_map(entries()), StatisticsLevel::ColumnDistinct);
        let mut got =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&analysed, query, &NeedsColumns).unwrap();
        got.sort();
        assert_eq!(got, vec![vec![2], vec![3]]);
    }
```

In `kermit/src/execution.rs`'s `tests` module, add:

```rust
    /// Each engine gathers exactly the statistics its optimiser reads,
    /// along both build paths: `iteration` never pays for a walk, and
    /// `end_to_end` pays only for one its optimiser needs.
    #[test]
    fn engines_gather_the_statistics_their_optimiser_reads() {
        let header = || RelationHeader::new_positional("edge", 2);
        let edges = || vec![vec![1, 2], vec![1, 3], vec![2, 3]];
        for &optimiser in Optimiser::value_variants() {
            let want = optimiser.instantiate().required_statistics();
            let tree = TrieLftj::<TreeTrie>::new((), optimiser);
            let built = tree.build(vec![tree.build_relation(header(), edges())]);
            assert_eq!(built.level(), want, "{optimiser:?}");
            assert_eq!(tree.build_from_tuples(vec![(header(), edges())]).level(), want);
            let column = TrieLftj::<ColumnTrie>::new(ColumnTrieBuildMode::default(), optimiser);
            assert_eq!(column.build_from_tuples(vec![(header(), edges())]).level(), want);
            let hash =
                HashHtj::<SipHashStrategy, NoPruning>::new(HashTrieConfig::default(), optimiser);
            let built = hash.build(vec![hash.build_relation(header(), edges())]);
            assert_eq!(built.level(), want, "{optimiser:?}");
            assert_eq!(hash.build_from_tuples(vec![(header(), edges())]).level(), want);
        }
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --lib`
Expected: compile errors: no variant `MissingStatistics`, mismatched types
(`&Database<_>` where `&BTreeMap<_, _>` is expected), and no method `level`
on `BTreeMap`.

- [ ] **Step 3: Add `JoinError::MissingStatistics`**

In `kermit/src/db/validation.rs`, add `StatisticsLevel` to the
`kermit_algos::{…}` import, then add the last variant of `JoinError`:

```rust
    /// The query optimiser reads statistics the database was not built
    /// with. A library-usage error, not a malformed query: build the
    /// [`Database`](super::Database) at the optimiser's
    /// [`required_statistics`](kermit_algos::QueryOptimiser::required_statistics),
    /// as the CLI always does.
    MissingStatistics {
        /// What the optimiser reads.
        required: StatisticsLevel,
        /// What the database gathered.
        available: StatisticsLevel,
    },
```

and its `Display` arm, last in the match:

```rust
            | JoinError::MissingStatistics {
                required,
                available,
            } => write!(
                f,
                "the query optimiser reads {required}, but the database gathered only \
                 {available}; build the database at the optimiser's required statistics"
            ),
```

Also update the enum's doc. It currently says "Every variant but
`CyclicAttributeOrder` means the query itself is malformed". Change it to
"Every variant but `CyclicAttributeOrder` and `MissingStatistics` means the
query itself is malformed", with the same link style.

- [ ] **Step 4: Switch `run_join` and the entry points to `Database`**

In `kermit/src/db.rs`:

1. Imports: `std::collections::{BTreeMap, HashMap}` becomes
   `std::collections::HashMap`, and drop the `RelationStats` that Task 2
   added. The test modules now import `BTreeMap` themselves, as the Step 1
   snippet does for `tests`; add `std::collections::BTreeMap` to
   `hash_join_tests`'s `use` too.
2. In the module doc, replace "all over the same shape of relation store,
   a `BTreeMap<String, R>` keyed by relation name." with "all over a
   [`Database`]: the relations, keyed by name, plus the statistics
   planners read, gathered once when it is built."
3. Replace `run_join`'s signature and its body up to `let plan = …` with:

```rust
fn run_join<'a, R, F, JA, S>(
    database: &'a Database<R>, query: JoinQuery, optimiser: &dyn QueryOptimiser, mut emit: S,
) -> Result<(), JoinError>
where
    R: Relation + Cardinality + 'a,
    F: JoinFamily<R>,
    JA: JoinAlgo<F::Wrapper<'a>>,
    S: FnMut(&[usize]),
{
    let head_len = query.head.terms.len();
    let Prepared {
        query: rewritten,
        const_specs,
        selection_specs,
    } = prepare(&query, database)?;

    // The plan comes from the database's statistics, so the optimiser must
    // not read more than were gathered when it was built.
    let required = optimiser.required_statistics();
    if required > database.level() {
        return Err(JoinError::MissingStatistics {
            required,
            available: database.level(),
        });
    }

    // `prepare` has checked that every relation the body names is present.
    let lookup = move |name: &str| -> &'a R {
        database
            .get(name)
            .expect("`prepare` checked that the database holds every body relation")
    };

    let mut wrappers: HashMap<String, F::Wrapper<'a>> = HashMap::new();
    for pred in &rewritten.body {
        if wrappers.contains_key(&pred.name) {
            continue;
        }
        // Const_* and Select_* predicates are synthetic — created by the
        // rewrites above and materialised from their specs below. They
        // aren't expected to live in the database.
        if is_const_predicate(&pred.name) || is_selection_predicate(&pred.name) {
            continue;
        }
        wrappers.insert(pred.name.clone(), F::wrap_relation(lookup(&pred.name)));
    }
    for (name, id) in const_specs {
        wrappers.entry(name).or_insert_with(|| F::wrap_const(id));
    }
    for spec in &selection_specs {
        let base = lookup(&spec.relation);
        wrappers.insert(
            spec.name.clone(),
            F::wrap_selection(base, spec.equalities.clone()),
        );
    }

    let ds_map: HashMap<String, &F::Wrapper<'a>> =
        wrappers.iter().map(|(k, v)| (k.clone(), v)).collect();

    // Stats + planning run per join — inside benchmarks' measured region —
    // so this stays O(#predicates) reads of statistics the database
    // gathered when it was built. A selection view reports its base
    // relation's statistics: upper bounds, the conservative values for a
    // size-driven planner.
    let base_of: HashMap<&str, &str> = selection_specs
        .iter()
        .map(|s| (s.name.as_str(), s.relation.as_str()))
        .collect();
    let stats = CatalogStats::for_query(&rewritten, |name| {
        let base = base_of.get(name).copied().unwrap_or(name);
        database.statistics(base).cloned()
    });
    let plan = optimiser.plan(&rewritten, &stats);
```

   Keep the rest of the body (the projection comment and the
   `JA::join_for_each` call) as it is.

4. In each of `lftj_join_for_each`, `lftj_join`, `hash_join_for_each` and
   `hash_join`:
   - the parameter `relations: &BTreeMap<String, R>` becomes
     `database: &Database<R>`;
   - pass `database` where the body passed `relations`;
   - "over `relations`" becomes "over `database`" in the first doc
     sentence and under `# Errors`.

   In the two `_for_each` functions, the `# Errors` section becomes:

```rust
/// # Errors
///
/// Returns a [`JoinError`], before emitting anything, if the query cannot
/// run over `database` (see [`validate_query`]), or if `optimiser` reads
/// statistics `database` was not built with
/// ([`JoinError::MissingStatistics`]).
```

5. Tests in `kermit/src/db.rs`:
   - In `tests`, `rels` now returns a `Database`, so most tests compile
     unchanged.
   - In `hash_join_tests`, every test that builds `let mut relations:
     BTreeMap<String, HashTrie> = …; relations.insert(…);` gains, after its
     last `insert`, the line `let relations = Database::from(relations);`.
   - In `both_families_reject_malformed_queries_identically`, wrap both
     maps: `let hash: Database<HashTrie> = Database::from(BTreeMap::from([…]));`
     and `let sorted: Database<TreeTrie> = Database::from(BTreeMap::from([…]));`.

- [ ] **Step 5: Engines are databases**

In `kermit/src/execution.rs`:

1. The import becomes
   `kermit::db::{hash_join_for_each, lftj_join_for_each, Database, HashFamily, JoinError, SortedFamily},`.
2. In `impl ExecutionFamily for TrieLftj<R>`:

```rust
    /// The relations, keyed by name, plus the statistics this family's
    /// optimiser reads, gathered here once so no timed join pays for them.
    type Engine = Database<R>;

    fn build(&self, relations: Vec<R>) -> Self::Engine {
        let relations = relations
            .into_iter()
            .map(|r| (r.header().name().to_string(), r))
            .collect();
        Database::new::<SortedFamily>(relations, self.optimiser.required_statistics())
    }

    fn build_from_tuples(&self, inputs: Vec<(RelationHeader, Vec<Vec<usize>>)>) -> Self::Engine {
        let relations = inputs
            .into_iter()
            .map(|(header, tuples)| {
                let name = header.name().to_string();
                (name, self.build_relation(header, tuples))
            })
            .collect();
        Database::new::<SortedFamily>(relations, self.optimiser.required_statistics())
    }

    fn relations(engine: &Self::Engine) -> Vec<&R> { engine.relations().collect() }
```

3. The same in `impl ExecutionFamily for HashHtj<H, P>`, with
   `type Engine = Database<HashTrie<H, P>>;`, `HashFamily<H>` in place of
   `SortedFamily`, and
   `fn relations(engine: &Self::Engine) -> Vec<&HashTrie<H, P>> { engine.relations().collect() }`.
4. In test `hash_family_builds_relations_with_its_config`, `let rel =
   &engine["r"];` becomes `let rel = engine.get("r").unwrap();`.

- [ ] **Step 6: Migrate the integration tests**

1. `kermit/tests/common/utils.rs`. The import becomes
   `kermit::db::{hash_join, hash_join_for_each, lftj_join, lftj_join_for_each, Database, HashFamily, JoinError, SortedFamily},`
   and `kermit_algos::{HashTriejoin, JoinQuery, LeapfrogTriejoin, QueryOptimiser, StatisticsLevel},`.
   Replace the trait and its three impls with:

```rust
/// The entry point in `kermit::db` that runs algorithm `Self` over `R`.
pub trait JoinEntry<R> {
    /// The database the entry point reads, with statistics up to `level`.
    fn database(relations: BTreeMap<String, R>, level: StatisticsLevel) -> Database<R>;

    fn join(
        database: &Database<R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Result<Vec<Vec<usize>>, JoinError>;

    /// Counts the result through the streaming `_for_each` entry point —
    /// the path `bench run`'s `iteration` metric times.
    fn count(
        database: &Database<R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Result<usize, JoinError>;
}

impl<R: TrieIterable + Relation + Cardinality> JoinEntry<R> for LeapfrogTriejoin {
    fn database(relations: BTreeMap<String, R>, level: StatisticsLevel) -> Database<R> {
        Database::new::<SortedFamily>(relations, level)
    }

    fn join(
        database: &Database<R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        lftj_join::<R, LeapfrogTriejoin>(database, query, optimiser)
    }

    fn count(
        database: &Database<R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        lftj_join_for_each::<R, LeapfrogTriejoin>(database, query, optimiser, |_| rows += 1)?;
        Ok(rows)
    }
}

/// `hash_join` needs the relation's hash strategy `H` for constant
/// singletons, so the hash-family impls are per concrete relation type
/// rather than blanket over `HashTrieIterable`.
impl<H: HashStrategy, P: PruningPolicy> JoinEntry<HashTrie<H, P>> for HashTriejoin {
    fn database(
        relations: BTreeMap<String, HashTrie<H, P>>, level: StatisticsLevel,
    ) -> Database<HashTrie<H, P>> {
        Database::new::<HashFamily<H>>(relations, level)
    }

    fn join(
        database: &Database<HashTrie<H, P>>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        hash_join::<HashTrie<H, P>, H>(database, query, optimiser)
    }

    fn count(
        database: &Database<HashTrie<H, P>>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        hash_join_for_each::<HashTrie<H, P>, H>(database, query, optimiser, |_| rows += 1)?;
        Ok(rows)
    }
}

impl<H: HashStrategy, P: PruningPolicy, C: ConfigProvider<HashTrieConfig>>
    JoinEntry<Configured<HashTrie<H, P>, C>> for HashTriejoin
{
    fn database(
        relations: BTreeMap<String, Configured<HashTrie<H, P>, C>>, level: StatisticsLevel,
    ) -> Database<Configured<HashTrie<H, P>, C>> {
        Database::new::<HashFamily<H>>(relations, level)
    }

    fn join(
        database: &Database<Configured<HashTrie<H, P>, C>>, query: JoinQuery,
        optimiser: &dyn QueryOptimiser,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        hash_join::<Configured<HashTrie<H, P>, C>, H>(database, query, optimiser)
    }

    fn count(
        database: &Database<Configured<HashTrie<H, P>, C>>, query: JoinQuery,
        optimiser: &dyn QueryOptimiser,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        hash_join_for_each::<Configured<HashTrie<H, P>, C>, H>(
            database,
            query,
            optimiser,
            |_| rows += 1,
        )?;
        Ok(rows)
    }
}
```

   In `test_join`, after `let query: JoinQuery = …;`, the two entry-point
   calls become:

```rust
    // The database is analysed to exactly what the optimiser reads, as the
    // CLI's engines are.
    let optimiser = O::default();
    let database = JA::database(relations, optimiser.required_statistics());

    // The streamed count is what `bench run --verify` checks and what the
    // `iteration` metric times; it must agree with the expected rows.
    let streamed = JA::count(&database, query.clone(), &optimiser)
        .unwrap_or_else(|e| panic!("{query_str}: {e}"));
```

   and further down, `JA::join(&database, query, &optimiser)`. Add a
   sentence to `test_join`'s doc: "The relations are put in a `Database`
   built at the optimiser's `required_statistics()`, as the CLI's engines
   are."

2. `kermit/tests/lubm_cardinalities.rs`. Import
   `kermit::db::{lftj_join, Database, SortedFamily}`. In
   `cardinality_mismatches`, after the loop that fills `relations`, add
   `let relations = Database::new::<SortedFamily>(relations, optimiser.required_statistics());`.

3. `kermit/tests/lubm_mini_oracle.rs`. The same import. In
   `cardinality_mismatches`, after the fill loop, add
   `let relations = Database::new::<SortedFamily>(relations, planner.required_statistics());`.

4. `kermit/tests/watdiv_correctness.rs`. Import
   `kermit::db::{lftj_join, Database}`. After the fill loop, add
   `let relations = Database::from(relations);`. Task 9 parameterises this
   file.

5. `kermit/tests/subject_position_constant.rs`. Import
   `kermit::db::{hash_join, lftj_join, Database}`. The helpers become:

```rust
/// edge = {(1,2), (1,3), (2,4)} as a `TreeTrie` database (LFTJ path).
fn edge_tries() -> Database<TreeTrie> {
    let edge = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
    Database::from(BTreeMap::from([("edge".to_string(), edge)]))
}

/// edge = {(1,2), (1,3), (2,4)} as a `HashTrie` database (hash path).
fn edge_rels() -> Database<HashTrieSip> {
    let edge = HashTrieSip::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
    Database::from(BTreeMap::from([("edge".to_string(), edge)]))
}
```

   Task 9 parameterises this file.

6. `kermit/tests/result_allocation.rs`. Import
   `kermit::db::{hash_join_for_each, lftj_join_for_each, Database}`.
   - `lftj_join_allocations` takes `relations: BTreeMap<String, Rel>` by
     value, and `htj_join_allocations` takes
     `relations: BTreeMap<String, HashTrie<H, P>>` by value.
   - Each helper's first line becomes
     `let database = Database::from(relations);`, which builds the database
     before `allocation_counter::measure`, so its allocations are not
     counted.
   - The entry-point call inside `measure` passes `&database`.
   - Drop the `&` at the four call sites: `lftj_allocations`,
     `htj_allocations`, `lftj_descent_allocations` and
     `htj_descent_allocations`.

   Doc comments that say "over `relations`" stay accurate.

- [ ] **Step 7: Run the whole crate**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit`
Expected: all pass. The count is the baseline's `kermit` count plus 2
(Task 3), plus 5 (Task 4), plus 2 (this task). `lubm_cardinalities` skips
outside `nix develop`, and this command runs inside it, so it runs in about
1 minute.

- [ ] **Step 8: Format, check, commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets -p kermit -- -Dwarnings
git -C $WT add kermit/src/db.rs kermit/src/db/validation.rs kermit/src/execution.rs kermit/tests/common/utils.rs kermit/tests/lubm_cardinalities.rs kermit/tests/lubm_mini_oracle.rs kermit/tests/watdiv_correctness.rs kermit/tests/subject_position_constant.rs kermit/tests/result_allocation.rs
git -C $WT commit -m "refactor(db): the join entry points read a Database (#81)" -m "Relations and the planner statistics gathered when the database is built
replace the bare BTreeMap. Every existing optimiser reads tuple counts
only, so no statistics walk runs and no behaviour changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HYRXJE5mQn8ZJjRgxMur8D"
```

- [ ] **Step 9: Mutation check**

Mutant: in `run_join`, change `if required > database.level() {` to
`if false {`. Expected: `an_optimiser_reading_ungathered_statistics_is_refused`
fails.

## Controller checkpoint 1 (after Task 5)

- Verify the tree yourself: `git -C $WT status --porcelain` is empty, and
  `git -C $WT log --oneline -6` shows Tasks 1–5.
- Run `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace`.
  Expected: the baseline count plus the new tests, and 0 failed.
- Tell the user in a few lines that the catalog refactor has landed on the
  branch and changes no behaviour, then continue. This is a status line,
  not an approval gate: the user approved the design and expects
  continuous execution.

---

# P3 — the optimiser

## Task 6 [P3]: `CostBasedOptimiser`

Use an opus implementer.

**Files:**
- Modify: `kermit-algos/src/optimiser/ordering.rs` (`Precedence` becomes
  `pub(crate)` and gains `predecessor_masks`)
- Create: `kermit-algos/src/optimiser/cost_based.rs`
- Modify: `kermit-algos/src/optimiser/mod.rs`, `kermit-algos/src/lib.rs` (module and re-export)

- [ ] **Step 1: Write the failing `predecessor_masks` tests**

Add to `ordering.rs`'s `tests` module:

```rust
    #[test]
    fn predecessor_masks_mirror_the_edges() {
        // Triangle: R(0,1), S(1,2), T(0,2) — edges 0->1, 1->2, 0->2.
        let preds = vec![vec![0, 1], vec![1, 2], vec![0, 2]];
        let masks = Precedence::new(3, &preds).predecessor_masks().unwrap();
        assert_eq!(masks, vec![0b000, 0b001, 0b011]);
    }

    #[test]
    fn predecessor_masks_stop_at_64_variables() {
        let preds: Vec<Vec<usize>> = (0..65).map(|v| vec![v]).collect();
        assert!(Precedence::new(65, &preds).predecessor_masks().is_none());
        let masks = Precedence::new(64, &preds[..64]).predecessor_masks().unwrap();
        assert_eq!(masks.len(), 64);
    }
```

- [ ] **Step 2: Implement `predecessor_masks`**

In `ordering.rs`, make the struct `pub(crate) struct Precedence` and its
constructor `pub(crate) fn new`, and add to `impl Precedence`:

```rust
    /// The variables each variable must follow, as one bitmask per variable:
    /// bit `u` of entry `v` is set when `u -> v` is an edge. A set of bound
    /// variables `S` can bind `v` next exactly when `masks[v] & !S == 0`.
    /// `None` past 64 variables, which a `u64` cannot index.
    pub(crate) fn predecessor_masks(&self) -> Option<Vec<u64>> {
        let num_vars = self.in_degree.len();
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
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos predecessor_masks`
Expected: 2 passed. Until Step 5 uses the method, `cargo check` warns that
it is unused (`dead_code`); that's expected at this step.

- [ ] **Step 3: Write the optimiser's failing tests**

Create `kermit-algos/src/optimiser/cost_based.rs` holding only this test
module for now, and register it in `optimiser/mod.rs` with `mod cost_based;`
(after `mod cardinality;`):

```rust
#[cfg(test)]
mod tests {
    use {super::*, crate::optimiser::RelationStats, kermit_parser::JoinQuery};

    /// Statistics for `q`'s relations: `(name, tuples, per-column distinct
    /// counts)`.
    fn stats_for(q: &JoinQuery, relations: &[(&str, usize, &[usize])]) -> CatalogStats {
        CatalogStats::for_query(q, |name| {
            let &(_, tuples, distinct) = relations.iter().find(|(n, ..)| *n == name)?;
            Some(RelationStats::new(tuples, distinct.len()).with_column_distinct(distinct.to_vec()))
        })
    }

    /// `est` of the set of canonical variable indices `vars`.
    fn estimate(q: &JoinQuery, stats: &CatalogStats, vars: &[usize]) -> f64 {
        let analysis = analyse(q);
        let model = CostModel::new(q, &analysis.predicate_variables, analysis.num_vars, stats);
        model.estimate(vars.iter().fold(0, |set, &v| set | bit(v)))
    }

    #[test]
    fn a_join_on_one_variable_divides_by_the_larger_distinct_count() {
        // |R ⋈ S| on Y = |R| · |S| / max(V(R, Y), V(S, Y)) = 100 · 60 / 40.
        let q: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Z, Y).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 100, &[10, 20]), ("S", 60, &[30, 40])]);
        assert_eq!(estimate(&q, &stats, &[0, 1, 2]), 150.0);
    }

    #[test]
    fn a_partly_bound_atom_contributes_its_first_columns_distinct_count() {
        let q: JoinQuery = "Q(X, Y) :- R(X, Y).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 100, &[10, 20])]);
        assert_eq!(estimate(&q, &stats, &[0]), 10.0);
        assert_eq!(estimate(&q, &stats, &[]), 1.0);
    }

    #[test]
    fn a_longer_prefix_assumes_independent_columns_capped_by_the_tuples() {
        let q: JoinQuery = "Q(X, Y, Z) :- T(X, Y, Z).".parse().unwrap();
        let stats = stats_for(&q, &[("T", 1000, &[10, 5, 7])]);
        assert_eq!(estimate(&q, &stats, &[0, 1]), 50.0);
        let stats = stats_for(&q, &[("T", 1000, &[100, 50, 7])]);
        assert_eq!(estimate(&q, &stats, &[0, 1]), 1000.0);
    }

    #[test]
    fn empty_relations_estimate_zero_not_nan() {
        let q: JoinQuery = "Q(X) :- R(X), S(X).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 0, &[0]), ("S", 0, &[0])]);
        assert_eq!(estimate(&q, &stats, &[0]), 0.0);
    }

    /// The worked micro-example of `docs/optimisers/cost-based.md`, already
    /// rewritten: `Const_c7` pins `K0`, which selects few `B`s through
    /// `link`; `small` has few tuples, but its `A`s meet the rest only
    /// through `Y`. Canonical indices: A = 0, B = 1, Y = 2, K0 = 3.
    fn worked_example() -> (JoinQuery, CatalogStats) {
        let q: JoinQuery = "Q(A, B, Y) :- small(A, Y), link(K0, B), owns(B, Y), Const_c7(K0)."
            .parse()
            .unwrap();
        let stats = stats_for(&q, &[
            ("small", 100, &[100, 10]),
            ("link", 50_000, &[5_000, 20_000]),
            ("owns", 40_000, &[20_000, 10]),
        ]);
        (q, stats)
    }

    #[test]
    fn worked_example_estimates() {
        let (q, stats) = worked_example();
        for (vars, want) in [
            (&[3][..], 1.0),
            (&[0][..], 100.0),
            (&[0, 3][..], 100.0),
            (&[1, 3][..], 10.0),
            (&[0, 1, 3][..], 1000.0),
            (&[0, 1, 2, 3][..], 200.0),
        ] {
            assert_eq!(estimate(&q, &stats, vars), want, "est({vars:?})");
        }
    }

    #[test]
    fn worked_example_binds_the_selective_side_first() {
        let (q, stats) = worked_example();
        // K0, B, A, Y costs 1 + 10 + 1000 + 200 = 1211. Cardinality binds
        // small's A second, for 1 + 100 + 1000 + 200 = 1301.
        let plan = CostBasedOptimiser::default().plan(&q, &stats);
        assert_eq!(plan.variable_ordering, vec![3, 1, 0, 2]);
        let cardinality = CardinalityOptimiser.plan(&q, &stats);
        assert_eq!(cardinality.variable_ordering, vec![3, 0, 1, 2]);
    }

    #[test]
    fn column_order_constraints_bind() {
        // Y's relation S is tiny, but R(X, Y) puts X first.
        let q: JoinQuery = "Q(X, Y) :- R(X, Y), S(Y).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 1000, &[1000, 1000]), ("S", 1, &[1])]);
        let plan = CostBasedOptimiser::default().plan(&q, &stats);
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }

    #[test]
    fn equal_costs_take_the_lexicographically_smaller_order() {
        // X and Y are interchangeable, so both orders cost 10 + 100.
        let q: JoinQuery = "Q(X, Y) :- R(X), S(Y).".parse().unwrap();
        let stats = stats_for(&q, &[("R", 10, &[10]), ("S", 10, &[10])]);
        let plan = CostBasedOptimiser::default().plan(&q, &stats);
        assert_eq!(plan.variable_ordering, vec![0, 1]);
        // Swapping the head swaps the canonical indices; the smaller still
        // comes first.
        let q: JoinQuery = "Q(Y, X) :- R(X), S(Y).".parse().unwrap();
        let plan = CostBasedOptimiser::default().plan(&q, &stats);
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }

    #[test]
    fn the_budget_counts_every_set_reached() {
        // The worked example reaches six sets: {A}, {K0}, {A, K0}, {K0, B},
        // {A, K0, B} and all four.
        let (q, stats) = worked_example();
        let fits = CostBasedOptimiser { state_budget: 6 }.plan(&q, &stats);
        assert_eq!(fits.variable_ordering, vec![3, 1, 0, 2]);
        let cardinality = CardinalityOptimiser.plan(&q, &stats);
        assert_eq!(CostBasedOptimiser { state_budget: 5 }.plan(&q, &stats), cardinality);
        assert_eq!(CostBasedOptimiser { state_budget: 0 }.plan(&q, &stats), cardinality);
    }

    #[test]
    fn more_than_64_variables_get_cardinalitys_plan() {
        let body: Vec<String> = (0..65).map(|v| format!("R(V{v})")).collect();
        let q: JoinQuery = format!("Q(V0) :- {}.", body.join(", ")).parse().unwrap();
        let stats = stats_for(&q, &[("R", 10, &[10])]);
        assert_eq!(
            CostBasedOptimiser::default().plan(&q, &stats),
            CardinalityOptimiser.plan(&q, &stats)
        );
    }

    #[test]
    fn it_reads_column_distinct_counts() {
        assert_eq!(
            CostBasedOptimiser::default().required_statistics(),
            StatisticsLevel::ColumnDistinct
        );
    }
}
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos cost_based`
Expected: compile errors, ``cannot find type `CostModel` `` and others.

The `stats_for` call sites mix array lengths (`&[1000, 1000]` beside
`&[1]`), which relies on expected-type coercion to `&[usize]` inside the
tuple literals. If the compiler rejects one, write `&[1][..]` at that site.

- [ ] **Step 4: Check the expected numbers by hand before implementing**

The worked example's estimates follow from the formula in the spec;
re-derive at least `est({A, K0, B}) = 1000` before trusting the tests:
- numerator: `small` prefix 1 gives 100; `link` is fully bound, giving
  50,000; `owns` prefix 1 gives 20,000; `Const_c7` gives 1. That is 1e11.
- denominator: K0's counts are [1, 5000], so it contributes 5000; B's are
  [20,000, 20,000], so it contributes 20,000. That is 1e8.

1e11 / 1e8 = 1000. Then run the DP by hand:

| Set | Cheapest order | Cost |
|---|---|---|
| {A} | A | 100 |
| {K0} | K0 | 1 |
| {A, K0} | K0, A | 101 (A, K0 costs 200) |
| {K0, B} | K0, B | 11 |
| {A, K0, B} | K0, B, A | 1011 (K0, A, B costs 1101) |
| all | K0, B, A, Y | 1211 |

- [ ] **Step 5: Implement**

Put above the test module in `cost_based.rs`:

```rust
//! Cost-based ordering policy: the plan with the lowest estimated cost.

use {
    crate::{
        analysis::analyse,
        optimiser::{
            ordering::{topological_order, Precedence},
            CardinalityOptimiser, CatalogStats, QueryOptimiser, QueryPlan, StatisticsLevel,
        },
    },
    kermit_parser::JoinQuery,
    std::{
        cmp::Ordering,
        collections::{hash_map::Entry, HashMap},
    },
};

/// Plans the global attribute order with the lowest estimated cost.
///
/// At depth k, Leapfrog Triejoin binds one key per tuple of the join of
/// every atom projected onto the plan's first k variables, `S_k`. That
/// number depends only on the set `S_k`, not on its order, so a plan's cost
/// is `Σ_k est(S_k)`, where `est` is the System R estimate of the join's
/// size: the product of the atoms' projection sizes, divided, per variable,
/// by every column's distinct count except the smallest. The cheapest plan
/// is then a shortest path over sets of bound variables, found by dynamic
/// programming in the style of Selinger et al. (1979). The search visits
/// only the sets a valid plan can bind, those closed under the column-order
/// constraints, so its plan is valid by construction.
///
/// Ties break towards the lexicographically smaller order, so plans are
/// deterministic. A query with more than 64 variables, or whose search
/// would reach more than [`state_budget`](Self::state_budget) sets, gets
/// [`CardinalityOptimiser`]'s plan.
///
/// Reads per-column distinct counts, so it declares
/// [`StatisticsLevel::ColumnDistinct`]. See `docs/optimisers/cost-based.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostBasedOptimiser {
    /// The most variable sets the search may reach before it falls back to
    /// [`CardinalityOptimiser`].
    pub state_budget: usize,
}

impl CostBasedOptimiser {
    /// The default [`state_budget`](Self::state_budget): 20 times the 820
    /// sets of the largest WatDiv stress template (q0034, 13 variables).
    pub const DEFAULT_STATE_BUDGET: usize = 1 << 14;
}

impl Default for CostBasedOptimiser {
    fn default() -> Self {
        Self {
            state_budget: Self::DEFAULT_STATE_BUDGET,
        }
    }
}

impl QueryOptimiser for CostBasedOptimiser {
    fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan {
        let analysis = analyse(query);
        let model = CostModel::new(
            query,
            &analysis.predicate_variables,
            analysis.num_vars,
            stats,
        );
        let cheapest = Precedence::new(analysis.num_vars, &analysis.predicate_variables)
            .predecessor_masks()
            .and_then(|predecessors| cheapest_order(&model, &predecessors, self.state_budget));
        let Some(order) = cheapest else {
            return CardinalityOptimiser.plan(query, stats);
        };

        // The search binds only ready variables, so `order` is already a
        // topological order: ranked by its own positions, `topological_order`
        // returns it unchanged, so this plan comes out of `topological_order`
        // like every optimiser's. The assertion keeps that pass from quietly
        // repairing a search bug.
        let mut position = vec![0; analysis.num_vars];
        for (index, &v) in order.iter().enumerate() {
            position[v] = index;
        }
        let variable_ordering = topological_order(
            analysis.num_vars,
            &analysis.predicate_variables,
            |v| position[v],
        );
        debug_assert_eq!(
            variable_ordering, order,
            "the search returned an order that violates a column-order constraint"
        );
        QueryPlan {
            variable_ordering,
        }
    }

    fn required_statistics(&self) -> StatisticsLevel { StatisticsLevel::ColumnDistinct }
}

/// The one-variable set `{v}`. Sets of variables are `u64` bitmasks.
fn bit(v: usize) -> u64 { 1 << v }

/// Whether `set` holds variable `v`.
fn contains(set: u64, v: usize) -> bool { set & bit(v) != 0 }

/// One body atom's statistics, as the estimate reads them.
struct AtomStats {
    tuples: f64,
    /// Distinct values per column.
    distinct: Vec<f64>,
}

impl AtomStats {
    /// The statistics of relation `name`, with `arity` columns. Missing
    /// statistics mean "assume large", as for [`CardinalityOptimiser`]; a
    /// missing distinct count assumes the column is a key.
    fn of(name: &str, arity: usize, stats: &CatalogStats) -> Self {
        let tuples = stats.tuples(name).unwrap_or(usize::MAX) as f64;
        let distinct = (0..arity)
            .map(|column| stats.distinct(name, column).map_or(tuples, |d| d as f64))
            .collect();
        Self {
            tuples,
            distinct,
        }
    }

    /// The size of the atom's projection onto its first `prefix` columns.
    fn projection_size(&self, prefix: usize) -> f64 {
        if prefix == self.distinct.len() {
            self.tuples
        } else if prefix == 1 {
            self.distinct[0]
        } else {
            // No statistic covers a multi-column prefix: assume independent
            // columns.
            self.distinct[..prefix]
                .iter()
                .product::<f64>()
                .min(self.tuples)
        }
    }
}

/// `est(S)`: the System R estimate of how many keys LFTJ binds at the depth
/// where the bound variables are `S`.
struct CostModel<'q> {
    atoms: Vec<AtomStats>,
    /// Each atom's variables in column order, as `analyse` numbers them.
    predicate_variables: &'q [Vec<usize>],
    num_vars: usize,
}

impl<'q> CostModel<'q> {
    fn new(
        query: &JoinQuery, predicate_variables: &'q [Vec<usize>], num_vars: usize,
        stats: &CatalogStats,
    ) -> Self {
        let atoms = query
            .body
            .iter()
            .map(|atom| AtomStats::of(&atom.name, atom.terms.len(), stats))
            .collect();
        Self {
            atoms,
            predicate_variables,
            num_vars,
        }
    }

    /// `est(bound)`: over the atoms with a bound column, the product of
    /// their projections' sizes onto those columns, divided, for each bound
    /// variable, by the distinct counts of every column it occupies except
    /// the smallest (the containment-of-value-sets assumption).
    ///
    /// `bound` must be closed under the column-order constraints, so each
    /// atom's bound columns form a prefix.
    fn estimate(&self, bound: u64) -> f64 {
        if bound == 0 {
            return 1.0;
        }
        let mut numerator = 1.0;
        // Each bound variable's column distinct counts, and the order the
        // variables were first met in: atoms in body order, an atom's
        // variables in canonical order. Any fixed order gives the same value
        // up to rounding; this one follows the #68 prototype
        // (`planners.py`), which the parity check compares against.
        let mut distinct_of: Vec<Vec<f64>> = vec![Vec::new(); self.num_vars];
        let mut met: Vec<usize> = Vec::new();
        for (atom, vars) in self.atoms.iter().zip(self.predicate_variables) {
            let prefix = vars.iter().take_while(|&&v| contains(bound, v)).count();
            debug_assert!(
                vars[prefix..].iter().all(|&v| !contains(bound, v)),
                "a set closed under column order binds a prefix of every atom"
            );
            if prefix == 0 {
                continue;
            }
            numerator *= atom.projection_size(prefix);
            let mut columns: Vec<usize> = (0..prefix).collect();
            columns.sort_by_key(|&column| vars[column]);
            for column in columns {
                let v = vars[column];
                if distinct_of[v].is_empty() {
                    met.push(v);
                }
                distinct_of[v].push(atom.distinct[column]);
            }
        }
        let mut denominator = 1.0;
        for v in met {
            let counts = &mut distinct_of[v];
            counts.sort_by(f64::total_cmp);
            for &count in &counts[1..] {
                // Clamped, so an empty relation cannot divide by zero.
                denominator *= count.max(1.0);
            }
        }
        numerator / denominator
    }
}

/// The cheapest known way to bind one set of variables.
struct Partial {
    cost: f64,
    order: Vec<usize>,
}

impl Partial {
    /// Whether `self` is preferred to `other`: the lower cost, ties to the
    /// lexicographically smaller order, so the search is deterministic.
    fn beats(&self, other: &Partial) -> bool {
        match self.cost.total_cmp(&other.cost) {
            | Ordering::Less => true,
            | Ordering::Greater => false,
            | Ordering::Equal => self.order < other.order,
        }
    }
}

/// The cheapest order of the variables, by a forward dynamic programme
/// over the sets a valid plan can bind. Layer k holds the k-variable sets,
/// each with its cheapest order; a set grows only by a variable whose
/// `predecessors` are all bound.
///
/// `None` once more than `budget` sets have been reached.
fn cheapest_order(
    model: &CostModel<'_>, predecessors: &[u64], budget: usize,
) -> Option<Vec<usize>> {
    let num_vars = predecessors.len();
    let mut layer = HashMap::from([(0u64, Partial {
        cost: 0.0,
        order: Vec::new(),
    })]);
    let mut reached = 0usize;
    for _ in 0..num_vars {
        let mut next: HashMap<u64, Partial> = HashMap::new();
        let mut estimates: HashMap<u64, f64> = HashMap::new();
        for (&bound, partial) in &layer {
            let ready = |v: usize| !contains(bound, v) && predecessors[v] & !bound == 0;
            for v in (0..num_vars).filter(|&v| ready(v)) {
                let extended = bound | bit(v);
                let estimate = *estimates
                    .entry(extended)
                    .or_insert_with(|| model.estimate(extended));
                let mut order = partial.order.clone();
                order.push(v);
                let candidate = Partial {
                    cost: partial.cost + estimate,
                    order,
                };
                match next.entry(extended) {
                    | Entry::Vacant(slot) => {
                        reached += 1;
                        if reached > budget {
                            return None;
                        }
                        slot.insert(candidate);
                    },
                    | Entry::Occupied(mut slot) => {
                        if candidate.beats(slot.get()) {
                            slot.insert(candidate);
                        }
                    },
                }
            }
        }
        layer = next;
    }
    layer.into_values().next().map(|partial| partial.order)
}
```

In `optimiser/mod.rs`, add `cost_based::CostBasedOptimiser,` to the
`pub use` list. In `lib.rs`, add `CostBasedOptimiser` to the
`optimiser::{…}` list. Do **not** add an `Optimiser` variant yet; Task 8
does that.

- [ ] **Step 6: Run the tests to see them pass**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos`
Expected: all pass, including the 12 new tests.

- [ ] **Step 7: Commit, then the mutation checks**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets -p kermit-algos -- -Dwarnings
git -C $WT add kermit-algos/src/optimiser/ordering.rs kermit-algos/src/optimiser/cost_based.rs kermit-algos/src/optimiser/mod.rs kermit-algos/src/lib.rs
git -C $WT commit -m "feat(algos): CostBasedOptimiser (#81)" -m "Ports #68's estimate-based DP: System R estimates of LFTJ's per-depth
bindings, minimised over the downward-closed variable sets of the
column-order precedence DAG." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HYRXJE5mQn8ZJjRgxMur8D"
```

Each mutant below must make at least one test fail. Use the ground-rule
protocol: one at a time, revert by exact edit.

| Mutant (exact edit) | Must fail |
|---|---|
| `denominator *= count.max(1.0);` becomes `denominator *= 1.0;` | `a_join_on_one_variable_divides_by_the_larger_distinct_count`, `worked_example_estimates` |
| `count.max(1.0)` becomes `count` | `empty_relations_estimate_zero_not_nan` |
| `for &count in &counts[1..] {` becomes `for &count in &counts[..counts.len() - 1] {` | `a_join_on_one_variable_divides_by_the_larger_distinct_count` |
| `\| Ordering::Equal => self.order < other.order,` becomes `\| Ordering::Equal => self.order > other.order,` | `equal_costs_take_the_lexicographically_smaller_order` |
| `!contains(bound, v) && predecessors[v] & !bound == 0` becomes `!contains(bound, v)` | `column_order_constraints_bind` or `worked_example_binds_the_selective_side_first`, by assertion or panic |
| `if reached > budget {` becomes `if false {` | `the_budget_counts_every_set_reached` |
| `.min(self.tuples)` removed | `a_longer_prefix_assumes_independent_columns_capped_by_the_tuples` |

If a mutant survives, add a test that kills it, commit that test, and
record it.

## Task 7 [P3]: The WatDiv plan fixture (acceptance criterion 3)

**Files:**
- Create: `kermit-algos/tests/fixtures/watdiv-stress-100-stats.tsv`
- Create: `kermit-algos/tests/cost_based_watdiv_plans.rs`

- [ ] **Step 1: Write the fixture from #68's `relstats.txt`**

```bash
mkdir -p $WT/kermit-algos/tests/fixtures
{
  printf '# Statistics of the 61 watdiv-stress-100-test-1 relations: issue #68'"'"'s relstats.txt,\n'
  printf '# computed over the Parquet files the benchmark loads. The planner reads only these.\n'
  printf '# relation\ttuples\tdistinct subjects\tdistinct objects\n'
  tr ' ' '\t' < $EVID/relstats.txt
} > $WT/kermit-algos/tests/fixtures/watdiv-stress-100-stats.tsv
grep -vc '^#' $WT/kermit-algos/tests/fixtures/watdiv-stress-100-stats.tsv
grep -P '^purchasefor\t' $WT/kermit-algos/tests/fixtures/watdiv-stress-100-stats.tsv
```

Expected: `61`, then `purchasefor	150000	150000	17674`.

- [ ] **Step 2: Write the test**

Create `kermit-algos/tests/cost_based_watdiv_plans.rs`:

```rust
//! The cost-based optimiser reproduces the plans #68 simulated for the
//! WatDiv stress templates it was built to fix (issue #81).
//!
//! #68 counted, exactly, the bindings LFTJ makes under any plan of each
//! `watdiv-stress-100-test-1` template, and scored a Python prototype of
//! this optimiser (`plan_est_dp` in its `planners.py`) with those counts.
//! The planner sees only statistics, never data, so the 61 relations'
//! tuple and distinct counts (`fixtures/watdiv-stress-100-stats.tsv`, #68's
//! `relstats.txt`) reproduce its decisions without the data. Each test pins
//! the prototype's plan; its exact cost, from #68's counter, is in its doc
//! comment.

use {
    kermit_algos::{
        rewrite_atoms, rewrite_placeholders, rewrite_repeated_variables, CatalogStats,
        CostBasedOptimiser, JoinQuery, QueryOptimiser, RelationStats,
    },
    kermit_parser::Term,
    std::collections::HashMap,
};

/// `relation<TAB>tuples<TAB>distinct subjects<TAB>distinct objects`.
const STATS: &str = include_str!("fixtures/watdiv-stress-100-stats.tsv");

const Q0020: &str = "Q_test_1_q0020(V3, V2, V1, V4, V5, V0, V6) :- purchasefor(c450066, V3), \
                     includes(V2, V3), eligibleregion(V2, V1), validthrough(V2, V4), \
                     eligiblequantity(V2, V5), parentcountry(V0, V1), nationality(V6, V1).";

const Q0034: &str = "Q_test_1_q0034(V0, V4, V1, V9, V10, V11, V12, V5, V6, V7, V8, V2) :- \
                     type(V0, c206066), hasreview(V0, V4), tag(V0, V1), contentsize(V0, V9), \
                     description(V0, V10), keywords(V0, V11), purchasefor(V12, V0), \
                     rating(V4, V5), reviewer(V4, V6), text_601771(V4, V7), \
                     title_601766(V4, V8), tag(V2, V1).";

const Q0008: &str = "Q_test_1_q0008(V2, V0, V3, V4, V5, V8, V1, V10, V11, V6, V7) :- \
                     eligibleregion(V2, c17), offers(V0, V2), price(V2, V3), validfrom(V2, V4), \
                     validthrough(V2, V5), contactpoint(V0, V8), name(V0, V1), email(V0, V10), \
                     openinghours(V0, V11), reviewer(V6, V8), rating(V6, V7).";

/// The query as `kermit::db` hands it to the planner: after the const,
/// placeholder and selection rewrites.
fn rewritten(text: &str) -> JoinQuery {
    let (query, _) = rewrite_atoms(text.parse().expect("parse")).expect("const rewrite");
    rewrite_repeated_variables(rewrite_placeholders(query)).0
}

/// The fixture's statistics for `query`'s relations.
fn catalog(query: &JoinQuery) -> CatalogStats {
    let table: HashMap<&str, Vec<usize>> = STATS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().expect("relation name");
            (name, fields.map(|f| f.parse().expect("a count")).collect())
        })
        .collect();
    CatalogStats::for_query(query, |name| {
        let counts = table.get(name)?;
        Some(RelationStats::new(counts[0], 2).with_column_distinct(counts[1..].to_vec()))
    })
}

/// `query`'s variables in canonical order (`kermit_algos::analyse`): the
/// head's, then the body's in order of first appearance.
fn canonical_names(query: &JoinQuery) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let terms = query
        .head
        .terms
        .iter()
        .chain(query.body.iter().flat_map(|atom| &atom.terms));
    for term in terms {
        if let Term::Var(name) = term {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    names
}

/// The cost-based plan for `text`, by variable name.
fn plan(text: &str) -> Vec<String> {
    let query = rewritten(text);
    let names = canonical_names(&query);
    CostBasedOptimiser::default()
        .plan(&query, &catalog(&query))
        .variable_ordering
        .into_iter()
        .map(|v| names[v].clone())
        .collect()
}

/// Both current optimisers bind the small `parentcountry` and
/// `nationality` subjects before the constant and cost 9.68e10 bindings.
/// This plan binds the constant's offer first and costs 1.81e4 (#68).
#[test]
fn q0020_binds_the_constants_side_first() {
    assert_eq!(plan(Q0020), [
        "K0", "V2", "V3", "V4", "V5", "V0", "V6", "V1"
    ]);
}

/// `cardinality` binds `tag`'s 15,080 objects first and costs 2.27e9
/// bindings, 187x `lexicographic`. This plan costs 9.67e5 (#68).
#[test]
fn q0034_avoids_the_tag_cross_product() {
    assert_eq!(plan(Q0034), [
        "V12", "V0", "K0", "V9", "V10", "V11", "V4", "V5", "V6", "V7", "V8", "V2", "V1"
    ]);
}

/// The documented regression: 8.85e7 bindings, against `cardinality`'s
/// 3.81e7 (2.32x; #68). Pinned so that a change to it is deliberate.
#[test]
fn q0008_keeps_its_documented_regression() {
    assert_eq!(plan(Q0008), [
        "V0", "V1", "V10", "V11", "V6", "V8", "V7", "V2", "K0", "V3", "V4", "V5"
    ]);
}
```

- [ ] **Step 3: Run it**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos --test cost_based_watdiv_plans`
Expected: 3 passed. These plans are what #68's prototype computes from
this exact fixture (verified while writing the plan: 118 of 124 templates
match `planners.jsonl`; the 6 that differ all have selection views).

**If a plan differs, do not edit the expectation.**
1. Check the canonical numbering: print `canonical_names(&rewritten(Q…))`.
2. Check for a floating-point tie. The prototype multiplies the
   denominator in CPython set order; see the comment in
   `CostModel::estimate`.
3. Report the differing plan to the controller. Task 10 scores it with
   #68's exact counter.

- [ ] **Step 4: Mutation check**

With the same protocol, the first mutant of Task 6 (no denominator) must
also fail `q0020_binds_the_constants_side_first` here. Record the result.

- [ ] **Step 5: Commit**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit-algos/tests/cost_based_watdiv_plans.rs kermit-algos/tests/fixtures/watdiv-stress-100-stats.tsv
git -C $WT commit -m "test(algos): cost-based plans for WatDiv q0020, q0034 and q0008 (#81)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HYRXJE5mQn8ZJjRgxMur8D"
```

---

# P4 — wiring and coverage

## Task 8 [P4]: `--optimiser cost-based`

**Files:**
- Modify: `kermit-algos/src/optimiser/mod.rs`
- Modify: `kermit/tests/join_tests.rs`
- Modify: `kermit/tests/lubm_cardinalities.rs`
- Modify: `kermit/tests/cli_optimiser_choice.rs`

- [ ] **Step 1: Write the failing tests**

In `kermit-algos/src/optimiser/mod.rs`, change the match in
`each_optimiser_declares_the_statistics_it_reads` to:

```rust
            let want = match v {
                | Optimiser::Lexicographic | Optimiser::Cardinality => StatisticsLevel::TupleCounts,
                | Optimiser::CostBased => StatisticsLevel::ColumnDistinct,
            };
```

Append to `kermit/tests/cli_optimiser_choice.rs`:

```rust
#[test]
fn cli_bench_join_with_cost_based_optimiser_records_axis() {
    let reports = run_bench_join(&["--optimiser", "cost-based"]);
    assert_eq!(reports[0]["axes"]["optimiser"], "cost-based");
    // Sibling axis guard: the optimiser entry must extend the axes map,
    // not displace existing keys.
    assert_eq!(reports[0]["axes"]["algorithm"], "LeapfrogTriejoin");
}

/// `kermit join` writes the same rows under every optimiser, in every
/// cell: the plan changes the descent order, never the answer.
#[test]
fn cli_join_answers_identically_under_every_optimiser() {
    let rows = |optimiser: &str, structure: &str, algorithm: &str| -> Vec<String> {
        let output = Command::new(kermit_bin())
            .args([
                "join",
                "--relations",
                edge_fixture().to_str().unwrap(),
                "--query",
                fixtures_dir().join("path_query.dl").to_str().unwrap(),
                "--algorithm",
                algorithm,
                "--indexstructure",
                structure,
                "--optimiser",
                optimiser,
            ])
            .output()
            .expect("failed to run kermit binary");
        assert!(
            output.status.success(),
            "kermit join --optimiser {optimiser} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut lines: Vec<String> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        lines.sort();
        lines
    };
    for (structure, algorithm) in [
        ("tree-trie", "leapfrog-triejoin"),
        ("column-trie", "leapfrog-triejoin"),
        ("hash-trie", "hash-triejoin"),
    ] {
        let expected = rows("lexicographic", structure, algorithm);
        assert!(expected.len() > 1, "the fixture query must return rows");
        for optimiser in ["cardinality", "cost-based"] {
            assert_eq!(
                rows(optimiser, structure, algorithm),
                expected,
                "{optimiser} on {structure}"
            );
        }
    }
}
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos optimiser_enum`
Expected: a compile error, ``no variant named `CostBased` ``.

- [ ] **Step 2: Add the variant**

In `kermit-algos/src/optimiser/mod.rs`, add to `enum Optimiser`, after
`Cardinality`:

```rust
    /// Lowest estimated cost, by dynamic programming over the sets of bound
    /// variables; see [`CostBasedOptimiser`].
    CostBased,
```

an `instantiate()` arm `| Self::CostBased => Box::new(CostBasedOptimiser::default()),`,
and an `axis_value()` arm `| Self::CostBased => "cost-based",`.

- [ ] **Step 3: The join suites**

In `kermit/tests/join_tests.rs`, import `CostBasedOptimiser` alongside
`CardinalityOptimiser`, and add one `CostBasedOptimiser` invocation directly
after each `CardinalityOptimiser` one. There are 13:

```rust
define_multiway_join_test_suite!(TreeTrieLinear, LeapfrogTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(TreeTrieBinary, LeapfrogTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(TreeTrieGalloping, LeapfrogTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(ColumnTrieLinear, LeapfrogTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(ColumnTrieBinary, LeapfrogTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(ColumnTrieGalloping, LeapfrogTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(HashTrieSipPruned, HashTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(HashTrieFxPruned, HashTriejoin, CostBasedOptimiser);

define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    CostBasedOptimiser,
    HalfFull
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    CostBasedOptimiser,
    HalfFull
);

define_multiway_join_test_suite_for_build_mode!(
    ColumnTrie,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    Incremental
);
```

`test_join` builds each database at `ColumnDistinct` for these, through the
`JoinEntry::database` from Task 5, so every pattern plans from real
distinct counts.

- [ ] **Step 4: The LUBM row**

In `kermit/tests/lubm_cardinalities.rs`, import `CostBasedOptimiser` and
add the row:

```rust
    let optimisers: Vec<(&str, Box<dyn QueryOptimiser>)> = vec![
        ("lexicographic", Box::new(LexicographicOptimiser)),
        ("cardinality", Box::new(CardinalityOptimiser)),
        ("cost-based", Box::new(CostBasedOptimiser::default())),
    ];
```

- [ ] **Step 5: Run everything that changed**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-algos
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests --test cli_optimiser_choice --test lubm_mini_oracle --lib
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test lubm_cardinalities -- --nocapture
```

Expected:
- All pass.
- `join_tests` gains 13 × 16 = 208 tests.
- `lubm_mini_oracle` now covers three optimisers.
- `engines_gather_the_statistics_their_optimiser_reads` now exercises
  `ColumnDistinct`.
- `lubm_cardinalities` prints no "skipping" line, since it runs inside
  `nix develop`, where `java` is on PATH.

If `lubm_cardinalities` reports a mismatch under `cost-based` only, it is
an executor bug exposed by a new variable order, as in the LFTJ
failed-descent bug. Do not adjust the optimiser to hide it: stop and report
it to the controller with the query and both counts.

- [ ] **Step 6: Commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets -p kermit-algos -p kermit -- -Dwarnings
git -C $WT add kermit-algos/src/optimiser/mod.rs kermit/tests/join_tests.rs kermit/tests/lubm_cardinalities.rs kermit/tests/cli_optimiser_choice.rs
git -C $WT commit -m "feat: --optimiser cost-based (#81)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HYRXJE5mQn8ZJjRgxMur8D"
```

## Task 9 [P4]: The WatDiv and subject-position tests under every optimiser

Both tests ran only `lexicographic`, a gap recorded since the LFTJ
failed-descent fix. Subject-position constants are where cost-based plans
differ most from the others.

**Files:**
- Modify: `kermit/tests/watdiv_correctness.rs`
- Modify: `kermit/tests/subject_position_constant.rs`

- [ ] **Step 1: `watdiv_correctness.rs`**

1. Imports: `clap::ValueEnum`, `kermit::db::{lftj_join, Database, SortedFamily}`,
   and `kermit_algos::{JoinQuery, LeapfrogTriejoin, Optimiser}` (drop
   `LexicographicOptimiser`).
2. `check_cardinalities` takes `optimiser: Optimiser`. Its first lines
   after loading become:

```rust
    let planner = optimiser.instantiate();
    let relations = Database::new::<SortedFamily>(relations, planner.required_statistics());
```

   (replacing Task 5's `Database::from`). The join call passes
   `planner.as_ref()`, and the assertion message names the optimiser:

```rust
        assert_eq!(
            got,
            want,
            "cardinality mismatch on {key} ({} / {}): got {got}, expected {want}\nquery: {}",
            optimiser.axis_value(),
            std::any::type_name::<R>(),
            q.query
        );
```

3. The test becomes:

```rust
#[test]
fn watdiv_mini_cardinalities_match() {
    // The plan an optimiser picks changes the descent order, never the
    // answer; iterating the CLI enum covers optimisers added later.
    for &optimiser in Optimiser::value_variants() {
        check_cardinalities::<TreeTrie<LinearSeek>>(optimiser);
        check_cardinalities::<TreeTrie<BinarySeek>>(optimiser);
        check_cardinalities::<TreeTrie<GallopingSeek>>(optimiser);
        check_cardinalities::<ColumnTrie<LinearSeek>>(optimiser);
        check_cardinalities::<ColumnTrie<BinarySeek>>(optimiser);
        check_cardinalities::<ColumnTrie<GallopingSeek>>(optimiser);
    }
}
```

4. Add to the module doc: "Every query runs under every optimiser, because
   a plan changes the descent order, never the answer."

- [ ] **Step 2: `subject_position_constant.rs`**

1. Imports: `clap::ValueEnum`,
   `kermit::db::{hash_join, lftj_join, Database, HashFamily, SortedFamily}`,
   and `kermit_algos::{JoinQuery, LeapfrogTriejoin, Optimiser}`.
2. Replace `edge_tries`, `edge_rels`, `lftj` and `hash` with:

```rust
const EDGES: [[usize; 2]; 3] = [[1, 2], [1, 3], [2, 4]];

fn edges() -> Vec<Vec<usize>> { EDGES.iter().map(|e| e.to_vec()).collect() }

/// `query` over edge = {(1,2), (1,3), (2,4)} as a `TreeTrie` (LFTJ path),
/// planned by `optimiser`.
fn lftj(query: &str, optimiser: Optimiser) -> Vec<Vec<usize>> {
    let planner = optimiser.instantiate();
    let edge: TreeTrie = TreeTrie::from_tuples(2.into(), edges());
    let database = Database::new::<SortedFamily>(
        BTreeMap::from([("edge".to_string(), edge)]),
        planner.required_statistics(),
    );
    let q: JoinQuery = query.parse().expect("parse");
    lftj_join::<TreeTrie, LeapfrogTriejoin>(&database, q, planner.as_ref()).unwrap()
}

/// The same over a `HashTrie` (hash path).
fn hash(query: &str, optimiser: Optimiser) -> Vec<Vec<usize>> {
    let planner = optimiser.instantiate();
    let edge = HashTrieSip::from_tuples(2.into(), edges());
    let database = Database::new::<HashFamily<SipHashStrategy>>(
        BTreeMap::from([("edge".to_string(), edge)]),
        planner.required_statistics(),
    );
    let q: JoinQuery = query.parse().expect("parse");
    hash_join::<HashTrieSip, SipHashStrategy>(&database, q, planner.as_ref()).unwrap()
}
```

3. Each of the four tests loops over the optimisers, e.g.:

```rust
// Q(X) :- edge(c1, X).  — successors of node 1 are {2, 3}.
#[test]
fn subject_position_constant_lftj() {
    for &optimiser in Optimiser::value_variants() {
        let got = head_col(lftj("Q(X) :- edge(c1, X).", optimiser));
        assert_eq!(got, vec![2, 3], "{optimiser:?}");
    }
}
```

   and likewise `subject_position_constant_hash` (calling `hash`) and the
   two object-position controls, `edge(X, c4)`, which expect `vec![2]`.
4. Add to the module doc: "Each case runs under every optimiser: the
   shape exists to guard descent order, and each optimiser orders it
   differently."

- [ ] **Step 3: Run**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test watdiv_correctness --test subject_position_constant`
Expected: all pass (1 + 4 tests).

- [ ] **Step 4: Mutation check (the guard against a no-op loop)**

Mutant: in `watdiv_mini_cardinalities_match`, change
`for &optimiser in Optimiser::value_variants() {` to
`for &optimiser in &Optimiser::value_variants()[..0] {`. The test now
passes vacuously. Confirm that it does, and that the test's runtime drops,
which shows the loop was doing real work. Revert. This is a sanity check,
not a failing mutant: record it as such.

- [ ] **Step 5: Commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets -p kermit -- -Dwarnings
git -C $WT add kermit/tests/watdiv_correctness.rs kermit/tests/subject_position_constant.rs
git -C $WT commit -m "test: WatDiv and subject-position constants under every optimiser (#81)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HYRXJE5mQn8ZJjRgxMur8D"
```

---

## Task 10 [controller]: Verification beyond CI

**Files:** none in the repo. Everything goes under `$SCRATCH`, and the
findings go in `$SCRATCH/verification.md`, which Task 11 quotes.

- [ ] **Step 1: The prototype's plans from the fixture statistics (baseline)**

Write `$SCRATCH/proto_plans.py`. It is the brainstorming session's
`relstats_parity.py`, changed to dump plans:

```python
import json, sys, types
D = "/tb/Source/Academia/kermit-bench-runs/watdiv-stress-100-prelim-2026-09-10/diagnostics/issue-68"
sys.path.insert(0, D)
duck = types.ModuleType("duckdb")
class _Con:
    def execute(self, *a): pass
duck.connect = lambda: _Con()
sys.modules["duckdb"] = duck   # planning needs no data; bindings.py imports duckdb at load
import yaml
from bindings import Query
from planners import plan_est_dp

rel = {}
for line in open(f"{D}/relstats.txt"):
    n, t, s, o = line.split()
    rel[n] = (int(t), int(s), int(o))
queries = yaml.safe_load(open(f"{D}/queries-124.yml"))["queries"]
with open(sys.argv[1], "w") as out:
    for x in queries:
        q = Query(x["query"])
        stats = []
        for (pname, _), vars_ in zip(q.atoms, q.pv):
            r = q.rel.get(pname)
            if r is not None and r[0] == "const":
                stats.append((1, {vars_[0]: 1}))
            else:
                t, s, o = rel[r[1] if r is not None else pname]
                stats.append((t, {vars_[0]: s, vars_[1]: o}))
        out.write(json.dumps({"query": x["name"], "plan": q.label(plan_est_dp(q, stats, True))}) + "\n")
```

```bash
uv run --with pyyaml --with numpy --with pandas python $SCRATCH/proto_plans.py $SCRATCH/proto_plans.jsonl
wc -l $SCRATCH/proto_plans.jsonl
```

Expected: `124`.

- [ ] **Step 2: The Rust plans, and planning time**

Create the scratch crate `$SCRATCH/parity/`:

`$SCRATCH/parity/Cargo.toml` (substitute the literal `$WT` path):

```toml
[package]
name = "parity"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
kermit-algos = { path = "/tb/Source/Academia/kermit/.loom/worktrees/aidanb/81_18db9218353440cf/kermit-algos" }
kermit-parser = { path = "/tb/Source/Academia/kermit/.loom/worktrees/aidanb/81_18db9218353440cf/kermit-parser" }
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
serde_json = "1"

[workspace]
```

`$SCRATCH/parity/src/main.rs`:

```rust
use {
    kermit_algos::{
        rewrite_atoms, rewrite_placeholders, rewrite_repeated_variables, CatalogStats,
        CostBasedOptimiser, JoinQuery, QueryOptimiser, RelationStats,
    },
    kermit_parser::Term,
    std::{collections::HashMap, time::Instant},
};

#[derive(serde::Deserialize)]
struct File { queries: Vec<Entry> }
#[derive(serde::Deserialize)]
struct Entry { name: String, query: String }

fn main() {
    let evid = std::env::args().nth(1).expect("EVID dir");
    let stats: HashMap<String, Vec<usize>> = std::fs::read_to_string(format!("{evid}/relstats.txt"))
        .unwrap()
        .lines()
        .map(|l| {
            let mut f = l.split_whitespace();
            (f.next().unwrap().to_string(), f.map(|x| x.parse().unwrap()).collect())
        })
        .collect();
    let file: File =
        serde_yaml::from_str(&std::fs::read_to_string(format!("{evid}/queries-124.yml")).unwrap())
            .unwrap();
    let optimiser = CostBasedOptimiser::default();
    for e in &file.queries {
        let (q, _) = rewrite_atoms(e.query.parse().unwrap()).unwrap();
        let q: JoinQuery = rewrite_repeated_variables(rewrite_placeholders(q)).0;
        let mut base_of: HashMap<String, String> = HashMap::new();
        for atom in &q.body {
            // Select_<n>_<base>: the selection view reports its base's statistics.
            if let Some(rest) = atom.name.strip_prefix("Select_") {
                let base = rest.split_once('_').unwrap().1.to_string();
                base_of.insert(atom.name.clone(), base);
            }
        }
        let catalog = CatalogStats::for_query(&q, |name| {
            let base = base_of.get(name).map(String::as_str).unwrap_or(name);
            let c = stats.get(base)?;
            Some(RelationStats::new(c[0], 2).with_column_distinct(c[1..].to_vec()))
        });
        let mut names: Vec<String> = Vec::new();
        for t in q.head.terms.iter().chain(q.body.iter().flat_map(|a| &a.terms)) {
            if let Term::Var(n) = t {
                if !names.contains(n) { names.push(n.clone()); }
            }
        }
        let start = Instant::now();
        const RUNS: u32 = 200;
        let mut plan = optimiser.plan(&q, &catalog);
        for _ in 1..RUNS { plan = optimiser.plan(&q, &catalog); }
        let micros = start.elapsed().as_secs_f64() * 1e6 / f64::from(RUNS);
        let labels: Vec<&str> = plan.variable_ordering.iter().map(|&v| names[v].as_str()).collect();
        println!("{}", serde_json::json!({"query": e.name, "plan": labels, "plan_us": micros}));
    }
}
```

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo run --release --manifest-path $SCRATCH/parity/Cargo.toml -- $EVID > $SCRATCH/rust_plans.jsonl
wc -l $SCRATCH/rust_plans.jsonl
```

Expected: `124`.

- [ ] **Step 3: Diff, then score every difference with #68's exact counter**

```bash
python3 - "$SCRATCH" <<'EOF'
import json, sys
s = sys.argv[1]
proto = {json.loads(l)["query"]: json.loads(l)["plan"] for l in open(f"{s}/proto_plans.jsonl")}
rust = {json.loads(l)["query"]: json.loads(l) for l in open(f"{s}/rust_plans.jsonl")}
same = [q for q in proto if rust[q]["plan"] == proto[q]]
diff = [q for q in proto if rust[q]["plan"] != proto[q]]
print("identical", len(same), "different", len(diff), diff)
slow = sorted(rust.values(), key=lambda r: -r["plan_us"])[:3]
print("slowest plans (us):", [(r["query"], round(r["plan_us"], 1)) for r in slow])
json.dump(diff, open(f"{s}/diff.json", "w"))
EOF
```

Expected: identical on every template whose prototype plan used the same
statistics. The brainstorm's Python check found 118 identical between the
relstats-based prototype and `planners.jsonl`, so for this Rust-vs-relstats
comparison expect 124 identical. Any difference is either a floating-point
tie or a bug.
- **Investigate a bug** with Task 6's unit tests.
- **Score a tie, and the 6 selection-view templates** (`planners.jsonl`'s
  `estdp_sf` against the Rust plan), with the exact counter. Run it inside
  the memory cap, since it reads the cached Parquet data read-only.

```bash
cat > $SCRATCH/score.py <<'EOF'
import json, sys
D = "/tb/Source/Academia/kermit-bench-runs/watdiv-stress-100-prelim-2026-09-10/diagnostics/issue-68"
sys.path.insert(0, D)
import yaml
from bindings import Query
s = sys.argv[1]
qs = {x["name"]: x["query"] for x in yaml.safe_load(open(f"{D}/queries-124.yml"))["queries"]}
rust = {json.loads(l)["query"]: json.loads(l)["plan"] for l in open(f"{s}/rust_plans.jsonl")}
ref = {json.loads(l)["query"]: json.loads(l) for l in open(f"{D}/planners.jsonl")}
targets = sorted(set(json.load(open(f"{s}/diff.json"))) | {q for q in ref if rust[q] != ref[q]["estdp_sf"]["plan"]})
for name in targets:
    q = Query(qs[name])
    order = [q.names.index(v) for v in rust[name]]
    total = sum(q.profile(order))
    print(json.dumps({"query": name, "rust_plan": rust[name], "rust_cost": total,
                      "prototype_cost": ref[name]["estdp_sf"]["sum"], "cardinality_cost": ref[name]["card_sf"]["sum"]}))
EOF
systemd-run --user --scope -p MemoryMax=3G -p MemorySwapMax=0 \
  uv run --with duckdb --with pandas --with pyyaml --with numpy python $SCRATCH/score.py $SCRATCH > $SCRATCH/scored.jsonl
cat $SCRATCH/scored.jsonl
```

Expected: one line per template whose Rust plan differs from
`planners.jsonl` (at least the 6 selection-view templates: q0006, q0031,
q0052, q0071, q0091, q0259), each with exact costs.

- [ ] **Step 4: Smoke run on real data**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build --release -p kermit
C=$HOME/.cache/kermit/benchmarks/watdiv-stress-100-test-1
printf '%s\n' 'Q_test_1_q0020(V3, V2, V1, V4, V5, V0, V6) :- purchasefor(c450066, V3), includes(V2, V3), eligibleregion(V2, V1), validthrough(V2, V4), eligiblequantity(V2, V5), parentcountry(V0, V1), nationality(V6, V1).' > $SCRATCH/q0020.dl
smoke() {  # $1 structure, $2 algorithm
  local t0=$(date +%s.%N)
  systemd-run --user --scope -q -p MemoryMax=3G -p MemorySwapMax=0 \
    timeout 120 $WT/target/release/kermit join \
      --relations $C/purchasefor.parquet $C/includes.parquet $C/eligibleregion.parquet \
        $C/validthrough.parquet $C/eligiblequantity.parquet $C/parentcountry.parquet $C/nationality.parquet \
      --query $SCRATCH/q0020.dl --algorithm $2 --indexstructure $1 --optimiser cost-based | wc -l
  echo "$1: $(awk "BEGIN {print $(date +%s.%N) - $t0}") s"
}
smoke column-trie leapfrog-triejoin
smoke tree-trie leapfrog-triejoin
smoke hash-trie hash-triejoin
```

Expected: `1` line per cell (the CSV header only; q0020 has 0 rows), and
each finishing in a few seconds, dominated by loading. Both current
optimisers ran past 30 s on this query. Do **not** run
`bench run --force` or `bench gen`, and write nothing under
`~/.cache/kermit`.

- [ ] **Step 5: Record**

Write `$SCRATCH/verification.md` with:
- the parity counts;
- the `scored.jsonl` table;
- the planning time for q0034 and the three slowest templates, in µs,
  release build;
- the smoke-run times.

Then post a short summary to the user.

---

## Task 11 [controller]: Documentation

**Files:**
- Create: `docs/optimisers/cost-based.md`
- Modify: `docs/optimisers/lexicographic.md`, `docs/optimisers/cardinality.md` (See also)
- Modify: `CLAUDE.md`, `ARCHITECTURE.md`, `kermit/README.md`

- [ ] **Step 1: Write `docs/optimisers/cost-based.md`**

Write `docs/optimisers/cost-based.md` with this content. In three places
the text asks for a value from Task 10: fill in the "Complexity" sentence,
the parity paragraph and the selection-view table from
`$SCRATCH/verification.md`.

````markdown
# Cost-Based Optimiser

> **Status:** experimental · **CLI:** `--optimiser cost-based` · **Implementation:** [`kermit_algos::optimiser::cost_based`](../../kermit-algos/src/optimiser/cost_based.rs)

Chooses the variable order with the lowest *estimated* cost, by dynamic
programming over the sets of bound variables, in the style of System R
(Selinger et al., 1979). Leapfrog Triejoin's work at each depth depends
only on which variables are bound, not on their order, so the cheapest
plan is a shortest path over those sets; the column-order constraints keep
the space small enough to search exactly. It reads per-column distinct
counts as well as tuple counts. Prefer it when a query mixes a selective
constant with small relations unrelated to it: there `cardinality` binds
the small relations first and enumerates their cross product (WatDiv q0020
and q0034, issue #68).

## Cost function

At depth k, LFTJ binds one key per tuple of the join of every atom
projected onto the plan's first k variables, `S_k`. A plan costs the sum
over depths:

    cost(v₁ … vₙ) = Σₖ est(Sₖ)

`est(S)` is the System R estimate of that join's size:

- **Numerator:** over the atoms with a bound column, the size of their
  projection onto the bound columns. A valid plan binds an atom's columns
  left to right, so they are a prefix of length p, and the size is:
  - the relation's tuple count when p is its arity;
  - its first column's distinct count when p = 1;
  - otherwise, the product of the prefix's distinct counts, capped at the
    tuple count (independence).
- **Denominator:** for each bound variable, the distinct counts of every
  column it occupies, except the smallest. This is containment of value
  sets: the variable's values are assumed to lie within the column with
  the fewest.

An empty relation estimates 0. Distinct counts are clamped to at least 1 in
the denominator.

## Search

A forward dynamic programme.
- **Layers.** Layer k holds every k-variable set a valid plan can bind,
  that is, every set closed under the column-order constraints, each with
  its cheapest order.
- **Validity.** A set grows only by a variable whose predecessors are all
  in it, so every plan found is a topological order of the constraint
  DAG. The plan is passed through `kermit_algos::topological_order`,
  ranked by its own positions, and comes back unchanged.
- **Ties** break towards the lexicographically smaller order of canonical
  indices, so plans are deterministic.

**Budget.** The sets searched are the downward-closed sets of the
constraint DAG, at most 2ⁿ. The largest WatDiv stress template (q0034, 13
variables) has 820. Past `state_budget` sets (default 2¹⁴ = 16,384), or
past 64 variables (sets are `u64` bitmasks), the optimiser returns
`CardinalityOptimiser`'s plan.

## Statistics consumed

`CatalogStats::tuples(name)` and `CatalogStats::distinct(name, column)`,
for every body predicate. So it declares `required_statistics() ==
StatisticsLevel::ColumnDistinct`:
- the engine's `Database` counts each relation's distinct values per
  column once, when it is built (one walk per relation);
- the join entry points refuse to plan from less
  (`JoinError::MissingStatistics`);
- the other optimisers read only tuple counts, so their runs never pay for
  the walk.

Special cases:
- `Const_*` singletons have one tuple and one distinct value.
- `Select_*` views (repeated variables) report their base relation's
  statistics, as upper bounds.
- Missing statistics only arise when the optimiser is called directly with
  a hand-built `CatalogStats`. The tuple count is then "assume large"
  (`usize::MAX`), and each distinct count equals the tuple count.

## Complexity

O(states × (V + P·a)) for the estimates plus O(transitions) comparisons,
with states ≤ `state_budget`, V variables, and P body predicates of arity
at most a. Planning q0034 takes <the µs value from `verification.md`> per
join in a release build. It runs on every join, inside the `iteration`
metric.

## Worked micro-example

Query `Q(A, B, Y) :- small(A, Y), link(c7, B), owns(B, Y).`, rewritten to
`small(A, Y), link(K0, B), owns(B, Y), Const_c7(K0)`.

**Variables.** Canonical indices: A = 0, B = 1, Y = 2, K0 = 3.

**Constraints.** A → Y, K0 → B, B → Y.

| Relation | Tuples | Distinct col 0 | Distinct col 1 |
|---|---:|---:|---:|
| `small` | 100 | 100 | 10 |
| `link` | 50,000 | 5,000 | 20,000 |
| `owns` | 40,000 | 20,000 | 10 |
| `Const_c7` | 1 | 1 | — |

**Estimates.** For each set a valid plan can bind:

| S | est(S) |
|---|---:|
| {K0} | 5,000 / 5,000 = 1 |
| {A} | 100 |
| {A, K0} | 100 · 5,000 / 5,000 = 100 |
| {K0, B} | 50,000 · 20,000 / (5,000 · 20,000) = 10 |
| {A, K0, B} | 100 · 50,000 · 20,000 / (5,000 · 20,000) = 1,000 |
| {A, K0, B, Y} | 100 · 50,000 · 40,000 / (5,000 · 20,000 · 10) = 200 |

**Plans** reaching the full set:

| Plan | Cost | |
|---|---:|---|
| K0, B, A, Y | 1 + 10 + 1,000 + 200 = **1,211** | chosen |
| K0, A, B, Y | 1 + 100 + 1,000 + 200 = 1,301 | `cardinality`'s: `small` has only 100 tuples |
| A, K0, B, Y | 100 + 100 + 1,000 + 200 = 1,400 | `lexicographic`'s |

`variable_ordering = [3, 1, 0, 2]`. The unit tests
`worked_example_estimates` and `worked_example_binds_the_selective_side_first`
pin these numbers.

## Regressions and limitations

#68 scored a Python prototype of this optimiser (`plan_est_dp`) on all 124
`watdiv-stress-100-test-1` templates with exact binding counts. Against
`cardinality`, it was better on 32, equal on 89 and worse on 3:

| Query | `cardinality` | `cost-based` | |
|---|---:|---:|---|
| q0020 | 9.68e10 | 1.81e4 | fixed |
| q0034 | 2.27e9 | 9.67e5 | fixed |
| q0008 | 3.81e7 | 8.85e7 | 2.32× worse |
| q0360 | 1.34e4 | 1.67e4 | 1.25× worse |
| q0070 | 3.17e3 | 3.21e3 | 1.01× worse |

The workload total falls from 3.85e11 to 2.64e11 bindings. The search is
exact for its cost model, so each regression is an estimate that ranks a
worse plan as cheaper.

**Parity with the prototype.** <The Task 10 counts: the Rust plans equal
the prototype's on N of 124 templates computed from the same statistics.
Then a table of the templates whose plan differs from #68's
`planners.jsonl`, with their exact costs from `scored.jsonl`. These are at
least the 6 selection-view templates, where the prototype measured the
filtered view and kermit reads the base relation's statistics.>
`kermit-algos/tests/cost_based_watdiv_plans.rs` pins q0020, q0034 and
q0008 against the committed statistics.

**Limitations:**
- **Subject-first tries.** Every relation is stored subject-first, so 10
  of #68's 15 slow templates cost 3.7e7 to 1.7e11 bindings under *every*
  valid plan. Object-first tries (#82) lift that ceiling; this optimiser is
  the planner they need.
- **Selection views** use their base relation's statistics.
- **Multi-column prefixes** assume independent columns. No WatDiv or LUBM
  relation has more than two columns.
- **Past the budget**, the plan is `cardinality`'s, not an approximation of
  this cost model.

## CLI

    kermit bench run watdiv-stress-100-test-1 -i column-trie -a leapfrog-triejoin --optimiser cost-based

Bench-report axis: `optimiser: "cost-based"`. Its `end_to_end` includes
the statistics walk. Its `iteration` does not, because the engine is built
before timing starts.

## See also

- [`LexicographicOptimiser`](./lexicographic.md) and [`CardinalityOptimiser`](./cardinality.md): the tuple-count-only siblings, and the control arms when ablating this one.
- [`LeapfrogTriejoin`](../algorithms/leapfrog-triejoin.md), [`HashTriejoin`](../algorithms/hash-triejoin.md): the algorithms that execute the produced `QueryPlan`.
- [`docs/specs/2026-10-05-cost-based-optimiser-design.md`](../specs/2026-10-05-cost-based-optimiser-design.md): the design, including the catalog (`Database`) it reads from.
- [`docs/specs/optimization-standard.md`](../specs/optimization-standard.md): the optimiser is a first-class benchmark dimension with its own `optimiser` report axis.
````

- [ ] **Step 2: Cross-links**

In `docs/optimisers/lexicographic.md` and `docs/optimisers/cardinality.md`,
add under "See also":
`- [`CostBasedOptimiser`](./cost-based.md) — plans from per-column distinct counts by dynamic programming over bound-variable sets.`

- [ ] **Step 3: `CLAUDE.md`**

Make these edits:
1. In the workspace-architecture block's `kermit` entry, replace
   ``over a `BTreeMap<String, R>` relation store; all return`` with
   ``over a `db::Database<R>` (relations plus the planner statistics gathered when it is built); all return``.
2. In "Key Trait Hierarchy", replace the end of the **QueryOptimiser**
   bullet, ``Implementations: `LexicographicOptimiser` (default), `CardinalityOptimiser`.``,
   with:
   ``Implementations: `LexicographicOptimiser` (default), `CardinalityOptimiser`, `CostBasedOptimiser` (System R–style DP over bound-variable sets). Each declares what it reads (`required_statistics() -> StatisticsLevel`: `TupleCounts` by default, `ColumnDistinct` for cost-based); the engine's `Database` gathers exactly that when it is built, and the entry points return `JoinError::MissingStatistics` rather than plan from less.``
3. In "Adding a new query optimiser", append to step 2:
   ``If `plan` reads more than tuple counts (e.g. `CatalogStats::distinct`), override `required_statistics()`; engines then gather it when they build their `Database`, outside every timed join.``
4. In the Testing Patterns bullet for `define_multiway_join_test_suite!()`,
   after "via `kermit/tests/common/utils.rs::JoinEntry`)", insert
   ``, each over a `Database` built at the optimiser's `required_statistics()`, ``.
5. In "Component Reference Docs", after the cardinality line, add
   ``- `docs/optimisers/cost-based.md` — cost-based DP over bound-variable sets (`--optimiser cost-based`).``

- [ ] **Step 4: `ARCHITECTURE.md`**

Make these edits:
1. In the families table's `Engine` row, `` `lftj_join` free function over `BTreeMap<String, R>` ``
   becomes `` `lftj_join` free function over `Database<R>` ``, and
   `` `hash_join` free function over `BTreeMap<String, HashTrie<H, P>>` ``
   becomes `` `hash_join` free function over `Database<HashTrie<H, P>>` ``.
2. In the "Query Planning" paragraph, replace
   ``it gathers per-relation tuple counts (`Cardinality::tuple_count`) into `CatalogStats` ``
   with
   ``it reads the statistics its `Database` gathered when it was built — tuple counts (`Cardinality::tuple_count`) always, per-column distinct counts when the optimiser's `required_statistics()` asks — into `CatalogStats` ``.
   Then append to the paragraph:
   ``` `CostBasedOptimiser` (`--optimiser cost-based`) minimises the System R estimate of LFTJ's per-depth bindings by dynamic programming over the sets a valid plan can bind; see `docs/optimisers/cost-based.md`.```
3. In the entry-points paragraph, replace
   ``both free functions over a `BTreeMap<String, R>` relation store keyed by relation name``
   with
   ``both free functions over a `Database<R>` (`kermit/src/db/database.rs`): the relations keyed by name plus the planner statistics gathered once when it is built, up to the `StatisticsLevel` its optimiser declares``.
4. In the next paragraph, change "abstracts the only three steps that differ
   between families" to "abstracts the only four steps that differ between
   families". Before "Everything else", add the item
   ``and lending a relation's tuples (`for_each_tuple`, which `Database` walks to count distinct values per column)``.
5. In "New Query Optimiser" step 2, append
   ``Override `required_statistics()` if the policy reads more than tuple counts.``

- [ ] **Step 5: `kermit/README.md`**

Lines 12–13: `over a `BTreeMap<String, R>` of `TrieIterable` relations`
becomes `over a `Database<R>` of `TrieIterable` relations (relations plus
planner statistics)`, and `over a `BTreeMap<String, HashTrie<H>>`` becomes
`over a `Database<HashTrie<H>>``.

- [ ] **Step 6: Check and commit**

```bash
RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo doc --workspace --no-deps
rg -n 'BTreeMap<String, R>' $WT/CLAUDE.md $WT/ARCHITECTURE.md $WT/kermit/README.md
git -C $WT add docs/optimisers/cost-based.md docs/optimisers/lexicographic.md docs/optimisers/cardinality.md CLAUDE.md ARCHITECTURE.md kermit/README.md
git -C $WT commit -m "docs: cost-based optimiser and the Database catalog (#81)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HYRXJE5mQn8ZJjRgxMur8D"
```

Expected: `cargo doc` clean, and the `rg` finds no stale store description.

---

## Task 12 [controller]: Gate and hand-off

- [ ] **Step 1: Review the whole branch**

Dispatch one opus code reviewer over `git -C $WT diff 27bb10b..HEAD`, with
the spec, and ask "which mutant would survive these tests?". Fix findings
with new commits (never amend).

- [ ] **Step 2: The CI gate, locally**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace
RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets --verbose
nix develop $WT --command cargo fmt --all --check
RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo doc --workspace --no-deps
setsid nohup nix develop $WT --command bash -c 'cargo miri setup && MIRIFLAGS=-Zmiri-disable-isolation CARGO_BUILD_JOBS=2 cargo miri test --workspace --exclude kermit --exclude kermit-bench' > $SCRATCH/miri.log 2>&1 & disown
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit
KERMIT_BIN=$WT/target/debug/kermit uv --directory $WT/python/kermit-lab run --group test pytest
```

Expected:
- All green.
- Miri takes about 5 minutes; check `$SCRATCH/miri.log` for `test result`
  lines with 0 failed. The WatDiv plan fixture runs under miri.
- If `fmt --check` fails only on comment wrapping, compare the flake's
  nightly with CI's (CLAUDE.md gotcha) before reformatting.

- [ ] **Step 3: Checkpoint 2 with the user**

Report:
- the commits;
- test counts against `$SCRATCH/baseline.txt`;
- the parity and smoke results from `$SCRATCH/verification.md`;
- the mutation-check table.

Then follow superpowers:finishing-a-development-branch. Landing is the
user's call: fetch, merge `origin/master` (never rebase), re-run the gate,
then `git push origin HEAD:master` only on their instruction. Offer to post
the parity and smoke results on issue #81, and to tick its acceptance
boxes.
