# Streamed Join Iteration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the `iteration` metric, `end_to_end`'s K joins and
`--verify` from materialising the join result (issue #65), so that timing
excludes per-row allocation and memory doesn't grow with result size.

**Architecture:** `JoinAlgo::join_for_each` becomes the primitive both
algorithms implement. It passes each result tuple to a generic
`FnMut(&[usize])` sink as a borrowed slice, so no tuple is allocated.
`join_iter`, `lftj_join` and `hash_join` stay, and now collect through it.
`ExecutionFamily::count` drives the sink with `black_box` and a counter, and
`bench run` times that. The report schema goes to version 3, and kermit-lab
refuses to mix v2 and v3 reports.

**Tech Stack:** Rust nightly (through `nix develop`), Criterion 0.8.2, the
`allocation-counter` 0.8 dev-dependency (new), Python 3.13 kermit-lab with
pytest via `uv`.

**Spec:** `docs/specs/2026-10-02-streamed-join-iteration-design.md`. Read it
first.

---

## Ground rules for every task

- **Run all cargo commands inside `nix develop`.** Outside it, this host has
  system stable rustc 1.95 with no rustup, and `rust-toolchain.toml` is
  ignored. Use this pattern:
  `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo …'`
- **Run cargo in the foreground with `CARGO_BUILD_JOBS=2`.** The host is
  shared with three other sessions, and background jobs get killed when
  free RAM dips. The first build in this worktree is a full build, so expect
  several minutes.
- **Stay on branch `aidanb/65`.** Don't push to `origin/master`, and don't
  commit a `flake.lock` change.
- **Every commit message ends with these two lines:**
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01LbYG3WiiRqrZ8zek8u7AyZ
  ```
- **Backtick code-like identifiers in `///` and `//!` comments.** Clippy's
  `doc_markdown` fails CI under `-Dwarnings`.
- **Format before every commit:**
  `nix develop --command cargo fmt --all`

## File map

| File | Responsibility in this change |
|---|---|
| `kermit-iters/src/trie.rs` | `TrieIteratorWrapper::advance`: lending, allocation-free traversal; `Iterator::next` delegates to it |
| `kermit-algos/src/join_algo.rs` | `JoinAlgo::join_for_each` (required) and `join_iter` (provided, collects) |
| `kermit-algos/src/sorted/leapfrog_triejoin.rs` | LFTJ `join_for_each`: one scratch row, reorders each descent tuple into it |
| `kermit-algos/src/hash/hash_triejoin.rs` | HashTriejoin `join_for_each`; `LeafScratch`; allocation-free `emit_leaf` / `verify_and_construct` |
| `kermit/src/db.rs` | `run_join` takes a sink; new `lftj_join_for_each` / `hash_join_for_each`; `lftj_join` / `hash_join` collect through them |
| `kermit/tests/common/utils.rs` | `JoinEntry::count`; `test_join` asserts the streamed count |
| `kermit/tests/result_allocation.rs` (new) | Allocations during a counted join don't scale with result size, in every cell |
| `kermit/Cargo.toml`, `Cargo.lock` | `allocation-counter` dev-dependency |
| `kermit/src/execution.rs` | `ExecutionFamily::join_for_each` (required), `count` and `join` (provided) |
| `kermit/src/bench/run.rs` | `--verify`, `iteration` and `end_to_end` use `family.count` |
| `kermit/src/bench_report.rs`, `python/kermit-lab/kermit_lab/__init__.py` | Schema version 2 → 3 |
| `python/kermit-lab/kermit_lab/loader.py`, `frame.py`, `drivers/main.py` | Mixing guard, `allow_mixed_schema` keyword, CLI exit code 4 |
| `python/kermit-lab/tests/test_loader.py`, `test_frame.py`, `test_cli.py` | Guard tests |
| Docs (Task 7) | `BENCHMARKING.md`, `ARCHITECTURE.md`, `CLAUDE.md`, `docs/algorithms/{leapfrog,hash}-triejoin.md`, `docs/specs/{bench-report-schema,benchmarking-architecture}.md`, `python/kermit-lab/README.md`, `scripts/watdiv_stress_sample.py` |

---

### Task 1: `TrieIteratorWrapper::advance`

**Files:**
- Modify: `kermit-iters/src/trie.rs` (the inherent `fn next` on
  `TrieIteratorWrapper`, the `Iterator` impl, and the tests module)

- [ ] **Step 1: Write the failing test.** Append it inside
  `mod tests` in `kermit-iters/src/trie.rs`, after
  `without_arity_returns_all_depths`:

```rust
    /// `advance` lends each tuple from the wrapper's own stack; it must
    /// visit exactly the tuples `next` yields, in the same order, under an
    /// arity filter too.
    #[test]
    fn advance_lends_the_tuples_next_yields() {
        let trie = MockTrie {
            roots: vec![
                node(1, vec![
                    node(2, vec![leaf(5), leaf(6)]),
                    node(3, vec![leaf(7)]),
                ]),
                leaf(4),
            ],
        };
        let mut lent = Vec::new();
        let mut wrapper = TrieIteratorWrapper::with_arity(MockTrieIter::new(&trie), 3);
        while let Some(tuple) = wrapper.advance() {
            lent.push(tuple.to_vec());
        }
        let yielded: Vec<Vec<usize>> =
            TrieIteratorWrapper::with_arity(MockTrieIter::new(&trie), 3).collect();
        assert_eq!(lent, yielded);
        assert_eq!(lent, vec![vec![1, 2, 5], vec![1, 2, 6], vec![1, 3, 7]]);
    }
```

- [ ] **Step 2: Run it and check that it fails.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit-iters advance_lends'`
Expected: compile error, "no method named `advance` found".

- [ ] **Step 3: Implement `advance`.** In `kermit-iters/src/trie.rs`,
  replace the whole inherent method `fn next(&mut self) -> Option<Vec<usize>>`
  (its doc comment starts "Produces the next complete tuple") with:

```rust
    /// Advances to the next complete tuple and lends it: the slice is the
    /// wrapper's own path stack, valid until the next call, so the
    /// traversal allocates nothing per tuple. Callers that need an owned
    /// tuple use the [`Iterator`] impl, which copies this slice.
    ///
    /// Moves through the trie in depth-first order. Backtracks via
    /// `up`/`next_sibling` when a leaf is reached, then descends again via
    /// `down` until the next leaf. Returns `None` when the entire trie has
    /// been exhausted.
    pub fn advance(&mut self) -> Option<&[usize]> {
        loop {
            // Phase 1: Backtrack — advance to the next sibling, moving up
            // through ancestors until one has a remaining sibling.
            if !self.stack.is_empty() {
                while !self.next_sibling() {
                    if !self.up() {
                        return None;
                    }
                }
            }

            // Phase 2: Descend — greedily open children until a leaf.
            while self.down() {}

            if self.stack.is_empty() {
                return None;
            }

            // Phase 3: Filter — skip tuples shorter than the expected arity
            // (partial paths produced by joins at intermediate depths).
            if let Some(arity) = self.expected_arity {
                if self.stack.len() < arity {
                    continue;
                }
            }

            return Some(&self.stack);
        }
    }
```

  Then change the `Iterator` impl's body to:

```rust
    fn next(&mut self) -> Option<Self::Item> { self.advance().map(<[usize]>::to_vec) }
```

- [ ] **Step 4: Run the crate's tests and check that they pass.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit-iters'`
Expected: every test passes, including `advance_lends_the_tuples_next_yields`.
The existing `collect_tuples` helper calls `wrapper.next()`, which now
resolves to `Iterator::next`.

- [ ] **Step 5: Check that the dependents still build.** `#[derive(IntoTrieIter)]`
  and three `kermit-algos` files construct `TrieIteratorWrapper`.

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo build --workspace --all-targets'`
Expected: success.

- [ ] **Step 6: Commit.**

```bash
nix develop --command cargo fmt --all
git add kermit-iters/src/trie.rs
git commit -m "feat(iters): lend tuples from TrieIteratorWrapper::advance (#65)

Iterator::next now copies the slice advance lends, so callers that only
consume each tuple can traverse without allocating per tuple.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01LbYG3WiiRqrZ8zek8u7AyZ"
```

---

### Task 2: `JoinAlgo::join_for_each` in both algorithms

The trait change breaks both implementations at once, so this task makes
all three edits before anything compiles again.

**Files:**
- Modify: `kermit-algos/src/join_algo.rs`
- Modify: `kermit-algos/src/sorted/leapfrog_triejoin.rs` (`impl JoinAlgo`
  and the tests module)
- Modify: `kermit-algos/src/hash/hash_triejoin.rs` (`verify_and_construct`,
  `enumerate`, `emit_leaf`, `advance_cursor`, `impl JoinAlgo`, and the tests)

- [ ] **Step 1: Write the failing LFTJ test.** In
  `kermit-algos/src/sorted/leapfrog_triejoin.rs`, `mod tests`, extend the
  `use` block to:

```rust
    use {
        crate::{
            optimiser::{CatalogStats, LexicographicOptimiser, QueryOptimiser},
            sorted::{
                leapfrog_join::LeapfrogJoinIterator,
                leapfrog_triejoin::{LeapfrogTriejoinIter, LeapfrogTriejoinIterator},
            },
            JoinAlgo, LeapfrogTriejoin,
        },
        kermit_ds::{Relation, TreeTrie},
        kermit_iters::TrieIterable,
        kermit_parser::JoinQuery,
        std::collections::HashMap,
    };
```

  Then append this test to the module:

```rust
    /// `Q(X, Y) :- R(Y, X).` forces the descent order Y, X (R's column
    /// order) against the head order X, Y, so `join_for_each` must reorder
    /// every descent tuple into its scratch row before emitting it.
    #[test]
    fn join_for_each_emits_head_order_under_a_reordering_plan() {
        let r = TreeTrie::from_tuples(2.into(), vec![vec![1, 10], vec![2, 20]]);
        let query: JoinQuery = "Q(X, Y) :- R(Y, X).".parse().unwrap();
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        assert_eq!(plan.variable_ordering, vec![1, 0], "the plan must descend Y first");
        let mut rows = Vec::new();
        LeapfrogTriejoin::join_for_each(
            &plan,
            query,
            HashMap::from([("R".to_string(), &r)]),
            |tuple| rows.push(tuple.to_vec()),
        );
        assert_eq!(rows, vec![vec![10, 1], vec![20, 2]]);
    }
```

- [ ] **Step 2: Write the failing HashTriejoin tests.** In
  `kermit-algos/src/hash/hash_triejoin.rs`, `mod tests`:

  First, replace the three `verify_*` unit tests
  (`verify_constructs_when_shared_var_agrees`,
  `verify_rejects_when_shared_var_disagrees`, `verify_passes_no_shared_var`)
  with:

```rust
    #[test]
    fn verify_constructs_when_shared_var_agrees() {
        // R(X, Y), S(Y, Z) with X=1, Y=2, Z=3.
        let pv = vec![vec![0, 1], vec![1, 2]];
        let (mut row, mut bound) = (vec![0; 3], vec![false; 3]);
        assert!(verify_and_construct(
            [&[1, 2][..], &[2, 3][..]],
            &pv,
            &mut row,
            &mut bound
        ));
        assert_eq!(row, vec![1, 2, 3]);
    }

    #[test]
    fn verify_rejects_when_shared_var_disagrees() {
        // R(X, Y), S(Y, Z) but Y differs in R (=2) vs S (=99)
        let pv = vec![vec![0, 1], vec![1, 2]];
        let (mut row, mut bound) = (vec![0; 3], vec![false; 3]);
        assert!(!verify_and_construct(
            [&[1, 2][..], &[99, 3][..]],
            &pv,
            &mut row,
            &mut bound
        ));
    }

    #[test]
    fn verify_passes_no_shared_var() {
        // Two disjoint unary predicates R(X), S(Y).
        let pv = vec![vec![0], vec![1]];
        let (mut row, mut bound) = (vec![0; 2], vec![false; 2]);
        assert!(verify_and_construct(
            [&[1][..], &[2][..]],
            &pv,
            &mut row,
            &mut bound
        ));
        assert_eq!(row, vec![1, 2]);
    }

    /// The scratch row outlives each candidate: a rejected candidate leaves
    /// a partial write, and the next one must not mistake it for a binding.
    #[test]
    fn verify_resets_scratch_between_candidates() {
        let pv = vec![vec![0, 1], vec![1, 2]];
        let (mut row, mut bound) = (vec![0; 3], vec![false; 3]);
        assert!(!verify_and_construct(
            [&[1, 2][..], &[99, 3][..]],
            &pv,
            &mut row,
            &mut bound
        ));
        assert!(verify_and_construct(
            [&[4, 5][..], &[5, 6][..]],
            &pv,
            &mut row,
            &mut bound
        ));
        assert_eq!(row, vec![4, 5, 6]);
    }
```

  Second, in `enumerate_unary_intersection`, replace everything from the
  comment `// The result is fully materialised before any tuple is yielded;`
  down to `output.sort();` with:

```rust
        let mut scratch = LeafScratch::new(iters.len(), 1);
        let mut output = Vec::new();
        enumerate(
            0,
            1,
            &mut iters,
            &predicate_variables,
            &variable_to_iter_map,
            &mut scratch,
            &mut |row: &[usize]| output.push(row.to_vec()),
        );
        output.sort();
```

  Third, in `verify_rejects_real_inner_level_collision`, replace the last
  five statements (from `let candidate: Vec<&Vec<usize>> = …` to the end of
  the `assert_eq!`) with:

```rust
        let pv = vec![vec![0, 1], vec![1, 2]];
        let (mut row, mut bound) = (vec![0; 3], vec![false; 3]);
        assert!(
            !verify_and_construct(
                [r_chain[0].as_slice(), s_chain[0].as_slice()],
                &pv,
                &mut row,
                &mut bound
            ),
            "Y = 2 in R but 12 in S: the leaf-level check must reject it"
        );
```

  Fourth, append a streaming end-to-end test to the module:

```rust
    #[test]
    fn join_for_each_streams_every_verified_row() {
        use kermit_ds::{HashTrie, Relation};
        let r = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![2, 3], vec![3, 1]]);
        let s = HashTrie::from_tuples(2.into(), vec![vec![2, 3], vec![3, 1], vec![1, 2]]);
        let t = HashTrie::from_tuples(2.into(), vec![vec![1, 3], vec![2, 1], vec![3, 2]]);
        let query: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        let ds: HashMap<String, &HashTrie> = HashMap::from([
            ("R".to_string(), &r),
            ("S".to_string(), &s),
            ("T".to_string(), &t),
        ]);
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        let mut out = Vec::new();
        HashTriejoin::join_for_each(&plan, query, ds, |tuple| out.push(tuple.to_vec()));
        out.sort();
        assert_eq!(out, vec![vec![1, 2, 3], vec![2, 3, 1], vec![3, 1, 2]]);
    }
```

- [ ] **Step 3: Run the tests and check that they fail.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit-algos'`
Expected: compile errors: no `join_for_each`, no `LeafScratch`, and a
`verify_and_construct` signature mismatch.

- [ ] **Step 4: Change the trait.** Replace the whole `pub trait JoinAlgo`
  item in `kermit-algos/src/join_algo.rs` (keep the module doc and the `use`
  block) with:

```rust
/// The `JoinAlgo` trait is used as a base for join algorithms.
pub trait JoinAlgo<DS>
where
    DS: JoinIterable,
{
    /// Joins the given iterables according to `plan`, which must have been
    /// produced by a [`QueryOptimiser`](crate::optimiser::QueryOptimiser)
    /// for this exact `query` (after any const-view rewrite), and passes
    /// each result tuple to `emit` in canonical (head-first) variable order.
    ///
    /// # Streaming
    ///
    /// This is the method every algorithm implements, and it never
    /// materialises the result. The slice handed to `emit` is borrowed for
    /// that call only, so an implementation reuses one scratch row and
    /// allocates nothing per tuple. The `iteration` and `end_to_end`
    /// benchmark metrics time exactly this path, counting rows through a
    /// `black_box` sink (issue #65). The sink is a generic parameter rather
    /// than `&mut dyn FnMut`, so the per-tuple call is monomorphised, not
    /// an indirect call inside the timed region.
    ///
    /// # Panics
    ///
    /// Implementations panic if `plan` fails
    /// [`QueryPlan::validate`] against `query` — an invalid plan is a
    /// caller programming error, never a recoverable runtime condition.
    fn join_for_each<S: FnMut(&[usize])>(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>, emit: S,
    );

    /// Joins as [`join_for_each`](JoinAlgo::join_for_each) does and returns
    /// the materialised result. Allocates one `Vec` per tuple, so it is for
    /// callers that need the rows themselves, never for a timed region.
    ///
    /// # Panics
    ///
    /// As [`join_for_each`](JoinAlgo::join_for_each).
    fn join_iter(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>> {
        let mut tuples = Vec::new();
        Self::join_for_each(plan, query, datastructures, |tuple| {
            tuples.push(tuple.to_vec())
        });
        tuples.into_iter()
    }
}
```

- [ ] **Step 5: Implement LFTJ `join_for_each`.** In
  `kermit-algos/src/sorted/leapfrog_triejoin.rs`, replace the whole
  `impl<DS> JoinAlgo<DS> for LeapfrogTriejoin` block with:

```rust
impl<DS> JoinAlgo<DS> for LeapfrogTriejoin
where
    DS: TrieIterable,
{
    fn join_for_each<S: FnMut(&[usize])>(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>, mut emit: S,
    ) {
        let analysis = analyse(&query);
        if let Err(e) = plan.validate(&analysis) {
            panic!("LeapfrogTriejoin::join_for_each: invalid query plan: {e}");
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

        // One scratch row for the whole join: `advance` lends the descent
        // tuple and `emit` borrows the reordered one, so no result tuple is
        // ever allocated (issue #65).
        let mut descents =
            LeapfrogTriejoinIter::new(variable_ordering, predicate_variables, trie_iters)
                .into_iter();
        let mut tuple = vec![0usize; arity];
        while let Some(descent_tuple) = descents.advance() {
            for (v, slot) in tuple.iter_mut().enumerate() {
                *slot = descent_tuple[descent_pos_of_var[v]];
            }
            emit(&tuple);
        }
    }
}
```

- [ ] **Step 6: Implement the HashTriejoin changes.** In
  `kermit-algos/src/hash/hash_triejoin.rs`, make five edits.

  6a. Replace `verify_and_construct` and its doc comment with:

```rust
/// Verify one candidate result tuple's join condition, writing its output
/// tuple into `row` in canonical (head-first) variable-index order (i.e.
/// indexed by the variable indices in `predicate_variables`). Returns
/// `false` if any variable mentioned by two or more predicates has
/// inconsistent values in the candidate (a hash false positive); `row` then
/// holds a partial write and must not be emitted.
///
/// `candidate` yields one tuple per participating relation, the k-th from
/// the k-th relation's leaf chain. `predicate_variables[k]` lists the
/// variable indices carried by the k-th relation (in attribute order).
/// `row` and `bound` are per-join scratch with one slot per distinct query
/// variable. `bound` is reset on entry, so a rejected candidate leaves
/// nothing behind for the next one.
fn verify_and_construct<'t>(
    candidate: impl IntoIterator<Item = &'t [usize]>, predicate_variables: &[Vec<usize>],
    row: &mut [usize], bound: &mut [bool],
) -> bool {
    bound.fill(false);
    for (tuple, vars) in candidate.into_iter().zip(predicate_variables) {
        for (col, &var_idx) in vars.iter().enumerate() {
            let v = tuple[col];
            if !bound[var_idx] {
                row[var_idx] = v;
                bound[var_idx] = true;
            } else if row[var_idx] != v {
                return false;
            }
        }
    }
    true
}

/// Per-join scratch for the leaf cross product (Algorithm 3 lines 16–19),
/// allocated once in `join_for_each` so that emitting a result tuple
/// allocates nothing (issue #65).
struct LeafScratch {
    /// Mixed-base cursor: `cursor[k]` indexes the k-th iterator's leaf chain.
    cursor: Vec<usize>,
    /// `chain_lens[k]` is the length of the k-th iterator's current chain.
    chain_lens: Vec<usize>,
    /// The candidate's output tuple, in canonical variable order.
    row: Vec<usize>,
    /// `bound[v]` records whether `row[v]` was written for this candidate.
    bound: Vec<bool>,
}

impl LeafScratch {
    /// Scratch for `relations` body predicates over `arity` distinct
    /// variables.
    fn new(relations: usize, arity: usize) -> Self {
        Self {
            cursor: vec![0; relations],
            chain_lens: vec![0; relations],
            row: vec![0; arity],
            bound: vec![false; arity],
        }
    }
}
```

  6b. In `enumerate`, change the doc's first paragraph, the signature, the
  leaf call and the recursive call. Leave the body between them unchanged.

```rust
/// Algorithm 3 from the paper. Recursively descends through attribute
/// positions; passes every verified result tuple to `emit`.
```

```rust
fn enumerate<IT: HashTrieIterator, S: FnMut(&[usize])>(
    i: usize, arity: usize, iters: &mut [IT], predicate_variables: &[Vec<usize>],
    variable_to_iter_map: &[Vec<usize>], scratch: &mut LeafScratch, emit: &mut S,
) {
    if i == arity {
        emit_leaf(iters, predicate_variables, scratch, emit);
        return;
    }
```

```rust
            if all_match {
                enumerate(
                    i + 1,
                    arity,
                    iters,
                    predicate_variables,
                    variable_to_iter_map,
                    scratch,
                    emit,
                );
            }
```

  6c. Replace `emit_leaf` and `advance_cursor` (with their doc comments)
  with:

```rust
/// Algorithm 3 lines 16–19. Cross-product the leaf chains of every
/// iterator and emit each verified candidate.
///
/// Allocates nothing: each chain is re-borrowed through `leaf_tuples()`
/// rather than collected, and the cursor and output row live in `scratch`.
fn emit_leaf<IT: HashTrieIterator, S: FnMut(&[usize])>(
    iters: &[IT], predicate_variables: &[Vec<usize>], scratch: &mut LeafScratch, emit: &mut S,
) {
    let chain = |k: usize| {
        iters[k]
            .leaf_tuples()
            .expect("at leaf level for every participating iter")
    };
    for (k, len) in scratch.chain_lens.iter_mut().enumerate() {
        *len = chain(k).len();
    }
    if scratch.chain_lens.contains(&0) {
        return;
    }

    scratch.cursor.fill(0);
    loop {
        let candidate = scratch
            .cursor
            .iter()
            .enumerate()
            .map(|(k, &i)| chain(k)[i].as_slice());
        if verify_and_construct(
            candidate,
            predicate_variables,
            &mut scratch.row,
            &mut scratch.bound,
        ) {
            emit(&scratch.row);
        }
        if !advance_cursor(&mut scratch.cursor, &scratch.chain_lens) {
            break;
        }
    }
}

/// Advance a mixed-base cursor over the chain lengths. Returns `false`
/// once every position has been exhausted (overflow off the high end).
fn advance_cursor(cursor: &mut [usize], chain_lens: &[usize]) -> bool {
    let mut k = 0;
    loop {
        if k >= cursor.len() {
            return false;
        }
        cursor[k] += 1;
        if cursor[k] < chain_lens[k] {
            return true;
        }
        cursor[k] = 0;
        k += 1;
    }
}
```

  6d. Replace the whole `impl<DS> JoinAlgo<DS> for HashTriejoin` block
  with:

```rust
impl<DS> JoinAlgo<DS> for HashTriejoin
where
    DS: HashTrieIterable,
{
    fn join_for_each<S: FnMut(&[usize])>(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>, mut emit: S,
    ) {
        let analysis = analyse(&query);
        if let Err(e) = plan.validate(&analysis) {
            panic!("HashTriejoin::join_for_each: invalid query plan: {e}");
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
        let mut scratch = LeafScratch::new(iters.len(), variable_ordering.len());
        enumerate(
            0,
            variable_ordering.len(),
            &mut iters,
            &predicate_variables,
            &variable_to_iter_map,
            &mut scratch,
            &mut emit,
        );
    }
}
```

  6e. Check whether the collision-section comment in `mod tests` still
  reads right. It ends "…`emit_leaf` cross product, and only then
  `verify_and_construct`", which is still true, so leave it.

- [ ] **Step 7: Run the crate's tests and check that they pass.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit-algos'`
Expected: every test passes, including
`join_for_each_emits_head_order_under_a_reordering_plan`,
`verify_resets_scratch_between_candidates`,
`join_for_each_streams_every_verified_row`, and the collision tests, which
still go through the provided `join_iter`.

- [ ] **Step 8: Run the workspace build and the join suite.** `kermit`'s
  `compute_join` and `db::run_join` call `join_iter`, which still exists.

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests'`
Expected: all join-suite tests pass.

- [ ] **Step 9: Run clippy on the crate.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-algos --all-targets'`
Expected: no warnings. If `too_many_arguments` fires on `enumerate` (it has
7, the lint's limit), report it rather than suppressing it.

- [ ] **Step 10: Commit.**

```bash
nix develop --command cargo fmt --all
git add kermit-algos/src/join_algo.rs kermit-algos/src/sorted/leapfrog_triejoin.rs kermit-algos/src/hash/hash_triejoin.rs
git commit -m "feat(algos): stream join results through JoinAlgo::join_for_each (#65)

join_for_each passes each tuple to a generic sink as a borrowed slice and
is the method both algorithms implement; join_iter is now a provided
method that collects through it. LFTJ reorders each lent descent tuple
into one scratch row. HashTriejoin threads the sink through Algorithm 3
and its leaf cross product writes into per-join scratch, so neither
algorithm allocates per result tuple.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01LbYG3WiiRqrZ8zek8u7AyZ"
```

---

### Task 3: `lftj_join_for_each` / `hash_join_for_each`, and the suite's count check

**Files:**
- Modify: `kermit/src/db.rs` (`run_join`, `lftj_join`, `hash_join`, both
  test modules)
- Modify: `kermit/tests/common/utils.rs`

- [ ] **Step 1: Write the failing db tests.** In `kermit/src/db.rs`, append
  to `mod tests`:

```rust
    /// The streaming entry point visits every row without collecting;
    /// `lftj_join` is the same traversal, collected.
    #[test]
    fn lftj_join_for_each_visits_every_row() {
        let relations = rels(vec![
            ("first", 1, vec![vec![1], vec![2], vec![3]]),
            ("second", 1, vec![vec![2], vec![3], vec![4]]),
        ]);
        let query: JoinQuery = "Q(X) :- first(X), second(X).".parse().unwrap();
        let mut got = Vec::new();
        lftj_join_for_each::<TreeTrie, LeapfrogTriejoin>(
            &relations,
            query,
            &LexicographicOptimiser,
            |row| got.push(row[0]),
        );
        got.sort();
        assert_eq!(got, vec![2, 3]);
    }
```

  and append to `mod hash_join_tests`:

```rust
    #[test]
    fn hash_join_for_each_visits_every_row() {
        let mut relations: BTreeMap<String, HashTrie> = BTreeMap::new();
        relations.insert(
            "R".to_string(),
            HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]),
        );
        relations.insert(
            "S".to_string(),
            HashTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![4]]),
        );
        let q: JoinQuery = "Q(X) :- R(X), S(X).".parse().unwrap();
        let mut got = Vec::new();
        hash_join_for_each::<HashTrie<SipHashStrategy>, SipHashStrategy>(
            &relations,
            q,
            &LexicographicOptimiser,
            |row| got.push(row[0]),
        );
        got.sort();
        assert_eq!(got, vec![2, 3]);
    }
```

- [ ] **Step 2: Run the tests and check that they fail.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit --lib db::'`
Expected: compile error, "cannot find function `lftj_join_for_each`".

- [ ] **Step 3: Implement.** In `kermit/src/db.rs`:

  3a. Change `run_join`'s signature and tail. Insert a paragraph into its
  doc comment, after "statistics, plan, execute.":

```rust
/// Each result tuple is passed to `emit` as a borrowed slice; the result
/// is never materialised here.
```

```rust
fn run_join<'a, R, F, JA, S>(
    relations: &'a BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    label: &str, emit: S,
) where
    R: Cardinality + 'a,
    F: JoinFamily<R>,
    JA: JoinAlgo<F::Wrapper<'a>>,
    S: FnMut(&[usize]),
{
```

  and change its last line from `JA::join_iter(&plan, rewritten, ds_map).collect()` to:

```rust
    JA::join_for_each(&plan, rewritten, ds_map, emit);
```

  3b. Replace `lftj_join` and `hash_join` (with their doc comments) with
  these four functions:

```rust
/// Sorted-family join entry point: runs `query` over `relations` with the
/// [`TrieIterable`]-family algorithm `JA` (normally
/// [`LeapfrogTriejoin`](kermit_algos::LeapfrogTriejoin)), planned by
/// `optimiser`, and passes each result tuple to `emit` without
/// materialising the result. Mirror of [`hash_join_for_each`].
///
/// # Panics
///
/// Panics if the query references a relation name not present in
/// `relations`, or if it contains a malformed constant atom.
pub fn lftj_join_for_each<R, JA>(
    relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    emit: impl FnMut(&[usize]),
) where
    R: TrieIterable + Cardinality,
    JA: for<'a> JoinAlgo<TrieIterKind<'a, R>>,
{
    run_join::<R, SortedFamily, JA, _>(relations, query, optimiser, "lftj_join", emit);
}

/// [`lftj_join_for_each`], collected: returns every result tuple. For
/// callers that need the rows themselves; allocates one `Vec` per tuple.
///
/// # Panics
///
/// As [`lftj_join_for_each`].
pub fn lftj_join<R, JA>(
    relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
) -> Vec<Vec<usize>>
where
    R: TrieIterable + Cardinality,
    JA: for<'a> JoinAlgo<TrieIterKind<'a, R>>,
{
    let mut tuples = Vec::new();
    lftj_join_for_each::<R, JA>(relations, query, optimiser, |tuple| {
        tuples.push(tuple.to_vec())
    });
    tuples
}

/// Hash-family join entry point: runs `query` over `relations` with
/// [`HashTriejoin`], planned by `optimiser`, and passes each result tuple
/// to `emit` without materialising the result. Mirror of
/// [`lftj_join_for_each`].
///
/// `H` selects the hash function used for any constant-atom singletons;
/// callers must thread the same `H` used when constructing the
/// `HashTrie<H>` relations so the algorithm sees matching hashes on both
/// sides of the intersection. Rust forbids defaults on free-function type
/// parameters (see issue #36887), so callers specify it via turbofish.
///
/// # Panics
///
/// Panics if the query references a relation name not present in
/// `relations`, or if it contains a malformed constant atom.
pub fn hash_join_for_each<R, H>(
    relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    emit: impl FnMut(&[usize]),
) where
    R: HashTrieIterable + Cardinality,
    H: HashStrategy,
{
    run_join::<R, HashFamily<H>, HashTriejoin, _>(relations, query, optimiser, "hash_join", emit);
}

/// [`hash_join_for_each`], collected: returns every result tuple. For
/// callers that need the rows themselves; allocates one `Vec` per tuple.
///
/// # Panics
///
/// As [`hash_join_for_each`].
pub fn hash_join<R, H>(
    relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
) -> Vec<Vec<usize>>
where
    R: HashTrieIterable + Cardinality,
    H: HashStrategy,
{
    let mut tuples = Vec::new();
    hash_join_for_each::<R, H>(relations, query, optimiser, |tuple| {
        tuples.push(tuple.to_vec())
    });
    tuples
}
```

  3c. Update the module doc's first paragraph so it names both forms.
  Replace "Each iterator family has one free function — [`lftj_join`] for
  sorted tries under a [`TrieIterable`]-family algorithm, [`hash_join`] for
  [`HashTrieIterable`] structures under [`HashTriejoin`] — over the same"
  with:

```rust
//! Each iterator family has one streaming free function —
//! [`lftj_join_for_each`] for sorted tries under a [`TrieIterable`]-family
//! algorithm, [`hash_join_for_each`] for [`HashTrieIterable`] structures
//! under [`HashTriejoin`] — plus a collecting wrapper ([`lftj_join`],
//! [`hash_join`]), all over the same
```

- [ ] **Step 4: Run the db tests and check that they pass.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit --lib db::'`
Expected: every `db::` test passes, including
`test_join_panics_on_missing_relation`, whose `"unknown relation"` message
is unchanged.

- [ ] **Step 5: Add the count to the suite driver.** In
  `kermit/tests/common/utils.rs`:

  Change the import to
  `kermit::db::{hash_join, hash_join_for_each, lftj_join, lftj_join_for_each},`.

  Give the `JoinEntry` trait a second method:

```rust
/// The entry point in `kermit::db` that runs algorithm `Self` over `R`.
pub trait JoinEntry<R> {
    fn join(
        relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> Vec<Vec<usize>>;

    /// Counts the result through the streaming `_for_each` entry point —
    /// the path `bench run`'s `iteration` metric times.
    fn count(
        relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> usize;
}
```

  Add a `count` to each of the three impls:

```rust
    // impl<R: TrieIterable + Cardinality> JoinEntry<R> for LeapfrogTriejoin
    fn count(
        relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    ) -> usize {
        let mut rows = 0;
        lftj_join_for_each::<R, LeapfrogTriejoin>(relations, query, optimiser, |_| rows += 1);
        rows
    }
```

```rust
    // impl<H, P> JoinEntry<HashTrie<H, P>> for HashTriejoin
    fn count(
        relations: &BTreeMap<String, HashTrie<H, P>>, query: JoinQuery,
        optimiser: &dyn QueryOptimiser,
    ) -> usize {
        let mut rows = 0;
        hash_join_for_each::<HashTrie<H, P>, H>(relations, query, optimiser, |_| rows += 1);
        rows
    }
```

```rust
    // impl<H, P, C> JoinEntry<Configured<HashTrie<H, P>, C>> for HashTriejoin
    fn count(
        relations: &BTreeMap<String, Configured<HashTrie<H, P>, C>>, query: JoinQuery,
        optimiser: &dyn QueryOptimiser,
    ) -> usize {
        let mut rows = 0;
        hash_join_for_each::<Configured<HashTrie<H, P>, C>, H>(
            relations,
            query,
            optimiser,
            |_| rows += 1,
        );
        rows
    }
```

  In `test_join`, insert this just before the
  `let mut actual: Vec<Vec<usize>> = JA::join(…)` statement:

```rust
    // The streamed count is what `bench run --verify` checks and what the
    // `iteration` metric times; it must agree with the expected rows.
    let streamed = JA::count(&relations, query.clone(), &O::default());
    assert_eq!(
        streamed,
        result.len(),
        "streamed count disagrees with the expected row count"
    );
```

  and extend the doc comment on `test_join`, after its first paragraph:

```rust
/// It also counts the result through the streaming entry point and checks
/// that count against `result`, so the path `bench run` times is covered
/// for every structure × algorithm × optimiser invocation.
```

- [ ] **Step 6: Run the join suites and check that they pass.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests'`
Expected: all invocations pass, every Layout alias and both optimisers
included. If another file under `kermit/tests/` uses `mod common;` and fails
to compile, it is using `JoinEntry` and needs the same `count`; fix it there.

- [ ] **Step 7: Commit.**

```bash
nix develop --command cargo fmt --all
git add kermit/src/db.rs kermit/tests/common/utils.rs
git commit -m "feat(db): streaming join entry points; suite checks the count (#65)

lftj_join_for_each / hash_join_for_each pass each tuple to a sink;
lftj_join / hash_join now collect through them, so every existing caller
runs the streaming path. The standard suite also counts each result
through the streaming entry point.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01LbYG3WiiRqrZ8zek8u7AyZ"
```

---

### Task 4: Result-allocation test

**Files:**
- Modify: `kermit/Cargo.toml` (`[dev-dependencies]`), `Cargo.lock`
- Create: `kermit/tests/result_allocation.rs`

- [ ] **Step 1: Add the dev-dependency.**

Run: `nix develop --command bash -c 'cargo add --dev -p kermit allocation-counter@0.8'`
Expected: `kermit/Cargo.toml`'s `[dev-dependencies]` gains
`allocation-counter = "0.8"` (keep the section alphabetical by hand if
`cargo add` didn't), and `Cargo.lock` changes. If cargo also re-locks
unrelated packages, restore `Cargo.lock` with
`git checkout -- Cargo.lock` and re-run `cargo add` with `--offline`: the
crate is already in `~/.cargo/registry`.

- [ ] **Step 2: Write the test.** Create
  `kermit/tests/result_allocation.rs`:

```rust
//! Issue #65: counting a join's result allocates nothing per result row.
//!
//! `bench run`'s `iteration` metric times a join whose rows are counted
//! through a `black_box` sink. If that path allocated per row, the timed
//! region would again include result materialisation, and memory would
//! again grow with the result. Each test here counts the calling thread's
//! allocations (via `allocation-counter`, which installs the counting global
//! allocator for this test binary only) during one streamed join at two
//! result sizes 1000x apart, and requires the two counts to be equal.
//!
//! **What this isolates.** The query is `Q(X, Y, Z) :- R(X, Y), S(X, Z).`
//! with `R` fixed at 10 tuples, and only `S`'s fan-out per `X` grows. The
//! number of trie descents is therefore the same at both sizes, so
//! per-descent allocation — LFTJ's `update_iters` builds a `Vec` on every
//! `open`/`up`, which is algorithm cost, not materialisation — cancels out.
//! The only term that could differ is one that scales with the result. The
//! shared `X` makes HashTriejoin verify a real join condition at the leaf.

use {
    kermit::db::{hash_join_for_each, lftj_join_for_each},
    kermit_algos::{JoinQuery, LeapfrogTriejoin, LexicographicOptimiser},
    kermit_ds::{
        Cardinality, ColumnTrie, HashTrie, NoPruning, PruningPolicy, Relation, SingletonPruning,
        TreeTrie,
    },
    kermit_iters::{FxHashStrategy, HashStrategy, SipHashStrategy, TrieIterable},
    std::collections::BTreeMap,
};

const QUERY: &str = "Q(X, Y, Z) :- R(X, Y), S(X, Z).";

/// Distinct `X` values; fixed, so the number of descents is fixed.
const XS: usize = 10;

/// `S` fan-out per `X` for the small (100-row) and large (100,000-row)
/// results.
const SMALL: usize = 10;
const LARGE: usize = 10_000;

/// `R = {(x, 0)}` and `S = {(x, z) : z < fan_out}` for each of the `XS`
/// keys, so the join has `XS * fan_out` rows.
fn relations<Rel: Relation>(fan_out: usize) -> BTreeMap<String, Rel> {
    let r: Vec<Vec<usize>> = (0..XS).map(|x| vec![x, 0]).collect();
    let s: Vec<Vec<usize>> = (0..XS)
        .flat_map(|x| (0..fan_out).map(move |z| vec![x, z]))
        .collect();
    BTreeMap::from([
        ("R".to_string(), Rel::from_tuples(2.into(), r)),
        ("S".to_string(), Rel::from_tuples(2.into(), s)),
    ])
}

/// Allocations made by one streamed LFTJ join, after checking that it
/// produced every row.
fn lftj_allocations<Rel: TrieIterable + Cardinality + Relation>(fan_out: usize) -> u64 {
    let relations = relations::<Rel>(fan_out);
    let query: JoinQuery = QUERY.parse().unwrap();
    let mut rows = 0usize;
    let info = allocation_counter::measure(|| {
        lftj_join_for_each::<Rel, LeapfrogTriejoin>(
            &relations,
            query,
            &LexicographicOptimiser,
            |tuple| {
                std::hint::black_box(tuple);
                rows += 1;
            },
        );
    });
    assert_eq!(rows, XS * fan_out, "the join must produce every row");
    info.count_total
}

/// Allocations made by one streamed HashTriejoin join over
/// `HashTrie<H, P>`, after checking that it produced every row.
fn htj_allocations<H: HashStrategy, P: PruningPolicy>(fan_out: usize) -> u64 {
    let relations = relations::<HashTrie<H, P>>(fan_out);
    let query: JoinQuery = QUERY.parse().unwrap();
    let mut rows = 0usize;
    let info = allocation_counter::measure(|| {
        hash_join_for_each::<HashTrie<H, P>, H>(
            &relations,
            query,
            &LexicographicOptimiser,
            |tuple| {
                std::hint::black_box(tuple);
                rows += 1;
            },
        );
    });
    assert_eq!(rows, XS * fan_out, "the join must produce every row");
    info.count_total
}

fn assert_flat(cell: &str, small: u64, large: u64) {
    assert_eq!(
        small, large,
        "{cell}: a 1000x larger result changed the allocation count from {small} to {large}; \
         the streamed join is allocating per result row"
    );
}

#[test]
fn tree_trie_lftj_allocates_independently_of_result_size() {
    assert_flat(
        "TreeTrie/LFTJ",
        lftj_allocations::<TreeTrie>(SMALL),
        lftj_allocations::<TreeTrie>(LARGE),
    );
}

#[test]
fn column_trie_lftj_allocates_independently_of_result_size() {
    assert_flat(
        "ColumnTrie/LFTJ",
        lftj_allocations::<ColumnTrie>(SMALL),
        lftj_allocations::<ColumnTrie>(LARGE),
    );
}

#[test]
fn hash_trie_sip_allocates_independently_of_result_size() {
    assert_flat(
        "HashTrie<Sip>/HTJ",
        htj_allocations::<SipHashStrategy, NoPruning>(SMALL),
        htj_allocations::<SipHashStrategy, NoPruning>(LARGE),
    );
}

#[test]
fn hash_trie_fx_allocates_independently_of_result_size() {
    assert_flat(
        "HashTrie<Fx>/HTJ",
        htj_allocations::<FxHashStrategy, NoPruning>(SMALL),
        htj_allocations::<FxHashStrategy, NoPruning>(LARGE),
    );
}

#[test]
fn hash_trie_sip_pruned_allocates_independently_of_result_size() {
    assert_flat(
        "HashTrie<Sip, Pruned>/HTJ",
        htj_allocations::<SipHashStrategy, SingletonPruning>(SMALL),
        htj_allocations::<SipHashStrategy, SingletonPruning>(LARGE),
    );
}

#[test]
fn hash_trie_fx_pruned_allocates_independently_of_result_size() {
    assert_flat(
        "HashTrie<Fx, Pruned>/HTJ",
        htj_allocations::<FxHashStrategy, SingletonPruning>(SMALL),
        htj_allocations::<FxHashStrategy, SingletonPruning>(LARGE),
    );
}
```

- [ ] **Step 3: Run it.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit --test result_allocation'`
Expected: all 6 tests pass.
- If a cell fails, read the two counts in the message. A difference
  proportional to the row delta (about 99,900 or a multiple) is a real
  per-row allocation, so find it before going on.
- A small constant difference means an allocation that scales with
  something else, such as a `Vec` that grows with fan-out. Investigate it
  with `superpowers:systematic-debugging`. Do not loosen the assertion.

- [ ] **Step 4: Commit the test before mutation-checking it.** The mutation
  check restores files with `git checkout`, which is only safe once the real
  code is committed.

```bash
nix develop --command cargo fmt --all
git add kermit/Cargo.toml Cargo.lock kermit/tests/result_allocation.rs
git commit -m "test(kermit): streamed join allocations don't scale with result size (#65)

Counts the calling thread's allocations (allocation-counter, dev-only)
during one streamed join at 100 and 100,000 rows in every cell and
requires them equal. Only the leaf fan-out varies, so LFTJ's per-descent
allocations cancel out and only a per-row term could differ.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01LbYG3WiiRqrZ8zek8u7AyZ"
```

- [ ] **Step 5: Mutation-check LFTJ.** Make the LFTJ sink allocate per row,
  confirm that the edit applied, then run the test.

```bash
sed -i 's/            emit(&tuple);/            emit(\&tuple.clone());/' kermit-algos/src/sorted/leapfrog_triejoin.rs
grep -n 'emit(&tuple.clone());' kermit-algos/src/sorted/leapfrog_triejoin.rs   # MUST print one line; if not, the mutation did not apply — stop
nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit --test result_allocation'
```

Expected: `tree_trie_…` and `column_trie_…` FAIL, with a difference of
about 99,900. The four hash tests pass. Then restore and confirm:

```bash
git checkout -- kermit-algos/src/sorted/leapfrog_triejoin.rs
git diff --quiet && echo clean
```

- [ ] **Step 6: Mutation-check HashTriejoin** the same way:

```bash
sed -i 's/            emit(&scratch.row);/            emit(\&scratch.row.clone());/' kermit-algos/src/hash/hash_triejoin.rs
grep -n 'emit(&scratch.row.clone());' kermit-algos/src/hash/hash_triejoin.rs   # MUST print one line
nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit --test result_allocation'
```

Expected: the four `hash_trie_…` tests FAIL and the two LFTJ tests pass.
Then restore:

```bash
git checkout -- kermit-algos/src/hash/hash_triejoin.rs
git diff --quiet && echo clean
```

  Record both mutation results, with the counts the failures printed, for
  the hand-off report.

---

### Task 5: `ExecutionFamily::count`, and `bench run` uses it

**Files:**
- Modify: `kermit/src/execution.rs` (imports, the `ExecutionFamily`
  trait, both `impl ExecutionFamily` blocks, doc links, `mod tests`)
- Modify: `kermit/src/bench/run.rs` (the `--verify` gate, the `iteration`
  closure and the `end_to_end` body inside `run_benchmark`)

- [ ] **Step 1: Write the failing test.** In `kermit/src/execution.rs`,
  `mod tests`, add `kermit_iters::SipHashStrategy` to the `use` block, then
  append:

```rust
    /// `count` (what `bench run` times and `--verify` checks) and `join`
    /// (what `kermit join` writes) are one traversal: they must agree in
    /// every family.
    #[test]
    fn count_agrees_with_join_in_every_family() {
        // Triangles in this graph: (1, 2, 3) and (2, 3, 4).
        let edges = vec![vec![1, 2], vec![2, 3], vec![1, 3], vec![3, 4], vec![2, 4]];
        let inputs = || vec![(RelationHeader::new_positional("edge", 2), edges.clone())];
        let query: JoinQuery = "Q(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z)."
            .parse()
            .unwrap();

        let tree = TrieLftj::<TreeTrie>::new(Optimiser::Lexicographic);
        let engine = tree.build_from_tuples(inputs());
        assert_eq!(tree.count(&engine, query.clone()), 2);
        assert_eq!(tree.join(&engine, query.clone()).len(), 2);

        let column = TrieLftj::<ColumnTrie>::new(Optimiser::Lexicographic);
        let engine = column.build_from_tuples(inputs());
        assert_eq!(column.count(&engine, query.clone()), 2);
        assert_eq!(column.join(&engine, query.clone()).len(), 2);

        let hash = HashHtj::<SipHashStrategy, NoPruning>::new(
            HashTrieConfig::default(),
            Optimiser::Lexicographic,
        );
        let engine = hash.build_from_tuples(inputs());
        assert_eq!(hash.count(&engine, query.clone()), 2);
        assert_eq!(hash.join(&engine, query).len(), 2);
    }
```

- [ ] **Step 2: Run it and check that it fails.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit count_agrees'`
Expected: compile error, "no method named `count`".

- [ ] **Step 3: Change the trait.** In the `ExecutionFamily` trait,
  replace the `join` method and its doc comment (`/// Runs `query` against
  `engine`.`) with:

```rust
    /// Runs `query` against `engine`, passing each result tuple to `emit`
    /// as a borrowed slice; the result is never materialised.
    fn join_for_each<S: FnMut(&[usize])>(&self, engine: &Self::Engine, query: JoinQuery, emit: S);

    /// Runs `query` against `engine` and counts its result tuples, passing
    /// each through [`std::hint::black_box`] so the traversal cannot be
    /// optimised away. Allocates nothing per tuple: this is what the
    /// `iteration` and `end_to_end` metrics time and what `--verify`
    /// checks (issue #65).
    fn count(&self, engine: &Self::Engine, query: JoinQuery) -> u64 {
        let mut rows = 0u64;
        self.join_for_each(engine, query, |tuple| {
            std::hint::black_box(tuple);
            rows += 1;
        });
        rows
    }

    /// Runs `query` against `engine` and returns every result tuple. Only
    /// for callers that need the rows themselves (`kermit join`,
    /// `bench join --output`); never inside a timed region.
    fn join(&self, engine: &Self::Engine, query: JoinQuery) -> Vec<Vec<usize>> {
        let mut tuples = Vec::new();
        self.join_for_each(engine, query, |tuple| tuples.push(tuple.to_vec()));
        tuples
    }
```

- [ ] **Step 4: Implement it for both families.** In
  `impl<R: SortedTrieRelation + 'static> ExecutionFamily for TrieLftj<R>`,
  replace `fn join(…)` with:

```rust
    fn join_for_each<S: FnMut(&[usize])>(
        &self, engine: &Self::Engine, query: JoinQuery, emit: S,
    ) {
        lftj_join_for_each::<R, LeapfrogTriejoin>(engine, query, self.optimiser.as_ref(), emit);
    }
```

  In `impl<H: HashStrategy + 'static, P: PruningPolicy> ExecutionFamily for HashHtj<H, P>`,
  replace `fn join(…)` with:

```rust
    fn join_for_each<S: FnMut(&[usize])>(
        &self, engine: &Self::Engine, query: JoinQuery, emit: S,
    ) {
        hash_join_for_each::<HashTrie<H, P>, H>(engine, query, self.optimiser.as_ref(), emit);
    }
```

  Change the import `kermit::db::{hash_join, lftj_join},` to
  `kermit::db::{hash_join_for_each, lftj_join_for_each},`. Then retarget
  the intra-doc links, which resolved through that import:

```bash
sed -i 's/\[`lftj_join`\]/[`lftj_join_for_each`]/g; s/\[`hash_join`\]/[`hash_join_for_each`]/g' kermit/src/execution.rs
grep -n 'lftj_join\b\|hash_join\b' kermit/src/execution.rs
```

  The second command lists what's left. Unbracketed mentions in the
  module's history paragraph (for example, "let `-i all -a leapfrog-triejoin`
  run `hash_join`") describe the past, so leave them. Then read each
  retargeted sentence and check that it still reads correctly.

- [ ] **Step 5: Run the test and check that it passes.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit count_agrees'`
Expected: PASS.

- [ ] **Step 6: Point `bench run` at `count`.** In
  `kermit/src/bench/run.rs`, `run_benchmark`:

  6a. In the `--verify` gate, extend the comment that begins
  "Correctness gate: with `--verify`, run the query once (untimed)" with
  "— counted through the same streaming path the `iteration` metric times,
  so verifying a huge result never materialises it". Change the count line
  to:

```rust
                    let actual = family.count(&engine, query_def.query.clone());
```

  6b. Replace the `iteration` block with:

```rust
            if metrics.contains(&Metric::Iteration) {
                // Times the join with its rows counted, never collected: the
                // timed region holds no per-row allocation, and a batch keeps
                // only `u64`s alive, so memory is independent of result size
                // (issue #65).
                group.bench_function("iteration", |b| {
                    b.iter_batched(
                        || query_def.query.clone(),
                        |q| family.count(&engine, q),
                        criterion::BatchSize::SmallInput,
                    );
                });
```

  Keep the `criterion_groups.push(…)` after it unchanged.

  6c. In the `end_to_end` body, change
  `std::hint::black_box(family.join(&fresh, q));` to:

```rust
                                std::hint::black_box(family.count(&fresh, q));
```

- [ ] **Step 7: Run the binary's tests and the `--verify` CLI tests.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit && CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_bench_run_verify --test cli_bench_run_sweep'`
Expected: all pass. The `--verify` tests compare counts end to end through
the CLI.

- [ ] **Step 8: Commit.**

```bash
nix develop --command cargo fmt --all
git add kermit/src/execution.rs kermit/src/bench/run.rs
git commit -m "feat(bench): iteration, end_to_end and --verify count rows, never collect (#65)

ExecutionFamily gains join_for_each (required) plus provided count,
which black_boxes each tuple, and join, kept for the CSV output paths.
bench run's timed regions and the --verify gate now go through count,
so they hold no per-row allocation and memory no longer grows with the
result.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01LbYG3WiiRqrZ8zek8u7AyZ"
```

---

### Task 6: Schema version 3, and kermit-lab's mixing guard

**Files:**
- Modify: `kermit/src/bench_report.rs:79-81`
- Modify: `python/kermit-lab/kermit_lab/__init__.py:8`
- Modify: `python/kermit-lab/kermit_lab/loader.py`
- Modify: `python/kermit-lab/kermit_lab/frame.py` (`load`, `load_samples`)
- Modify: `python/kermit-lab/kermit_lab/drivers/main.py`
- Test: `python/kermit-lab/tests/test_loader.py`, `tests/test_frame.py`,
  `tests/test_cli.py`

Run Python commands from `python/kermit-lab/` inside `nix develop`. The
shell sets `LD_LIBRARY_PATH` for the numpy and matplotlib wheels:
`nix develop --command bash -c 'cd python/kermit-lab && uv run pytest …'`.

- [ ] **Step 1: Write the failing loader tests.** Append to
  `python/kermit-lab/tests/test_loader.py`:

```python
def _versioned_report(path: Path, version: int) -> Path:
    path.write_text(
        json.dumps(
            [
                {
                    "schema_version": version,
                    "kind": "run",
                    "metadata": [],
                    "axes": {},
                    "criterion_groups": [],
                }
            ]
        )
    )
    return path


def test_refuses_to_mix_reports_across_the_streamed_join_boundary(tmp_path: Path) -> None:
    old = _versioned_report(tmp_path / "old.json", 2)
    new = _versioned_report(tmp_path / "new.json", 3)
    with pytest.raises(SchemaError, match="refusing to mix schema_version 2"):
        load_reports([old, new])
    with pytest.raises(SchemaError, match="old.json"):
        load_reports([new, old])


def test_allow_mixed_schema_loads_both_sides(tmp_path: Path) -> None:
    old = _versioned_report(tmp_path / "old.json", 2)
    new = _versioned_report(tmp_path / "new.json", 3)
    reports = load_reports([old, new], allow_mixed_schema=True)
    assert [r.schema_version for r in reports] == [2, 3]


def test_single_sided_loads_are_unaffected(tmp_path: Path) -> None:
    v2 = [_versioned_report(tmp_path / f"a{i}.json", 2) for i in range(2)]
    v3 = [_versioned_report(tmp_path / f"b{i}.json", 3) for i in range(2)]
    assert len(load_reports(v2)) == 2
    assert len(load_reports(v3)) == 2
```

  Append to `python/kermit-lab/tests/test_frame.py`, adding any of these
  imports the file lacks (`json`, `Path`, `pytest`, `kermit_lab as kl`,
  `from kermit_lab.loader import SchemaError`):

```python
def _bare_report(path: Path, version: int) -> Path:
    path.write_text(json.dumps([{"schema_version": version, "kind": "run",
                                 "metadata": [], "axes": {}, "criterion_groups": []}]))
    return path


def test_load_refuses_mixed_schema_and_passes_the_escape_hatch(tmp_path: Path) -> None:
    paths = [_bare_report(tmp_path / "v2.json", 2), _bare_report(tmp_path / "v3.json", 3)]
    with pytest.raises(SchemaError, match="refusing to mix"):
        kl.load(paths, criterion_root=tmp_path)
    with pytest.raises(SchemaError, match="refusing to mix"):
        kl.load_samples(paths, criterion_root=tmp_path)
    assert len(kl.load_samples(paths, criterion_root=tmp_path, allow_mixed_schema=True)) == 0
```

  Append to `python/kermit-lab/tests/test_cli.py` (add `import json` at
  the top):

```python
def test_mixed_schema_reports_exit_with_code_4(tmp_path: Path) -> None:
    paths = []
    for version in (2, 3):
        p = tmp_path / f"v{version}.json"
        p.write_text(json.dumps([{"schema_version": version, "kind": "run",
                                  "metadata": [], "axes": {}, "criterion_groups": []}]))
        paths.append(str(p))
    rc = main(["scaling", *paths, "--criterion-root", str(tmp_path),
               "--out", str(tmp_path / "s.pdf")])
    assert rc == 4
```

- [ ] **Step 2: Run them and check that they fail.**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv sync --group test && uv run pytest tests/test_loader.py tests/test_frame.py tests/test_cli.py -q'`
Expected: the new tests fail: `SchemaError` is not raised, and the
`allow_mixed_schema` keyword is unexpected. Existing tests pass.

- [ ] **Step 3: Implement the guard in `loader.py`.**
  - Change `from typing import Any, Iterable, Iterator` to
    `from typing import Any, Iterable, Iterator, Sequence`.
  - Change `SchemaError`'s docstring to
    `"""Report's ``schema_version`` is missing, unsupported, or mixed across an incompatible boundary."""`.
  - Insert after the `SchemaError` class:

```python
STREAMED_JOIN_SCHEMA = 3
"""First schema version whose ``iteration`` / ``end_to_end`` phases time a
streamed join with counted, never-materialised rows (issue #65). Reports on
either side of it measure different things, so one load may not mix them."""
```

  Replace `load_reports` with the version below, and add
  `_refuse_mixed_schema` above it:

```python
def _refuse_mixed_schema(reports: Sequence[BenchReport]) -> None:
    older = next((r for r in reports if r.schema_version < STREAMED_JOIN_SCHEMA), None)
    newer = next((r for r in reports if r.schema_version >= STREAMED_JOIN_SCHEMA), None)
    if older is not None and newer is not None:
        raise SchemaError(
            f"refusing to mix schema_version {older.schema_version} ({older.source_path}) "
            f"with schema_version {newer.schema_version} ({newer.source_path}): from "
            f"v{STREAMED_JOIN_SCHEMA} the iteration and end_to_end phases time a streamed, "
            "counted join, so their values are not comparable with earlier reports. Load "
            "each side separately, or pass allow_mixed_schema=True to compare space only."
        )


def load_reports(
    paths: Iterable[Path], *, allow_mixed_schema: bool = False
) -> list[BenchReport]:
    """Load all reports from one or more JSON files; flattens the array shape.

    Raises :class:`SchemaError` when the reports straddle
    :data:`STREAMED_JOIN_SCHEMA`, unless ``allow_mixed_schema`` is true.
    """
    out: list[BenchReport] = []
    for path in paths:
        with Path(path).open() as f:
            data = json.load(f)
        if not isinstance(data, list):
            raise SchemaError(f"{path}: top level must be a JSON array")
        for obj in data:
            out.append(_parse_one(obj, Path(path)))
    if not allow_mixed_schema:
        _refuse_mixed_schema(out)
    return out
```

  The message names the older report first however the files were ordered,
  which is what `test_refuses_to_mix_…`'s second `raises` pins.

- [ ] **Step 4: Thread the keyword through `frame.py`.** Change `load`'s
  signature and body:

```python
def load(
    paths: Iterable[Path | str] | Path | str,
    criterion_root: Path | str = "target/criterion",
    *,
    apply_defaults: bool = True,
    allow_mixed_schema: bool = False,
) -> pd.DataFrame:
```

  Append to its docstring: ``allow_mixed_schema`` passes through to
  :func:`~kermit_lab.loader.load_reports`. Change its first statement to:

```python
    reports = load_reports(_resolve_paths(paths), allow_mixed_schema=allow_mixed_schema)
```

  Make the same change to `load_samples`:

```python
def load_samples(
    paths: Iterable[Path | str] | Path | str,
    criterion_root: Path | str = "target/criterion",
    *,
    allow_mixed_schema: bool = False,
) -> pd.DataFrame:
```

  with `reports = load_reports(_resolve_paths(paths), allow_mixed_schema=allow_mixed_schema)`.

- [ ] **Step 5: Surface the error in the CLI.** In
  `kermit_lab/drivers/main.py`, change `from ..loader import load_reports`
  to `from ..loader import SchemaError, load_reports`. In `main`, add a
  handler after the `except InsufficientAxesError` block:

```python
    except SchemaError as e:
        log.error("%s: %s", args.command, e)
        return 4
```

- [ ] **Step 6: Bump both schema versions.** In
  `kermit/src/bench_report.rs`:

```rust
/// Schema version for the JSON report. Bump on any breaking change to
/// [`BenchReport`] field names or value types, or to what a metric
/// measures. 3: `iteration` / `end_to_end` time a streamed join whose rows
/// are counted, never materialised (issue #65).
pub const REPORT_SCHEMA_VERSION: u32 = 3;
```

  In `python/kermit-lab/kermit_lab/__init__.py`: `SCHEMA_VERSION = 3`.

- [ ] **Step 7: Run the Python suite.**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run pytest -q'`
Expected: all pass. The contract test needs `KERMIT_BIN`; it is covered in
Task 8.

- [ ] **Step 8: Run the Rust pin test.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit python_schema_version'`
Expected: `python_schema_version_matches_rust` passes.

- [ ] **Step 9: Commit.**

```bash
nix develop --command cargo fmt --all
git add kermit/src/bench_report.rs python/kermit-lab/kermit_lab python/kermit-lab/tests
git commit -m "feat(report,lab): schema 3; kermit-lab refuses to mix v2 and v3 (#65)

iteration and end_to_end now time a streamed, counted join, so their v3
values are not comparable with v2. A version bump alone would not stop a
mix, since the loader accepts any version it supports, so load_reports
raises SchemaError when one load straddles v3; allow_mixed_schema=True
is the deliberate escape hatch, and the CLI exits 4.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01LbYG3WiiRqrZ8zek8u7AyZ"
```

---

### Task 7: Documentation

**Files:** the docs row of the file map. Each edit is an exact replacement.
After the edits, `grep` for leftovers.

- [ ] **Step 1: `BENCHMARKING.md`.** In the "Time — three phases" table,
  replace the Iteration and End-to-end rows with:

```markdown
| **Iteration** | `iteration` | Running the join itself (`open`/`up`/`seek`/`next` traversal). Each result tuple is passed to a counting `black_box` sink and dropped: no result rows are allocated, and memory does not grow with the result | The worst-case-optimal join's actual cost; the headline number for query performance |
| **End-to-end** | `end-to-end` | One database build **plus K query executions** in a single timed body (`T = build + K × query`, K from `--queries-per-build`, default 1); each execution counts its rows the same way as Iteration | Amortisation/crossover questions: which structure wins depends on how many queries run per build |
```

  Then add this paragraph after the table:

```markdown
Reports with `schema_version` below 3 timed a join that also collected
every result row into memory, so their Iteration and End-to-end values are
not comparable with later ones. `kermit-lab` refuses to load both kinds
together unless you pass `allow_mixed_schema=True` (see
[`docs/specs/bench-report-schema.md`](docs/specs/bench-report-schema.md)).
`--verify` counts rows the same way, so verifying a result of billions of
rows needs no more memory than a small one.
```

- [ ] **Step 2: `kermit-algos` docs and `ARCHITECTURE.md`.**
  - In `docs/algorithms/hash-triejoin.md`:
    - line 43: "(the top-level `join_iter` entry point)" → "(the
      top-level `join_for_each` entry point)";
    - line 49: "`join_iter` no longer derives" → "`join_for_each` no
      longer derives";
    - replace the bullet at line 48 with:

```markdown
- **The result is streamed, not materialised.** `join_for_each` threads the caller's sink through `enumerate`; `emit_leaf` re-borrows each leaf chain through `leaf_tuples()` instead of collecting it, and the cross-product cursor and output row are scratch buffers allocated once per join, so emitting a tuple allocates nothing. Algorithm 3 itself is unchanged — `output.push(t)` became `emit(&row)` (issue #65). `JoinAlgo::join_iter` is a provided method that collects through `join_for_each`, for callers that need the rows.
```

  - In `docs/algorithms/leapfrog-triejoin.md`:
    - line 13: "before `join_iter` runs" → "before `join_for_each` runs";
    - line 49: "`join_iter` validates it" → "`join_for_each` validates
      it", and "permutes each result tuple back to head-first order" →
      "permutes each result tuple back to head-first order into one
      scratch row (lent by `TrieIteratorWrapper::advance`, so no tuple is
      allocated)", and "not from `join_iter`)" → "not from
      `join_for_each`)";
    - line 31: replace "plan validation and variable ordering in
      [`join_iter`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs)
      (line 355)" with the same text naming `join_for_each`, with the line
      number from
      `grep -n "fn join_for_each" kermit-algos/src/sorted/leapfrog_triejoin.rs`.
  - In `ARCHITECTURE.md`:
    - replace the paragraph at line 299 ("One behavioural asymmetry is
      worth knowing: …") with:

```markdown
Both algorithms stream. `JoinAlgo::join_for_each` passes each result tuple to a caller-supplied sink as a borrowed slice, so neither algorithm materialises its result: LFTJ reorders each tuple that `TrieIteratorWrapper::advance` lends into one scratch row, and HashTriejoin's leaf cross product writes into scratch buffers allocated once per join. `join_iter` is a provided method that collects through `join_for_each`, for callers that need the rows; benchmarks never call it inside a timed region (issue #65).
```

    - replace the `JoinAlgo` trait snippet with:

```rust
pub trait JoinAlgo<DS> where DS: JoinIterable {
    fn join_for_each<S: FnMut(&[usize])>(
        plan: &QueryPlan,
        query: JoinQuery,
        datastructures: HashMap<String, &DS>,
        emit: S,
    );

    // Provided: collects through `join_for_each`.
    fn join_iter(
        plan: &QueryPlan,
        query: JoinQuery,
        datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>> { … }
}
```

    - line 326: "Before handing a query to `JoinAlgo::join_iter`" →
      "Before handing a query to `JoinAlgo::join_for_each`";
    - line 334: "`join_iter` asserts `QueryPlan::validate` defensively" →
      "`join_for_each` asserts `QueryPlan::validate` defensively";
    - line 407: "`join_iter(..).collect()`" → "`join_for_each(.., emit)`".

- [ ] **Step 3: `CLAUDE.md`.**
  - line 17: "`join_iter` for algorithms" → "`join_for_each` for
    algorithms";
  - line 82: "JoinAlgo::join_iter executes." → "JoinAlgo::join_for_each
    executes.";
  - line 125: "consumed by `JoinAlgo::join_iter`" → "consumed by
    `JoinAlgo::join_for_each`";
  - line 264: "before handing the query to `JoinAlgo::join_iter`" →
    "before handing the query to `JoinAlgo::join_for_each`";
  - line 255: "(currently `2`)" → "(currently `3`)", and append to that
    bullet: "kermit-lab refuses to load reports from both sides of v3 in
    one call (`allow_mixed_schema=True` overrides), because from v3
    `iteration` / `end_to_end` time a streamed, counted join.";
  - in "Adding a new join algorithm" step 2, after "**Implement
    `JoinAlgo<DS>`**,", insert: "whose one required method is
    `join_for_each` — pass each tuple to the sink as a borrowed slice and
    never collect, since the `iteration` metric times exactly this path
    (`join_iter` is provided) —".

- [ ] **Step 4: Spec docs.**
  - `docs/specs/bench-report-schema.md`:
    - "**Current schema version:** `2`" → `3`;
    - the example's `"schema_version": 2` → `3`;
    - the field catalogue's "Currently `2`." → "Currently `3`.";
    - after "Consumers should refuse to parse if `schema_version` is
      missing or greater than the highest version they know about.", add:
      "kermit-lab also refuses to load reports from both sides of version 3
      in one call, because the `iteration` and `end_to_end` values changed
      meaning there.";
    - append this change-log row. It carries only #65's entry. Task 11
      adds the entries of the sibling changes that land first.

```markdown
| 3       | 2026-10-02 | What the time metrics measure changed, so values are not comparable with v2, and kermit-lab refuses to load v2 and v3 reports together unless `allow_mixed_schema=True`. `bench run` / `bench join` `iteration` and `end_to_end` time a streamed join whose rows are counted through a `black_box` sink and never materialised (#65); `--verify` counts the same way. |
```

  - `docs/specs/benchmarking-architecture.md`, step 5 of the `bench run`
    flow: after "(mismatch aborts; a query without `expected` is noted as
    not verified)", insert "— the count comes from
    `ExecutionFamily::count`, which streams the join through a `black_box`
    sink and never materialises it, as do the `Iteration` and `EndToEnd`
    timed bodies". In the "`EndToEnd` is the only metric…" paragraph,
    change "and then executes the query K times" to "and then executes and
    counts the query K times".
  - `python/kermit-lab/README.md`, "## Schema": "parses `BenchReport` JSON
    v2" → "parses `BenchReport` JSON up to v3"; after "The loader refuses
    to parse unknown major versions.", add: "It also refuses to mix
    reports from both sides of v3 in one load, because `iteration` and
    `end_to_end` changed meaning there; pass `allow_mixed_schema=True` to
    `kl.load` / `kl.load_samples` to override, for example to compare
    `space` only."

