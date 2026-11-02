# Join-Key Trie Levels Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The owner chose **inline execution** (superpowers:executing-plans) for this plan.

**Goal:** Implement #116 as `docs/specs/2026-11-02-join-key-levels-design.md` (commit a602de8) designs it. Step 1 makes every optimiser plan payload-last. Step 2 adds a `HashTrie` Layout, `--ds-layout-levels all|keys`, whose `keys` value builds levels only up to an index's last join-key column.

**Architecture:** `analyse` marks the join keys, and `order_by_rank` and cost-based's search put them first. `plan_indexes` gives each `IndexSpec` a level count under a `TrieLevels` policy, and `prepare` takes the policy from the structure (`Relation::TRIE_LEVELS`). The count reaches the structure in its `RelationHeader`. `HashTrie<H, P, E, L: LevelPolicy>` reads it under `KeyLevels` and ignores it under `AllLevels`. Hash Triejoin descends an atom only at its levels' positions, skips a position no trie has a level for, and runs its leaf product unchanged.

**Tech Stack:** Rust nightly workspace (kermit-iters, kermit-ds, kermit-algos, kermit), clap, Python kermit-lab (uv, pytest).

**The spec is the record.** Every design decision is in the spec. Where this plan and the spec disagree, the spec wins; stop and ask the owner.

---

## Conventions for every task

