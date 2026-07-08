# Query Optimiser Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a first-class query-optimiser component (optimizer/executor split): a `QueryOptimiser` trait producing a `QueryPlan` (the LFTJ global attribute order), a `Cardinality` statistics seam, a `LexicographicOptimiser` default preserving current behaviour bit-for-bit, and a `CardinalityOptimiser` (smallest-relation-first), surfaced as `--optimiser` on the CLI and an `optimiser` bench-report axis.

**Architecture:** A new `kermit-algos/src/optimiser/` module hosts the trait, plan, shared query analysis (hoisted from the two duplicated `build_variable_index` functions), and a Kahn's-topological-sort helper with a pluggable ranking function (valid orders by construction). `JoinAlgo::join_iter` gains the plan as an explicit first parameter. `DatabaseEngine` holds a `Box<dyn QueryOptimiser>`; `hash_join` takes `&dyn QueryOptimiser`. Statistics flow as plain data (`CatalogStats`) gathered from a new `Cardinality` trait in `kermit-ds`.

**Tech Stack:** Rust nightly workspace, clap 4 (`ValueEnum`), `paste` in test macros, Criterion bench reporting (`BenchReport.axes`).

**Spec:** `docs/specs/2026-07-08-query-optimiser-design.md` (approved).

**Verification commands** (used throughout; run from the workspace root):

```bash
cargo test --package kermit-ds          # DS-only tasks
cargo test --package kermit-algos       # optimiser/algo tasks
cargo test --verbose                    # whole workspace
cargo clippy --all-targets --verbose    # CI parity needs RUSTFLAGS=-Dwarnings
nix develop --command cargo fmt --all   # NEVER plain `cargo fmt` (stable rustfmt rewrites ~30 files)
```

---

## File Structure

**Created:**

| File | Responsibility |
|---|---|
| `kermit-ds/src/cardinality.rs` | `Cardinality` trait (stored-tuple count), mirrors `heap_size.rs` |
| `kermit-algos/src/optimiser/mod.rs` | `QueryOptimiser` trait, `CatalogStats`/`RelationStats`, re-exports |
| `kermit-algos/src/optimiser/analysis.rs` | `QueryAnalysis` + `analyse()` — canonical variable indexing (hoisted) |
| `kermit-algos/src/optimiser/ordering.rs` | `topological_order()` — Kahn's with pluggable rank (hoisted from `global_attribute_order`) |
| `kermit-algos/src/optimiser/plan.rs` | `QueryPlan` + `validate()` + `PlanError` |
| `kermit-algos/src/optimiser/lexicographic.rs` | `LexicographicOptimiser` (default) |
| `kermit-algos/src/optimiser/cardinality.rs` | `CardinalityOptimiser` (smallest-relation-first) |
| `kermit/tests/cli_optimiser_choice.rs` | CLI smoke test: `--optimiser` → report axis |
| `docs/optimisers/TEMPLATE.md`, `lexicographic.md`, `cardinality.md` | Per-component docs |

**Modified (main):** `kermit-ds/src/lib.rs`, all three `kermit-ds/src/ds/*/implementation.rs`, `kermit-algos/src/lib.rs`, `join_algo.rs`, `leapfrog_triejoin.rs`, `hash_triejoin.rs`, `kermit/src/lib.rs`, `kermit/src/db.rs`, `kermit/src/main.rs`, `kermit/tests/common/{utils,macros}.rs`, `kermit/tests/join_tests.rs`, `kermit/tests/subject_position_constant.rs`, `CLAUDE.md`, `ARCHITECTURE.md`, `docs/specs/bench-report-schema.md`, `docs/algorithms/{leapfrog-triejoin,hash-triejoin}.md`.

**Compile-safety ordering:** Tasks 1–7 are purely additive (workspace stays green after each). Task 8 is the atomic breaking flip (`JoinAlgo` signature) and must update every call site in one commit. Tasks 9–12 build on the flipped API.

---

### Task 1: `Cardinality` trait + `TreeTrie` impl

**Files:**
- Create: `kermit-ds/src/cardinality.rs`
- Modify: `kermit-ds/src/lib.rs:19-28`
- Modify: `kermit-ds/src/ds/tree_trie/implementation.rs` (struct ~98–102, `insert_into_children` ~11–27, `Relation` impl ~108–187)

- [ ] **Step 1: Write the failing test**

Append to `kermit-ds/src/ds/tree_trie/implementation.rs` (alongside the existing `mod heap_size_tests`):

```rust
#[cfg(test)]
mod cardinality_tests {
    use {super::*, crate::cardinality::Cardinality, kermit_iters::TrieIterable};

    #[test]
    fn empty_relation_has_zero_tuples() {
        let trie = TreeTrie::new(2.into());
        assert_eq!(trie.tuple_count(), 0);
    }

    #[test]
    fn tuple_count_matches_iteration_count() {
        let trie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        assert_eq!(trie.tuple_count(), 3);
        assert_eq!(trie.trie_iter().into_iter().count(), 3);
    }

    #[test]
    fn duplicate_insert_does_not_inflate_count() {
        let mut trie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        trie.insert(vec![1, 2]); // exact duplicate — absorbed
        assert_eq!(trie.tuple_count(), 1);
        trie.insert(vec![1, 3]); // shares prefix, genuinely new
        assert_eq!(trie.tuple_count(), 2);
    }
}
```

- [ ] **Step 2: Create the trait so the test can name it, then verify the test fails to compile on the missing impl**

Create `kermit-ds/src/cardinality.rs`:

```rust
/// Trait for reporting the number of stored tuples.
///
/// The count matches what a full iteration of the relation yields.
/// Set-semantics structures (`TreeTrie`, `ColumnTrie`) absorb duplicate
/// inserts, so their count is the number of *distinct* tuples. `HashTrie`
/// is deliberately a multiset (duplicates append to leaf chains; key
/// equality is deferred to the join algorithm), so its count includes
/// duplicates.
///
/// Query optimisers consume this via `kermit-algos`' `CatalogStats` as the
/// size signal for cardinality-aware variable ordering. Implementations
/// must be O(1) — the count is read during planning on every join.
pub trait Cardinality {
    /// Returns the number of stored tuples.
    fn tuple_count(&self) -> usize;
}
```

In `kermit-ds/src/lib.rs`, register and re-export (the file currently has `mod ds; mod heap_size; mod relation;` at lines 19–21 and a `pub use` block at 24–28):

```rust
mod cardinality;
mod ds;
mod heap_size;
mod relation;

// Re-export IndexStructure for external crates (CLI) to reference directly
pub use {
    cardinality::Cardinality,
    ds::{ColumnTrie, HashTrie, IndexStructure, TreeTrie},
    heap_size::HeapSize,
    relation::{ModelType, Projectable, Relation, RelationError, RelationFileExt, RelationHeader},
};
```

Run: `cargo test --package kermit-ds cardinality_tests`
Expected: compile FAIL — `TreeTrie` has no method `tuple_count` (trait exists but no impl).

- [ ] **Step 3: Implement — counter field + new-tuple detection**

In `kermit-ds/src/ds/tree_trie/implementation.rs`:

3a. Change `insert_into_children` (lines ~11–27) to report whether the tuple was new. A duplicate takes the `Ok` branch at every level; the first `Err` branch means everything below is fresh:

```rust
/// Inserts `tuple` into the sorted children, level by level. Returns
/// `true` iff the tuple was not already present (some level created a
/// new node).
fn insert_into_children(children: &mut Vec<TrieNode>, tuple: Vec<usize>) -> bool {
    let mut key_iter = tuple.into_iter();
    let Some(key) = key_iter.next() else {
        // Exhausted every key along an already-existing path — duplicate.
        return false;
    };

    match children.binary_search_by(|node| node.key().cmp(&key)) {
        | Ok(pos) => insert_into_children(children[pos].children_mut(), key_iter.collect()),
        | Err(pos) => {
            let mut new_node = TrieNode::new(key);
            insert_into_children(new_node.children_mut(), key_iter.collect());
            children.insert(pos, new_node);
            true
        },
    }
}
```

3b. Add the field to the struct (lines ~98–102):

```rust
#[derive(Clone, Debug)]
pub struct TreeTrie {
    header: RelationHeader,
    children: Vec<TrieNode>,
    /// Number of distinct tuples stored; maintained by `insert`.
    tuple_count: usize,
}
```

3c. In `Relation::new` add `tuple_count: 0,` to the struct literal. In `Relation::insert` (line ~167) replace the call `insert_into_children(&mut self.children, tuple);` with:

```rust
        if insert_into_children(&mut self.children, tuple) {
            self.tuple_count += 1;
        }
```

(`from_tuples` and `insert_all` route through `insert`, so they count automatically.)

3d. Add the impl next to the `HeapSize` impl (~line 197):

```rust
impl crate::cardinality::Cardinality for TreeTrie {
    fn tuple_count(&self) -> usize { self.tuple_count }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --package kermit-ds`
Expected: PASS (including all pre-existing TreeTrie tests — set semantics unchanged).

- [ ] **Step 5: Commit**

```bash
git add kermit-ds/src/cardinality.rs kermit-ds/src/lib.rs kermit-ds/src/ds/tree_trie/implementation.rs
git commit -m "feat(kermit-ds): Cardinality trait with O(1) TreeTrie tuple counter"
```

---

### Task 2: `ColumnTrie` `Cardinality` impl

**Files:**
- Modify: `kermit-ds/src/ds/column_trie/implementation.rs` (struct ~110–119, `internal_insert` ~141–153, `step_layer` ~166–212, `Relation` impl ~266–330)

- [ ] **Step 1: Write the failing test**

Append to `kermit-ds/src/ds/column_trie/implementation.rs`:

```rust
#[cfg(test)]
mod cardinality_tests {
    use {super::*, crate::cardinality::Cardinality, kermit_iters::TrieIterable};

    #[test]
    fn empty_relation_has_zero_tuples() {
        let trie = ColumnTrie::new(2.into());
        assert_eq!(trie.tuple_count(), 0);
    }

    #[test]
    fn tuple_count_matches_iteration_count() {
        let trie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        assert_eq!(trie.tuple_count(), 3);
        assert_eq!(trie.trie_iter().into_iter().count(), 3);
    }

    #[test]
    fn duplicate_insert_does_not_inflate_count() {
        let mut trie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        trie.insert(vec![1, 2]); // exact duplicate — absorbed
        assert_eq!(trie.tuple_count(), 1);
        trie.insert(vec![1, 3]); // shared prefix, new tuple
        assert_eq!(trie.tuple_count(), 2);
        trie.insert(vec![0, 9]); // insert-before-existing path
        assert_eq!(trie.tuple_count(), 3);
    }
}
```