- [ ] **Step 5: `scripts/watdiv_stress_sample.py`.** In the module
  docstring, replace

```
    # test-1's q0005 has 4,169,173,508 result rows, beyond what the
    # `iteration` metric can materialise, so leave it out:
    uv run scripts/watdiv_stress_sample.py watdiv-stress-100-test-1 --exclude q0005
```

  with

```
    # Every template, q0005's 4,169,173,508 rows included: the metrics
    # count rows without materialising them. Use --exclude to drop any.
    uv run scripts/watdiv_stress_sample.py watdiv-stress-100-test-1
```

- [ ] **Step 6: Sweep for leftovers.**

```bash
grep -rn "join_iter" CLAUDE.md ARCHITECTURE.md BENCHMARKING.md docs/algorithms docs/optimisers docs/data-structures
grep -rn "materialis" docs/algorithms/hash-triejoin.md ARCHITECTURE.md BENCHMARKING.md
```

  Each `join_iter` hit should now be about the collecting convenience,
  such as "end-to-end through `join_iter`" in the hash-triejoin collision
  bullet, which is still true. Each "materialis" hit should not claim the
  join materialises.

- [ ] **Step 7: Check the docs build and lint.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps && CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets'`
Expected: no warnings.

- [ ] **Step 8: Commit.**

