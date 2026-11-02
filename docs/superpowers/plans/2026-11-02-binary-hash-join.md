# Binary Hash Join Baseline Implementation Plan

> **For agentic workers:** this plan runs under the supervised flow: `/flow-draft` writes the
> stories below the marker from the drafts, then `/flow-supervise` executes them story by story.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a left-deep pipelined binary hash join (`-a binary-hash-join`) over a new keyed hash
index (`-i hash-index`) as a fourth `(structure, algorithm)` cell: a baseline for what worst-case
optimal joins buy, built and timed on the same boundaries as the other three cells.

**Architecture:** A third iterator family, `KeyedIterable` (an index keyed on its leading columns,
probed by exact match), with `HashIndex<H>` as its structure and `BinaryHashJoin` as its algorithm.
The join order is derived from the optimiser's variable order by a new index-planning policy,
`TrieLevels::ProbeKeys`. Under it, `plan_indexes` reorders the plan so each atom's probe key leads
its columns, sets each spec's `levels` to that key's length, and lists the executed body in join
order. No optimiser, no `QueryPlan` and no existing structure or algorithm changes.

**Tech Stack:** Rust nightly (workspace crates `kermit-iters`, `kermit-algos`, `kermit-ds`,
`kermit`), clap, Criterion (unchanged), Python kermit-lab (one comment and one test).

**Spec:** `docs/specs/2026-11-02-binary-hash-join-design.md` (47093008, amended in the commit
that adds this skeleton). Read its Decisions and Design sections before any story. Every design
decision is there.

## Conventions for every story