Run: `cargo test --package kermit-ds column_trie`
Expected: compile FAIL — no `tuple_count` on `ColumnTrie`.

- [ ] **Step 2: Implement — thread a `matched_existing` signal through `step_layer`**

A tuple is a duplicate iff **every** layer hit the existing-key equality branch. Any insert/append/empty-push branch means the tuple is new.

2a. Add the field (struct at ~110–119): after `layers: Vec<ColumnTrieLayer>,` add:

```rust
    /// Number of distinct tuples stored; maintained by `insert`.
    tuple_count: usize,
```

and add `tuple_count: 0,` to the struct literal in `Relation::new` (~269–279).

2b. Change `step_layer` to return `(LayerStep, bool)` where the bool is `matched_existing` — `true` only in the equality branch. Exact edits within the existing body (~166–212):

- Signature: `) -> (LayerStep, bool) {`
- Empty-layer fast path (`data.is_empty()`): `return (LayerStep::Recurse { next_interval_index: 0 }, false);`
- Equality branch (`data[i] == k`): `return (LayerStep::Recurse { next_interval_index: i }, true);`
- Insert-before branch: `return (LayerStep::Stop, false);` and `return (LayerStep::Recurse { next_interval_index: i }, false);`
- Append tail: `return (LayerStep::Stop, false);` and final expression `(LayerStep::Recurse { next_interval_index: insert_pos }, false)`

Update the `step_layer` doc comment to mention the second tuple element ("`true` iff the key was already present in the parent group").

2c. Change `internal_insert` (~141–153) to return `bool` (tuple was new):

```rust
    fn internal_insert(&mut self, tuple: &[usize]) -> bool {
        let arity = self.header().arity();
        let mut interval_index = 0;
        let mut all_matched = true;
        for (layer_i, &k) in tuple.iter().enumerate() {
            let is_last_layer = layer_i == arity - 1;
            let (step, matched_existing) =
                self.step_layer(layer_i, k, interval_index, is_last_layer);
            all_matched &= matched_existing;
            match step {
                | LayerStep::Stop => return !all_matched,
                | LayerStep::Recurse {
                    next_interval_index,
                } => interval_index = next_interval_index,
            }
        }
        !all_matched
    }
```

Extend its doc comment: returns `true` iff the tuple was not already present (a duplicate matches an existing key at every layer).

2d. In `Relation::insert` (~316–323) replace `self.internal_insert(&tuple);` with:

```rust
        if self.internal_insert(&tuple) {
            self.tuple_count += 1;
        }
```

2e. Add the impl next to the `HeapSize` impl (~line 332):

```rust
impl crate::cardinality::Cardinality for ColumnTrie {
    fn tuple_count(&self) -> usize { self.tuple_count }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --package kermit-ds`
Expected: PASS — including the pre-existing `duplicate_insert_is_noop` pinning test.

- [ ] **Step 4: Commit**

```bash
git add kermit-ds/src/ds/column_trie/implementation.rs
git commit -m "feat(kermit-ds): ColumnTrie Cardinality impl via matched-existing insert signal"
```

---

### Task 3: `HashTrie` `Cardinality` impl (multiset count)

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (struct ~42–46, `Relation` impl ~119–164)

- [ ] **Step 1: Write the failing test**

Append to `kermit-ds/src/ds/hash_trie/implementation.rs`:

```rust
#[cfg(test)]
mod cardinality_tests {
    use {super::*, crate::cardinality::Cardinality};

    #[test]
    fn empty_relation_has_zero_tuples() {
        let trie: HashTrie = HashTrie::new(2.into());
        assert_eq!(trie.tuple_count(), 0);
    }

    #[test]
    fn tuple_count_matches_collect_tuples_len() {
        let trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        assert_eq!(trie.tuple_count(), 3);
        assert_eq!(trie.collect_tuples().len(), 3);
    }

    #[test]
    fn duplicate_insert_counts_multiset_semantics() {
        // HashTrie is deliberately a multiset: duplicates append to leaf
        // chains (equality is deferred to the join algorithm), so the
        // count includes them — matching what iteration yields.
        let mut trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        trie.insert(vec![1, 2]);
        assert_eq!(trie.tuple_count(), 2);
        assert_eq!(trie.collect_tuples().len(), 2);
    }
}
```

Run: `cargo test --package kermit-ds hash_trie`
Expected: compile FAIL — no `tuple_count` on `HashTrie`.

- [ ] **Step 2: Implement**

2a. Add the field (struct ~42–46):

```rust
pub struct HashTrie<H: HashStrategy = SipHashStrategy> {
    header: RelationHeader,
    root: HashTrieNode,
    /// Number of stored tuples (multiset: duplicates count); maintained
    /// by `insert` and `from_tuples`.
    tuple_count: usize,
    _hasher: PhantomData<H>,
}
```

2b. In `Relation::new` (~122–129) add `tuple_count: 0,` to the struct literal.

2c. In `Relation::from_tuples` (~131–145), inside the `for tuple in tuples` loop after the `Self::insert_at(...)` call, add `trie.tuple_count += 1;`.

2d. In `Relation::insert` (~147–157), after the `Self::insert_at(...)` call, add `self.tuple_count += 1;`. (`insert_all` loops over `insert`.)

2e. Add the impl next to the `HeapSize` impl (~line 195):

```rust
impl<H: HashStrategy> crate::cardinality::Cardinality for HashTrie<H> {
    fn tuple_count(&self) -> usize { self.tuple_count }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --package kermit-ds`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/implementation.rs
git commit -m "feat(kermit-ds): HashTrie Cardinality impl (multiset stored-tuple count)"
```

---

### Task 4: Optimiser module skeleton — shared query analysis

**Files:**
- Create: `kermit-algos/src/optimiser/mod.rs`, `kermit-algos/src/optimiser/analysis.rs`
- Modify: `kermit-algos/src/lib.rs` (module list ~9–17, `pub use` block ~20–30)

This hoists passes 1–3 of the duplicated `build_variable_index` (in `leapfrog_triejoin.rs:325-376` and `hash_triejoin.rs:27-68`) into one shared, deterministic function. The two algorithms keep their old copies until Task 8 (the flip) so the workspace stays green.

- [ ] **Step 1: Write the failing test**

Create `kermit-algos/src/optimiser/analysis.rs` with tests first (module body follows in Step 3; put the test mod at the bottom of the file now):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_head_first_indexing() {
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        let analysis = analyse(&query);
        assert_eq!(analysis.num_vars, 3);
        assert_eq!(
            analysis.predicate_variables,
            vec![vec![0, 1], vec![1, 2], vec![0, 2]]
        );
    }

    #[test]
    fn body_only_variables_index_after_head() {
        // Y never appears in the head — it gets the next index after X.
        let query: JoinQuery = "Q(X) :- R(X, Y), S(Y).".parse().unwrap();
        let analysis = analyse(&query);
        assert_eq!(analysis.num_vars, 2);
        assert_eq!(analysis.predicate_variables, vec![vec![0, 1], vec![1]]);
    }

    #[test]
    fn placeholders_and_atoms_are_skipped() {
        let query: JoinQuery = "Q(X) :- R(X, _), S(X, c5).".parse().unwrap();
        let analysis = analyse(&query);
        assert_eq!(analysis.num_vars, 1);
        assert_eq!(analysis.predicate_variables, vec![vec![0], vec![0]]);
    }
}
```

- [ ] **Step 2: Wire the module and verify the test fails**

Create `kermit-algos/src/optimiser/mod.rs`:

```rust
//! Query optimisers: planners that choose the global attribute order.
//!
//! Kermit separates planning from execution. A [`QueryOptimiser`] (Task 6)
//! consumes a parsed, const-rewritten [`JoinQuery`](kermit_parser::JoinQuery)
//! plus per-relation statistics and produces a `QueryPlan`; join algorithms
//! execute the plan. This module hosts the shared query analysis both sides
//! rely on for a consistent canonical variable numbering.

mod analysis;

pub use analysis::{analyse, QueryAnalysis};
```

In `kermit-algos/src/lib.rs`, add `mod optimiser;` to the module list (alphabetical: after `leapfrog_triejoin`) and add to the `pub use` block:

```rust
    optimiser::{analyse, QueryAnalysis},
```

Run: `cargo test --package kermit-algos analysis`
Expected: compile FAIL — `analyse`/`QueryAnalysis` not defined.

- [ ] **Step 3: Implement `analyse` (moved logic, not new logic)**

Fill in `kermit-algos/src/optimiser/analysis.rs` above the test mod. This is passes 1–3 of `leapfrog_triejoin.rs::build_variable_index` verbatim, returning a struct instead of a tuple:

```rust
//! Query analysis shared by planners and executors.

use {
    kermit_parser::{JoinQuery, Term},
    std::collections::HashMap,
};

/// Structural facts about a [`JoinQuery`]: the canonical variable
/// numbering and which variables each body predicate carries.
///
/// Planners and executors both call [`analyse`] on the same query and
/// therefore agree on the numbering — the `QueryPlan` carries canonical
/// indices, never names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryAnalysis {
    /// Number of distinct variables in the query.
    pub num_vars: usize,
    /// For each body predicate (in body order), the canonical indices of
    /// the variables it carries, in the predicate's term (physical column)
    /// order.
    pub predicate_variables: Vec<Vec<usize>>,
}

/// Indexes the variables in a [`JoinQuery`].
///
/// Two passes assign canonical indices: head variables first (so output
/// tuple order matches the head declaration), then body-only variables in
/// first-appearance order. A third pass collects per-predicate variable
/// index lists. Placeholders (`_`) and atoms are skipped in all passes —
/// they occupy trie levels but don't bind a join variable.
///
/// Deterministic: same query, same numbering, always.
pub fn analyse(query: &JoinQuery) -> QueryAnalysis {
    let mut var_to_index: HashMap<String, usize> = HashMap::new();
    let mut next_index: usize = 0;

    // Helper: assigns a fresh index to a variable name on first sight,
    // returns the existing index on subsequent encounters.
    let register_var = |name: &str, map: &mut HashMap<String, usize>, next: &mut usize| {
        *map.entry(name.to_string()).or_insert_with(|| {
            let idx = *next;
            *next += 1;
            idx
        })
    };

    // Pass 1: head variables — establishes output tuple ordering.
    for t in &query.head.terms {
        if let Term::Var(ref vname) = t {
            let _ = register_var(vname, &mut var_to_index, &mut next_index);
        }
    }

    // Pass 2: body-only variables — any variable not already seen in the head.
    for pred in &query.body {
        for t in &pred.terms {
            if let Term::Var(ref vname) = t {
                let _ = register_var(vname, &mut var_to_index, &mut next_index);
            }
        }
    }

    // Pass 3: per-predicate variable index lists.
    let mut predicate_variables: Vec<Vec<usize>> = Vec::with_capacity(query.body.len());
    for pred in &query.body {
        let mut vars_for_pred: Vec<usize> = Vec::new();
        for t in &pred.terms {
            if let Term::Var(ref vname) = t {
                if let Some(idx) = var_to_index.get(vname) {
                    vars_for_pred.push(*idx);
                }
            }
        }
        predicate_variables.push(vars_for_pred);
    }

    QueryAnalysis {
        num_vars: var_to_index.len(),
        predicate_variables,
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --package kermit-algos`
Expected: PASS (new analysis tests + all pre-existing tests; algorithms untouched).

- [ ] **Step 5: Commit**

```bash
git add kermit-algos/src/optimiser/ kermit-algos/src/lib.rs
git commit -m "feat(kermit-algos): optimiser module with shared query analysis (hoisted from build_variable_index)"
```

---

### Task 5: `topological_order` + `QueryPlan`/`validate`

**Files:**
- Create: `kermit-algos/src/optimiser/ordering.rs`, `kermit-algos/src/optimiser/plan.rs`
- Modify: `kermit-algos/src/optimiser/mod.rs`, `kermit-algos/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `kermit-algos/src/optimiser/ordering.rs` with the test mod at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_rank_reproduces_lexicographic_order() {
        // Triangle: R(0,1), S(1,2), T(0,2) — edges 0->1, 1->2, 0->2.
        let preds = vec![vec![0, 1], vec![1, 2], vec![0, 2]];
        assert_eq!(topological_order(3, &preds, |v| v), vec![0, 1, 2]);
    }

    #[test]
    fn constraint_forces_late_canonical_variable_first() {
        // r(K, Y) with K canonical index 1, Y index 0: edge 1 -> 0.
        // The subject-position-constant shape — K must come first.
        let preds = vec![vec![1, 0], vec![1]];
        assert_eq!(topological_order(2, &preds, |v| v), vec![1, 0]);
    }

    #[test]
    fn rank_breaks_ties_among_ready_variables() {
        // Star: R(0,1), S(0,2). After 0, both 1 and 2 are ready; the rank
        // function prefers 2.
        let preds = vec![vec![0, 1], vec![0, 2]];
        let order = topological_order(3, &preds, |v| if v == 2 { 0 } else { v + 1 });
        assert_eq!(order, vec![0, 2, 1]);
    }

    #[test]
    #[should_panic(expected = "cyclic global attribute order")]
    fn cyclic_constraints_panic() {
        // r(X, Y), s(Y, X): edges 0->1 and 1->0.
        let preds = vec![vec![0, 1], vec![1, 0]];
        topological_order(2, &preds, |v| v);
    }
}
```

Create `kermit-algos/src/optimiser/plan.rs` with the test mod at the bottom:

```rust
#[cfg(test)]
mod tests {
    use {super::*, crate::optimiser::analysis::analyse, kermit_parser::JoinQuery};

    fn triangle() -> QueryAnalysis {
        let q: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        analyse(&q)
    }

    #[test]
    fn valid_plan_passes() {
        let plan = QueryPlan {
            variable_ordering: vec![0, 1, 2],
        };
        assert_eq!(plan.validate(&triangle()), Ok(()));
    }

    #[test]
    fn wrong_length_is_not_a_permutation() {
        let plan = QueryPlan {
            variable_ordering: vec![0, 1],
        };
        assert_eq!(plan.validate(&triangle()), Err(PlanError::NotAPermutation));
    }

    #[test]
    fn repeated_variable_is_not_a_permutation() {
        let plan = QueryPlan {
            variable_ordering: vec![0, 1, 1],
        };
        assert_eq!(plan.validate(&triangle()), Err(PlanError::NotAPermutation));
    }

    #[test]
    fn column_order_violation_is_rejected() {
        // Ordering Y before X violates R(X, Y)'s physical column order.
        let plan = QueryPlan {
            variable_ordering: vec![1, 0, 2],
        };
        assert_eq!(
            plan.validate(&triangle()),
            Err(PlanError::ColumnOrderViolated {
                predicate: 0,
                earlier: 0,
                later: 1,
            })
        );
    }
}
```

- [ ] **Step 2: Register modules so the failure is a missing implementation**

In `kermit-algos/src/optimiser/mod.rs` add `mod ordering;` and `mod plan;` to the module list and extend the re-export:

```rust
pub use {
    analysis::{analyse, QueryAnalysis},
    ordering::topological_order,
    plan::{PlanError, QueryPlan},
};
```

In `kermit-algos/src/lib.rs` extend the optimiser re-export line to:

```rust
    optimiser::{analyse, topological_order, PlanError, QueryAnalysis, QueryPlan},
```

Run: `cargo test --package kermit-algos optimiser`
Expected: compile FAIL — items not yet defined.

- [ ] **Step 3: Implement `ordering.rs`**

This is `leapfrog_triejoin.rs::global_attribute_order` (lines 405–442) with the ready-heap key generalised from `Reverse<usize>` to `Reverse<(K, usize)>`. Above the test mod:

```rust
//! Constraint-respecting topological ordering shared by all optimisers.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashSet},
};

/// Computes a *global attribute order* (GAO): a permutation of
/// `0..num_vars` in which every relation's variables appear in physical
/// column order.
///
/// Trie-descending joins bind each relation one physical column per depth,
/// so a relation `r(K, Y)` participates correctly only if its first column
/// `K` is bound before its second column `Y`. Each relation contributes
/// edges `col[i] -> col[i+1]` (a variable repeated within one predicate
/// imposes no self-constraint); Kahn's algorithm yields a valid order.
///
/// `rank` maps a candidate variable to an [`Ord`] key; among the variables
/// whose constraints are currently satisfied (the *ready set*), the
/// smallest key is emitted next, with the canonical index as the final
/// deterministic tie-break. Because candidates are restricted to the ready
/// set, **any** rank function yields a valid order — an ordering policy
/// can be slow, never wrong.
///
/// # Panics
///
/// Panics if the constraints are cyclic (e.g. `r(X, Y), s(Y, X)`):
/// answering such a query would require a relation sorted in two different
/// column orders at once, which a single fixed trie order cannot provide.
pub fn topological_order<K: Ord>(
    num_vars: usize, predicate_variables: &[Vec<usize>], rank: impl Fn(usize) -> K,
) -> Vec<usize> {
    let mut adjacency: Vec<HashSet<usize>> = vec![HashSet::new(); num_vars];
    let mut in_degree: Vec<usize> = vec![0; num_vars];

    for vars in predicate_variables {
        for pair in vars.windows(2) {
            let (earlier, later) = (pair[0], pair[1]);
            if earlier != later && adjacency[earlier].insert(later) {
                in_degree[later] += 1;
            }
        }
    }

    let mut ready: BinaryHeap<Reverse<(K, usize)>> = (0..num_vars)
        .filter(|&v| in_degree[v] == 0)
        .map(|v| Reverse((rank(v), v)))
        .collect();
    let mut order = Vec::with_capacity(num_vars);
    while let Some(Reverse((_, v))) = ready.pop() {
        order.push(v);
        for &w in &adjacency[v] {
            in_degree[w] -= 1;
            if in_degree[w] == 0 {
                ready.push(Reverse((rank(w), w)));
            }
        }
    }

    assert_eq!(
        order.len(),
        num_vars,
        "query imposes a cyclic global attribute order; the join cannot answer it with a single \
         trie column order per relation"
    );
    order
}
```

- [ ] **Step 4: Implement `plan.rs`**

Above the test mod (error shape mirrors `const_rewrite::RewriteError` — manual `Display` + `Error`):