```bash
git add BENCHMARKING.md ARCHITECTURE.md CLAUDE.md docs/algorithms docs/specs/bench-report-schema.md docs/specs/benchmarking-architecture.md python/kermit-lab/README.md scripts/watdiv_stress_sample.py
git commit -m "docs: streamed join iteration and schema 3 (#65)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01LbYG3WiiRqrZ8zek8u7AyZ"
```

---

### Task 8: Run the full CI gate locally

- [ ] **Step 1: Run the whole workspace's tests.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test --workspace'`
Expected: all pass. Report any failure with its output; do not skip it.

- [ ] **Step 2: Run clippy, doc and fmt as CI does.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets --verbose && CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace && cargo fmt --all --check'`
Expected: clean. The flake's nightly is pinned from 2026-09-09, while CI
resolves `nightly` fresh, so CI can still differ on fmt and clippy. The
coordinating session refreshes the lock at landing; don't commit a
`flake.lock` change here.

- [ ] **Step 3: Run kermit-lab with the real binary.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo build -p kermit && cd python/kermit-lab && KERMIT_BIN=$PWD/../../target/debug/kermit uv run pytest -q'`
Expected: all pass, including `tests/test_contract.py`, which now loads
real v3 reports.

- [ ] **Step 4: Run the LUBM oracles.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo test -p kermit --test lubm_mini_oracle && which java && CARGO_BUILD_JOBS=2 cargo test -p kermit --test lubm_cardinalities -- --nocapture'`
Expected: both pass, and `which java` prints a path inside the nix store.
Then confirm that `lubm_cardinalities` really ran rather than skipping:
with `--nocapture`, a skip prints a note, and a run takes noticeably long
and reports per-optimiser checks. Record "ran" or "skipped" for the
hand-off report.