- Every build and test command runs inside `nix develop --command` (CLAUDE.md, "NEVER run `cargo
  fmt` outside `nix develop`"). `cargo` is the profile's exclusive build, so stories run one at a
  time.
- Each story leaves the gate green on its own: fmt, clippy under `-D warnings`, tests, docs.
  That is why the `IndexStructure::HashIndex` and `JoinAlgorithm::BinaryHashJoin` registry
  variants wait for Story 4 (spec, "Sequencing", step 4).
- Commits go by pathspec, ending with the session's attribution lines. Never amend: fix rounds add
  commits.
- No story touches `TreeTrie`, `ColumnTrie`, `HashTrie`, `LeapfrogTriejoin` or `HashTriejoin`
  (Priority 6). The cross-structure test files listed under a story's **Files** gain rows; they
  change no existing row.
- Doc comments backtick code-like identifiers (clippy `doc_markdown`). The binary's modules are
  linted only by `cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings`.

## File structure

New files:

| File | Responsibility | Story |
|---|---|---|
| `kermit-algos/src/join_order.rs` | The join-order rule: a variable order to a left-deep join order plus `V′`. Family-agnostic, beside `index_plan.rs`. | 1 |
| `kermit-iters/src/keyed.rs` | The `KeyedIterable` trait and its family rationale. | 2 |
| `kermit-ds/src/ds/hash_index/mod.rs` | Module root and re-exports. | 2 |
| `kermit-ds/src/ds/hash_index/implementation.rs` | `HashIndex<H>`: representation, `from_view`, `Relation`, `Projectable`, `HeapSize`, `Cardinality`, `HasOptimizationAxes`. | 2 |
| `kermit-ds/src/ds/hash_index/hash_index_iter.rs` | `KeyedIterable` for `HashIndex<H>`: the scan and probe iterators. | 2 |
| `kermit-ds/src/ds/hash_index/tests.rs` | In-crate unit tests: hashing, sizing, chain order, the view-build equivalence. | 2 |
| `kermit-ds/tests/hash_index_tests.rs` | The `keyed_index_test_suite!` and `parquet_test_suite!` invocations, and the one-bucket strategy. | 2 |
| `docs/data-structures/hash-index.md` | The structure's page, from `TEMPLATE.md`. | 2 |
| `kermit-algos/src/keyed/mod.rs` | The keyed family's contract. | 3 |
| `kermit-algos/src/keyed/binary_hash_join.rs` | `BinaryHashJoin`: the pipelined left-deep join. | 3 |
| `docs/algorithms/binary-hash-join.md` | The algorithm's page, from `TEMPLATE.md`. | 3 |
| `kermit/tests/cli_hash_index_hasher_choice.rs` | `--ds-layout-hasher` reaches `hash-index`; every other `--ds-*` flag is refused. | 4 |

Modified files, by story: see each entry's **Files** line. Story 1 changes planning only (one new
`TrieLevels` variant and its planning branch). Story 2 adds a structure and its rows in the
cross-structure tests. Story 3 adds the algorithm, the `db` entry points and the library-level test
cells. Story 4 wires the binary. Story 5 sweeps the docs.

## Names contract

Every name one story defines for another. Only `/flow-draft`'s manager edits this section.

**Story 1 defines:**
- `kermit_iters::TrieLevels::ProbeKeys`, a third unit variant after `JoinKeys`, documented as in
  the spec's "Index planning under `ProbeKeys`".
- `kermit-algos/src/join_order.rs`, a private module (`mod join_order;` in `kermit-algos/src/lib.rs`,
  not re-exported):
  ```rust
  pub(crate) struct JoinOrder {
      /// Indices into `query.body`, in join order.
      pub(crate) atoms: Vec<usize>,
      /// `key_lens[i]`: how many variables `atoms[i]` shares with `atoms[..i]`.
      pub(crate) key_lens: Vec<usize>,
      /// V′: the plan, over `query`'s canonical numbering, with every atom's key variables before its new ones.
      pub(crate) plan: QueryPlan,
  }
  pub(crate) fn join_order(query: &JoinQuery, plan: &QueryPlan) -> JoinOrder;
  ```
- `kermit_algos::plan_indexes(accesses, plan, TrieLevels::ProbeKeys)` behaves as the spec's "Index
  planning under `ProbeKeys`" states: the body is in join order (`#0` the driver), each spec's
  `levels` is its key length, and `IndexPlan.plan` is `V′`. The signature is unchanged.

**Story 2 defines:**
- In `kermit-iters/src/keyed.rs`, re-exported from `kermit_iters`:
  ```rust
  pub trait KeyedIterable: JoinIterable {
      fn key_len(&self) -> usize;
      fn scan(&self) -> impl Iterator<Item = &[usize]> + '_;
      fn probe<'a>(&'a self, key: &'a [usize]) -> impl Iterator<Item = &'a [usize]> + 'a;
  }
  ```
  `probe` requires `key.len() == self.key_len()` (assert). With `key_len() == 0` it lends every
  row. Both iterators lend rows in source order and allocate nothing.
- `kermit_ds::HashIndex<H: HashStrategy = SipHashStrategy>`, re-exported from `kermit_ds`
  (`kermit-ds/src/ds/mod.rs` `pub use hash_index::HashIndex;`, then `kermit-ds/src/lib.rs`).
  `Relation` has `TRIE_LEVELS = TrieLevels::ProbeKeys` and `KEEPS_DUPLICATES = true`. It also
  implements `Projectable`, `HeapSize`, `Cardinality`, `KeyedIterable` and `HasOptimizationAxes`,
  the last with the one key `ds_layout_hasher` and value `H::NAME`.
- The test macro `keyed_index_test_suite!($type:ty)` in `kermit-ds/tests/common/macros.rs`.

**Story 3 defines:**
- `kermit_algos::BinaryHashJoin`, a unit struct, re-exported flat from `kermit-algos/src/lib.rs`,
  with `impl<DS: KeyedIterable> JoinAlgo<DS> for BinaryHashJoin`. Its invariant panic message
  starts with `probe keys lead the columns`.
- In `kermit/src/db.rs`, beside `hash_join_for_each` / `hash_join` and mirroring them (bodies
  `run_join::<R, BinaryHashJoin>` / `collect_join::<R, BinaryHashJoin>`):
  ```rust
  pub fn binary_join_for_each<R>(
      catalog: &Catalog, query: JoinQuery, planner: &Planner, emit: impl FnMut(&[usize]),
  ) -> Result<(), JoinError>
  where
      R: Relation + KeyedIterable;
  pub fn binary_join<R>(
      catalog: &Catalog, query: JoinQuery, planner: &Planner,
  ) -> Result<(RelationHeader, Tuples), JoinError>
  where
      R: Relation + KeyedIterable;
  ```
- `impl<R: Relation + KeyedIterable> JoinEntry<R> for BinaryHashJoin` in
  `kermit/tests/common/utils.rs`.
- The test aliases `type HashIndexSip = HashIndex<SipHashStrategy>;` and
  `type HashIndexFx = HashIndex<FxHashStrategy>;`, each defined in the test file that uses it.

**Story 4 defines:**
- `kermit_ds::IndexStructure::HashIndex` (axis value `"HashIndex"`, CLI value `hash-index`,
  `FromStr` spelling `hash_index`).
- `kermit_algos::JoinAlgorithm::BinaryHashJoin` (axis value `"BinaryHashJoin"`, CLI value
  `binary-hash-join`).
- In `kermit/src/main.rs`: `IndexStructureSelector::HashIndex` and
  `JoinAlgorithmSelector::BinaryHashJoin`.
- In `kermit/src/execution.rs`:
  - `Execution::IndexBhj { hasher: HasherChoice }`, whose `trie_levels()` returns
    `TrieLevels::ProbeKeys`;
  - `pub struct HashIndexFamily<H>`, a `RelationFamily`;
  - `pub struct IndexBhj<H>`, an `ExecutionFamily` with `JOIN_MUTATES = false`.
- In `kermit/src/options.rs`: the macro `with_hash_index_layout!`, and `IndexStructure::HashIndex`
  in `DsFlag::LayoutHasher`'s row of `DsFlag::structures`.

## Stories

- **Story 1** — The join order and `ProbeKeys` index planning.

  Adds the third planning policy, with no consumer yet. `join_order.rs` turns a variable order
  into a left-deep join order and `V′` by the spec's rule. `plan_indexes` gains its `ProbeKeys`
  branch: columns sorted by `V′`, `levels` the key length, the body in join order. The
  `IndexSpec::levels` doc gains the `ProbeKeys` clause. Unit tests pin:
  - the spec's three worked examples;
  - the properties in its Testing table, on the 16 suite shapes under every variable order;
  - the pinned cross-product case;
  - that `All` and `JoinKeys` output is unchanged.

  The only exhaustive match on `TrieLevels` is in `index_plan.rs`, so the rest of the workspace
  compiles untouched.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-iters/src/trie_levels.rs`, `kermit-algos/src/join_order.rs`, `kermit-algos/src/index_plan.rs`, `kermit-algos/src/lib.rs`
  - **Interfaces:** exposes `TrieLevels::ProbeKeys` and `plan_indexes`'s `ProbeKeys` behaviour (names contract, Story 1) to Stories 2–4; uses nothing from other stories
  - **Verify:** `nix develop --command cargo test -p kermit-iters -p kermit-algos`
  - **Review:** deep
  - **Reading:** spec "Decisions" 2–4, "Background: index planning today", "The join order", "Index planning under `ProbeKeys`", "Testing" (the two `kermit-algos` rows on `join_order.rs` and `index_plan.rs`); `kermit-iters/src/trie_levels.rs:1-18`; `kermit-algos/src/index_plan.rs:1-185` and its tests `:186-501`; `kermit-algos/src/analysis.rs:1-120`; `kermit-algos/src/optimiser/plan.rs:1-100`; `kermit-algos/src/lib.rs:1-50`; `kermit/tests/all_orders.rs:120-215` (the 16 shapes, to copy into the unit tests)

- **Story 2** — `KeyedIterable` and `HashIndex`.

  Adds the family trait and the structure, following the "Adding a new index structure" recipe
  steps 1–5 and 8, without the `IndexStructure` variant, which Story 4 adds. The layout:
  - the view's rows, one copy;
  - a power-of-two directory of `RowId` bucket heads, at least 2 slots;
  - one `RowId` link per row, as plain `Vec<RowId>` with a NIL sentinel. Not `ChainLinks`: its
    `AtomicU32` links exist only for lazy HashTrie, and using them would bring in that landmine.

  Hashing is FxHash's fold followed by Fibonacci bucket selection. Linking last-to-first leaves
  every chain in source order. `probe` filters each chain by key comparison.

  Tests:
  - the in-crate unit tests: sizing, chain order, and the view-build equivalence;
  - the integration suite `keyed_index_test_suite!`, invoked for `HashIndex<SipHashStrategy>`,
    `HashIndex<FxHashStrategy>` and a test-local `OneBucketHash` strategy (a `HashStrategy` that
    hashes every value to 0), so the key comparison alone must filter;
  - `parquet_test_suite!` with a scan collector;
  - HashIndex rows in `kermit-ds/src/relation.rs`'s cross-structure view tests: from-view
    equivalence, `KEEPS_DUPLICATES`, and the wrong-arity and empty-source panics.

  Also writes the structure's page.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-iters/src/keyed.rs`, `kermit-iters/src/lib.rs`, `kermit-ds/src/ds/hash_index/mod.rs`, `kermit-ds/src/ds/hash_index/implementation.rs`, `kermit-ds/src/ds/hash_index/hash_index_iter.rs`, `kermit-ds/src/ds/hash_index/tests.rs`, `kermit-ds/src/ds/mod.rs`, `kermit-ds/src/lib.rs`, `kermit-ds/src/relation.rs`, `kermit-ds/tests/common/macros.rs`, `kermit-ds/tests/hash_index_tests.rs`, `docs/data-structures/hash-index.md`
  - **Interfaces:** exposes `KeyedIterable`, `HashIndex<H>` and `keyed_index_test_suite!` (names contract, Story 2) to Stories 3–4; uses `TrieLevels::ProbeKeys` from Story 1
  - **Verify:** `nix develop --command cargo test -p kermit-iters -p kermit-ds`
  - **Review:** deep
  - **Reading:** spec "Decisions" 4–7, "`KeyedIterable`", "`HashIndex<H>`", "Testing" (the `kermit-ds` row); CLAUDE.md "Adding a new index structure"; `kermit-iters/src/hash_trie.rs:1-101` (a family trait's rationale doc); `kermit-iters/src/lib.rs:1-75`; `kermit-iters/src/hash_strategy.rs:1-60`; `kermit-iters/src/optimization.rs:1-70`; `kermit-ds/src/relation.rs:160-330` and `:1300-1520`; `kermit-ds/src/cardinality.rs`; `kermit-ds/src/heap_size.rs`; `kermit-ds/src/ds/mod.rs:1-30`; `kermit-ds/src/lib.rs:20-60`; `kermit-ds/src/ds/tree_trie/implementation.rs` (the recipe's precedent); `kermit-ds/tests/common/macros.rs:723-800` (`parquet_test_suite!`) and `:1773-1790`; `kermit-ds/tests/hash_trie_tests.rs:1-60`; `docs/data-structures/TEMPLATE.md`

- **Story 3** — `BinaryHashJoin`, its entry points, and the library-level test cells.

  Adds the keyed family's algorithm (recipe "Adding a new join algorithm", steps 1–3, 5 and 6,
  without the `JoinAlgorithm` variant, which Story 4 adds). It reads the executed body left to
  right per the spec's pseudocode: scratch allocated once per join, and the "probe keys lead the
  columns" assertion. It also adds:
  - `binary_join_for_each` / `binary_join` in `kermit::db`;
  - the `JoinEntry` impl;
  - the six `define_multiway_join_test_suite!` invocations (`HashIndexSip` and `HashIndexFx` ×
    the three optimisers);
  - a `HashIndex` cell in `all_orders.rs`: shapes, pinned queries, and the duplicated constant
    atom, which multiplies, since it keeps duplicates;
  - cells in `result_allocation.rs` and `tuple_allocation.rs`;
  - rows in `lubm_mini_oracle.rs` and `lubm_cardinalities.rs`;
  - two `kermit::db` lifecycle tests: all-constant multiplicity on `HashIndex`, and a `ProbeKeys`
    query built by a structure of another policy panics.

  Also writes the algorithm's page, with Free Join §2.2's pseudocode and the triangle as its
  micro-example.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-algos/src/keyed/mod.rs`, `kermit-algos/src/keyed/binary_hash_join.rs`, `kermit-algos/src/lib.rs`, `kermit/src/db.rs`, `kermit/src/db/lifecycle.rs`, `kermit/tests/common/utils.rs`, `kermit/tests/join_tests.rs`, `kermit/tests/all_orders.rs`, `kermit/tests/result_allocation.rs`, `kermit/tests/tuple_allocation.rs`, `kermit/tests/lubm_mini_oracle.rs`, `kermit/tests/lubm_cardinalities.rs`, `docs/algorithms/binary-hash-join.md`
  - **Interfaces:** exposes `BinaryHashJoin` and `binary_join_for_each` / `binary_join` (names contract, Story 3) to Story 4; uses `ProbeKeys` planning (Story 1) and `KeyedIterable` / `HashIndex` (Story 2)
  - **Verify:** `nix develop --command bash -c 'cargo test -p kermit-algos && cargo test -p kermit --lib --test join_tests --test all_orders --test result_allocation --test tuple_allocation --test lubm_mini_oracle --test lubm_cardinalities'`
  - **Review:** deep
  - **Reading:** spec "Decisions" 1, 3, 7, "`BinaryHashJoin`", "Entry points", "Testing" (every row from `binary_hash_join.rs` to `lubm_cardinalities.rs`); CLAUDE.md "Adding a new join algorithm"; `kermit-algos/src/join_algo.rs`; `kermit-algos/src/hash/mod.rs`; `kermit-algos/src/hash/hash_triejoin.rs:1-140` (a family algorithm's shape); `kermit-algos/src/lib.rs:1-75`; `kermit/src/db.rs:25-185`; `kermit/src/db/lifecycle.rs:290-440` and `:1105-1160`; `kermit/tests/common/utils.rs:1-130`; `kermit/tests/join_tests.rs:1-120`; `kermit/tests/all_orders.rs:250-340` and `:440-520`; `kermit/tests/result_allocation.rs:110-260`; `kermit/tests/tuple_allocation.rs` (one existing cell, as the pattern); `kermit/tests/lubm_mini_oracle.rs:100-160`; `kermit/tests/lubm_cardinalities.rs`; `docs/algorithms/TEMPLATE.md`; Free Join §2.2 at `/tb/Source/Academia/kermit-msc-thesis/background/markdown/Wang et al. - 2023 - Free Join Unifying Worst-Case Optimal and Traditional Joins.md:255-345`

- **Story 4** — The binary: registry variants, the `IndexBhj` cell, flags, dispatch and CLI tests.

  This story makes the cell reachable from every command. It adds:
  - `IndexStructure::HashIndex` and `JoinAlgorithm::BinaryHashJoin`, with their pinned
    axis-value tests;
  - the two selectors;
  - `Execution::IndexBhj` across `for_pair`, `for_structure`, `index_structure`, `algorithm` and
    `trie_levels`;
  - the two families, with `execution()` re-deriving `hasher` through `hasher_of::<H>()`;
  - `with_hash_index_layout!`;
  - the `DsFlag::LayoutHasher` row;
  - the `dispatch_run_bench`, `dispatch_ds_bench` and `write_query_csv` arms.

  Tests:
  - `execution.rs` unit tests: `for_structure` agrees with `for_pair`, and the sweep's skipped
    pairs;
  - `cli_query_errors.rs` gains the cell;
  - `cli_bench_run_sweep.rs` asserts four cells;
  - the new `cli_hash_index_hasher_choice.rs`.

  It also makes the kermit-lab comment and test change from the spec's "CLI, reports and
  kermit-lab".

  - **Developer:** intermediate-dev
  - **Files:** `kermit-ds/src/ds/mod.rs`, `kermit-algos/src/lib.rs`, `kermit/src/main.rs`, `kermit/src/execution.rs`, `kermit/src/options.rs`, `kermit/src/bench/run.rs`, `kermit/src/bench/ds.rs`, `kermit/tests/cli_query_errors.rs`, `kermit/tests/cli_bench_run_sweep.rs`, `kermit/tests/cli_hash_index_hasher_choice.rs`, `python/kermit-lab/kermit_lab/analysis.py`, `python/kermit-lab/tests/test_analysis.py`
  - **Interfaces:** exposes the registry variants, `Execution::IndexBhj`, `HashIndexFamily<H>`, `IndexBhj<H>` and `with_hash_index_layout!` (names contract, Story 4) to Story 5; uses `HashIndex<H>` (Story 2) and `BinaryHashJoin` (Story 3)
  - **Verify:** `nix develop --command bash -c 'cargo test -p kermit && cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings && cd python/kermit-lab && uv run --locked --group test pytest -q'`
  - **Review:** deep
  - **Reading:** spec "The binary", "CLI, reports and kermit-lab", "Sequencing" step 4, "Testing" (the CLI row); CLAUDE.md "Adding a new index structure" step 6 and "Adding a new join algorithm" step 4; `kermit-ds/src/ds/mod.rs:20-90`; `kermit-algos/src/lib.rs:45-110`; `kermit/src/main.rs:125-190` and `:552-640`; `kermit/src/execution.rs:1-330`, `:360-470`, `:560-742` and its tests from `:742`; `kermit/src/options.rs:40-130`, `:230-330`, `:620-760` and `:1020-1060`; `kermit/src/bench/run.rs:320-420`; `kermit/src/bench/ds.rs:130-185`; `kermit/tests/cli_query_errors.rs:1-80`; `kermit/tests/cli_bench_run_sweep.rs:115-160`; `kermit/tests/cli_hash_trie_hasher_choice.rs` (the precedent for the new CLI test); `python/kermit-lab/kermit_lab/analysis.py:178-205` and `:280-292`; `python/kermit-lab/tests/test_analysis.py:170-190`

- **Story 5** — The documentation sweep.

  Updates every doc the spec's "Documentation" section lists beyond the two component pages:
  - `ARCHITECTURE.md`: three iterator families, `ProbeKeys` planning, `join_order` beside
    `access` and `index_plan`;
  - `CLAUDE.md`: 3 → 4 valid pairs, the family lists in the workspace architecture and the trait
    hierarchy, the `TrieLevels` entry, the Testing Patterns aliases, the Component Reference
    Docs list, a build command;
  - `README.md`, `USAGE.md`, `kermit/README.md`, `kermit-ds/README.md`;
  - `docs/specs/bench-report-schema.md`'s value lists.

  Docs only, so the proof is the doc builds.

  - **Developer:** intermediate-dev
  - **Files:** `ARCHITECTURE.md`, `CLAUDE.md`, `README.md`, `USAGE.md`, `kermit/README.md`, `kermit-ds/README.md`, `docs/specs/bench-report-schema.md`
  - **Interfaces:** none exposed; uses every name in the names contract, as text
  - **Verify:** `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps && cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings'`
  - **Review:** light (documentation only; no code changes)
  - **Reading:** spec "Documentation"; CLAUDE.md (every passage that says "3 valid", "two iterator families", or lists `TrieLevels`, the structures or the algorithms); `ARCHITECTURE.md` (its "Two Iterator Families" section and the crate descriptions); `README.md:45-140`; `USAGE.md:80-100` and `:335-355`; `docs/specs/bench-report-schema.md:110-125` and `:195-235`

<!-- plan_join: the stories below are generated from the drafts; edit the drafts, then re-join -->