```rust
//! The query plan consumed by join algorithms.

use {crate::optimiser::analysis::QueryAnalysis, std::fmt};

/// An executable plan for a join query, produced by a
/// [`QueryOptimiser`](crate::optimiser::QueryOptimiser).
///
/// v1 carries only the global attribute order. Future planning decisions
/// (body-atom order, join trees for binary algorithms) grow as new fields,
/// non-breaking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlan {
    /// Canonical variable indices (see
    /// [`analyse`](crate::optimiser::analyse)) in descent order.
    pub variable_ordering: Vec<usize>,
}

/// Error returned by [`QueryPlan::validate`] when a plan cannot execute
/// against the query it claims to plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The ordering is not a permutation of `0..num_vars`.
    NotAPermutation,
    /// A relation's physical column order is violated: body predicate
    /// `predicate` requires `earlier` to be bound before `later`.
    ColumnOrderViolated {
        /// Index of the violated body predicate.
        predicate: usize,
        /// Variable that must come first.
        earlier: usize,
        /// Variable the plan wrongly ordered before `earlier`.
        later: usize,
    },
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | PlanError::NotAPermutation => {
                write!(f, "variable ordering is not a permutation of the query's variables")
            },
            | PlanError::ColumnOrderViolated {
                predicate,
                earlier,
                later,
            } => write!(
                f,
                "body predicate {predicate} requires variable {earlier} bound before {later}, \
                 but the plan orders them the other way"
            ),
        }
    }
}

impl std::error::Error for PlanError {}

impl QueryPlan {
    /// Checks that this plan can execute against a query with the given
    /// [`QueryAnalysis`]: the ordering must be a permutation of
    /// `0..num_vars` respecting every predicate's physical column order.
    ///
    /// Executors call this before descending; the provided optimisers are
    /// valid by construction (they only pick among Kahn-ready variables),
    /// so a failure here means a hand-built or third-party plan is broken.
    ///
    /// # Errors
    ///
    /// Returns the first violation found, checking the permutation
    /// property before column-order constraints.
    pub fn validate(&self, analysis: &QueryAnalysis) -> Result<(), PlanError> {
        let n = analysis.num_vars;
        if self.variable_ordering.len() != n {
            return Err(PlanError::NotAPermutation);
        }
        let mut position = vec![usize::MAX; n];
        for (pos, &v) in self.variable_ordering.iter().enumerate() {
            if v >= n || position[v] != usize::MAX {
                return Err(PlanError::NotAPermutation);
            }
            position[v] = pos;
        }
        for (p, vars) in analysis.predicate_variables.iter().enumerate() {
            for pair in vars.windows(2) {
                let (earlier, later) = (pair[0], pair[1]);
                if earlier != later && position[earlier] > position[later] {
                    return Err(PlanError::ColumnOrderViolated {
                        predicate: p,
                        earlier,
                        later,
                    });
                }
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --package kermit-algos`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add kermit-algos/src/optimiser/ kermit-algos/src/lib.rs
git commit -m "feat(kermit-algos): QueryPlan with validation and rank-parameterised topological ordering"
```

---

### Task 6: `QueryOptimiser` trait, `CatalogStats`, `LexicographicOptimiser`

**Files:**
- Create: `kermit-algos/src/optimiser/lexicographic.rs`
- Modify: `kermit-algos/src/optimiser/mod.rs`, `kermit-algos/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Add a test mod at the bottom of `kermit-algos/src/optimiser/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use {super::*, kermit_parser::JoinQuery};

    #[test]
    fn for_query_records_relations_and_const_singletons() {
        let q: JoinQuery = "Q(X) :- R(X, K0), Const_c5(K0).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, |name| match name {
            | "R" => Some(42),
            | _ => None,
        });
        assert_eq!(stats.tuples("R"), Some(42));
        assert_eq!(stats.tuples("Const_c5"), Some(1));
        assert_eq!(stats.tuples("Unknown"), None);
    }

    #[test]
    fn for_query_skips_unknown_relations() {
        let q: JoinQuery = "Q(X) :- R(X), Mystery(X).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, |name| (name == "R").then_some(7));
        assert_eq!(stats.tuples("Mystery"), None);
    }
}
```

Create `kermit-algos/src/optimiser/lexicographic.rs` with tests at the bottom:

```rust
#[cfg(test)]
mod tests {
    use {super::*, kermit_parser::JoinQuery};

    #[test]
    fn triangle_orders_head_first() {
        let q: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        let plan = LexicographicOptimiser.plan(&q, &CatalogStats::default());
        assert_eq!(plan.variable_ordering, vec![0, 1, 2]);
    }

    #[test]
    fn subject_position_constant_shape_respects_column_order() {
        // K (canonical 1) is physically first in r, so it must precede Y
        // (canonical 0) despite the head-first numbering.
        let q: JoinQuery = "Q(Y) :- r(K, Y), Const_c7(K).".parse().unwrap();
        let plan = LexicographicOptimiser.plan(&q, &CatalogStats::default());
        assert_eq!(plan.variable_ordering, vec![1, 0]);
    }

    #[test]
    fn stats_are_ignored() {
        let q: JoinQuery = "Q(X, Y) :- R(X), S(Y).".parse().unwrap();
        // Even with S tiny, lexicographic keeps canonical order.
        let stats = CatalogStats::for_query(&q, |name| match name {
            | "R" => Some(1_000_000),
            | "S" => Some(1),
            | _ => None,
        });
        let plan = LexicographicOptimiser.plan(&q, &stats);
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }
}
```

Run: `cargo test --package kermit-algos optimiser`
Expected: compile FAIL.

- [ ] **Step 2: Implement the trait + stats in `mod.rs`**

Replace the body of `kermit-algos/src/optimiser/mod.rs` (keeping the test mod from Step 1) with:

```rust
//! Query optimisers: planners that choose the global attribute order.
//!
//! Kermit separates planning from execution. A [`QueryOptimiser`] consumes
//! a parsed, const-rewritten [`JoinQuery`] plus per-relation statistics
//! ([`CatalogStats`]) and produces a [`QueryPlan`]; join algorithms
//! (`JoinAlgo::join_iter`) execute the plan. The space of valid plans is
//! exactly the set of topological orders of the column-order constraint
//! DAG (see [`topological_order`]); the provided optimisers are valid by
//! construction, and executors defensively assert
//! [`QueryPlan::validate`].

mod analysis;
mod lexicographic;
mod ordering;
mod plan;

pub use {
    analysis::{analyse, QueryAnalysis},
    lexicographic::LexicographicOptimiser,
    ordering::topological_order,
    plan::{PlanError, QueryPlan},
};

use {kermit_parser::JoinQuery, std::collections::BTreeMap};

/// Statistics for one relation, as visible to the planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationStats {
    /// Number of stored tuples (`kermit_ds::Cardinality` semantics: what a
    /// full iteration yields).
    pub tuples: usize,
    /// Number of attributes.
    pub arity: usize,
}

/// Per-relation statistics for the predicates of one query.
///
/// Plain data: planners never touch data structures, so they stay
/// data-structure-agnostic and unit-testable with literal maps. Callers
/// build one per join via [`CatalogStats::for_query`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogStats {
    relations: BTreeMap<String, RelationStats>,
}

impl CatalogStats {
    /// Records stats for `name`, replacing any previous entry.
    pub fn insert(&mut self, name: impl Into<String>, stats: RelationStats) {
        self.relations.insert(name.into(), stats);
    }

    /// Tuple count for `name`, if known.
    pub fn tuples(&self, name: &str) -> Option<usize> {
        self.relations.get(name).map(|s| s.tuples)
    }

    /// Builds stats for every body predicate of `query`.
    ///
    /// `tuple_count_of` supplies per-relation counts (typically from
    /// `kermit_ds::Cardinality::tuple_count`). Synthetic `Const_*`
    /// predicates (introduced by the const-view rewrite) are recorded as
    /// single-tuple unary relations. Predicates the lookup does not know
    /// get no entry — planners treat missing stats as "assume large".
    pub fn for_query(query: &JoinQuery, tuple_count_of: impl Fn(&str) -> Option<usize>) -> Self {
        let mut stats = CatalogStats::default();
        for pred in &query.body {
            if stats.relations.contains_key(&pred.name) {
                continue;
            }
            if pred.name.starts_with("Const_") {
                stats.insert(pred.name.clone(), RelationStats {
                    tuples: 1,
                    arity: 1,
                });
            } else if let Some(tuples) = tuple_count_of(&pred.name) {
                stats.insert(pred.name.clone(), RelationStats {
                    tuples,
                    arity: pred.terms.len(),
                });
            }
        }
        stats
    }
}

/// A query optimiser plans how a join executes.
///
/// Implementations receive the query *after* the const-view rewrite (the
/// exact query the executor will run, including synthetic `Const_*`
/// predicates) and produce a [`QueryPlan`] the executor consumes. Object
/// safe: the engine holds `Box<dyn QueryOptimiser>` so the choice is a
/// runtime decision.
pub trait QueryOptimiser {
    /// Produces a plan for `query` given per-relation `stats`.
    fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan;
}
```

Note: `mod cardinality;` and the `cardinality::CardinalityOptimiser,` re-export are deliberately absent — Task 7 adds them when the file exists, so this task compiles on its own.

- [ ] **Step 3: Implement `lexicographic.rs`**

Above its test mod:

```rust
//! The default ordering policy: smallest canonical variable index first.

use crate::optimiser::{
    analysis::analyse, ordering::topological_order, CatalogStats, QueryOptimiser, QueryPlan,
};

/// Plans the global attribute order by Kahn's topological sort with a
/// smallest-canonical-index tie-break.
///
/// This reproduces, bit for bit, the ordering kermit hardcoded before
/// query optimisers existed — it keeps head variables early when
/// unconstrained and is fully deterministic. It ignores statistics, which
/// makes it the control arm for optimiser ablation studies and the
/// default everywhere.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LexicographicOptimiser;

impl QueryOptimiser for LexicographicOptimiser {
    fn plan(&self, query: &kermit_parser::JoinQuery, _stats: &CatalogStats) -> QueryPlan {
        let analysis = analyse(query);
        QueryPlan {
            variable_ordering: topological_order(
                analysis.num_vars,
                &analysis.predicate_variables,
                |v| v,
            ),
        }
    }
}
```

Register in `mod.rs` (`mod lexicographic;` + re-export — already shown in Step 2) and extend the `kermit-algos/src/lib.rs` re-export line:

```rust
    optimiser::{
        analyse, topological_order, CatalogStats, LexicographicOptimiser, PlanError,
        QueryAnalysis, QueryOptimiser, QueryPlan, RelationStats,
    },
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --package kermit-algos`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add kermit-algos/src/optimiser/ kermit-algos/src/lib.rs
git commit -m "feat(kermit-algos): QueryOptimiser trait, CatalogStats, LexicographicOptimiser default"
```

---

### Task 7: `CardinalityOptimiser`

**Files:**
- Create: `kermit-algos/src/optimiser/cardinality.rs`
- Modify: `kermit-algos/src/optimiser/mod.rs`, `kermit-algos/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `kermit-algos/src/optimiser/cardinality.rs` with the test mod:

```rust
#[cfg(test)]
mod tests {
    use {super::*, kermit_parser::JoinQuery};

    fn stats_for(q: &JoinQuery, sizes: &[(&str, usize)]) -> CatalogStats {
        CatalogStats::for_query(q, |name| {
            sizes.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
        })
    }

    #[test]
    fn star_prefers_variable_from_smaller_relation() {
        // Q(A, B, C) :- R(A, B), S(A, C). After A, both B and C are ready;
        // S is smaller, so C (its variable) comes first.
        let q: JoinQuery = "Q(A, B, C) :- R(A, B), S(A, C).".parse().unwrap();
        let plan =
            CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 1000), ("S", 3)]));
        assert_eq!(plan.variable_ordering, vec![0, 2, 1]);
        // Flip the sizes: canonical order wins.
        let plan =
            CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 3), ("S", 1000)]));
        assert_eq!(plan.variable_ordering, vec![0, 1, 2]);
    }

    #[test]
    fn const_singleton_binds_first() {
        // Const_c9 has one tuple, so its variable K wins over X even
        // though X is canonically first and unconstrained.
        let q: JoinQuery = "Q(X, K) :- R(X), Const_c9(K).".parse().unwrap();
        let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 500)]));
        assert_eq!(plan.variable_ordering, vec![1, 0]);
    }

    #[test]
    fn missing_stats_rank_last() {
        // Unknown has no stats — assume large; R's variable goes first.
        let q: JoinQuery = "Q(X, Y) :- R(X), Unknown(Y).".parse().unwrap();
        let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 500)]));
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }

    #[test]
    fn equal_sizes_fall_back_to_canonical_order() {
        let q: JoinQuery = "Q(X, Y) :- R(X), S(Y).".parse().unwrap();
        let plan =
            CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 10), ("S", 10)]));
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }

    #[test]
    fn column_order_constraints_still_bind() {
        // Even though Y's relation is tiny, R(X, Y) forces X first.
        let q: JoinQuery = "Q(X, Y) :- R(X, Y), S(Y).".parse().unwrap();
        let plan =
            CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 1000), ("S", 1)]));
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }
}
```