---

### Task 9: Check counts on real data

Every command in Tasks 9 and 10 runs from the worktree root, with:

```bash
SCRATCH=/tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-65-18daaefc8d732d0d/043869f1-bac9-4e99-afcf-9019f54c8918/scratchpad
QUICK="--sample-size 10 --measurement-time 1 --warm-up-time 1"
```

`QUICK` keeps Criterion's overhead to the minimum, 10 samples. These runs
check counts and memory, not timings. `--sample-size`, `--measurement-time`,
`--warm-up-time` and `--report-json` belong to the parent `bench` command, so
they go between `bench` and `run`; after `run`, clap rejects them.

- [ ] **Step 1: Run triangle with `--verify` in every cell.**

Run: `nix develop --command bash -c 'CARGO_BUILD_JOBS=2 cargo build --release -p kermit' && target/release/kermit bench $QUICK --report-json "$SCRATCH/triangle.json" run triangle -i all -a all -m iteration --verify`
Expected: three cells, each printing `verified: yes`, and exit 0.

- [ ] **Step 2: Run the prelim sample's largest results with `--verify`.**
  The cached `watdiv-stress-100-test-1-prelim` benchmark carries DuckDB
  `expected` counts. Its three largest results are q0131 (439,622 rows),
  q0034 (243,150) and q0021 (36,526).
  - Use `-m iteration`, not `--metrics space`. Space benchmarks every
    relation in the workload (61 here) for each query, which would take
    hours.
  - Skip TreeTrie: its linear `seek` (#67) makes these queries slow.
  - Run detached, one cell and one query at a time:

```bash
setsid nohup bash -c '
  for cell in "column-trie leapfrog-triejoin" "hash-trie hash-triejoin"; do
    set -- $cell
    for q in q0131 q0034 q0021; do
      target/release/kermit bench '"$QUICK"' --report-json "'"$SCRATCH"'/prelim-$1-$q.json" \
        run watdiv-stress-100-test-1-prelim -q $q -i $1 -a $2 -m iteration --verify \
        > "'"$SCRATCH"'/prelim-$1-$q.log" 2>&1
      echo "exit $?" >> "'"$SCRATCH"'/prelim-$1-$q.log"
    done
  done' > /dev/null 2>&1 &
```

  Poll with the Monitor tool, using an until-loop on
  `cat $SCRATCH/prelim-*.log | grep -c '^exit'` reaching 6. Expected:
  - all six logs end `exit 0`;
  - `grep -l 'verified:  *yes' $SCRATCH/prelim-*.log | wc -l` prints 6.

  A count mismatch aborts with "verification failed". That is a real bug:
  stop and use `superpowers:systematic-debugging`.

---

### Task 10: q0005 on two cells

- [ ] **Step 1: Build a one-query benchmark.** This lives in the user's
  cache, outside the repo. `meta.json` is the marker that makes the
  directory a discoverable cached benchmark, as with the prelim sample.

```bash
B=~/.cache/kermit/benchmarks/watdiv-q0005-check
SRC=~/.cache/kermit/benchmarks/watdiv-stress-100-test-1
mkdir -p $B
for r in gender nationality givenname parentcountry eligibleregion; do ln -f $SRC/$r.parquet $B/$r.parquet; done
cat > $B/benchmark.yml <<'EOF'
name: watdiv-q0005-check
description: watdiv-stress-100-test-1 q0005 alone (4,169,173,508 rows per DuckDB COUNT(*)); issue #65 verification
relations:
- name: gender
  url: https://zivahub.uct.ac.za/ndownloader/files/gender.parquet
- name: nationality
  url: https://zivahub.uct.ac.za/ndownloader/files/nationality.parquet
- name: givenname
  url: https://zivahub.uct.ac.za/ndownloader/files/givenname.parquet
- name: parentcountry
  url: https://zivahub.uct.ac.za/ndownloader/files/parentcountry.parquet
- name: eligibleregion
  url: https://zivahub.uct.ac.za/ndownloader/files/eligibleregion.parquet
queries:
- name: q0005
  description: WatDiv query q0005
  query: Q_test_1_q0005(V0, V3, V1, V2, V4) :- gender(V0, c779870), nationality(V0, V3), givenname(V0, V1), parentcountry(V2, V3), eligibleregion(V4, V3).
  expected: 4169173508
EOF
echo '{"kind": "manual-check", "source_benchmark": "watdiv-stress-100-test-1"}' > $B/meta.json
target/release/kermit bench list | grep watdiv-q0005-check
```

  Expected: `bench list` shows `watdiv-q0005-check` as cached. If it isn't
  discovered, compare against
  `~/.cache/kermit/benchmarks/watdiv-stress-100-test-1-prelim/` (same
  layout) before going on.

- [ ] **Step 2: Run both cells one after the other, fully detached, each
  capped at 45 minutes of wall clock.**

  Why the cap: q0005 has the shape the #68 session is investigating.
  - V3, the country, is the object of `nationality`, `parentcountry` and
    `eligibleregion`, so every valid plan binds V0, V2 and V4 before V3.
  - The lexicographic plan is V0, V1, V2, V4, V3, K: it enumerates
    users × given names × cities × offers before it checks country or
    gender.
  - That intermediate work may exceed the 4.17 B output rows by orders of
    magnitude, so an uncapped run could take hours or days per cell.

  GNU `time` lives at `/run/current-system/sw/bin/time` on this NixOS host (there is no `/usr/bin/time`). `timeout` sits *inside* it, so `time` still prints its
  report when the cap kills the run. On Linux, `wait4` reports the waited
  child's peak RSS, `kermit`'s included.

```bash
setsid nohup bash -c '
  for cell in "column-trie leapfrog-triejoin" "hash-trie hash-triejoin"; do
    set -- $cell
    /run/current-system/sw/bin/time -v timeout 45m target/release/kermit bench '"$QUICK"' \
      --report-json "'"$SCRATCH"'/q0005-$1.json" \
      run watdiv-q0005-check -q q0005 -i $1 -a $2 --metrics space --verify \
      > "'"$SCRATCH"'/q0005-$1.log" 2>&1
    echo "exit $?" >> "'"$SCRATCH"'/q0005-$1.log"
  done' > /dev/null 2>&1 &
```

  Poll with Monitor at a long interval; the whole loop can take up to
  about 90 minutes. Then collect:

```bash
for f in $SCRATCH/q0005-*.log; do echo "== $f"; grep -E 'verified|verification failed|Elapsed \(wall|Maximum resident|^exit' $f; done
```

  For each cell, expect one of two outcomes:
  - **Completed** (`exit 0`): `verified: yes` (the count was exactly
    4169173508), a "Maximum resident set size" in MB rather than hundreds
    of GB, and an elapsed time that is the per-execution time. That time
    also includes loading five relations and about 10 s of `space`
    benchmarking, both small next to the join.
  - **Capped** (`exit 124`, elapsed ≈ 45 min): no count, but a peak RSS in
    MB after 45 minutes of streaming. Since nothing collects rows, that
    still shows memory independent of result size. Record that the cell
    timed out, and at what cap. The sweep's q0005 policy then goes back to
    the user, with #68's diagnosis as the probable cause.

  Either way, the times are indicative only, because the host is shared.
  They go in the hand-off report, not in commits or issues. `exit 1` with
  "verification failed" is a real bug: stop and use
  `superpowers:systematic-debugging`.

- [ ] **Step 3: Remove the check benchmark.** It is discoverable: it has
  both `benchmark.yml` and `meta.json`. Left in place, `bench list` and
  `bench run --all` would pick up a 4.17 B-row query with placeholder URLs.
  The parquet files are hard links, so removing them leaves
  `watdiv-stress-100-test-1` intact.

```bash
rm -r ~/.cache/kermit/benchmarks/watdiv-q0005-check
ls ~/.cache/kermit/benchmarks/watdiv-stress-100-test-1/nationality.parquet   # must still exist
target/release/kermit bench list | grep -c watdiv-q0005-check                 # must print 0
```

---

### Task 12: `bench ds` scans without materialising (#79)

Folded in by the user after #66 landed; see the spec's #79 addendum. Do it
after merging `origin/master` (`71bb843`), because #66 rewrote `bench/ds.rs`
and `execution.rs`.

- [ ] **Step 1 (red):** add a test to `kermit-ds/src/ds/hash_trie/implementation.rs`,
  `for_each_tuple_visits_what_collect_tuples_returns`. It must cover both
  pruning policies, a duplicate tuple and a pruned singleton. Run
  `cargo test -p kermit-ds for_each_tuple` and check that it fails to
  compile.
- [ ] **Step 2 (green):** add `pub fn for_each_tuple<V: FnMut(&[usize])>(&self, visit: V)`
  next to `collect_tuples`. It is a recursive walk mirroring `collect_at`,
  calling `visit(tuple)` where `collect_at` pushes a clone. Update
  `collect_tuples`'s doc, which says `bench ds` times it. Mention the new
  traversal in `docs/data-structures/hash-trie.md`.
- [ ] **Step 3 (red, then green):** in `kermit/src/execution.rs`, replace
  `RelationFamily::tuples` with
  `fn for_each_tuple<V: FnMut(&[usize])>(rel: &Self::Rel, visit: V)` and a
  provided `fn scan(rel: &Self::Rel) -> u64` (`black_box` each tuple, then
  count it). Implement `for_each_tuple` in all four families:
  - sorted: `TrieIteratorWrapper::new(rel.trie_iter())` and `advance`;
  - hash: `rel.for_each_tuple(visit)`;
  - the join families delegate.

  Move the two existing tests that call `tuples` onto `for_each_tuple`, and
  add `scan_agrees_with_tuple_count_in_every_family`.
- [ ] **Step 4:** in `kermit/src/bench/ds.rs`, `{ds}/iteration` becomes
  `b.iter(|| F::scan(&relation))`, and `end_to_end` ends with
  `std::hint::black_box(F::scan(&built))`. Update the runner's doc comment,
  which names `F::tuples`.
- [ ] **Step 5:** add six scan tests to `kermit/tests/result_allocation.rs`,
  one per structure and Layout. Each scans a relation of 100 vs 100,000
  tuples through the primitive its family uses, with one unmeasured warm-up
  scan, and requires equal allocation counts. Mutation-check: make the
  sorted and hash visitors clone each tuple, confirm the mutation applied,
  and watch the tests fail. Restore with `git checkout` only after
  committing.
- [ ] **Step 6:** docs.
  - `BENCHMARKING.md`: drop the "`bench ds` still collects" parenthesis.
  - `docs/specs/benchmarking-architecture.md`: the `bench ds` metric
    table's Iteration and EndToEnd rows.
  - The v3 change-log row names every measurement change on master: #65,
    #66, #67, #74, the toolchain, and #79.
- [ ] **Step 7:** rerun Task 8's gate on the merged tree, with a private
  `MIRI_SYSROOT` and miri for `kermit-ds` too.

### Task 11: Hand off

- [ ] **Step 1: Review the branch.** Use
  `superpowers:requesting-code-review` on `origin/master..HEAD`.

- [ ] **Step 2: Report to the coordinating session.** SendMessage to
  "Current repo issues" (`uds:/run/user/1000/cc-socks/2487518.sock`).
  Include:
  - the branch (`aidanb/65`) and the HEAD sha;
  - the files touched;
  - the tests run, saying whether `lubm_cardinalities` ran or skipped;
  - the mutation-check results;
  - the prelim verify results;
  - q0005, per cell: completed or capped at 45 minutes, the wall time and
    the peak RSS, all marked indicative. If a cell was capped, say that the
    sweep's q0005 policy goes back to the user, pending #68;
  - that `~/.cache/kermit/benchmarks/watdiv-q0005-check` was removed;
  - the decisions the user made: `iteration` redefined; no q0005 policy;
    push sink; no `Result` before #78;
  - landing notes: whichever of #65 / #66 lands second merges
    `origin/master` (never rebase) and re-runs the gate. The schema-3
    change-log row in `docs/specs/bench-report-schema.md` must name every
    measurement change on master at landing; the coordinating session
    confirms the list. The expected additions to #65's entry, for each one
    that has landed:
    - `bench run` and `bench ds` `insertion` / `end_to_end` build from the
      reader's file order (#66);
    - HashTrie build and lookup cost change with its new bucket mapping
      (#66);
    - TreeTrie `seek` is a binary search, which changes TreeTrie
      `iteration` (#67).

    `bench ds`'s `{ds}/iteration` keeps its meaning. Its `insertion` and
    `end_to_end` change only through #66. The #66 entry must say *every
    structure*: the sorted tries also lose their sorted, deduplicated
    rebuild input. When merging #66, fix the comment in `run.rs`'s
    `end_to_end` block that still says `insertion` times "the presorting
    `from_tuples` path", and expect a small merge in
    `docs/specs/benchmarking-architecture.md`. Merge `origin/master` before
    the final gate; #67 moves the toolchain to nightly 2026-10-01, whose
    rustfmt formats inside macros that use `$metavariables`. Run miri with
    a private `MIRI_SYSROOT`: the shared `~/.cache/miri` races between
    sessions.

- [ ] **Step 3: Summarise for the user.** Cover what changed, the
  verification evidence, and the follow-up candidates from the spec's Out
  of scope section, notably `bench ds`'s `iteration` and LFTJ's
  per-descent allocation.