- **Branch:** `aidanb/flat-vec-in-out` (loom). No checkout, rebase, new branch or worktree. Never bare `git stash`.
- **Commit by pathspec,** one commit per task:
  ```bash
  git add -- <paths> && git commit -m "<message>" -- <paths>
  ```
  End every message with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_0177pqmFxfPofJ3SdgA4q2V3
  ```
  New files are marked intent-to-add. If one shows as ` D` after a delete, clear it with `git rm --cached`.
- **Builds:** always `CARGO_BUILD_JOBS=2`, in the foreground (the host's memory monitor kills wide builds). Run `java`-dependent tests inside `nix develop`.
- **Formatting:** only `nix develop --command cargo fmt --all`, never a bare `cargo fmt` (stable rustfmt rewrites ~30 files). Before trusting a local fmt check, refresh the toolchain pin: `nix flake update rust-overlay` (CLAUDE.md's skew gotcha). That lock bump is a commit of its own, and only if it changes anything.
- **Doc comments:** clippy's `doc_markdown` runs under `-Dwarnings`, so backtick every identifier. The binary's docs are never linted by `cargo doc`, so after touching `kermit/src/{options,execution,main}.rs` or `kermit/src/bench/*`, also run
  `cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings`.
- **A task is done when:**
  - its listed tests pass;
  - `cargo test -p <crate>` passes for every crate it touched (`cargo test --workspace` from Task 2 on);
  - `cargo clippy --all-targets` reports no warning.
  Then commit.
- **The full gate** (Task 17, and wherever a task says so):
  ```bash
  export CARGO_BUILD_JOBS=2
  nix develop --command bash -c '
    RUSTFLAGS=-Dwarnings cargo clippy --all-targets &&
    cargo fmt --all --check &&
    RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps &&
    cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings &&
    cargo test --workspace'
  (cd python/kermit-lab && KERMIT_BIN="$PWD/../../target/debug/kermit" uv run --group test pytest)
  ```

---

## Task 0: Bring report v6 into this branch, then a baseline

The spec makes this the owner's step. v6 (local master 2b4d385, 20 commits ahead of origin) must reach origin before step 1 lands. The binary tasks below are written against v6's runner (`bench probe`, `bench/measure.rs`, the `plan` axis).

- [ ] **Step 1: Ask the owner which route.**
  - **(a)** Push local master to origin now (`git push origin master:master`, a fast-forward), then merge origin/master here.
  - **(b)** Merge local master here now, and push v6 any time before #116 lands.
- [ ] **Step 2: Check that nothing moved.**
  ```bash
  git fetch origin && git rev-list --left-right --count master...origin/master
  ```
  Expected: `20	0`. On any other count, stop and ask.
- [ ] **Step 3: Merge, never rebase.** Under (a), `git merge --no-edit origin/master`; under (b), `git merge --no-edit master`. Expected: a merge commit with no conflicts, since this branch adds only the spec and this plan.
- [ ] **Step 4: Baseline.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop --command cargo test --workspace 2>&1 | tail -30
  ```
  Expected: PASS. Record any failure here as pre-existing before Task 1 starts.

---

## Task 1: Join keys in `QueryAnalysis`

**Files:**
- Modify: `kermit-algos/src/analysis.rs` (`QueryAnalysis`, `analyse`, tests)
- Modify: `kermit-algos/src/optimiser/plan.rs` (the test helper `triangle()` builds a `QueryAnalysis` by hand)

- [ ] **Step 1: Write the failing tests** in `analysis.rs`'s `mod tests`:

```rust
    #[test]
    fn join_keys_are_the_variables_two_atoms_share() {
        // X = 0, P = 1, Y = 2.
        let query: JoinQuery = "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).".parse().unwrap();
        assert_eq!(analyse(&query).join_keys, vec![true, false, true]);
    }

    #[test]
    fn a_variable_repeated_in_one_atom_is_not_a_join_key() {
        let query: JoinQuery = "Q(X) :- R(X, X).".parse().unwrap();
        assert_eq!(analyse(&query).join_keys, vec![false]);
    }

    #[test]
    fn a_single_atom_has_no_join_key() {
        let query: JoinQuery = "Q(X, Y) :- R(X, Y).".parse().unwrap();
        assert_eq!(analyse(&query).join_keys, vec![false, false]);
    }
```

- [ ] **Step 2: Run them.** `cargo test -p kermit-algos analysis` should fail to compile: no field `join_keys`.

- [ ] **Step 3: Implement.**
  - Add the field after `predicate_variables`:
    ```rust
        /// `join_keys[v]`: whether two or more body atoms carry variable `v`
        /// (an atom that carries it twice counts once). Every other variable is
        /// payload: one atom holds it, so no intersection constrains it (#116).
        pub join_keys: Vec<bool>,
    ```
  - In `analyse`, after pass 3, replace the struct literal with:
    ```rust
        // Pass 4: the join keys, the variables two or more atoms carry.
        let num_vars = var_to_index.len();
        let mut carriers = vec![0usize; num_vars];
        for vars in &predicate_variables {
            let mut distinct = vars.clone();
            distinct.sort_unstable();
            distinct.dedup();
            for v in distinct {
                carriers[v] += 1;
            }
        }
        QueryAnalysis {
            num_vars,
            predicate_variables,
            join_keys: carriers.iter().map(|&n| n >= 2).collect(),
        }
    ```
  - Extend `analyse`'s doc with: "A fourth pass marks the join keys (`join_keys`)."
  - In `optimiser/plan.rs`, the helper `fn triangle() -> QueryAnalysis` gains `join_keys: vec![true, true, true],`.

- [ ] **Step 4: Run** `cargo test -p kermit-algos`. Expected: PASS.

- [ ] **Step 5: Commit**: `feat(algos): mark a query's join keys in QueryAnalysis (#116)`.

---

## Task 2: Payload-last `order_by_rank`, for Lexicographic and Cardinality

**Files:**
- Modify: `kermit-algos/src/optimiser/ordering.rs`, `lexicographic.rs`, `cardinality.rs`
- Modify: `kermit-algos/src/optimiser/cost_based.rs`: only `worked_example_plans`' Cardinality assertion
- Modify: every test the change breaks elsewhere (Step 6)

- [ ] **Step 1: Rewrite `ordering.rs`'s tests** for the new signature, and add the payload-last test:

```rust
#[cfg(test)]
mod tests {
    use {super::*, std::cell::Cell};

    /// An analysis of `num_vars` variables whose join keys are `keys`.
    fn analysis(num_vars: usize, keys: &[usize]) -> QueryAnalysis {
        QueryAnalysis {
            num_vars,
            predicate_variables: Vec::new(),
            join_keys: (0..num_vars).map(|v| keys.contains(&v)).collect(),
        }
    }

    #[test]
    fn the_rank_alone_decides_with_the_canonical_index_as_tie_break() {
        let all = analysis(3, &[0, 1, 2]);
        assert_eq!(order_by_rank(&all, |v| v), vec![0, 1, 2]);
        assert_eq!(order_by_rank(&all, |v| 2 - v), vec![2, 1, 0]);
        // Equal ranks fall back to the canonical index.
        assert_eq!(order_by_rank(&all, |_| 0), vec![0, 1, 2]);
        assert_eq!(order_by_rank(&all, |v| v == 0), vec![1, 2, 0]);
        assert_eq!(order_by_rank(&analysis(0, &[]), |v| v), Vec::<usize>::new());
    }

    #[test]
    fn join_keys_go_before_payload_whatever_their_rank() {
        // 1 is the only join key: it goes first even when ranked last.
        let one_key = analysis(3, &[1]);
        assert_eq!(order_by_rank(&one_key, |v| v), vec![1, 0, 2]);
        assert_eq!(order_by_rank(&one_key, |v| 2 - v), vec![1, 2, 0]);
        assert_eq!(order_by_rank(&one_key, |v| usize::from(v == 1)), vec![1, 0, 2]);
    }
}
```

  Keep the file's existing test that counts `rank` calls (the reason for `Cell`), with its `order_by_rank(n, …)` calls changed to `order_by_rank(&analysis(n, &(0..n).collect::<Vec<_>>()), …)`.

- [ ] **Step 2: Rewrite the plan pins.** In `lexicographic.rs`:
  - Replace `a_late_canonical_variable_is_not_forced_first`, whose K became a join key and Y payload, with a query in which both are join keys, so it keeps its point:
    ```rust
        #[test]
        fn a_late_canonical_variable_is_not_forced_first() {
            // K (canonical 1) is the first column of r, but nothing forces it
            // before Y (canonical 0): both are join keys, and the index is
            // built in plan order.
            let q: JoinQuery = "Q(Y) :- r(K, Y), s(Y), Const_c7(K).".parse().unwrap();
            let plan = LexicographicOptimiser.plan(&q, &CatalogStats::default());
            assert_eq!(plan.variable_ordering, vec![0, 1]);
        }
    ```
  - Add:
    ```rust
        /// Payload last (#116): P is canonically second but carried by R alone.
        #[test]
        fn payload_follows_the_join_keys() {
            let q: JoinQuery = "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).".parse().unwrap();
            let plan = LexicographicOptimiser.plan(&q, &CatalogStats::default());
            assert_eq!(plan.variable_ordering, vec![0, 2, 1]);
        }
    ```

  In `cardinality.rs`:
  - In `shared_variable_ranks_by_its_smallest_relation`, X is now first only because it is a join key. Make Y a join key too, so the fold is tested again. The sizes become `&[("Big", 1000), ("Small", 2), ("Other", 5), ("Another", 6)]`, and the two queries become `Q(X, Y) :- Big(X), Small(X), Other(Y), Another(Y).` and `Q(X, Y) :- Small(X), Big(X), Other(Y), Another(Y).`. Both still expect `vec![0, 1]`. A max fold or a first-wins bug ranks X at 1000 against Y's 6 or 5, and a last-wins bug fails the second shape.
  - In `the_smallest_relation_goes_first_regardless_of_column_order`, make X a join key with `U(X)`, so the ranking alone still decides. The query becomes `Q(X, Y) :- R(X, Y), S(Y), U(X).` with sizes `&[("R", 1000), ("S", 1), ("U", 1000)]`, still expecting `vec![1, 0]`.
  - Add:
    ```rust
        /// Payload last (#116): P's relation is the smallest, but P is payload,
        /// so the join key X still goes first.
        #[test]
        fn a_payload_variable_follows_every_join_key_whatever_its_rank() {
            let q: JoinQuery = "Q(X, P) :- R(P), S(X), T(X).".parse().unwrap();
            let sizes: &[(&str, usize)] = &[("R", 1), ("S", 1000), ("T", 1000)];
            let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, sizes));
            assert_eq!(plan.variable_ordering, vec![0, 1]);
        }
    ```

  In `cost_based.rs`, `worked_example_plans`: Cardinality's plan of the worked example becomes `vec![1, 2, 0]`, since B (12) and Y (100) are join keys and A is payload. Update the assertion, and the doc comment's plans table if it lists Cardinality's order.

- [ ] **Step 3: Run** `cargo test -p kermit-algos optimiser`. Expected: FAIL, since `order_by_rank` still takes `num_vars`.

- [ ] **Step 4: Implement `order_by_rank`:**

```rust
use crate::analysis::QueryAnalysis;

/// Computes a *global attribute order* (GAO): the join keys of `analysis`
/// first, then the payload, each group sorted by `rank`, with the canonical
/// index as the final deterministic tie-break.
///
/// Payload last (#116): a payload variable is carried by one atom, so binding
/// it early only multiplies the search below it, and a `HashTrie` under
/// `--ds-layout-levels keys` has no level for it at all. A query builds each
/// of its indexes with the columns in plan order, so no stored column order
/// constrains the plan: an ordering policy can be slow, never wrong. `rank`
/// maps a variable to an [`Ord`] key and is evaluated at most once per
/// variable, cheap enough for statistics-driven policies.
pub fn order_by_rank<K: Ord>(analysis: &QueryAnalysis, rank: impl Fn(usize) -> K) -> Vec<usize> {
    let mut order: Vec<usize> = (0..analysis.num_vars).collect();
    order.sort_by_cached_key(|&v| (!analysis.join_keys[v], rank(v), v));
    order
}
```

  - `lexicographic.rs`:
    ```rust
    variable_ordering: order_by_rank(&analyse(query), |v| v),
    ```
  - `cardinality.rs`:
    ```rust
    variable_ordering: order_by_rank(&analysis, |v| min_size[v]),
    ```
  - Update both structs' doc comments to say that join keys come first.

- [ ] **Step 5: Run** `cargo test -p kermit-algos`. Expected: PASS.

- [ ] **Step 6: Run the workspace, and fix every pin the new plans break.**
  ```bash
  CARGO_BUILD_JOBS=2 cargo test --workspace 2>&1 | grep -E "^test .* FAILED|panicked" | head -40
  ```
  Known candidates, all at 2607a04:
  - the `lifecycle.rs` tests that pin `prepared.indexes()` (:460–:595);
  - `kermit/src/db.rs`'s `indexes().len() == 1` (:682);
  - `kermit/tests/subject_position_constant.rs` (:140);
  - v6's CLI tests that pin a `plan` axis or an `index` line (`cli_bench_probe.rs`, `cli_bench_join_axes.rs`, `cli_join_tests.rs`).

  For each failure:
  - **(a)** Check the new plan is payload-last: join keys first by rank, then payload by rank.
  - **(b)** Check the expected index specs follow from that plan: columns sorted by plan position, identical specs merged.
  - **(c)** Update the pin, and its comment if the comment states the plan.

  Never weaken an assertion to make it pass. If a test's point depended on payload going first, rewrite its query so the point survives (as in Step 2), and say so in the commit message.

- [ ] **Step 7: Run** `CARGO_BUILD_JOBS=2 cargo test --workspace`. Expected: PASS, apart from Task 0's recorded failures.

- [ ] **Step 8: Commit**: `feat(algos)!: lexicographic and cardinality plan payload last (#116 step 1)`. List any rewritten test intent in the body.

---

## Task 3: Cost-based searches the join keys only

**Files:**
- Modify: `kermit-algos/src/optimiser/cost_based.rs`
- Modify: `kermit-algos/tests/cost_based_watdiv_plans.rs` (the five pins)

- [ ] **Step 1: Change the tests first.**
  - `estimate(q, stats, vars)`, the test helper, builds its model with every variable boundable:
    ```rust
    let model = CostModel::new(q, &analysis.predicate_variables, identity_bits(analysis.num_vars), stats);
    ```
    with:
    ```rust
    /// Every variable boundable, at its own bit: the model the estimate
    /// tables are written against.
    fn identity_bits(num_vars: usize) -> Vec<Option<usize>> { (0..num_vars).map(Some).collect() }
    ```
    `search_and_enumerate`'s model uses `identity_bits` too.
  - `equal_costs_take_the_lexicographically_smaller_order`. X and Y must be join keys for the search to tie them, so both queries gain the mirrored atoms: `"Q(X, Y) :- R(X), R(Y), S(X), S(Y)."` and `"Q(Y, X) :- R(X), R(Y), S(X), S(Y)."`, with stats `&[("R", 10, &[10]), ("S", 10, &[10])]`. Both orders cost 10 + 100. The expected plans stay `vec![0, 1]`.
  - `the_budget_counts_every_set_reached`. Only B and Y are join keys (A is payload), so the search reaches 2 + 1 = 3 sets:
    ```rust
        #[test]
        fn the_budget_counts_every_set_reached() {
            // Only B and Y are join keys (A is payload), so the search reaches
            // the 2 + 1 = 3 non-empty sets of them.
            let (q, stats) = worked_example();
            let fits = CostBasedOptimiser { state_budget: 3 }.plan(&q, &stats);
            assert_eq!(fits.variable_ordering, vec![2, 1, 0]);
            let cardinality = CardinalityOptimiser.plan(&q, &stats);
            assert_eq!(CostBasedOptimiser { state_budget: 2 }.plan(&q, &stats), cardinality);
            assert_eq!(CostBasedOptimiser { state_budget: 0 }.plan(&q, &stats), cardinality);
        }
    ```
  - `more_than_64_variables_get_cardinalitys_plan` becomes 65 join keys. Each variable sits in two atoms, so the guard still trips:
    ```rust
        #[test]
        fn more_than_64_join_keys_get_cardinalitys_plan() {
            let body: Vec<String> = (0..65).map(|v| format!("R(V{v}), S(V{v})")).collect();
            let q: JoinQuery = format!("Q(V0) :- {}.", body.join(", ")).parse().unwrap();
            let stats = stats_for(&q, &[("R", 10, &[10]), ("S", 10, &[10])]);
            assert_eq!(
                CostBasedOptimiser::default().plan(&q, &stats),
                CardinalityOptimiser.plan(&q, &stats)
            );
        }
    ```
  - New: the guard counts join keys, not variables:
    ```rust
        /// The worked example with 64 payload atoms added: 67 variables, but
        /// two join keys, so the search runs and picks Y before B (cost
        /// 10 + 24 against 12 + 24), where Cardinality picks B (12 rows) first.
        #[test]
        fn many_payload_variables_leave_the_search_to_the_join_keys() {
            let payload: Vec<String> = (0..64).map(|v| format!("p(V{v})")).collect();
            let text = format!(
                "Q(A, B, Y) :- small(A, Y), link(c7, B), owns(B, Y), {}.",
                payload.join(", ")
            );
            let accesses = crate::extract_accesses(&text.parse().unwrap()).unwrap();
            let mut relations: Vec<(String, usize, Vec<usize>)> = vec![
                ("@0".into(), 100, vec![100, 10]),
                ("@1".into(), 12, vec![12]),
                ("@2".into(), 40_000, vec![20_000, 10]),
            ];
            relations.extend((3..67).map(|k| (format!("@{k}"), 5, vec![5])));
            let borrowed: Vec<(&str, usize, &[usize])> = relations
                .iter()
                .map(|(name, tuples, distinct)| (name.as_str(), *tuples, distinct.as_slice()))
                .collect();
            let stats = stats_for(&accesses.query, &borrowed);
            let plan = CostBasedOptimiser::default().plan(&accesses.query, &stats).variable_ordering;
            assert_eq!(&plan[..2], &[2, 1], "the search's key order, not Cardinality's");
            assert_eq!(plan.len(), 67);
        }
    ```
    The access ids follow body order: the three join atoms are `@0`–`@2`, and the payload atoms `@3`–`@66`. If `stats_for`'s signature differs, adapt the collection, not the assertions.
  - `the_cheap_side_goes_first`. Make X a join key with `U(X)`, so the cost alone decides: the query becomes `"Q(X, Y) :- R(X, Y), S(Y), U(X)."` and the stats `&[("R", 1000, &[1000, 1000]), ("S", 1, &[1]), ("U", 1000, &[1000])]`, still expecting `vec![1, 0]`. Here est({Y}) = 1 and est({X}) = 1000.
  - `the_most_selective_column_goes_first`. One atom makes every variable payload, so the query becomes the self-join `"Q(X, Y, Z) :- T(X, Y, Z), T(X, Y, Z)."`, with the same stats `&[("T", 1000, &[10, 5, 7])]`, still expecting `vec![1, 2, 0]`. The comment's numbers become:
    - est({Y}) = 5;
    - est({Y, Z}) = 35, beating est({X, Y}) = 50;
    - every order ends at est({X, Y, Z}) = 1000² / 350.
  - `search_and_enumerate`, the brute force, enumerates only orders the search can return: each order of the join keys, followed by the payload in canonical order:
    ```rust
        let keys: Vec<usize> = (0..analysis.num_vars).filter(|&v| analysis.join_keys[v]).collect();
        let payload: Vec<usize> = (0..analysis.num_vars).filter(|&v| !analysis.join_keys[v]).collect();
        for key_order in every_order(keys.len()) {
            let order: Vec<usize> =
                key_order.iter().map(|&k| keys[k]).chain(payload.iter().copied()).collect();
            let cost = cost_of(&model, &order);
            if best.as_ref().is_none_or(|(least, _)| cost < *least) {
                best = Some((cost, order));
            }
        }
    ```
  - `the_search_finds_the_cheapest_order`'s "nontrivial" count compares against Lexicographic's plan rather than the identity order: `searched.plan != LexicographicOptimiser.plan(&q, &stats).variable_ordering`, where `stats` is built as `search_and_enumerate` builds it (return it in `Searched` if needed). Run once, record the observed count in the comment, and set the floor to about three quarters of it, as the original floor (160 of an observed 215) was.

- [ ] **Step 2: Run** `cargo test -p kermit-algos cost_based`. Expected: FAIL to compile, since `CostModel::new` takes `num_vars`.

- [ ] **Step 3: Implement the search over join keys.**
  - `CostModel` loses `num_vars` and gains `bits: Vec<Option<usize>>`, with the doc: "`bits[v]`: variable `v`'s bit in a set of bound variables, or `None` for a variable the search never binds (a payload variable)". In `CostModel::new`, the parameter `num_vars: usize` becomes `bits: Vec<Option<usize>>`.
  - In `estimate`:
    ```rust
    let bound_columns: Vec<usize> = (0..vars.len())
        .filter(|&c| self.bits[vars[c]].is_some_and(|b| contains(bound, b)))
        .collect();
    ```
    and `distinct_of` has `self.bits.len()` entries.
  - `plan` becomes:
    ```rust
    fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan {
        let analysis = analyse(query);
        // The search orders the join keys only (#116); the payload follows in
        // canonical order. Every key order ends with every key bound, so the
        // payload's estimates are the same after each, and leaving them out
        // cannot change which key order is cheapest.
        let keys: Vec<usize> = (0..analysis.num_vars).filter(|&v| analysis.join_keys[v]).collect();
        let model = CostModel::new(
            query,
            &analysis.predicate_variables,
            key_bits(&keys, analysis.num_vars),
            stats,
        );
        let Some(mut variable_ordering) = cheapest_order(&model, &keys, self.state_budget) else {
            return CardinalityOptimiser.plan(query, stats);
        };
        variable_ordering.extend((0..analysis.num_vars).filter(|&v| !analysis.join_keys[v]));
        QueryPlan { variable_ordering }
    }
    ```
    with:
    ```rust
    /// The bits the search gives the join keys: `keys[b]` (a canonical
    /// variable) is bit `b`, in canonical order, and every other variable has
    /// none. So a set of bound keys fits a `u64` whenever there are at most 64
    /// join keys, however many variables are payload, and comparing orders
    /// bit by bit compares them canonically, which keeps the tie-break.
    fn key_bits(keys: &[usize], num_vars: usize) -> Vec<Option<usize>> {
        let mut bits = vec![None; num_vars];
        for (b, &v) in keys.iter().enumerate() {
            bits[v] = Some(b);
        }
        bits
    }
    ```
  - `cheapest_order(model, keys: &[usize], budget)`:
    - its guard is `keys.len() > 64`;
    - its layers run `keys.len()` times;
    - it extends a set by each `b` in `0..keys.len()` not yet in it, with `extended = bound | bit(b)`;
    - it pushes `keys[b]` onto `order`, so `order` holds canonical variables and `Partial::beats` compares them as before.

    Update its doc: "The cheapest order of the join keys `keys` … `None` past 64 join keys …".
  - Update the struct and module docs: the search is over join keys, with payload last.
  - `CostModel::new`'s debug assertion (one variable per column) is unchanged.

- [ ] **Step 4: Run** `cargo test -p kermit-algos`. The unit tests should PASS. `cost_based_watdiv_plans.rs` may FAIL on its five pins.

- [ ] **Step 5: Regenerate the WatDiv pins.**
  - Run `cargo test -p kermit-algos --test cost_based_watdiv_plans -- --nocapture`, and read each actual plan from the failure message.
  - Check that each is payload-last on its query: every variable two atoms carry comes before every other.
  - Update the five pins (q0020, q0034, q0008, q0264, q0085) and any comment that narrates them.
  - The budget-fit checks must still pass unchanged, since fewer variables reach fewer sets.
  - Re-run: PASS.

- [ ] **Step 6: Run** `CARGO_BUILD_JOBS=2 cargo test --workspace`. Expected: PASS. Fix any cost-based plan pinned elsewhere by Task 2's Step 6 rule.

- [ ] **Step 7: Commit**: `feat(algos)!: cost-based searches the join keys only (#116 step 1)`.

---

## Task 4: Step 1's end-to-end test and documentation

**Files:**
- Modify: `kermit/tests/all_orders.rs` (the new test)
- Modify: `docs/optimisers/lexicographic.md`, `cardinality.md`, `cost-based.md`
- Modify: `docs/specs/bench-report-schema.md` (a no-bump row)
- Modify: `ARCHITECTURE.md` (planning)
- Modify: `CLAUDE.md` (the `QueryOptimiser` bullet)

- [ ] **Step 1: Write the test** in `all_orders.rs`. Imports to add: `clap::ValueEnum`, `kermit_algos::{analyse, CostBasedOptimiser, Optimiser}`.

```rust
/// Step 1 of #116: every optimiser plans the join keys before the payload,
/// on every shape this file runs. Cost-based is also run with a budget of
/// one set, which forces its Cardinality fallback.
#[test]
fn every_optimiser_plans_payload_last() {
    let mut planners: Vec<(String, Planner)> = Optimiser::value_variants()
        .iter()
        .map(|optimiser| (optimiser.axis_value().to_string(), Planner::new(optimiser.instantiate())))
        .collect();
    planners.push((
        "cost-based, budget 1".to_string(),
        Planner::new(Box::new(CostBasedOptimiser { state_budget: 1 })),
    ));
    for shape in shapes() {
        let relations = random_data(&mut Lcg(0), shape.arities);
        let catalog = catalog_at(&relations, StatisticsLevel::ColumnDistinct);
        let query: JoinQuery = shape.query.parse().unwrap();
        for (name, planner) in &planners {
            let prepared = prepare(&catalog, &query, planner, TrieLevels::All)
                .unwrap_or_else(|e| panic!("{} under {name}: {e}", shape.name));
            let analysis = analyse(prepared.query());
            let order = &prepared.plan().variable_ordering;
            let first_payload =
                order.iter().position(|&v| !analysis.join_keys[v]).unwrap_or(order.len());
            assert!(
                order[first_payload..].iter().all(|&v| !analysis.join_keys[v]),
                "{} under {name}: plan {:?} puts a join key after payload",
                shape.name,
                prepared.variable_order()
            );
        }
    }
}
```

  Factor `catalog_for`'s body into `catalog_at(relations, level)`, with `catalog_for(relations)` calling `catalog_at(relations, StatisticsLevel::TupleCounts)`. Adapt `Lcg(0)` to the file's seeding helper if its constructor differs.

  `prepare` gains its policy argument in Task 7. Until then, write the call without `TrieLevels::All`, and add the argument in Task 7.

  Also add the pinned queries (`pinned()`) to the loop if their shapes are not among `shapes()`.

- [ ] **Step 2: Run** `CARGO_BUILD_JOBS=2 cargo test -p kermit --test all_orders every_optimiser`. Expected: PASS, since Tasks 2–3 made it true. To check that the test can fail, temporarily revert `order_by_rank`'s key to `(rank(v), v)`, see it FAIL, then restore the key. Commit only the restored code (the memory note on mutation false negatives).

- [ ] **Step 3: Document step 1.** In each optimiser doc, add a "Payload last (#116)" paragraph saying:
  - **What:** join keys are the variables two or more atoms of the access query share; join keys come first, then the payload.
  - **Lexicographic and Cardinality:** each group is ranked as before.
  - **Cost-based:** the DP runs over the join keys, renumbered `0..k`; the guard and the budget count keys; the payload follows in canonical order; the suffix argument; fewer fallbacks.
  - **The claims,** copied from the spec's "What step 1 changes": no more key bindings, never worse under `keys`, the payload walk under `all` with its 2,001 against 3,000 example, and the `path3` index split.

  Replace any sentence these docs have that says "the ranking alone decides" without the payload rule.

  Then:
  - **`bench-report-schema.md`:** add a version-table row:
    ```
    | 6 (no bump) | 2026-11-02 | Plans are payload last (#116 step 1): every optimiser orders the join keys first, so a query with a payload variable can change plan, and so its indexes, on every structure. Rows are told apart by the `plan` axis (v6), as the seek-default and allocator changes were told apart by their axes. |
    ```
  - **`ARCHITECTURE.md`'s planning section:** one paragraph on join keys and payload last.
  - **`CLAUDE.md`, Key Trait Hierarchy → `QueryOptimiser`:** add "Every optimiser plans payload last: the join keys (variables two or more atoms carry, `QueryAnalysis::join_keys`) first, then the payload (#116)." Also update "Adding a new query optimiser" step 2: `order_by_rank(analysis, rank)` puts join keys first.

- [ ] **Step 4: Run the full gate** (Conventions). Expected: PASS.

- [ ] **Step 5: Commit**: `test,docs: every optimiser plans payload last (#116 step 1)`.

---

## Task 5: `with_hash_trie_layout!` becomes a nested macro (no behaviour change)

**Files:**
- Modify: `kermit/src/options.rs`, the macro and its doc (at 2607a04 :546–:650)

- [ ] **Step 1: Replace the macro.** Its callers keep their syntax.

```rust
/// Monomorphises `$body` over the `HashTrie` Layout cell selected at
/// runtime.
///
/// The dispatchers (`dispatch_ds_bench` and `dispatch_run_bench`, in
/// `bench/ds.rs` and `bench/run.rs`) and `write_query_csv` (in `main.rs`)
/// all go through here, so the Layout product is written out once rather
/// than once per caller. It expands one dimension at a time (`@hasher`, then
/// `@pruning`, then `@expansion`): each rule matches one choice and declares
/// one alias, then hands the rest on. So a dimension costs one rule, though
/// the cells still double with each (2^n monomorphisations of `$body`).
///
/// Hygiene contract: the identifiers named in the closure-like pattern
/// become *type aliases* scoped to the arm that declares them and every arm
/// nested in it, so `$body` must not need a different type of any of those
/// names.
macro_rules! with_hash_trie_layout {
    (@hasher $choice:expr; $H:ident; $then:expr) => {
        match $choice {
            | $crate::options::HasherChoice::Sip => {
                type $H = ::kermit_iters::SipHashStrategy;
                $then
            },
            | $crate::options::HasherChoice::Fxhash => {
                type $H = ::kermit_iters::FxHashStrategy;
                $then
            },
        }
    };
    (@pruning $choice:expr; $P:ident; $then:expr) => {
        match $choice {
            | $crate::options::PruningChoice::Off => {
                type $P = ::kermit_ds::NoPruning;
                $then
            },
            | $crate::options::PruningChoice::On => {
                type $P = ::kermit_ds::SingletonPruning;
                $then
            },
        }
    };
    (@expansion $choice:expr; $E:ident; $then:expr) => {
        match $choice {
            | $crate::options::ExpansionChoice::Eager => {
                type $E = ::kermit_ds::EagerExpansion;
                $then
            },
            | $crate::options::ExpansionChoice::Lazy => {
                type $E = ::kermit_ds::LazyExpansion;
                $then
            },
        }
    };
    ($hasher:expr, $pruning:expr, $expansion:expr, | $H:ident, $P:ident, $E:ident | $body:expr) => {
        $crate::options::with_hash_trie_layout!(@hasher $hasher; $H;
            $crate::options::with_hash_trie_layout!(@pruning $pruning; $P;
                $crate::options::with_hash_trie_layout!(@expansion $expansion; $E; $body)))
    };
}
```

  The `@` rules come first, because an `@` token cannot start an `expr` fragment. If rustc rejects nesting the `type` aliases (E0401 or a resolution error), make each `$then` a block, `{ $then }`. If it still fails, stop and report the error rather than reverting to hand-written arms.

- [ ] **Step 2: Run** the options tests (the macro's own test, near :1048 at 2607a04) and the binary's build:
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit options && CARGO_BUILD_JOBS=2 cargo build -p kermit
  ```
  Expected: PASS. Also run `cargo test -p kermit --test cli_hash_trie_layout_expansion --test cli_hash_trie_layout_pruning --test cli_hash_trie_hasher_choice`, which drive every cell end to end: PASS.

- [ ] **Step 3: Binary rustdoc check** (Conventions): PASS.

- [ ] **Step 4: Commit**: `refactor(cli): expand the HashTrie Layout product one dimension at a time`.

---

## Task 6: `TrieLevels`, `IndexSpec::levels` and `plan_indexes(…, levels)`

**Files:**
- Create: `kermit-iters/src/trie_levels.rs`
- Modify: `kermit-iters/src/lib.rs` (`mod trie_levels;` and `pub use trie_levels::TrieLevels;`)
- Modify: `kermit-ds/src/lib.rs` (re-export `TrieLevels` beside `Filter`)
- Modify: `kermit-algos/src/index_plan.rs`
- Modify: `kermit/src/db/lifecycle.rs` (its `plan_indexes` call passes `TrieLevels::All` until Task 7)
- Modify: every `IndexSpec { … }` literal the compiler flags (the `index_plan.rs` test helper `spec`, the lifecycle tests, `kermit/tests/subject_position_constant.rs`)

- [ ] **Step 1: Create `kermit-iters/src/trie_levels.rs`:**

```rust
//! Which columns of an index its trie has levels for (#116).

/// Which columns of an index its trie has levels for, the rest being payload
/// kept in its leaf rows. `kermit::db::prepare` plans each index's level
/// count under the policy of the structure that builds it
/// (`Relation::TRIE_LEVELS`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TrieLevels {
    /// A level per column: every structure's trie before #116, and the
    /// sorted tries' always.
    #[default]
    All,
    /// Levels up to the index's last join-key column, a join key being a
    /// variable two or more atoms of the query carry. On a payload-last plan
    /// that is one level per join key, the trie of Freitag et al., VLDB 2020
    /// §3.1. `HashTrie` builds it under `--ds-layout-levels keys`.
    JoinKeys,
}
```

- [ ] **Step 2: Write the failing tests** in `index_plan.rs`'s `mod tests`:
  - The helper `spec(relation, filter, columns)` sets `levels: columns.len()`.
  - `planned` becomes a call of a new `planned_with`, which is followed by the new tests:

```rust
    fn planned_with(text: &str, order: &[&str], levels: TrieLevels) -> IndexPlan {
        let accesses = accesses_of(text);
        let plan = plan_of(&accesses.query, order);
        plan_indexes(&accesses, &plan, levels)
    }

    fn planned(text: &str, order: &[&str]) -> IndexPlan { planned_with(text, order, TrieLevels::All) }

    fn levels_of(plan: &IndexPlan) -> Vec<(Vec<usize>, usize)> {
        plan.indexes.iter().map(|spec| (spec.columns.clone(), spec.levels)).collect()
    }

    #[test]
    fn under_all_every_column_is_a_level() {
        let out = planned_with("Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).", &["X", "Y", "P"], TrieLevels::All);
        assert_eq!(levels_of(&out), vec![(vec![0, 1], 2), (vec![0, 1], 2), (vec![0], 1)]);
    }

    #[test]
    fn under_join_keys_a_payload_last_plan_levels_the_join_keys() {
        let out =
            planned_with("Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).", &["X", "Y", "P"], TrieLevels::JoinKeys);
        assert_eq!(levels_of(&out), vec![(vec![0, 1], 1), (vec![0, 1], 2), (vec![0], 1)]);
    }

    /// Rule (iii): a trie keeps levels up to its last join-key column, so a
    /// payload column before a join key stays a level.
    #[test]
    fn under_join_keys_payload_before_a_join_key_stays_a_level() {
        let out =
            planned_with("Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).", &["P", "X", "Y"], TrieLevels::JoinKeys);
        // R is read P then X: its last join key is its second column.
        assert_eq!(levels_of(&out)[0], (vec![1, 0], 2));
    }

    #[test]
    fn under_join_keys_an_index_with_no_join_key_has_no_level() {
        let out = planned_with("Q(X, Y) :- R(X), S(Y).", &["X", "Y"], TrieLevels::JoinKeys);
        assert_eq!(levels_of(&out), vec![(vec![0], 0), (vec![0], 0)]);
    }

    #[test]
    fn under_join_keys_an_index_of_join_keys_keeps_every_level() {
        let out = planned_with(
            "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).",
            &["X", "Y", "Z"],
            TrieLevels::JoinKeys,
        );
        assert!(out.indexes.iter().all(|spec| spec.levels == spec.columns.len()));
    }

    /// SNAP's `path3` under lexicographic's payload-last plan B, C, A, D:
    /// `edge(B, C)` and `edge(C, D)` read `edge` the same way, but at 2 and 1
    /// levels, so only `all` shares them (exact sharing, the spec's
    /// Decision 5).
    #[test]
    fn path3_shares_an_index_under_all_but_not_under_join_keys() {
        let text = "P(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D).";
        let order = &["B", "C", "A", "D"];
        let all = planned_with(text, order, TrieLevels::All);
        assert_eq!(levels_of(&all), vec![(vec![1, 0], 2), (vec![0, 1], 2)]);
        assert_eq!(body_text(&all.query), ["#0(B, A)", "#1(B, C)", "#1(C, D)"]);
        let keys = planned_with(text, order, TrieLevels::JoinKeys);
        assert_eq!(levels_of(&keys), vec![(vec![1, 0], 1), (vec![0, 1], 2), (vec![0, 1], 1)]);
        assert_eq!(body_text(&keys.query), ["#0(B, A)", "#1(B, C)", "#2(C, D)"]);
    }
```

  Adapt `body_text`'s expected strings to the helper's actual format. Read the helper first, and keep the ids and variable orders as shown.

- [ ] **Step 3: Run** `cargo test -p kermit-algos index_plan`. Expected: FAIL to compile.

- [ ] **Step 4: Implement.**
  - The new field on `IndexSpec`, after `columns`:
    ```rust
        /// How many of `columns` the index's trie has levels for, the rest
        /// being payload kept in its leaf rows (#116): `columns.len()` under
        /// [`TrieLevels::All`], fewer only under [`TrieLevels::JoinKeys`].
        /// Identical specs merge, so two specs that differ only here are two
        /// indexes.
        pub levels: usize,
    ```
  - The new signature:
    `pub fn plan_indexes(accesses: &AccessQuery, plan: &QueryPlan, levels: TrieLevels) -> IndexPlan`.
  - Its doc gains: "`levels` decides each spec's level count: every column under `All`; under `JoinKeys`, up to the last column whose variable is a join key of `accesses.query` (rule (iii) of the #116 design), or none when no column is."
  - In the body, before the loop:
    ```rust
        let analysis = analyse(source);
        let is_join_key: HashMap<&str, bool> = names
            .iter()
            .enumerate()
            .map(|(v, name)| (name.as_str(), analysis.join_keys[v]))
            .collect();
    ```
  - In the loop, after `order` is sorted:
    ```rust
            let levels = match levels {
                | TrieLevels::All => order.len(),
                | TrieLevels::JoinKeys => order
                    .iter()
                    .rposition(|&kept| is_join_key[access.variables[kept].as_str()])
                    .map_or(0, |last| last + 1),
            };
    ```
  - The spec literal gains `levels,`. The merge (`*existing == spec`) is unchanged.
  - Import `kermit_iters::TrieLevels`, and give the function parameter or the local a distinct name if they clash.
  - In `lifecycle.rs`, `plan_indexes(&access_query, &ordering, TrieLevels::All)`.

- [ ] **Step 5: Fix the literals the compiler flags** with `levels: <columns.len()>`, and run `CARGO_BUILD_JOBS=2 cargo test --workspace`. Expected: PASS.

- [ ] **Step 6: Commit**: `feat(algos): index specs carry a level count under a TrieLevels policy (#116)`.

---

## Task 7: Level counts through the lifecycle

**Files:**
- Modify: `kermit-ds/src/relation.rs` (`RelationHeader`, `Relation::TRIE_LEVELS`), `kermit-ds/src/configured.rs`, `kermit-ds/src/built_with.rs`
- Modify: `kermit/src/db/lifecycle.rs` (`prepare`, `Prepared`, `spec_header`, `build_with`, tests)
- Modify: `kermit/src/db.rs` and `kermit/src/lib.rs` (the entry points pass `R::TRIE_LEVELS`)
- Modify: `kermit/src/bench/workload.rs` (passes `TrieLevels::All` until Task 11), and `kermit/src/execution.rs`'s test call (:744 at 2607a04)
- Modify: the tests' `prepare` calls: `kermit/tests/common/utils.rs` (2), `kermit/tests/all_orders.rs` (2, plus Task 4's test), `kermit/tests/result_allocation.rs` (1)
- Modify: `kermit/src/bench/run.rs` (`describe_index`'s suffix)

- [ ] **Step 1: Write the failing tests.**

  In `relation.rs`'s tests:

```rust
    #[test]
    fn a_header_has_a_level_per_column_until_told_otherwise() {
        let header = RelationHeader::new("r", vec!["a".into(), "b".into(), "c".into()]);
        assert_eq!(header.levels(), 3);
        assert_eq!(header.clone().with_levels(1).levels(), 1);
        assert_eq!(RelationHeader::new_nameless_positional(2).levels(), 2);
        assert_ne!(header.clone().with_levels(1), header, "the level count is part of a header");
    }

    #[test]
    #[should_panic(expected = "levels")]
    fn a_header_cannot_have_more_levels_than_columns() {
        let _ = RelationHeader::new_positional("r", 2).with_levels(3);
    }
```

  In `lifecycle.rs`'s tests, using the module's helpers `catalog`, `parse` and `lexicographic`:

```rust
    /// R(X, P), S(X, Y), T(Y): X and Y are join keys, P payload.
    fn payload_catalog() -> Catalog {
        catalog(vec![("R", 2, vec![]), ("S", 2, vec![]), ("T", 1, vec![])])
    }

    const PAYLOAD_QUERY: &str = "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).";

    #[test]
    fn prepare_records_its_trie_levels_and_gives_them_to_the_specs() {
        let c = payload_catalog();
        let query = parse(PAYLOAD_QUERY);
        let keys = prepare(&c, &query, &lexicographic(), TrieLevels::JoinKeys).unwrap();
        assert_eq!(keys.trie_levels(), TrieLevels::JoinKeys);
        assert_eq!(keys.indexes().iter().map(|s| s.levels).collect::<Vec<_>>(), vec![1, 2, 1]);
        let all = prepare(&c, &query, &lexicographic(), TrieLevels::All).unwrap();
        assert_eq!(all.trie_levels(), TrieLevels::All);
        assert!(all.indexes().iter().all(|s| s.levels == s.columns.len()));
    }

    #[test]
    #[should_panic(expected = "trie levels")]
    fn a_sorted_trie_refuses_a_query_prepared_for_join_key_levels() {
        let c = payload_catalog();
        let keys = prepare(&c, &parse(PAYLOAD_QUERY), &lexicographic(), TrieLevels::JoinKeys).unwrap();
        let _ = build::<TreeTrie>(&c, &keys);
    }
```

  The module's `prepared_for` passes `TrieLevels::All`, and `join_with::<R, JA>` passes `R::TRIE_LEVELS`.

  The reverse direction (a `keys` HashTrie given an `All` query) needs `KeyLevels`, so it is added in Task 12.

  In `kermit/src/bench/run.rs`'s tests:

```rust
    #[test]
    fn an_index_line_names_its_levels_only_when_it_has_fewer_than_columns() {
        let spec = |levels| IndexSpec {
            relation: "edge".into(),
            filter: Filter::default(),
            columns: vec![1, 0],
            levels,
        };
        assert_eq!(describe_index(0, &spec(2), 3, 64), "#0 edge filter - columns [1, 0]: 3 rows, 64 bytes");
        assert_eq!(
            describe_index(0, &spec(1), 3, 64),
            "#0 edge filter - columns [1, 0] levels 1: 3 rows, 64 bytes"
        );
    }
```

  If `Filter` has no `Default`, build the empty filter the way the run.rs tests already do.

- [ ] **Step 2: Run** the three groups. Expected: FAIL to compile.

- [ ] **Step 3: Implement `RelationHeader`.**
  - The field, after `arity`:
    ```rust
        /// How many leading columns a trie over this header has levels for,
        /// at most `arity`: `arity` unless [`with_levels`](Self::with_levels)
        /// says otherwise. `kermit::db` sets it from an index's spec; only a
        /// structure that builds levels by it reads it (#116).
        levels: usize,
    ```
  - Every constructor sets `levels: arity`, and so does any `From<usize>` impl that builds a header.
  - The methods:
    ```rust
        /// This header with a trie of `levels` levels over its first columns.
        ///
        /// # Panics
        ///
        /// If `levels` exceeds the arity.
        pub fn with_levels(mut self, levels: usize) -> Self {
            assert!(
                levels <= self.arity,
                "a header of arity {} cannot have {levels} levels",
                self.arity
            );
            self.levels = levels;
            self
        }

        /// How many leading columns a trie over this header has levels for.
        pub fn levels(&self) -> usize { self.levels }
    ```

- [ ] **Step 4: Implement `Relation::TRIE_LEVELS`.**
  - Beside `KEEPS_DUPLICATES` in `relation.rs`:
    ```rust
        /// Which columns of an index this structure builds levels for, the
        /// policy `kermit::db::prepare` plans its index specs under (#116).
        /// `All` (every column) unless a Layout says otherwise, as
        /// `HashTrie`'s `KeyLevels` does.
        const TRIE_LEVELS: TrieLevels = TrieLevels::All;
    ```
    Import `kermit_iters::TrieLevels`.
  - `configured.rs` and `built_with.rs` forward it: `const TRIE_LEVELS: TrieLevels = R::TRIE_LEVELS;`.

- [ ] **Step 5: Implement the lifecycle.**
  - `Prepared` gains a field and an accessor:
    ```rust
        /// The policy the index specs were planned under.
        levels: TrieLevels,
    ```
    ```rust
        /// The trie-level policy [`prepare`] planned the index specs under:
        /// the policy of the structure the query is meant to be built as.
        pub fn trie_levels(&self) -> TrieLevels { self.levels }
    ```
  - `prepare(catalog, query, planner, levels: TrieLevels)` passes `levels` to `plan_indexes` and stores it. Its doc gains: "`levels` is the policy of the structure that will build the indexes (`R::TRIE_LEVELS`), which decides each spec's level count (#116). `build` refuses a structure with another."
  - `spec_header` ends both branches with `.with_levels(spec.levels)`.
  - `build_with`, first statement:
    ```rust
        assert_eq!(
            prepared.levels,
            R::TRIE_LEVELS,
            "a query prepared for {:?} trie levels cannot be built by a structure with {:?} trie levels",
            prepared.levels,
            R::TRIE_LEVELS,
        );
    ```
    Add a `# Panics` line for it.
  - Entry points:
    - `db.rs` (:60, :86 at 2607a04): `prepare(catalog, query, planner, R::TRIE_LEVELS)?`.
    - `lib.rs`'s `compute_join`: the same.
    - `workload.rs`: `db::prepare(&catalog, &q.query, planner, TrieLevels::All)`, replaced in Task 11.
    - `execution.rs`'s test call: `TrieLevels::All`.
    - The test helpers: `utils.rs`'s `run` and `join_under_planner` pass `R::TRIE_LEVELS`.
    - `all_orders.rs`'s `rows_in_order` passes `R::TRIE_LEVELS`; its index-pin test passes `TrieLevels::All`, and Task 4's test already does.
    - `result_allocation.rs`'s `prebuilt::<R>` passes `R::TRIE_LEVELS`.
    - The lifecycle and `db.rs` tests pass `TrieLevels::All`.

- [ ] **Step 6: Implement `describe_index`** in `kermit/src/bench/run.rs`:

```rust
/// The value of one `index` metadata line, for the `position`th index of a
/// query: `{id} {relation} filter {filter} columns {columns:?}[ levels {n}]:
/// {rows} rows, {bytes} bytes`. ` levels {n}` appears only when the trie has
/// levels for fewer columns than it keeps (`--ds-layout-levels keys`, #116), so
/// every `all` line reads as before. `rows` is what the build copied, the index
/// view's count, and `bytes` the index's footprint as built.
fn describe_index(position: usize, spec: &IndexSpec, rows: usize, bytes: usize) -> String {
    let levels = if spec.levels < spec.columns.len() {
        format!(" levels {}", spec.levels)
    } else {
        String::new()
    };
    format!(
        "{INDEX_ID_PREFIX}{position} {} filter {} columns {:?}{levels}: {rows} rows, {bytes} bytes",
        spec.relation, spec.filter, spec.columns
    )
}
```

- [ ] **Step 7: Run** `CARGO_BUILD_JOBS=2 cargo test --workspace`, and the binary rustdoc check. Expected: PASS.

- [ ] **Step 8: Commit**: `feat(db): prepare plans index levels under the structure's TrieLevels (#116)`.

---

## Task 8: The HashTrie `levels` Layout

**Files:**
- Create: `kermit-ds/src/ds/hash_trie/levels.rs`
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs` (`mod levels;` and `pub use levels::{AllLevels, KeyLevels, LevelPolicy};`), `kermit-ds/src/lib.rs` (re-export the three)
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs`, `bulk.rs`, `radix.rs`, `parallel.rs`, `hash_trie_iter.rs`, `identity.rs`
- Modify: `kermit-iters/src/hash_trie.rs` (`HashTrieIterable::levels`, the `leaf_tuples` doc)
- Modify: `kermit-ds/src/configured.rs`, `kermit-ds/src/built_with.rs` (forward `levels`)

- [ ] **Step 1: Create `levels.rs`:**

```rust
//! The level policy: which columns a `HashTrie` has levels for (#116).
//!
//! [`AllLevels`] (the default) gives every column a level, as before #116:
//! it reads the header's arity and never builds a trie with no level, so its
//! zero-level root is the uninhabited [`Never`] and the instantiation
//! compiles to the trie without the policy. [`KeyLevels`] gives levels to the
//! first `header.levels()` columns, the count `kermit::db::prepare` plans
//! under `TrieLevels::JoinKeys`; the rest stay in the leaf chains, whose rows
//! are whole tuples, and Hash Triejoin's leaf product reads them. Bench axis
//! `ds_layout_levels`.

use {
    super::{
        chain::Chain, expansion::ExpansionPolicy, node::HashTrieNode, pruning::{Never, PruningPolicy},
    },
    crate::relation::RelationHeader,
    kermit_iters::{LayoutOption, TrieLevels},
};

/// What the root of a trie with no level holds under a [`LevelPolicy`]: its
/// one chain of rows. Generic over the chain, so the public policy trait never
/// names the crate-private `Chain` (as `PendingChild<N>` keeps
/// `HashTrieNode` out of `ExpansionPolicy`).
pub trait ZeroLevelRows<C>: Sized + Send {
    /// Wrap `rows` as the root of a trie with no level.
    fn from_rows(rows: C) -> Self;
    /// The rows.
    fn rows(&self) -> &C;
    /// The rows, to append to.
    fn rows_mut(&mut self) -> &mut C;
}

impl<C> ZeroLevelRows<C> for Never {
    fn from_rows(_rows: C) -> Self {
        unreachable!("AllLevels never builds a trie with no level: its level count is the arity")
    }

    fn rows(&self) -> &C { match *self {} }

    fn rows_mut(&mut self) -> &mut C { match *self {} }
}

/// [`KeyLevels`]' zero-level root: the rows themselves.
pub struct ZeroLevel<C>(C);

impl<C: Send> ZeroLevelRows<C> for ZeroLevel<C> {
    fn from_rows(rows: C) -> Self { Self(rows) }

    fn rows(&self) -> &C { &self.0 }

    fn rows_mut(&mut self) -> &mut C { &mut self.0 }
}

/// `HashTrie`'s fourth Layout: how many leading columns its trie has levels
/// for.
pub trait LevelPolicy: LayoutOption + Copy + Default + 'static {
    /// The policy `kermit::db::prepare` plans this trie's index specs under
    /// (`Relation::TRIE_LEVELS`).
    const TRIE_LEVELS: TrieLevels;
    /// What a zero-level root holds: uninhabited under [`AllLevels`].
    type Rows<C: Send>: ZeroLevelRows<C>;
    /// How many leading columns a trie over `header` has levels for.
    fn levels(header: &RelationHeader) -> usize;
}

/// A level per column, the header's level count ignored: the trie before
/// #116, and the default.
#[derive(Copy, Clone, Debug, Default)]
pub struct AllLevels;

impl LayoutOption for AllLevels {
    const NAME: &'static str = "all";
}

impl LevelPolicy for AllLevels {
    const TRIE_LEVELS: TrieLevels = TrieLevels::All;

    type Rows<C: Send> = Never;

    fn levels(header: &RelationHeader) -> usize { header.arity() }
}

/// Levels for the header's first `levels()` columns, the join keys of a
/// payload-last plan: the trie of Freitag et al., VLDB 2020 §3.1
/// (`--ds-layout-levels keys`).
#[derive(Copy, Clone, Debug, Default)]
pub struct KeyLevels;

impl LayoutOption for KeyLevels {
    const NAME: &'static str = "keys";
}

impl LevelPolicy for KeyLevels {
    const TRIE_LEVELS: TrieLevels = TrieLevels::JoinKeys;

    type Rows<C: Send> = ZeroLevel<C>;

    fn levels(header: &RelationHeader) -> usize { header.levels() }
}

/// A trie's root: a table, when it has levels, or else its rows, one chain
/// in source order (`None` while there is no row).
pub(crate) enum Root<P: PruningPolicy, E: ExpansionPolicy, L: LevelPolicy> {
    /// The table of the first level.
    Table(HashTrieNode<P, E>),
    /// The rows of a trie with no level.
    Rows(L::Rows<Option<Chain>>),
}
```

  If `private_interfaces` or `private_bounds` fires on the public trait (as it might for `ZeroLevelRows` in a private module), follow exactly what `expansion.rs` does for `PendingChild`, its visibility and re-exports, rather than adding an `allow`.

- [ ] **Step 2: Thread `L` through `HashTrie`** (`implementation.rs`).
  - The struct gains `L: LevelPolicy = AllLevels`; `root: Root<P, E, L>`; `_layout: PhantomData<(H, P, E, L)>`.
  - Every `impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> … HashTrie<H, P, E>` gains `L: LevelPolicy` and `HashTrie<H, P, E, L>`. This covers the inherent blocks in `implementation.rs`, `bulk.rs` and the radix and parallel helpers that use `Self`, plus `Relation`, `ConfigurableRelation`, `BuildModeRelation`, `ConfiguredBuildModeRelation`, `Projectable`, `HeapSize`, `Cardinality`, `HashTrieIterable` and `HasOptimizationAxes`.
  - Free functions that call `HashTrie::<H, P, E>::helper(…)` for node-building helpers (`radix.rs`, `parallel.rs`) keep calling them through the `AllLevels` instantiation, since those helpers never read `L`.
  - Rename every parameter that means "leaf depth" from `arity` to `levels`:
    - `is_leaf_depth(depth, levels)`, `make_root_sized(levels, …)`, `insert_at(…, levels, …)`;
    - `build`, `build_nested`, `child`, `build_child_table` (`bulk.rs`);
    - `radix::fill_root`, `build_and_merge`, `build_scratch_root`;
    - `parallel::fill_root`, `fill_root_in_morsels`, `fill_presized_root`, `fill_presized_root_in` (including its `if levels <= 1`).

    Update their comments: "the last attribute" becomes "the last level".
  - Their empty-source returns, `Tuples::new(arity)` (radix.rs :123, parallel.rs :177 and :280 at 2607a04), become `Tuples::new(view.arity())`. These are only reached from tests now (next bullet).
  - Add these helpers:
    ```rust
        /// How many levels this trie has: its arity under `AllLevels`, its
        /// header's level count under `KeyLevels`.
        pub(crate) fn level_count(&self) -> usize { L::levels(&self.header) }

        /// Whether a trie over `header` has no level: an index with no join key
        /// under `KeyLevels`. Never under `AllLevels` (a nullary header keeps
        /// its leaf root, as before #116), so the zero-level paths compile away
        /// there.
        fn is_unleveled(header: &RelationHeader) -> bool {
            matches!(L::TRIE_LEVELS, TrieLevels::JoinKeys) && header.arity() > 0 && L::levels(header) == 0
        }

        /// The root table.
        ///
        /// # Panics
        ///
        /// On a trie with no level, which has none.
        pub(crate) fn root(&self) -> &HashTrieNode<P, E> {
            self.root_table().expect("a trie with no level has no root table")
        }

        /// The root table, or `None` for a trie with no level.
        pub(crate) fn root_table(&self) -> Option<&HashTrieNode<P, E>> {
            match &self.root {
                | Root::Table(node) => Some(node),
                | Root::Rows(_) => None,
            }
        }

        /// A trie with no level: its rows as one leaf chain, empty when it has
        /// none. `None` for a trie with levels.
        pub(crate) fn root_rows(&self) -> Option<LeafRows<'_>> {
            match &self.root {
                | Root::Table(_) => None,
                | Root::Rows(rows) => Some(match rows.rows() {
                    | Some(chain) => chain.leaf_rows(&self.tuples, &self.links),
                    | None => LeafRows::new(&self.tuples, &[]),
                }),
            }
        }

        /// A trie with no level over `view`: its matching rows copied once in
        /// source order (`materialize`, the copy `bulk` makes), threaded into
        /// one chain. Every build mode builds this: with no level there is no
        /// key to group or partition on (#116).
        fn unleveled(header: RelationHeader, config: HashTrieConfig, view: &TupleView<'_>) -> Self {
            let tuples = Self::batch_from_view(&header, view);
            let links = ChainLinks::new(tuples.len());
            let mut rows: Option<Chain> = None;
            for row in all_rows(&tuples) {
                match &mut rows {
                    | Some(chain) => chain.push(&links, row),
                    | None => rows = Some(Chain::start(&links, row)),
                }
            }
            Self {
                header,
                tuples,
                links,
                root: Root::Rows(L::Rows::from_rows(rows)),
                config,
                _layout: PhantomData,
            }
        }
    ```
  - `with_config_for`:
    ```rust
    let root = if Self::is_unleveled(&header) {
        Root::Rows(L::Rows::from_rows(None))
    } else {
        Root::Table(Self::make_root_sized(L::levels(&header), config.root_log2_capacity(tuples.len())))
    };
    ```
  - `from_view_with_config_and_build_mode`:
    - It starts with `Self::assert_mode_prerequisites(mode, config);`, a new function holding the existing `incremental` and `presized:N` assertions, moved out of their arms unchanged.
    - Then: `if Self::is_unleveled(&header) { return Self::unleveled(header, config, &view); }`.
    - Then `let levels = L::levels(&header);`. Every arm uses `levels` where it used `header.arity()` as the leaf depth, and `Root::Table(...)` around the root it builds. `BULK_BUILDS` stays in the `bulk` arm.
  - `from_view_partitioned`:
    - `let levels = L::levels(&header);`, and the `fill` closure's last parameter is `levels`.
    - On `count == 0` it returns before calling `fill`: `tuples: Tuples::new(header.arity())`, `root: Root::Table(Self::make_root_sized(levels, log2_capacity))`, with the links and config as built. This is the empty trie every partitioned build makes today, with no partition and no worker. Check that no test hook record is pushed for an empty build, as before: `take_parallel_builds` callers in tests.
    - The trie it returns wraps the root in `Root::Table`.
  - `for_each_tuple`:
    ```rust
    match &self.root {
        | Root::Table(node) => Self::visit_at(&self.tuples, &self.links, node, &mut visit),
        | Root::Rows(rows) => {
            if let Some(chain) = rows.rows() {
                Self::visit_chain(&self.tuples, &self.links, chain, &mut visit);
            }
        },
    }
    ```
  - `resolve`: `self.header.arity()` becomes `self.level_count()`.
  - `Relation::insert`: keep the arity check, then:
    ```rust
    let levels = self.level_count();
    match &mut self.root {
        | Root::Table(node) => {
            Self::insert_at(&self.tuples, &self.links, node, 0, levels, row, self.config.load_factor);
        },
        | Root::Rows(rows) => match rows.rows_mut() {
            | Some(chain) => chain.push(&self.links, row),
            | slot @ None => *slot = Some(Chain::start(&self.links, row)),
        },
    }
    ```
  - `Relation`: `const TRIE_LEVELS: TrieLevels = L::TRIE_LEVELS;`.
  - `HeapSize`: the root's term is `match &self.root { Root::Table(node) => node_heap_bytes(node), Root::Rows(_) => 0 }`.
  - `HasOptimizationAxes`: insert `"ds_layout_levels"` = `<L as LayoutOption>::NAME`.
  - `HashTrieIterable`: `fn levels(&self) -> usize { self.level_count() }`; `hash_trie_iter` builds `HashTrieIter::<H, P, E, L>`.
  - `project`: unchanged. Its nameless header has every level, so it builds a full trie under either policy.
  - Update the struct's `# Invariants`: "The depth of every root-to-leaf path is the level count: `header.arity()` under `AllLevels`, `header.levels()` under `KeyLevels`. A trie with no level has no table: its root is one chain of its rows, in source order."
  - Document `L` beside `P` and `E` in the struct doc.

- [ ] **Step 3: Update the iterator** (`hash_trie_iter.rs`).
  - `HashTrieIter<'a, H, P, E, L: LevelPolicy = AllLevels>`, with `trie: &'a HashTrie<H, P, E, L>`, and every impl gains `L`.
  - `fn arity(&self)` becomes `fn levels(&self) -> usize { self.trie.level_count() }`. Its two uses in `descent` and `leaf_tuples`, the singleton checks, read `self.levels()`.
  - In `descent`:
    ```rust
    | None => match self.trie.root_table() {
        | Some(root) => Descent::Node(root),
        | None => Descent::Blocked,
    },
    ```
  - In `leaf_tuples`: `let Some(frame) = self.stack.last() else { return self.trie.root_rows(); };`, then match `frame` as before.
  - The module's frame-size test stays.

- [ ] **Step 4: Update the trait** (`kermit-iters/src/hash_trie.rs`).
  - `HashTrieIterable` gains:
    ```rust
        /// How many leading columns this structure's trie has levels for: all
        /// of them, or, for an index planned under `TrieLevels::JoinKeys`, those
        /// up to its last join key (#116). A join descends a relation only at
        /// the variables of these columns; the rest are read from its leaf rows.
        fn levels(&self) -> usize;
    ```
  - The `leaf_tuples` doc becomes: "Returns `Some` iff the current node is the last level and the current bucket is occupied, or the iterator stands at the pre-root of a trie with no level (`levels() == 0`), where it lends the whole chain, possibly empty; otherwise `None`."
  - The `open` doc: "Returns `false` at the last level (at the pre-root, for a trie with no level) or if the current bucket is empty."
  - `configured.rs` and `built_with.rs`'s `HashTrieIterable` impls forward `levels`, delegating to the wrapped relation as `hash_trie_iter` does.

- [ ] **Step 5: Update `identity.rs`** (test-only). Every helper generic over `<H, P, E>` that takes a `HashTrie` gains `L: LevelPolicy`. Where it reads `root()`, compare `root_table()`: equal when both are `None`. For two zero-level tries, compare their `root_rows()` row by row (`to_vecs()`), plus the buffers and links where the helper is array-level.

- [ ] **Step 6: Write the unit tests** in `levels.rs`'s `#[cfg(test)] mod tests`. They use the crate-internal `identity` helpers.

```rust
#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            ds::hash_trie::{
                identity, EagerExpansion, HashTrie, HashTrieBuildMode, HashTrieConfig, LazyExpansion,
                NoPruning, RadixBits, RootCapacity, SingletonPruning,
            },
            relation::Relation,
        },
        kermit_iters::SipHashStrategy,
        std::mem::size_of,
    };

    #[test]
    fn root_does_not_grow_under_the_level_policy() {
        assert_eq!(
            size_of::<Root<NoPruning, EagerExpansion, AllLevels>>(),
            size_of::<HashTrieNode<NoPruning, EagerExpansion>>()
        );
        assert_eq!(
            size_of::<Root<SingletonPruning, LazyExpansion, AllLevels>>(),
            size_of::<HashTrieNode<SingletonPruning, LazyExpansion>>()
        );
    }

    fn rows() -> Vec<Vec<usize>> {
        vec![
            vec![1, 10, 100],
            vec![1, 10, 101],
            vec![1, 11, 102],
            vec![2, 20, 103],
            vec![2, 20, 103],
            vec![3, 30, 104],
        ]
    }

    /// `KeyLevels` over a full-depth header builds the `AllLevels` trie.
    #[test]
    fn full_depth_key_levels_build_the_all_levels_trie() {
        let all: HashTrie<SipHashStrategy> = HashTrie::from_tuples(3.into(), rows());
        let keys: HashTrie<SipHashStrategy, NoPruning, EagerExpansion, KeyLevels> =
            HashTrie::from_tuples(3.into(), rows());
        identity::assert_same_trie(&all, &keys, "full depth");
    }
}
```

  The two tries above differ in type. If `identity::assert_same_trie` takes one type, Step 5 gives it a second level-policy parameter (`<H, P, E, L1, L2>`): what it compares (buffers, links and root tables) does not depend on `L`.

  Add the following. Each uses `rows()`, and the existing identity helper the mode's Amendment 2 rung names; read `identity.rs` for their exact names and signatures.
  - **(a) Equivalence at every level count.** For `levels in 1..=3` and each `(P, E)` of `(NoPruning, EagerExpansion)`, `(SingletonPruning, EagerExpansion)`, `(NoPruning, LazyExpansion)` and `(SingletonPruning, LazyExpansion)`, compare against `bulk`:
    - `incremental` must be array-identical;
    - `radix:2` and `parallel:2` must have the same contents;
    - `presized:2` (under `root-capacity=tuples`) must pass `assert_equivalent_trie`.
  - **(b) Zero levels:** every mode builds the same buffer and links as `bulk`, and its `root_rows()` yield the rows in source order.
  - **(c)** `insert`ing `rows()` one by one into `HashTrie::new(header.with_levels(n))` equals the `bulk` build, for `n` in `0..=3`.

  Use a small generic function per check, called once per `(P, E)`, rather than a macro, unless the generics fight the identity helpers' bounds.

- [ ] **Step 7: Run** `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds && CARGO_BUILD_JOBS=2 cargo test --workspace`. Expected: PASS.

  Also run `cargo test -p kermit-ds node_does_not_grow off_frame_is_the_bare_table_pair root_does_not_grow`: PASS. The size pins hold under `AllLevels`.

- [ ] **Step 8: Commit**: `feat(ds): a HashTrie Layout of join-key levels, LevelPolicy (#116 step 2)`.

---

## Task 9: HashTrie level behaviour through the public API

**Files:**
- Create: `kermit-ds/tests/hash_trie_levels.rs`

- [ ] **Step 1: Write the tests.** They are public-API only: `HashTrie`, `KeyLevels`, `RelationHeader::with_levels`, `HashTrieIterable`, `HashTrieIterator`, `from_tuples_with_config_and_build_mode`.

```rust
//! `--ds-layout-levels keys` (#116) at every level count, through the public
//! API: contents, leaf chains of whole rows, where `open` stops, and the
//! zero-level root, for every build mode, pruning and expansion value.

use {
    kermit_ds::{
        EagerExpansion, ExpansionPolicy, HashTrie, HashTrieBuildMode, HashTrieConfig, KeyLevels,
        LazyExpansion, NoPruning, PruningPolicy, RadixBits, Relation, RelationHeader, RootCapacity,
        SingletonPruning, Threads,
    },
    kermit_iters::{HashTrieIterable, HashTrieIterator, SipHashStrategy},
};

type Keys<P, E> = HashTrie<SipHashStrategy, P, E, KeyLevels>;

fn rows() -> Vec<Vec<usize>> {
    vec![
        vec![1, 10, 100],
        vec![1, 10, 101],
        vec![1, 11, 102],
        vec![2, 20, 103],
        vec![2, 20, 103],
        vec![3, 30, 104],
    ]
}

/// Every build mode, each under a config it accepts.
fn modes() -> Vec<(HashTrieBuildMode, HashTrieConfig)> {
    let presized = HashTrieConfig {
        root_capacity: RootCapacity::Tuples,
        ..HashTrieConfig::default()
    };
    vec![
        (HashTrieBuildMode::Bulk, HashTrieConfig::default()),
        (HashTrieBuildMode::Incremental, HashTrieConfig::default()),
        (HashTrieBuildMode::Radix(RadixBits::new(2).unwrap()), HashTrieConfig::default()),
        (HashTrieBuildMode::Parallel(Threads::new(2).unwrap()), HashTrieConfig::default()),
        (HashTrieBuildMode::Presized(Threads::new(2).unwrap()), presized),
    ]
}

fn built<P: PruningPolicy, E: ExpansionPolicy>(
    levels: usize, mode: HashTrieBuildMode, config: HashTrieConfig, rows: Vec<Vec<usize>>,
) -> Keys<P, E> {
    Keys::<P, E>::from_tuples_with_config_and_build_mode(
        RelationHeader::new_positional("r", 3).with_levels(levels),
        config,
        mode,
        rows,
    )
}

fn sorted(mut rows: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
    rows.sort();
    rows
}

fn check_every_level_count<P: PruningPolicy, E: ExpansionPolicy>() {
    for levels in 0..=3 {
        for (mode, config) in modes() {
            let label = format!("{levels} levels, {mode:?}");
            let trie = built::<P, E>(levels, mode, config, rows());
            assert_eq!(trie.levels(), levels, "{label}");

            let mut walked = Vec::new();
            trie.for_each_tuple(|row| walked.push(row.to_vec()));
            assert_eq!(sorted(walked), sorted(rows()), "{label}: contents");

            // Down the first path: `levels` opens succeed, the next fails,
            // and the leaf lends whole rows.
            let mut it = trie.hash_trie_iter();
            for depth in 0..levels {
                assert!(it.open(), "{label}: open at depth {depth}");
            }
            assert!(!it.open(), "{label}: open past the last level");
            let leaf = it.leaf_tuples().expect("at the last level");
            assert!(leaf.len() >= 1, "{label}");
            assert!(leaf.to_vecs().iter().all(|row| row.len() == 3), "{label}: whole rows");
            if levels == 0 {
                assert_eq!(leaf.to_vecs(), rows(), "{label}: one chain in source order");
            }
        }
    }
}

#[test]
fn every_level_count_eager() { check_every_level_count::<NoPruning, EagerExpansion>(); }

#[test]
fn every_level_count_pruned() { check_every_level_count::<SingletonPruning, EagerExpansion>(); }

#[test]
fn every_level_count_lazy() { check_every_level_count::<NoPruning, LazyExpansion>(); }

#[test]
fn every_level_count_pruned_lazy() {
    check_every_level_count::<SingletonPruning, LazyExpansion>();
}

#[test]
fn an_empty_zero_level_trie_lends_an_empty_chain() {
    for (mode, config) in modes() {
        let trie = built::<NoPruning, EagerExpansion>(0, mode, config, Vec::new());
        let it = trie.hash_trie_iter();
        let leaf = it.leaf_tuples().expect("a zero-level trie lends its chain at the pre-root");
        assert_eq!(leaf.len(), 0, "{mode:?}");
    }
}

#[test]
fn insert_into_a_zero_level_trie_appends_to_its_chain() {
    let mut trie: Keys<NoPruning, EagerExpansion> =
        HashTrie::new(RelationHeader::new_positional("r", 3).with_levels(0));
    for row in rows() {
        trie.insert(row);
    }
    assert_eq!(trie.hash_trie_iter().leaf_tuples().unwrap().to_vecs(), rows());
}

#[test]
fn the_axis_names_the_level_policy() {
    let all: HashTrie<SipHashStrategy> = HashTrie::from_tuples(3.into(), rows());
    let keys: Keys<NoPruning, EagerExpansion> = HashTrie::from_tuples(3.into(), rows());
    use kermit_iters::HasOptimizationAxes;
    assert_eq!(all.optimization_axes()["ds_layout_levels"], "all");
    assert_eq!(keys.optimization_axes()["ds_layout_levels"], "keys");
}
```

  - If an iterator's `open` at depth 0 needs a scan position first (it is positioned at the first occupied bucket on open), the first-path walk above is enough. Otherwise follow `hash_trie_tests.rs`'s idiom.
  - If `Threads::new` returns an `Option`, `.unwrap()` stands.
  - Fix import paths to the real re-exports, keeping the assertions.

- [ ] **Step 2: Run** `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --test hash_trie_levels`. Expected: PASS. If a test fails, the bug is in Task 8's code: fix it there, in this commit.

- [ ] **Step 3: Commit**: `test(ds): HashTrie join-key levels at every count, mode and Layout (#116)`.

---

## Task 10: Hash Triejoin descends by level, and skips unlevelled positions

**Files:**
- Modify: `kermit-algos/src/hash/hash_triejoin.rs`

- [ ] **Step 1: Write the failing tests** in its `mod tests`. Build `KeyLevels` tries from headers with levels (`kermit-ds` is a dev-dependency).

```rust
    type Keys = kermit_ds::HashTrie<
        kermit_iters::SipHashStrategy,
        kermit_ds::NoPruning,
        kermit_ds::EagerExpansion,
        kermit_ds::KeyLevels,
    >;

    fn keys_trie(arity: usize, levels: usize, rows: Vec<Vec<usize>>) -> Keys {
        <Keys as kermit_ds::Relation>::from_tuples(
            kermit_ds::RelationHeader::new_nameless_positional(arity).with_levels(levels),
            rows,
        )
    }

    fn rows_of<DS: HashTrieIterable>(
        text: &str, order: &[usize], relations: &[(&str, &DS)],
    ) -> Vec<Vec<usize>> {
        let query: JoinQuery = text.parse().unwrap();
        let plan = QueryPlan { variable_ordering: order.to_vec() };
        let map = relations.iter().map(|(name, ds)| ((*name).to_string(), *ds)).collect();
        let mut rows = Vec::new();
        HashTriejoin::join_for_each(&plan, query, map, |row| rows.push(row.to_vec()));
        rows.sort();
        rows
    }

    /// The spec's worked example: P is payload, skipped at its position and
    /// bound from R's leaf rows; X and Y are joined by level.
    #[test]
    fn payload_is_skipped_at_its_position_and_bound_from_the_leaf_rows() {
        let r = keys_trie(2, 1, vec![vec![1, 10], vec![1, 11], vec![2, 20]]);
        let s = keys_trie(2, 2, vec![vec![1, 5], vec![2, 6]]);
        let t = keys_trie(1, 1, vec![vec![5], vec![7]]);
        let text = "Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).";
        let want = vec![vec![1, 10, 5], vec![1, 11, 5]];
        // Today's lexicographic order (payload before a key), and payload last.
        for order in [&[0, 1, 2][..], &[0, 2, 1][..]] {
            assert_eq!(rows_of(text, order, &[("R", &r), ("S", &s), ("T", &t)]), want, "{order:?}");
        }
    }

    /// Two zero-level atoms: every position is skipped and the leaf product
    /// is their cross product.
    #[test]
    fn atoms_with_no_level_cross_their_whole_chains() {
        let r = keys_trie(1, 0, vec![vec![1], vec![2]]);
        let s = keys_trie(1, 0, vec![vec![7]]);
        assert_eq!(
            rows_of("Q(X, Y) :- R(X), S(Y).", &[0, 1], &[("R", &r), ("S", &s)]),
            vec![vec![1, 7], vec![2, 7]]
        );
        let empty = keys_trie(1, 0, Vec::new());
        assert!(rows_of("Q(X, Y) :- R(X), S(Y).", &[0, 1], &[("R", &r), ("S", &empty)]).is_empty());
    }
```

  Adapt `rows_of` to the existing test helper if one already collects `join_for_each` rows; keep the assertions. Check that the tests' `QueryPlan` validates: each atom's variables are in plan order for the orders used. `R(X, P)` under the plan X, Y, P is X before P, which is valid.

- [ ] **Step 2: Run** `cargo test -p kermit-algos hash_triejoin`. Expected: FAIL. The worked example returns nothing, because `enumerate` returns at P.

- [ ] **Step 3: Implement.**
  - `build_variable_to_iter_map` gains `levels: &[usize]`:
    ```rust
    /// Build `variable_to_iter_map[i]`: the predicates whose trie has a level
    /// for the `i`-th variable of `variable_ordering`, i.e. that carry it among
    /// their first `levels[p]` columns (#116). Under full levels that is every
    /// predicate carrying it, LFTJ's inline construction; under join-key levels
    /// a payload column is no level, and its variable is bound from the leaf
    /// rows instead.
    fn build_variable_to_iter_map(
        variable_ordering: &[usize], predicate_variables: &[Vec<usize>], levels: &[usize],
    ) -> Vec<Vec<usize>> {
        variable_ordering
            .iter()
            .map(|v| {
                predicate_variables
                    .iter()
                    .zip(levels)
                    .enumerate()
                    .filter_map(|(p, (vars, &depth))| vars[..depth].contains(v).then_some(p))
                    .collect()
            })
            .collect()
    }
    ```
  - In `join_for_each`, collect the relations once (`let relations: Vec<&DS> = …`, with the same `expect`), take `iters` from them, then:
    ```rust
    let levels: Vec<usize> = relations.iter().map(|relation| relation.levels()).collect();
    ```
    and pass `&levels`.
  - In `enumerate`:
    ```rust
        let participating = &variable_to_iter_map[i];
        if participating.is_empty() {
            // No trie has a level for this variable: it is payload, bound from
            // the leaf rows once the join keys are (Algorithm 3, lines 16–19).
            // Under full levels every variable is some trie's level, so this
            // never runs.
            enumerate(i + 1, arity, iters, predicate_variables, variable_to_iter_map, scratch, emit);
            return;
        }
    ```
  - Its contract comment becomes: "`enumerate(i, …)` is called with each iterator in `variable_to_iter_map[i]` standing at the level just before its level for the `i`-th variable (at its pre-root for its first level). It descends each one level, scans, recurses on matches, then ascends back, so the caller's stack is unchanged on return. A position no iterator has a level for is skipped. After the last position every iterator stands at its last level, or at its pre-root when it has no level, so `emit_leaf` reads every chain."
  - Update `leaf_chain`'s expect message: "every iterator is at its last level, or at the pre-root of a trie with no level".
  - Update the module doc: participation is by level, and payload is bound in the leaf product.

- [ ] **Step 4: Run** `cargo test -p kermit-algos` and `CARGO_BUILD_JOBS=2 cargo test --workspace`. Expected: PASS.

- [ ] **Step 5: Commit**: `feat(algos): hash triejoin descends each atom at its levels only (#116 step 2)`.

---

## Task 11: The binary: `--ds-layout-levels`, the cells, preparation per policy

**Files:**
- Modify: `kermit/src/options.rs`, `kermit/src/execution.rs`, `kermit/src/main.rs`, `kermit/src/bench/run.rs`, `kermit/src/bench/ds.rs`, `kermit/src/bench/workload.rs`

- [ ] **Step 1: Write the failing unit tests.**

  In `options.rs`'s tests, beside the expansion round trips:

```rust
    #[test]
    fn levels_labels_round_trip_through_the_layout_names() {
        assert_eq!(levels_of::<kermit_ds::AllLevels>(), LevelsChoice::All);
        assert_eq!(levels_of::<kermit_ds::KeyLevels>(), LevelsChoice::Keys);
        for choice in LevelsChoice::value_variants() {
            let name = choice.to_possible_value().unwrap().get_name().to_string();
            assert_eq!(LevelsChoice::from_layout_name(&name), Some(*choice));
        }
        assert_eq!(LevelsChoice::All.trie_levels(), TrieLevels::All);
        assert_eq!(LevelsChoice::Keys.trie_levels(), TrieLevels::JoinKeys);
    }
```

  In `execution.rs`'s tests: `execution_axes_round_trip_through_for_pair` gains `levels` in every `HashHtj` literal. Add:

```rust
    #[test]
    fn a_cells_trie_levels_follow_its_structure_and_layout() {
        let hash = |levels| Execution::HashHtj {
            hasher: HasherChoice::Sip,
            pruning: PruningChoice::Off,
            expansion: ExpansionChoice::Eager,
            levels,
            config: HashTrieConfig::default(),
            build: HashTrieBuildMode::default(),
        };
        assert_eq!(hash(LevelsChoice::All).trie_levels(), TrieLevels::All);
        assert_eq!(hash(LevelsChoice::Keys).trie_levels(), TrieLevels::JoinKeys);
        let sorted = Execution::for_structure(IndexStructure::TreeTrie, DsChoices::default());
        assert_eq!(sorted.trie_levels(), TrieLevels::All);
    }
```

  `DsChoices` may have no `Default`. If so, build the sorted cell the way the other tests in that module do.

  In `workload.rs`'s tests, beside `prepare_prepares_each_query_in_definition_order`:

```rust
    #[test]
    fn prepare_prepares_each_query_once_per_policy() {
        // In `start`, Y is the join key and X and Z are payload, so under
        // `JoinKeys` both of its `edge` indexes have one level.
        let (root, def) = def_with_an_unreadable_relation(&[
            ("pair", "Q(X, Y) :- edge(X, Y)."),
            ("start", "Q(X) :- edge(X, Y), edge(Y, Z)."),
        ]);
        let workload = Workload::from_definition(&def, root.path(), None).unwrap();
        let prepared =
            workload.prepare(&free(), &[TrieLevels::All, TrieLevels::JoinKeys, TrieLevels::All]).unwrap();
        assert_eq!(prepared.prepared(TrieLevels::All).len(), prepared.workload.queries.len());
        assert!(prepared.prepared(TrieLevels::All).iter().all(|p| p.trie_levels() == TrieLevels::All));
        assert!(prepared
            .prepared(TrieLevels::JoinKeys)
            .iter()
            .all(|p| p.trie_levels() == TrieLevels::JoinKeys));
    }
```

  Update the neighbouring tests' `prepare(&free())` calls to `prepare(&free(), &[TrieLevels::All])`, and their `.prepared` reads to `.prepared(TrieLevels::All)`.

- [ ] **Step 2: Run** `CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit`. Expected: FAIL to compile.

- [ ] **Step 3: Implement `options.rs`.**
  - **`LevelsChoice`**, after `expansion_of`, following `ExpansionChoice`'s pattern exactly:
    ```rust
    /// `--ds-layout-levels`: whether a `HashTrie` index has a level per column
    /// (`all`, the default) or only up to its last join-key column (`keys`, the
    /// trie of Freitag et al., VLDB 2020 §3.1; #116). Selects the `L` of
    /// `HashTrie<H, P, E, L>`.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
    pub(crate) enum LevelsChoice {
        #[default]
        All,
        Keys,
    }

    impl LevelsChoice {
        /// The choice whose CLI value is `name`, a `LevelPolicy`'s
        /// `LayoutOption::NAME`.
        pub(crate) fn from_layout_name(name: &str) -> Option<Self> {
            match name {
                | "all" => Some(Self::All),
                | "keys" => Some(Self::Keys),
                | _ => None,
            }
        }

        /// The policy `kermit::db::prepare` plans this choice's indexes under.
        pub(crate) fn trie_levels(self) -> TrieLevels {
            match self {
                | Self::All => TrieLevels::All,
                | Self::Keys => TrieLevels::JoinKeys,
            }
        }
    }

    /// The `--ds-layout-levels` choice of the level policy `L`, the label a
    /// `HashHtj` derives from its type (as `expansion_of`).
    pub(crate) fn levels_of<L: LevelPolicy>() -> LevelsChoice {
        let name = <L as LayoutOption>::NAME;
        LevelsChoice::from_layout_name(name).unwrap_or_else(|| {
            panic!(
                "no --ds-layout-levels choice for level policy {name:?} ({})",
                std::any::type_name::<L>()
            )
        })
    }
    ```
  - **`LayoutChoices`** gains a field:
    ```rust
        /// Trie levels of `HashTrie<H, P, E, L>` (default: `all`). Only valid
        /// when `--indexstructure hash-trie` is selected, and not on `bench
        /// ds`, whose relation has no query and so no join key.
        #[arg(long = "ds-layout-levels", value_name = "LEVELS", value_enum)]
        hash_trie_levels: Option<LevelsChoice>,
    ```
    It also gains `hash_trie_levels_resolved` and `hash_trie_levels_explicit` (the same shape as expansion's), and `(DsFlag::LayoutLevels, self.hash_trie_levels_explicit())` in `given()`.
  - **`DsFlag`** gains `LayoutLevels`, which joins the `HashTrie` row of `structures` and displays as `"--ds-layout-levels"`.
  - **`DsChoices`** gains `pub levels: LevelsChoice` (doc: "The `--ds-layout-levels` choice."), which `resolve` fills from `layout.hash_trie_levels_resolved()`.
  - **The macro.** `with_hash_trie_layout!` gains the `@levels` rule and a four-dimension entry. Keep the three-dimension entry for `bench ds`, which builds `AllLevels` only:
    ```rust
        (@levels $choice:expr; $L:ident; $then:expr) => {
            match $choice {
                | $crate::options::LevelsChoice::All => {
                    type $L = ::kermit_ds::AllLevels;
                    $then
                },
                | $crate::options::LevelsChoice::Keys => {
                    type $L = ::kermit_ds::KeyLevels;
                    $then
                },
            }
        };
        (
            $hasher:expr, $pruning:expr, $expansion:expr, $levels:expr,
            | $H:ident, $P:ident, $E:ident, $L:ident | $body:expr
        ) => {
            $crate::options::with_hash_trie_layout!(@hasher $hasher; $H;
                $crate::options::with_hash_trie_layout!(@pruning $pruning; $P;
                    $crate::options::with_hash_trie_layout!(@expansion $expansion; $E;
                        $crate::options::with_hash_trie_layout!(@levels $levels; $L; $body))))
        };
    ```
    The doc names the four dimensions, 16 cells, and the three-dimension form `bench ds` uses.

- [ ] **Step 4: Implement `execution.rs`.**
  - `Execution::HashHtj` gains `levels: LevelsChoice`, documented after `expansion` ("The `--ds-layout-levels` choice, in the same two roles as `hasher` and `pruning`."). `for_pair` and `for_structure` destructure `levels` from `DsChoices` and pass it.
  - Add:
    ```rust
        /// The trie-level policy this cell's queries are prepared under
        /// (`kermit::db::prepare`): `All` for the sorted tries, the
        /// `--ds-layout-levels` choice for `HashTrie` (#116).
        pub fn trie_levels(self) -> TrieLevels {
            match self {
                | Self::TrieLftj(_) => TrieLevels::All,
                | Self::HashHtj { levels, .. } => levels.trie_levels(),
            }
        }
    ```
  - `HashTrieFamily<H, P, E, L = AllLevels>` and `HashHtj<H, P, E, L = AllLevels>`:
    - `_layout: PhantomData<(H, P, E, L)>`;
    - `L: LevelPolicy` on every impl;
    - `type Rel = HashTrie<H, P, E, L>`;
    - `execution()` adds `levels: levels_of::<L>()`.
  - Every `HashTrie<H, P, E>` in these impls becomes `HashTrie<H, P, E, L>`.

- [ ] **Step 5: Implement preparation per policy** (`workload.rs`).
  - `PreparedWorkload.prepared` becomes private: `prepared: BTreeMap<TrieLevels, Vec<Prepared>>` (doc: "One list per trie-level policy the cells need, each one entry per query of `workload.queries`, in order."). Add:
    ```rust
    impl PreparedWorkload {
        /// The queries prepared under `levels`, one per entry of
        /// `workload.queries`, in order.
        ///
        /// # Panics
        ///
        /// If no cell asked for `levels` when the workload was prepared.
        pub fn prepared(&self, levels: TrieLevels) -> &[Prepared] {
            self.prepared
                .get(&levels)
                .unwrap_or_else(|| panic!("the workload was not prepared for {levels:?} trie levels"))
        }
    }
    ```
  - `Workload::prepare(self, planner: &Planner, levels: &[TrieLevels])`:
    - validation and reading are unchanged and run once;
    - then, for each distinct policy (`levels.iter().copied().collect::<BTreeSet<_>>()`), it prepares every query with `db::prepare(&catalog, &q.query, planner, policy)`, under the same error mapping.

    Its doc gains: "once per distinct policy in `levels`, the policies of the cells that will run (`Execution::trie_levels`)".
  - Callers in `main.rs`:
    - `run_bench_run_command`, which also serves `bench probe`:
      ```rust
      let policies: Vec<TrieLevels> = cells.iter().map(|cell| cell.trie_levels()).collect();
      ```
      then `workload.prepare(&prepare_with, &policies)`.
    - `prepare_query(args, cell)` passes `&[cell.trie_levels()]`. Its two callers (`run_join`, and `bench join`'s) pass the cell they already computed, so move `prepare_query` after `query_cell` where needed.
  - `run_benchmark` (`bench/run.rs`): destructure `workload` and `catalog`, then:
    ```rust
    let prepared_queries = workload.prepared(<F::Rel as Relation>::TRIE_LEVELS);
    ```
    Import `kermit_ds::Relation` if needed.
  - `write_prepared_join` (`main.rs`): `let prepared = &workload.prepared(<F::Rel as Relation>::TRIE_LEVELS)[0];`.

- [ ] **Step 6: Implement the dispatch sites.**
  - The `HashHtj` arms of `dispatch_run_bench` (`bench/run.rs`) and `write_query_csv` (`main.rs`) destructure `levels` and call
    `with_hash_trie_layout!(hasher, pruning, expansion, levels, |H, P, E, L| … HashHtj::<H, P, E, L>::new(config, build) …)`.
  - `dispatch_ds_bench` (`bench/ds.rs`) keeps the three-dimension form, with `HashTrieFamily::<H, P, E>` building `AllLevels`. Its `HashHtj { .. }` destructure gains `levels: _`, or a `debug_assert_eq!(levels, LevelsChoice::All)`, since `bench ds` refuses the flag.
  - `bench ds`'s refusal, beside `--ds-layout-seek`'s (v6 `main.rs` near :805):
    ```rust
        // `bench ds` builds one relation from a whole file, with no query, so no
        // column is a join key: `keys` would label an `all` build.
        if layout.hash_trie_levels_explicit() {
            anyhow::bail!(
                "--ds-layout-levels has no effect on bench ds: it builds one relation from a whole \
                 file, with no query, so no column is a join key and every level is kept. Time a \
                 query with bench run or bench join."
            );
        }
    ```

- [ ] **Step 7: Run** the binary's tests, the CLI tests, and the binary rustdoc check:
  ```bash
  CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit && CARGO_BUILD_JOBS=2 cargo test -p kermit
  ```
  Expected: PASS.

- [ ] **Step 8: Smoke-test by hand:**
  ```bash
  cargo run -q -- bench run triangle -i hash-trie -a hash-triejoin --ds-layout-levels keys -m space --report-json /tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-flat-vec-in-out-18e0d193434047dd/33ecdfc4-9010-40b0-9ffa-df63472c4ddf/scratchpad/levels-smoke.json
  ```
  Expected: exit 0, and the report's axes include `"ds_layout_levels": "keys"`. Triangle's index lines carry no ` levels`, since every variable is a join key.

- [ ] **Step 9: Commit**: `feat(cli): --ds-layout-levels all|keys for HashTrie cells (#116 step 2)`.

---

## Task 12: The join suite, `all_orders` and the lifecycle under `keys`

**Files:**
- Modify: `kermit/tests/join_tests.rs`, `kermit/tests/all_orders.rs`, `kermit/src/db/lifecycle.rs` (tests), `kermit/src/db.rs` (tests)

- [ ] **Step 1: Add the aliases to `join_tests.rs`,** after the existing eight (import `EagerExpansion` and `KeyLevels` from `kermit_ds`):

```rust
// ── Layout aliases: the same eight under join-key levels (#116) ─────────
type HashTrieSipKeys = HashTrie<SipHashStrategy, NoPruning, EagerExpansion, KeyLevels>;
type HashTrieFxKeys = HashTrie<FxHashStrategy, NoPruning, EagerExpansion, KeyLevels>;
type HashTrieSipPrunedKeys = HashTrie<SipHashStrategy, SingletonPruning, EagerExpansion, KeyLevels>;
type HashTrieFxPrunedKeys = HashTrie<FxHashStrategy, SingletonPruning, EagerExpansion, KeyLevels>;
type HashTrieSipLazyKeys = HashTrie<SipHashStrategy, NoPruning, LazyExpansion, KeyLevels>;
type HashTrieFxLazyKeys = HashTrie<FxHashStrategy, NoPruning, LazyExpansion, KeyLevels>;
type HashTrieSipPrunedLazyKeys = HashTrie<SipHashStrategy, SingletonPruning, LazyExpansion, KeyLevels>;
type HashTrieFxPrunedLazyKeys = HashTrie<FxHashStrategy, SingletonPruning, LazyExpansion, KeyLevels>;
```

- [ ] **Step 2: Add the plain invocations:** each of the 8 aliases × `LexicographicOptimiser`, `CardinalityOptimiser` and `CostBasedOptimiser`, beside the existing plain block (24 invocations):

```rust
define_multiway_join_test_suite!(HashTrieSipKeys, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSipKeys, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieSipKeys, HashTriejoin, CostBasedOptimiser);
```

  …and the same three lines for each of the other seven aliases. Write all 24 lines out.

- [ ] **Step 3: Add the Config and BuildMode invocations,** one `Keys` alias per group × the 3 optimisers (27 invocations), each block placed after its group's existing invocations:
  - **`HashTrieSipKeys`** with `define_multiway_join_test_suite_with_config!(…, HashTriejoin, <Opt>, HalfFull)`, the same with `PresizedRoot`, and the same with `SizedChildren`.
  - **`HashTrieSipPrunedLazyKeys`** with `…_with_config!(…, PaperSizing)`, and with `define_multiway_join_test_suite_for_build_mode!(…, HashTriejoin, <Opt>, Radix2)`, `HashIncremental` and `HashParallel2`.
  - **`type HashTrieSipPrunedLazyKeysPresized = Configured<HashTrieSipPrunedLazyKeys, PresizedRoot>;`** with `…_for_build_mode!(…, HashPresized2)`.
  - **`type HashTrieSipPrunedLazyKeysPaper = Configured<HashTrieSipPrunedLazyKeys, PaperSizing>;`** in the stacked group, under the same build mode as `HashTrieSipPrunedLazyPaper`'s invocations (read them).

  Write every invocation out. Count them: 24 + 27 = 51 new.

- [ ] **Step 4: Add the `KeyLevels` cells to `all_orders.rs`:**

```rust
type HashTrieSipKeys = HashTrie<SipHashStrategy, NoPruning, EagerExpansion, KeyLevels>;

#[test]
fn keys_hash_trie_duplicated_constant_atom_under_every_order() {
    run_duplicated_constant_atom::<HashTrieSipKeys, HashTriejoin>(3);
}

#[test]
fn keys_hash_trie_shapes_under_every_order() { run_shapes::<HashTrieSipKeys, HashTriejoin>(); }

#[test]
fn keys_hash_trie_pinned_queries_under_every_order() {
    run_pinned::<HashTrieSipKeys, HashTriejoin>();
}
```

  These run every order, including the ones with payload before a join key, under rule (iii) and the skip. They amend #116's acceptance.

- [ ] **Step 5: Add the lifecycle tests.** They go in `kermit/src/db/lifecycle.rs`'s test module, using its `catalog`, `parse`, `lexicographic` and `join_with` helpers and Task 7's `payload_catalog` and `PAYLOAD_QUERY`. Imports to add: `kermit_ds::{EagerExpansion, KeyLevels, NoPruning}` and `kermit_iters::{SipHashStrategy, TrieLevels}`.

```rust
    type HashTrieSipKeys = HashTrie<SipHashStrategy, NoPruning, EagerExpansion, KeyLevels>;

    #[test]
    #[should_panic(expected = "trie levels")]
    fn a_keys_hash_trie_refuses_a_query_prepared_for_all_levels() {
        let c = payload_catalog();
        let all = prepare(&c, &parse(PAYLOAD_QUERY), &lexicographic(), TrieLevels::All).unwrap();
        let _ = build::<HashTrieSipKeys>(&c, &all);
    }

    /// SNAP's `path3` (#116 spec, Decision 5): exact sharing builds three
    /// `edge` tries under `keys` and two under `all`, and every structure
    /// answers alike.
    #[test]
    fn path3_builds_three_edge_tries_under_keys_and_two_under_all() {
        let c = catalog(vec![("edge", 2, vec![vec![1, 2], vec![2, 3], vec![3, 4], vec![4, 5]])]);
        let query = parse("P(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D).");
        let keys = prepare(&c, &query, &lexicographic(), TrieLevels::JoinKeys).unwrap();
        let all = prepare(&c, &query, &lexicographic(), TrieLevels::All).unwrap();
        assert_eq!(keys.indexes().len(), 3);
        assert_eq!(all.indexes().len(), 2);
        let want = vec![vec![1, 2, 3, 4], vec![2, 3, 4, 5]];
        assert_eq!(join_with::<HashTrieSipKeys, HashTriejoin>(&c, &query, &lexicographic()), want);
        assert_eq!(join_with::<HashTrie, HashTriejoin>(&c, &query, &lexicographic()), want);
        assert_eq!(join_with::<TreeTrie, LeapfrogTriejoin>(&c, &query, &lexicographic()), want);
    }

    /// An all-constant atom multiplies the answers of a query whose index
    /// has no level: `r` carries no join key, and `s` holds `1` twice.
    #[test]
    fn a_count_multiplies_a_zero_level_index() {
        let c = catalog(vec![("r", 1, vec![vec![1], vec![2]]), ("s", 1, vec![vec![1], vec![1]])]);
        let query = parse("Q(X) :- r(X), s(c1).");
        let keys = prepare(&c, &query, &lexicographic(), TrieLevels::JoinKeys).unwrap();
        assert_eq!(keys.indexes()[0].levels, 0);
        assert_eq!(
            join_with::<HashTrieSipKeys, HashTriejoin>(&c, &query, &lexicographic()),
            vec![vec![1], vec![1], vec![2], vec![2]]
        );
    }
```

- [ ] **Step 6: Run** `CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests --test all_orders && CARGO_BUILD_JOBS=2 cargo test -p kermit --lib db`. Expected: PASS. If an invocation fails, debug the HashTrie or join code with superpowers:systematic-debugging; never weaken the suite.

- [ ] **Step 7: Commit**: `test: the join suite, all orders and the lifecycle under join-key levels (#116)`.

---

## Task 13: Real-data oracles gain HashTrie cells

**Files:**
- Modify: `kermit/tests/watdiv_correctness.rs`, `kermit/tests/lubm_mini_oracle.rs`, `kermit/tests/lubm_cardinalities.rs`

- [ ] **Step 1: Make each file's checker generic over the join entry.** For example, in `watdiv_correctness.rs`:

```rust
fn check_cardinalities<R: Relation + Cardinality, JA: JoinEntry<R>>(optimiser: Optimiser) {
    // … unchanged, except:
    let (rows, _) = join_under_planner::<R, JA>(&catalog, parsed, &planner)
        .unwrap_or_else(|e| panic!("query {}: {e}", q.name));
}
```

  Its existing calls become `check_cardinalities::<TreeTrie<LinearSeek>, LeapfrogTriejoin>(optimiser)` and so on. Add:

```rust
        check_cardinalities::<HashTrie<SipHashStrategy>, HashTriejoin>(optimiser);
        check_cardinalities::<HashTrieSipKeys, HashTriejoin>(optimiser);
        check_cardinalities::<HashTrieSipPrunedLazyKeys, HashTriejoin>(optimiser);
```

  `HashTrieSipKeys` and `HashTrieSipPrunedLazyKeys` are defined at the top of each file, as in Task 12. Do the same in `lubm_mini_oracle.rs`'s and `lubm_cardinalities.rs`'s `cardinality_mismatches`, and keep the type name in each mismatch's label.

- [ ] **Step 2: Run** `CARGO_BUILD_JOBS=2 nix develop --command cargo test -p kermit --test watdiv_correctness --test lubm_mini_oracle --test lubm_cardinalities`. Expected: PASS. `lubm_cardinalities` needs `java`, so run it inside `nix develop`. Check its output names the HashTrie cells; it skips silently without `java`.

- [ ] **Step 3: Commit**: `test: HashTrie cells, all and keys, in the WatDiv and LUBM oracles (#116)`.

---

## Task 14: The allocation tests under `keys`

**Files:**
- Modify: `kermit/tests/result_allocation.rs`, `kermit/tests/tuple_allocation.rs`

- [ ] **Step 1: `result_allocation.rs`.** Each block that lists the 8 HashTrie aliases (at 2607a04: :329–:395, :650–:716, and the one near :829) lists the 8 `Keys` aliases after them, with the same test bodies and their names suffixed `_keys`. Define the aliases as in Task 12. A lazy `Keys` cell is measured warm, as its lazy twin is.

- [ ] **Step 2: `tuple_allocation.rs`.** Read how a HashTrie build cell is measured (`measure_hash_build::<P, E>` near :504 at 2607a04). Then:
  - extend it, or add a sibling, to take `L: LevelPolicy` and a header;
  - for each HashTrie build-mode cell (:525–:620, :961, :1084, presized :654–:674), add a `KeyLevels` twin built from a header with fewer levels than columns (`with_levels(1)` on the arity-2 or arity-3 inputs those cells use), and one at zero levels (`with_levels(0)`);
  - each twin asserts what its `AllLevels` cell asserts: no allocation per input row.

- [ ] **Step 3: Run** `CARGO_BUILD_JOBS=2 cargo test -p kermit --test result_allocation --test tuple_allocation`. Expected: PASS.

- [ ] **Step 4: Commit**: `test: allocation pins for join-key-level builds and joins (#116)`.

---

## Task 15: The CLI test and kermit-lab

**Files:**
- Create: `kermit/tests/cli_hash_trie_layout_levels.rs`
- Modify: `python/kermit-lab/kermit_lab/defaults.py`, plus the kermit-lab test that pins `ds_layout_expansion`'s back-fill (grep `ds_layout_expansion` under `python/kermit-lab/tests/`)

- [ ] **Step 1: Write the CLI test,** modelled on `cli_hash_trie_layout_expansion.rs`:

```rust
//! CLI smoke test: `--ds-layout-levels keys` records `ds_layout_levels:
//! "keys"`, the absent flag records `"all"`, a payload query's `index` lines
//! name their levels only under `keys`, and the flag is refused by `bench ds`
//! and on a structure without the dimension (#116).

mod common;

use common::cli::{axes_of, bench_ds, bench_run, reports_of};

fn assert_success(output: &std::process::Output) {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn cli_bench_run_with_keys_records_the_axis() {
    let (output, report) = bench_run("triangle", &[
        "-i", "hash-trie", "-a", "hash-triejoin", "-m", "space", "--ds-layout-levels", "keys",
    ]);
    assert_success(&output);
    assert_eq!(axes_of(&report)["ds_layout_levels"], "keys");
}

#[test]
fn cli_bench_run_default_levels_are_all() {
    let (output, report) = bench_run("triangle", &["-i", "hash-trie", "-a", "hash-triejoin", "-m", "space"]);
    assert_success(&output);
    assert_eq!(axes_of(&report)["ds_layout_levels"], "all");
}

#[test]
fn cli_bench_run_sweep_carries_levels_only_to_hash_trie_cells() {
    let (output, report) =
        bench_run("triangle", &["-i", "all", "-a", "all", "-m", "space", "--ds-layout-levels", "keys"]);
    assert_success(&output);
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        if axes["data_structure"] == "HashTrie" {
            assert_eq!(axes["ds_layout_levels"], "keys", "{axes}");
        } else {
            assert!(axes.get("ds_layout_levels").is_none(), "{axes}");
        }
    }
}

#[test]
fn cli_bench_ds_refuses_the_levels_flag() {
    let (output, _) = bench_ds("hash-trie", &["--ds-layout-levels", "keys"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-layout-levels has no effect on bench ds"), "{stderr}");
}

#[test]
fn cli_bench_run_refuses_the_levels_flag_on_tree_trie() {
    let (output, _) =
        bench_run("triangle", &["-i", "tree-trie", "-a", "leapfrog-triejoin", "--ds-layout-levels", "keys"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--ds-layout-levels"));
}
```

  Add one more test. It runs `bench join` (or `kermit join`) on a payload query over temp files, using `common::cli`'s helpers for those commands (read `common/cli.rs`; add a small helper there if `bench join` has none), with a relation `r` of `(X, P)` rows and `s` of `X` rows, and the query `Q(X, P) :- r(X, P), s(X).`. It checks that:
  - under `--ds-layout-levels keys`, the report's `index` metadata line for `r` contains ` levels 1`;
  - under the default, no `index` line contains ` levels `;
  - the CSV rows `kermit join` writes under `keys` and under `all` are equal, once sorted.

  If triangle has no committed data the CLI tests can run offline, use the benchmark the expansion test uses.

- [ ] **Step 2: Add the kermit-lab default.** In `defaults.py`'s `SCOPED_AXIS_DEFAULTS`, beside `("ds_layout_expansion", "HashTrie"): "eager",`:
  ```python
  # #116: a HashTrie index had a level per column before `--ds-layout-levels`.
  ("ds_layout_levels", "HashTrie"): "all",
  ```
  Add a test case mirroring `ds_layout_expansion`'s back-fill test: a HashTrie row without the axis loads with `all`, and a sorted-trie row gets no `ds_layout_levels`.

- [ ] **Step 3: Run** `CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_hash_trie_layout_levels`, then `cd python/kermit-lab && uv run --group test pytest`. Expected: PASS.

- [ ] **Step 4: Commit**: `test,lab: --ds-layout-levels end to end; kermit-lab back-fills all (#116)`.

---

## Task 16: Documentation

**Files:**
- Modify: `docs/data-structures/hash-trie.md`, `docs/algorithms/hash-triejoin.md`, `docs/specs/bench-report-schema.md`, `docs/specs/optimization-standard.md`, `CLAUDE.md`, `ARCHITECTURE.md`

- [ ] **Step 1: `hash-trie.md`.**
  - **Representation:** the level count, and the zero-level root (`Root::Rows`, one chain in source order).
  - **Invariants:** the depth is the level count; leaf chains hold whole rows.
  - **Complexity:** `open` is bounded by the level count; at zero levels there is no table.
  - **A "Join-key levels (`--ds-layout-levels`)" subsection** under Layout options, in the shape of the pruning and expansion subsections:
    - what `keys` builds;
    - rule (iii), with the spec's P, X, Y example;
    - zero levels, and every mode building the same copy there;
    - exact sharing (`path3`);
    - the axis `ds_layout_levels`;
    - the 8 `Keys` aliases;
    - `bench ds` refusing the flag;
    - its parity with the paper.
  - **Parity:** the spec's parity table, its rows added where this doc tabulates parity, or as the subsection's own table. Also the new closest-to-paper configuration, where line ~625 names it at 2607a04.

- [ ] **Step 2: `hash-triejoin.md`.**
  - **Pseudocode:** participation by level, and the skip.
  - **Invariants:** each atom is opened exactly `levels()` times.
  - **The leaf product** binding the payload (Algorithm 3 lines 16–19, §3.2.4).
  - **A worked example:** the spec's R/S/T example under X, P, Y.

- [ ] **Step 3: `bench-report-schema.md`.**
  - **An axis row:**
    ```
    | `ds_layout_levels` | `ds`, `join`, `run`, `probe` (HashTrie) | string | `"all"` (default) or `"keys"`: `--ds-layout-levels`, whether a HashTrie index has a level per column or only up to its last join key (#116). kermit-lab back-fills `"all"` on HashTrie rows without it. |
    ```
  - **The index line's documented format** (near :74) gains ` levels {n}`, present only when the trie has fewer levels than columns.
  - **A no-bump row:**
    ```
    | 6 (no bump) | 2026-11-02 | Added the `ds_layout_levels` axis (`--ds-layout-levels all\|keys`, #116) and the `index` line's ` levels n` suffix, present only under `keys`. Additive: an `all` report is unchanged, so `schema_version` stays 6. |
    ```

- [ ] **Step 4: `optimization-standard.md`:** HashTrie's fourth Layout and its 16 aliases, wherever the standard lists HashTrie's Layouts.

- [ ] **Step 5: `CLAUDE.md`.**
  - **Priorities item 1:** hasher × pruning × expansion × levels, with the 16 aliases (the eight, and each again with a `Keys` suffix).
  - **Build Commands:**
    ```
    cargo run -- bench run triangle -i hash-trie -a hash-triejoin --ds-layout-levels keys  # Join-key levels Layout axis (#116)
    ```
  - **Key Trait Hierarchy:**
    - a `LevelPolicy` bullet: `AllLevels` (default) or `KeyLevels`, the axis `ds_layout_levels`, and the zero-level root;
    - in the `Relation` bullet, `TRIE_LEVELS` beside `KEEPS_DUPLICATES`;
    - in the `HashTrieIterable` bullet, `levels()`, and `leaf_tuples` at the pre-root of a trie with no level.
  - **Testing Patterns:** the `Keys` aliases, and `all_orders`' keys cells.
  - **The singleton-pruning gotcha:** the macro is nested, with 16 cells, plus the three-dimension form for `bench ds`.
  - **The index-planning gotcha:** `IndexSpec::levels`, `TrieLevels`, exact sharing, and `path3`.
  - **The load/prepare/build gotcha:** `bench run` prepares each query once per policy its cells need.

- [ ] **Step 6: `ARCHITECTURE.md`:** the HashTrie and Hash Triejoin sections, with level counts and participation by level.

- [ ] **Step 7: Run** `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`, and check every intra-doc link in the touched docs resolves.

- [ ] **Step 8: Commit**: `docs: join-key trie levels (#116)`.

---

## Task 17: The full gate and a whole-change review

- [ ] **Step 1: Refresh the toolchain pin, then run the full gate.**
  - Run `nix flake update rust-overlay`. If it changes `flake.lock`, commit that alone: `chore(nix): bump rust-overlay`.
  - Run the full gate (Conventions). Expected: PASS on every step. Fix any failure in a commit of its own.
- [ ] **Step 2: Check the spec's acceptance criteria one by one** (`docs/specs/2026-11-02-join-key-levels-design.md` § Acceptance), and note the evidence for each: the test name or command.
- [ ] **Step 3: Review the whole change.** Dispatch one `code-quality-reviewer` with the spec path and `git diff <Task 0's merge commit>..HEAD`. Read its findings, verify each against the code (superpowers:receiving-code-review), and fix the real ones in their own commits.
- [ ] **Step 4: Report to the owner.** Cover what landed (commits), the gate output, the acceptance evidence, and what is the owner's:
  - pushing to origin master, after a fetch and merge of origin/master (memory: landing loom work);
  - #116's comment and closure;
  - #99's sweep.