Run: `cargo test --package kermit-algos cardinality` — Expected: compile FAIL.

- [ ] **Step 2: Implement**

Above the test mod in `cardinality.rs`:

```rust
//! Smallest-relation-first ordering policy.

use crate::optimiser::{
    analysis::analyse, ordering::topological_order, CatalogStats, QueryOptimiser, QueryPlan,
};

/// Plans the global attribute order preferring variables that appear in
/// the smallest relations.
///
/// Each variable is ranked by the minimum tuple count over the body
/// predicates that mention it (a variable in a small relation can only
/// take few values, so binding it early shrinks the search space).
/// Relations without statistics rank as `usize::MAX` ("assume large");
/// synthetic `Const_*` singletons rank 1 and are therefore bound as early
/// as constraints allow — exactly the right treatment for constants. Ties
/// break on the canonical index, keeping plans deterministic for
/// reproducible benchmarks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CardinalityOptimiser;

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
            variable_ordering: topological_order(
                analysis.num_vars,
                &analysis.predicate_variables,
                |v| (min_size[v], v),
            ),
        }
    }
}
```

In `optimiser/mod.rs`: add `mod cardinality;` (alphabetically first in the module list) and `cardinality::CardinalityOptimiser,` to the re-export. In `kermit-algos/src/lib.rs`: add `CardinalityOptimiser` to the optimiser re-export list.

- [ ] **Step 3: Run the tests**

Run: `cargo test --package kermit-algos`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add kermit-algos/src/optimiser/ kermit-algos/src/lib.rs
git commit -m "feat(kermit-algos): CardinalityOptimiser — smallest-relation-first variable ordering"
```

---

### Task 8: The flip — `JoinAlgo` takes a plan; every call site updated

This task is atomic by necessity: the trait signature change breaks every caller until all are updated. Run the full workspace test suite before committing.

**Files:**
- Modify: `kermit-algos/src/join_algo.rs`
- Modify: `kermit-algos/src/leapfrog_triejoin.rs` (join_iter ~448–486; **delete** `build_variable_index` 325–376 and `global_attribute_order` 405–442)
- Modify: `kermit-algos/src/hash_triejoin.rs` (join_iter ~252–283; **delete** its `build_variable_index` 27–68; fix tests 289–392)
- Modify: `kermit/src/lib.rs` (`compute_join` ~34–79)
- Modify: `kermit/src/db.rs` (struct ~59–68, `DB` impl ~70–183, inherent impl ~185–198, `hash_join` ~230–270, `instantiate_database` ~286–319, tests ~330–446)
- Modify: `kermit/src/main.rs` (three call sites: `load_query` ~550, `run_benchmark` ~921, `run_benchmark_hash` ~1188)
- Modify: `kermit/tests/common/utils.rs`, `kermit/tests/subject_position_constant.rs`

- [ ] **Step 1: Change the trait**

`kermit-algos/src/join_algo.rs` becomes:

```rust
//! This module defines the `JoinAlgo` trait, used as a base for join
//! algorithms.

use {
    crate::optimiser::QueryPlan, kermit_iters::JoinIterable, kermit_parser::JoinQuery,
    std::collections::HashMap,
};

/// The `JoinAlgo` trait is used as a base for join algorithms.
pub trait JoinAlgo<DS>
where
    DS: JoinIterable,
{
    /// Joins the given iterables according to `plan`, which must have been
    /// produced by a [`QueryOptimiser`](crate::optimiser::QueryOptimiser)
    /// for this exact `query` (after any const-view rewrite). Returns an
    /// iterator over the resulting join.
    ///
    /// # Panics
    ///
    /// Implementations panic if `plan` fails
    /// [`QueryPlan::validate`] against `query` — an invalid plan is a
    /// caller programming error, never a recoverable runtime condition.
    fn join_iter(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>>;
}
```

- [ ] **Step 2: Update `LeapfrogTriejoin`**

In `kermit-algos/src/leapfrog_triejoin.rs`:

2a. **Delete** `build_variable_index` (lines 325–376) and `global_attribute_order` (lines 405–442) including their doc comments.

2b. Update imports: remove `Term` from the `kermit_parser` import and remove `cmp::Reverse` and `BinaryHeap`/`HashSet` from the `std` import (keep `HashMap`); add `crate::optimiser::{analyse, QueryPlan}` to the `crate` import group.

2c. New `join_iter` (the descent-order output remapping is untouched):

```rust
impl<DS> JoinAlgo<DS> for LeapfrogTriejoin
where
    DS: TrieIterable,
{
    fn join_iter(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>> {
        let analysis = analyse(&query);
        if let Err(e) = plan.validate(&analysis) {
            panic!("LeapfrogTriejoin::join_iter: invalid query plan: {e}");
        }
        let variable_ordering = plan.variable_ordering.clone();
        let predicate_variables = analysis.predicate_variables;

        let trie_iters: Vec<_> = query
            .body
            .iter()
            .map(|pred| {
                let ds = datastructures
                    .get(&pred.name)
                    .expect("Missing datastructure for predicate name");
                ds.trie_iter()
            })
            .collect();

        // The triejoin descends in `variable_ordering` (a valid descent order)
        // and yields tuples in *descent* order. Map descent position back to
        // canonical variable index so output columns stay in head-first order
        // regardless of the descent order chosen.
        let arity = variable_ordering.len();
        let mut descent_pos_of_var = vec![0usize; arity];
        for (pos, &v) in variable_ordering.iter().enumerate() {
            descent_pos_of_var[v] = pos;
        }

        LeapfrogTriejoinIter::new(variable_ordering, predicate_variables, trie_iters)
            .into_iter()
            .map(move |descent_tuple| {
                (0..arity)
                    .map(|v| descent_tuple[descent_pos_of_var[v]])
                    .collect()
            })
    }
}
```

(The inline tests in this file drive `LeapfrogTriejoinIter::new` directly and need no changes.)

- [ ] **Step 3: Update `HashTriejoin`**

In `kermit-algos/src/hash_triejoin.rs`:

3a. **Delete** its `build_variable_index` (lines 27–68) and its doc comment. Remove the now-unused `Term` import; add `crate::optimiser::{analyse, QueryPlan}` to the crate import group.

3b. New `join_iter`:

```rust
    fn join_iter(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>> {
        let analysis = analyse(&query);
        if let Err(e) = plan.validate(&analysis) {
            panic!("HashTriejoin::join_iter: invalid query plan: {e}");
        }
        let variable_ordering = &plan.variable_ordering;
        let predicate_variables = analysis.predicate_variables;
        let mut iters: Vec<_> = query
            .body
            .iter()
            .map(|pred| {
                datastructures
                    .get(&pred.name)
                    .expect("Missing datastructure for predicate name")
                    .hash_trie_iter()
            })
            .collect();
        let variable_to_iter_map =
            build_variable_to_iter_map(variable_ordering, &predicate_variables);
        let mut output = Vec::new();
        enumerate(
            0,
            variable_ordering.len(),
            &mut iters,
            &predicate_variables,
            &variable_to_iter_map,
            &mut output,
        );
        output.into_iter()
    }
```

3c. Fix the inline tests: **delete** `variable_index_triangle` (lines 289–295 — the behaviour is covered by `optimiser::analysis` tests now). In `join_algo_unary_intersection` and `join_algo_triangle`, add `use crate::optimiser::{CatalogStats, LexicographicOptimiser, QueryOptimiser};` at the top of the test mod and change the invocation lines to:

```rust
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        let mut out: Vec<Vec<usize>> = HashTriejoin::join_iter(&plan, query, ds).collect();
```

- [ ] **Step 4: Verify kermit-algos is green in isolation**

Run: `cargo test --package kermit-algos`
Expected: PASS. (`kermit` crate still broken — that's next.)

- [ ] **Step 5: Update the engine (`kermit/src/db.rs`)**

5a. Imports — extend the `kermit_algos` group with `CatalogStats, LexicographicOptimiser, QueryOptimiser` and the `kermit_ds` group with `Cardinality`.

5b. Struct gains the optimiser (after `relations`):

```rust
pub struct DatabaseEngine<R, JA>
where
    R: Relation,
{
    name: String,
    relations: HashMap<String, R>,
    /// Plans each join's variable ordering. Defaults to
    /// [`LexicographicOptimiser`]; see [`DatabaseEngine::with_optimiser`].
    optimiser: Box<dyn QueryOptimiser>,
    // `JA` does not appear in any field; PhantomData satisfies the
    // unused-type-parameter rule. `R` is already used by `relations`.
    phantom_ja: std::marker::PhantomData<JA>,
}
```

5c. The `DB for DatabaseEngine` impl block bound becomes `R: Relation + TrieIterable + Cardinality` (add `+ Cardinality`). Both constructors (`DB::new` and the inherent `new`) add `optimiser: Box::new(LexicographicOptimiser),` to their struct literals. Add an inherent constructor below the inherent `new` (in the `impl<R, JA> DatabaseEngine<R, JA> where R: Relation` block):

```rust
    /// Like [`DatabaseEngine::new`] but with an explicit query optimiser.
    pub fn with_optimiser(name: String, optimiser: Box<dyn QueryOptimiser>) -> Self {
        DatabaseEngine {
            name,
            relations: HashMap::new(),
            optimiser,
            phantom_ja: std::marker::PhantomData,
        }
    }
```

5d. In `DB::join` (currently ~117–154), after the wrapper-building loops and before building `ds_map`, insert the stats + plan and pass the plan to `join_iter`:

```rust
        let stats = CatalogStats::for_query(&rewritten, |name| {
            self.relations.get(name).map(Cardinality::tuple_count)
        });
        let plan = self.optimiser.plan(&rewritten, &stats);

        let ds_map: HashMap<String, &TrieIterKind<'_, R>> =
            wrappers.iter().map(|(k, v)| (k.clone(), v)).collect();

        JA::join_iter(&plan, rewritten, ds_map).collect()
```

5e. `hash_join` gains the optimiser parameter and the same stats/plan step. New signature and tail (bound gains `+ Cardinality`):

```rust
pub fn hash_join<R, H>(
    relations: &HashMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
) -> Vec<Vec<usize>>
where
    R: HashTrieIterable + Cardinality,
    H: HashStrategy,
{
    // ... existing rewrite + wrapper loops unchanged ...

    let stats = CatalogStats::for_query(&rewritten, |name| {
        relations.get(name).map(Cardinality::tuple_count)
    });
    let plan = optimiser.plan(&rewritten, &stats);

    let ds_map: HashMap<String, &HashTrieIterKind<'_, R>> =
        wrappers.iter().map(|(k, v)| (k.clone(), v)).collect();

    <HashTriejoin as JoinAlgo<HashTrieIterKind<'_, R>>>::join_iter(&plan, rewritten, ds_map)
        .collect()
}
```

Update its doc comment: add a `optimiser` sentence ("`optimiser` plans the variable ordering; pass `&LexicographicOptimiser` for the historical default").

5f. `instantiate_database` gains the optimiser and threads it:

```rust
pub fn instantiate_database(
    ds: IndexStructure, ja: JoinAlgorithm, optimiser: Box<dyn QueryOptimiser>, name: String,
) -> Box<dyn DB> {
    match (ds, ja) {
        | (IndexStructure::TreeTrie, JoinAlgorithm::LeapfrogTriejoin) => Box::new(
            DatabaseEngine::<TreeTrie, LeapfrogTriejoin>::with_optimiser(name, optimiser),
        ),
        | (IndexStructure::ColumnTrie, JoinAlgorithm::LeapfrogTriejoin) => Box::new(
            DatabaseEngine::<ColumnTrie, LeapfrogTriejoin>::with_optimiser(name, optimiser),
        ),
        // ... panic arms unchanged ...
    }
}
```

5g. Fix the db.rs tests: the two `hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(&relations, q)` calls (~405, ~437) gain a third argument `&LexicographicOptimiser`.

- [ ] **Step 6: Update `compute_join` (`kermit/src/lib.rs`)**

Extend the imports to `kermit_algos::{CatalogStats, JoinAlgo, JoinQuery, LexicographicOptimiser, QueryOptimiser}` and replace the final line:

```rust
    // Synthetic queries carry no constants and this helper predates
    // statistics, so plan with the stats-free default policy.
    let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
    JA::join_iter(&plan, query, ds_map).collect()
```

- [ ] **Step 7: Update the three `main.rs` call sites (temporary hardwired default; Task 10 threads the CLI flag)**

Add `LexicographicOptimiser` to main.rs's `kermit_algos` import. Then:

- `load_query` (~550): `instantiate_database(args.indexstructure, args.algorithm, Box::new(LexicographicOptimiser), "join".to_string())`
- `run_benchmark` (~921): `instantiate_database(indexstructure, algorithm, Box::new(LexicographicOptimiser), benchmark.name.clone())`
- `run_benchmark_hash` (~1188): `hash_join::<HashTrie<H>, H>(&named, q, &LexicographicOptimiser)`

- [ ] **Step 8: Update the test helpers**

`kermit/tests/common/utils.rs` — extend imports and plan before joining (the generic `O` parameter comes in Task 9; use the default policy for now):

```rust
use {
    kermit_algos::{CatalogStats, JoinAlgo, JoinQuery, LexicographicOptimiser, QueryOptimiser},
    kermit_ds::Relation,
    std::collections::HashMap,
};
```

and replace the execution line:

```rust
    let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
    let mut actual: Vec<Vec<usize>> = JA::join_iter(&plan, query, ds_map).collect();
```

`kermit/tests/subject_position_constant.rs` — add `LexicographicOptimiser` to its `kermit` / `kermit_algos` imports and change the `hash()` helper's call to `hash_join::<HashTrieSip, SipHashStrategy>(&edge_rels(), q, &LexicographicOptimiser)`.

- [ ] **Step 9: Full workspace test run**

Run: `cargo test --verbose`
Expected: PASS — every existing test unchanged in outcome, because `LexicographicOptimiser` + `topological_order(|v| v)` reproduces the old `global_attribute_order` exactly.

- [ ] **Step 10: Commit**

```bash
git add kermit-algos/src kermit/src kermit/tests
git commit -m "feat!: JoinAlgo::join_iter consumes a QueryPlan; engine plans via QueryOptimiser

LeapfrogTriejoin and HashTriejoin no longer compute their own variable
ordering: the shared optimiser::analyse + QueryPlan (validated) replace
the duplicated build_variable_index/global_attribute_order.
DatabaseEngine holds Box<dyn QueryOptimiser> (default lexicographic,
bit-for-bit the old ordering); hash_join takes &dyn QueryOptimiser."
```

---

### Task 9: Thread the optimiser through the multiway-join test suite

**Files:**
- Modify: `kermit/tests/common/utils.rs`, `kermit/tests/common/macros.rs`, `kermit/tests/join_tests.rs`

- [ ] **Step 1: Generalise `test_join` over the optimiser and give it real stats**

Replace the signature/bounds and the plan lines in `kermit/tests/common/utils.rs`:

```rust
use {
    kermit_algos::{CatalogStats, JoinAlgo, JoinQuery, QueryOptimiser},
    kermit_ds::{Cardinality, Relation},
    std::collections::HashMap,
};

pub fn test_join<R, JA, O>(
    input: Vec<Vec<Vec<usize>>>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>,
    result: Vec<Vec<usize>>,
) where
    R: Relation + Cardinality,
    JA: JoinAlgo<R>,
    O: QueryOptimiser + Default,
{
    // ... relation building, query synthesis, ds_map — unchanged ...

    let stats =
        CatalogStats::for_query(&query, |name| ds_map.get(name).map(|r| r.tuple_count()));
    let plan = O::default().plan(&query, &stats);

    // Multiset equality (relational algebra semantics) — sort both sides
    // before asserting so algorithms with non-sorted output (hash-trie
    // family) and plans with different enumeration orders pass the same
    // suite.
    let mut actual: Vec<Vec<usize>> = JA::join_iter(&plan, query, ds_map).collect();
    actual.sort();
    let mut expected = result;
    expected.sort();
    assert_eq!(actual, expected);
}
```

- [ ] **Step 2: Thread `$optimiser:ty` through the macros**

`kermit/tests/common/macros.rs`. Base macro (lines 1–27) — add the parameter and pass it as the third type argument:

```rust
#[macro_export]
macro_rules! define_multiway_join_test {
    (
        $test_name:ident,
        $relation_type:ident,
        $join_algorithm:ty,
        $optimiser:ty,
        [ $( $input:expr ),+ $(,)? ],
        $join_vars:expr,
        $projection:expr,
        $expected:expr,
        $debugger:block
    ) => {
        #[test]
        fn $test_name() {
            let inputs: Vec<Vec<Vec<usize>>> = vec![$($input.to_vec()),+];

            $debugger

            $crate::common::utils::test_join::<$relation_type, $join_algorithm, $optimiser>(
                inputs,
                $join_vars.to_vec(),
                $projection.to_vec(),
                $expected.to_vec(),
            );
        }
    };
}
```

Each of the 11 per-pattern macros gets the **same three mechanical edits** (shown here on `define_unary_multiway_join_test!`, lines 30–48; apply identically to `define_triangle_…` 51–70, `define_chain_…` 73–92, `define_star_…` 95–113, `define_self_…` 116–134, `define_existential_…` 137–155, `define_empty_result_…` 158–176, `define_single_relation_…` 179–196, `define_four_way_chain_…` 199–219, `define_wide_fanout_…` 222–244, `define_dead_end_…` 247–265):

1. Matcher: `($relation_type:ident, $join_algorithm:ty)` → `($relation_type:ident, $join_algorithm:ty, $optimiser:ty)`
2. Test name: append `_ $optimiser:lower` inside the paste brackets
3. Forward: add `$optimiser,` on a new line after `$join_algorithm,`

Result for the unary macro:

```rust
#[macro_export]
macro_rules! define_unary_multiway_join_test {
    ($relation_type:ident, $join_algorithm:ty, $optimiser:ty) => {
        paste::paste! {
        $crate::define_multiway_join_test!(
            [<simple_multiwayjoin_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower>],
            $relation_type,
            $join_algorithm,
            $optimiser,
            [
                vec![vec![1], vec![2], vec![3]],
                vec![vec![1], vec![2], vec![3]]
            ],
            vec![0],
            vec![vec![0]],
            vec![vec![1], vec![2], vec![3]],
            {print!("");}
        );
        }
    };
}
```

Suite macro (lines 267–332): matcher becomes triples and each of the 11 inner calls gains `, $optimiser`:

```rust
#[macro_export]
macro_rules! define_multiway_join_test_suite {
    (
        $(
            $relation_type:ident,
            $join_algorithm:ty,
            $optimiser:ty
        ),+
    ) => {
        $(
                $crate::define_unary_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_triangle_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_chain_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_star_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_self_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_existential_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_empty_result_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_single_relation_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_four_way_chain_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_wide_fanout_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
                $crate::define_dead_end_multiway_join_test!( $relation_type, $join_algorithm, $optimiser );
        )+
    };
}
```

- [ ] **Step 3: Invoke the suite for every (DS, algo, optimiser) combination**

`kermit/tests/join_tests.rs` — add `CardinalityOptimiser, LexicographicOptimiser` to the `kermit_algos` import and replace the four invocations with eight:

```rust
define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, CardinalityOptimiser);
```

- [ ] **Step 4: Run and count**

Run: `cargo test --package kermit --test join_tests`
Expected: PASS, **88** tests (8 combinations × 11 patterns). Every pattern must produce identical result multisets under both optimisers.

- [ ] **Step 5: Commit**

```bash
git add kermit/tests/common kermit/tests/join_tests.rs
git commit -m "test: run the 11-pattern multiway join suite under both optimisers (88 tests)"
```

---

### Task 10: CLI `--optimiser` flag + bench-report `optimiser` axis

**Files:**
- Modify: `kermit-algos/src/lib.rs` (new `Optimiser` CLI enum next to `JoinAlgorithm`)
- Modify: `kermit/src/main.rs` (`QueryArgs` ~64–87, `BenchSubcommand::Run` ~322–370, `load_query` ~542–557, `run_benchmark` ~878–1055, `run_benchmark_hash` ~1067–1240, bench-join handler ~1392–1457, bench-run dispatch ~1514–1620)
- Create: `kermit/tests/cli_optimiser_choice.rs`

- [ ] **Step 1: Write the failing CLI test**

Create `kermit/tests/cli_optimiser_choice.rs` (modelled on `cli_hash_trie_hasher_choice.rs`):

```rust
//! CLI smoke tests: `--optimiser` selection lands in the bench-report
//! `optimiser` axis, defaulting to `lexicographic`.

use std::{fs, path::PathBuf, process::Command};

fn kermit_bin() -> &'static str { env!("CARGO_BIN_EXE_kermit") }

fn edge_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/edge.csv")
}

/// Runs `bench join` against the edge fixture with `extra_args` appended,
/// returning the parsed report array. `tag` keeps concurrent tests' temp
/// files apart.
fn run_bench_join(tag: &str, extra_args: &[&str]) -> Vec<serde_json::Value> {
    let pid = std::process::id();
    let report_path = std::env::temp_dir().join(format!("kermit-optimiser-{tag}-{pid}.json"));
    let query_path = std::env::temp_dir().join(format!("kermit-optimiser-{tag}-{pid}.dl"));
    fs::write(&query_path, "Q(X, Y) :- edge(X, Y).").unwrap();

    let mut cmd = Command::new(kermit_bin());
    cmd.args([
        "bench",
        "--sample-size",
        "10",
        "--measurement-time",
        "1",
        "--warm-up-time",
        "1",
        "--report-json",
        report_path.to_str().unwrap(),
        "join",
        "--relations",
        edge_fixture().to_str().unwrap(),
        "--query",
        query_path.to_str().unwrap(),
        "--algorithm",
        "leapfrog-triejoin",
        "--indexstructure",
        "tree-trie",
    ]);
    cmd.args(extra_args);
    let output = cmd.output().expect("failed to run kermit binary");
    assert!(
        output.status.success(),
        "kermit bench join failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let reports: Vec<serde_json::Value> =
        serde_json::from_str(&fs::read_to_string(&report_path).unwrap()).unwrap();
    let _ = fs::remove_file(&report_path);
    let _ = fs::remove_file(&query_path);
    assert!(!reports.is_empty(), "report array should not be empty");
    reports
}

#[test]
fn cli_bench_join_with_cardinality_optimiser_records_axis() {
    let reports = run_bench_join("cardinality", &["--optimiser", "cardinality"]);
    assert_eq!(reports[0]["axes"]["optimiser"], "cardinality");
}

#[test]
fn cli_bench_join_default_optimiser_is_lexicographic() {
    let reports = run_bench_join("default", &[]);
    assert_eq!(reports[0]["axes"]["optimiser"], "lexicographic");
}
```

Run: `cargo test --package kermit --test cli_optimiser_choice`
Expected: FAIL — `--optimiser` is an unknown argument (first test) and the `optimiser` axis is missing (second test).

- [ ] **Step 2: Add the `Optimiser` CLI enum in `kermit-algos/src/lib.rs`**

Below the `JoinAlgorithm` enum + `FromStr` impl, following the same shape:

```rust
/// The available query optimisers.
///
/// Used as a CLI argument to select which [`QueryOptimiser`] plans the
/// join's variable ordering. Distinct from the optimization *axes*
/// standard (`ds_layout_*` etc.) — the optimiser is a first-class
/// benchmark dimension with its own `optimiser` report axis.
#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
pub enum Optimiser {
    /// Smallest canonical variable index first — reproduces the
    /// pre-optimiser hardcoded ordering. The default.
    Lexicographic,
    /// Smallest-relation-first; see [`CardinalityOptimiser`].
    Cardinality,
}

impl Optimiser {
    /// Boxes the corresponding [`QueryOptimiser`] implementation.
    pub fn instantiate(self) -> Box<dyn QueryOptimiser> {
        match self {
            | Self::Lexicographic => Box::new(LexicographicOptimiser),
            | Self::Cardinality => Box::new(CardinalityOptimiser),
        }
    }

    /// The bench-report axis value for this optimiser (the `optimiser`
    /// key).
    pub fn axis_value(self) -> &'static str {
        match self {
            | Self::Lexicographic => "lexicographic",
            | Self::Cardinality => "cardinality",
        }
    }
}
```

(The `use` items are already re-exported at the crate root; the enum can reference them directly.)

- [ ] **Step 3: Wire the flag through `main.rs`**

3a. Add `Optimiser` to main.rs's `kermit_algos` import.

3b. `QueryArgs` (shared by top-level `join` and `bench join`) gains, after `indexstructure`:

```rust
    /// Query optimiser (plans the join's variable ordering). Long-only:
    /// `-o` belongs to `--output`.
    #[arg(long, value_enum, default_value_t = Optimiser::Lexicographic)]
    optimiser: Optimiser,
```

(If clap rejects `default_value_t` for a non-`Display` type, use `#[arg(long, value_enum, default_value = "lexicographic")]` instead — the `Metric` field's `default_values_t` at main.rs:314 shows the `value_enum` + default machinery working, so `default_value_t` is expected to compile.)

3c. `BenchSubcommand::Run` gains the same field (after `algorithm`):

```rust
        /// Query optimiser (plans the join's variable ordering)
        #[arg(long, value_enum, default_value_t = Optimiser::Lexicographic)]
        optimiser: Optimiser,
```

3d. `load_query` (~550): `instantiate_database(args.indexstructure, args.algorithm, args.optimiser.instantiate(), "join".to_string())`.

3e. `run_benchmark` (~878): add parameter `optimiser: Optimiser`; at ~921 pass `optimiser.instantiate()` to `instantiate_database`; in the axes map (~1039–1045) add:

```rust
            ("optimiser".to_string(), serde_json::json!(optimiser.axis_value())),
```

3f. `run_benchmark_hash` (~1067): add parameter `optimiser: Optimiser`; before the bench closures create `let planned: Box<dyn kermit_algos::QueryOptimiser> = optimiser.instantiate();` and change the `hash_join` call (~1188) to `hash_join::<HashTrie<H>, H>(&named, q, planned.as_ref())`; add the same `("optimiser", …)` entry to its axes map (~1215–1230).

3g. Bench-run dispatch (~1514–1620): destructure the new `optimiser` field from `BenchSubcommand::Run` and pass it to every `run_benchmark::<…>` / `run_benchmark_hash::<…>` call in the loop.

3h. Bench-join handler (~1392–1457): the engine already plans via `load_query`; add to its axes map (~1432–1445):

```rust
            ("optimiser".to_string(), serde_json::json!(query_args.optimiser.axis_value())),
```

- [ ] **Step 4: Run the CLI tests, then the workspace**

Run: `cargo test --package kermit --test cli_optimiser_choice`
Expected: PASS (both tests).

Run: `cargo test --verbose`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add kermit-algos/src/lib.rs kermit/src/main.rs kermit/tests/cli_optimiser_choice.rs
git commit -m "feat(cli): --optimiser flag on join/bench join/bench run with 'optimiser' report axis"
```

---

### Task 11: Documentation

**Files:**
- Create: `docs/optimisers/TEMPLATE.md`, `docs/optimisers/lexicographic.md`, `docs/optimisers/cardinality.md`
- Modify: `CLAUDE.md`, `ARCHITECTURE.md`, `docs/specs/bench-report-schema.md`, `docs/algorithms/leapfrog-triejoin.md`, `docs/algorithms/hash-triejoin.md`

- [ ] **Step 1: Create `docs/optimisers/TEMPLATE.md`**

```markdown
# <Optimiser Name>

One-paragraph summary: what ordering policy this optimiser implements and
when to prefer it.

## Ranking function

The key the policy assigns each candidate variable, and how ties break.
Every optimiser orders variables within Kahn's topological sort over the
column-order constraint DAG (`kermit_algos::topological_order`), so every
produced plan is valid by construction — the policy only chooses among
valid orders.

## Statistics consumed

Which `CatalogStats` fields the policy reads (or "none").

## Complexity

Cost of `plan()` in terms of variables V, body predicates P, and
constraint edges E.

## Worked micro-example

A small query (plus stats if consumed), the constraint DAG, and the
resulting `variable_ordering`, step by step.

## CLI

    kermit bench run <bench> -i <ds> -a <algo> --optimiser <name>

Bench-report axis: `optimiser: "<name>"`.
```

- [ ] **Step 2: Create `docs/optimisers/lexicographic.md`**

```markdown
# Lexicographic Optimiser

The default ordering policy: among the variables whose column-order
constraints are satisfied, always bind the one with the smallest canonical
index next. Reproduces, bit for bit, the ordering kermit hardcoded before
query optimisers existed (`global_attribute_order`), so benchmarks planned
with it are directly comparable with pre-optimiser measurements. Prefer it
as the control arm of optimiser ablations and whenever no statistics are
available.

## Ranking function

`rank(v) = v` — the canonical variable index (head variables first, then
body-only variables in first-appearance order, per
`kermit_algos::analyse`). No tie-break is needed: the rank is already
unique per variable. Keeps head variables early when unconstrained.

## Statistics consumed

None. `plan()` ignores `CatalogStats` entirely.

## Complexity

O(E + V log V) for Kahn's sort with a binary-heap ready set, plus O(P · a)
to build the constraint edges (a = max predicate arity).

## Worked micro-example

Query: `Q(Y) :- r(K, Y), Const_c7(K).` (the subject-position-constant
shape). Canonical indices: `Y = 0` (head), `K = 1`. Constraint DAG:
`r(K, Y)` contributes the edge `1 -> 0`.

1. Ready set: `{1}` (only `K` has in-degree 0). Emit `1`.
2. Edge `1 -> 0` releases `0`. Ready set: `{0}`. Emit `0`.

`variable_ordering = [1, 0]` — the constant's variable binds first even
though the head variable is canonically smaller.

## CLI

    kermit bench run triangle -i tree-trie -a leapfrog-triejoin --optimiser lexicographic

Bench-report axis: `optimiser: "lexicographic"` (also the default when the
flag is omitted).
```

- [ ] **Step 3: Create `docs/optimisers/cardinality.md`**

```markdown
# Cardinality Optimiser

Smallest-relation-first: among the variables whose column-order
constraints are satisfied, bind next the one appearing in the smallest
relation. A variable occurring in a small relation can take only few
values, so binding it early shrinks the search space at every deeper
level. This is the classic size-driven heuristic for worst-case-optimal
joins; prefer it when relation sizes are skewed.

## Ranking function

`rank(v) = (min_size(v), v)` where `min_size(v)` is the minimum tuple
count over the body predicates that mention `v`. The canonical index is
the final tie-break, keeping plans deterministic for reproducible
benchmarks.

## Statistics consumed

`CatalogStats::tuples(name)` per body predicate. Relations without an
entry rank as `usize::MAX` ("assume large"). Synthetic `Const_*`
predicates (const-view rewrite) are recorded with one tuple, so constants
bind as early as constraints allow — no special-casing needed.

## Complexity

O(P · a) to compute per-variable minimum sizes, plus O(E + V log V) for
Kahn's sort (a = max predicate arity).

## Worked micro-example

Query: `Q(A, B, C) :- R(A, B), S(A, C).` with |R| = 1000, |S| = 3.
Canonical indices: `A = 0`, `B = 1`, `C = 2`. Constraint edges: `0 -> 1`
(from R) and `0 -> 2` (from S). Per-variable minimum sizes:
`A -> min(1000, 3) = 3`, `B -> 1000`, `C -> 3`.

1. Ready set: `{A}`. Emit `0`.
2. Both edges release `B` and `C`. Ready ranks: `B = (1000, 1)`,
   `C = (3, 2)`. `C` wins. Emit `2`.
3. Emit `1`.

`variable_ordering = [0, 2, 1]` — the small relation's variable is bound
before the large one's, unlike the lexicographic order `[0, 1, 2]`.

## CLI

    kermit bench run triangle -i tree-trie -a leapfrog-triejoin --optimiser cardinality

Bench-report axis: `optimiser: "cardinality"`.
```

- [ ] **Step 4: Update `CLAUDE.md`** (five insertions)

4a. Workspace Architecture — extend the `kermit-algos` line:

```
kermit-algos    → Join algorithms: LeapfrogJoinIter (binary), LeapfrogTriejoinIter (multi-way),
                  HashTriejoin (hash-based multi-way). Generic over data structures via the
                  JoinAlgo<DS> trait. Also hosts query optimisers (optimiser/ module):
                  QueryOptimiser implementations plan the variable ordering (QueryPlan)
                  that JoinAlgo::join_iter executes.
```

4b. Key Trait Hierarchy — add two bullets:

```
- **QueryOptimiser**: plans a `QueryPlan` (the LFTJ global attribute order) from a query + `CatalogStats`; consumed by `JoinAlgo::join_iter`. Implementations: `LexicographicOptimiser` (default), `CardinalityOptimiser`.
- **Cardinality**: stored-tuple count for optimiser statistics (`tuple_count()`; `HashTrie` counts multiset size)
```

4c. Extending the System — new recipe section after "Adding a new join algorithm":

```markdown
### Adding a new query optimiser

1. **Module layout.** Create `kermit-algos/src/optimiser/<name>.rs`. Existing precedents: `lexicographic.rs` (stats-free default) and `cardinality.rs` (smallest-relation-first).
2. **Implement `QueryOptimiser`.** Build the ordering with `topological_order(num_vars, predicate_variables, rank)` from the shared `ordering` module — ranking only chooses among Kahn-ready variables, so your plan is valid by construction. Consume statistics via `CatalogStats`; treat missing entries as "assume large". Make the final tie-break the canonical index so plans stay deterministic.
3. **Register the module.** Add `mod <name>;` and the re-export in `kermit-algos/src/optimiser/mod.rs`, plus the crate-root re-export in `kermit-algos/src/lib.rs`.
4. **Wire the CLI.** Add a variant to the `Optimiser` enum in `kermit-algos/src/lib.rs` (with `instantiate()` and `axis_value()` arms). The `--optimiser` flag on `join`, `bench join`, and `bench run` picks it up via `ValueEnum`.
5. **Wire the tests.** Add a `define_multiway_join_test_suite!(<DS>, <Algo>, <YourOptimiser>);` invocation per valid (DS, algorithm) pair in `kermit/tests/join_tests.rs` (Priorities item 1) plus unit tests for the ranking itself in your module.
6. **Write the doc.** Create `docs/optimisers/<name>.md` from `docs/optimisers/TEMPLATE.md` (Priorities item 3).

Do **not** modify other optimisers, algorithms, or index structures during this work (Priorities item 6).
```

4d. JSON bench reports gotcha — extend the conventional keys list: after `data_structure`, `algorithm`, add `optimiser`.

4e. Component Reference Docs — add:

```
- `docs/optimisers/lexicographic.md` — default variable-ordering policy (`--optimiser lexicographic`).
- `docs/optimisers/cardinality.md` — smallest-relation-first policy (`--optimiser cardinality`).
- `docs/optimisers/TEMPLATE.md` — skeleton for new optimiser docs.
```

- [ ] **Step 5: Update `ARCHITECTURE.md`**

Read the Datalog query-processing / data-flow section and insert (adapted to the surrounding prose style) at the point between the const-view rewrite and algorithm execution:

```markdown
Between the const-view rewrite and execution, the engine plans the join:
it gathers per-relation tuple counts (`Cardinality::tuple_count`) into
`CatalogStats` and asks its `QueryOptimiser` for a `QueryPlan` — the
global attribute order the algorithm will descend. The space of valid
plans is exactly the set of topological orders of the column-order
constraint DAG; provided optimisers rank candidates inside Kahn's
algorithm and are valid by construction, and `join_iter` asserts
`QueryPlan::validate` defensively. `LexicographicOptimiser` (default)
reproduces the historical hardcoded order; `CardinalityOptimiser`
prefers variables from small relations (`--optimiser cardinality`).
```

- [ ] **Step 6: Update `docs/specs/bench-report-schema.md`**

In the axes key catalogue, add a row/entry for `optimiser`: "Query optimiser that planned the join's variable ordering. Values: `lexicographic` (default), `cardinality`. Emitted by `bench join` and `bench run` (not `bench ds`, which performs no join)." Do **not** bump `schema_version` — the axes map is open; added keys are non-breaking.

- [ ] **Step 7: Update the two algorithm docs**

In `docs/algorithms/leapfrog-triejoin.md` and `docs/algorithms/hash-triejoin.md`, find the paragraph describing how the variable ordering / global attribute order is computed, and rewrite it to state: the ordering now arrives as a `QueryPlan` produced by a `QueryOptimiser` (see `docs/optimisers/`); the algorithm validates the plan (`QueryPlan::validate`) and panics on an invalid one; the default `lexicographic` optimiser reproduces the previously hardcoded Kahn's-with-smallest-index order, so documented behaviour is unchanged by default.

- [ ] **Step 8: Verify docs-affecting lints and commit**

Doc-comment/doc-file changes are covered by `cargo doc` + clippy (per the CLAUDE.md gotcha):

Run: `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace && RUSTFLAGS=-Dwarnings cargo clippy --all-targets`
Expected: clean.

```bash
git add docs/ CLAUDE.md ARCHITECTURE.md
git commit -m "docs: optimiser component docs, extension recipe, architecture and schema updates"
```

---

### Task 12: Final gates (CI parity)

- [ ] **Step 1: Full test suite** — `cargo test --verbose` → all green.
- [ ] **Step 2: Clippy as CI runs it** — `RUSTFLAGS=-Dwarnings cargo clippy --all-targets --verbose` → no warnings.
- [ ] **Step 3: Format** — `nix develop --command cargo fmt --all` then `git diff --stat` (review; formatting-only changes expected). **Never** run bare `cargo fmt` outside `nix develop`.
- [ ] **Step 4: Docs** — `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace` → clean.
- [ ] **Step 5: Miri** — `MIRIFLAGS="-Zmiri-disable-isolation" cargo miri setup && MIRIFLAGS="-Zmiri-disable-isolation" cargo miri test --package kermit-ds --package kermit-algos` (the packages this change touches; CI excludes `kermit`/`kermit-bench` from miri anyway) → clean.
- [ ] **Step 6: Smoke the CLI end-to-end**

```bash
cargo run -- bench --sample-size 10 --measurement-time 1 --warm-up-time 1 \
    --report-json /tmp/opt-smoke.json \
    join -r kermit/tests/fixtures/edge.csv -q <(echo 'Q(X, Y) :- edge(X, Y).') \
    -a leapfrog-triejoin -i tree-trie --optimiser cardinality
```

(If process substitution trips the query loader, write the query to a temp file first.) Then `jq '.[0].axes.optimiser' /tmp/opt-smoke.json` → `"cardinality"`.

- [ ] **Step 7: Commit any format/lint fixups**

```bash
git add -A && git commit -m "chore: fmt/lint fixups for query optimiser feature"
```

(Skip the commit if the tree is clean.)

---

## Self-Review Notes (already applied)

- **Spec coverage:** trait+plan+analysis (Tasks 4–6), cardinality policy (7), stats seam (1–3), executor flip incl. `hash_join`/`compute_join`/engine field (8), suite obligation with sorted comparison (9), CLI flag + axis + smoke test (10), docs incl. recipe + schema (11). Non-goals untouched.
- **Type consistency:** `tuple_count()` (Tasks 1–3) ↔ `CatalogStats::for_query` lookups (6, 8, 9); `topological_order(num_vars, preds, rank)` used identically in 5–7; `join_iter(&plan, query, ds_map)` shape identical across 8–10.
- **Behaviour preservation:** `LexicographicOptimiser` = old `global_attribute_order` because `Reverse<(v, v)>` orders exactly like `Reverse<v>`; Task 8 Step 9 (full suite) is the proof gate.
