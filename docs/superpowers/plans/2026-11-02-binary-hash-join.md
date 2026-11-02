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
- No story touches the code of `TreeTrie`, `ColumnTrie`, `HashTrie`, `LeapfrogTriejoin` or
  `HashTriejoin` (Priority 6). Doc text this change makes false is corrected, and nothing more:
  Story 5's edits to `docs/data-structures/hash-trie.md` and to the heading reference in
  `kermit-algos/src/sorted/mod.rs`. The cross-structure test files listed under a story's **Files** gain rows; they
  change no existing row's behaviour. Two kinds of existing line do change, each named in its
  story's paragraph: a count of the cells a `-i all -a all` sweep runs (3 becomes 4, Story 4), and
  an assertion that branches on `R::TRIE_LEVELS` so that `ProbeKeys` checks `V′` (Story 3's
  `rows_in_order` in `all_orders.rs`).
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
- The test macro `keyed_index_test_suite!($relation_type:ident)` in
  `kermit-ds/tests/common/macros.rs`: an `ident`, as `parquet_test_suite!` and
  `hash_trie_test_suite!` take, because `paste` names the module from it. Callers pass an alias
  (`keyed_index_test_suite!(HashIndexSip)`).

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

**Cross-story decisions and quoted texts** (from the seams check; every fixer leaves a named text
unchanged):
- Quoted text: `kermit-algos/src/lib.rs`'s Layout paragraph, its `join_order` doc lines and
  `mod join_order;`, as Story 1 leaves them (Story 1, Task 1.1), are quoted by Story 3 (Task 3.1:
  `mod keyed;`, the number word +1, the families lines). Story 4's edits to that file
  (`join_algorithm_tests`, the `JoinAlgorithm` enum and `axis_value`, the "spans both families"
  comment) anchor on text neither earlier story changes.
- `describe_index` (`kermit/src/bench/run.rs`) keeps its behaviour: it appends ` levels {n}`
  whenever an index has fewer levels than columns, so under `ProbeKeys` it reports each index's
  key length (` levels 0` for the driver). Story 4 rewords its doc comment; Story 5 rewords the
  same claim in `docs/specs/bench-report-schema.md` and `docs/data-structures/hash-trie.md`.
- One owner per file for the crate READMEs: Story 5 edits `kermit-algos/README.md`,
  `kermit-iters/README.md`, `kermit/README.md` and `kermit-ds/README.md`, and the
  `kermit/src/lib.rs` entry-point list; Stories 2 and 3 touch none of them.
- Story 5 renames `ARCHITECTURE.md`'s "Two Iterator Families" heading to "Three Iterator
  Families" and updates every reference to it (`kermit-algos/src/sorted/mod.rs`,
  `kermit-algos/README.md`).

**Design stage:** /flow-draft session e838d54f-a791-44d5-a2ba-3ff236d142c3, 2026-11-02T12:16:35Z to 2026-11-02T13:10:52Z UTC: $25.86 logged, $25.91 with estimated thinking; 8752 plan lines, $2.95 per 1,000 lines.

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
  - **Files:** `kermit-ds/src/ds/mod.rs`, `kermit-algos/src/lib.rs`, `kermit/src/main.rs`, `kermit/src/execution.rs`, `kermit/src/options.rs`, `kermit/src/bench/run.rs`, `kermit/src/bench/ds.rs`, `kermit/tests/cli_query_errors.rs`, `kermit/tests/cli_bench_run_sweep.rs`, `kermit/tests/cli_hash_index_hasher_choice.rs`, `kermit/tests/cli_bench_run_ds_flag_reach.rs`, `kermit/tests/cli_bench_run_group_collision.rs`, `kermit/tests/cli_bench_probe.rs`, `kermit/tests/cli_column_trie_build_mode.rs`, `kermit/tests/cli_tree_trie_build_mode.rs`, `kermit/tests/cli_hash_trie_build_mode.rs`, `kermit/tests/cli_hash_trie_layout_expansion.rs`, `kermit/tests/cli_hash_trie_layout_pruning.rs`, `kermit/tests/cli_hash_trie_layout_levels.rs`, `kermit/tests/cli_sorted_trie_layout_seek.rs`, `kermit/tests/cli_snap_gen.rs`, `kermit/tests/cli_join_tests.rs`, `kermit/tests/cli_optimiser_choice.rs`, `python/kermit-lab/kermit_lab/analysis.py`, `python/kermit-lab/tests/test_analysis.py`, `python/kermit-lab/kermit_lab/axis_mapping.py`, `python/kermit-lab/tests/test_axis_mapping.py`
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
  - **Files:** `ARCHITECTURE.md`, `CLAUDE.md`, `README.md`, `USAGE.md`, `kermit/README.md`, `kermit-ds/README.md`, `kermit-algos/README.md`, `kermit-iters/README.md`, `kermit/src/lib.rs`, `kermit-algos/src/sorted/mod.rs`, `docs/data-structures/hash-trie.md`, `docs/specs/bench-report-schema.md`
  - **Interfaces:** none exposed; uses every name in the names contract, as text
  - **Verify:** `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps && cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings'`
  - **Review:** light (documentation only; no code changes)
  - **Reading:** spec "Documentation"; CLAUDE.md (every passage that says "3 valid", "two iterator families", or lists `TrieLevels`, the structures or the algorithms); `ARCHITECTURE.md` (its "Two Iterator Families" section and the crate descriptions); `README.md:45-140`; `USAGE.md:80-100` and `:335-355`; `docs/specs/bench-report-schema.md:110-125` and `:195-235`

<!-- plan_join: the stories below are generated from the drafts; edit the drafts, then re-join -->

## Story 1: The join order and `ProbeKeys` index planning

- **Developer:** intermediate-dev
- **Files:** `kermit-iters/src/trie_levels.rs`, `kermit-algos/src/join_order.rs`, `kermit-algos/src/index_plan.rs`, `kermit-algos/src/lib.rs`
- **Interfaces:** exposes `TrieLevels::ProbeKeys` and `plan_indexes`'s `ProbeKeys` behaviour (names contract, Story 1) to Stories 2–4; uses nothing from other stories
- **Verify:** `nix develop --command cargo test -p kermit-iters -p kermit-algos`
- **Review:** deep

This story adds the third index-planning policy, with no consumer yet. A new private module, `join_order.rs`, reads a variable order as a sequence of binding events and turns it into a left-deep join order plus the reordered plan `V′`, by the spec's rule. `TrieLevels` gains its `ProbeKeys` variant, and `plan_indexes` gains the matching branch: the columns follow `V′`, each spec's `levels` is the access's key length, and the executed body is listed in join order with `#0` the driver. `All` and `JoinKeys` plan exactly as before. The only exhaustive `match` on `TrieLevels` in the workspace is in `index_plan.rs`, so no other crate changes. Three tasks: the join-order module (Task 1.1), the variant and the planning branch (Task 1.2), and the property sweep that ties the two together on the 16 suite shapes (Task 1.3). None of the code holds a landmine marker.

### Task 1.1: The join-order rule

**Files:**
- Create: `kermit-algos/src/join_order.rs`
- Modify: `kermit-algos/src/lib.rs:10-18` (the layout doc) and `:31-33` (the module list)
- Test: `kermit-algos/src/join_order.rs`

- [ ] **Step 1: Write the failing tests**

Create `kermit-algos/src/join_order.rs` with the types, a stub body for `join_order` (replaced in Step 3) and the whole test module. The `suite` module holds the 16 standard patterns' queries and a permutation helper that Task 1.3 reuses.

```rust
//! The left-deep join order of a planned query.
//!
//! A variable order is a sequence of binding events: read left to right, each
//! variable not yet bound is bound by joining an atom that carries it.
//! [`join_order`] turns that reading into a left-deep join order over the
//! atoms of an access query ([`crate::extract_accesses`]), together with the
//! plan `V′` the same joins bind in. It reads only the atoms' variables and
//! the plan, never data, and belongs to no iterator family: index planning
//! applies it under `TrieLevels::ProbeKeys` ([`crate::plan_indexes`]).
//!
//! ```text
//! bound ← ∅;  order ← []
//! while some atom is unjoined:
//!     u ← the first variable of V not in bound
//!     a ← among the unjoined atoms containing u, the one with the most
//!         variables in bound (ties: the earliest in the body)
//!     join a;  bound ← bound ∪ vars(a)
//!     join, in body order, every unjoined atom whose variables are all in
//!         bound (pure filters, probed as soon as their key is complete)
//! V′ ← for each atom in order, its variables not bound before it joined,
//!      each group in V order
//! ```
//!
//! An atom's key is its variables bound before it joins. In `V′` every key
//! variable precedes the atom's new variables, so an index whose columns
//! follow `V′` has its key first. The first atom's key is empty, and so is
//! the key of an atom joined across a cross product: both are an empty key,
//! which selects every row.

// Temporary (Task 1.2 removes this line): `plan_indexes` is the first caller.
#![allow(dead_code)]

use {crate::optimiser::QueryPlan, kermit_parser::JoinQuery};

/// A left-deep join order over the atoms of an access query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JoinOrder {
    /// Indices into `query.body`, in join order.
    pub(crate) atoms: Vec<usize>,
    /// `key_lens[i]`: how many variables `atoms[i]` shares with `atoms[..i]`.
    pub(crate) key_lens: Vec<usize>,
    /// V′: the plan, over `query`'s canonical numbering, with every atom's
    /// key variables before its new ones.
    pub(crate) plan: QueryPlan,
}

/// The join order of `query` under the variable order `plan`, by the rule in
/// the module documentation.
///
/// # Panics
///
/// Panics if `plan` is not a plan of `query`: a variable of the plan that no
/// atom carries, or one it does not name.
pub(crate) fn join_order(_query: &JoinQuery, _plan: &QueryPlan) -> JoinOrder {
    todo!("Task 1.1, Step 3 replaces this stub")
}

/// The 16 standard join patterns' queries and the variable orders to run
/// them under, shared by the tests of this module and of `index_plan`.
#[cfg(test)]
pub(crate) mod suite {
    use {
        crate::{extract_accesses, rewrite_placeholders, AccessQuery},
        kermit_parser::JoinQuery,
    };

    /// `(name, query)` of the 16 standard patterns, as in
    /// `kermit/tests/all_orders.rs`.
    pub(crate) const QUERIES: [(&str, &str); 16] = [
        ("unary", "Q(V0) :- r0(V0)."),
        (
            "triangle",
            "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2), r2(V0, V2).",
        ),
        (
            "chain",
            "Q(V0, V1, V2, V3) :- r0(V0, V1), r1(V1, V2), r2(V2, V3).",
        ),
        ("star", "Q(V0, V1, V2) :- r0(V0, V1), r1(V0, V2)."),
        ("selfjoin", "Q(V0, V1, V2) :- r0(V0, V1), r0(V1, V2)."),
        ("existential", "Q(V0) :- r0(V0), r1(V0)."),
        ("empty-result", "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2)."),
        ("single-relation", "Q(V0, V1) :- r0(V0, V1)."),
        (
            "four-way-chain",
            "Q(V0, V1, V2, V3, V4) :- r0(V0, V1), r1(V1, V2), r2(V2, V3), r3(V3, V4).",
        ),
        ("wide-fanout", "Q(V0, V1, V2) :- r0(V0, V1), r1(V0, V2)."),
        ("dead-end", "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2)."),
        (
            "deep-dead-end",
            "Q(V0, V1, V2, V3) :- r0(V0, V1), r1(V1, V2), r2(V2, V3).",
        ),
        ("diagonal", "Q(V0) :- r0(V0, V0)."),
        (
            "repeated-nonadjacent",
            "Q(V0, V1) :- r0(V0, V1, V0), r1(V1).",
        ),
        ("trailing-placeholder", "Q(V0) :- r0(V0, _), r1(V0)."),
        ("middle-placeholder", "Q(V0, V2) :- r0(V0, _, V2), r1(V2)."),
    ];

    /// `text` as the access query `kermit::db::prepare` plans over: the
    /// placeholders rewritten, then the accesses extracted.
    pub(crate) fn accesses_of(text: &str) -> AccessQuery {
        let query: JoinQuery = text.parse().expect("parse");
        extract_accesses(&rewrite_placeholders(query)).expect("extract")
    }

    /// Every permutation of `0..n`: the variable orders a plan can take.
    pub(crate) fn permutations(n: usize) -> Vec<Vec<usize>> {
        fn extend(prefix: &mut Vec<usize>, left: &[usize], out: &mut Vec<Vec<usize>>) {
            if left.is_empty() {
                out.push(prefix.clone());
                return;
            }
            for (at, &variable) in left.iter().enumerate() {
                let mut rest = left.to_vec();
                rest.remove(at);
                prefix.push(variable);
                extend(prefix, &rest, out);
                prefix.pop();
            }
        }
        let mut out = Vec::new();
        extend(&mut Vec::new(), &(0..n).collect::<Vec<_>>(), &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{
            suite::{accesses_of, permutations, QUERIES},
            *,
        },
        crate::{analyse, canonical_names},
        std::collections::HashSet,
    };

    /// `order` (variable names) as a plan of `query`.
    fn plan_of(query: &JoinQuery, order: &[&str]) -> QueryPlan {
        let names = canonical_names(query);
        QueryPlan {
            variable_ordering: order
                .iter()
                .map(|name| {
                    names
                        .iter()
                        .position(|n| n == name)
                        .expect("a query variable")
                })
                .collect(),
        }
    }

    /// The join order of `text` under the variable order `order`.
    fn joined(text: &str, order: &[&str]) -> JoinOrder {
        let accesses = accesses_of(text);
        let plan = plan_of(&accesses.query, order);
        join_order(&accesses.query, &plan)
    }

    fn expected(atoms: &[usize], key_lens: &[usize], ordering: &[usize]) -> JoinOrder {
        JoinOrder {
            atoms: atoms.to_vec(),
            key_lens: key_lens.to_vec(),
            plan: QueryPlan {
                variable_ordering: ordering.to_vec(),
            },
        }
    }

    /// Worked example 1: a tie between `R` and `T` goes to the earlier atom,
    /// and `T` is probed on both its variables once `S` has bound `Z`.
    #[test]
    fn a_triangle_joins_in_body_order_with_growing_keys() {
        let got = joined("Q(A, B, C) :- R(A, B), S(B, C), T(A, C).", &["A", "B", "C"]);
        assert_eq!(got, expected(&[0, 1, 2], &[0, 1, 2], &[0, 1, 2]));
    }

    /// Worked example 2: `B(y, z)` is probed on `z`, which `A(x, z)` bound
    /// before `y`, so `z` moves ahead of `y` in `V′`.
    #[test]
    fn a_shared_variable_is_bound_before_the_variable_that_precedes_it() {
        let got = joined("Q(X, Y, Z) :- P(X, Z), S(Y, Z).", &["X", "Y", "Z"]);
        // X = 0, Y = 1, Z = 2; V′ = [X, Z, Y].
        assert_eq!(got, expected(&[0, 1], &[0, 1], &[0, 2, 1]));
    }

    /// Worked example 3: atoms that share nothing join as a cross product,
    /// each with an empty key, and `V′` groups each atom's variables.
    #[test]
    fn atoms_that_share_nothing_join_as_a_cross_product() {
        let got = joined("Q(A, B, C, D) :- R(A, B), S(C, D).", &["A", "C", "B", "D"]);
        assert_eq!(got, expected(&[0, 1], &[0, 0], &[0, 1, 2, 3]));
    }

    /// The rule follows `V`: with `D` first among the unbound variables, `T`
    /// (which carries it, and shares nothing bound) beats `S` (which shares
    /// `B` but does not carry `D`), so `V` forces a cross product, as it does
    /// for LFTJ. `S` is then probed as a filter on both its variables.
    #[test]
    fn a_connected_atom_without_the_next_variable_loses_to_an_unconnected_one_with_it() {
        let got = joined("Q(A, B, C, D) :- R(A, B), S(B, C), T(D, C).", &[
            "A", "D", "B", "C",
        ]);
        // A = 0, B = 1, C = 2, D = 3; V′ = [A, B, D, C].
        assert_eq!(got, expected(&[0, 2, 1], &[0, 0, 2], &[0, 1, 3, 2]));
    }

    /// Among the atoms that carry the next variable, the one with the most
    /// bound variables wins over an earlier one with fewer.
    #[test]
    fn the_atom_with_the_most_bound_variables_wins_over_an_earlier_one() {
        let got = joined(
            "Q(A, B, C, D) :- R(A, B), S(C, D), T(C, B, D).",
            &["A", "B", "C", "D"],
        );
        // After R, C is next: S has no bound variable, T has B.
        assert_eq!(got, expected(&[0, 2, 1], &[0, 1, 2], &[0, 1, 2, 3]));
    }

    /// An atom whose variables are all bound is probed at once, in body
    /// order, before the next unbound variable picks its atom.
    #[test]
    fn a_complete_atom_is_probed_as_soon_as_its_key_is_bound() {
        let got = joined("Q(X, Y, Z) :- R(X, Y), S(Z), T(X), U(Y, Z).", &[
            "X", "Y", "Z",
        ]);
        // R drives and binds X, Y, so T is probed at once; Z then goes to U
        // (Y is bound) rather than S, and S is probed last.
        assert_eq!(got, expected(&[0, 2, 3, 1], &[0, 1, 1, 1], &[0, 1, 2]));
    }

    #[test]
    fn a_query_with_no_accesses_has_the_empty_order() {
        let mut query: JoinQuery = "Q(c9) :- r0(c1, c2), s(c3).".parse().expect("parse");
        query.head.terms.clear();
        let accesses = crate::extract_accesses(&crate::rewrite_placeholders(query)).expect("extract");
        assert!(accesses.accesses.is_empty());
        let plan = QueryPlan {
            variable_ordering: vec![],
        };
        assert_eq!(join_order(&accesses.query, &plan), expected(&[], &[], &[]));
    }

    /// Checks the properties of the rule on one result, without re-running
    /// the rule: it reads `ordered` back against `query` and `order`.
    fn check(context: &str, query: &JoinQuery, order: &[usize], ordered: &JoinOrder) {
        let vars = analyse(query).predicate_variables;
        let prime = &ordered.plan.variable_ordering;

        // V′ is a permutation, and each atom joins once.
        let mut sorted = prime.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            (0..order.len()).collect::<Vec<_>>(),
            "{context}: V′ is a permutation"
        );
        let mut once = ordered.atoms.clone();
        once.sort_unstable();
        assert_eq!(
            once,
            (0..vars.len()).collect::<Vec<_>>(),
            "{context}: each atom joins once"
        );
        assert_eq!(ordered.key_lens.len(), ordered.atoms.len(), "{context}");

        let position_in = |ordering: &[usize]| -> Vec<usize> {
            let mut position = vec![0; ordering.len()];
            for (at, &variable) in ordering.iter().enumerate() {
                position[variable] = at;
            }
            position
        };
        let in_prime = position_in(prime);
        let in_order = position_in(order);

        let mut bound: HashSet<usize> = HashSet::new();
        let mut rebuilt: Vec<usize> = Vec::new();
        let mut previous_filter: Option<usize> = None;
        for (rank, &atom) in ordered.atoms.iter().enumerate() {
            // The key is a prefix of the atom's V′-ordered variables, and it
            // is exactly the variables an earlier atom bound.
            let mut by_prime = vars[atom].clone();
            by_prime.sort_by_key(|&v| in_prime[v]);
            let key: Vec<usize> = by_prime
                .iter()
                .copied()
                .filter(|v| bound.contains(v))
                .collect();
            assert_eq!(
                key.len(),
                ordered.key_lens[rank],
                "{context}: the key length of atom {atom}"
            );
            assert_eq!(
                by_prime[..key.len()],
                key[..],
                "{context}: the key of atom {atom} leads its columns"
            );
            let fresh = &by_prime[key.len()..];
            assert!(
                fresh.windows(2).all(|pair| in_order[pair[0]] < in_order[pair[1]]),
                "{context}: the new variables of atom {atom} are in V order"
            );
            rebuilt.extend_from_slice(fresh);

            if fresh.is_empty() {
                // A pure filter: filters that follow one another are in body
                // order.
                if let Some(previous) = previous_filter {
                    assert!(previous < atom, "{context}: filters are in body order");
                }
                previous_filter = Some(atom);
            } else {
                previous_filter = None;
                // A driving join: it carries the first unbound variable of
                // V, no unjoined atom is complete (it would have been probed
                // already), and no unjoined atom that carries that variable
                // has more bound variables, or as many and an earlier place
                // in the body.
                let u = order
                    .iter()
                    .copied()
                    .find(|v| !bound.contains(v))
                    .expect("an unbound variable");
                assert!(vars[atom].contains(&u), "{context}: atom {atom} carries {u}");
                let bound_in = |a: usize| vars[a].iter().filter(|v| bound.contains(v)).count();
                for &other in &ordered.atoms[rank + 1..] {
                    assert!(
                        !vars[other].iter().all(|v| bound.contains(v)),
                        "{context}: atom {other} was complete before atom {atom} joined"
                    );
                    if vars[other].contains(&u) {
                        let (mine, theirs) = (bound_in(atom), bound_in(other));
                        assert!(
                            theirs < mine || (theirs == mine && atom < other),
                            "{context}: atom {other} beats atom {atom} on variable {u}"
                        );
                    }
                }
            }
            bound.extend(vars[atom].iter().copied());
        }
        assert_eq!(
            &rebuilt, prime,
            "{context}: V′ is each atom's new variables, in join order"
        );
    }

    /// The properties on the 16 suite shapes under every variable order
    /// (the number of orders is the sum of `n!` over their variable counts:
    /// 1 + 6 + 24 + 6 + 6 + 1 + 6 + 2 + 120 + 6 + 6 + 24 + 1 + 2 + 2 + 6).
    #[test]
    fn the_properties_hold_on_every_suite_shape_under_every_order() {
        let mut checked = 0usize;
        for (name, text) in QUERIES {
            let accesses = accesses_of(text);
            let query = &accesses.query;
            for order in permutations(analyse(query).num_vars) {
                let plan = QueryPlan {
                    variable_ordering: order.clone(),
                };
                let ordered = join_order(query, &plan);
                check(&format!("{name} under {order:?}"), query, &order, &ordered);
                checked += 1;
            }
        }
        assert_eq!(checked, 219);
    }
}
```

Register the module and mention it in the crate docs. In `kermit-algos/src/lib.rs`, replace

```rust
//! The crate is organised in seven parts. Four are family-agnostic and sit
```

with

```rust
//! The crate is organised in eight parts. Five are family-agnostic and sit
```

replace

```rust
//! canonical variable numbering that planners and executors must agree on;
//! and `index_plan`, which runs after planning, orders each access's columns
//! by the plan and merges identical accesses into the indexes a query
//! builds. `optimiser` plans a [`QueryPlan`]. The remaining two are the
```

with

```rust
//! canonical variable numbering that planners and executors must agree on;
//! `join_order`, which reads a plan as a left-deep join order; and
//! `index_plan`, which runs after planning, orders each access's columns
//! by the plan and merges identical accesses into the indexes a query
//! builds. `optimiser` plans a [`QueryPlan`]. The remaining two are the
```

and replace

```rust
mod index_plan;
mod join_algo;
mod optimiser;
```

with

```rust
mod index_plan;
mod join_algo;
mod join_order;
mod optimiser;
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit-algos --lib join_order::`

Expected: the crate compiles (unused-import warnings from the stub are fine) and all eight tests fail on the stub, each printing `not yet implemented: Task 1.1, Step 3 replaces this stub`; the summary line is `test result: FAILED. 0 passed; 8 failed`.

- [ ] **Step 3: Implement**

In `kermit-algos/src/join_order.rs`, replace the import line

```rust
use {crate::optimiser::QueryPlan, kermit_parser::JoinQuery};
```

with

```rust
use {
    crate::{analysis::analyse, optimiser::QueryPlan},
    kermit_parser::JoinQuery,
    std::cmp::Reverse,
};
```

and replace the stub

```rust
pub(crate) fn join_order(_query: &JoinQuery, _plan: &QueryPlan) -> JoinOrder {
    todo!("Task 1.1, Step 3 replaces this stub")
}
```

with

```rust
pub(crate) fn join_order(query: &JoinQuery, plan: &QueryPlan) -> JoinOrder {
    let analysis = analyse(query);
    let atom_vars = &analysis.predicate_variables;
    let atom_count = atom_vars.len();
    let mut joiner = Joiner {
        plan: &plan.variable_ordering,
        atom_vars,
        bound: vec![false; analysis.num_vars],
        joined: vec![false; atom_count],
        atoms: Vec::with_capacity(atom_count),
        key_lens: Vec::with_capacity(atom_count),
        ordering: Vec::with_capacity(analysis.num_vars),
    };
    while joiner.atoms.len() < atom_count {
        // Every unjoined atom has an unbound variable (one with none was
        // joined as a filter), so the plan names one.
        let u = plan
            .variable_ordering
            .iter()
            .copied()
            .find(|&v| !joiner.bound[v])
            .expect("join_order: an unjoined atom has an unbound variable of the plan");
        let driver = (0..atom_count)
            .filter(|&a| !joiner.joined[a] && atom_vars[a].contains(&u))
            .min_by_key(|&a| (Reverse(joiner.bound_in(a)), a))
            .expect("join_order: some unjoined atom carries the plan's next variable");
        joiner.join(driver);
        for a in 0..atom_count {
            if !joiner.joined[a] && atom_vars[a].iter().all(|&v| joiner.bound[v]) {
                joiner.join(a);
            }
        }
    }
    assert_eq!(
        joiner.ordering.len(),
        plan.variable_ordering.len(),
        "join_order: the plan names a variable that no atom carries"
    );
    JoinOrder {
        atoms: joiner.atoms,
        key_lens: joiner.key_lens,
        plan: QueryPlan {
            variable_ordering: joiner.ordering,
        },
    }
}

/// The joins made so far: the atoms in join order with their key lengths,
/// and the variables they bound, in the order they bound them (`V′`).
struct Joiner<'a> {
    /// The plan `V`, in descent order.
    plan: &'a [usize],
    /// The canonical variables of each atom.
    atom_vars: &'a [Vec<usize>],
    /// `bound[v]`: whether a joined atom carries variable `v`.
    bound: Vec<bool>,
    /// `joined[a]`: whether atom `a` is in `atoms`.
    joined: Vec<bool>,
    atoms: Vec<usize>,
    key_lens: Vec<usize>,
    ordering: Vec<usize>,
}

impl<'a> Joiner<'a> {
    /// How many variables of `atom` are bound.
    fn bound_in(&self, atom: usize) -> usize {
        self.atom_vars[atom]
            .iter()
            .filter(|&&v| self.bound[v])
            .count()
    }

    /// Joins `atom`: its key is the variables already bound, and it binds
    /// the rest, in the order of `V`.
    fn join(&mut self, atom: usize) {
        let (plan, atom_vars): (&'a [usize], &'a [Vec<usize>]) = (self.plan, self.atom_vars);
        let vars = &atom_vars[atom];
        let key_len = self.bound_in(atom);
        self.key_lens.push(key_len);
        for &v in plan {
            if vars.contains(&v) && !self.bound[v] {
                self.bound[v] = true;
                self.ordering.push(v);
            }
        }
        self.atoms.push(atom);
        self.joined[atom] = true;
    }
}
```

The module still carries the `#![allow(dead_code)]` line until Task 1.2 adds the first caller.

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt --all`, then `nix develop --command cargo test -p kermit-algos --lib join_order::`

Expected: `test result: ok. 8 passed; 0 failed`.

Then run `nix develop --command cargo clippy -p kermit-algos --all-targets -- -D warnings`, which must print no warning.

- [ ] **Step 5: Commit**

```bash
git add -N kermit-algos/src/join_order.rs
git commit -m "feat(algos): join order of a variable order, for binary join planning" -- kermit-algos/src/join_order.rs kermit-algos/src/lib.rs
```

### Task 1.2: `TrieLevels::ProbeKeys` and its planning branch

**Files:**
- Modify: `kermit-iters/src/trie_levels.rs:16-18`
- Modify: `kermit-algos/src/index_plan.rs:16-25`, `:43-48`, `:66-72`, `:77-126`, and the end of the tests (after `path3_shares_an_index_under_all_but_not_under_join_keys`)
- Modify: `kermit-algos/src/join_order.rs` (drops its temporary `allow`)
- Test: `kermit-iters/src/trie_levels.rs`, `kermit-algos/src/index_plan.rs`

- [ ] **Step 1: Write the failing tests**

In `kermit-iters/src/trie_levels.rs`, after the enum, add a test module. Replace the last lines of the file

```rust
    JoinKeys,
}
```

with

```rust
    JoinKeys,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `bench run` keys its prepared queries by policy (a `BTreeMap`), so the
    /// declaration order is the iteration order: `ProbeKeys` is the third.
    #[test]
    fn the_policies_are_ordered_all_then_join_keys_then_probe_keys() {
        assert_eq!(TrieLevels::default(), TrieLevels::All);
        assert!(TrieLevels::All < TrieLevels::JoinKeys);
        assert!(TrieLevels::JoinKeys < TrieLevels::ProbeKeys);
    }
}
```

In `kermit-algos/src/index_plan.rs`, append these tests to the `tests` module. Replace the end of the module

```rust
        assert_eq!(body_text(&keys.query), vec![
            "#0(B, A)", "#1(B, C)", "#2(C, D)"
        ]);
    }
}
```

with

```rust
        assert_eq!(body_text(&keys.query), vec![
            "#0(B, A)", "#1(B, C)", "#2(C, D)"
        ]);
    }

    fn probe(text: &str, order: &[&str]) -> IndexPlan {
        planned_with(text, order, TrieLevels::ProbeKeys)
    }

    fn relations_of(plan: &IndexPlan) -> Vec<&str> {
        plan.indexes
            .iter()
            .map(|spec| spec.relation.as_str())
            .collect()
    }

    #[test]
    fn under_probe_keys_each_triangle_index_levels_its_key() {
        let out = probe("Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).", &["X", "Y", "Z"]);
        assert_eq!(relations_of(&out), vec!["R", "S", "T"]);
        assert_eq!(levels_of(&out), vec![
            (vec![0, 1], 0),
            (vec![0, 1], 1),
            (vec![0, 1], 2)
        ]);
        assert_eq!(body_text(&out.query), vec![
            "#0(X, Y)", "#1(Y, Z)", "#2(X, Z)"
        ]);
        assert_eq!(out.plan.variable_ordering, vec![0, 1, 2]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    /// The body is listed in join order, so the specs are numbered by it and
    /// `#0` is the driver: `T` (which carries `C` and has `B` bound) joins
    /// before `S`, and its columns follow `V′`.
    #[test]
    fn under_probe_keys_the_body_is_in_join_order_with_the_driver_first() {
        let out = probe("Q(A, B, C, D) :- R(A, B), S(C, D), T(C, B, D).", &[
            "A", "B", "C", "D",
        ]);
        assert_eq!(relations_of(&out), vec!["R", "T", "S"]);
        // T(C, B, D) is read B, C, D: source columns 1, 0, 2.
        assert_eq!(levels_of(&out), vec![
            (vec![0, 1], 0),
            (vec![1, 0, 2], 1),
            (vec![0, 1], 2)
        ]);
        assert_eq!(body_text(&out.query), vec![
            "#0(A, B)",
            "#1(B, C, D)",
            "#2(C, D)"
        ]);
        assert_eq!(out.plan.variable_ordering, vec![0, 1, 2, 3]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    /// The driver is the atom that binds the plan's first variable, not the
    /// first atom of the body.
    #[test]
    fn under_probe_keys_the_driver_binds_the_plans_first_variable() {
        let out = probe("Q(X, Y) :- R(Y), S(X, Y).", &["X", "Y"]);
        assert_eq!(relations_of(&out), vec!["S", "R"]);
        assert_eq!(levels_of(&out), vec![(vec![0, 1], 0), (vec![0], 1)]);
        assert_eq!(body_text(&out.query), vec!["#0(X, Y)", "#1(Y)"]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    /// `S(Y, Z)` is probed on `Z`, which `P(X, Z)` bound before `Y`: its
    /// columns become `(Z, Y)` and the returned plan is `V′ = [X, Z, Y]`, in
    /// the executed query's numbering (`X` = 0, `Y` = 1, `Z` = 2). Under `All`
    /// and `JoinKeys` the same query keeps its body and its plan as given.
    #[test]
    fn under_probe_keys_the_returned_plan_is_the_reordered_one() {
        let text = "Q(X, Y, Z) :- P(X, Z), S(Y, Z).";
        let order = ["X", "Y", "Z"];
        let out = probe(text, &order);
        assert_eq!(levels_of(&out), vec![(vec![0, 1], 0), (vec![1, 0], 1)]);
        assert_eq!(body_text(&out.query), vec!["#0(X, Z)", "#1(Z, Y)"]);
        assert_eq!(out.plan.variable_ordering, vec![0, 2, 1]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));

        for levels in [TrieLevels::All, TrieLevels::JoinKeys] {
            let out = planned_with(text, &order, levels);
            assert_eq!(body_text(&out.query), vec!["#0(X, Z)", "#1(Y, Z)"], "{levels:?}");
            assert_eq!(out.plan.variable_ordering, vec![0, 1, 2], "{levels:?}");
        }
        let all = planned(text, &order);
        assert_eq!(levels_of(&all), vec![(vec![0, 1], 2), (vec![0, 1], 2)]);
    }

    /// Atoms that share nothing join as a cross product, each with an empty
    /// key and so no level.
    #[test]
    fn under_probe_keys_a_cross_product_levels_nothing() {
        let out = probe("Q(A, B, C, D) :- R(A, B), S(C, D).", &["A", "C", "B", "D"]);
        assert_eq!(levels_of(&out), vec![(vec![0, 1], 0), (vec![0, 1], 0)]);
        assert_eq!(body_text(&out.query), vec!["#0(A, B)", "#1(C, D)"]);
        assert_eq!(out.plan.variable_ordering, vec![0, 1, 2, 3]);
        assert_eq!(out.plan.validate(&analyse(&out.query)), Ok(()));
    }

    /// A spec's levels are part of the comparison: the second read of `edge`
    /// is a filter on both columns, the first the driver, so they are two
    /// indexes under `ProbeKeys` and one under `All`.
    #[test]
    fn specs_that_differ_only_in_levels_stay_apart_under_probe_keys() {
        let text = "Q(X, Y) :- edge(X, Y), edge(X, Y).";
        let out = probe(text, &["X", "Y"]);
        assert_eq!(levels_of(&out), vec![(vec![0, 1], 0), (vec![0, 1], 2)]);
        assert_eq!(body_text(&out.query), vec!["#0(X, Y)", "#1(X, Y)"]);
        assert_eq!(planned(text, &["X", "Y"]).indexes.len(), 1);
    }

    #[test]
    fn a_query_with_no_accesses_plans_no_indexes_under_probe_keys() {
        let mut query: JoinQuery = "Q(c9) :- r(c1, c2), s(c3).".parse().expect("parse");
        query.head.terms.clear();
        let accesses = extract_accesses(&rewrite_placeholders(query)).expect("extract");
        let plan = QueryPlan {
            variable_ordering: vec![],
        };
        let out = plan_indexes(&accesses, &plan, TrieLevels::ProbeKeys);
        assert!(out.indexes.is_empty());
        assert!(out.query.body.is_empty());
        assert!(out.plan.variable_ordering.is_empty());
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit-iters -p kermit-algos`

Expected: compilation fails with `error[E0599]: no variant or associated item named `ProbeKeys` found for enum `TrieLevels` in the current scope`, once in `kermit-iters` (its test module) and in `kermit-algos` for every use in `index_plan.rs`.

- [ ] **Step 3: Implement**

1. `kermit-iters/src/trie_levels.rs`: replace

```rust
    JoinKeys,
}

#[cfg(test)]
```

with

```rust
    JoinKeys,
    /// Levels for the columns earlier atoms of the binary join order bind: the
    /// key each index is probed on. Planning under it also reorders the plan
    /// and the executed query's body into join order (`plan_indexes`).
    /// `HashIndex` is planned under it.
    ProbeKeys,
}

#[cfg(test)]
```

2. `kermit-algos/src/join_order.rs`: delete the two lines

```rust
// Temporary (Task 1.2 removes this line): `plan_indexes` is the first caller.
#![allow(dead_code)]
```

and the blank line that follows them, leaving one blank line between the module doc and the `use`.

3. `kermit-algos/src/index_plan.rs`, imports: replace

```rust
        analysis::{analyse, canonical_names},
        optimiser::QueryPlan,
```

with

```rust
        analysis::{analyse, canonical_names},
        join_order::join_order,
        optimiser::QueryPlan,
```

4. `IndexSpec::levels`'s doc: replace

```rust
    /// being payload kept in its leaf rows (#116): `columns.len()` under
    /// [`TrieLevels::All`], fewer only under [`TrieLevels::JoinKeys`].
    /// Identical specs merge, so two specs that differ only here are two
    /// indexes.
```

with

```rust
    /// being payload kept in its leaf rows (#116): `columns.len()` under
    /// [`TrieLevels::All`], fewer under [`TrieLevels::JoinKeys`], and under
    /// [`TrieLevels::ProbeKeys`] the access's key length, the columns an
    /// earlier atom of the join order binds (`0` for the driver).
    /// Identical specs merge, so two specs that differ only here are two
    /// indexes.
```

5. `IndexPlan`'s docs: replace

```rust
    /// The query to execute: the head unchanged, and `body[j]` the atom
    /// `#{id}` of the index the `j`-th access reads, over its variables in
    /// plan order.
    pub query: JoinQuery,
    /// The plan, translated to `query`'s variable numbering; it validates
    /// against `query`.
```

with

```rust
    /// The query to execute: the head unchanged, and `body[j]` the atom
    /// `#{id}` of the index the `j`-th access reads, over its variables in
    /// plan order. Under [`TrieLevels::ProbeKeys`] the body is in join
    /// order instead: `body[j]` reads the `j`-th atom of the join order,
    /// and `body[0]` is the driver.
    pub query: JoinQuery,
    /// The plan, translated to `query`'s variable numbering; it validates
    /// against `query`. Under [`TrieLevels::ProbeKeys`] it is the reordered
    /// plan `V′` of the join order, the order the joins bind in.
```

6. `plan_indexes`'s docs: replace

```rust
/// number of join-key columns, since an index's columns follow the plan.
///
/// # Panics
```

with

```rust
/// number of join-key columns, since an index's columns follow the plan.
///
/// Under [`TrieLevels::ProbeKeys`] the plan is first read as a left-deep
/// join order (`join_order`): the columns follow the plan `V′` it returns,
/// each spec's level count is the access's key length (the variables an
/// earlier atom of the join order binds, `0` for the driver), and the
/// accesses are visited in join order, so the spec ids and the executed
/// body follow it, `#0` being the driver. The returned plan is `V′`.
///
/// # Panics
```

7. `plan_indexes`'s body: replace

```rust
    let source = &accesses.query;
    let names = canonical_names(source);
    let plan_names: Vec<&str> = plan
        .variable_ordering
        .iter()
        .map(|&v| names[v].as_str())
        .collect();
```

with

```rust
    let source = &accesses.query;
    let names = canonical_names(source);
    // Under `ProbeKeys` the plan is read as a join order: the columns follow
    // its `V′`, the accesses are visited in join order, and each index
    // levels its probe key.
    let join = matches!(levels, TrieLevels::ProbeKeys).then(|| join_order(source, plan));
    let ordering = join
        .as_ref()
        .map_or(&plan.variable_ordering, |join| &join.plan.variable_ordering);
    let visit: Vec<usize> = match &join {
        | Some(join) => join.atoms.clone(),
        | None => (0..accesses.accesses.len()).collect(),
    };
    let plan_names: Vec<&str> = ordering.iter().map(|&v| names[v].as_str()).collect();
```

replace

```rust
    for access in &accesses.accesses {
```

with

```rust
    for (rank, &at) in visit.iter().enumerate() {
        let access = &accesses.accesses[at];
```

and replace

```rust
                .rposition(|&kept| is_join_key[access.variables[kept].as_str()])
                .map_or(0, |last| last + 1),
        };
```

with

```rust
                .rposition(|&kept| is_join_key[access.variables[kept].as_str()])
                .map_or(0, |last| last + 1),
            | TrieLevels::ProbeKeys => {
                join.as_ref()
                    .expect("ProbeKeys computes the join order")
                    .key_lens[rank]
            },
        };
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt --all`, then `nix develop --command cargo test -p kermit-iters -p kermit-algos`

Expected: `test result: ok` for every target; among them the new `trie_levels::tests::the_policies_are_ordered_all_then_join_keys_then_probe_keys`, the five `under_probe_keys_*`, `specs_that_differ_only_in_levels_stay_apart_under_probe_keys` and `a_query_with_no_accesses_plans_no_indexes_under_probe_keys` tests of `index_plan`, and every earlier test unchanged.

Then run `nix develop --command cargo check --workspace --all-targets`, which proves no other crate matches `TrieLevels` exhaustively, and `nix develop --command cargo clippy -p kermit-iters -p kermit-algos --all-targets -- -D warnings`. Both must print no error or warning.

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(algos): plan indexes under TrieLevels::ProbeKeys, in join order" -- kermit-iters/src/trie_levels.rs kermit-algos/src/index_plan.rs kermit-algos/src/join_order.rs
```

### Task 1.3: The property sweep of `ProbeKeys` planning

**Files:**
- Modify: `kermit-algos/src/index_plan.rs` (the test module's imports and its end)
- Test: `kermit-algos/src/index_plan.rs`

- [ ] **Step 1: Write the test**

In the `tests` module of `kermit-algos/src/index_plan.rs`, replace the imports

```rust
    use {
        super::*,
        crate::{analyse, canonical_names, extract_accesses, rewrite_placeholders},
    };
```

with

```rust
    use {
        super::*,
        crate::{
            analyse, canonical_names, extract_accesses,
            join_order::suite::{permutations, QUERIES},
            rewrite_placeholders,
        },
        std::collections::HashSet,
    };
```

and replace the end of the module (the last test of Task 1.2)

```rust
        let out = plan_indexes(&accesses, &plan, TrieLevels::ProbeKeys);
        assert!(out.indexes.is_empty());
        assert!(out.query.body.is_empty());
        assert!(out.plan.variable_ordering.is_empty());
    }
}
```

with

```rust
        let out = plan_indexes(&accesses, &plan, TrieLevels::ProbeKeys);
        assert!(out.indexes.is_empty());
        assert!(out.query.body.is_empty());
        assert!(out.plan.variable_ordering.is_empty());
    }

    /// The variables of an executed atom, in column order.
    fn variables_of(atom: &Predicate) -> Vec<String> {
        atom.terms
            .iter()
            .map(|term| match term {
                | Term::Var(name) => name.clone(),
                | _ => panic!("an executed atom holds variables only"),
            })
            .collect()
    }

    /// On the 16 suite shapes under every variable order, `ProbeKeys`
    /// planning follows the join order of `join_order`: the body lists the
    /// atoms in join order with `#0` the driver, each spec levels exactly its
    /// key, which leads the atom's columns, spec ids follow first appearance,
    /// and the returned plan validates against the executed query. The
    /// count is the sum of `n!` over the shapes' variable counts, as in
    /// `join_order`'s own sweep.
    #[test]
    fn under_probe_keys_every_suite_shape_under_every_order_plans_its_join_order() {
        let mut checked = 0usize;
        for (name, text) in QUERIES {
            let accesses = accesses_of(text);
            for order in permutations(analyse(&accesses.query).num_vars) {
                let context = format!("{name} under {order:?}");
                let plan = QueryPlan {
                    variable_ordering: order,
                };
                let join = join_order(&accesses.query, &plan);
                let out = plan_indexes(&accesses, &plan, TrieLevels::ProbeKeys);

                assert_eq!(
                    out.plan.validate(&analyse(&out.query)),
                    Ok(()),
                    "{context}: the plan validates"
                );
                assert_eq!(out.query.head, accesses.query.head, "{context}");
                assert_eq!(out.query.body.len(), join.atoms.len(), "{context}");
                assert_eq!(out.query.body[0].name, "#0", "{context}: #0 is the driver");
                assert_eq!(out.indexes[0].levels, 0, "{context}: the driver has no key");

                let mut bound: HashSet<String> = HashSet::new();
                let mut next_id = 0;
                for (rank, atom) in out.query.body.iter().enumerate() {
                    let read = &accesses.accesses[join.atoms[rank]];
                    let id: usize = atom
                        .name
                        .strip_prefix(INDEX_ID_PREFIX)
                        .and_then(|id| id.parse().ok())
                        .expect("an index id");
                    assert!(id <= next_id, "{context}: ids follow first appearance");
                    next_id = next_id.max(id + 1);

                    let spec = &out.indexes[id];
                    assert_eq!(spec.relation, read.relation, "{context}: atom {rank}");
                    assert_eq!(spec.levels, join.key_lens[rank], "{context}: atom {rank}");

                    let terms = variables_of(atom);
                    assert_eq!(
                        terms.iter().collect::<HashSet<_>>(),
                        read.variables.iter().collect::<HashSet<_>>(),
                        "{context}: atom {rank} keeps its variables"
                    );
                    let (key, rest) = terms.split_at(spec.levels);
                    assert!(
                        key.iter().all(|v| bound.contains(v)),
                        "{context}: the key of atom {rank} is bound"
                    );
                    assert!(
                        rest.iter().all(|v| !bound.contains(v)),
                        "{context}: atom {rank} binds only what follows its key"
                    );
                    bound.extend(terms);
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 219);
    }
}
```

- [ ] **Step 2: Run it to see it fail**

This test pins behaviour that Task 1.2 implemented, so it passes as written. Prove it can fail by breaking the level count: in `plan_indexes`, temporarily replace

```rust
                    .key_lens[rank]
```

with

```rust
                    .key_lens[0]
```

Run: `nix develop --command cargo test -p kermit-algos --lib under_probe_keys_every_suite_shape`

Expected: FAILED, with a panic whose left and right differ, printed with a context such as `triangle under [0, 1, 2]: atom 1` (the spec's `levels` is `0`, the join's key length is `1`).

Restore the line with the reverse edit (`.key_lens[0]` back to `.key_lens[rank]`; do not use `git checkout`, which would also discard this task's uncommitted test), and check with `git diff -- kermit-algos/src/index_plan.rs | grep -c 'key_lens\[0\]'`, which must print `0`.

- [ ] **Step 3: Implement**

No implementation change: the behaviour is Task 1.2's. This task adds the test only.

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt --all`, then `nix develop --command cargo test -p kermit-iters -p kermit-algos`

Expected: `test result: ok` for every target, including `under_probe_keys_every_suite_shape_under_every_order_plans_its_join_order` and the eight tests of `join_order`.

Then run `nix develop --command cargo clippy -p kermit-iters -p kermit-algos --all-targets -- -D warnings` and `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit-iters -p kermit-algos'`; both must print no warning.

- [ ] **Step 5: Commit**

```bash
git commit -m "test(algos): ProbeKeys planning follows the join order on every suite shape and order" -- kermit-algos/src/index_plan.rs
```

## Story 2: `KeyedIterable` and `HashIndex`

- **Developer:** intermediate-dev
- **Files:** `kermit-iters/src/keyed.rs`, `kermit-iters/src/lib.rs`, `kermit-ds/src/ds/hash_index/mod.rs`, `kermit-ds/src/ds/hash_index/implementation.rs`, `kermit-ds/src/ds/hash_index/hash_index_iter.rs`, `kermit-ds/src/ds/hash_index/tests.rs`, `kermit-ds/src/ds/mod.rs`, `kermit-ds/src/lib.rs`, `kermit-ds/src/relation.rs`, `kermit-ds/tests/common/macros.rs`, `kermit-ds/tests/hash_index_tests.rs`, `docs/data-structures/hash-index.md`
- **Interfaces:** exposes `KeyedIterable`, `HashIndex<H>` and `keyed_index_test_suite!` (names contract, Story 2) to Stories 3–4; uses `TrieLevels::ProbeKeys` from Story 1
- **Verify:** `nix develop --command cargo test -p kermit-iters -p kermit-ds`
- **Review:** deep

This story adds the third iterator family's trait and its structure. `KeyedIterable` (in `kermit-iters`) is an index keyed on a column prefix and probed by exact match: `scan` lends every row, `probe(key)` lends the rows whose first `key_len()` values equal `key`, both in source order and allocating nothing. `HashIndex<H: HashStrategy = SipHashStrategy>` (in `kermit-ds`) implements it as Free Join §2.2's join hash table: the view's rows copied once, a power-of-two directory of `RowId` bucket heads (at least 2 slots), and one `RowId` link per row, both plain `Vec<RowId>` with `RowId::MAX` as the NIL sentinel. It does not use `ChainLinks`. The key hash folds `H::hash` with FxHash's combine step and picks the bucket by Fibonacci hashing; rows are linked last to first, so every chain is in source order, and `probe` filters each chain by comparing the key columns. `Relation::TRIE_LEVELS` is `TrieLevels::ProbeKeys` (Story 1) and `KEEPS_DUPLICATES` is `true`. The `IndexStructure::HashIndex` registry variant is Story 4's, so the module is registered in `kermit-ds/src/ds/mod.rs` and re-exported, and nothing else changes in the binary. No task touches `HashTrie`, `TreeTrie` or `ColumnTrie`. Tests: in-crate unit tests (sizing, pinned placement, chain order, insert against the bulk build, the view-build equivalence), `keyed_index_test_suite!` invoked for `HashIndex<SipHashStrategy>`, `HashIndex<FxHashStrategy>` and a test-local one-bucket strategy (so the key comparison alone must filter), `parquet_test_suite!` with a scan collector, and HashIndex rows in `relation.rs`'s cross-structure view tests. The structure's page closes the story. The join-suite cells (`join_tests.rs`, `all_orders.rs` and the allocation and LUBM tests) are Story 3's, once an algorithm drives the index.

### Task 2.1: The `KeyedIterable` trait

**Files:**
- Create: `kermit-iters/src/keyed.rs`
- Modify: `kermit-iters/src/lib.rs:20-24`, `kermit-iters/src/lib.rs:53-54`, `kermit-iters/src/lib.rs:67-68`

- [ ] **Step 1: Write the failing test**

Create `kermit-iters/src/keyed.rs` holding only a test module: the smallest implementation of the contract (a `Vec` of rows probed by a filtering scan) and a generic driver, which is how Story 3's algorithm will use the trait.

```rust
#[cfg(test)]
mod tests {
    use crate::{JoinIterable, KeyedIterable};

    /// The smallest keyed index: the rows in a `Vec`, probed by a filtering
    /// scan. It pins the trait's shape, not any structure's behaviour.
    struct Scanned {
        key_len: usize,
        rows: Vec<Vec<usize>>,
    }

    impl JoinIterable for Scanned {}

    impl KeyedIterable for Scanned {
        fn key_len(&self) -> usize { self.key_len }

        fn scan(&self) -> impl Iterator<Item = &[usize]> + '_ {
            self.rows.iter().map(Vec::as_slice)
        }

        fn probe<'a>(&'a self, key: &'a [usize]) -> impl Iterator<Item = &'a [usize]> + 'a {
            assert_eq!(key.len(), self.key_len, "probe: a key of the wrong length");
            self.scan().filter(move |row| row.starts_with(key))
        }
    }

    fn fixture(key_len: usize) -> Scanned {
        Scanned {
            key_len,
            rows: vec![vec![1, 10], vec![2, 20], vec![1, 11]],
        }
    }

    /// What an algorithm does with the trait: only bounds, no structure.
    fn probed<I: KeyedIterable>(index: &I, key: &[usize]) -> Vec<Vec<usize>> {
        index.probe(key).map(<[usize]>::to_vec).collect()
    }

    fn scanned<I: KeyedIterable>(index: &I) -> Vec<Vec<usize>> {
        index.scan().map(<[usize]>::to_vec).collect()
    }

    #[test]
    fn probe_lends_the_rows_of_a_key_in_scan_order() {
        let index = fixture(1);
        assert_eq!(index.key_len(), 1);
        assert_eq!(probed(&index, &[1]), vec![vec![1, 10], vec![1, 11]]);
        assert_eq!(probed(&index, &[2]), vec![vec![2, 20]]);
        assert!(probed(&index, &[3]).is_empty());
    }

    #[test]
    fn a_key_of_no_columns_lends_every_row() {
        let index = fixture(0);
        assert_eq!(index.key_len(), 0);
        assert_eq!(probed(&index, &[]), scanned(&index));
        assert_eq!(scanned(&index).len(), 3);
    }
}
```

Register the module and the re-export in `kermit-iters/src/lib.rs`.

Current (lines 53-54):

```rust
mod key_type;
mod leaf_rows;
```

Replace with:

```rust
mod key_type;
mod keyed;
mod leaf_rows;
```

Current (lines 67-68):

```rust
    key_type::Key,
    leaf_rows::{LeafCursor, LeafRows},
```

Replace with:

```rust
    key_type::Key,
    keyed::KeyedIterable,
    leaf_rows::{LeafCursor, LeafRows},
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit-iters --lib keyed`

Expected: the build fails, because `keyed.rs` defines no `KeyedIterable` yet:

```
error[E0432]: unresolved import `keyed::KeyedIterable`
  --> kermit-iters/src/lib.rs:..
   |     keyed::KeyedIterable,
   |     ^^^^^^^^^^^^^^^^^^^^ no `KeyedIterable` in `keyed`
```

(The same name is unresolved in the test module's `use crate::{JoinIterable, KeyedIterable}`.)

- [ ] **Step 3: Implement**

Insert at the top of `kermit-iters/src/keyed.rs`, above `#[cfg(test)]`, the family's rationale and the trait:

```rust
//! Keyed index iterator family — counterpart to
//! [`TrieIterator`](crate::TrieIterator) and
//! [`HashTrieIterator`](crate::HashTrieIterator) for binary hash joins.
//!
//! A keyed index stores one relation's rows under a key, a prefix of their
//! columns, and is probed by exact match: [`probe`](KeyedIterable::probe)
//! lends every row whose key columns equal a given key. It is the build side
//! of a hash join. A left-deep plan `[R1, …, Rm]` is evaluated by pipelining:
//! the first relation is scanned, and every later one is a hash table keyed on
//! the attributes it shares with the relations before it, probed once per
//! partial result (Wang et al., "Free Join: Unifying Worst-Case Optimal and
//! Traditional Joins", SIGMOD 2023, §2.2, Example 2.2).
//!
//! # Not a trie family
//!
//! The two trie families navigate a relation level by level: a
//! [`TrieIterator`](crate::TrieIterator) has `open` and `up` and a
//! least-upper-bound `seek` over sorted keys, and a
//! [`HashTrieIterator`](crate::HashTrieIterator) has `open`, `up` and an exact
//! `lookup` of one level's hash. A keyed index has no levels to descend: one
//! `probe` call names the whole key and lends whole rows, key columns and
//! payload alike, so an algorithm has nothing to verify afterwards. The
//! contracts differ, so `KeyedIterable` is a parallel family rather than a
//! subtrait of either. The fork propagates up the stack, as the hash family's
//! did: a separate algorithm (`BinaryHashJoin`), a separate join entry point
//! and separate test suites.
//!
//! The key length is a property of the built index, taken from the header's
//! level count ([`TrieLevels::ProbeKeys`](crate::TrieLevels::ProbeKeys) plans
//! it as the columns earlier atoms of the join order bind).

use crate::joinable::JoinIterable;

/// A relation stored as an index keyed on its leading columns and probed by
/// exact match, the unit a binary hash join is built from.
///
/// Both iterators borrow the index, lend rows in source order, and allocate
/// nothing. A structure that keeps duplicate rows lends each copy.
pub trait KeyedIterable: JoinIterable {
    /// How many leading columns form the key (`RelationHeader::levels()` at
    /// build). `0` means the index has no key: it lends every row to every
    /// probe, the case of a join order's first atom and of a cross product.
    fn key_len(&self) -> usize;

    /// Every stored row, in source order.
    fn scan(&self) -> impl Iterator<Item = &[usize]> + '_;

    /// The stored rows whose first [`key_len`](Self::key_len) values equal
    /// `key`, in source order.
    ///
    /// # Panics
    ///
    /// Panics unless `key.len() == self.key_len()`. With `key_len() == 0` the
    /// key is empty and every row is lent.
    fn probe<'a>(&'a self, key: &'a [usize]) -> impl Iterator<Item = &'a [usize]> + 'a;
}
```

Mention the family in the crate docs.

Current (lines 20-24):

```rust
//! A [`TupleView`] reads a [`Tuples`] batch through a [`Filter`] (constants
//! and column equalities) and a list of output columns, so a build copies
//! only the rows and columns an index needs, once.
//!
//! # Example
```

Replace with:

```rust
//! A [`TupleView`] reads a [`Tuples`] batch through a [`Filter`] (constants
//! and column equalities) and a list of output columns, so a build copies
//! only the rows and columns an index needs, once.
//!
//! [`KeyedIterable`] is the third iterator family, beside [`TrieIterable`]
//! and [`HashTrieIterable`]: an index keyed on its leading columns and probed
//! by exact match, which a binary hash join drives.
//!
//! # Example
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt -p kermit-iters`, then `nix develop --command cargo test -p kermit-iters --lib keyed`

Expected: `test keyed::tests::a_key_of_no_columns_lends_every_row ... ok`, `test keyed::tests::probe_lends_the_rows_of_a_key_in_scan_order ... ok`, `test result: ok. 2 passed; 0 failed`.

Then `nix develop --command cargo clippy -p kermit-iters --all-targets -- -D warnings` (no warnings) and `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit-iters'` (no warnings; the intra-doc links to `TrieLevels::ProbeKeys` resolve because Story 1 added it).

- [ ] **Step 5: Commit**

```
git add -N kermit-iters/src/keyed.rs
git commit -m "feat(iters): KeyedIterable, the keyed-index iterator family" -- kermit-iters/src/keyed.rs kermit-iters/src/lib.rs
```

### Task 2.2: `HashIndex<H>` and its unit tests

**Files:**
- Create: `kermit-ds/src/ds/hash_index/mod.rs`
- Create: `kermit-ds/src/ds/hash_index/implementation.rs`
- Create: `kermit-ds/src/ds/hash_index/hash_index_iter.rs`
- Create: `kermit-ds/src/ds/hash_index/tests.rs`
- Modify: `kermit-ds/src/ds/mod.rs:1-3`, `kermit-ds/src/ds/mod.rs:16-18`
- Modify: `kermit-ds/src/lib.rs:18-21`, `kermit-ds/src/lib.rs:43-44`

- [ ] **Step 1: Write the failing tests**

Create `kermit-ds/src/ds/hash_index/mod.rs`:

```rust
//! A chained hash index keyed on its leading columns: the join hash table of
//! a binary hash join, in the keyed iterator family.

mod hash_index_iter;
mod implementation;

#[cfg(test)]
mod tests;

pub use implementation::HashIndex;
```

Create `kermit-ds/src/ds/hash_index/tests.rs`. The `Identity` strategy (`hash(v) = v`) makes the key fold and the bucket computable by hand: the literals below were computed independently of the Rust code, by folding `h = (h.rotate_left(5) ^ v) * 0x517cc1b727220a95` over the key and taking the top bits of `h * 0x9E3779B97F4A7C15` (both modulo 2⁶⁴).

```rust
use {
    super::implementation::*,
    crate::{Cardinality, Filter, HeapSize, Relation, RelationHeader, TupleView, Tuples},
    kermit_iters::{
        FxHashStrategy, HasOptimizationAxes, HashStrategy, KeyedIterable, LayoutOption, RowId,
        SipHashStrategy, TrieLevels,
    },
};

/// Test-only strategy that sends every value to hash 0, so every row of an
/// index lands in bucket 0 and only the key comparison separates the keys.
/// A twin drives the contract suite in `kermit-ds/tests/hash_index_tests.rs`;
/// neither is shared, so no crate ships a colliding strategy.
#[derive(Copy, Clone, Default, Debug)]
struct OneBucket;

impl LayoutOption for OneBucket {
    const NAME: &'static str = "one-bucket";
}

impl HashStrategy for OneBucket {
    fn hash(_key: usize) -> u64 { 0 }
}

/// Test-only strategy `hash(v) = v`, so that the bucket of a key can be
/// computed by hand and pinned.
#[derive(Copy, Clone, Default, Debug)]
struct Identity;

impl LayoutOption for Identity {
    const NAME: &'static str = "identity";
}

impl HashStrategy for Identity {
    fn hash(key: usize) -> u64 { key as u64 }
}

fn header(arity: usize, key_len: usize) -> RelationHeader {
    RelationHeader::new_nameless_positional(arity).with_levels(key_len)
}

fn build<H: HashStrategy>(arity: usize, key_len: usize, rows: Vec<Vec<usize>>) -> HashIndex<H> {
    HashIndex::from_tuples(header(arity, key_len), rows)
}

/// The row ids of bucket `bucket`'s chain, front to back.
fn chain<H: HashStrategy>(index: &HashIndex<H>, bucket: usize) -> Vec<RowId> {
    let mut ids = vec![];
    let mut at = index.directory[bucket];
    while at != NIL {
        ids.push(at);
        at = index.next[at as usize];
    }
    ids
}

fn keeps_duplicates<R: Relation>() -> bool { R::KEEPS_DUPLICATES }

fn trie_levels<R: Relation>() -> TrieLevels { R::TRIE_LEVELS }

/// `slots = max(2, rows.next_power_of_two())`, so the load factor is at most
/// 1 and the bucket shift (`64 - log2(slots)`) is at most 63.
#[test]
fn the_directory_has_the_next_power_of_two_slots_and_at_least_two() {
    for (rows, slots) in [(0, 2), (1, 2), (2, 2), (3, 4), (4, 4), (5, 8), (8, 8), (9, 16)] {
        let index = build::<SipHashStrategy>(1, 1, (0..rows).map(|i| vec![i]).collect());
        assert_eq!(index.directory.len(), slots, "{rows} rows");
        assert_eq!(index.next.len(), rows, "{rows} rows");
    }
}

#[test]
fn an_index_with_no_key_columns_has_no_directory_and_lends_every_row() {
    let rows = vec![vec![3, 4], vec![1, 2], vec![3, 4]];
    let index = build::<SipHashStrategy>(2, 0, rows.clone());
    assert_eq!(index.key_len(), 0);
    assert!(index.directory.is_empty());
    assert!(index.next.is_empty());
    let lent: Vec<Vec<usize>> = index.probe(&[]).map(<[usize]>::to_vec).collect();
    assert_eq!(lent, rows);
}

#[test]
fn the_bucket_is_the_top_bits_of_the_fibonacci_product_of_the_fx_fold() {
    // Sixteen slots: the top four bits.
    let sixteen = build::<Identity>(2, 2, (0..16_usize).map(|i| vec![i, i + 1]).collect());
    assert_eq!(sixteen.directory.len(), 16);
    assert_eq!(sixteen.bucket(&[]), 0);
    assert_eq!(sixteen.bucket(&[1]), 9);
    assert_eq!(sixteen.bucket(&[2]), 2);
    assert_eq!(sixteen.bucket(&[3]), 11);
    assert_eq!(sixteen.bucket(&[1, 2]), 3);
    assert_eq!(sixteen.bucket(&[2, 1]), 4);
    assert_eq!(sixteen.bucket(&[5, 9]), 8);

    // Two slots: the top bit.
    let two = build::<Identity>(2, 2, vec![vec![1, 2], vec![3, 4]]);
    assert_eq!(two.directory.len(), 2);
    assert_eq!(two.bucket(&[1]), 1);
    assert_eq!(two.bucket(&[2]), 0);
    assert_eq!(two.bucket(&[3]), 1);
    assert_eq!(two.bucket(&[1, 2]), 0);
    assert_eq!(two.bucket(&[5, 9]), 1);
}

#[test]
fn the_build_places_every_row_in_its_pinned_bucket() {
    let sixteen = build::<Identity>(2, 2, (0..16_usize).map(|i| vec![i, i + 1]).collect());
    let expected: [&[RowId]; 16] = [
        &[14],
        &[10],
        &[],
        &[1, 9],
        &[],
        &[],
        &[2],
        &[12],
        &[5, 6],
        &[0, 8, 13],
        &[15],
        &[3, 11],
        &[],
        &[7],
        &[4],
        &[],
    ];
    for (bucket, ids) in expected.iter().enumerate() {
        assert_eq!(chain(&sixteen, bucket), *ids, "bucket {bucket}");
    }
}

#[test]
fn rows_with_one_key_share_a_chain_in_source_order() {
    // Keyed on the first column: rows 0 and 2 carry key 2 (bucket 0 of 4),
    // row 1 carries key 3 (bucket 2).
    let index = build::<Identity>(2, 1, vec![vec![2, 10], vec![3, 11], vec![2, 12]]);
    assert_eq!(index.directory.len(), 4);
    assert_eq!(chain(&index, 0), vec![0, 2]);
    assert_eq!(chain(&index, 1), Vec::<RowId>::new());
    assert_eq!(chain(&index, 2), vec![1]);
    assert_eq!(chain(&index, 3), Vec::<RowId>::new());
}

#[test]
fn a_hasher_that_maps_everything_to_zero_chains_every_row_in_source_order() {
    let rows: Vec<Vec<usize>> = (0..9_usize).map(|i| vec![i % 3, i]).collect();
    let index = build::<OneBucket>(2, 1, rows);
    assert_eq!(index.directory.len(), 16);
    assert_eq!(chain(&index, 0), (0..9).collect::<Vec<RowId>>());
    for bucket in 1..16 {
        assert!(chain(&index, bucket).is_empty(), "bucket {bucket}");
    }
    // The chain holds three keys, so the key comparison does the separating.
    let lent: Vec<Vec<usize>> = index.probe(&[1]).map(<[usize]>::to_vec).collect();
    assert_eq!(lent, vec![vec![1, 1], vec![1, 4], vec![1, 7]]);
}

#[test]
fn every_chain_is_in_source_order_and_the_chains_partition_the_rows() {
    let rows: Vec<Vec<usize>> = (0..100_usize).map(|i| vec![(i * 7919) % 31, i]).collect();
    let index = build::<SipHashStrategy>(2, 1, rows);
    assert!(index.directory.len() >= index.rows.len(), "load factor at most 1");
    let mut seen: Vec<RowId> = vec![];
    for bucket in 0..index.directory.len() {
        let ids = chain(&index, bucket);
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "bucket {bucket}: {ids:?}");
        for &id in &ids {
            assert_eq!(index.bucket(&index.rows.row(id)[..1]), bucket, "row {id}");
        }
        seen.extend(ids);
    }
    seen.sort_unstable();
    assert_eq!(seen, (0..100).collect::<Vec<RowId>>());
}

/// Rows inserted one at a time end in the arrays the bulk build makes of
/// them: the directory doubles exactly when rows outnumber slots, a rehash
/// links last to first like the build, and a plain insert appends to its
/// chain's tail, so every chain stays in source order.
#[test]
fn insert_builds_the_arrays_the_bulk_build_builds() {
    let rows: Vec<Vec<usize>> = (0..40_usize).map(|i| vec![(i * 13) % 7, i]).collect();
    let mut inserted = HashIndex::<SipHashStrategy>::new(header(2, 1));
    for (n, row) in rows.iter().enumerate() {
        inserted.insert(row);
        let built = build::<SipHashStrategy>(2, 1, rows[..=n].to_vec());
        assert_eq!(inserted.directory, built.directory, "after {} inserts", n + 1);
        assert_eq!(inserted.next, built.next, "after {} inserts", n + 1);
        assert_eq!(inserted.rows, built.rows, "after {} inserts", n + 1);
    }
}

#[test]
fn a_view_build_equals_the_build_from_its_materialised_batch() {
    let source = Tuples::from(vec![
        vec![1, 1, 5],
        vec![2, 3, 5],
        vec![2, 2, 6],
        vec![1, 1, 6],
        vec![3, 3, 5],
    ]);
    // Column 2 holds 5 and columns 0 and 1 agree (rows 0 and 4), reading
    // columns 2 then 0: the output rows [5, 1] and [5, 3].
    let view = TupleView::new(&source, Filter::new(vec![(2, 5)], vec![(0, 1)]), vec![2, 0]);

    let from_view = HashIndex::<SipHashStrategy>::from_view(header(2, 1), view.clone());
    let from_batch = HashIndex::<SipHashStrategy>::from_tuples(header(2, 1), view.materialize());

    assert_eq!(from_view.rows, Tuples::from(vec![vec![5, 1], vec![5, 3]]));
    assert_eq!(from_view.rows, from_batch.rows);
    assert_eq!(from_view.directory, from_batch.directory);
    assert_eq!(from_view.next, from_batch.next);
    assert_eq!(from_view.heap_size_bytes(), from_batch.heap_size_bytes());

    // A filter that keeps nothing builds an empty index.
    let none = TupleView::new(&source, Filter::new(vec![(2, 99)], vec![]), vec![2, 0]);
    let empty = HashIndex::<SipHashStrategy>::from_view(header(2, 1), none);
    assert_eq!(empty.tuple_count(), 0);
    assert_eq!(empty.directory.len(), 2);
}

#[test]
fn heap_size_counts_the_rows_the_directory_and_the_links() {
    let words = std::mem::size_of::<usize>();
    let links = std::mem::size_of::<RowId>();
    let rows = vec![vec![1, 2], vec![3, 4], vec![1, 5]];

    // Three rows of two values, four slots, three links.
    let keyed = build::<SipHashStrategy>(2, 1, rows.clone());
    assert_eq!(keyed.heap_size_bytes(), 3 * 2 * words + (4 + 3) * links);

    // No key columns: the rows alone.
    let unkeyed = build::<SipHashStrategy>(2, 0, rows);
    assert_eq!(unkeyed.heap_size_bytes(), 3 * 2 * words);

    // An empty keyed index still holds its two empty slots.
    let empty = build::<SipHashStrategy>(2, 1, vec![]);
    assert_eq!(empty.heap_size_bytes(), 2 * links);
}

/// An empty source builds an empty index under any header, as
/// `from_tuples(header, vec![])` does.
#[test]
fn an_empty_source_of_another_arity_builds_an_empty_index() {
    let source = Tuples::new(3);
    let index = HashIndex::<SipHashStrategy>::from_view(header(2, 1), TupleView::all(&source));
    assert_eq!(index.header().arity(), 2);
    assert_eq!(index.tuple_count(), 0);
    assert_eq!(index.directory.len(), 2);
    assert_eq!(index.scan().count(), 0);
}

#[test]
fn key_len_is_the_headers_levels() {
    let rows = vec![vec![1, 2, 3]];
    assert_eq!(build::<SipHashStrategy>(3, 2, rows.clone()).key_len(), 2);
    assert_eq!(build::<SipHashStrategy>(3, 0, rows.clone()).key_len(), 0);
    // A header with no explicit level count has one per column, the keyed
    // header `bench ds` builds from: the index is keyed on every column.
    let whole = HashIndex::<SipHashStrategy>::from_tuples(RelationHeader::from(3), rows);
    assert_eq!(whole.key_len(), 3);
    assert_eq!(HashIndex::<SipHashStrategy>::new(header(3, 1)).key_len(), 1);
}

#[test]
fn it_keeps_duplicates_and_is_planned_under_probe_keys() {
    assert!(keeps_duplicates::<HashIndex>());
    assert_eq!(trie_levels::<HashIndex>(), TrieLevels::ProbeKeys);
    let index = build::<SipHashStrategy>(2, 2, vec![vec![1, 2], vec![1, 2], vec![3, 4]]);
    assert_eq!(index.tuple_count(), 3);
    assert_eq!(index.probe(&[1, 2]).count(), 2);
}

#[test]
fn the_hasher_axis_names_the_strategy() {
    let sip = build::<SipHashStrategy>(1, 1, vec![vec![1]]).optimization_axes();
    assert_eq!(sip.len(), 1);
    assert_eq!(sip["ds_layout_hasher"], "sip");
    let fx = build::<FxHashStrategy>(1, 1, vec![vec![1]]).optimization_axes();
    assert_eq!(fx.len(), 1);
    assert_eq!(fx["ds_layout_hasher"], "fxhash");
}
```

Register the module and the re-export.

Current (`kermit-ds/src/ds/mod.rs`, lines 1-3):

```rust
mod column_trie;
mod hash_trie;
mod tree_trie;
```

Replace with:

```rust
mod column_trie;
mod hash_index;
mod hash_trie;
mod tree_trie;
```

Current (`kermit-ds/src/ds/mod.rs`, lines 16-18):

```rust
pub use {
    column_trie::{ColumnTrie, ColumnTrieBuildMode},
    hash_trie::{
```

Replace with:

```rust
pub use {
    column_trie::{ColumnTrie, ColumnTrieBuildMode},
    hash_index::HashIndex,
    hash_trie::{
```

Current (`kermit-ds/src/lib.rs`, lines 43-44):

```rust
        AllLevels, ChildCapacity, ColumnTrie, ColumnTrieBuildMode, EagerExpansion, ExpansionPolicy,
        HashTrie, HashTrieBuildMode, HashTrieConfig, IndexStructure, InvalidLoadFactor,
```

Replace with (`cargo fmt` reflows the list):

```rust
        AllLevels, ChildCapacity, ColumnTrie, ColumnTrieBuildMode, EagerExpansion, ExpansionPolicy,
        HashIndex, HashTrie, HashTrieBuildMode, HashTrieConfig, IndexStructure, InvalidLoadFactor,
```

Current (`kermit-ds/src/lib.rs`, lines 18-21):

```rust
//! The two sorted tries take their seek algorithm as a Layout type
//! parameter, [`SeekStrategy`]: [`LinearSeek`], [`BinarySeek`] (the
//! default) or [`GallopingSeek`].
#![deny(missing_docs)]
```

Replace with:

```rust
//! The two sorted tries take their seek algorithm as a Layout type
//! parameter, [`SeekStrategy`]: [`LinearSeek`], [`BinarySeek`] (the
//! default) or [`GallopingSeek`].
//!
//! [`HashIndex`] is not a trie. It is a chained hash table keyed on its
//! leading columns, the build side of a binary hash join, and it implements
//! [`KeyedIterable`](kermit_iters::KeyedIterable), a third iterator family,
//! beside [`Relation`].
#![deny(missing_docs)]
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit-ds --lib ds::hash_index::tests`

Expected: the build fails; `mod.rs` declares two modules whose files do not exist yet:

```
error[E0583]: file not found for module `hash_index_iter`
error[E0583]: file not found for module `implementation`
```

- [ ] **Step 3: Implement**

Create `kermit-ds/src/ds/hash_index/implementation.rs`:

```rust
//! [`HashIndex`]: the representation, the build and the [`Relation`] impls.

use {
    crate::{
        cardinality::Cardinality,
        heap_size::HeapSize,
        morsel::row_id,
        relation::{check_view_arity, Projectable, Relation, RelationHeader},
    },
    kermit_iters::{
        HasOptimizationAxes, HashStrategy, JoinIterable, LayoutOption, RowId, SipHashStrategy,
        TrieLevels, TupleView, Tuples,
    },
    serde_json::Value,
    std::{collections::BTreeMap, marker::PhantomData},
};

/// No row: an empty bucket's head, and the link of a chain's last row. A
/// [`Tuples`] batch holds at most `RowId::MAX` rows, so `RowId::MAX` is never
/// a row id.
pub(super) const NIL: RowId = RowId::MAX;

/// `FxHash`'s multiplier: the combine step of the key hash.
const FX_SEED: u64 = 0x517c_c1b7_2722_0a95;

/// 2⁶⁴ over the golden ratio, rounded to an odd number: the multiplier of
/// Fibonacci hashing, which spreads the key hash over the directory by its top
/// bits.
const FIBONACCI: u64 = 0x9E37_79B9_7F4A_7C15;

/// A chained hash table over one relation's rows, keyed on their leading
/// columns: the build side of a binary hash join (Wang et al., "Free Join:
/// Unifying Worst-Case Optimal and Traditional Joins", SIGMOD 2023, §2.2: a
/// hash table keyed on the attributes a relation shares with the relations
/// before it).
///
/// # Representation
///
/// - `rows`: the view's matching rows, copied once, in source order.
/// - `directory`: `slots` bucket heads, each a [`RowId`] or `NIL`, with
///   `slots = max(2, rows.next_power_of_two())`, so the load factor is at
///   most 1.
/// - `next`: per row, the next row of its bucket's chain, or `NIL`.
///
/// Both arrays are plain `Vec<RowId>`. `HashTrie`'s `ChainLinks` exist so
/// that a lazy trie can re-thread rows through `&self`; a keyed index never
/// does, so it needs none of that machinery. Both arrays are empty when the
/// index has no key columns.
///
/// # Keys and hashing
///
/// The key is the first [`key_len`](kermit_iters::KeyedIterable::key_len)
/// columns, the header's level count at build (`RelationHeader::levels()`;
/// every column unless the header says fewer, as `bench ds` builds it). The
/// key hash folds `H::hash` over the key values with `FxHash`'s combine step,
/// `h = (h.rotate_left(5) ^ H::hash(v)) * 0x517c_c1b7_2722_0a95`, from
/// `h = 0`. The bucket is the top `log2(slots)` bits of
/// `h * 0x9E37_79B9_7F4A_7C15` (Fibonacci hashing), all modulo 2⁶⁴. The
/// hasher is the Layout `H`, bench axis `ds_layout_hasher`.
///
/// # Invariants
///
/// - Rows are linked last to first, each prepended to its bucket's chain, so
///   every chain lists its rows in ascending row id: source order.
/// - `directory.len()` is a power of two, at least 2, and at least the row
///   count. `directory` and `next` are empty exactly when `key_len == 0`.
/// - `key_len` is `header.levels()`, at most the arity.
///
/// # Duplicates
///
/// A repeated row is stored and lent once per copy
/// ([`Relation::KEEPS_DUPLICATES`] is `true`), so a join's counts equal
/// `HashTrie`'s. It is planned under [`TrieLevels::ProbeKeys`].
///
/// # Example
///
/// ```
/// use {
///     kermit_ds::{HashIndex, Relation, RelationHeader},
///     kermit_iters::KeyedIterable,
/// };
///
/// // S(b, c), keyed on its first column b.
/// let s: HashIndex = HashIndex::from_tuples(
///     RelationHeader::new_nameless_positional(2).with_levels(1),
///     vec![vec![2, 3], vec![2, 4], vec![3, 4]],
/// );
/// assert_eq!(s.key_len(), 1);
/// let matches: Vec<Vec<usize>> = s.probe(&[2]).map(<[usize]>::to_vec).collect();
/// assert_eq!(matches, vec![vec![2, 3], vec![2, 4]]);
/// assert_eq!(s.probe(&[5]).count(), 0);
/// ```
#[derive(Clone, Debug)]
pub struct HashIndex<H: HashStrategy = SipHashStrategy> {
    /// The relation's header; its level count is the key length.
    pub(super) header: RelationHeader,
    /// The rows, in source order.
    pub(super) rows: Tuples,
    /// How many leading columns form the key (`header.levels()`).
    pub(super) key_len: usize,
    /// Per bucket, the first row of its chain or `NIL`.
    pub(super) directory: Vec<RowId>,
    /// Per row, the next row of its chain or `NIL`.
    pub(super) next: Vec<RowId>,
    /// The hasher: a type-level choice, zero-sized.
    _hasher: PhantomData<H>,
}

impl<H: HashStrategy> HashIndex<H> {
    /// An index over `rows` under `header`, sized for exactly those rows.
    fn with_rows(header: RelationHeader, rows: Tuples) -> Self {
        let key_len = header.levels();
        let mut index = Self {
            header,
            rows,
            key_len,
            directory: Vec::new(),
            next: Vec::new(),
            _hasher: PhantomData,
        };
        index.link_rows();
        index
    }

    /// The bucket of `key`: the top `log2(slots)` bits of the Fibonacci
    /// product of the `FxHash` fold of `H::hash` over `key`.
    pub(super) fn bucket(&self, key: &[usize]) -> usize {
        debug_assert!(!self.directory.is_empty(), "an index with no key has no buckets");
        let mut hash = 0_u64;
        for &value in key {
            hash = (hash.rotate_left(5) ^ H::hash(value)).wrapping_mul(FX_SEED);
        }
        // `slots >= 2`, so the shift is at most 63.
        let shift = u64::BITS - self.directory.len().trailing_zeros();
        (hash.wrapping_mul(FIBONACCI) >> shift) as usize
    }

    /// Sizes the directory for the rows held and links every row, last to
    /// first, each prepended to its bucket's chain: three allocations in all
    /// (`rows` is the caller's), none per row. An index with no key columns
    /// has no chains to link.
    fn link_rows(&mut self) {
        if self.key_len == 0 {
            return;
        }
        let rows = self.rows.len();
        self.directory = vec![NIL; rows.next_power_of_two().max(2)];
        self.next = vec![NIL; rows];
        for position in (0..rows).rev() {
            let row = row_id(position);
            let bucket = self.bucket(&self.rows.row(row)[..self.key_len]);
            self.next[position] = self.directory[bucket];
            self.directory[bucket] = row;
        }
    }

    /// Appends `row`, the newest and so the highest id, to the tail of its
    /// chain, which keeps the chain in source order. Linear in the chain.
    fn append_to_chain(&mut self, row: RowId) {
        let bucket = self.bucket(&self.rows.row(row)[..self.key_len]);
        let mut at = self.directory[bucket];
        if at == NIL {
            self.directory[bucket] = row;
            return;
        }
        while self.next[at as usize] != NIL {
            at = self.next[at as usize];
        }
        self.next[at as usize] = row;
    }
}

impl<H: HashStrategy> JoinIterable for HashIndex<H> {}

impl<H: HashStrategy> Relation for HashIndex<H> {
    /// An index is a multiset: a repeated row is stored and lent once per
    /// copy (see [`Cardinality`]).
    const KEEPS_DUPLICATES: bool = true;
    /// Planned under [`TrieLevels::ProbeKeys`]: each index's level count is
    /// the length of the key it is probed on.
    const TRIE_LEVELS: TrieLevels = TrieLevels::ProbeKeys;

    fn header(&self) -> &RelationHeader { &self.header }

    /// An empty index under `header`: two empty slots when the header has key
    /// columns, none when it has not.
    fn new(header: RelationHeader) -> Self {
        let rows = Tuples::new(header.arity());
        Self::with_rows(header, rows)
    }

    /// Builds an index from the rows a view keeps.
    ///
    /// Copies them once with [`TupleView::materialize`] (an exact-size
    /// buffer, in source order, the index's own `rows`), then sizes the
    /// directory and links every row: no allocation per input row.
    ///
    /// # Panics
    ///
    /// Panics if the view's source has rows and the view's arity does not
    /// match `header.arity()`, even when the filter keeps none
    /// (`relation::check_view_arity`, which owns the message). An empty
    /// source builds the empty index under any header.
    fn from_view(header: RelationHeader, view: TupleView<'_>) -> Self {
        check_view_arity(&header, &view);
        if view.source_rows() == 0 {
            return Self::new(header);
        }
        Self::with_rows(header, view.materialize())
    }

    /// Appends `tuple` and links it. Once the rows outnumber the slots the
    /// directory doubles and every row is relinked, so the arrays equal those
    /// of a bulk build of the same rows. `from_view` is the only build that
    /// is benchmarked.
    ///
    /// # Panics
    ///
    /// Panics if `tuple`'s length is not the relation's arity, or if the
    /// index already holds `RowId::MAX` rows ([`Tuples::push`]).
    fn insert(&mut self, tuple: impl AsRef<[usize]>) {
        let tuple = tuple.as_ref();
        let arity = self.header.arity();
        assert_eq!(
            tuple.len(),
            arity,
            "tuple arity {} does not match relation arity {}",
            tuple.len(),
            arity
        );
        self.rows.push(tuple);
        if self.key_len == 0 {
            return;
        }
        if self.rows.len() > self.directory.len() {
            self.link_rows();
        } else {
            self.next.push(NIL);
            let row = row_id(self.rows.len() - 1);
            self.append_to_chain(row);
        }
    }

    fn insert_all(&mut self, tuples: impl Into<Tuples>) {
        let tuples: Tuples = tuples.into();
        for tuple in tuples.rows() {
            self.insert(tuple);
        }
    }
}

impl<H: HashStrategy> Projectable for HashIndex<H> {
    /// Rebuilds from the stored rows (the rows `scan` lends) with only
    /// `columns`, in the order given, under a nameless header; the new index
    /// is keyed on every projected column.
    ///
    /// # Panics
    ///
    /// Panics if any element of `columns` is `>= self.header().arity()`.
    fn project(&self, columns: Vec<usize>) -> Self {
        let arity = self.header.arity();
        assert!(
            columns.iter().all(|&column| column < arity),
            "project: a column is out of range for arity {arity}"
        );
        let attrs: Vec<String> = columns
            .iter()
            .filter_map(|&column| self.header.attrs().get(column).cloned())
            .collect();
        let header = if attrs.is_empty() {
            RelationHeader::new_nameless_positional(columns.len())
        } else {
            RelationHeader::new_nameless(attrs)
        };

        let mut projected = Tuples::with_capacity(columns.len(), self.rows.len());
        let mut row = Vec::with_capacity(columns.len());
        for tuple in self.rows.rows() {
            row.clear();
            row.extend(columns.iter().map(|&column| tuple[column]));
            projected.push(&row);
        }
        Self::from_tuples(header, projected)
    }
}

impl<H: HashStrategy> HeapSize for HashIndex<H> {
    /// The rows, the directory and the links, by capacity.
    fn heap_size_bytes(&self) -> usize {
        self.rows.heap_size_bytes()
            + (self.directory.capacity() + self.next.capacity()) * std::mem::size_of::<RowId>()
    }
}

impl<H: HashStrategy> Cardinality for HashIndex<H> {
    /// The row count, duplicates included.
    fn tuple_count(&self) -> usize { self.rows.len() }
}

impl<H: HashStrategy> HasOptimizationAxes for HashIndex<H> {
    /// The one layout axis `ds_layout_hasher`: the strategy's
    /// `LayoutOption::NAME`.
    fn optimization_axes(&self) -> BTreeMap<String, Value> {
        BTreeMap::from([(
            "ds_layout_hasher".to_string(),
            Value::String(<H as LayoutOption>::NAME.to_string()),
        )])
    }
}
```

Create `kermit-ds/src/ds/hash_index/hash_index_iter.rs`:

```rust
//! The [`KeyedIterable`] impl of [`HashIndex`]: a scan over the rows, and a
//! probe that walks one bucket's chain.

use {
    super::implementation::{HashIndex, NIL},
    kermit_iters::{HashStrategy, KeyedIterable, RowId, Tuples},
};

/// The rows a probe lends: one bucket's chain, each row kept only if its key
/// columns equal the probe's key. A key of no columns has no chains, so it
/// walks every row in order.
struct Probe<'a> {
    rows: &'a Tuples,
    /// The index's per-row links; unused (and empty) for a key of no columns.
    links: &'a [RowId],
    key: &'a [usize],
    /// The next row to consider, or `NIL`.
    at: RowId,
}

impl<'a> Iterator for Probe<'a> {
    type Item = &'a [usize];

    fn next(&mut self) -> Option<&'a [usize]> {
        let rows: &'a Tuples = self.rows;
        while self.at != NIL {
            let position = self.at as usize;
            let row = rows.row(self.at);
            self.at = if self.key.is_empty() {
                // No key columns, no chains: every row, in order.
                if position + 1 < rows.len() { self.at + 1 } else { NIL }
            } else {
                self.links[position]
            };
            // Other keys share the bucket: only a row whose key columns equal
            // the probe's key is lent.
            if row.starts_with(self.key) {
                return Some(row);
            }
        }
        None
    }
}

impl<H: HashStrategy> KeyedIterable for HashIndex<H> {
    fn key_len(&self) -> usize { self.key_len }

    /// Every row, in source order.
    fn scan(&self) -> impl Iterator<Item = &[usize]> + '_ { self.rows.rows() }

    /// Hashes `key`, then walks the bucket's chain, which holds its rows in
    /// source order. Allocates nothing.
    fn probe<'a>(&'a self, key: &'a [usize]) -> impl Iterator<Item = &'a [usize]> + 'a {
        assert_eq!(
            key.len(),
            self.key_len,
            "probe: a key of {} values on an index keyed on {} columns",
            key.len(),
            self.key_len
        );
        let at = if self.key_len == 0 {
            if self.rows.is_empty() { NIL } else { 0 }
        } else {
            self.directory[self.bucket(key)]
        };
        Probe {
            rows: &self.rows,
            links: &self.next,
            key,
            at,
        }
    }
}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt -p kermit-ds`, then `nix develop --command cargo test -p kermit-ds --lib ds::hash_index::tests`

Expected: fourteen tests, all `ok`, among them `the_build_places_every_row_in_its_pinned_bucket`, `insert_builds_the_arrays_the_bulk_build_builds` and `a_view_build_equals_the_build_from_its_materialised_batch`; `test result: ok. 14 passed; 0 failed`.

Run: `nix develop --command cargo test -p kermit-ds --doc HashIndex`

Expected: `test kermit-ds/src/ds/hash_index/implementation.rs - ds::hash_index::implementation::HashIndex (line ..) ... ok`, `test result: ok. 1 passed`.

Run: `nix develop --command cargo clippy -p kermit-iters -p kermit-ds --all-targets -- -D warnings` (no warnings) and `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit-iters -p kermit-ds'` (no warnings).

- [ ] **Step 5: Commit**

```
git add -N kermit-ds/src/ds/hash_index/mod.rs kermit-ds/src/ds/hash_index/implementation.rs kermit-ds/src/ds/hash_index/hash_index_iter.rs kermit-ds/src/ds/hash_index/tests.rs
git commit -m "feat(ds): HashIndex, a chained hash index keyed on its leading columns" -- kermit-ds/src/ds/hash_index/mod.rs kermit-ds/src/ds/hash_index/implementation.rs kermit-ds/src/ds/hash_index/hash_index_iter.rs kermit-ds/src/ds/hash_index/tests.rs kermit-ds/src/ds/mod.rs kermit-ds/src/lib.rs
```

### Task 2.3: HashIndex rows in the cross-structure view tests

**Files:**
- Modify: `kermit-ds/src/relation.rs:1202-1206`, `kermit-ds/src/relation.rs:1366-1368`, `kermit-ds/src/relation.rs:1481-1483`, `kermit-ds/src/relation.rs:1500-1501`, `kermit-ds/src/relation.rs:1518-1520`

These add rows to the tests of `mod tests` that already build every structure from a view; each edit is an insertion, and no existing line changes. They pass on arrival, since Task 2.2 built the structure.

- [ ] **Step 1: Write the tests**

A helper for a HashIndex's rows. Current (lines 1202-1206):

```rust
    fn hash_rows(relation: &crate::ds::HashTrie) -> Vec<Vec<usize>> {
        let mut rows = relation.collect_tuples().to_vecs();
        rows.sort();
        rows
    }
```

Replace with:

```rust
    fn hash_rows(relation: &crate::ds::HashTrie) -> Vec<Vec<usize>> {
        let mut rows = relation.collect_tuples().to_vecs();
        rows.sort();
        rows
    }

    /// A `HashIndex`'s rows, scanned: it keeps them in source order.
    fn index_rows(index: &crate::ds::HashIndex) -> Vec<Vec<usize>> {
        use kermit_iters::KeyedIterable;
        index.scan().map(<[usize]>::to_vec).collect()
    }
```

From-view equivalence. Current (lines 1366-1368), in `from_view_builds_what_the_materialised_batch_builds`:

```rust
        assert_eq!(column.heap_size_bytes(), column_batch.heap_size_bytes());

        let hash: HashTrie = HashTrie::from_view(2.into(), view);
```

Replace with (the new block sits before the line that moves `view`):

```rust
        assert_eq!(column.heap_size_bytes(), column_batch.heap_size_bytes());

        // `HashIndex` keeps the view's rows in source order, keyed on every column.
        {
            use crate::ds::HashIndex;
            let index: HashIndex = HashIndex::from_view(2.into(), view.clone());
            let index_batch: HashIndex = HashIndex::from_tuples(2.into(), batch.clone());
            assert_eq!(index_rows(&index), expected);
            assert_eq!(index_rows(&index_batch), expected);
            assert_eq!(index.heap_size_bytes(), index_batch.heap_size_bytes());
        }

        let hash: HashTrie = HashTrie::from_view(2.into(), view);
```

`KEEPS_DUPLICATES`. Current (lines 1481-1483), the end of `keeps_duplicates_names_whether_a_repeated_row_is_kept`:

```rust
        assert_eq!(column.tuple_count(), 2);
        assert_eq!(hash.tuple_count(), 3);
    }
```

Replace with:

```rust
        assert_eq!(column.tuple_count(), 2);
        assert_eq!(hash.tuple_count(), 3);

        // `HashIndex` is a multiset too.
        let index: crate::ds::HashIndex =
            crate::ds::HashIndex::from_view(2.into(), TupleView::all(&source));
        assert!(keeps_duplicates::<crate::ds::HashIndex>());
        assert_eq!(index.tuple_count(), 3);
    }
```

The wrong-arity panic. Current (lines 1500-1501):

```rust
        let _: crate::ds::HashTrie = build_from_a_rejecting_view_of_the_wrong_arity();
    }
```

Replace with:

```rust
        let _: crate::ds::HashTrie = build_from_a_rejecting_view_of_the_wrong_arity();
    }

    #[test]
    #[should_panic(expected = "from_tuples: tuple arity 2 does not match header arity 3")]
    fn a_rejecting_view_of_the_wrong_arity_panics_for_a_hash_index() {
        let _: crate::ds::HashIndex = build_from_a_rejecting_view_of_the_wrong_arity();
    }
```

The empty source. Current (lines 1518-1520), the end of `an_empty_source_builds_an_empty_relation_under_any_header`:

```rust
        assert_eq!(column.tuple_count(), 0);
        assert_eq!(hash.tuple_count(), 0);
    }
```

Replace with:

```rust
        assert_eq!(column.tuple_count(), 0);
        assert_eq!(hash.tuple_count(), 0);

        let index: crate::ds::HashIndex = build_from_an_empty_source_of_another_arity();
        assert_eq!(index.header().arity(), 2);
        assert_eq!(index.tuple_count(), 0);
    }
```

- [ ] **Step 2: Run them**

Run: `nix develop --command cargo test -p kermit-ds --lib relation::tests`

Expected: they pass on arrival, since Task 2.2 built the structure. To see an assertion can fail, temporarily change `assert_eq!(index.tuple_count(), 3);` in the `KEEPS_DUPLICATES` block to `4` and run `nix develop --command cargo test -p kermit-ds --lib keeps_duplicates_names_whether_a_repeated_row_is_kept`. Expected: `assertion `left == right` failed` with `left: 3` and `right: 4`. Restore the `3`.

- [ ] **Step 3: Implement**

Nothing to implement: the structure is Task 2.2's.

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt -p kermit-ds`, then `nix develop --command cargo test -p kermit-ds --lib relation::tests`

Expected: the existing tests and `a_rejecting_view_of_the_wrong_arity_panics_for_a_hash_index` are `ok`; `test result: ok`, with no failure. Then `nix develop --command cargo clippy -p kermit-ds --all-targets -- -D warnings` (no warnings).

- [ ] **Step 5: Commit**

```
git commit -m "test(ds): HashIndex rows in the cross-structure view tests" -- kermit-ds/src/relation.rs
```

### Task 2.4: `keyed_index_test_suite!` and the HashIndex suites

**Files:**
- Create: `kermit-ds/tests/hash_index_tests.rs`
- Modify: `kermit-ds/tests/common/macros.rs` (append after `hash_trie_test_suite!`, the file's last macro)
- Test: `kermit-ds/tests/hash_index_tests.rs`

- [ ] **Step 1: Write the failing test**

Create `kermit-ds/tests/hash_index_tests.rs`. It invokes the suite before the macro exists. `OneBucketHash` hashes every value to 0, so every row of an index lands in one chain and only the key comparison can separate the keys (the unit tests in `src/ds/hash_index/tests.rs` pin that one chain through the private arrays).

```rust
//! Contract suites for `HashIndex`, the keyed-family structure — the
//! counterpart of `hash_trie_tests.rs`.
//!
//! `HashIndex` implements `KeyedIterable`, a third iterator family, so it
//! cannot be passed to `relation_trie_test_suite!` or `hash_trie_test_suite!`;
//! `keyed_index_test_suite!` (in `common/macros.rs`) states the keyed
//! contract: `probe` lends exactly a key's rows in source order, a key of no
//! columns lends every row, inserts keep that true through every rehash.
//! One invocation per hasher alias, matching `kermit/tests/join_tests.rs`,
//! plus one under a strategy that puts every row in one bucket.

use {
    kermit_ds::HashIndex,
    kermit_iters::{FxHashStrategy, HashStrategy, KeyedIterable, LayoutOption, SipHashStrategy},
};
mod common;

type HashIndexSip = HashIndex<SipHashStrategy>;
type HashIndexFx = HashIndex<FxHashStrategy>;

/// Test-only strategy that sends every value to hash 0: every row of an index
/// lands in bucket 0, so the key comparison alone must filter. A twin lives in
/// `kermit-ds/src/ds/hash_index/tests.rs`, where it is shown to leave every
/// row in one chain. The two are deliberately not shared: neither crate target
/// should ship a colliding strategy in its public API.
#[derive(Copy, Clone, Default, Debug)]
pub struct OneBucketHash;

impl LayoutOption for OneBucketHash {
    const NAME: &'static str = "one-bucket";
}

impl HashStrategy for OneBucketHash {
    fn hash(_key: usize) -> u64 { 0 }
}

type HashIndexOneBucket = HashIndex<OneBucketHash>;

#[test]
fn the_one_bucket_strategy_hashes_every_value_alike() {
    assert_eq!(OneBucketHash::hash(1), OneBucketHash::hash(2));
    assert_eq!(OneBucketHash::hash(0), OneBucketHash::hash(usize::MAX));
}

keyed_index_test_suite!(HashIndexSip);

keyed_index_test_suite!(HashIndexFx);

keyed_index_test_suite!(HashIndexOneBucket);

/// The rows of a loaded `HashIndex`, scanned: the file's rows in file order
/// (the fixtures write them ascending).
fn scanned_rows<H: HashStrategy>(relation: &HashIndex<H>) -> Vec<Vec<usize>> {
    relation.scan().map(<[usize]>::to_vec).collect()
}

// The Parquet round-trip: an index loaded from a file keys on every column
// (the header's level count is its arity) and scans the file's rows.
parquet_test_suite!(HashIndexSip, scanned_rows);

parquet_test_suite!(HashIndexFx, scanned_rows);
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit-ds --test hash_index_tests`

Expected: the build fails, because the macro is not defined yet:

```
error: cannot find macro `keyed_index_test_suite` in this scope
  --> kermit-ds/tests/hash_index_tests.rs:..
```

- [ ] **Step 3: Implement**

Append to the end of `kermit-ds/tests/common/macros.rs`:

```rust

// ---------------------------------------------------------------------------
// Keyed-family suite.
//
// `HashIndex` implements `KeyedIterable`, a third iterator family beside the
// sorted tries' and `HashTrie`'s, so it cannot be passed to the suites above.
// The contract is exact-match: `probe(key)` lends the rows whose first
// `key_len()` values equal `key`, in source order, whatever else shares their
// bucket, so these tests state it against a brute-force filter of the rows
// and never against a structure's internals.
// ---------------------------------------------------------------------------

/// Keyed-family counterpart of `hash_trie_test_suite!`. Takes the relation
/// type (a `HashIndex<H>` alias such as `HashIndexSip`). Use a hasher that
/// maps every value to one hash (see `hash_index_tests.rs`) to make the key
/// comparison the only thing that separates keys.
#[macro_export]
macro_rules! keyed_index_test_suite {
    ($relation_type:ident) => {
        paste::paste! {
            #[cfg(test)]
            mod [<keyed_ $relation_type:lower>] {
                use {
                    super::*,
                    kermit_ds::{Cardinality, Projectable, Relation, RelationHeader},
                    kermit_iters::{HasOptimizationAxes, KeyedIterable},
                };

                /// An index over `rows` of `arity` columns, keyed on the first
                /// `key_len`.
                fn build(
                    arity: usize, key_len: usize, rows: Vec<Vec<usize>>,
                ) -> $relation_type {
                    $relation_type::from_tuples(
                        RelationHeader::new_nameless_positional(arity).with_levels(key_len),
                        rows,
                    )
                }

                /// `probe(key)` as owned rows.
                fn probed(index: &$relation_type, key: &[usize]) -> Vec<Vec<usize>> {
                    index.probe(key).map(<[usize]>::to_vec).collect()
                }

                /// `scan()` as owned rows.
                fn scanned(index: &$relation_type) -> Vec<Vec<usize>> {
                    index.scan().map(<[usize]>::to_vec).collect()
                }

                /// The rows of `rows` that start with `key`, in order: the
                /// reference every probe is compared with.
                fn matching(rows: &[Vec<usize>], key: &[usize]) -> Vec<Vec<usize>> {
                    rows.iter()
                        .filter(|row| row.starts_with(key))
                        .cloned()
                        .collect()
                }

                #[test]
                fn probe_lends_exactly_the_rows_of_a_key_in_source_order() {
                    let rows = vec![
                        vec![1, 10],
                        vec![2, 20],
                        vec![1, 11],
                        vec![3, 30],
                        vec![1, 12],
                        vec![2, 21],
                    ];
                    let index = build(2, 1, rows.clone());
                    assert_eq!(index.key_len(), 1);
                    assert_eq!(
                        probed(&index, &[1]),
                        vec![vec![1, 10], vec![1, 11], vec![1, 12]]
                    );
                    assert_eq!(probed(&index, &[2]), vec![vec![2, 20], vec![2, 21]]);
                    assert_eq!(probed(&index, &[3]), vec![vec![3, 30]]);
                    // An absent key, and a payload value, find nothing.
                    assert!(probed(&index, &[4]).is_empty());
                    assert!(probed(&index, &[10]).is_empty());
                    for key in 0..6 {
                        assert_eq!(probed(&index, &[key]), matching(&rows, &[key]), "key {key}");
                    }
                }

                #[test]
                fn a_composite_key_compares_every_key_column() {
                    let rows = vec![
                        vec![1, 1, 5],
                        vec![1, 2, 6],
                        vec![2, 1, 7],
                        vec![1, 1, 8],
                    ];
                    let index = build(3, 2, rows);
                    assert_eq!(index.key_len(), 2);
                    assert_eq!(probed(&index, &[1, 1]), vec![vec![1, 1, 5], vec![1, 1, 8]]);
                    assert_eq!(probed(&index, &[1, 2]), vec![vec![1, 2, 6]]);
                    assert_eq!(probed(&index, &[2, 1]), vec![vec![2, 1, 7]]);
                    assert!(probed(&index, &[2, 2]).is_empty());
                }

                #[test]
                fn a_key_of_no_columns_lends_every_row() {
                    let rows = vec![vec![3, 4], vec![1, 2], vec![3, 4], vec![5, 6]];
                    let index = build(2, 0, rows.clone());
                    assert_eq!(index.key_len(), 0);
                    assert_eq!(probed(&index, &[]), rows);
                    assert_eq!(probed(&index, &[]), scanned(&index));
                }

                #[test]
                fn scan_lends_every_row_in_source_order() {
                    let rows = vec![vec![3, 4], vec![1, 2], vec![3, 4], vec![5, 6]];
                    let index = build(2, 1, rows.clone());
                    assert_eq!(scanned(&index), rows);
                    assert_eq!(index.tuple_count(), 4);
                }

                #[test]
                fn an_empty_index_lends_nothing() {
                    let keyed = build(2, 1, vec![]);
                    assert!(scanned(&keyed).is_empty());
                    assert!(probed(&keyed, &[1]).is_empty());
                    assert_eq!(keyed.tuple_count(), 0);

                    let unkeyed = build(2, 0, vec![]);
                    assert!(scanned(&unkeyed).is_empty());
                    assert!(probed(&unkeyed, &[]).is_empty());
                }

                /// `KEEPS_DUPLICATES` read through a function, so that the
                /// assertion is not a constant expression.
                fn keeps_duplicates() -> bool {
                    <$relation_type as Relation>::KEEPS_DUPLICATES
                }

                #[test]
                fn duplicates_are_kept() {
                    assert!(keeps_duplicates());
                    let rows = vec![vec![1, 2], vec![1, 2], vec![1, 3]];
                    let index = build(2, 2, rows.clone());
                    assert_eq!(index.tuple_count(), 3);
                    assert_eq!(scanned(&index), rows);
                    assert_eq!(probed(&index, &[1, 2]), vec![vec![1, 2], vec![1, 2]]);
                    let by_first = build(2, 1, rows);
                    assert_eq!(probed(&by_first, &[1]).len(), 3);
                }

                /// Seventy inserts pass the directory through every doubling
                /// from 2 slots to 128; after each, every probe equals a
                /// filter of the rows inserted so far.
                #[test]
                fn insert_keeps_probe_exact_through_every_rehash() {
                    let mut index = $relation_type::new(
                        RelationHeader::new_nameless_positional(2).with_levels(1),
                    );
                    let mut inserted: Vec<Vec<usize>> = vec![];
                    for i in 0..70_usize {
                        let row = vec![i % 5, i];
                        index.insert(&row);
                        inserted.push(row);
                        for key in 0..6_usize {
                            assert_eq!(
                                probed(&index, &[key]),
                                matching(&inserted, &[key]),
                                "key {key} after {} inserts",
                                i + 1
                            );
                        }
                        assert_eq!(index.tuple_count(), i + 1);
                    }
                    assert_eq!(scanned(&index), inserted);
                }

                #[test]
                fn insert_into_an_index_with_no_key_columns() {
                    let mut index = $relation_type::new(RelationHeader::new_nameless_positional(2));
                    let mut keyless = $relation_type::new(
                        RelationHeader::new_nameless_positional(2).with_levels(0),
                    );
                    for i in 0..5_usize {
                        index.insert([i, i + 1]);
                        keyless.insert([i, i + 1]);
                    }
                    let expected: Vec<Vec<usize>> = (0..5).map(|i| vec![i, i + 1]).collect();
                    assert_eq!(scanned(&keyless), expected);
                    assert_eq!(probed(&keyless, &[]), expected);
                    // The same rows keyed on both columns find one row per key.
                    assert_eq!(probed(&index, &[2, 3]), vec![vec![2, 3]]);
                }

                #[test]
                fn insert_all_inserts_every_row() {
                    let mut index = build(2, 1, vec![vec![9, 9]]);
                    index.insert_all(vec![vec![1, 2], vec![1, 3], vec![9, 8]]);
                    assert_eq!(index.tuple_count(), 4);
                    assert_eq!(probed(&index, &[1]), vec![vec![1, 2], vec![1, 3]]);
                    assert_eq!(probed(&index, &[9]), vec![vec![9, 9], vec![9, 8]]);
                }

                #[test]
                fn project_rebuilds_the_index_keyed_on_every_projected_column() {
                    let source = build(3, 1, vec![vec![1, 2, 3], vec![1, 4, 5], vec![2, 2, 3]]);
                    let projected = source.project(vec![2, 0]);
                    assert_eq!(projected.header().arity(), 2);
                    assert_eq!(projected.key_len(), 2);
                    assert_eq!(
                        scanned(&projected),
                        vec![vec![3, 1], vec![5, 1], vec![3, 2]]
                    );
                    assert_eq!(probed(&projected, &[3, 2]), vec![vec![3, 2]]);
                    assert!(probed(&projected, &[3, 3]).is_empty());
                }

                #[test]
                #[should_panic(expected = "project: a column is out of range for arity 3")]
                fn project_of_a_column_past_the_arity_panics() {
                    let source = build(3, 1, vec![vec![1, 2, 3]]);
                    let _ = source.project(vec![3]);
                }

                #[test]
                #[should_panic(expected = "probe: a key of 1 values on an index keyed on 2 columns")]
                fn probe_with_a_key_of_the_wrong_length_panics() {
                    let index = build(3, 2, vec![vec![1, 2, 3]]);
                    let _ = index.probe(&[1]);
                }

                #[test]
                #[should_panic(expected = "tuple arity 1 does not match relation arity 2")]
                fn insert_of_the_wrong_arity_panics() {
                    let mut index = build(2, 1, vec![]);
                    index.insert([1]);
                }

                #[test]
                #[should_panic(
                    expected = "from_tuples: tuple arity 2 does not match header arity 3"
                )]
                fn build_from_tuples_of_the_wrong_arity_panics() {
                    let _ = build(3, 1, vec![vec![1, 2]]);
                }

                #[test]
                fn the_hasher_is_the_one_layout_axis() {
                    let axes = build(2, 1, vec![vec![1, 2]]).optimization_axes();
                    assert_eq!(axes.len(), 1);
                    assert!(axes.contains_key("ds_layout_hasher"));
                }
            }
        }
    };
}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt -p kermit-ds`, then `nix develop --command cargo test -p kermit-ds --test hash_index_tests`

Expected: the three keyed modules (`keyed_hashindexsip`, `keyed_hashindexfx`, `keyed_hashindexonebucket`) run fifteen tests each, the two parquet modules (`parquet_hashindexsip`, `parquet_hashindexfx`) seven each, plus `the_one_bucket_strategy_hashes_every_value_alike`; `test result: ok. 60 passed; 0 failed`.

Then `nix develop --command cargo clippy -p kermit-ds --all-targets -- -D warnings` (no warnings).

- [ ] **Step 5: Commit**

```
git add -N kermit-ds/tests/hash_index_tests.rs
git commit -m "test(ds): keyed_index_test_suite! and the HashIndex suites" -- kermit-ds/tests/common/macros.rs kermit-ds/tests/hash_index_tests.rs
```

### Task 2.5: The `HashIndex` page

**Files:**
- Create: `docs/data-structures/hash-index.md`

- [ ] **Step 1: Write the page**

Create `docs/data-structures/hash-index.md`, from `docs/data-structures/TEMPLATE.md`. A page has no test, so Step 2 is its check against the code. The Developer checks every numeric claim against `implementation.rs` while writing it: sizing, the two constants and the pinned buckets in the example.

````markdown
# `HashIndex`

> **Status:** experimental · **CLI:** `-i hash-index` (pairs only with `-a binary-hash-join`) · **Implementation:** [`kermit_ds::ds::hash_index`](../../kermit-ds/src/ds/hash_index/)

## Representation

`HashIndex<H: HashStrategy = SipHashStrategy>` is the textbook chained hash table of a binary hash join: Free Join §2.2's "hash table keyed on" the attributes a relation shares with the relations before it in a left-deep plan (Wang et al., SIGMOD 2023). It is not a trie: it has one composite level, the key, and the rest of each row is payload kept in the row.

```rust
HashIndex<H: HashStrategy = SipHashStrategy> {
    header: RelationHeader,
    rows: Tuples,            // view.materialize(): one copy, in source order
    key_len: usize,          // header.levels()
    directory: Vec<RowId>,   // `slots` bucket heads, NIL when empty; empty when key_len == 0
    next: Vec<RowId>,        // per row, the next row of its bucket's chain; empty when key_len == 0
    _hasher: PhantomData<H>, // hasher, zero-sized
}
```

`NIL` is `RowId::MAX`, which is never a row id (a `Tuples` batch holds at most `RowId::MAX` rows). `directory` and `next` are plain `Vec<RowId>`, not `ChainLinks`: `ChainLinks` exists so that a lazy `HashTrie` can re-thread a child's rows through `&self`, and a keyed index never re-threads anything.

- **Sizing.** `slots = max(2, rows.next_power_of_two())`, so the load factor is at most 1 and the bucket shift (`64 - log2(slots)`) never reaches 64.
- **Hashing.** The key hash folds `H::hash` over the key columns with `FxHash`'s combine step, `h = (h.rotate_left(5) ^ H::hash(v)).wrapping_mul(0x517c_c1b7_2722_0a95)`, from `h = 0`. The bucket is the top `log2(slots)` bits of `h.wrapping_mul(0x9E37_79B9_7F4A_7C15)` (Fibonacci hashing). `HashTrie` uses the same multiply-then-take-top-bits idea in its own code; neither structure depends on the other's.
- **Probe.** `probe(key)` hashes `key`, walks the bucket's chain, and lends each row whose first `key_len` values equal `key`; rows of other keys in the same bucket are skipped by that comparison. With `key_len == 0` it lends every row.

### Construction

`from_view(header, view)` reads a [`TupleView`](../../kermit-iters/src/tuple_view.rs) and copies the matching rows once, with `view.materialize()`: an exact-size buffer, in source order, which becomes the index's own `rows`. It then allocates `directory` and `next` and links every row from last to first, each prepended to its bucket's chain, so every chain ends in source order. That is three allocations (`rows`, `directory`, `next`) and none per input row. The build only reads the source, the catalog's flat area. There is one build and no BuildMode: `bench ds` and `bench run` time the same `from_view`. `insert` appends a row and links it at its chain's tail, and doubles the directory with a rehash once rows outnumber slots; it is for tests and `Relation::new` users and is not benchmarked.

The key length is `header.levels()`. `kermit::db::prepare` plans an index under `TrieLevels::ProbeKeys`, which sets each spec's `levels` to the length of the key its atom is probed on (`0` for the join order's first atom). `bench ds` builds from a header with no query, whose `levels()` is the arity, so the index is keyed on every column.

## Invariants

- **Chains are in source order.** Linking last to first prepends each row to its bucket, so each chain lists its rows in ascending row id; `probe` and `scan` therefore lend rows in source order. `insert` keeps this by appending to a chain's tail and relinking last to first on a rehash, so an index built by inserts has the arrays a bulk build of the same rows has (`insert_builds_the_arrays_the_bulk_build_builds`).
- **`key_len == header.levels()`**, at most the arity. `probe` asserts `key.len() == key_len()`: a key of the wrong length would compare the wrong columns.
- **The directory is a power of two, at least 2, and at least the row count**, and `directory` and `next` are empty exactly when `key_len == 0`.
- **Duplicates are kept.** `Relation::KEEPS_DUPLICATES` is `true`: a repeated row is stored and lent once per copy, `Cardinality::tuple_count` counts it, and an all-constant atom's row count multiplies a join's results, as on `HashTrie` (`kermit::db::build` clamps it only for structures that absorb duplicates).
- **No collision means no match.** A hash collision only puts rows of other keys in the same chain. The key comparison lends a row only if its key columns equal the probe's key, so `probe` is exact whatever the hasher does (`keyed_index_test_suite!` runs it under a hasher that sends every value to bucket 0).

## Complexity

Let `n` = tuple count, `a` = arity, `k` = `key_len`, and `c` = the length of the chain a probe walks (about 1 in expectation, `n` for a hasher that sends every key to one bucket).

| Operation | Time | Space | Notes |
|---|---|---|---|
| `insert(tuple)` | expected O(a + c), O(n) at a doubling | O(a) | appends to the chain's tail; a rehash relinks all `n` rows |
| `from_view(view)` | O(n · (a + k)) | O(n · a + n) | `view.materialize()` (a filtered view also costs O(s) filter tests over the source's s rows), then one hash of `k` values per row |
| `KeyedIterable::key_len()` | O(1) | | field read |
| `KeyedIterable::scan()` | O(1) to start, O(1) per row | | walks `rows` |
| `KeyedIterable::probe(key)` | expected O(k + c · k) | | one hash of `k` values, then up to `c` key comparisons of `k` values; `O(1)` to start when `k == 0` |
| `HeapSize::heap_size_bytes()` | O(1) | | `rows` + `directory` + `next`, by capacity |

Space is `8 · a · n` bytes of rows, `4 · slots` bytes of directory (at most `8 · n` once `n ≥ 1`, since `slots ≤ 2n`) and `4 · n` bytes of links: at most `8 · a · n + 12 · n` bytes.

## Worked micro-example

The triangle `Q(A, B, C) :- R(A, B), S(B, C), T(A, C)` under the variable order `[A, B, C]` has the join order `R, S, T` (Free Join Example 2.2): `R` is scanned, `S` is probed on `B`, `T` on `(A, C)`. So `S` is an index of `key_len == 1` and `T` of `key_len == 2`. The illustration takes the identity hash `h(v) = v`, which is not one of kermit's hashers (they scatter the keys); the structure is the same.

`S(B, C) = {(2, 3), (2, 4), (3, 4)}`, keyed on `B`. Three rows, so 4 slots (the top two bits of the Fibonacci product): key `(2)` falls in bucket 0 and key `(3)` in bucket 2.

```
rows        0:(2,3)   1:(2,4)   2:(3,4)
directory   [0]=row 0  [1]=NIL  [2]=row 2  [3]=NIL
next        row 0 → row 1   row 1 → NIL   row 2 → NIL
```

- `probe(&[2])`: bucket 0, row 0 `(2, 3)` key matches, row 1 `(2, 4)` key matches, end. Both rows lent, in source order.
- `probe(&[3])`: bucket 2, row 2 `(3, 4)`. One row.

`T(A, C) = {(1, 3), (1, 4)}`, keyed on both columns. Two rows, so 2 slots (the top bit): keys `(1, 3)` and `(1, 4)` both fall in bucket 1, a collision.

```
rows        0:(1,3)   1:(1,4)
directory   [0]=NIL    [1]=row 0
next        row 0 → row 1   row 1 → NIL
```

- `probe(&[1, 4])`: bucket 1, row 0 `(1, 3)` is skipped (its key differs), row 1 `(1, 4)` is lent.
- `probe(&[1, 5])`: bucket 0 is empty, nothing is lent.

Take `R = {(1, 2), (1, 3)}`. For `(A, B) = (1, 2)`, `S.probe(&[2])` lends `(2, 3)` and `(2, 4)`; `T.probe(&[1, 3])` and `T.probe(&[1, 4])` each lend one row, so `(1, 2, 3)` and `(1, 2, 4)` are emitted. For `(1, 3)`, `S.probe(&[3])` lends `(3, 4)` and `T.probe(&[1, 4])` lends `(1, 4)`: `(1, 3, 4)`. The work is the sum of the intermediate results, `|R ⋈ S|` probes into `T`; see [`binary-hash-join.md`](../algorithms/binary-hash-join.md).

## When to prefer this structure

It is the baseline's structure, not a competitor to the tries: it exists to put a number on what worst-case optimal joins buy over a conventional binary plan. Use it with `-a binary-hash-join`, the only algorithm that drives it, and compare it with `HashTrie` under the same `--ds-layout-hasher`. On acyclic queries with selective joins it is a sound plan; on cyclic queries (a triangle, a 4-cycle) its intermediate results can exceed the AGM bound by orders of magnitude, which is the gap the baseline measures.

## Optimizations

| Dimension | Category | Axis | Flag | Default | Test aliases |
|---|---|---|---|---|---|
| Hasher | Layout (`H: HashStrategy`) | `ds_layout_hasher` | `--ds-layout-hasher sip\|fxhash` | `sip` | `HashIndexSip`, `HashIndexFx` |

The hasher changes only the key hash: the sizing, the build and the answers are the same under every `H`. It reuses `HashTrie`'s flag and its two hashers, so a HashTrie-against-HashIndex comparison can hold the hasher fixed. There are no Config and no BuildMode axes: the baseline has one build with a fixed sizing rule, so `--ds-config` and `--ds-build` are refused for `hash-index`.

## See also

- [`HashTrie`](./hash-trie.md) — the hash-based trie; the same hashers, a trie's levels instead of one composite key.
- [`BinaryHashJoin`](../algorithms/binary-hash-join.md) — the algorithm that drives this structure.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; `HashIndex` must pass all 16 patterns under `BinaryHashJoin` and every optimiser (Priorities item 1).
- [`kermit-ds/tests/hash_index_tests.rs`](../../kermit-ds/tests/hash_index_tests.rs) — the keyed contract suite, run under both hashers and a one-bucket strategy.
````

- [ ] **Step 2: Check the page against the code**

Run: `grep -n 'FX_SEED\|FIBONACCI\|next_power_of_two' kermit-ds/src/ds/hash_index/implementation.rs`

Expected: the two multipliers on the page equal `0x517c_c1b7_2722_0a95` and `0x9E37_79B9_7F4A_7C15`, and the sizing line on the page equals `rows.next_power_of_two().max(2)`.

Run: `nix develop --command cargo test -p kermit-ds --lib ds::hash_index::tests::the_build_places_every_row_in_its_pinned_bucket`

Expected: `ok`, the same bucket arithmetic the page's example relies on (the example's buckets, 0 and 2 of 4 for keys 2 and 3, and 1 and 1 of 2 for `(1, 3)` and `(1, 4)`, come from the same fold and were computed independently of the Rust code).

- [ ] **Step 3: Implement**

Nothing beyond the page itself.

- [ ] **Step 4: Run the story's verification**

Run: `nix develop --command cargo test -p kermit-iters -p kermit-ds`

Expected: every test passes (`test result: ok.` for each target, including `hash_index_tests` with 60 tests, `ds::hash_index::tests` with 14, `keyed::tests` with 2 and the `HashIndex` doctest). Then the story gates for these two packages and the packages that depend on them (`kermit-algos`, `kermit`): `nix develop --command cargo fmt --all -- --check`, `nix develop --command cargo clippy -p kermit-iters -p kermit-ds -p kermit-algos -p kermit --all-targets -- -D warnings`, `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit-iters -p kermit-ds -p kermit-algos -p kermit'`.

- [ ] **Step 5: Commit**

```
git add -N docs/data-structures/hash-index.md
git commit -m "docs(ds): the HashIndex page" -- docs/data-structures/hash-index.md
```

## Story 3: `BinaryHashJoin`, its entry points, and the library-level test cells

- **Developer:** intermediate-dev
- **Files:** `kermit-algos/src/keyed/mod.rs`, `kermit-algos/src/keyed/binary_hash_join.rs`, `kermit-algos/src/lib.rs`, `kermit/src/db.rs`, `kermit/src/db/lifecycle.rs`, `kermit/tests/common/utils.rs`, `kermit/tests/join_tests.rs`, `kermit/tests/all_orders.rs`, `kermit/tests/result_allocation.rs`, `kermit/tests/tuple_allocation.rs`, `kermit/tests/lubm_mini_oracle.rs`, `kermit/tests/lubm_cardinalities.rs`, `docs/algorithms/binary-hash-join.md`
- **Interfaces:** exposes `BinaryHashJoin` and `binary_join_for_each` / `binary_join` (names contract, Story 3) to Story 4; uses `ProbeKeys` planning (Story 1) and `KeyedIterable` / `HashIndex` (Story 2)
- **Verify:** `nix develop --command bash -c 'cargo test -p kermit-algos && cargo test -p kermit --lib --test join_tests --test all_orders --test result_allocation --test tuple_allocation --test lubm_mini_oracle --test lubm_cardinalities'`
- **Review:** deep

This story adds the keyed family's algorithm, `BinaryHashJoin` (Free Join §2.2's left-deep pipelined hash join), in `kermit-algos/src/keyed/`, and makes it reachable from the library: `kermit::db::binary_join_for_each` / `binary_join` and a `JoinEntry` impl for the tests. The algorithm reads the executed body left to right, one loop level per atom, with scratch allocated once per join and the "probe keys lead the columns" assertion. Every cross-structure test file then gains its `HashIndex` row: the join suite (six invocations), the every-variable-order cell, the two allocation files, both LUBM oracles, and two `kermit::db` lifecycle tests. The tests rely on `prepare` under `TrieLevels::ProbeKeys` (Story 1) and on `HashIndex` / `KeyedIterable` (Story 2). The registry variants `IndexStructure::HashIndex` and `JoinAlgorithm::BinaryHashJoin` wait for Story 4, so the binary is untouched. No code here holds a landmine marker. `all_orders.rs` needs one change to existing code, in `rows_in_order`: under `ProbeKeys` the plan `prepare` returns is V′, not the forced order (Task 3.4).

### Task 3.1: `BinaryHashJoin` and the keyed family module

**Files:**
- Create: `kermit-algos/src/keyed/mod.rs`
- Create: `kermit-algos/src/keyed/binary_hash_join.rs`
- Modify: `kermit-algos/src/lib.rs` (the `mod` list, the `pub use` list and the crate docs' `# Layout` paragraph)
- Test: `kermit-algos/src/keyed/binary_hash_join.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Create `kermit-algos/src/keyed/mod.rs` (complete):

```rust
//! The keyed join family: algorithms over
//! [`KeyedIterable`](kermit_iters::KeyedIterable) relations.
//!
//! Every module here reads a relation through an index keyed on its leading
//! columns: [`key_len`](kermit_iters::KeyedIterable::key_len) says how many,
//! [`scan`](kermit_iters::KeyedIterable::scan) lends every row, and
//! [`probe`](kermit_iters::KeyedIterable::probe) lends the rows whose key
//! equals a given one, by exact match. A probe imposes no order on the data
//! and the index has no levels to descend, so the family shares nothing with
//! an iterator that seeks or opens (rationale in `kermit-iters/src/keyed.rs`).
//!
//! An algorithm here runs over the executed query that `kermit::db::prepare`
//! builds under [`TrieLevels::ProbeKeys`](kermit_iters::TrieLevels::ProbeKeys):
//! its body atoms are in join order, each names an index over its variables
//! in column order, and each index's key is the leading columns that the
//! atoms before it bind. The algorithm reads the body left to right and
//! derives no join order of its own.
//!
//! The module shape follows the other families: one module per algorithm
//! ([`binary_hash_join`]), named to match its `docs/algorithms/<name>.md`
//! page and its CLI value.

mod binary_hash_join;

pub use binary_hash_join::BinaryHashJoin;
```

Create `kermit-algos/src/keyed/binary_hash_join.rs` with the module docs, the final `use` block, a temporary stub (Step 3 of this task replaces it) and the complete tests:

```rust
//! Binary Hash Join: the left-deep, pipelined hash join of Free Join §2.2.
//!
//! The baseline that worst-case optimal joins are measured against. A binary
//! plan joins one atom at a time. Written as a left-deep plan `[R_1, …, R_m]`
//! it is executed by pipelining (Wang et al., "Free Join: Unifying
//! Worst-Case Optimal and Traditional Joins", PACMMOD 2023, §2.2, Figure 2a):
//! the engine iterates over each tuple of the first atom, probes the hash
//! table of the second atom on the variables the first binds, and for each
//! match probes the third, and so on. Nothing intermediate is stored. For
//! the triangle `Q(x, y, z) :- R(x, y), S(y, z), T(x, z)`:
//!
//! ```text
//! for (x, y) in R:
//!     for (y, z) in S[y]:        // a failed lookup continues the loop above
//!         for (x, z) in T[x, z]:
//!             output(x, y, z)
//! ```
//!
//! The work is proportional to the sum of the intermediate results (on the
//! triangle, `|R ⋈ S|` probes into `T`), which can exceed the AGM bound that
//! worst-case optimal joins meet. That gap is what this algorithm is in
//! kermit to measure.
//!
//! **Where the join order comes from.** The executed query's body is already
//! in join order, and each atom's index is keyed on the variables the atoms
//! before it bind: `kermit_algos::plan_indexes` under
//! [`TrieLevels::ProbeKeys`](kermit_iters::TrieLevels::ProbeKeys) derives both
//! from the optimiser's variable order (the `join_order` rule of
//! `docs/algorithms/binary-hash-join.md`). This module derives nothing: step
//! `j` is body atom `j`.

use {
    crate::{analysis::analyse, join_algo::JoinAlgo, optimiser::QueryPlan},
    kermit_iters::KeyedIterable,
    kermit_parser::JoinQuery,
    std::collections::HashMap,
};

/// Temporary stub: Step 3 of Task 3.1 replaces it with the algorithm.
pub struct BinaryHashJoin;

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_ds::{HashIndex, Relation, RelationHeader},
        kermit_iters::Tuples,
    };

    /// `rows` as an index of `arity` columns keyed on its first `levels`.
    fn index(name: &str, arity: usize, levels: usize, rows: &[&[usize]]) -> HashIndex {
        let mut tuples = Tuples::new(arity);
        for row in rows {
            tuples.push(row);
        }
        HashIndex::from_tuples(
            RelationHeader::new_positional(name, arity).with_levels(levels),
            tuples,
        )
    }

    /// `query` joined by [`BinaryHashJoin`] under the plan `order`, over the
    /// named indexes, with the rows sorted.
    fn joined(order: &[usize], query: &str, indexes: &[(&str, &HashIndex)]) -> Vec<Vec<usize>> {
        let plan = QueryPlan {
            variable_ordering: order.to_vec(),
        };
        let datastructures: HashMap<String, &HashIndex> = indexes
            .iter()
            .map(|&(name, index)| (name.to_string(), index))
            .collect();
        let mut rows = Vec::new();
        BinaryHashJoin::join_for_each(&plan, query.parse().unwrap(), datastructures, |row| {
            rows.push(row.to_vec());
        });
        rows.sort();
        rows
    }

    const TRIANGLE: &str = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).";

    /// The triangle as `kermit::db::prepare` plans it under `ProbeKeys` for
    /// the variable order `[X, Y, Z]`: `R` is the driver (no key), `S` is
    /// keyed on `Y`, `T` on `(X, Z)`. `|R ⋈ S|` is 5, the output 3: the
    /// worked micro-example of `docs/algorithms/binary-hash-join.md`.
    fn triangle_indexes() -> (HashIndex, HashIndex, HashIndex) {
        (
            index("R", 2, 0, &[&[1, 2], &[1, 3], &[2, 3]]),
            index("S", 2, 1, &[&[2, 3], &[3, 4], &[3, 5]]),
            index("T", 2, 2, &[&[1, 3], &[1, 4], &[2, 5]]),
        )
    }

    #[test]
    fn the_triangle_probes_each_atom_on_what_the_earlier_ones_bind() {
        let (r, s, t) = triangle_indexes();
        let rows = joined(&[0, 1, 2], TRIANGLE, &[("R", &r), ("S", &s), ("T", &t)]);
        assert_eq!(rows, vec![vec![1, 2, 3], vec![1, 3, 4], vec![2, 3, 5]]);
    }

    /// An atom sharing no variable with the earlier ones has an empty key, and
    /// its probe lends every row: the cross product.
    #[test]
    fn an_empty_key_crosses_every_row() {
        let r = index("R", 2, 0, &[&[1, 2], &[3, 4]]);
        let s = index("S", 2, 0, &[&[5, 6], &[7, 8]]);
        let rows = joined(
            &[0, 1, 2, 3],
            "Q(A, B, C, D) :- R(A, B), S(C, D).",
            &[("R", &r), ("S", &s)],
        );
        assert_eq!(rows, vec![
            vec![1, 2, 5, 6],
            vec![1, 2, 7, 8],
            vec![3, 4, 5, 6],
            vec![3, 4, 7, 8],
        ]);
    }

    /// A probe that finds nothing ends its branch, and an empty atom ends the
    /// whole join.
    #[test]
    fn a_probe_with_no_match_ends_its_branch() {
        let (r, s, t) = triangle_indexes();
        let no_s = index("S", 2, 1, &[]);
        let no_t = index("T", 2, 2, &[&[9, 9]]);
        assert!(joined(&[0, 1, 2], TRIANGLE, &[("R", &r), ("S", &no_s), ("T", &t)]).is_empty());
        assert!(joined(&[0, 1, 2], TRIANGLE, &[("R", &r), ("S", &s), ("T", &no_t)]).is_empty());
    }

    /// Duplicate rows are matched separately: bag semantics, as for
    /// `HashTrie`.
    #[test]
    fn duplicate_rows_multiply() {
        let r = index("R", 1, 0, &[&[1], &[1]]);
        let s = index("S", 2, 1, &[&[1, 5]]);
        let rows = joined(
            &[0, 1],
            "Q(X, Y) :- R(X), S(X, Y).",
            &[("R", &r), ("S", &s)],
        );
        assert_eq!(rows, vec![vec![1, 5], vec![1, 5]]);
    }

    /// Rows are emitted by canonical variable index (head variables first), not
    /// by atom column: `Q(Y, X) :- R(X, Y)` numbers `Y` 0 and `X` 1, so the
    /// plan binds `X` first (`[1, 0]`) and the row is `[y, x]`.
    #[test]
    fn rows_are_emitted_in_canonical_variable_order() {
        let r = index("R", 2, 0, &[&[1, 10], &[2, 20]]);
        let rows = joined(&[1, 0], "Q(Y, X) :- R(X, Y).", &[("R", &r)]);
        assert_eq!(rows, vec![vec![10, 1], vec![20, 2]]);
    }

    /// A variable no atom carries can take no value, so the query has no
    /// answer. Validation rules it out for every query from `kermit::db`.
    #[test]
    fn a_variable_no_atom_carries_has_no_answer() {
        let r = index("R", 1, 0, &[&[1]]);
        assert!(joined(&[0, 1], "Q(X, W) :- R(X).", &[("R", &r)]).is_empty());
    }

    #[test]
    #[should_panic(expected = "invalid query plan")]
    fn a_plan_that_is_not_a_permutation_is_refused() {
        let (r, s, t) = triangle_indexes();
        let _ = joined(&[0, 0, 1], TRIANGLE, &[("R", &r), ("S", &s), ("T", &t)]);
    }

    /// Indexes keyed on every column, as `bench ds` builds them (the header's
    /// `levels()` is the arity), are not planned for a join: the driver's key
    /// is not bound by any atom before it.
    #[test]
    #[should_panic(expected = "probe keys lead the columns")]
    fn an_index_keyed_on_every_column_is_refused() {
        let (_, s, t) = triangle_indexes();
        let r = index("R", 2, 2, &[&[1, 2]]);
        let _ = joined(&[0, 1, 2], TRIANGLE, &[("R", &r), ("S", &s), ("T", &t)]);
    }

    /// `S(Z, Y)` is keyed on `Z`, which `R(X, Y)` does not bind: a query not
    /// planned under `ProbeKeys`.
    #[test]
    #[should_panic(expected = "probe keys lead the columns")]
    fn a_key_no_earlier_atom_binds_is_refused() {
        let r = index("R", 2, 0, &[&[1, 2]]);
        let s = index("S", 2, 1, &[&[3, 2]]);
        let _ = joined(
            &[0, 2, 1],
            "Q(X, Y, Z) :- R(X, Y), S(Z, Y).",
            &[("R", &r), ("S", &s)],
        );
    }
}
```

Edit `kermit-algos/src/lib.rs`:
- Add `mod keyed;` on its own line directly above the line `mod optimiser;`.
- In the `pub use { … }` list, add `keyed::BinaryHashJoin,` on a new line directly after `kermit_parser::JoinQuery,`.
- The crate docs' `# Layout` paragraph counts the crate's parts in its first sentence ("The crate is organised in seven parts" before this story; Story 1 may already have raised the number). Add one to whatever number word it states now, because `keyed` is one more part.
- In the same paragraph, the line that ends "The remaining two are the" (it begins with `optimiser` plans a `QueryPlan`) must end "The remaining three are the" instead. Then replace the lines

```rust
//! iterator families, `sorted` (over
//! [`TrieIterable`](kermit_iters::TrieIterable)) and `hash` (over
//! [`HashTrieIterable`](kermit_iters::HashTrieIterable)); they share no
//! code and never reference each other.
```

with

```rust
//! iterator families, `sorted` (over
//! [`TrieIterable`](kermit_iters::TrieIterable)), `hash` (over
//! [`HashTrieIterable`](kermit_iters::HashTrieIterable)) and `keyed` (over
//! [`KeyedIterable`](kermit_iters::KeyedIterable)); they share no code and
//! never reference each other.
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit-algos binary_hash_join`

Expected: the test module does not compile: `error[E0277]: the trait bound \`BinaryHashJoin: JoinAlgo<HashIndex>\` is not satisfied` (and `no function or associated item named \`join_for_each\` found for struct \`BinaryHashJoin\``), since the stub implements nothing.

- [ ] **Step 3: Implement**

In `kermit-algos/src/keyed/binary_hash_join.rs`, replace the stub

```rust
/// Temporary stub: Step 3 of Task 3.1 replaces it with the algorithm.
pub struct BinaryHashJoin;
```

with the complete algorithm (everything between the `use` block and `#[cfg(test)]`):

```rust
/// One level of the loop nest: a body atom, probed on the variables the
/// atoms before it bind.
struct Step<'a, DS> {
    /// The atom's index. Its first `key_len` columns are the probe key.
    index: &'a DS,
    /// The atom's variables (canonical indices), one per column, in column
    /// order (the paper's `x_j`).
    variables: &'a [usize],
    /// How many leading columns form the key: `index.key_len()`.
    key_len: usize,
}

impl<'a, DS: KeyedIterable> Step<'a, DS> {
    /// The step for the atom `name`, the `position`-th of the body, with
    /// `bound[v]` recording the variables the steps before it bind. Marks the
    /// variables this step binds.
    ///
    /// # Panics
    ///
    /// If the atom's index is not keyed on exactly the variables bound
    /// before it: the invariant "probe keys lead the columns". A key column
    /// that no earlier atom binds would probe with a stale value, and a
    /// column past the key that an earlier atom or column binds already would
    /// overwrite a bound variable instead of joining on it.
    fn new(
        position: usize, name: &str, index: &'a DS, variables: &'a [usize], bound: &mut [bool],
    ) -> Self {
        let key_len = index.key_len();
        assert!(
            key_len <= variables.len() && variables[..key_len].iter().all(|&v| bound[v]),
            "probe keys lead the columns: atom {position} ({name}) is keyed on its first \
             {key_len} of {} columns, but the atoms before it do not bind them all",
            variables.len()
        );
        for &variable in &variables[key_len..] {
            assert!(
                !bound[variable],
                "probe keys lead the columns: atom {position} ({name}) carries variable \
                 {variable} past its key, but an earlier atom or column already binds it"
            );
            bound[variable] = true;
        }
        Self {
            index,
            variables,
            key_len,
        }
    }
}

/// Free Join Figure 2a's loop nest from step `steps[0]` on, for the prefix
/// `row` binds so far.
///
/// Probes the first step's index on the key `row` holds for its leading
/// variables, writes each match's remaining columns into `row`, and descends
/// into the later steps. Past the last step every variable is bound and `row`
/// is emitted. `keys` is scratch, `key_len` slots per step, so the recursion
/// allocates nothing: this level's key is the front of `keys`, kept intact
/// while the deeper levels use the rest.
fn descend<DS: KeyedIterable, S: FnMut(&[usize])>(
    steps: &[Step<'_, DS>], row: &mut [usize], keys: &mut [usize], emit: &mut S,
) {
    let Some((step, later)) = steps.split_first() else {
        emit(row);
        return;
    };
    let (key, later_keys) = keys.split_at_mut(step.key_len);
    for (slot, &variable) in key.iter_mut().zip(step.variables) {
        *slot = row[variable];
    }
    let key: &[usize] = key;
    for tuple in step.index.probe(key) {
        for (&variable, &value) in step.variables[step.key_len..]
            .iter()
            .zip(&tuple[step.key_len..])
        {
            row[variable] = value;
        }
        descend(later, row, later_keys, emit);
    }
}

/// Entry point for the binary hash join.
///
/// Implements [`JoinAlgo`] for any [`KeyedIterable`] data structure. See the
/// module docs for the algorithm. The query it runs is the executed query of
/// `kermit::db::prepare` under
/// [`TrieLevels::ProbeKeys`](kermit_iters::TrieLevels::ProbeKeys): body atom
/// `j` is step `j`, and its index is keyed on the variables the atoms before
/// it bind.
///
/// # Panics
///
/// `join_for_each` panics if the plan is invalid, and if an index is not
/// keyed on exactly the variables the atoms before it bind (the panic message
/// starts with `probe keys lead the columns`). Both are caller errors:
/// `kermit::db::build` already refuses an index set planned under another
/// trie-level policy.
pub struct BinaryHashJoin;

impl<DS> JoinAlgo<DS> for BinaryHashJoin
where
    DS: KeyedIterable,
{
    fn join_for_each<S: FnMut(&[usize])>(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>, mut emit: S,
    ) {
        let analysis = analyse(&query);
        if let Err(e) = plan.validate(&analysis) {
            panic!("BinaryHashJoin::join_for_each: invalid query plan: {e}");
        }
        // One step per body atom, in body order, which is the join order.
        // `bound` records which variables the steps so far bind.
        let mut bound = vec![false; analysis.num_vars];
        let mut steps = Vec::with_capacity(query.body.len());
        for (position, (atom, variables)) in query
            .body
            .iter()
            .zip(&analysis.predicate_variables)
            .enumerate()
        {
            let index = *datastructures
                .get(&atom.name)
                .expect("Missing datastructure for predicate name");
            steps.push(Step::new(position, &atom.name, index, variables, &mut bound));
        }
        // A variable no atom carries can take no value, so the query has no
        // answer (validation rules it out for every query from `kermit::db`).
        if !bound.iter().all(|&b| b) {
            return;
        }
        // The only allocations of the join besides the steps: the row, in
        // canonical variable order, and the keys, `key_len` slots per step.
        let mut row = vec![0; analysis.num_vars];
        let mut keys = vec![0; steps.iter().map(|step| step.key_len).sum()];
        descend(&steps, &mut row, &mut keys, &mut emit);
    }
}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt --all && nix develop --command cargo test -p kermit-algos binary_hash_join`

Expected: `test result: ok. 9 passed` for the `keyed::binary_hash_join::tests` module (the other `kermit-algos` tests are filtered out).

Then the story gates for this crate: `nix develop --command cargo clippy -p kermit-algos --all-targets -- -D warnings` and `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit-algos'`. Both pass with no output beyond the build lines.

- [ ] **Step 5: Commit**

```
git add -N kermit-algos/src/keyed/mod.rs kermit-algos/src/keyed/binary_hash_join.rs
git commit -m "feat(algos): BinaryHashJoin, the keyed family's left-deep pipelined hash join" -- kermit-algos/src/keyed/mod.rs kermit-algos/src/keyed/binary_hash_join.rs kermit-algos/src/lib.rs
```

### Task 3.2: `binary_join_for_each` / `binary_join` and the `kermit::db` lifecycle tests

**Files:**
- Modify: `kermit/src/db.rs` (module docs `:1-24`, imports `:33-37`, new functions after `hash_join`, tests)
- Modify: `kermit/src/db/lifecycle.rs` (the test module's imports `:435-446`, two new tests at its end)
- Test: `kermit/src/db.rs` and `kermit/src/db/lifecycle.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

In `kermit/src/db.rs`'s `mod tests`, replace the import line

```rust
        kermit_ds::{HashTrie, RelationHeader, TreeTrie, Tuples},
```

with

```rust
        kermit_ds::{HashIndex, HashTrie, RelationHeader, TreeTrie, Tuples},
```

and append these tests at the end of `mod tests`, after `names_that_used_to_be_reserved_are_ordinary_relations` (the last test, ending the file):

```rust
    fn keyed_family(catalog: &Catalog, query: &JoinQuery) -> Result<Rows, JoinError> {
        let planner = planner_of(LexicographicOptimiser);
        binary_join::<HashIndex>(catalog, query.clone(), &planner).map(|(_, rows)| rows.to_vecs())
    }

    /// The keyed family (`HashIndex` under `BinaryHashJoin`) answers like the
    /// hash family, bag semantics included: a triangle, a self-join read in
    /// both column orders (two indexes of different key lengths), a cross
    /// product (an atom with an empty key that is not the driver), duplicate
    /// rows, and a duplicated join partner.
    #[test]
    fn the_keyed_family_answers_like_the_hash_family() {
        let catalog = counts(vec![
            ("r", 2, vec![vec![1, 2], vec![1, 3], vec![2, 3]]),
            ("s", 2, vec![vec![2, 3], vec![3, 4], vec![3, 5]]),
            ("t", 2, vec![vec![1, 3], vec![1, 4], vec![2, 5]]),
            ("edge", 2, mutual_edges()),
            ("dup", 2, vec![vec![1, 7], vec![1, 7], vec![2, 8]]),
        ]);
        let cases: Vec<(&str, Rows)> = vec![
            ("Q(X, Y, Z) :- r(X, Y), s(Y, Z), t(X, Z).", vec![
                vec![1, 2, 3],
                vec![1, 3, 4],
                vec![2, 3, 5],
            ]),
            ("Q(X, Y) :- edge(X, Y), edge(Y, X).", vec![
                vec![1, 2],
                vec![2, 1],
                vec![2, 3],
                vec![3, 2],
            ]),
            ("Q(X, Z) :- r(X, _), s(Z, c3).", vec![
                vec![1, 2],
                vec![1, 2],
                vec![2, 2],
            ]),
            ("Q(X, Y) :- dup(X, Y).", vec![
                vec![1, 7],
                vec![1, 7],
                vec![2, 8],
            ]),
            ("Q(X) :- dup(X, c7), r(X, Y).", vec![vec![1]; 4]),
        ];
        for (text, want) in cases {
            let query: JoinQuery = text.parse().unwrap();
            let mut keyed = keyed_family(&catalog, &query).unwrap();
            keyed.sort();
            assert_eq!(keyed, want, "{text}");
            let mut hashed = hash_family(&catalog, &query).unwrap();
            hashed.sort();
            assert_eq!(hashed, want, "{text} (hash family)");
        }
    }

    /// The streaming entry point visits the rows `binary_join` collects,
    /// without collecting.
    #[test]
    fn the_keyed_streaming_entry_point_visits_every_row() {
        let catalog = counts(vec![
            ("first", 1, vec![vec![1], vec![2], vec![3]]),
            ("second", 1, vec![vec![2], vec![3], vec![4]]),
        ]);
        let query: JoinQuery = "Q(X) :- first(X), second(X).".parse().unwrap();
        let planner = planner_of(LexicographicOptimiser);
        let mut streamed = Vec::new();
        binary_join_for_each::<HashIndex>(&catalog, query.clone(), &planner, |row| {
            streamed.push(row.to_vec());
        })
        .unwrap();
        streamed.sort();
        assert_eq!(streamed, vec![vec![2], vec![3]]);

        let (header, rows) = binary_join::<HashIndex>(&catalog, query, &planner).unwrap();
        assert_eq!(header.name(), "Q");
        assert_eq!(header.attrs().to_vec(), vec!["X"]);
        let mut collected = rows.to_vecs();
        collected.sort();
        assert_eq!(collected, streamed);
    }

    /// Validation runs before the keyed family's code: every #78 shape gets
    /// the hash family's error, and nothing is emitted.
    #[test]
    fn the_keyed_family_rejects_malformed_queries_like_the_hash_family() {
        let catalog = counts(vec![("edge", 2, vec![vec![1, 2], vec![2, 1], vec![2, 3]])]);
        let planner = planner_of(LexicographicOptimiser);
        for q in [
            "Q(X, Y) :- edge(X, Z).",
            "Q(X, X) :- edge(X, Y).",
            "Q(X, Y, Z) :- edge(X, Y, Z).",
            "Q(X) :- edge(X).",
            "Q(X, Y) :- nope(X, Y).",
            "Q(_) :- edge(X, Y).",
            "Q(X) :- edge(X, cfoo).",
        ] {
            let query: JoinQuery = q.parse().unwrap();
            let keyed = keyed_family(&catalog, &query);
            assert!(keyed.is_err(), "{q} was accepted");
            assert_eq!(keyed, hash_family(&catalog, &query), "{q}");
            let mut rows = 0;
            let _ = binary_join_for_each::<HashIndex>(&catalog, query, &planner, |_| rows += 1);
            assert_eq!(rows, 0, "{q}");
        }
    }
```

In `kermit/src/db/lifecycle.rs`'s `mod tests`, replace the import block

```rust
    use {
        super::*,
        kermit_algos::{
            analyse, CardinalityOptimiser, CostBasedOptimiser, HashTriejoin, LeapfrogTriejoin,
            LexicographicOptimiser, QueryOptimiser, RewriteError,
        },
        kermit_ds::{Cardinality, Filter, HashTrie, RelationHeader, TreeTrie},
        std::{cell::RefCell, rc::Rc},
    };
```

with

```rust
    use {
        super::*,
        kermit_algos::{
            analyse, BinaryHashJoin, CardinalityOptimiser, CostBasedOptimiser, HashTriejoin,
            LeapfrogTriejoin, LexicographicOptimiser, QueryOptimiser, RewriteError,
        },
        kermit_ds::{Cardinality, Filter, HashIndex, HashTrie, RelationHeader, TreeTrie},
        std::{cell::RefCell, rc::Rc},
    };
```

and append these two tests at the end of `mod tests` (after `a_count_multiplies_a_zero_level_index`, the last test):

```rust
    /// `HashIndex` keeps duplicate rows like `HashTrie`, so an all-constant
    /// atom multiplies the answer under `ProbeKeys` too: `s(c7, c8)` holds
    /// twice and `t(c5)` three times, so `r`'s two rows are answered six times
    /// each. A count of zero empties the product.
    #[test]
    fn checks_multiply_on_a_hash_index() {
        let c = catalog(vec![
            ("r", 1, vec![vec![1], vec![2]]),
            ("s", 2, vec![vec![7, 8], vec![7, 8], vec![9, 9]]),
            ("t", 1, vec![vec![5], vec![5], vec![5]]),
        ]);
        let multiplicity = |text: &str| {
            let prepared =
                prepare(&c, &parse(text), &lexicographic(), TrieLevels::ProbeKeys).unwrap();
            build::<HashIndex>(&c, &prepared).multiplicity()
        };
        assert_eq!(multiplicity("Q(X) :- r(X)."), 1);
        assert_eq!(multiplicity("Q(X) :- r(X), s(c7, c8)."), 2);
        assert_eq!(multiplicity("Q(X) :- r(X), s(c7, c8), t(c5)."), 6);
        assert_eq!(multiplicity("Q(X) :- r(X), s(c7, c8), s(c7, c9)."), 0);

        let query = parse("Q(X) :- r(X), s(c7, c8), t(c5).");
        let mut copies = vec![vec![1]; 6];
        copies.extend(vec![vec![2]; 6]);
        assert_eq!(
            join_with::<HashIndex, BinaryHashJoin>(&c, &query, &lexicographic()),
            copies
        );
        let none = parse("Q(X) :- r(X), s(c7, c9).");
        assert!(join_with::<HashIndex, BinaryHashJoin>(&c, &none, &lexicographic()).is_empty());
    }

    /// A query prepared for `ProbeKeys` names its atoms in join order and
    /// keys each index on the variables the earlier atoms bind. A structure
    /// with another policy must refuse it, as it refuses a `JoinKeys` plan.
    #[test]
    #[should_panic(expected = "trie levels")]
    fn a_sorted_trie_refuses_a_query_prepared_for_probe_key_levels() {
        let c = catalog(vec![
            ("r", 2, vec![vec![1, 2]]),
            ("s", 2, vec![vec![2, 3]]),
        ]);
        let probe = prepare(
            &c,
            &parse("Q(X, Y, Z) :- r(X, Y), s(Y, Z)."),
            &lexicographic(),
            TrieLevels::ProbeKeys,
        )
        .unwrap();
        let _ = build::<TreeTrie>(&c, &probe);
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --lib`

Expected: the `kermit` lib test target does not compile: `error[E0425]: cannot find function \`binary_join\` in this scope` and `cannot find function \`binary_join_for_each\` in this scope`, both from `kermit/src/db.rs`'s tests. (The two lifecycle tests compile: they use `join_with`, which exists, and need only Stories 1 and 2.)

- [ ] **Step 3: Implement**

In `kermit/src/db.rs`, replace the module-doc passage

```rust
//! sink. Each iterator family has one streaming function that runs all
//! three — [`lftj_join_for_each`] for sorted tries under a
//! [`TrieIterable`]-family algorithm, [`hash_join_for_each`] for
//! [`HashTrieIterable`] structures under [`HashTriejoin`] — and one
//! collecting function ([`lftj_join`], [`hash_join`]) whose sink is a flat
//! [`Tuples`] batch. A collected answer is a relation: the
```

with

```rust
//! sink. Each iterator family has one streaming function that runs all
//! three — [`lftj_join_for_each`] for sorted tries under a
//! [`TrieIterable`]-family algorithm, [`hash_join_for_each`] for
//! [`HashTrieIterable`] structures under [`HashTriejoin`],
//! [`binary_join_for_each`] for [`KeyedIterable`] structures under
//! [`BinaryHashJoin`] — and one collecting function ([`lftj_join`],
//! [`hash_join`], [`binary_join`]) whose sink is a flat [`Tuples`] batch. A
//! collected answer is a relation: the
```

Replace `identically for both` / `families` in the second module-doc paragraph: the sentence

```rust
//! Every entry point is total: a query that cannot run over the catalog
//! returns a [`JoinError`] rather than panicking, identically for both
//! families. [`validate_query`] runs the same checks without joining, for
```

becomes

```rust
//! Every entry point is total: a query that cannot run over the catalog
//! returns a [`JoinError`] rather than panicking, identically for every
//! family. [`validate_query`] runs the same checks without joining, for
```

Replace the import block

```rust
use {
    kermit_algos::{HashTriejoin, JoinAlgo, JoinQuery, Planner},
    kermit_ds::{Relation, RelationHeader, Tuples},
    kermit_iters::{HashTrieIterable, TrieIterable},
};
```

with

```rust
use {
    kermit_algos::{BinaryHashJoin, HashTriejoin, JoinAlgo, JoinQuery, Planner},
    kermit_ds::{Relation, RelationHeader, Tuples},
    kermit_iters::{HashTrieIterable, KeyedIterable, TrieIterable},
};
```

Add the two entry points directly after `hash_join` (before `#[cfg(test)]`):

```rust
/// Keyed-family join entry point: runs `query` over `catalog` with
/// [`BinaryHashJoin`], planned by `planner`, and passes each result tuple to
/// `emit` without materialising the result. `R` is the structure every index
/// is built as; the query is prepared under `R::TRIE_LEVELS`
/// (`TrieLevels::ProbeKeys` for `HashIndex`), which puts the executed body in
/// join order and keys each index on the variables the atoms before it bind.
/// Mirror of [`hash_join_for_each`].
///
/// # Errors
///
/// As [`lftj_join_for_each`].
pub fn binary_join_for_each<R>(
    catalog: &Catalog, query: JoinQuery, planner: &Planner, emit: impl FnMut(&[usize]),
) -> Result<(), JoinError>
where
    R: Relation + KeyedIterable,
{
    run_join::<R, BinaryHashJoin>(catalog, &query, planner, emit)
}

/// [`binary_join_for_each`], collected: returns the answer as a relation, as
/// [`lftj_join`] does.
///
/// # Errors
///
/// As [`binary_join_for_each`].
///
/// # Panics
///
/// If the answer has more than [`RowId::MAX`](kermit_ds::RowId) rows; stream
/// such an answer with [`binary_join_for_each`] instead.
pub fn binary_join<R>(
    catalog: &Catalog, query: JoinQuery, planner: &Planner,
) -> Result<(RelationHeader, Tuples), JoinError>
where
    R: Relation + KeyedIterable,
{
    collect_join::<R, BinaryHashJoin>(catalog, &query, planner)
}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt --all && nix develop --command cargo test -p kermit --lib db::`

Expected: all `db::tests::*` and `db::lifecycle::tests::*` pass, including `the_keyed_family_answers_like_the_hash_family`, `the_keyed_streaming_entry_point_visits_every_row`, `the_keyed_family_rejects_malformed_queries_like_the_hash_family`, `checks_multiply_on_a_hash_index` and `a_sorted_trie_refuses_a_query_prepared_for_probe_key_levels` (`ok`, the last as `should panic ... ok`).

Then `nix develop --command cargo clippy -p kermit --all-targets -- -D warnings` and `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit'` pass.

- [ ] **Step 5: Commit**

```
git commit -m "feat(kermit): binary_join_for_each and binary_join over the keyed family" -- kermit/src/db.rs kermit/src/db/lifecycle.rs
```

### Task 3.3: The `JoinEntry` impl and the join suite

**Files:**
- Modify: `kermit/tests/common/utils.rs` (module docs `:1-11`, imports `:12-21`, a new impl after the `HashTriejoin` one)
- Modify: `kermit/tests/join_tests.rs` (imports `:3-15`, a new section at the end)
- Test: `kermit/tests/join_tests.rs`

- [ ] **Step 1: Write the failing test**

In `kermit/tests/join_tests.rs`, replace the import block

```rust
use {
    kermit_algos::{
        CardinalityOptimiser, CostBasedOptimiser, HashTriejoin, LeapfrogTriejoin,
        LexicographicOptimiser,
    },
    kermit_ds::{
        define_build_mode_provider, define_config_provider, BinarySeek, ChildCapacity, ColumnTrie,
        ColumnTrieBuildMode, Configured, EagerExpansion, GallopingSeek, HashTrie,
        HashTrieBuildMode, HashTrieConfig, KeyLevels, LazyExpansion, LinearSeek, LoadFactor,
        NoPruning, RadixBits, RootCapacity, SingletonPruning, Threads, TreeTrie, TreeTrieBuildMode,
    },
    kermit_iters::{FxHashStrategy, SipHashStrategy},
};
```

with

```rust
use {
    kermit_algos::{
        BinaryHashJoin, CardinalityOptimiser, CostBasedOptimiser, HashTriejoin, LeapfrogTriejoin,
        LexicographicOptimiser,
    },
    kermit_ds::{
        define_build_mode_provider, define_config_provider, BinarySeek, ChildCapacity, ColumnTrie,
        ColumnTrieBuildMode, Configured, EagerExpansion, GallopingSeek, HashIndex, HashTrie,
        HashTrieBuildMode, HashTrieConfig, KeyLevels, LazyExpansion, LinearSeek, LoadFactor,
        NoPruning, RadixBits, RootCapacity, SingletonPruning, Threads, TreeTrie, TreeTrieBuildMode,
    },
    kermit_iters::{FxHashStrategy, SipHashStrategy},
};
```

and append at the end of the file:

```rust

// ── The keyed family: HashIndex under BinaryHashJoin ────────────────────
// The third iterator family, planned under `ProbeKeys`: the executed body is
// in join order and each index is keyed on the variables the atoms before it
// bind. It has no Layout beyond the hasher, no Config and no BuildMode, so
// the cells are the two hashers under every optimiser. Every variable order
// is covered separately by `all_orders.rs`.
type HashIndexSip = HashIndex<SipHashStrategy>;
type HashIndexFx = HashIndex<FxHashStrategy>;
define_multiway_join_test_suite!(HashIndexSip, BinaryHashJoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashIndexSip, BinaryHashJoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashIndexSip, BinaryHashJoin, CostBasedOptimiser);
define_multiway_join_test_suite!(HashIndexFx, BinaryHashJoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashIndexFx, BinaryHashJoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashIndexFx, BinaryHashJoin, CostBasedOptimiser);
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --test join_tests hashindex`

Expected: `join_tests` does not compile: `error[E0277]: the trait bound \`BinaryHashJoin: JoinEntry<HashIndex>\` is not satisfied`, once per generated test.

- [ ] **Step 3: Implement**

In `kermit/tests/common/utils.rs`, replace the module-doc passage

```rust
//! it, mirroring how the CLI's `execution` module pairs them: `join`
//! collects through `kermit::db::lftj_join` / `hash_join`, and `count`
//! streams through `execute` into a counter.
```

with

```rust
//! it, mirroring how the CLI's `execution` module pairs them: `join`
//! collects through `kermit::db::lftj_join` / `hash_join` / `binary_join`,
//! and `count` streams through `execute` into a counter.
```

Replace the import block

```rust
use {
    kermit::db::{build, execute, hash_join, lftj_join, prepare, Catalog, JoinError},
    kermit_algos::{
        HashTriejoin, IndexSpec, JoinAlgo, JoinQuery, LeapfrogTriejoin, Planner, QueryOptimiser,
        StatisticsLevel,
    },
    kermit_ds::{Relation, RelationHeader, Tuples},
    kermit_iters::{HashTrieIterable, TrieIterable},
    std::{collections::BTreeMap, path::Path},
};
```

with

```rust
use {
    kermit::db::{binary_join, build, execute, hash_join, lftj_join, prepare, Catalog, JoinError},
    kermit_algos::{
        BinaryHashJoin, HashTriejoin, IndexSpec, JoinAlgo, JoinQuery, LeapfrogTriejoin, Planner,
        QueryOptimiser, StatisticsLevel,
    },
    kermit_ds::{Relation, RelationHeader, Tuples},
    kermit_iters::{HashTrieIterable, KeyedIterable, TrieIterable},
    std::{collections::BTreeMap, path::Path},
};
```

Add this impl directly after the `impl<R: Relation + HashTrieIterable> JoinEntry<R> for HashTriejoin { … }` block (before `/// A fixture's relations by name`):

```rust
/// One impl for every keyed relation (`HashIndex`): the keyed family's entry
/// point. `R::TRIE_LEVELS` is `ProbeKeys`, so `prepare` puts the executed
/// body in join order and keys each index for it.
impl<R: Relation + KeyedIterable> JoinEntry<R> for BinaryHashJoin {
    fn join(
        catalog: &Catalog, query: JoinQuery, planner: &Planner,
    ) -> Result<(RelationHeader, Tuples), JoinError> {
        binary_join::<R>(catalog, query, planner)
    }

    fn count(catalog: &Catalog, query: JoinQuery, planner: &Planner) -> Result<u64, JoinError> {
        count_rows::<R, BinaryHashJoin>(catalog, &query, planner)
    }
}
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt --all && nix develop --command cargo test -p kermit --test join_tests hashindex`

Expected: 96 tests pass (16 patterns × 6 invocations), e.g. `triangle_hashindexsip_binaryhashjoin_costbasedoptimiser ... ok`. Then `nix develop --command cargo clippy -p kermit --all-targets -- -D warnings` passes.

- [ ] **Step 5: Commit**

```
git commit -m "test(kermit): HashIndex under BinaryHashJoin in the join suite" -- kermit/tests/common/utils.rs kermit/tests/join_tests.rs
```

### Task 3.4: `HashIndex` under every variable order

**Files:**
- Modify: `kermit/tests/all_orders.rs` (module docs, imports `:17-34`, `rows_in_order` `:171-205`, new cells after the `keys_hash_trie_*` tests)
- Test: `kermit/tests/all_orders.rs`

`prepare` under `ProbeKeys` rewrites the plan into V′, the binary join's variable order (an atom's key variables before its new ones), so `prepared.plan()` need not begin with the whole forced order: for `Q(V0, V1, V2) :- r0(V0, V1), r1(V0, V2)` forced to `[V1, V2, V0]` it is `[V1, V0, V2]`. The first forced variable always leads: the first atom joined contains it, and V′ lists that atom's variables in V's order. `rows_in_order` is the only place that assumes more.

- [ ] **Step 1: Write the failing test**

In `kermit/tests/all_orders.rs`, replace the import block

```rust
    kermit_algos::{
        analyse, canonical_names, rewrite_placeholders, CatalogStats, CostBasedOptimiser,
        HashTriejoin, JoinAlgo, JoinQuery, LeapfrogTriejoin, Optimiser, Planner, QueryOptimiser,
        StatisticsLevel,
    },
    kermit_ds::{
        Cardinality, ColumnTrie, EagerExpansion, HashTrie, KeyLevels, LazyExpansion, NoPruning,
        Relation, RelationHeader, SingletonPruning, TreeTrie, Tuples,
    },
```

with

```rust
    kermit_algos::{
        analyse, canonical_names, rewrite_placeholders, BinaryHashJoin, CatalogStats,
        CostBasedOptimiser, HashTriejoin, JoinAlgo, JoinQuery, LeapfrogTriejoin, Optimiser,
        Planner, QueryOptimiser, StatisticsLevel,
    },
    kermit_ds::{
        Cardinality, ColumnTrie, EagerExpansion, HashIndex, HashTrie, KeyLevels, LazyExpansion,
        NoPruning, Relation, RelationHeader, SingletonPruning, TreeTrie, Tuples,
    },
```

Add the alias directly after the `HashTrieSipKeys` alias (`type HashTrieSipKeys = HashTrie<SipHashStrategy, NoPruning, EagerExpansion, KeyLevels>;`):

```rust
/// The keyed family (`ProbeKeys`): every order is a variable order for the
/// binary join, whose rule turns it into a join order and keys, including
/// the orders that force a cross product or an atom's probe on every column.
type HashIndexSip = HashIndex<SipHashStrategy>;
```

Replace the module-doc lines

```rust
//! structure: `TreeTrie` and `ColumnTrie` under `LeapfrogTriejoin`,
//! `HashTrie` under `HashTriejoin`. Rows are compared with the
//! nested-loop oracle, which shares nothing with the executors.
```

with

```rust
//! structure: `TreeTrie` and `ColumnTrie` under `LeapfrogTriejoin`,
//! `HashTrie` under `HashTriejoin`, and `HashIndex` under
//! `BinaryHashJoin`. Rows are compared with the nested-loop oracle, which
//! shares nothing with the executors.
```

Append these tests directly after `keys_hash_trie_pinned_queries_under_every_order` (before the doc comment of `a_forced_order_reverses_the_index_columns`):

```rust
/// The duplicated constant atom multiplies on the keyed family, which keeps
/// duplicates like `HashTrie`.
#[test]
fn hash_index_duplicated_constant_atom_under_every_order() {
    run_duplicated_constant_atom::<HashIndexSip, BinaryHashJoin>(3);
}

#[test]
fn hash_index_shapes_under_every_order() { run_shapes::<HashIndexSip, BinaryHashJoin>(); }

#[test]
fn hash_index_pinned_queries_under_every_order() { run_pinned::<HashIndexSip, BinaryHashJoin>(); }
```

- [ ] **Step 2: Run it to see it fail**

Run: `nix develop --command cargo test -p kermit --test all_orders hash_index`

Expected: `hash_index_duplicated_constant_atom_under_every_order` passes (one variable, one order), and `hash_index_shapes_under_every_order` and `hash_index_pinned_queries_under_every_order` FAIL (`test result: FAILED. 1 passed; 2 failed`), each with a panic ending `the plan does not bind the forced variables first`: `rows_in_order` compares the whole forced order with the plan's prefix, and the plan is V′, not the forced order.

- [ ] **Step 3: Implement**

In `rows_in_order`, replace the doc passage

```rust
/// in `order`: `prepare`, `build`, `execute`. Asserts that the prepared
/// plan binds the forced variables first, so a planner that stopped
/// honouring `order` fails here instead of running one order many times.
```

with

```rust
/// in `order`: `prepare`, `build`, `execute`. Asserts that the prepared
/// plan binds the forced variables first, so a planner that stopped
/// honouring `order` fails here instead of running one order many times.
/// Under `ProbeKeys` the prepared plan is V′, the binary join's variable
/// order, which lists each joined atom's key variables before its new ones,
/// so only the first forced variable is certain to lead.
```

and replace the assertion

```rust
    assert_eq!(
        &bound[..order.len()],
        order,
        "{label}: the plan does not bind the forced variables first"
    );
```

with

```rust
    let forced = if R::TRIE_LEVELS == kermit_ds::TrieLevels::ProbeKeys {
        &order[..order.len().min(1)]
    } else {
        order
    };
    assert_eq!(
        &bound[..forced.len()],
        forced,
        "{label}: the plan does not bind the forced variables first"
    );
```

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt --all && nix develop --command cargo test -p kermit --test all_orders`

Expected: all tests pass, including the three `hash_index_*` ones and every earlier cell (their assertion is unchanged for `TrieLevels::All` and `JoinKeys`). Then `nix develop --command cargo clippy -p kermit --all-targets -- -D warnings` passes.

- [ ] **Step 5: Commit**

```
git commit -m "test(kermit): HashIndex under every variable order" -- kermit/tests/all_orders.rs
```

### Task 3.5: Allocation cells for `HashIndex`

**Files:**
- Modify: `kermit/tests/result_allocation.rs` (module docs, imports `:49-70`, new cells at the end)
- Modify: `kermit/tests/tuple_allocation.rs` (imports `:56-72`, new cells at the end)
- Test: `kermit/tests/result_allocation.rs`, `kermit/tests/tuple_allocation.rs`

- [ ] **Step 1: Write the failing test**

In `kermit/tests/result_allocation.rs`, replace the import block

```rust
use {
    kermit::db::{
        build, execute, hash_join, hash_join_for_each, lftj_join, lftj_join_for_each, prepare,
        Catalog, Indexes, Prepared,
    },
    kermit_algos::{
        HashTriejoin, JoinAlgo, JoinQuery, LeapfrogTriejoin, LexicographicOptimiser, Planner,
        StatisticsLevel,
    },
    kermit_ds::{
        AllLevels, BinarySeek, ColumnTrie, EagerExpansion, ExpansionPolicy, HashTrie, KeyLevels,
        LazyExpansion, LevelPolicy, LinearSeek, NoPruning, PruningPolicy, Relation, RelationHeader,
        SingletonPruning, TreeTrie, Tuples,
    },
    kermit_iters::{
        FxHashStrategy, HashStrategy, SipHashStrategy, TrieIterable, TrieIteratorWrapper,
    },
};
```

with

```rust
use {
    kermit::db::{
        binary_join, binary_join_for_each, build, execute, hash_join, hash_join_for_each,
        lftj_join, lftj_join_for_each, prepare, Catalog, Indexes, Prepared,
    },
    kermit_algos::{
        BinaryHashJoin, HashTriejoin, JoinAlgo, JoinQuery, LeapfrogTriejoin, LexicographicOptimiser,
        Planner, StatisticsLevel,
    },
    kermit_ds::{
        AllLevels, BinarySeek, ColumnTrie, EagerExpansion, ExpansionPolicy, HashIndex, HashTrie,
        KeyLevels, LazyExpansion, LevelPolicy, LinearSeek, NoPruning, PruningPolicy, Relation,
        RelationHeader, SingletonPruning, TreeTrie, Tuples,
    },
    kermit_iters::{
        FxHashStrategy, HashStrategy, KeyedIterable, SipHashStrategy, TrieIterable,
        TrieIteratorWrapper,
    },
};
```

Add a paragraph to the module docs, directly after

```rust
//! Since the per-query index build every cell prepares its query and builds
//! its indexes outside the measured region, and measures only the streamed
//! `kermit::db::execute` over them.
```

```rust
//!
//! The keyed family (`HashIndex` under `BinaryHashJoin`) is held to the same
//! standard: its join recurses once per atom and reuses one row and one key
//! scratch, and its probe and scan iterators borrow the index. The cells
//! below are the result-size, permuted-index, descent-count, collected and
//! scan checks, each on the same workloads as the other families'.
```

Append at the end of the file:

```rust

// ── The keyed family: HashIndex under BinaryHashJoin ─────────────────────

/// Allocations made by one streamed `BinaryHashJoin` join of `QUERY`, after
/// checking that it produced every row. The plan binds `X`, `Y`, `Z`; `R` is
/// the driver and `S` is probed on `X`.
fn bhj_allocations(fan_out: usize) -> u64 {
    join_allocations::<HashIndex, BinaryHashJoin>(QUERY, relations(fan_out), XS * fan_out)
}

/// The `BinaryHashJoin` twin of [`lftj_reversed_allocations`]: the reversed
/// `R` is read through an index whose columns are permuted into the join's
/// variable order (`reversed` asserts it).
fn bhj_reversed_allocations(fan_out: usize) -> u64 {
    streamed_allocations::<HashIndex, BinaryHashJoin>(
        &reversed::<HashIndex>(fan_out),
        XS * fan_out,
    )
}

/// Allocations made by one streamed `BinaryHashJoin` join of the descent
/// workload: `R` drives, `S` is probed on `Y` and `T` on `Z`, and each dead
/// end costs a hit on `S` and a miss on `T`.
fn bhj_descent_allocations(dead_ends: usize) -> u64 {
    join_allocations::<HashIndex, BinaryHashJoin>(
        DESCENT_QUERY,
        descent_relations(dead_ends),
        DESCENT_ROWS,
    )
}

/// The allocations `binary_join` makes beyond `binary_join_for_each` over the
/// same catalog, as [`lftj_collected_extra`] measures them.
fn bhj_collected_extra(fan_out: usize) -> u64 {
    let (catalog, query) = collected_workload(fan_out);
    let planner = planner();
    let (collected_query, streamed_query) = (query.clone(), query);
    let mut rows = 0;
    let collected = allocation_counter::measure(|| {
        let (_, answer) = binary_join::<HashIndex>(&catalog, collected_query, &planner).unwrap();
        rows = std::hint::black_box(answer).len();
    });
    assert_eq!(rows, XS * fan_out, "the answer must hold every row");
    let streamed = allocation_counter::measure(|| {
        binary_join_for_each::<HashIndex>(&catalog, streamed_query, &planner, |row| {
            std::hint::black_box(row);
        })
        .unwrap();
    });
    collected
        .count_total
        .checked_sub(streamed.count_total)
        .expect("collecting allocates at least what streaming does")
}

/// Allocations made by one scan of a `HashIndex` holding `S`, through
/// `KeyedIterable::scan`, after checking that it lent every tuple.
fn hash_index_scan_allocations(fan_out: usize) -> u64 {
    let rel: HashIndex = HashIndex::from_tuples(2.into(), s_tuples(fan_out));
    let mut tuples = 0usize;
    let info = allocation_counter::measure(|| {
        for tuple in rel.scan() {
            std::hint::black_box(tuple);
            tuples += 1;
        }
    });
    assert_eq!(tuples, XS * fan_out, "the scan must visit every tuple");
    info.count_total
}

#[test]
fn hash_index_bhj_allocates_independently_of_result_size() {
    bhj_allocations(SMALL);
    assert_flat(
        "HashIndex/BinaryHashJoin",
        bhj_allocations(SMALL),
        bhj_allocations(LARGE),
    );
}

#[test]
fn hash_index_bhj_over_a_permuted_index_allocates_independently_of_result_size() {
    bhj_reversed_allocations(SMALL);
    assert_flat(
        "HashIndex/BinaryHashJoin/permuted index",
        bhj_reversed_allocations(SMALL),
        bhj_reversed_allocations(LARGE),
    );
}

#[test]
fn hash_index_bhj_allocates_independently_of_descent_count() {
    bhj_descent_allocations(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "HashIndex/BinaryHashJoin",
        bhj_descent_allocations(FEW_DEAD_ENDS),
        bhj_descent_allocations(MANY_DEAD_ENDS),
    );
}

#[test]
fn hash_index_bhj_collects_without_allocating_per_row() {
    bhj_collected_extra(SMALL);
    assert_collected_growth(
        "HashIndex/BinaryHashJoin",
        bhj_collected_extra(SMALL),
        bhj_collected_extra(LARGE),
    );
}

#[test]
fn hash_index_scan_allocates_independently_of_relation_size() {
    hash_index_scan_allocations(SMALL);
    assert_flat(
        "HashIndex scan",
        hash_index_scan_allocations(SMALL),
        hash_index_scan_allocations(LARGE),
    );
}
```

In `kermit/tests/tuple_allocation.rs`, replace the `kermit_ds` import

```rust
    kermit_ds::{
        test_hooks::{
            take_hash_trie_parallel_builds, take_tree_trie_parallel_builds, HashTrieParallelBuild,
        },
        BuildModeRelation, Cardinality, ColumnTrie, ColumnTrieBuildMode,
        ConfiguredBuildModeRelation, EagerExpansion, ExpansionPolicy, HashTrie, HashTrieBuildMode,
        HashTrieConfig, HeapSize, KeyLevels, LazyExpansion, NoPruning, PruningPolicy, RadixBits,
        Relation, RelationHeader, RootCapacity, SingletonPruning, Threads, TreeTrie,
        TreeTrieBuildMode, Tuples,
    },
```

with

```rust
    kermit_ds::{
        test_hooks::{
            take_hash_trie_parallel_builds, take_tree_trie_parallel_builds, HashTrieParallelBuild,
        },
        BuildModeRelation, Cardinality, ColumnTrie, ColumnTrieBuildMode,
        ConfiguredBuildModeRelation, EagerExpansion, ExpansionPolicy, HashIndex, HashTrie,
        HashTrieBuildMode, HashTrieConfig, HeapSize, KeyLevels, LazyExpansion, NoPruning,
        PruningPolicy, RadixBits, Relation, RelationHeader, RootCapacity, SingletonPruning,
        Threads, TreeTrie, TreeTrieBuildMode, Tuples,
    },
```

and append at the end of the file:

```rust

// ── HashIndex builds ────────────────────────────────────────────────────

/// A `HashIndex` build allocates three arrays, whatever the row count: the one
/// copy of the rows, the bucket directory and the per-row links. A per-row
/// allocation adds 9 900 over the larger fixture.
const HASH_INDEX_ARRAYS: u64 = 3;

/// Allocations of one `HashIndex` build of [`repeated`]`(repeats)`, keyed on
/// its first `levels` columns (`0` is a driver's empty key, `2` the
/// `bench ds` build keyed on every column), after checking it stored every
/// row.
fn hash_index_build_allocations(levels: usize, repeats: usize) -> u64 {
    let tuples = repeated(repeats);
    let rows = tuples.len();
    let header = RelationHeader::new_positional("r", 2).with_levels(levels);
    let mut built: Option<HashIndex> = None;
    let info = allocation_counter::measure(|| {
        built = Some(HashIndex::from_tuples(header, tuples));
    });
    assert_eq!(
        built.expect("measured").tuple_count(),
        rows,
        "levels {levels}"
    );
    info.count_total
}

/// The build allocates its arrays once, at either repetition and at every key
/// length: the larger build may allocate no more than the smaller, and
/// neither more than the arrays plus [`SCRATCH`].
#[test]
fn hash_index_builds_allocate_independently_of_their_row_count() {
    for levels in 0..=2 {
        let (once, many) = (
            hash_index_build_allocations(levels, 1),
            hash_index_build_allocations(levels, REPEATS),
        );
        assert!(
            once <= HASH_INDEX_ARRAYS + SCRATCH,
            "HashIndex keyed on {levels} columns: {once} allocations for {} rows; its arrays \
             account for {HASH_INDEX_ARRAYS}",
            KEYS * KEYS
        );
        assert!(
            many <= once,
            "HashIndex keyed on {levels} columns: {once} allocations for {} rows, {many} for {}: \
             the larger build allocates more, so some row allocates",
            KEYS * KEYS,
            REPEATS * KEYS * KEYS
        );
    }
}

/// What [`hash_index_view_build`] observed of one build.
struct HashIndexViewBuild {
    /// The counter's totals and peak for the build.
    info: allocation_counter::AllocationInfo,
    /// The built index's `heap_size_bytes`.
    heap: usize,
    /// Rows the view keeps.
    kept_rows: usize,
}

/// [`hash_index_build_allocations`] for a build from [`kept_rows`] over
/// [`repeated_in_decoys`]: the rows the view keeps are `repeated(repeats)`'s,
/// and the decoys it rejects must cost nothing.
fn hash_index_view_build(levels: usize, repeats: usize) -> HashIndexViewBuild {
    let source = repeated_in_decoys(repeats);
    let view = kept_rows(&source);
    let rows = repeats * KEYS * KEYS;
    assert_eq!(
        view.count(),
        rows,
        "the view keeps the repeated rows and no decoy"
    );
    assert!(
        source.len() > rows,
        "the source holds decoys the build must skip"
    );
    let header = RelationHeader::new_positional("r", 2).with_levels(levels);
    let mut built: Option<HashIndex> = None;
    let info = allocation_counter::measure(|| {
        built = Some(HashIndex::from_view(header, view));
    });
    let built = built.expect("measured");
    assert_eq!(built.tuple_count(), rows, "levels {levels}");
    HashIndexViewBuild {
        info,
        heap: built.heap_size_bytes(),
        kept_rows: rows,
    }
}

/// A build from a filtered, projected and permuted view allocates the same
/// three arrays, and holds the kept rows once: its peak is the finished
/// index's `heap_size_bytes` plus a margin under one copy (see
/// [`assert_peak`]; a `HashIndex` keeps its one copy of the rows as its own
/// array and builds the directory and links in place, so it holds no scratch
/// beside the finished index). The kept rows are 10 000 of 11 000 in the
/// larger source, a copy of 160 000 bytes.
#[test]
fn hash_index_view_builds_allocate_once_per_array_and_hold_the_kept_rows_once() {
    for levels in 0..=2 {
        let small = hash_index_view_build(levels, 1);
        let large = hash_index_view_build(levels, REPEATS);
        assert!(
            small.info.count_total <= HASH_INDEX_ARRAYS + SCRATCH,
            "HashIndex keyed on {levels} columns from a view: {} allocations for {} kept rows; \
             its arrays account for {HASH_INDEX_ARRAYS}",
            small.info.count_total,
            small.kept_rows
        );
        assert!(
            large.info.count_total <= small.info.count_total,
            "HashIndex keyed on {levels} columns from a view: {} allocations for {} kept rows, \
             {} for {}: the larger build allocates more, so some row allocates",
            small.info.count_total,
            small.kept_rows,
            large.info.count_total,
            large.kept_rows
        );
        let copy = (large.kept_rows * 2 * WORD) as u64;
        assert_peak(
            &format!("HashIndex keyed on {levels} columns from a view"),
            &large.info,
            large.heap,
            0,
            copy / 8,
            copy,
        );
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run, with the mutant applied. These cells guard against exactly one regression, a join that allocates per step or per row, so first confirm the result-size, permuted-index and descent cells can fail. Temporarily add this line as the first statement of `descend` in `kermit-algos/src/keyed/binary_hash_join.rs` (Task 3.1 is committed, so `git checkout` restores it exactly):

```rust
    std::hint::black_box(vec![0usize; 1]);
```

Run: `nix develop --command cargo test -p kermit --test result_allocation hash_index`

Expected: `hash_index_bhj_allocates_independently_of_result_size`, `hash_index_bhj_over_a_permuted_index_allocates_independently_of_result_size` and `hash_index_bhj_allocates_independently_of_descent_count` FAIL, each with `... changed the allocation count from N to M` (`the streamed path is allocating per row` / `the join is allocating per descent`). The scan and collected cells pass: the mutant does not touch them, and the collected one measures only the difference between two joins that both carry it.

`tuple_allocation.rs` has no code step to mutate: its cells pin Story 2's build, which exists. They pass on the first run (Step 4).

- [ ] **Step 3: Implement**

Restore the algorithm: `git checkout -- kermit-algos/src/keyed/binary_hash_join.rs`, then confirm `git diff --stat -- kermit-algos` prints nothing. There is no other code step: the cells are the deliverable.

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt --all && nix develop --command cargo test -p kermit --test result_allocation --test tuple_allocation hash_index`

Expected: the five `result_allocation` cells (`hash_index_bhj_allocates_independently_of_result_size`, `hash_index_bhj_over_a_permuted_index_allocates_independently_of_result_size`, `hash_index_bhj_allocates_independently_of_descent_count`, `hash_index_bhj_collects_without_allocating_per_row`, `hash_index_scan_allocates_independently_of_relation_size`) and the two `tuple_allocation` cells (`hash_index_builds_allocate_independently_of_their_row_count`, `hash_index_view_builds_allocate_once_per_array_and_hold_the_kept_rows_once`) pass. Then run both files whole: `nix develop --command cargo test -p kermit --test result_allocation --test tuple_allocation` passes, and `nix develop --command cargo clippy -p kermit --all-targets -- -D warnings` passes.

- [ ] **Step 5: Commit**

```
git commit -m "test(kermit): HashIndex allocation cells for the join and the build" -- kermit/tests/result_allocation.rs kermit/tests/tuple_allocation.rs
```

### Task 3.6: `HashIndex` in the LUBM oracles

**Files:**
- Modify: `kermit/tests/lubm_mini_oracle.rs` (imports `:57-67`, the cell loop, the failure message)
- Modify: `kermit/tests/lubm_cardinalities.rs` (imports `:20-31`, the cell loop, the failure message)
- Test: `kermit/tests/lubm_mini_oracle.rs`, `kermit/tests/lubm_cardinalities.rs`

- [ ] **Step 1: Write the failing test**

In `kermit/tests/lubm_mini_oracle.rs`, replace

```rust
    kermit_algos::{HashTriejoin, JoinQuery, LeapfrogTriejoin, Optimiser, Planner},
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{
        BinarySeek, Cardinality, ColumnTrie, EagerExpansion, GallopingSeek, HashTrie, KeyLevels,
        LazyExpansion, LinearSeek, NoPruning, Relation, SingletonPruning, TreeTrie,
    },
```

with

```rust
    kermit_algos::{BinaryHashJoin, HashTriejoin, JoinQuery, LeapfrogTriejoin, Optimiser, Planner},
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{
        BinarySeek, Cardinality, ColumnTrie, EagerExpansion, GallopingSeek, HashIndex, HashTrie,
        KeyLevels, LazyExpansion, LinearSeek, NoPruning, Relation, SingletonPruning, TreeTrie,
    },
```

then, in `mini_lubm_abox_query_cardinalities_match_hand_derivation`, replace the last cell of the optimiser loop

```rust
        mismatches.extend(cardinality_mismatches::<
            HashTrieSipPrunedLazyKeys,
            HashTriejoin,
        >(&bench, out.path(), &planner, &label, &expected));
    }
```

with

```rust
        mismatches.extend(cardinality_mismatches::<
            HashTrieSipPrunedLazyKeys,
            HashTriejoin,
        >(&bench, out.path(), &planner, &label, &expected));
        // The keyed family: the binary join over `HashIndex`, which keeps
        // duplicates like `HashTrie`, so its counts are the same.
        mismatches.extend(cardinality_mismatches::<
            HashIndex<SipHashStrategy>,
            BinaryHashJoin,
        >(&bench, out.path(), &planner, &label, &expected));
    }
```

and replace the message passage

```rust
        "mini LUBM cardinality mismatches ({} across {} queries x {} optimisers x (2 sorted tries \
         x 3 seek strategies + 3 HashTrie cells)):\n{}",
```

with

```rust
        "mini LUBM cardinality mismatches ({} across {} queries x {} optimisers x (2 sorted tries \
         x 3 seek strategies + 3 HashTrie cells + 1 HashIndex cell)):\n{}",
```

In `kermit/tests/lubm_cardinalities.rs`, replace

```rust
    kermit_algos::{
        CardinalityOptimiser, CostBasedOptimiser, HashTriejoin, JoinQuery, LeapfrogTriejoin,
        LexicographicOptimiser, Planner, QueryOptimiser,
    },
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{
        BinarySeek, Cardinality, ColumnTrie, EagerExpansion, GallopingSeek, HashTrie, KeyLevels,
        LazyExpansion, LinearSeek, NoPruning, Relation, SingletonPruning, TreeTrie,
    },
```

with

```rust
    kermit_algos::{
        BinaryHashJoin, CardinalityOptimiser, CostBasedOptimiser, HashTriejoin, JoinQuery,
        LeapfrogTriejoin, LexicographicOptimiser, Planner, QueryOptimiser,
    },
    kermit_bench::BenchmarkDefinition,
    kermit_ds::{
        BinarySeek, Cardinality, ColumnTrie, EagerExpansion, GallopingSeek, HashIndex, HashTrie,
        KeyLevels, LazyExpansion, LinearSeek, NoPruning, Relation, SingletonPruning, TreeTrie,
    },
```

and in `lubm_one_university_query_cardinalities_match_paper` replace the last cell of the optimiser loop

```rust
        mismatches.extend(cardinality_mismatches::<
            HashTrieSipPrunedLazyKeys,
            HashTriejoin,
        >(&bench, out.path(), &planner, &label, &expected));
    }
```

with

```rust
        mismatches.extend(cardinality_mismatches::<
            HashTrieSipPrunedLazyKeys,
            HashTriejoin,
        >(&bench, out.path(), &planner, &label, &expected));
        // The keyed family (a new structure and algorithm; the row this
        // file's comment above asks for): real data, with its long chains.
        mismatches.extend(cardinality_mismatches::<
            HashIndex<SipHashStrategy>,
            BinaryHashJoin,
        >(&bench, out.path(), &planner, &label, &expected));
    }
```

and the message passage

```rust
        "LUBM(1, 0) cardinality mismatches ({} across {} queries x {} optimisers x (2 sorted \
         tries x 3 seek strategies + 3 HashTrie cells)):\n{}",
```

with

```rust
        "LUBM(1, 0) cardinality mismatches ({} across {} queries x {} optimisers x (2 sorted \
         tries x 3 seek strategies + 3 HashTrie cells + 1 HashIndex cell)):\n{}",
```

- [ ] **Step 2: Run it to see it fail**

These cells add a row over an implementation that exists (Tasks 3.1 and 3.2 and Stories 1 and 2), so they pass on the first run; there is no failing state to show. A mismatch would print as `[<optimiser> / kermit_ds::...::HashIndex<...>] qN: got A, expected B`.

Run: `nix develop --command cargo test -p kermit --test lubm_mini_oracle --test lubm_cardinalities`

Expected: `mini_lubm_abox_query_cardinalities_match_hand_derivation` passes (14 queries × 3 optimisers on the new cell). `lubm_one_university_query_cardinalities_match_paper` passes, or prints `skipping: \`java\` not on PATH` and passes vacuously when run outside `nix develop`; inside it Java is present, so it generates LUBM(1, 0) and runs the new cell on the real data.

- [ ] **Step 3: Implement**

Nothing beyond Step 1: the cells are the deliverable.

- [ ] **Step 4: Run it to see it pass**

Run: `nix develop --command cargo fmt --all && nix develop --command cargo test -p kermit --test lubm_mini_oracle --test lubm_cardinalities && nix develop --command cargo clippy -p kermit --all-targets -- -D warnings`

Expected: `test result: ok` for both targets and a clean clippy.

- [ ] **Step 5: Commit**

```
git commit -m "test(kermit): HashIndex cell in the LUBM cardinality oracles" -- kermit/tests/lubm_mini_oracle.rs kermit/tests/lubm_cardinalities.rs
```

### Task 3.7: The `BinaryHashJoin` page

**Files:**
- Create: `docs/algorithms/binary-hash-join.md`

- [ ] **Step 1: Write the failing test**

The page is documentation; the checks are that every file it links exists and that the numbers in its micro-example match the unit test. Write the link check first:

```
for f in kermit-algos/src/keyed/binary_hash_join.rs kermit-algos/src/index_plan.rs kermit-algos/src/join_order.rs docs/data-structures/hash-index.md docs/algorithms/hash-triejoin.md docs/algorithms/leapfrog-triejoin.md kermit/tests/common/macros.rs; do test -f "$f" || echo "missing: $f"; done
```

- [ ] **Step 2: Run it to see it fail**

Run the loop above. Expected before the page is linked from anywhere: it prints nothing (every link target exists after Stories 1 and 2 and Task 3.1). The check that can fail is the next one: `test -f docs/algorithms/binary-hash-join.md || echo "missing: docs/algorithms/binary-hash-join.md"` prints `missing: docs/algorithms/binary-hash-join.md`.

- [ ] **Step 3: Implement**

Create `docs/algorithms/binary-hash-join.md` (complete):

````markdown
# `BinaryHashJoin`

> **Status:** experimental · **CLI:** `-a binary-hash-join` (pairs only with `-i hash-index`) · **Implementation:** [`kermit_algos::BinaryHashJoin`](../../kermit-algos/src/keyed/binary_hash_join.rs)

## What it is

A left-deep, pipelined binary hash join: the conventional join plan, and the baseline for what worst-case optimal joins buy. It is the plan of Wang, Willsey and Suciu, "Free Join: Unifying Worst-Case Optimal and Traditional Joins" (PACMMOD 2023), §2.2: the engine iterates over each tuple of the first atom, probes the hash table of the second on the variables the first binds, and for each match probes the third, and so on. Nothing intermediate is stored, so it fits `join_for_each`'s streaming contract unchanged.

It is not worst-case optimal. Its work is proportional to the sum of the sizes of its intermediate results, and on a cyclic query an intermediate result can exceed the AGM bound that [`HashTriejoin`](hash-triejoin.md) and [`LeapfrogTriejoin`](leapfrog-triejoin.md) meet. That gap is what this cell measures.

It runs over the third iterator family, `KeyedIterable` ([`HashIndex`](../data-structures/hash-index.md)): an index keyed on its leading columns, probed by exact match. Its join order comes from the optimiser's variable order, not from a binary-join optimiser (see "Where the join order comes from").

## Pseudocode

Free Join §2.2, Figure 2a, for the triangle `Q(x, y, z) :- R(x, y), S(y, z), T(x, z)` with the plan `[R, S, T]`:

```
for (x, y) in R:
    for (y, z) in S[y]:          # a failed lookup continues the loop above
        for (x, z) in T[x, z]:
            output(x, y, z)
```

The general form this crate runs. Step `j` is the `j`-th body atom of the executed query, with its index `I_j`, its variables `x_j` in column order, and `k_j = key_len(I_j)`:

```
join_for_each(plan, query, indexes, emit):
    validate plan against analyse(query)
    steps ← [(I_j, x_j, k_j) for each body atom j, in body order]
    assert for each step j:  x_j[..k_j] are bound by steps before j
                             x_j[k_j..] are not
    row ← scratch[num_vars];  keys ← scratch[Σ k_j]      # allocated once per join
    descend(0)

descend(j):
    if j = |steps|: emit(row); return
    key ← row[x_j[..k_j]]
    for t in I_j.probe(key):             # k_j = 0: every row, in source order
        row[x_j[i]] ← t[i]   for i in k_j..arity_j
        descend(j + 1)
```

Rust mapping, in [`kermit-algos/src/keyed/binary_hash_join.rs`](../../kermit-algos/src/keyed/binary_hash_join.rs):

- `join_for_each` → `<BinaryHashJoin as JoinAlgo<DS>>::join_for_each`, which validates the plan, builds the steps and calls `descend`.
- One loop level → `descend(steps, row, keys, emit)`. It splits `keys` into this level's key and the deeper levels' slots, so a level's key stays intact while the levels below it run.
- A step and the invariant check → `Step::new`.

**Where the join order comes from.** `join_for_each` reads the executed body left to right and derives nothing. [`plan_indexes`](../../kermit-algos/src/index_plan.rs) under `TrieLevels::ProbeKeys` does the deriving, by the rule of [`join_order.rs`](../../kermit-algos/src/join_order.rs), which reads a variable order as a sequence of binding events: each variable of the order that is not yet bound joins the atom that binds it (the unjoined atom containing it with the most variables already bound, ties to the earliest in the body), and an atom whose variables are then all bound is probed at once. The order `V′` it returns lists each joined atom's new variables after its key variables, so an index whose columns follow `V′` has its key first, and `IndexSpec::levels` is the key length: `0` for the driver (the first atom) and for an atom joined across a cross product.

## Invariants

- **Probe keys lead the columns.** For every step `j`, the first `key_len` variables of its atom are bound by the steps before `j`, and none of the others is bound yet. `Step::new` asserts it (the panic message starts with `probe keys lead the columns`), in the role LFTJ's ascent assertion plays. If it broke, a key column no earlier atom binds would probe with a stale value from the scratch row, and a column past the key that was already bound would be overwritten instead of joined on, so the join would return wrong rows silently. It catches a query not planned under `ProbeKeys` (for example, indexes keyed on every column, as `bench ds` builds them); `kermit::db::build` also refuses an index set planned under another trie-level policy, one layer up. Pinned by `an_index_keyed_on_every_column_is_refused` and `a_key_no_earlier_atom_binds_is_refused`.
- **The body order is the join order.** Step `j` is body atom `j`. `plan_indexes` lists the executed body in join order, with `#0` the driver. Reordering the body without re-planning the keys breaks the invariant above.
- **Constants and repeated variables never reach the join.** `kermit::db::prepare` turns each body atom into an access, and the build applies its filter while it copies the rows into the index, so every atom here has distinct variables, one per column. An atom of constants only builds no index: its row count multiplies the results on a structure that keeps duplicates (`Relation::KEEPS_DUPLICATES`, true for `HashIndex`), and a zero count ends the query. A placeholder arrives as its own fresh variable (`rewrite_placeholders`).
- **Every variable of the query is emitted, in canonical order.** `row[v]` is canonical variable `v` (head variables first). `kermit::db::execute` lends the head prefix `&row[..head_len]`, so body-only variables never reach the caller. A variable no atom carries has no value, so `join_for_each` returns before descending (validation rules it out for every query from `kermit::db`).
- **The result is streamed and the join allocates once.** `emit(row)` lends the one scratch row. The row, the key scratch and the steps are allocated once per join, and `probe` allocates nothing, so emitting a tuple, or descending, allocates nothing (`kermit/tests/result_allocation.rs`: result size, permuted index, descent count and collected cells).
- **Bag semantics.** `HashIndex` lends each duplicate row separately, so a duplicated row on either side of a join multiplies its matches, as on `HashTrie`. The cell's counts equal `HashTriejoin`'s on every query.
- **The plan is validated.** `join_for_each` calls `QueryPlan::validate` and panics on an invalid plan (a permutation of the query's variables whose order respects every atom's column order). Every plan `plan_indexes` returns under `ProbeKeys` validates, since each atom lists its variables in `V′` order.

## Complexity

Let `a` be the number of body atoms, `v` the number of variables, and `P_j` the number of partial results that reach step `j` (`P_0 = 1`, and `P_a` is the output size before head projection).

| Operation | Time | Notes |
|---|---|---|
| `join_for_each` setup | O(a + v) | One `analyse`, the steps, the row and the key scratch; none per result row. |
| One probe (`probe`) | O(1) expected, O(rows) worst | Directory load factor ≤ 1 and Fibonacci bucket selection; the key comparison filters other keys' rows from a chain, so a degenerate hash costs chain length, never correctness. |
| `join_for_each`, total | O(Σ_{j≥1} P_{j−1} · probe + Σ_j P_j) | The sum of the intermediate results: each partial result probes the next atom once, and each match is written once. |

There is no worst-case bound in the input size alone. On the triangle with `|R| = |S| = |T| = N` the AGM bound is `N^{3/2}`, which worst-case optimal joins meet, but the first intermediate result `|R ⋈ S|` can reach Θ(N²). The standard instance: `R = S = T = {(0, j), (j, 0) : 1 ≤ j ≤ m}`, so `N = 2m`. Then `R ⋈ S` on the middle variable holds `m² + m` tuples (every `(i, 0)` of `R` with every `(0, k)` of `S`, plus the `m` tuples `(0, j, 0)`) and the output is empty, since `T` has no tuple `(i, k)` with both values non-zero. At `m = 100` that is 10 100 probes into `T` against `N^{3/2} ≈ 2 828`.

## Worked micro-example

The triangle with the variable order `[X, Y, Z]` and

```
R(X, Y) = {(1, 2), (1, 3), (2, 3)}
S(Y, Z) = {(2, 3), (3, 4), (3, 5)}
T(X, Z) = {(1, 3), (1, 4), (2, 5)}
```

`prepare` under `ProbeKeys` applies the rule to `V = [X, Y, Z]`. The first unbound variable is `X`; `R` and `T` both contain it with nothing bound, so the earliest in the body, `R`, joins first and binds `X` and `Y`. No other atom is fully bound, so the next unbound variable is `Z`; `S` and `T` both contain it with one bound variable each, so `S` joins second, keyed on `Y`. `T` is now fully bound, so the filter step joins it third, keyed on `(X, Z)`. The executed query is `Q(X, Y, Z) :- #0(X, Y), #1(Y, Z), #2(X, Z)` with `V′ = [X, Y, Z]`, and the indexes are keyed as `#0`: no key (`levels` 0), `#1`: `Y` (1), `#2`: `(X, Z)` (2).

| Step | Loop | Probe | Matches |
|---|---|---|---|
| `#0` | `(X, Y)` over every row of `R` | key `()` | `(1,2)`, `(1,3)`, `(2,3)` |
| `#1` | for `(1,2)`: `Z` from `S[Y=2]` | key `(2)` | `(2,3)`, so `Z = 3` |
| `#2` | for `(1,2,3)`: `T[X=1, Z=3]` | key `(1,3)` | hit: emit `(1,2,3)` |
| `#1` | for `(1,3)`: `S[Y=3]` | key `(3)` | `(3,4)`, `(3,5)` |
| `#2` | for `(1,3,4)`: `T[1,4]` | key `(1,4)` | hit: emit `(1,3,4)` |
| `#2` | for `(1,3,5)`: `T[1,5]` | key `(1,5)` | miss |
| `#1` | for `(2,3)`: `S[Y=3]` | key `(3)` | `(3,4)`, `(3,5)` |
| `#2` | for `(2,3,4)`: `T[2,4]` | key `(2,4)` | miss |
| `#2` | for `(2,3,5)`: `T[2,5]` | key `(2,5)` | hit: emit `(2,3,5)` |

`|R ⋈ S|` is 5 (the five probes into `T`) and the output is `{(1,2,3), (1,3,4), (2,3,5)}`. This is the unit test `the_triangle_probes_each_atom_on_what_the_earlier_ones_bind` in `binary_hash_join.rs`; the same query runs end to end in `kermit/src/db.rs`'s `the_keyed_family_answers_like_the_hash_family`.

## See also

- [`HashIndex`](../data-structures/hash-index.md): the keyed index this algorithm probes.
- [`HashTriejoin`](hash-triejoin.md) and [`LeapfrogTriejoin`](leapfrog-triejoin.md): the worst-case optimal cells this one is the baseline for.
- `kermit_algos::plan_indexes` ([`index_plan.rs`](../../kermit-algos/src/index_plan.rs)) and `join_order.rs` ([`join_order.rs`](../../kermit-algos/src/join_order.rs)): the `ProbeKeys` planning that produces the executed query.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; this algorithm passes all 16 patterns over `HashIndex` under every optimiser (`kermit/tests/join_tests.rs`), and `kermit/tests/all_orders.rs` runs every shape under every variable order.
````

- [ ] **Step 4: Run it to see it pass**

Run the link loop of Step 1 and the existence check:

```
for f in docs/algorithms/binary-hash-join.md kermit-algos/src/keyed/binary_hash_join.rs kermit-algos/src/index_plan.rs kermit-algos/src/join_order.rs docs/data-structures/hash-index.md docs/algorithms/hash-triejoin.md docs/algorithms/leapfrog-triejoin.md kermit/tests/common/macros.rs; do test -f "$f" || echo "missing: $f"; done
```

Expected: no output. Then the story's gates once more over everything it changed: `nix develop --command cargo fmt --all -- --check`, `nix develop --command cargo clippy -p kermit-algos -p kermit --all-targets -- -D warnings`, `nix develop --command cargo test -p kermit-algos -p kermit`, `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit-algos -p kermit'`.

- [ ] **Step 5: Commit**

```
git add -N docs/algorithms/binary-hash-join.md
git commit -m "docs(algos): the BinaryHashJoin page" -- docs/algorithms/binary-hash-join.md
```

## Story 4: The binary: registry variants, the `IndexBhj` cell, flags, dispatch and CLI tests

- **Developer:** intermediate-dev
- **Files:** `kermit-ds/src/ds/mod.rs`, `kermit-algos/src/lib.rs`, `kermit/src/main.rs`, `kermit/src/execution.rs`, `kermit/src/options.rs`, `kermit/src/bench/run.rs`, `kermit/src/bench/ds.rs`, `kermit/tests/cli_query_errors.rs`, `kermit/tests/cli_bench_run_sweep.rs`, `kermit/tests/cli_hash_index_hasher_choice.rs`, `kermit/tests/cli_bench_run_ds_flag_reach.rs`, `kermit/tests/cli_bench_run_group_collision.rs`, `kermit/tests/cli_bench_probe.rs`, `kermit/tests/cli_column_trie_build_mode.rs`, `kermit/tests/cli_tree_trie_build_mode.rs`, `kermit/tests/cli_hash_trie_build_mode.rs`, `kermit/tests/cli_hash_trie_layout_expansion.rs`, `kermit/tests/cli_hash_trie_layout_pruning.rs`, `kermit/tests/cli_hash_trie_layout_levels.rs`, `kermit/tests/cli_sorted_trie_layout_seek.rs`, `kermit/tests/cli_snap_gen.rs`, `kermit/tests/cli_join_tests.rs`, `kermit/tests/cli_optimiser_choice.rs`, `python/kermit-lab/kermit_lab/analysis.py`, `python/kermit-lab/tests/test_analysis.py`, `python/kermit-lab/kermit_lab/axis_mapping.py`, `python/kermit-lab/tests/test_axis_mapping.py`
- **Interfaces:** exposes the registry variants, `Execution::IndexBhj`, `HashIndexFamily<H>`, `IndexBhj<H>` and `with_hash_index_layout!` (names contract, Story 4) to Story 5; uses `HashIndex<H>` (Story 2) and `BinaryHashJoin` (Story 3)
- **Verify:** `nix develop --command bash -c 'cargo test -p kermit && cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings && cd python/kermit-lab && uv run --locked --group test pytest -q'`
- **Review:** deep

This story makes the fourth `(structure, algorithm)` cell reachable from every command. Task 4.1 adds `IndexStructure::HashIndex` and `JoinAlgorithm::BinaryHashJoin` and everything that matches over them exhaustively, so it is one task: the two selectors, `Execution::IndexBhj` with `for_pair`, `for_structure`, `index_structure`, `algorithm` and `trie_levels`, the two families (`HashIndexFamily<H>` for `bench ds`, `IndexBhj<H>` for `bench run` / `bench join` / `kermit join`), `with_hash_index_layout!`, the `DsFlag::LayoutHasher` row, and the three dispatch arms. The registry variants cannot land earlier or later than the arms, or the binary does not compile. The same task updates every existing CLI test that counts the cells of a `-i all -a all` sweep: those tests would otherwise fail on the fourth cell. Eleven of those files are existing tests that only count cells (`cli_column_trie_build_mode.rs`, `cli_bench_run_ds_flag_reach.rs`, `cli_hash_trie_layout_expansion.rs`, `cli_hash_trie_layout_pruning.rs`, `cli_hash_trie_layout_levels.rs`, `cli_tree_trie_build_mode.rs`, `cli_hash_trie_build_mode.rs`, `cli_sorted_trie_layout_seek.rs`, `cli_snap_gen.rs`, `cli_bench_probe.rs`, `cli_bench_run_group_collision.rs`); the edit to each is one count and, where the test matches on the structure, one arm. Task 4.2 adds the new CLI tests, including `cli_hash_index_hasher_choice.rs`, and adds the new cell to the cell lists of `cli_join_tests.rs` and `cli_optimiser_choice.rs`. Task 4.3 makes kermit-lab's comment and test change and gives `HashIndex` its colour. Line numbers below are from the base revision; locate each edit by the quoted text. No code here holds a landmine marker.

### Task 4.1: The registry variants, the `IndexBhj` cell, and the dispatch

**Files:**
- Modify: `kermit-ds/src/ds/mod.rs:27-94` (the `IndexStructure` enum, `FromStr`, `axis_value` and their tests)
- Modify: `kermit-algos/src/lib.rs:61-113` (the `JoinAlgorithm` enum, `axis_value` and their tests)
- Modify: `kermit/src/main.rs:17-60`, `:127-184`, `:552-590`, `:866-872`, `:1346-1392`
- Modify: `kermit/src/execution.rs:1-45`, `:164-317`, `:634-742` and its tests from `:742`
- Modify: `kermit/src/options.rs:1-24`, `:81-93`, `:249-251`, `:507-510`, `:700`, `:912-932`, `:1885-1920` and its tests
- Modify: `kermit/src/bench/run.rs:12-20`, `:324-362`, `:428-476`, `:482-520`
- Modify: `kermit/src/bench/ds.rs:4-14`, `:130-185`
- Modify (existing cell-count tests): `kermit/tests/cli_bench_run_sweep.rs`, `kermit/tests/cli_column_trie_build_mode.rs`, `kermit/tests/cli_bench_run_ds_flag_reach.rs`, `kermit/tests/cli_hash_trie_layout_expansion.rs`, `kermit/tests/cli_hash_trie_layout_pruning.rs`, `kermit/tests/cli_hash_trie_layout_levels.rs`, `kermit/tests/cli_tree_trie_build_mode.rs`, `kermit/tests/cli_hash_trie_build_mode.rs`, `kermit/tests/cli_sorted_trie_layout_seek.rs`, `kermit/tests/cli_snap_gen.rs`, `kermit/tests/cli_bench_probe.rs`, `kermit/tests/cli_bench_run_group_collision.rs`

The enums declare their variants alphabetically (`ColumnTrie`, `HashIndex`, `HashTrie`, `TreeTrie`; `BinaryHashJoin`, `HashTriejoin`, `LeapfrogTriejoin`), so a sweep's cells and skipped pairs come out in that order; the tests below depend on it.

- [ ] **Step 1: Write the failing tests**

(a) `kermit-ds/src/ds/mod.rs`. In `index_structure_tests`, quote:

```rust
        assert_eq!(IndexStructure::ColumnTrie.axis_value(), "ColumnTrie");
        assert_eq!(IndexStructure::HashTrie.axis_value(), "HashTrie");
```

and make it:

```rust
        assert_eq!(IndexStructure::ColumnTrie.axis_value(), "ColumnTrie");
        assert_eq!(IndexStructure::HashIndex.axis_value(), "HashIndex");
        assert_eq!(IndexStructure::HashTrie.axis_value(), "HashTrie");
```

Then add this test at the end of `index_structure_tests`, after `axis_values_match_debug_repr`:

```rust
    /// The `-i` spelling and the `FromStr` spelling of the hash index.
    #[test]
    fn hash_index_is_spelled_hash_index_on_the_command_line() {
        let name = IndexStructure::HashIndex
            .to_possible_value()
            .expect("every IndexStructure is a CLI value")
            .get_name()
            .to_owned();
        assert_eq!(name, "hash-index");
        assert_eq!(
            "hash_index".parse::<IndexStructure>(),
            Ok(IndexStructure::HashIndex)
        );
    }
```

(b) `kermit-algos/src/lib.rs`. In `join_algorithm_tests`, quote:

```rust
        assert_eq!(JoinAlgorithm::HashTriejoin.axis_value(), "HashTriejoin");
```

and put before it:

```rust
        assert_eq!(
            JoinAlgorithm::BinaryHashJoin.axis_value(),
            "BinaryHashJoin"
        );
```

Add this test at the end of `join_algorithm_tests`:

```rust
    /// The `-a` spelling of the binary hash join.
    #[test]
    fn binary_hash_join_is_spelled_binary_hash_join_on_the_command_line() {
        let name = JoinAlgorithm::BinaryHashJoin
            .to_possible_value()
            .expect("every JoinAlgorithm is a CLI value")
            .get_name()
            .to_owned();
        assert_eq!(name, "binary-hash-join");
    }
```

(c) `kermit/src/main.rs` tests. Quote:

```rust
        assert_eq!(IndexStructureSelector::HashTrie.expand(), vec![
            IndexStructure::HashTrie
        ]);
    }
```

and make it:

```rust
        assert_eq!(IndexStructureSelector::HashTrie.expand(), vec![
            IndexStructure::HashTrie
        ]);
        assert_eq!(IndexStructureSelector::HashIndex.expand(), vec![
            IndexStructure::HashIndex
        ]);
    }

    /// `of` names exactly the structure it is given, for every structure.
    #[test]
    fn index_structure_selector_of_names_exactly_its_structure() {
        use clap::ValueEnum;
        for &ds in IndexStructure::value_variants() {
            assert_eq!(IndexStructureSelector::of(ds).expand(), vec![ds]);
        }
    }
```

Quote:

```rust
        assert_eq!(JoinAlgorithmSelector::HashTriejoin.expand(), vec![
            JoinAlgorithm::HashTriejoin
        ]);
    }
```

and make it:

```rust
        assert_eq!(JoinAlgorithmSelector::HashTriejoin.expand(), vec![
            JoinAlgorithm::HashTriejoin
        ]);
        assert_eq!(JoinAlgorithmSelector::BinaryHashJoin.expand(), vec![
            JoinAlgorithm::BinaryHashJoin
        ]);
    }
```

(d) `kermit/src/bench/run.rs` tests. In `resolve_sweep_rejects_a_flag_the_algorithm_leaves_without_a_cell`, quote:

```rust
            (
                JoinAlgorithmSelector::All,
                &[
                    LayoutHasher,
```

and put this row before it (a new `(algorithm, reached, unreached)` tuple):

```rust
            (
                JoinAlgorithmSelector::BinaryHashJoin,
                &[LayoutHasher],
                &[
                    LayoutPruning,
                    LayoutExpansion,
                    LayoutLevels,
                    LayoutSeek,
                    Config,
                    tree,
                    column,
                    hash,
                ],
            ),
```

(e) `kermit/src/options.rs` tests. Quote, in `build_choices_resolve_to_each_structures_labels`:

```rust
        let defaults = labels(&BuildModes::default());
        for ds in IndexStructure::value_variants() {
```

and make it:

```rust
        let defaults = labels(&BuildModes::default());
        // `hash-index` is built one way, so it is the one structure with no
        // `--ds-build` key.
        for ds in IndexStructure::value_variants()
            .iter()
            .filter(|&&ds| ds != IndexStructure::HashIndex)
        {
```

Add these tests at the end of the `tests` module, before its closing brace (all use names the module already imports through `super::*`):

```rust
    /// Every `with_hash_index_layout!` arm binds the marker its choice
    /// names, so a transposed arm fails here rather than mislabelling a
    /// bench report.
    #[test]
    fn hash_index_layout_macro_binds_the_marker_its_arm_names() {
        for &hasher in HasherChoice::value_variants() {
            assert_eq!(
                with_hash_index_layout!(hasher, |H| hasher_of::<H>()),
                hasher
            );
        }
    }

    /// The hasher is the hash index's only `--ds-*` axis: it reaches
    /// `hash-index`, and the table gives no other flag to it.
    #[test]
    fn ds_flag_structures_gives_hash_index_only_the_hasher() {
        let flags = [
            DsFlag::LayoutHasher,
            DsFlag::LayoutPruning,
            DsFlag::LayoutExpansion,
            DsFlag::LayoutLevels,
            DsFlag::LayoutSeek,
            DsFlag::Config,
            DsFlag::Build(IndexStructure::TreeTrie),
            DsFlag::Build(IndexStructure::ColumnTrie),
            DsFlag::Build(IndexStructure::HashTrie),
            DsFlag::Build(IndexStructure::HashIndex),
        ];
        for flag in flags {
            assert_eq!(
                flag.structures().contains(&IndexStructure::HashIndex),
                flag == DsFlag::LayoutHasher,
                "{flag}"
            );
        }
        assert_eq!(
            DsFlag::LayoutHasher.structures_label(),
            "hash-trie or hash-index"
        );
    }

    /// `hash-index` takes `--ds-layout-hasher` and refuses every other
    /// `--ds-*` flag, each with the usage error naming the structures that
    /// have its axis. `--ds-build hash-index=…` is refused as a structure
    /// with one build process.
    #[test]
    fn hash_index_takes_the_hasher_and_refuses_every_other_ds_flag() {
        let resolve = |layout: &LayoutChoices, config: &ConfigChoices, build: &BuildChoices| {
            DsChoices::resolve(IndexStructureSelector::HashIndex, layout, config, build)
        };
        let hasher = LayoutChoices {
            hash_trie_hasher: Some(HasherChoice::Fxhash),
            ..LayoutChoices::default()
        };
        let choices = resolve(&hasher, &ConfigChoices::default(), &BuildChoices::default())
            .expect("the hasher reaches the hash index");
        assert_eq!(choices.hasher, HasherChoice::Fxhash);

        let layouts = [
            (
                "--ds-layout-pruning",
                LayoutChoices {
                    hash_trie_pruning: Some(PruningChoice::On),
                    ..LayoutChoices::default()
                },
            ),
            (
                "--ds-layout-expansion",
                LayoutChoices {
                    hash_trie_expansion: Some(ExpansionChoice::Lazy),
                    ..LayoutChoices::default()
                },
            ),
            (
                "--ds-layout-levels",
                LayoutChoices {
                    hash_trie_levels: Some(LevelsChoice::Keys),
                    ..LayoutChoices::default()
                },
            ),
            (
                "--ds-layout-seek",
                LayoutChoices {
                    sorted_trie_seek: Some(SeekChoice::Linear),
                    ..LayoutChoices::default()
                },
            ),
        ];
        for (flag, layout) in &layouts {
            let msg = resolve(layout, &ConfigChoices::default(), &BuildChoices::default())
                .unwrap_err()
                .to_string();
            assert!(
                msg.contains(&format!("{flag} is only valid with --indexstructure")),
                "{msg}"
            );
        }
        let config = ConfigChoices {
            ds_config: vec!["load-factor=0.5".into()],
        };
        let msg = resolve(&LayoutChoices::default(), &config, &BuildChoices::default())
            .unwrap_err()
            .to_string();
        assert!(
            msg.contains("--ds-config is only valid with --indexstructure hash-trie"),
            "{msg}"
        );
        let msg = resolve(
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build(&["hash-trie=radix:4"]),
        )
        .unwrap_err()
        .to_string();
        assert!(
            msg.contains("--ds-build hash-trie is only valid with --indexstructure hash-trie"),
            "{msg}"
        );
        // A pair naming the hash index passes the selector check (no
        // structure has its flag) and is refused by `resolved`.
        let own = build(&["hash-index=bulk"]);
        assert_eq!(own.given(), vec![]);
        let msg = resolve(&LayoutChoices::default(), &ConfigChoices::default(), &own)
            .unwrap_err()
            .to_string();
        assert!(
            msg.contains("--ds-build: hash-index has a single build process"),
            "{msg}"
        );
    }
```

(f) `kermit/src/execution.rs` tests. Quote the helper:

```rust
    fn prepared_over(header: RelationHeader, tuples: Tuples, query: &str) -> (Catalog, Prepared) {
        use {
            kermit::db::prepare,
            kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        };
        let catalog = Catalog::new(vec![(header, tuples)], StatisticsLevel::TupleCounts);
        let query: JoinQuery = query.parse().expect("the query parses");
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared = prepare(&catalog, &query, &planner, kermit_ds::TrieLevels::All)
            .expect("the query prepares");
        (catalog, prepared)
    }
```

and make it:

```rust
    fn prepared_over(header: RelationHeader, tuples: Tuples, query: &str) -> (Catalog, Prepared) {
        prepared_over_levels(header, tuples, query, TrieLevels::All)
    }

    /// As [`prepared_over`], planned for the trie-level policy `levels`.
    fn prepared_over_levels(
        header: RelationHeader, tuples: Tuples, query: &str, levels: TrieLevels,
    ) -> (Catalog, Prepared) {
        use {
            kermit::db::prepare,
            kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        };
        let catalog = Catalog::new(vec![(header, tuples)], StatisticsLevel::TupleCounts);
        let query: JoinQuery = query.parse().expect("the query parses");
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared = prepare(&catalog, &query, &planner, levels).expect("the query prepares");
        (catalog, prepared)
    }
```

Quote the first sweep test:

```rust
    /// The full cross product yields exactly the three valid cells and
    /// skips the other three — the `-i all -a all` acceptance criterion.
    #[test]
    fn sweep_of_full_cross_product_yields_exactly_three_cells() {
        let sweep = Sweep::expand(&all_structures(), &all_algorithms(), DsChoices::default());
        assert_eq!(sweep.cells.len(), 3, "{sweep:?}");
        assert_eq!(sweep.skipped.len(), 3, "{sweep:?}");
```

and make it:

```rust
    /// The full cross product yields exactly the four valid cells and
    /// skips the other eight — the `-i all -a all` acceptance criterion.
    #[test]
    fn sweep_of_full_cross_product_yields_exactly_four_cells() {
        let sweep = Sweep::expand(&all_structures(), &all_algorithms(), DsChoices::default());
        assert_eq!(sweep.cells.len(), 4, "{sweep:?}");
        assert_eq!(sweep.skipped.len(), 8, "{sweep:?}");
```

Quote, in the same test:

```rust
        assert!(ran.contains(&(IndexStructure::HashTrie, JoinAlgorithm::HashTriejoin)));
    }
```

and make it:

```rust
        assert!(ran.contains(&(IndexStructure::HashTrie, JoinAlgorithm::HashTriejoin)));
        assert!(ran.contains(&(IndexStructure::HashIndex, JoinAlgorithm::BinaryHashJoin)));
    }
```

Quote the LFTJ test body:

```rust
        assert!(sweep
            .cells
            .iter()
            .all(|c| c.index_structure() != IndexStructure::HashTrie));
        assert_eq!(sweep.skipped, vec![(
            IndexStructure::HashTrie,
            JoinAlgorithm::LeapfrogTriejoin
        )]);
    }
```

and make it:

```rust
        assert!(sweep.cells.iter().all(|c| !matches!(
            c.index_structure(),
            IndexStructure::HashTrie | IndexStructure::HashIndex
        )));
        assert_eq!(sweep.skipped, vec![
            (IndexStructure::HashIndex, JoinAlgorithm::LeapfrogTriejoin),
            (IndexStructure::HashTrie, JoinAlgorithm::LeapfrogTriejoin),
        ]);
    }
```

Add these tests immediately before the helper `visited` (anchor: the doc comment `/// Every tuple `F::for_each_tuple` lends, collected — the test-side`):

```rust
    /// `-i all -a binary-hash-join` runs the hash-index cell alone and skips
    /// every other structure.
    #[test]
    fn sweep_with_binary_hash_join_runs_only_the_hash_index_cell() {
        let sweep = Sweep::expand(
            &all_structures(),
            &[JoinAlgorithm::BinaryHashJoin],
            DsChoices::default(),
        );
        assert_eq!(sweep.cells, vec![Execution::IndexBhj {
            hasher: HasherChoice::Sip
        }]);
        assert_eq!(sweep.skipped, vec![
            (IndexStructure::ColumnTrie, JoinAlgorithm::BinaryHashJoin),
            (IndexStructure::HashTrie, JoinAlgorithm::BinaryHashJoin),
            (IndexStructure::TreeTrie, JoinAlgorithm::BinaryHashJoin),
        ]);
    }

    /// A concrete pair that crosses the hash index with another family
    /// expands to no cell; the CLI turns that into a usage error.
    #[test]
    fn sweep_of_incompatible_hash_index_pairs_is_empty() {
        for (ds, algo) in [
            (IndexStructure::HashIndex, JoinAlgorithm::LeapfrogTriejoin),
            (IndexStructure::HashIndex, JoinAlgorithm::HashTriejoin),
            (IndexStructure::TreeTrie, JoinAlgorithm::BinaryHashJoin),
            (IndexStructure::ColumnTrie, JoinAlgorithm::BinaryHashJoin),
            (IndexStructure::HashTrie, JoinAlgorithm::BinaryHashJoin),
        ] {
            let sweep = Sweep::expand(&[ds], &[algo], DsChoices::default());
            assert!(sweep.cells.is_empty(), "{ds:?} with {algo:?}");
            assert_eq!(sweep.skipped, vec![(ds, algo)]);
        }
    }

    /// The hasher is the hash-index cell's only choice: no other `--ds-*`
    /// value changes it, `for_structure` and `for_pair` name the same cell,
    /// and its queries are prepared under `ProbeKeys` whatever the other
    /// cells' policies are.
    #[test]
    fn hash_index_cell_takes_only_the_hasher() {
        let busy = DsChoices {
            hasher: HasherChoice::Fxhash,
            pruning: PruningChoice::On,
            expansion: ExpansionChoice::Lazy,
            levels: LevelsChoice::Keys,
            seek: SeekChoice::Linear,
            config: HashTrieConfig {
                load_factor: LoadFactor::percent(50).unwrap(),
                ..HashTrieConfig::default()
            },
            build: BuildModes {
                hash_trie: HashTrieBuildMode::Radix(RadixBits::new(2).unwrap()),
                ..BuildModes::default()
            },
        };
        let cell = Execution::IndexBhj {
            hasher: HasherChoice::Fxhash,
        };
        assert_eq!(
            Execution::for_pair(
                IndexStructure::HashIndex,
                JoinAlgorithm::BinaryHashJoin,
                busy
            ),
            Some(cell)
        );
        assert_eq!(
            Execution::for_structure(IndexStructure::HashIndex, busy),
            cell
        );
        assert_eq!(cell.index_structure(), IndexStructure::HashIndex);
        assert_eq!(cell.algorithm(), JoinAlgorithm::BinaryHashJoin);
        assert_eq!(cell.trie_levels(), TrieLevels::ProbeKeys);
        assert_eq!(
            Execution::for_structure(IndexStructure::TreeTrie, busy).trie_levels(),
            TrieLevels::All
        );
        assert_eq!(
            Execution::for_structure(IndexStructure::HashTrie, busy).trie_levels(),
            TrieLevels::JoinKeys
        );
    }

    /// Both hash-index families derive the hasher label from `H`, and the
    /// join family names the binary hash join.
    #[test]
    fn hash_index_families_report_their_hasher_from_the_type() {
        use kermit_iters::FxHashStrategy;
        let sip = Execution::IndexBhj {
            hasher: HasherChoice::Sip,
        };
        let fx = Execution::IndexBhj {
            hasher: HasherChoice::Fxhash,
        };
        assert_eq!(HashIndexFamily::<SipHashStrategy>::default().execution(), sip);
        assert_eq!(IndexBhj::<SipHashStrategy>::default().execution(), sip);
        assert_eq!(HashIndexFamily::<FxHashStrategy>::default().execution(), fx);
        assert_eq!(IndexBhj::<FxHashStrategy>::default().execution(), fx);
        assert_eq!(
            IndexBhj::<FxHashStrategy>::default().execution().algorithm(),
            JoinAlgorithm::BinaryHashJoin
        );
    }

    /// A hash index has one build, and a join does not change it.
    #[test]
    fn hash_index_family_has_no_build_mode_and_does_not_mutate_on_join() {
        const { assert!(!<IndexBhj<SipHashStrategy> as ExecutionFamily>::JOIN_MUTATES) };
        assert!(HashIndexFamily::<SipHashStrategy>::default()
            .build_mode_axes()
            .is_empty());
        assert!(IndexBhj::<SipHashStrategy>::default()
            .build_mode_axes()
            .is_empty());
    }

    /// The family's report axes are the relation's own: the hasher it was
    /// built with, and no other `ds_*` axis.
    #[test]
    fn hash_index_family_reports_the_relations_hasher_axis() {
        fn axes<H: HashStrategy + 'static>() -> BTreeMap<String, serde_json::Value> {
            let tuples = Tuples::from(vec![vec![1, 2]]);
            let rel = HashIndexFamily::<H>::default().build_index(
                RelationHeader::new_positional("r", 2),
                TupleView::all(&tuples),
            );
            HashIndexFamily::<H>::optimization_axes(&rel)
        }
        let sip = axes::<SipHashStrategy>();
        assert_eq!(
            sip.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["ds_layout_hasher"]
        );
        assert_eq!(sip["ds_layout_hasher"], "sip");
        assert_eq!(axes::<kermit_iters::FxHashStrategy>()["ds_layout_hasher"], "fxhash");
    }

    /// `scan` counts every stored row, duplicates included, and agrees with
    /// `tuple_count`, in the marker and the join family alike.
    #[test]
    fn hash_index_scan_agrees_with_tuple_count_and_keeps_duplicates() {
        let tuples = Tuples::from(vec![vec![1, 2], vec![1, 2], vec![1, 3], vec![4, 5]]);
        let rel = HashIndexFamily::<SipHashStrategy>::default().build_index(
            RelationHeader::new_positional("r", 2),
            TupleView::all(&tuples),
        );
        assert_eq!(HashIndexFamily::<SipHashStrategy>::scan(&rel), 4);
        assert_eq!(HashIndexFamily::<SipHashStrategy>::tuple_count(&rel), 4);
        assert_eq!(IndexBhj::<SipHashStrategy>::scan(&rel), 4);
        assert_eq!(IndexBhj::<SipHashStrategy>::tuple_count(&rel), 4);
    }

    /// The hash-index families build what their view materialises, through
    /// `build_index`, on the four views of the shared fixture.
    #[test]
    fn hash_index_builds_what_its_view_materialises() {
        let source = view_source();
        let views = [
            TupleView::new(
                &source,
                kermit_ds::Filter::new(vec![(2, 5)], vec![]),
                vec![1, 0],
            ),
            TupleView::new(
                &source,
                kermit_ds::Filter::new(vec![], vec![(0, 1)]),
                vec![1, 2],
            ),
            TupleView::new(
                &source,
                kermit_ds::Filter::new(vec![(2, 9)], vec![]),
                vec![0, 1],
            ),
            TupleView::all(&source),
        ];
        for view in &views {
            assert_view_builds_like_batch(&HashIndexFamily::<SipHashStrategy>::default(), view);
            assert_view_builds_like_batch(&IndexBhj::<SipHashStrategy>::default(), view);
        }
    }

    /// The join family builds the indexes of a query prepared under
    /// `ProbeKeys` and streams the rows `count_rows` counts. The triangles
    /// of the graph below are (1, 2, 3) and (2, 3, 4); a duplicated row is
    /// kept, as it is by Hash Triejoin.
    #[test]
    fn index_bhj_streams_the_rows_count_rows_reports() {
        let family = IndexBhj::<SipHashStrategy>::default();
        let edges = Tuples::from(vec![vec![1, 2], vec![2, 3], vec![1, 3], vec![3, 4], vec![
            2, 4,
        ]]);
        let (catalog, prepared) = prepared_over_levels(
            RelationHeader::new_positional("edge", 2),
            edges,
            "Q(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).",
            TrieLevels::ProbeKeys,
        );
        let indexes = family.build_indexes(&catalog, &prepared);
        let mut streamed = Vec::new();
        family.execute(&prepared, &indexes, |row| streamed.push(row.to_vec()));
        streamed.sort();
        assert_eq!(streamed, vec![vec![1, 2, 3], vec![2, 3, 4]]);
        assert_eq!(family.count_rows(&prepared, &indexes), 2);

        let rows = Tuples::from(vec![vec![1, 2], vec![1, 2], vec![3, 4]]);
        let (catalog, prepared) = prepared_over_levels(
            RelationHeader::new_positional("r", 2),
            rows,
            "Q(X, Y) :- r(X, Y).",
            TrieLevels::ProbeKeys,
        );
        let indexes = family.build_indexes(&catalog, &prepared);
        assert_eq!(family.count_rows(&prepared, &indexes), 3);
    }
```

(g) The CLI integration tests that count a sweep's cells. Each edit is made in the named test, and no other. (The new hash-index cell reports `data_structure: "HashIndex"` and `algorithm: "BinaryHashJoin"`, carries `ds_layout_hasher` and no other `ds_*` axis, including no `ds_build_mode`.)

`kermit/tests/cli_bench_run_sweep.rs`. Quote:

```rust
//! must run exactly the three valid (structure, algorithm) cells, and no
```

and make it:

```rust
//! must run exactly the four valid (structure, algorithm) cells, and no
```

Quote:

```rust
        ("HashTrie", "HashTriejoin"),
    ]
```

and make it:

```rust
        ("HashTrie", "HashTriejoin"),
        ("HashIndex", "BinaryHashJoin"),
    ]
```

Quote:

```rust
/// Acceptance criterion: `-i all -a all` runs exactly the 3 valid cells.
#[test]
fn cli_bench_run_all_all_runs_exactly_the_three_valid_cells() {
```

and make it:

```rust
/// Acceptance criterion: `-i all -a all` runs exactly the 4 valid cells.
#[test]
fn cli_bench_run_all_all_runs_exactly_the_four_valid_cells() {
```

Quote:

```rust
    assert_eq!(reports.len(), 3, "one report per valid cell: {reports:?}");
```

and make it:

```rust
    assert_eq!(reports.len(), 4, "one report per valid cell: {reports:?}");
```

Quote:

```rust
        stderr.contains("skipping incompatible pair (HashTrie, LeapfrogTriejoin)"),
        "skipped cells must be announced; stderr: {stderr}"
    );
```

and make it:

```rust
        stderr.contains("skipping incompatible pair (HashTrie, LeapfrogTriejoin)"),
        "skipped cells must be announced; stderr: {stderr}"
    );
    assert!(
        stderr.contains("skipping incompatible pair (HashIndex, LeapfrogTriejoin)"),
        "skipped cells must be announced; stderr: {stderr}"
    );
```

Quote:

```rust
    assert!(
        !ran.iter().any(|(d, _)| d == "HashTrie"),
        "HashTrie must never be labelled LeapfrogTriejoin: {ran:?}"
    );
```

and make it:

```rust
    assert!(
        !ran.iter().any(|(d, _)| d == "HashTrie" || d == "HashIndex"),
        "a hash cell must never be labelled LeapfrogTriejoin: {ran:?}"
    );
```

`kermit/tests/cli_column_trie_build_mode.rs`, in both `cli_bench_run_sweep_reports_each_structures_build_mode` and `cli_bench_run_sweep_carries_incremental_only_to_column_trie_cells`: quote

```rust
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
```

and make it

```rust
    assert_eq!(reports.len(), 4, "four valid cells: {reports:?}");
```

and, in the `match axes["data_structure"].as_str()` of each, quote

```rust
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
```

and put before it

```rust
            | Some("HashIndex") => assert!(axes.get("ds_build_mode").is_none(), "{axes}"),
```

`kermit/tests/cli_tree_trie_build_mode.rs`, in `cli_bench_run_sweep_carries_parallel_only_to_tree_trie_cells`, and `kermit/tests/cli_hash_trie_build_mode.rs`, in both `cli_bench_run_sweep_carries_each_build_mode_to_its_cell` and `cli_bench_run_sweep_carries_each_structures_parallel_mode`: the same two edits (`3` to `4` with "four valid cells", and the `Some("HashIndex")` arm before `| other => panic!`).

`kermit/tests/cli_hash_trie_layout_expansion.rs` (`cli_bench_run_sweep_carries_expansion_only_to_hash_trie_cells`), `kermit/tests/cli_hash_trie_layout_pruning.rs` (`cli_bench_run_sweep_carries_pruning_only_to_hash_trie_cells`) and `kermit/tests/cli_hash_trie_layout_levels.rs` (`cli_bench_run_sweep_carries_levels_only_to_hash_trie_cells`): only the count, `assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");` to `assert_eq!(reports.len(), 4, "four valid cells: {reports:?}");`. Their `else` branches already assert that the cell has no such axis, which the hash index satisfies. In `cli_hash_trie_layout_pruning.rs` the comment line `// \`-i all -a all --ds-layout-pruning on\` runs the three valid cells; the` reads "four valid cells; the".

`kermit/tests/cli_bench_run_ds_flag_reach.rs`, in `every_flag_reaches_its_cell_under_all_all`: the count edit as above, and quote

```rust
            | Some("TreeTrie") => assert_eq!(axes["ds_layout_seek"], "binary", "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
```

and make it

```rust
            | Some("TreeTrie") => assert_eq!(axes["ds_layout_seek"], "binary", "{axes}"),
            | Some("HashIndex") => {
                assert_eq!(axes["ds_layout_hasher"], "fxhash", "{axes}");
                for absent in [
                    "ds_layout_pruning",
                    "ds_layout_seek",
                    "ds_config_load_factor",
                    "ds_build_mode",
                ] {
                    assert!(axes.get(absent).is_none(), "{absent} on {axes}");
                }
            },
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
```

`kermit/tests/cli_sorted_trie_layout_seek.rs`. In `cli_bench_run_sweep_carries_seek_only_to_sorted_trie_cells`, quote

```rust
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        if axes["data_structure"] == "HashTrie" {
            assert!(axes.get("ds_layout_seek").is_none(), "{axes}");
        } else {
            assert_eq!(axes["ds_layout_seek"], "linear", "{axes}");
        }
    }
```

and make it

```rust
    assert_eq!(reports.len(), 4, "four valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        if matches!(axes["data_structure"].as_str(), Some("HashTrie" | "HashIndex")) {
            assert!(axes.get("ds_layout_seek").is_none(), "{axes}");
        } else {
            assert_eq!(axes["ds_layout_seek"], "linear", "{axes}");
        }
    }
```

In `cli_bench_ds_reports_the_default_seek` (its doc comment reads "Both sorted cells of a `bench ds` sweep report the default, and the hash cell reports no seek axis."; make it "...and neither hash cell reports a seek axis."), quote

```rust
    assert_eq!(structures, ["ColumnTrie", "HashTrie", "TreeTrie"]);
    for r in &reports {
        let axes = &r["axes"];
        if axes["data_structure"] == "HashTrie" {
```

and make it

```rust
    assert_eq!(structures, ["ColumnTrie", "HashIndex", "HashTrie", "TreeTrie"]);
    for r in &reports {
        let axes = &r["axes"];
        if matches!(axes["data_structure"].as_str(), Some("HashTrie" | "HashIndex")) {
```

`kermit/tests/cli_snap_gen.rs`: quote

```rust
        12,
        "4 queries x the 3 valid cells; stderr: {stderr}"
```

and make it

```rust
        16,
        "4 queries x the 4 valid cells; stderr: {stderr}"
```

and quote `assert_eq!(structures.len(), 3, "{structures:?}");` and make it `assert_eq!(structures.len(), 4, "{structures:?}");`.

`kermit/tests/cli_bench_probe.rs`: quote `assert_eq!(reports.len(), 6, "two queries on three cells");` and make it `assert_eq!(reports.len(), 8, "two queries on four cells");`.

`kermit/tests/cli_bench_run_group_collision.rs`: quote

```rust
    let named: std::collections::BTreeSet<&str> = ["TreeTrie", "ColumnTrie", "HashTrie"]
```

and make it

```rust
    let named: std::collections::BTreeSet<&str> =
        ["TreeTrie", "ColumnTrie", "HashTrie", "HashIndex"]
```

(the sweep's first two cells are now `ColumnTrie` and `HashIndex`, and the error names those two groups).

- [ ] **Step 2: Run them to see them fail**

Run: `nix develop --command cargo test -p kermit-ds -p kermit-algos --lib`

Expected: neither crate compiles its tests. `error[E0599]: no variant or associated item named `HashIndex` found for enum `IndexStructure` in the current scope` in `kermit-ds/src/ds/mod.rs`, and `error[E0599]: no variant or associated item named `BinaryHashJoin` found for enum `JoinAlgorithm`` in `kermit-algos/src/lib.rs`. The `kermit` binary's tests fail the same way until Step 3 is complete.

- [ ] **Step 3: Implement**

(a) `kermit-ds/src/ds/mod.rs`. In the `IndexStructure` enum, quote:

```rust
    /// Column-oriented trie; see [`ColumnTrie`].
    ColumnTrie,
    /// Hash-based trie; see [`HashTrie`].
    HashTrie,
```

and make it:

```rust
    /// Column-oriented trie; see [`ColumnTrie`].
    ColumnTrie,
    /// Keyed hash index, probed by exact match on its leading columns; see
    /// [`HashIndex`].
    HashIndex,
    /// Hash-based trie; see [`HashTrie`].
    HashTrie,
```

In `from_str`, quote:

```rust
            | "column_trie" => Ok(Self::ColumnTrie),
```

and put after it:

```rust
            | "hash_index" => Ok(Self::HashIndex),
```

In `axis_value`, quote:

```rust
            | Self::ColumnTrie => "ColumnTrie",
```

and put after it:

```rust
            | Self::HashIndex => "HashIndex",
```

(b) `kermit-algos/src/lib.rs`. In the `JoinAlgorithm` enum, quote:

```rust
pub enum JoinAlgorithm {
    /// The Hash Trie Join algorithm (SIGMOD 2020); see [`HashTriejoin`].
    HashTriejoin,
```

and make it:

```rust
pub enum JoinAlgorithm {
    /// The left-deep pipelined binary hash join of Free Join (Wang et al.,
    /// 2023, §2.2); see [`BinaryHashJoin`].
    BinaryHashJoin,
    /// The Hash Trie Join algorithm (SIGMOD 2020); see [`HashTriejoin`].
    HashTriejoin,
```

In `axis_value`, quote:

```rust
            | Self::HashTriejoin => "HashTriejoin",
```

and put before it:

```rust
            | Self::BinaryHashJoin => "BinaryHashJoin",
```

In the comment above the `use clap::ValueEnum;` line, quote:

```rust
// aidan-bailey/kermit#60 (item 5). `JoinAlgorithm` lives here rather than in
// a family module because it spans both families; `Optimiser` lives beside
// its implementations in `optimiser`.
```

and make it:

```rust
// aidan-bailey/kermit#60 (item 5). `JoinAlgorithm` lives here rather than in
// a family module because it spans every family; `Optimiser` lives beside
// its implementations in `optimiser`.
```

(c) `kermit/src/options.rs`.

Module doc. Quote:

```rust
//! places the Layout products are monomorphised: `with_hash_trie_layout!`
//! and `with_sorted_trie_layout!`.
```

and make it:

```rust
//! places the Layout products are monomorphised: `with_hash_trie_layout!`,
//! `with_hash_index_layout!` and `with_sorted_trie_layout!`.
```

`DsFlag::structures`. Quote:

```rust
            | Self::LayoutHasher
            | Self::LayoutPruning
            | Self::LayoutExpansion
            | Self::LayoutLevels
            | Self::Config => &[IndexStructure::HashTrie],
```

and make it:

```rust
            | Self::LayoutHasher => &[IndexStructure::HashTrie, IndexStructure::HashIndex],
            | Self::LayoutPruning
            | Self::LayoutExpansion
            | Self::LayoutLevels
            | Self::Config => &[IndexStructure::HashTrie],
```

Quote:

```rust
            | Self::Build(IndexStructure::HashTrie) => &[IndexStructure::HashTrie],
```

and put after it:

```rust
            // The hash index is built one way, so `--ds-build` has no key for
            // it: `BuildChoices::given` never yields this flag.
            | Self::Build(IndexStructure::HashIndex) => &[],
```

`HasherChoice`'s doc. Quote:

```rust
/// This flag only applies when the selected index structure is `hash-trie`;
/// `validate_layout_choices` rejects it on other index structures so users
/// cannot silently pass it to a TreeTrie/ColumnTrie run.
```

and make it:

```rust
/// This flag only applies when the selected index structure is `hash-trie` or
/// `hash-index` (`HashIndex<H>` takes the same hasher); `validate_layout_choices`
/// rejects it on other index structures so users cannot silently pass it to a
/// TreeTrie/ColumnTrie run.
```

`LayoutChoices`' hasher field. Quote:

```rust
    /// Hash function used by `HashTrie<H, P>` (default: `sip`). Only valid
    /// when `--indexstructure hash-trie` is selected.
```

and make it:

```rust
    /// Hash function used by `HashTrie<H, P>` and `HashIndex<H>` (default:
    /// `sip`). Only valid when `--indexstructure hash-trie` or `hash-index` is
    /// selected.
```

The macro. Quote:

```rust
pub(crate) use with_hash_trie_layout;
```

and make it:

```rust
pub(crate) use with_hash_trie_layout;

/// Monomorphises `$body` over the `HashIndex` hasher selected at runtime.
///
/// The hash index has one Layout dimension, the hasher, so this is the
/// hasher rule of `with_hash_trie_layout!` and nothing else: the hasher
/// table stays written out once. Called by the hash-index arms of
/// `dispatch_run_bench`, `dispatch_ds_bench` and `write_query_csv`.
///
/// Hygiene contract as for `with_hash_trie_layout!`: `$H` becomes a type
/// alias scoped to the arm.
macro_rules! with_hash_index_layout {
    ($hasher:expr, | $H:ident | $body:expr) => {
        $crate::options::with_hash_trie_layout!(@hasher $hasher; $H; $body)
    };
}

pub(crate) use with_hash_index_layout;
```

`BuildChoices::resolved`. Quote:

```rust
                | IndexStructure::HashTrie => {
                    modes.hash_trie = mode
                        .parse()
                        .map_err(|why| anyhow::anyhow!("--ds-build hash-trie: {why}"))?;
                },
            }
```

and make it:

```rust
                | IndexStructure::HashTrie => {
                    modes.hash_trie = mode
                        .parse()
                        .map_err(|why| anyhow::anyhow!("--ds-build hash-trie: {why}"))?;
                },
                | IndexStructure::HashIndex => unreachable!("not in STRUCTURES, refused above"),
            }
```

(d) `kermit/src/execution.rs`.

Imports. Quote:

```rust
    kermit_algos::{HashTriejoin, JoinAlgorithm, LeapfrogTriejoin},
    kermit_ds::{
        AllLevels, BuildModeRelation, Cardinality, ColumnTrie, ColumnTrieBuildMode,
        ConfiguredBuildModeRelation, ExpansionPolicy, HashTrie, HashTrieBuildMode, HashTrieConfig,
        HeapSize, IndexStructure, LevelPolicy, PruningPolicy, Relation, RelationHeader,
        SeekStrategy, TreeTrie, TreeTrieBuildMode, TrieLevels, TupleView, Tuples,
    },
    kermit_iters::{
        BuildMode, HasOptimizationAxes, HashStrategy, TrieIterable, TrieIteratorWrapper,
    },
```

and make it:

```rust
    kermit_algos::{BinaryHashJoin, HashTriejoin, JoinAlgorithm, LeapfrogTriejoin},
    kermit_ds::{
        AllLevels, BuildModeRelation, Cardinality, ColumnTrie, ColumnTrieBuildMode,
        ConfiguredBuildModeRelation, ExpansionPolicy, HashIndex, HashTrie, HashTrieBuildMode,
        HashTrieConfig, HeapSize, IndexStructure, LevelPolicy, PruningPolicy, Relation,
        RelationHeader, SeekStrategy, TreeTrie, TreeTrieBuildMode, TrieLevels, TupleView, Tuples,
    },
    kermit_iters::{
        BuildMode, HasOptimizationAxes, HashStrategy, KeyedIterable, TrieIterable,
        TrieIteratorWrapper,
    },
```

Module doc. Quote:

```rust
//! The CLI exposes an index structure (`-i`) and a join algorithm (`-a`)
//! as two independent selectors, but only three of the six concrete
//! `(IndexStructure, JoinAlgorithm)` pairs are meaningful: the sorted
//! tries pair with [`LeapfrogTriejoin`] and the hash trie pairs with
//! [`HashTriejoin`]. The two families run through separate instantiations of
//! [`kermit::db::execute`] (`LeapfrogTriejoin` vs `HashTriejoin`), and
```

and make it:

```rust
//! The CLI exposes an index structure (`-i`) and a join algorithm (`-a`)
//! as two independent selectors, but only four of the twelve concrete
//! `(IndexStructure, JoinAlgorithm)` pairs are meaningful: the sorted
//! tries pair with [`LeapfrogTriejoin`], the hash trie with
//! [`HashTriejoin`] and the hash index with [`BinaryHashJoin`]. The three
//! families run through separate instantiations of [`kermit::db::execute`]
//! (`LeapfrogTriejoin`, `HashTriejoin` and `BinaryHashJoin`), and
```

The `Execution` enum. Quote the end of the `HashHtj` variant and the enum:

```rust
        /// The `--ds-build` mode every relation is built with.
        build: HashTrieBuildMode,
    },
}

impl Execution {
    /// The only way to obtain an `Execution` from a concrete pair. Returns
    /// `None` for the three incompatible pairs, which the sweep skips.
```

and make it:

```rust
        /// The `--ds-build` mode every relation is built with.
        build: HashTrieBuildMode,
    },
    /// `HashIndex<H>` joined by the binary hash join through
    /// [`kermit::db::execute`] with `BinaryHashJoin`, with `H` chosen by
    /// `--ds-layout-hasher`. The hash index has no other Layout, Config or
    /// BuildMode, and its queries are prepared under
    /// [`TrieLevels::ProbeKeys`].
    IndexBhj {
        /// The `--ds-layout-hasher` choice, in the two roles of
        /// [`Execution::HashHtj`]'s: the request that selects `H`, and the
        /// label re-derived from `H`.
        hasher: HasherChoice,
    },
}

impl Execution {
    /// The only way to obtain an `Execution` from a concrete pair. Returns
    /// `None` for the eight incompatible pairs, which the sweep skips.
```

`for_pair`. Quote:

```rust
            | (
                IndexStructure::TreeTrie | IndexStructure::ColumnTrie,
                JoinAlgorithm::HashTriejoin,
            )
            | (IndexStructure::HashTrie, JoinAlgorithm::LeapfrogTriejoin) => None,
```

and make it:

```rust
            | (IndexStructure::HashIndex, JoinAlgorithm::BinaryHashJoin) => {
                Some(Execution::IndexBhj {
                    hasher,
                })
            },
            | (
                IndexStructure::TreeTrie | IndexStructure::ColumnTrie,
                JoinAlgorithm::HashTriejoin,
            )
            | (IndexStructure::HashTrie, JoinAlgorithm::LeapfrogTriejoin)
            | (
                IndexStructure::HashIndex,
                JoinAlgorithm::HashTriejoin | JoinAlgorithm::LeapfrogTriejoin,
            )
            | (
                IndexStructure::TreeTrie | IndexStructure::ColumnTrie | IndexStructure::HashTrie,
                JoinAlgorithm::BinaryHashJoin,
            ) => None,
```

`for_structure`. Quote:

```rust
            | IndexStructure::HashTrie => Execution::HashHtj {
                hasher,
                pruning,
                expansion,
                levels,
                config,
                build: build.hash_trie,
            },
        }
    }
```

and make it:

```rust
            | IndexStructure::HashTrie => Execution::HashHtj {
                hasher,
                pruning,
                expansion,
                levels,
                config,
                build: build.hash_trie,
            },
            | IndexStructure::HashIndex => Execution::IndexBhj {
                hasher,
            },
        }
    }
```

The three accessors. Quote:

```rust
            | Self::HashHtj {
                ..
            } => IndexStructure::HashTrie,
```

and put after it:

```rust
            | Self::IndexBhj {
                ..
            } => IndexStructure::HashIndex,
```

Quote:

```rust
            | Self::HashHtj {
                ..
            } => JoinAlgorithm::HashTriejoin,
```

and put after it:

```rust
            | Self::IndexBhj {
                ..
            } => JoinAlgorithm::BinaryHashJoin,
```

Quote:

```rust
    /// The trie-level policy this cell's queries are prepared under
    /// (`kermit::db::prepare`): `All` for the sorted tries, the
    /// `--ds-layout-levels` choice for `HashTrie` (#116).
    pub fn trie_levels(self) -> TrieLevels {
        match self {
            | Self::TrieLftj(_) => TrieLevels::All,
            | Self::HashHtj {
                levels, ..
            } => levels.trie_levels(),
        }
    }
```

and make it:

```rust
    /// The trie-level policy this cell's queries are prepared under
    /// (`kermit::db::prepare`): `All` for the sorted tries, the
    /// `--ds-layout-levels` choice for `HashTrie` (#116), and `ProbeKeys` for
    /// the hash index, whose levels are the probe key of each atom.
    pub fn trie_levels(self) -> TrieLevels {
        match self {
            | Self::TrieLftj(_) => TrieLevels::All,
            | Self::HashHtj {
                levels, ..
            } => levels.trie_levels(),
            | Self::IndexBhj {
                ..
            } => TrieLevels::ProbeKeys,
        }
    }
```

The families. Insert `HashIndexFamily<H>` immediately before the doc comment `/// Sorted family: \`R\` under Leapfrog Triejoin, through \`kermit::db::execute\`.`:

```rust
/// `HashIndex<H>` on its own: what `bench ds -i hash-index` measures. The
/// index has one build and no Config, so the family carries no value; the
/// `--ds-layout-hasher` label is *not* a parameter either:
/// [`execution`](RelationFamily::execution) derives it from `H`, so a report
/// cannot name a hasher the family was not monomorphised over.
pub struct HashIndexFamily<H> {
    _hasher: PhantomData<H>,
}

impl<H> HashIndexFamily<H> {
    /// The family building every index with the hasher `H`.
    pub fn new() -> Self {
        Self {
            _hasher: PhantomData,
        }
    }
}

impl<H> Default for HashIndexFamily<H> {
    fn default() -> Self { Self::new() }
}

impl<H: HashStrategy + 'static> RelationFamily for HashIndexFamily<H> {
    type Rel = HashIndex<H>;

    fn execution(&self) -> Execution {
        Execution::IndexBhj {
            hasher: hasher_of::<H>(),
        }
    }

    /// The one build: `HashIndex::from_view`. The header's levels (the arity
    /// for `bench ds`, a probe key's length under a prepared query) are the
    /// index's key length.
    fn build_index(&self, header: RelationHeader, view: TupleView<'_>) -> HashIndex<H> {
        HashIndex::<H>::from_view(header, view)
    }

    fn for_each_tuple<V: FnMut(&[usize])>(rel: &HashIndex<H>, mut visit: V) {
        for row in rel.scan() {
            visit(row);
        }
    }

    /// The stored row count, duplicates included: a hash index keeps them.
    fn tuple_count(rel: &HashIndex<H>) -> usize { Cardinality::tuple_count(rel) }

    fn optimization_axes(rel: &HashIndex<H>) -> BTreeMap<String, serde_json::Value> {
        rel.optimization_axes()
    }

    /// A hash index has one build process, so its reports carry no
    /// `ds_build_mode`.
    fn build_mode_axes(&self) -> BTreeMap<String, serde_json::Value> { BTreeMap::new() }
}

```

Insert `IndexBhj<H>` immediately before `#[cfg(test)]\nmod tests {`:

```rust
/// Keyed family: `HashIndex<H>` under the binary hash join, through
/// `kermit::db::execute`.
pub struct IndexBhj<H> {
    structure: HashIndexFamily<H>,
}

impl<H> IndexBhj<H> {
    /// Creates the family. The `--ds-layout-hasher` label is *not* a
    /// parameter: [`execution`](RelationFamily::execution) derives it from
    /// `H`, as for [`HashHtj`].
    pub fn new() -> Self {
        Self {
            structure: HashIndexFamily::new(),
        }
    }
}

impl<H> Default for IndexBhj<H> {
    fn default() -> Self { Self::new() }
}

impl<H: HashStrategy + 'static> RelationFamily for IndexBhj<H> {
    type Rel = HashIndex<H>;

    fn execution(&self) -> Execution { self.structure.execution() }

    fn build_index(&self, header: RelationHeader, view: TupleView<'_>) -> HashIndex<H> {
        self.structure.build_index(header, view)
    }

    fn for_each_tuple<V: FnMut(&[usize])>(rel: &HashIndex<H>, visit: V) {
        HashIndexFamily::<H>::for_each_tuple(rel, visit);
    }

    fn tuple_count(rel: &HashIndex<H>) -> usize { HashIndexFamily::<H>::tuple_count(rel) }

    fn optimization_axes(rel: &HashIndex<H>) -> BTreeMap<String, serde_json::Value> {
        HashIndexFamily::<H>::optimization_axes(rel)
    }

    fn build_mode_axes(&self) -> BTreeMap<String, serde_json::Value> {
        self.structure.build_mode_axes()
    }
}

impl<H: HashStrategy + 'static> ExecutionFamily for IndexBhj<H> {
    /// A hash index is immutable under a join: the join only reads its
    /// buckets and rows.
    const JOIN_MUTATES: bool = false;

    fn execute<S: FnMut(&[usize])>(
        &self, prepared: &Prepared, indexes: &Indexes<HashIndex<H>>, emit: S,
    ) {
        db::execute::<HashIndex<H>, BinaryHashJoin>(prepared, indexes, emit);
    }
}

```

(e) `kermit/src/bench/run.rs`.

Imports. Quote:

```rust
        execution::{Execution, ExecutionFamily, HashHtj, SortedTrie, Sweep, TrieLftj},
        options::{
            unreached_flag, with_hash_trie_layout, with_sorted_trie_layout, DsChoices, DsFlag,
            PlannerArgs,
        },
```

and make it:

```rust
        execution::{Execution, ExecutionFamily, HashHtj, IndexBhj, SortedTrie, Sweep, TrieLftj},
        options::{
            unreached_flag, with_hash_index_layout, with_hash_trie_layout, with_sorted_trie_layout,
            DsChoices, DsFlag, PlannerArgs,
        },
```

`describe_index`. Quote its doc comment:

```rust
/// The value of one `index` metadata line, for the `position`th index of a
/// query: `{id} {relation} filter {filter} columns {columns:?}[ levels {n}]:
/// {rows} rows, {bytes} bytes`. ` levels {n}` appears only when the trie has
/// levels for fewer columns than it keeps (`--ds-layout-levels keys`, #116),
/// so every `all` line reads as before. `rows` is what the build copied, the
/// index view's count, and `bytes` the index's footprint as built.
```

and make it (the code below it is unchanged):

```rust
/// The value of one `index` metadata line, for the `position`th index of a
/// query: `{id} {relation} filter {filter} columns {columns:?}[ levels {n}]:
/// {rows} rows, {bytes} bytes`. ` levels {n}` appears whenever the index has
/// levels for fewer columns than it keeps: under `--ds-layout-levels keys`
/// (#116) that is a trie cut after its last join key, and under the hash
/// index's `ProbeKeys` planning it is the index's key length (` levels 0` for
/// the driver, which is scanned, not probed). So every `all` line reads as
/// before. `rows` is what the build copied, the index view's count, and
/// `bytes` the index's footprint as built.
```

`dispatch_run_bench`. Quote:

```rust
        } => with_hash_trie_layout!(hasher, pruning, expansion, levels, |H, P, E, L| {
            run_benchmark(
                &HashHtj::<H, P, E, L>::new(config, build),
                workload,
                settings,
                sink,
            )
        }),
    }
}
```

and make it:

```rust
        } => with_hash_trie_layout!(hasher, pruning, expansion, levels, |H, P, E, L| {
            run_benchmark(
                &HashHtj::<H, P, E, L>::new(config, build),
                workload,
                settings,
                sink,
            )
        }),
        | Execution::IndexBhj {
            hasher,
        } => with_hash_index_layout!(hasher, |H| run_benchmark(
            &IndexBhj::<H>::new(),
            workload,
            settings,
            sink,
        )),
    }
}
```

`resolve_sweep`. Quote the doc:

```rust
/// skipped, so `-i all -a all` runs exactly the
/// three valid cells; when the user named a single incompatible pair there
```

and make it:

```rust
/// skipped, so `-i all -a all` runs exactly the
/// four valid cells; when the user named a single incompatible pair there
```

Quote:

```rust
             with --algorithm {algorithm:?} (hash-trie pairs with hash-triejoin; sorted tries \
             pair with leapfrog-triejoin)"
```

and make it:

```rust
             with --algorithm {algorithm:?} (hash-trie pairs with hash-triejoin; hash-index \
             pairs with binary-hash-join; sorted tries pair with leapfrog-triejoin)"
```

(f) `kermit/src/bench/ds.rs`.

Imports. Quote:

```rust
        execution::{
            read_relation, Execution, HashTrieFamily, RelationFamily, SortedTrie, SortedTrieFamily,
        },
        options::{with_hash_trie_layout, with_sorted_trie_layout, DsChoices},
```

and make it:

```rust
        execution::{
            read_relation, Execution, HashIndexFamily, HashTrieFamily, RelationFamily, SortedTrie,
            SortedTrieFamily,
        },
        options::{
            with_hash_index_layout, with_hash_trie_layout, with_sorted_trie_layout, DsChoices,
        },
```

`dispatch_ds_bench`. Quote the end of the function:

```rust
            with_hash_trie_layout!(hasher, pruning, expansion, |H, P, E| run_ds_bench(
                &HashTrieFamily::<H, P, E>::new(config, build),
                relation,
                metrics,
                queries_per_build,
                group_name,
                criterion,
            ))
        },
    }
}
```

and make it:

```rust
            with_hash_trie_layout!(hasher, pruning, expansion, |H, P, E| run_ds_bench(
                &HashTrieFamily::<H, P, E>::new(config, build),
                relation,
                metrics,
                queries_per_build,
                group_name,
                criterion,
            ))
        },
        | Execution::IndexBhj {
            hasher,
        } => with_hash_index_layout!(hasher, |H| run_ds_bench(
            &HashIndexFamily::<H>::new(),
            relation,
            metrics,
            queries_per_build,
            group_name,
            criterion,
        )),
    }
}
```

(g) `kermit/src/main.rs`.

Imports. Quote:

```rust
    execution::{Execution, ExecutionFamily, HashHtj, SortedTrie, TrieLftj},
    options::{
        with_hash_trie_layout, with_sorted_trie_layout, BuildChoices, ConfigChoices, DsChoices,
        DsFlag, LayoutChoices, PlannerArgs,
    },
```

and make it:

```rust
    execution::{Execution, ExecutionFamily, HashHtj, IndexBhj, SortedTrie, TrieLftj},
    options::{
        with_hash_index_layout, with_hash_trie_layout, with_sorted_trie_layout, BuildChoices,
        ConfigChoices, DsChoices, DsFlag, LayoutChoices, PlannerArgs,
    },
```

`IndexStructureSelector`. Quote:

```rust
enum IndexStructureSelector {
    All,
    ColumnTrie,
    HashTrie,
    TreeTrie,
}
```

and make it:

```rust
enum IndexStructureSelector {
    All,
    ColumnTrie,
    HashIndex,
    HashTrie,
    TreeTrie,
}
```

Quote:

```rust
            | IndexStructure::ColumnTrie => Self::ColumnTrie,
            | IndexStructure::HashTrie => Self::HashTrie,
```

and make it:

```rust
            | IndexStructure::ColumnTrie => Self::ColumnTrie,
            | IndexStructure::HashIndex => Self::HashIndex,
            | IndexStructure::HashTrie => Self::HashTrie,
```

Quote:

```rust
            | Self::ColumnTrie => vec![IndexStructure::ColumnTrie],
            | Self::HashTrie => vec![IndexStructure::HashTrie],
```

and make it:

```rust
            | Self::ColumnTrie => vec![IndexStructure::ColumnTrie],
            | Self::HashIndex => vec![IndexStructure::HashIndex],
            | Self::HashTrie => vec![IndexStructure::HashTrie],
```

`JoinAlgorithmSelector`. Quote:

```rust
enum JoinAlgorithmSelector {
    All,
    HashTriejoin,
    LeapfrogTriejoin,
}
```

and make it:

```rust
enum JoinAlgorithmSelector {
    All,
    BinaryHashJoin,
    HashTriejoin,
    LeapfrogTriejoin,
}
```

Quote:

```rust
            | Self::All => JoinAlgorithm::value_variants().to_vec(),
            | Self::HashTriejoin => vec![JoinAlgorithm::HashTriejoin],
```

and make it:

```rust
            | Self::All => JoinAlgorithm::value_variants().to_vec(),
            | Self::BinaryHashJoin => vec![JoinAlgorithm::BinaryHashJoin],
            | Self::HashTriejoin => vec![JoinAlgorithm::HashTriejoin],
```

`write_query_csv`. Quote:

```rust
        } => with_hash_trie_layout!(hasher, pruning, expansion, levels, |H, P, E, L| {
            write_prepared_join(
                HashHtj::<H, P, E, L>::new(config, build),
                cell,
                workload,
                writer,
            )
        }),
    }
}
```

and make it:

```rust
        } => with_hash_trie_layout!(hasher, pruning, expansion, levels, |H, P, E, L| {
            write_prepared_join(
                HashHtj::<H, P, E, L>::new(config, build),
                cell,
                workload,
                writer,
            )
        }),
        | Execution::IndexBhj {
            hasher,
        } => with_hash_index_layout!(hasher, |H| write_prepared_join(
            IndexBhj::<H>::new(),
            cell,
            workload,
            writer,
        )),
    }
}
```

The policy comment in `run_bench_run_command`. Quote:

```rust
    // Each query is prepared once per trie-level policy its cells build
    // under: at most two, since each Layout flag takes one value (#116).
```

and make it:

```rust
    // Each query is prepared once per trie-level policy its cells build
    // under: at most three (`All`, `JoinKeys` and the hash index's
    // `ProbeKeys`), since each Layout flag takes one value (#116).
```

- [ ] **Step 4: Run them to see them pass**

Run: `nix develop --command bash -c 'cargo test -p kermit-ds -p kermit-algos --lib && cargo test -p kermit'`

Expected: PASS. The new unit tests (`hash_index_is_spelled_hash_index_on_the_command_line`, `binary_hash_join_is_spelled_binary_hash_join_on_the_command_line`, `sweep_with_binary_hash_join_runs_only_the_hash_index_cell`, `hash_index_cell_takes_only_the_hasher`, `index_bhj_streams_the_rows_count_rows_reports`, `hash_index_takes_the_hasher_and_refuses_every_other_ds_flag`, the macro test) and every existing CLI test pass, the sweep tests with four cells.

Then run the gate's lints over the touched packages: `nix develop --command bash -c 'cargo clippy -p kermit-ds -p kermit-algos -p kermit --all-targets -- -D warnings'` (expected: no warnings).

- [ ] **Step 5: Commit**

Run `nix develop --command cargo fmt --all` first (the repo's nightly rustfmt reflows the match arms and the long `use` lists), then:

```bash
git commit -m "feat(kermit): wire the hash-index binary hash join cell into the binary" -- kermit-ds/src/ds/mod.rs kermit-algos/src/lib.rs kermit/src/main.rs kermit/src/execution.rs kermit/src/options.rs kermit/src/bench/run.rs kermit/src/bench/ds.rs kermit/tests/cli_bench_run_sweep.rs kermit/tests/cli_column_trie_build_mode.rs kermit/tests/cli_bench_run_ds_flag_reach.rs kermit/tests/cli_hash_trie_layout_expansion.rs kermit/tests/cli_hash_trie_layout_pruning.rs kermit/tests/cli_hash_trie_layout_levels.rs kermit/tests/cli_tree_trie_build_mode.rs kermit/tests/cli_hash_trie_build_mode.rs kermit/tests/cli_sorted_trie_layout_seek.rs kermit/tests/cli_snap_gen.rs kermit/tests/cli_bench_probe.rs kermit/tests/cli_bench_run_group_collision.rs
```

### Task 4.2: The CLI tests for the new cell

**Files:**
- Create: `kermit/tests/cli_hash_index_hasher_choice.rs`
- Modify: `kermit/tests/cli_query_errors.rs:1-3`, `:22-23`, `:103-114`, `:452-457`
- Modify: `kermit/tests/cli_bench_run_sweep.rs` (after `cli_bench_run_explicit_incompatible_pair_is_rejected`)
- Modify: `kermit/tests/cli_bench_run_ds_flag_reach.rs` (after `a_narrowed_sweep_keeping_the_flags_cell_still_runs`)
- Modify: `kermit/tests/cli_join_tests.rs:276-282` (the cell list of `cli_join_writes_the_heads_variables_as_the_header_row`)
- Modify: `kermit/tests/cli_optimiser_choice.rs:121-125` (the cell list of the optimiser-choice row comparison)

- [ ] **Step 1: Write the failing tests**

Create `kermit/tests/cli_hash_index_hasher_choice.rs`:

```rust
//! CLI smoke test for the `hash-index` structure and the binary hash join:
//! `--ds-layout-hasher` reaches the structure and its report axis, a report
//! names `HashIndex` / `BinaryHashJoin` and carries no other `ds_*` axis, the
//! cell verifies the triangle benchmark's answer, and every other `--ds-*`
//! flag, and every other (structure, algorithm) pair with the cell's two
//! halves, is a usage error.

mod common;

use {
    common::cli::{axes_of, bench_ds, bench_join, bench_run, kermit_bin},
    std::{path::PathBuf, process::Command},
};

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: &std::process::Output) {
    assert!(output.status.success(), "{}", stderr_of(output));
}

/// Every `ds_*` key of `axes`, sorted.
fn ds_axes(axes: &serde_json::Value) -> Vec<&str> {
    let mut keys: Vec<&str> = axes
        .as_object()
        .expect("axes is an object")
        .keys()
        .map(String::as_str)
        .filter(|key| key.starts_with("ds_"))
        .collect();
    keys.sort_unstable();
    keys
}

#[test]
fn cli_bench_ds_hash_index_default_hasher_is_sip() {
    let (output, report) = bench_ds("hash-index", &[]);
    assert_success(&output);
    let axes = axes_of(&report);
    assert_eq!(axes["data_structure"], "HashIndex", "{axes}");
    assert_eq!(axes["ds_layout_hasher"], "sip", "{axes}");
    assert_eq!(ds_axes(&axes), ["ds_layout_hasher"], "{axes}");
}

#[test]
fn cli_bench_ds_hash_index_with_fxhash_records_axis() {
    let (output, report) = bench_ds("hash-index", &["--ds-layout-hasher", "fxhash"]);
    assert_success(&output);
    let axes = axes_of(&report);
    assert_eq!(axes["ds_layout_hasher"], "fxhash", "{axes}");
    assert_eq!(ds_axes(&axes), ["ds_layout_hasher"], "{axes}");
}

/// `bench run` on the triangle benchmark, verified against its `expected`
/// count: the hash index and the binary hash join answer it.
#[test]
fn cli_bench_run_hash_index_verifies_the_triangle() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "hash-index",
        "-a",
        "binary-hash-join",
        "-m",
        "space",
        "--verify",
        "--ds-layout-hasher",
        "fxhash",
    ]);
    assert_success(&output);
    let axes = axes_of(&report);
    assert_eq!(axes["data_structure"], "HashIndex", "{axes}");
    assert_eq!(axes["algorithm"], "BinaryHashJoin", "{axes}");
    assert_eq!(axes["ds_layout_hasher"], "fxhash", "{axes}");
    assert_eq!(axes["verified"], true, "{axes}");
    assert_eq!(ds_axes(&axes), ["ds_layout_hasher"], "{axes}");
}

#[test]
fn cli_bench_join_hash_index_reports_the_cell() {
    let (output, report) = bench_join("hash-index", "binary-hash-join", &["-m", "space"]);
    assert_success(&output);
    let axes = axes_of(&report);
    assert_eq!(axes["benchmark"], "adhoc", "{axes}");
    assert_eq!(axes["data_structure"], "HashIndex", "{axes}");
    assert_eq!(axes["algorithm"], "BinaryHashJoin", "{axes}");
    assert_eq!(axes["ds_layout_hasher"], "sip", "{axes}");
}

/// `kermit join` writes the same rows on the hash index as on the hash trie.
#[test]
fn cli_join_hash_index_writes_the_rows_the_hash_trie_writes() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let rows = |structure: &str, algorithm: &str| {
        let output = Command::new(kermit_bin())
            .arg("join")
            .arg("-r")
            .arg(fixtures.join("first.csv"))
            .arg(fixtures.join("second.csv"))
            .arg("-q")
            .arg(fixtures.join("intersect_query.dl"))
            .args(["-i", structure, "-a", algorithm])
            .output()
            .expect("failed to execute kermit binary");
        assert_success(&output);
        let mut lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_owned)
            .collect();
        lines.sort();
        lines
    };
    let hash_index = rows("hash-index", "binary-hash-join");
    assert_eq!(hash_index, ["2", "X"]);
    assert_eq!(hash_index, rows("hash-trie", "hash-triejoin"));
}

/// Every `--ds-*` flag but `--ds-layout-hasher` is refused on `hash-index`,
/// with the usage error naming the structures that have its axis.
#[test]
fn cli_hash_index_refuses_every_other_ds_flag() {
    let refused: [(&[&str], &str); 7] = [
        (
            &["--ds-layout-pruning", "on"],
            "--ds-layout-pruning is only valid with --indexstructure hash-trie",
        ),
        (
            &["--ds-layout-expansion", "lazy"],
            "--ds-layout-expansion is only valid with --indexstructure hash-trie",
        ),
        (
            &["--ds-layout-levels", "keys"],
            "--ds-layout-levels is only valid with --indexstructure hash-trie",
        ),
        (
            &["--ds-layout-seek", "galloping"],
            "--ds-layout-seek is only valid with --indexstructure tree-trie or column-trie",
        ),
        (
            &["--ds-config", "load-factor=0.5"],
            "--ds-config is only valid with --indexstructure hash-trie",
        ),
        (
            &["--ds-build", "hash-trie=radix:4"],
            "--ds-build hash-trie is only valid with --indexstructure hash-trie",
        ),
        (
            &["--ds-build", "hash-index=bulk"],
            "--ds-build: hash-index has a single build process",
        ),
    ];
    for (args, fragment) in refused {
        let (output, _) = bench_join("hash-index", "binary-hash-join", args);
        let stderr = stderr_of(&output);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {stderr}");
        assert!(stderr.contains(fragment), "{args:?}: expected {fragment:?} in {stderr}");
    }
}

/// The hash index pairs only with the binary hash join, and the binary hash
/// join only with the hash index.
#[test]
fn cli_hash_index_incompatible_pairs_are_usage_errors() {
    for (structure, algorithm, fragment) in [
        (
            "hash-index",
            "hash-triejoin",
            "incompatible selection: HashIndex cannot run under HashTriejoin",
        ),
        (
            "hash-index",
            "leapfrog-triejoin",
            "incompatible selection: HashIndex cannot run under LeapfrogTriejoin",
        ),
        (
            "tree-trie",
            "binary-hash-join",
            "incompatible selection: TreeTrie cannot run under BinaryHashJoin",
        ),
        (
            "hash-trie",
            "binary-hash-join",
            "incompatible selection: HashTrie cannot run under BinaryHashJoin",
        ),
    ] {
        let (output, _) = bench_join(structure, algorithm, &[]);
        let stderr = stderr_of(&output);
        assert_eq!(output.status.code(), Some(1), "{structure} {algorithm}: {stderr}");
        assert!(
            stderr.contains(fragment),
            "{structure} {algorithm}: expected {fragment:?} in {stderr}"
        );
    }
}
```

`kermit/tests/cli_query_errors.rs`. Quote:

```rust
//! Every query shape from issue #78's table, through the real binary, on
//! all ten cells: TreeTrie and ColumnTrie under LFTJ, and HashTrie under
//! Hash Triejoin with each hasher × pruning × expansion Layout.
```

and make it:

```rust
//! Every query shape from issue #78's table, through the real binary, on
//! all twelve cells: TreeTrie and ColumnTrie under LFTJ, HashTrie under
//! Hash Triejoin with each hasher × pruning × expansion Layout, and
//! HashIndex under the binary hash join with each hasher.
```

Quote:

```rust
/// The ten (structure, algorithm, Layout) cells `kermit join` can run.
const CELLS: [&[&str]; 10] = [
```

and make it:

```rust
/// The twelve (structure, algorithm, Layout) cells `kermit join` can run.
const CELLS: [&[&str]; 12] = [
```

Quote the last cell and the closing of the array:

```rust
        "--ds-layout-hasher",
        "fxhash",
        "--ds-layout-pruning",
        "on",
        "--ds-layout-expansion",
        "lazy",
    ],
];
```

and make it:

```rust
        "--ds-layout-hasher",
        "fxhash",
        "--ds-layout-pruning",
        "on",
        "--ds-layout-expansion",
        "lazy",
    ],
    &["-i", "hash-index", "-a", "binary-hash-join"],
    &[
        "-i",
        "hash-index",
        "-a",
        "binary-hash-join",
        "--ds-layout-hasher",
        "fxhash",
    ],
];
```

In `bench_join_output_and_benchmark_share_one_prepared_workload`, quote:

```rust
        ("hash-trie", "hash-triejoin"),
    ] {
```

and make it:

```rust
        ("hash-trie", "hash-triejoin"),
        ("hash-index", "binary-hash-join"),
    ] {
```

`kermit/tests/cli_bench_run_sweep.rs`. Add after `cli_bench_run_explicit_incompatible_pair_is_rejected`:

```rust
/// `-i all -a binary-hash-join` runs the hash-index cell alone and announces
/// every other structure it skips.
#[test]
fn cli_bench_run_all_structures_with_binary_hash_join_runs_only_the_hash_index() {
    if skip_unsupported() {
        return;
    }
    let (output, reports) = run_sweep("all", "binary-hash-join");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    let reports = reports.expect("a report must be written");
    assert_eq!(
        cells(&reports),
        BTreeSet::from([("HashIndex".to_string(), "BinaryHashJoin".to_string())]),
        "{reports:?}"
    );
    for ds in ["ColumnTrie", "HashTrie", "TreeTrie"] {
        assert!(
            stderr.contains(&format!("skipping incompatible pair ({ds}, BinaryHashJoin)")),
            "{ds} must be announced as skipped; stderr: {stderr}"
        );
    }
}

/// A single explicit pair that crosses the hash index with another family
/// is a usage error that writes no report.
#[test]
fn cli_bench_run_explicit_hash_index_incompatible_pairs_are_rejected() {
    if skip_unsupported() {
        return;
    }
    for (ds, algo) in [
        ("hash-index", "leapfrog-triejoin"),
        ("hash-index", "hash-triejoin"),
        ("tree-trie", "binary-hash-join"),
        ("column-trie", "binary-hash-join"),
        ("hash-trie", "binary-hash-join"),
    ] {
        let (output, reports) = run_sweep(ds, algo);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{ds} with {algo} must fail");
        assert!(
            stderr.contains("incompatible CLI selection"),
            "{ds} with {algo}: {stderr}"
        );
        assert!(reports.is_none(), "{ds} with {algo}: {reports:?}");
    }
}
```

`kermit/tests/cli_bench_run_ds_flag_reach.rs`. Add after `a_narrowed_sweep_keeping_the_flags_cell_still_runs`:

```rust
/// `-a binary-hash-join` keeps the hash-index cell, which has the hasher
/// axis, so `--ds-layout-hasher` reaches it.
#[test]
fn ds_layout_hasher_reaches_the_hash_index_cell_under_binary_hash_join() {
    let (output, report) = narrowed_sweep("binary-hash-join", &["--ds-layout-hasher", "fxhash"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 1, "only the hash-index cell: {reports:?}");
    assert_eq!(reports[0]["axes"]["data_structure"], "HashIndex");
    assert_eq!(reports[0]["axes"]["ds_layout_hasher"], "fxhash");
}

/// The hash-index cell has no other axis, so under `-a binary-hash-join`
/// every other flag reaches no cell and is a usage error.
#[test]
fn every_other_flag_is_rejected_when_binary_hash_join_leaves_only_the_hash_index_cell() {
    let rejected: [(&str, &[&str]); 7] = [
        ("--ds-layout-pruning", &["--ds-layout-pruning", "on"]),
        ("--ds-layout-expansion", &["--ds-layout-expansion", "lazy"]),
        ("--ds-layout-levels", &["--ds-layout-levels", "keys"]),
        ("--ds-layout-seek", &["--ds-layout-seek", "binary"]),
        ("--ds-config", &["--ds-config", "load-factor=0.5"]),
        (
            "--ds-build tree-trie",
            &["--ds-build", "tree-trie=parallel:2"],
        ),
        (
            "--ds-build hash-trie",
            &["--ds-build", "hash-trie=radix:4"],
        ),
    ];
    for (flag, args) in rejected {
        let (output, report) = narrowed_sweep("binary-hash-join", args);
        assert_rejected_unreached(&output, &report, flag);
    }
}
```

Add the hash-index cell to the two existing tests that run `kermit join` once per cell, so `write_query_csv`'s `IndexBhj` arm is exercised by a passing join and not only by `cli_query_errors`.

In `kermit/tests/cli_join_tests.rs`, quote:

```rust
        ("leapfrog-triejoin", "tree-trie"),
        ("leapfrog-triejoin", "column-trie"),
        ("hash-triejoin", "hash-trie"),
    ] {
        let output = run_join(
            &["edge.csv"],
            "reordered_head_query.dl",
```

and make it (the tuples are `(algorithm, indexstructure)` here):

```rust
        ("leapfrog-triejoin", "tree-trie"),
        ("leapfrog-triejoin", "column-trie"),
        ("hash-triejoin", "hash-trie"),
        ("binary-hash-join", "hash-index"),
    ] {
        let output = run_join(
            &["edge.csv"],
            "reordered_head_query.dl",
```

In `kermit/tests/cli_optimiser_choice.rs`, quote:

```rust
    for (structure, algorithm) in [
        ("tree-trie", "leapfrog-triejoin"),
        ("column-trie", "leapfrog-triejoin"),
        ("hash-trie", "hash-triejoin"),
    ] {
```

and make it (the tuples are `(structure, algorithm)` here):

```rust
    for (structure, algorithm) in [
        ("tree-trie", "leapfrog-triejoin"),
        ("column-trie", "leapfrog-triejoin"),
        ("hash-trie", "hash-triejoin"),
        ("hash-index", "binary-hash-join"),
    ] {
```

- [ ] **Step 2: Run them to see them fail**

Run: `nix develop --command cargo test -p kermit --test cli_hash_index_hasher_choice --test cli_query_errors --test cli_bench_run_sweep --test cli_bench_run_ds_flag_reach --test cli_join_tests --test cli_optimiser_choice`

Expected: PASS at once, because Task 4.1 already wired the cell; these tests pin behaviour rather than drive new code. To see that they can fail, apply one mutant and run the same command: in `DsFlag::structures` (`kermit/src/options.rs`) change `| Self::LayoutHasher => &[IndexStructure::HashTrie, IndexStructure::HashIndex],` to `&[IndexStructure::HashTrie]`. `cli_bench_ds_hash_index_with_fxhash_records_axis` then fails with `--ds-layout-hasher is only valid with --indexstructure hash-trie (or all); got --indexstructure HashIndex`, and `cli_bench_run_hash_index_verifies_the_triangle` fails the same way. Check that the failure appears (the mutant applied), then restore the line (`git checkout -- kermit/src/options.rs` is safe here, because Task 4.1 is already committed).

- [ ] **Step 3: Implement**

Nothing to implement: Task 4.1 added the cell. The tests above are the deliverable.

- [ ] **Step 4: Run them to see them pass**

Run: `nix develop --command cargo test -p kermit --test cli_hash_index_hasher_choice --test cli_query_errors --test cli_bench_run_sweep --test cli_bench_run_ds_flag_reach --test cli_join_tests --test cli_optimiser_choice`

Expected: PASS, including the two lists' fourth cell (`hash-index` with `binary-hash-join`: the header row `Z,X` and the three paths, and the same rows under every optimiser as on the other cells), `cli_bench_run_hash_index_verifies_the_triangle` (the triangle's four matches) and `mutual_edges_run_on_every_cell` and the placeholder rows on the two hash-index cells.

- [ ] **Step 5: Commit**

```bash
nix develop --command cargo fmt --all
git add -N kermit/tests/cli_hash_index_hasher_choice.rs
git commit -m "test(kermit): CLI coverage for the hash-index binary hash join cell" -- kermit/tests/cli_hash_index_hasher_choice.rs kermit/tests/cli_query_errors.rs kermit/tests/cli_bench_run_sweep.rs kermit/tests/cli_bench_run_ds_flag_reach.rs kermit/tests/cli_join_tests.rs kermit/tests/cli_optimiser_choice.rs
```

### Task 4.3: kermit-lab: the baseline map's comment and test

**Files:**
- Modify: `python/kermit-lab/kermit_lab/analysis.py:181-187`
- Modify: `python/kermit-lab/tests/test_analysis.py:175-181`
- Modify: `python/kermit-lab/kermit_lab/axis_mapping.py:26-32` (`DATA_STRUCTURE_COLOURS`)
- Modify: `python/kermit-lab/tests/test_axis_mapping.py:14`

`speedup_table` keeps only rows that carry a `ds_build_mode` (`df["ds_build_mode"].notna()`) before it reads `BASELINE_BUILD_MODES`, and a `HashIndex` report carries none, so the map stays at three entries. This task states that in the map's comment and pins it in the test. It also gives `HashIndex` its colour in `DATA_STRUCTURE_COLOURS`, which asks to be extended for every new `IndexStructure`.

- [ ] **Step 1: Write the failing tests**

In `python/kermit-lab/tests/test_axis_mapping.py`, quote:

```python
    assert "HashTrie" in DATA_STRUCTURE_COLOURS  # newly committed
```

and make it:

```python
    assert "HashTrie" in DATA_STRUCTURE_COLOURS  # newly committed
    assert "HashIndex" in DATA_STRUCTURE_COLOURS
    assert len(set(DATA_STRUCTURE_COLOURS.values())) == len(DATA_STRUCTURE_COLOURS)
```

In `python/kermit-lab/tests/test_analysis.py`, quote:

```python
def test_baseline_build_modes_cover_every_structure_kermit_reports() -> None:
    """The `data_structure` labels a report can carry, pinned on the Rust side
    by `IndexStructure::axis_value`."""
    assert set(BASELINE_BUILD_MODES) == {"TreeTrie", "ColumnTrie", "HashTrie"}
    assert baseline_build_mode("ColumnTrie") == "bulk"
```

and make it:

```python
def test_baseline_build_modes_cover_every_structure_kermit_reports() -> None:
    """Every structure with build modes, among the `data_structure` labels a
    report can carry (pinned on the Rust side by `IndexStructure::axis_value`).
    HashIndex has one build and its reports carry no `ds_build_mode`, so its
    rows pass through `speedup_table` without a baseline lookup."""
    assert set(BASELINE_BUILD_MODES) == {"TreeTrie", "ColumnTrie", "HashTrie"}
    assert baseline_build_mode("ColumnTrie") == "bulk"
    tree = _build_mode_rows({"serial": [100.0], "parallel:2": [50.0]})
    hash_index = _build_mode_rows({"serial": [70.0]}, data_structure="HashIndex")
    hash_index["ds_build_mode"] = pd.NA
    table = speedup_table(pd.concat([tree, hash_index], ignore_index=True))
    assert table["data_structure"].tolist() == ["TreeTrie"]
    assert table.iloc[0]["speedup"] == pytest.approx(2.0)
```

- [ ] **Step 2: Run them to see them fail or pass**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q tests/test_axis_mapping.py tests/test_analysis.py -k "committed_ds_colours or baseline_build_modes_cover"'`

Expected: `test_committed_ds_colours` FAILS with `assert 'HashIndex' in {'TreeTrie': '#0072B2', 'ColumnTrie': '#D55E00', 'HashTrie': '#009E73'}` (the colour is not there yet). `test_baseline_build_modes_cover_every_structure_kermit_reports` PASSES. The behaviour already holds (the rows are dropped before the map is read), so the test pins it rather than drives a change; it fails only if `speedup_table` stops filtering on `ds_build_mode` before the lookup (`ValueError: no default baseline build for data_structure 'HashIndex'`).

- [ ] **Step 3: Implement**

In `python/kermit-lab/kermit_lab/axis_mapping.py`, quote:

```python
    "HashTrie": WONG_PALETTE[3],    # bluish green
}
```

and make it (palette entries 5, 6 and 3 are taken; 1 is the orange none of the structures use):

```python
    "HashTrie": WONG_PALETTE[3],    # bluish green
    "HashIndex": WONG_PALETTE[1],   # orange
}
```

In `python/kermit-lab/kermit_lab/analysis.py`, quote:

```python
# Each structure's default single-threaded build: what `speedup_table` divides
# by when it is given no `baseline`. One entry per `data_structure` a report
# can carry (`IndexStructure::axis_value`), so a new structure must name its
# baseline here.
```

and make it:

```python
# Each structure's default single-threaded build: what `speedup_table` divides
# by when it is given no `baseline`. One entry per `data_structure` with build
# modes (`IndexStructure::axis_value`), so a new structure with build modes
# must name its baseline here. HashIndex has one build and its reports carry
# no `ds_build_mode`, so `speedup_table` drops its rows before it reads this
# map and never asks for it.
```

- [ ] **Step 4: Run them to see them pass**

Run: `nix develop --command bash -c 'cd python/kermit-lab && uv run --locked --group test pytest -q'`

Expected: PASS (the whole kermit-lab suite).

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(kermit-lab): colour HashIndex; the baseline map covers structures with build modes only" -- python/kermit-lab/kermit_lab/analysis.py python/kermit-lab/tests/test_analysis.py python/kermit-lab/kermit_lab/axis_mapping.py python/kermit-lab/tests/test_axis_mapping.py
```

## Story 5: The documentation sweep

- **Developer:** intermediate-dev
- **Files:** `ARCHITECTURE.md`, `CLAUDE.md`, `README.md`, `USAGE.md`, `kermit/README.md`, `kermit-ds/README.md`, `kermit-algos/README.md`, `kermit-iters/README.md`, `kermit/src/lib.rs`, `kermit-algos/src/sorted/mod.rs`, `docs/data-structures/hash-trie.md`, `docs/specs/bench-report-schema.md`
- **Interfaces:** none exposed; uses every name in the names contract, as text
- **Verify:** `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps && cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings'`
- **Review:** light (documentation only; no code changes)

Stories 1 to 4 added a third iterator family (`KeyedIterable`, `HashIndex<H>`, `BinaryHashJoin`), a third index-planning policy (`TrieLevels::ProbeKeys`, with `join_order.rs` beside `access` and `index_plan`) and a fourth `(structure, algorithm)` cell (`-i hash-index -a binary-hash-join`, `Execution::IndexBhj`). This story brings every prose doc that counts cells or families, or lists structures, algorithms, test aliases or report values, into line. It changes text only: no Python or YAML file is touched, and the only Rust files are two doc-comment edits (`kermit/src/lib.rs` and `kermit-algos/src/sorted/mod.rs`), with no code line changed. The two component pages (`docs/data-structures/hash-index.md`, `docs/algorithms/binary-hash-join.md`) were written by Stories 2 and 3. Each edit below quotes the exact current text it replaces (an anchor fragment inside a long line, never the whole line) and gives the replacement. None of the quoted or new text contains a landmine marker. The task order is by file, and each task leaves the doc build green, since docs-only edits cannot break it; Task 5.8 runs the story's Verify command once, at the end.

### Task 5.1: `ARCHITECTURE.md`

**Files:**
- Modify: `ARCHITECTURE.md:51`, `:70-75`, `:77-97`, `:244-247`, `:322-324`, `:326-344`, `:372`, `:458-474`, `:512-526`

- [ ] **Step 1: Confirm the stale text**

Run: `grep -n -E 'Two Iterator Families|Only three|Both algorithms stream|three valid cells|three incompatible|all three valid' ARCHITECTURE.md | cut -c1-60`

Expected: seven lines, at 51, 77, 97, 322, 466, 472 and 474.

- [ ] **Step 2: Apply the edits.** Each edit is made with the Edit tool; the old text occurs exactly once in the file.

**Edit 1.** The dependency note (line 51).

````text
each implementation narrows that bound to the iterator family it needs (`TrieIterable` or `HashTrieIterable`, see "Two Iterator Families" below)
````

with

````text
each implementation narrows that bound to the iterator family it needs (`TrieIterable`, `HashTrieIterable` or `KeyedIterable`, see "Three Iterator Families" below)
````

**Edit 2.** The trait tree (lines 73-75).

````text
     └── HashTrieIterable ─► HashTrieIterator
                               (hashed keys, exact-match `lookup`)
````

with

````text
     ├── HashTrieIterable ─► HashTrieIterator
     │                         (hashed keys, exact-match `lookup`)
     │
     └── KeyedIterable
                               (an index keyed on a column prefix: `scan`, exact-match `probe`)
````

**Edit 3.** The heading (line 77).

````text
#### Two Iterator Families
````

with

````text
#### Three Iterator Families
````

**Edit 4.** The first sentence under it (line 79).

````text
`TrieIterator` and `HashTrieIterator` are **parallel families, not a hierarchy**. They share only the empty `JoinIterable` marker.
````

with

````text
`TrieIterator`, `HashTrieIterator` and `KeyedIterable` are **parallel families, not a hierarchy**. They share only the empty `JoinIterable` marker.
````

**Edit 5.** The paragraph that introduces the layer table (line 83): add the keyed family's paragraph before it, and say "forks".

````text
The consequence is that the fork propagates up every layer of the stack, and each layer must be split by hand:
````

with

````text
`KeyedIterable` (`kermit-iters/src/keyed.rs`) is the third fork, added for the binary hash join baseline, and it is not a trie at all: an index keyed on its first `key_len()` columns, with no `open`/`up`, no per-level hash and no order. `scan` lends every row in source order, and `probe(key)` lends the rows whose first `key_len()` values equal `key`, in source order; neither allocates. The key length is the index's `RelationHeader::levels()`, which index planning sets under `TrieLevels::ProbeKeys` (see "Index planning" below).

The consequence is that each fork propagates up every layer of the stack, and each layer must be split by hand. The table shows the sorted and hash families; the keyed family follows it.
````

**Edit 6.** The valid pairs (line 97): add the keyed family's layers and the fourth pair.

````text
Only three `(index structure, algorithm)` pairs are valid: `(TreeTrie, LeapfrogTriejoin)`, `(ColumnTrie, LeapfrogTriejoin)`, and `(HashTrie, HashTriejoin)`.
````

with

````text
The keyed family has the same layers, with no iterator type of its own:

- **Structure** `HashIndex<H>`; **algorithm** `BinaryHashJoin`; **`Projectable`** hand-rolled on `HashIndex` (it rebuilds from `scan`, keyed on every projected column).
- **Engine** `binary_join_for_each` / `binary_join` free functions over a `Catalog`, building `HashIndex<H>` indexes per query, over the same `prepare` / `build` / `execute` lifecycle.
- **Bench cell** `Execution::IndexBhj { hasher }` / `IndexBhj<H>` (`JOIN_MUTATES = false`, since a probe changes nothing).
- **`bench run` dispatch** the same generic `run_benchmark<F>`; **`bench ds` dispatch** the same `run_ds_bench<F>`, over `HashIndexFamily<H>`.

Only four `(index structure, algorithm)` pairs are valid: `(TreeTrie, LeapfrogTriejoin)`, `(ColumnTrie, LeapfrogTriejoin)`, `(HashTrie, HashTriejoin)`, and `(HashIndex, BinaryHashJoin)`.
````

**Edit 7.** The new structure's section, before the parser section (line 247).

````text
### Query Representation (`kermit-parser`)
````

with

````text
#### HashIndex

A chained hash table keyed on the first `levels()` columns: the structure of the binary hash join baseline, Free Join §2.2's "hash table keyed on the attributes shared with the relations before it". It is generic over a `HashStrategy` (`SipHashStrategy` default, `FxHashStrategy`), reuses `--ds-layout-hasher`, and has no other Layout, Config or BuildMode axis. In outline:

```text
header:    RelationHeader
rows:      Tuples      // the view's rows, one copy, in source order
key_len:   usize       // header.levels()
directory: Vec<RowId>  // power-of-two bucket heads, NIL when empty
next:      Vec<RowId>  // per row, the next row of its bucket's chain
```

- **Sizing and hashing.** `slots = max(2, rows.next_power_of_two())`, so the load factor is at most 1. The key hash folds `H::hash` over the key columns with FxHash's combine step, and the bucket is the top bits of a Fibonacci multiply of that hash.
- **Construction.** `from_view` copies the view's matching rows once and links them from last to first, each prepended to its bucket's chain, so every chain is in source order. That is three allocations and none per row. The links are plain `Vec<RowId>`, not the HashTrie's `ChainLinks`.
- **Probe.** It hashes the key, walks the bucket's chain and lends each row whose first `key_len()` values equal the key, so rows of other keys that share the bucket are skipped by comparison. With `key_len() == 0` it lends every row.
- **`Relation`.** `TRIE_LEVELS` is `TrieLevels::ProbeKeys` and `KEEPS_DUPLICATES` is `true`, so each duplicate row is lent separately and an all-constant atom's multiplicity counts them, as on `HashTrie`. `bench ds` builds from a header whose `levels()` is the arity, so there the index is keyed on every column.

See `docs/data-structures/hash-index.md` for the invariants, the complexity of `from_view`, `probe` and `scan`, and a worked example.

### Query Representation (`kermit-parser`)
````

**Edit 8.** All three algorithms stream (line 322).

````text
Both algorithms stream. `JoinAlgo::join_for_each` passes each result tuple to a caller-supplied sink as a borrowed slice, so neither algorithm materialises its result:
````

with

````text
All three algorithms stream. `JoinAlgo::join_for_each` passes each result tuple to a caller-supplied sink as a borrowed slice, so none of them materialises its result:
````

**Edit 9.** The end of the same paragraph (line 322).

````text
and HashTriejoin's leaf cross product writes into scratch buffers allocated once per join.
````

with

````text
HashTriejoin's leaf cross product writes into scratch buffers allocated once per join, and `BinaryHashJoin`'s recursion writes into scratch rows allocated once per join.
````

**Edit 10.** The new algorithm's section, before the trait section (line 324).

````text
### JoinAlgo Trait
````

with

````text
### Binary Hash Join

`BinaryHashJoin` is the left-deep pipelined hash join of Free Join §2.2 (Wang et al., SIGMOD 2023), over the `KeyedIterable` family, and it is the in-kermit baseline for what the worst-case optimal joins buy. The executed query's first atom (`#0`, the driver) is scanned and every later atom is probed on the variables the atoms before it bound; each match binds the atom's other columns and descends. Nothing intermediate is stored. The executor reads the executed body left to right and derives nothing, because index planning under `TrieLevels::ProbeKeys` has already put the body in join order and given every index its probe key (see "Index planning" below):

```text
descend(j):
    if j = steps: emit(row); return
    key_j ← row[vars_j[..k_j]]
    for t in index_j.probe(key_j):
        row[vars_j[i]] ← t[i]   for i in k_j..arity_j
        descend(j + 1)
```

Its invariant is "probe keys lead the columns": for each step `j`, the first `k_j = index_j.key_len()` variables are bound by the steps before `j` and the rest are not. It is asserted at the start of every join, in the role LFTJ's ascent assertion plays, and it catches a query that was not planned under `ProbeKeys`. The join does work proportional to the sum of its intermediate results (on the triangle, `|R ⋈ S|`), which is the gap to the AGM bound that the worst-case optimal joins close. See `docs/algorithms/binary-hash-join.md`.

### JoinAlgo Trait
````

**Edit 11.** The family narrowing in the trait section (lines 341-342).

````text
impl<DS: TrieIterable>     JoinAlgo<DS> for LeapfrogTriejoin { … }
impl<DS: HashTrieIterable> JoinAlgo<DS> for HashTriejoin     { … }
````

with

````text
impl<DS: TrieIterable>     JoinAlgo<DS> for LeapfrogTriejoin { … }
impl<DS: HashTrieIterable> JoinAlgo<DS> for HashTriejoin     { … }
impl<DS: KeyedIterable>    JoinAlgo<DS> for BinaryHashJoin   { … }
````

**Edit 12.** The planning policy, at the end of "Index planning" (line 372).

````text
writing the atom `edge(A, D)` avoids it.
````

with

````text
writing the atom `edge(A, D)` avoids it.

The step runs under one of three policies, `TrieLevels`, which the structure declares (`Relation::TRIE_LEVELS`): `All` (a level per column), `JoinKeys` (levels up to the last join key, #116) and `ProbeKeys`. Under `ProbeKeys` (`HashIndex`) the step first calls `join_order` (`kermit-algos/src/join_order.rs`, a private module beside `access` and `index_plan`), which reads the plan's variable order left to right as a sequence of binding events: each unbound variable joins the atom that binds it (the one with the most variables already bound, ties to the earliest in the body), and any atom whose variables are then all bound is probed at once. The result is a left-deep join order and a reordered plan V′ in which every atom's key variables (those bound by the atoms before it) precede its new ones. `plan_indexes` then sorts each access's columns by V′, sets each spec's `levels` to that access's key length (`0` for the driver, and for an atom joined across a cross product), visits the accesses in join order, so spec ids and the executed body follow it with `#0` the driver, and returns V′ as `IndexPlan.plan`, which is also what the report's `plan` axis shows. A spec's `levels` is part of the merge comparison, so two reads of one relation with different key lengths are two indexes. `All` and `JoinKeys` plan exactly as before, and no optimiser, `QueryPlan` or existing structure changes.
````

**Edit 13.** The conveniences paragraph (line 460).

````text
The conveniences `lftj_join_for_each<R, JA>` and `hash_join_for_each<R>` (hardwired to `HashTriejoin`) run all three steps and stream; `lftj_join` / `hash_join` run the same steps
````

with

````text
The conveniences `lftj_join_for_each<R, JA>`, `hash_join_for_each<R>` (hardwired to `HashTriejoin`) and `binary_join_for_each<R>` (hardwired to `BinaryHashJoin`, for a `KeyedIterable` structure) run all three steps and stream; `lftj_join` / `hash_join` / `binary_join` run the same steps
````

**Edit 14.** The cell count in "Catalog and lifecycle" (line 466).

````text
all three valid cells, including `(HashTrie, HashTriejoin)`, are reachable from every command.
````

with

````text
all four valid cells, including `(HashTrie, HashTriejoin)` and `(HashIndex, BinaryHashJoin)`, are reachable from every command.
````

**Edit 15.** The `Execution` variants in "Selector dispatch" (line 472).

````text
) and `HashHtj { hasher, pruning, expansion, config, build }` — so an `Execution`
````

with

````text
), `HashHtj { hasher, pruning, expansion, config, build }` and `IndexBhj { hasher }` (`hasher` carries `--ds-layout-hasher`) — so an `Execution`
````

**Edit 16.** The incompatible pairs (line 472).

````text
returns `None` for the three incompatible pairs
````

with

````text
returns `None` for the eight incompatible pairs
````

**Edit 17.** The sweep (line 474).

````text
Consequently `-i all -a all` runs exactly the three valid cells
````

with

````text
Consequently `-i all -a all` runs exactly the four valid cells
````

**Edit 18.** The structure-only markers (line 476).

````text
`SortedTrieFamily<R>` / `HashTrieFamily<H, P, E>`, which implement the supertrait alone
````

with

````text
`SortedTrieFamily<R>` / `HashTrieFamily<H, P, E>` / `HashIndexFamily<H>`, which implement the supertrait alone
````

**Edit 19.** The join families that embed them (line 476).

````text
The join families `TrieLftj<R>` / `HashHtj<H, P, E>` embed those markers
````

with

````text
The join families `TrieLftj<R>` / `HashHtj<H, P, E>` / `IndexBhj<H>` embed those markers
````

**Edit 20.** The data-structure recipe, step 3 (line 516).

````text
`hash_trie_test_suite!` covers the hash family, `relation_trie_test_suite!` the sorted one.
````

with

````text
`hash_trie_test_suite!` covers the hash family, `relation_trie_test_suite!` the sorted one, and `keyed_index_test_suite!` the keyed family (whose structure implements `KeyedIterable` alone: `scan` and `probe`, no iterator type).
````

**Edit 21.** The algorithm recipe, step 1 (line 524).

````text
(`sorted/` or `hash/`, matching the iterator family) and implement `JoinAlgo<DS>`, narrowing `DS` to `TrieIterable` or `HashTrieIterable` accordingly.
````

with

````text
(`sorted/`, `hash/` or `keyed/`, matching the iterator family) and implement `JoinAlgo<DS>`, narrowing `DS` to `TrieIterable`, `HashTrieIterable` or `KeyedIterable` accordingly.
````

- [ ] **Step 3: Check that the stale text is gone**

Run: `grep -n -E 'Two Iterator Families|Only three|Both algorithms stream|three valid cells|three incompatible|all three valid' ARCHITECTURE.md`

Expected: no output.

Run: `grep -c -E 'KeyedIterable|HashIndex|BinaryHashJoin|ProbeKeys' ARCHITECTURE.md`

Expected: a count of 30 or more.

- [ ] **Step 4: Commit**

```bash
git commit -m "docs(architecture): the keyed iterator family, the binary hash join and ProbeKeys planning" -- ARCHITECTURE.md
```

### Task 5.2: `CLAUDE.md`

**Files:**
- Modify: `CLAUDE.md:13`, `:51`, `:85-105`, `:147-149`, `:170-171`, `:190`, `:193-194`, `:197`, `:196`, `:221`, `:314-316`, `:344-345`, `:353`, `:356`

This file is the repo's rules. Edit only the fragments below, and leave line 172 (the `Tuples` / `ChainLinks` entry) untouched. Every old fragment occurs exactly once.

- [ ] **Step 1: Confirm the stale text**

Run: `grep -n -E 'the 3 valid|exactly 3 valid|exactly the 3 valid|the three valid|Two iterator families|The two iterator families|and the two never reference|prepares at most two' CLAUDE.md | cut -c1-70`

Expected: seven lines, at 51, 170, 196, 221, 344, 345 and 356.

- [ ] **Step 2: Apply the edits.**

**Edit 1.** Priorities item 1, the Layout aliases (line 13).

````text
each through the 16 patterns under LFTJ and both optimisers.
````

with

````text
each through the 16 patterns under LFTJ and both optimisers. For `HashIndex` it is the hasher: `HashIndexSip` and `HashIndexFx`, each through the 16 patterns under `BinaryHashJoin` and the three optimisers.
````

**Edit 2.** A build command (line 51). Insert the new line before the sweep line.

````text
cargo run -- bench run --all -i all -a all
````

with

````text
cargo run -- bench run triangle -i hash-index -a binary-hash-join  # The binary hash join baseline: a left-deep pipelined join over a keyed hash index (pairs only with binary-hash-join; --ds-layout-hasher is its only --ds-* flag)
cargo run -- bench run --all -i all -a all
````

**Edit 3.** The cell count on the sweep line (line 51).

````text
× the 3 valid (structure, algorithm) cells;
````

with

````text
× the 4 valid (structure, algorithm) cells;
````

**Edit 4.** `kermit-iters` (line 85).

````text
kermit-iters    → Core iterator traits (LinearIterator, TrieIterator), the tuple batch
````

with

````text
kermit-iters    → Core iterator traits (LinearIterator, TrieIterator, KeyedIterable), the tuple batch
````

**Edit 5.** `kermit-ds` (lines 92-93).

````text
                  ExpansionPolicy and a LevelPolicy). TreeTrie/ColumnTrie implement Relation + TrieIterable;
                  HashTrie implements Relation + HashTrieIterable. TreeTrie and ColumnTrie
````

with

````text
                  ExpansionPolicy and a LevelPolicy), HashIndex (a chained hash table keyed on
                  its leading columns, generic over a HashStrategy; the binary hash join's
                  structure). TreeTrie/ColumnTrie implement Relation + TrieIterable;
                  HashTrie implements Relation + HashTrieIterable; HashIndex implements
                  Relation + KeyedIterable. TreeTrie and ColumnTrie
````

**Edit 6.** `kermit-algos` (lines 98-99).

````text
                  HashTriejoin (hash-based multi-way). Generic over data structures via the
````

with

````text
                  HashTriejoin (hash-based multi-way), BinaryHashJoin (left-deep pipelined
                  binary hash join over a KeyedIterable, in `keyed/`). Generic over data structures via the
````

**Edit 7.** `kermit-algos`, the steps around planning (lines 102-105).

````text
                  JoinAlgo::join_for_each executes, held by a Planner. It also hosts the two
                  steps around planning: `access`, which rewrites a query into one Access per
                  atom (relation, Filter, kept columns), and `index_plan`, which turns the
                  plan into the IndexSpecs (relation, Filter, column order) a query builds.
````

with

````text
                  JoinAlgo::join_for_each executes, held by a Planner. It also hosts the
                  steps around planning: `access`, which rewrites a query into one Access per
                  atom (relation, Filter, kept columns), `index_plan`, which turns the
                  plan into the IndexSpecs (relation, Filter, column order) a query builds,
                  and `join_order` (private), which `index_plan` calls under
                  TrieLevels::ProbeKeys to turn the variable order into a left-deep join order.
````

**Edit 8.** `kermit`, the join entry points (lines 147-149).

````text
                  Provides the join entry points: db::lftj_join_for_each /
                  db::hash_join_for_each stream each tuple to a sink, and
                  db::lftj_join / db::hash_join collect the answer as a relation (the
````

with

````text
                  Provides the join entry points: db::lftj_join_for_each /
                  db::hash_join_for_each / db::binary_join_for_each stream each tuple to a
                  sink, and db::lftj_join / db::hash_join / db::binary_join collect the answer as a relation (the
````

**Edit 9.** The fork paragraph in the trait hierarchy (line 170): the entry points and the pair count.

````text
separate join entry point (`lftj_join` vs `hash_join`, both over the one `prepare` / `build` / `execute` lifecycle), separate test suites — and leaves exactly 3 valid `(structure, algorithm)` pairs.
````

with

````text
separate join entry point (`lftj_join` vs `hash_join`, both over the one `prepare` / `build` / `execute` lifecycle), separate test suites — and, with the keyed family below, leaves exactly 4 valid `(structure, algorithm)` pairs.
````

**Edit 10.** The keyed family's entry, inserted before the `Relation` entry (line 171).

````text
- **Relation**: JoinIterable + Projectable — core data abstraction
````

with

````text
- **JoinIterable** (marker) → **KeyedIterable** (`key_len`, `scan`, exact-match `probe`; `kermit-iters/src/keyed.rs`) — a third **parallel family**: an index keyed on its first `key_len()` columns (`HashIndex<H>`), and the left-deep binary hash join (`BinaryHashJoin`, `kermit-algos/src/keyed/`) that scans the first index and probes the rest. It has no `open` / `up`, no per-level hash and no order: `probe(key)` lends the rows whose first `key_len()` values equal `key`, in source order, `scan` lends every row, and neither allocates. `key_len()` is the header's `levels()`, which `prepare` sets under `TrieLevels::ProbeKeys`. The fork has its own join entry points (`binary_join_for_each` / `binary_join`, over the same lifecycle), its own suite macro (`keyed_index_test_suite!`) and its own cell (`Execution::IndexBhj`).
- **Relation**: JoinIterable + Projectable — core data abstraction
````

**Edit 11.** `KEEPS_DUPLICATES` (line 171, the `Relation` entry).

````text
(default `false`; `true` for `HashTrie`; forwarded by `Configured` and `BuiltWith`)
````

with

````text
(default `false`; `true` for `HashTrie` and `HashIndex`; forwarded by `Configured` and `BuiltWith`)
````

**Edit 12.** `TRIE_LEVELS` (line 171).

````text
(`kermit_iters::TrieLevels`: `All` by default, `JoinKeys` for `HashTrie<…, KeyLevels>`; forwarded by both wrappers)
````

with

````text
(`kermit_iters::TrieLevels`: `All` by default, `JoinKeys` for `HashTrie<…, KeyLevels>`, `ProbeKeys` for `HashIndex`; forwarded by both wrappers)
````

**Edit 13.** The end of the `Relation` entry (line 171).

````text
and `build` refuses a structure whose policy differs from the prepared one's.
````

with

````text
and `build` refuses a structure whose policy differs from the prepared one's. Under `ProbeKeys` the level count is the probe key's length, and `plan_indexes` also lists the executed body in join order (`#0` the driver) and returns the reordered plan V′ (`kermit-algos/src/join_order.rs`; see the index-planning gotcha).
````

**Edit 14.** The suite's entry points (line 190).

````text
(`lftj_join` / `hash_join` via `kermit/tests/common/utils.rs::JoinEntry`
````

with

````text
(`lftj_join` / `hash_join` / `binary_join` via `kermit/tests/common/utils.rs::JoinEntry`
````

**Edit 15.** The all-orders cells (line 193).

````text
and a `KeyLevels` HashTrie, #116, whose odd orders exercise rule (iii) and the skipped positions)
````

with

````text
a `KeyLevels` HashTrie, #116, whose odd orders exercise rule (iii) and the skipped positions, and `HashIndex` under `BinaryHashJoin`)
````

**Edit 16.** The `kermit::db` checks (line 194).

````text
multiplies results on `HashTrie` and not on the sorted tries, and that a zero count yields no rows
````

with

````text
multiplies results on `HashTrie` and `HashIndex` and not on the sorted tries, that a zero count yields no rows, and that a query prepared under `ProbeKeys` panics when a structure of another policy builds it
````

**Edit 17.** The keyed suites, inserted before the last Testing Patterns bullet (line 197).

````text
- Uses `paste!` crate for macro hygiene
````

with

````text
- `keyed_index_test_suite!(Type)` / `parquet_test_suite!(Type, collector)` (`kermit-ds/tests/common/macros.rs`) — the same layer for `KeyedIterable` structures: `probe` lends exactly a key's rows in source order, `key_len() == 0` lends every row, an empty index, insert and rehash, duplicates kept; the Parquet arm's collector is a `scan`. It is invoked per hasher alias in `kermit-ds/tests/hash_index_tests.rs` (`HashIndexSip`, `HashIndexFx` and a test-local one-bucket strategy, so the key comparison alone must filter). The join suite uses the same aliases, each defined in the test file that uses it (`type HashIndexSip = HashIndex<SipHashStrategy>;`, `type HashIndexFx = HashIndex<FxHashStrategy>;`): `define_multiway_join_test_suite!` runs them under `BinaryHashJoin` and the three optimisers in `kermit/tests/join_tests.rs`, `JoinEntry` has a `BinaryHashJoin` impl over `binary_join_for_each` / `binary_join`, and `all_orders.rs`, `result_allocation.rs`, `tuple_allocation.rs`, `lubm_mini_oracle.rs` and `lubm_cardinalities.rs` each have a `HashIndex` cell. The keyed family is a separate trait, so neither the sorted-trie nor the hash-trie suites apply to it.
- Uses `paste!` crate for macro hygiene
````

**Edit 18.** The algorithm recipe, step 1 (line 221).

````text
`hash/` everything traversing `HashTrieIterable`, and the two never reference each other.
````

with

````text
`hash/` everything traversing `HashTrieIterable`, `keyed/` everything traversing `KeyedIterable`, and the families never reference each other.
````

**Edit 19.** The same step's precedents (line 221).

````text
and `kermit-algos/src/hash/hash_triejoin.rs`.
````

with

````text
`kermit-algos/src/hash/hash_triejoin.rs`, and `kermit-algos/src/keyed/binary_hash_join.rs`.
````

**Edit 20.** The algorithm page, in Component Reference Docs (line 315).

````text
- `docs/algorithms/hash-triejoin.md` — hash-trie-based multi-way join (`-a hash-triejoin`).
````

with

````text
- `docs/algorithms/hash-triejoin.md` — hash-trie-based multi-way join (`-a hash-triejoin`).
- `docs/algorithms/binary-hash-join.md` — the left-deep pipelined binary hash join, the baseline for what worst-case optimal joins buy (`-a binary-hash-join`).
````

**Edit 21.** The structure page (line 318).

````text
- `docs/data-structures/hash-trie.md` — hash-based trie (`-i hash-trie`).
````

with

````text
- `docs/data-structures/hash-trie.md` — hash-based trie (`-i hash-trie`).
- `docs/data-structures/hash-index.md` — chained hash index keyed on its leading columns (`-i hash-index`).
````

**Edit 22.** The `Execution` gotcha: the cell count (line 344).

````text
an enum whose variants *are* the three valid (structure, algorithm) cells
````

with

````text
an enum whose variants *are* the four valid (structure, algorithm) cells
````

**Edit 23.** The same gotcha: the fourth variant (line 344).

````text
and `HashHtj { hasher, pruning, expansion, config, build }`). `Sweep::expand`
````

with

````text
`HashHtj { hasher, pruning, expansion, config, build }` and `IndexBhj { hasher }`). `Sweep::expand`
````

**Edit 24.** The same gotcha: the sweep (line 344).

````text
`-i all -a all` runs exactly the 3 valid cells (skips announced on stderr)
````

with

````text
`-i all -a all` runs exactly the 4 valid cells (skips announced on stderr)
````

**Edit 25.** The same gotcha: the families (line 344).

````text
serves both trait families and `bench run`
````

with

````text
serves all three trait families and `bench run`
````

**Edit 26.** The same gotcha: the structure-only markers (line 344).

````text
`SortedTrieFamily<R>` / `HashTrieFamily<H, P, E>`, which cannot join
````

with

````text
`SortedTrieFamily<R>` / `HashTrieFamily<H, P, E>` / `HashIndexFamily<H>`, which cannot join
````

**Edit 27.** The test-macro gotcha (line 345): the title.

````text
- **Two iterator families, two test-macro families**:
````

with

````text
- **Three iterator families, three test-macro families**:
````

**Edit 28.** The same gotcha: the end.

````text
not extending the sorted-trie macros.
````

with

````text
not extending the sorted-trie macros. A keyed-family structure takes `keyed_index_test_suite!(Type)` instead (`kermit-ds/tests/hash_index_tests.rs` covers `HashIndexSip` / `HashIndexFx`), bound on `KeyedIterable` in the same way.
````

**Edit 29.** The index-planning gotcha (line 353): the `ProbeKeys` paragraph, at its end.

````text
since the parser admits only `[A-Za-z][A-Za-z0-9_]*`.
````

with

````text
since the parser admits only `[A-Za-z][A-Za-z0-9_]*`. Under `TrieLevels::ProbeKeys` (`HashIndex`) the step also reorders: `kermit-algos/src/join_order.rs` (private, beside `access` and `index_plan`) turns the plan's variable order into a left-deep join order and a reordered plan V′ in which each atom's key variables (those bound by the atoms before it) precede its new ones. The columns sort by V′, each spec's `levels` is its key length (`0` for the driver and for an atom joined across a cross product), the specs and the executed body follow the join order (`#0` is the driver), and `IndexPlan.plan`, which the report's `plan` axis shows, is V′. `BinaryHashJoin` reads the body left to right and derives nothing; it asserts that the probe keys lead the columns (its panic message starts with `probe keys lead the columns`). `All` and `JoinKeys` plan as before.
````

**Edit 30.** The test-layer bullet for the hash family (line 196).

````text
The two iterator families are deliberately separate traits
````

with

````text
The three iterator families are deliberately separate traits
````

**Edit 31.** The "what is timed" gotcha: the policies a sweep prepares (line 356).

````text
`bench run` prepares at most two: `All` for the sorted cells and the HashTrie cell's own,
````

with

````text
`bench run` prepares at most three: `All` for the sorted cells, the HashTrie cell's own (`All` or `JoinKeys`) and `ProbeKeys` for the HashIndex cell,
````

- [ ] **Step 3: Check**

Run: `grep -n -E 'the 3 valid|exactly 3 valid|exactly the 3 valid|the three valid|Two iterator families|The two iterator families|and the two never reference|prepares at most two' CLAUDE.md`

Expected: no output.

Run: `git diff --stat -- CLAUDE.md`

Expected: `CLAUDE.md` only, with insertions and deletions in the tens of lines (no unrelated line changed; line 172 is untouched).

- [ ] **Step 4: Commit**

```bash
git commit -m "docs(claude): four valid cells, the keyed family, ProbeKeys planning and the HashIndex test aliases" -- CLAUDE.md
```

### Task 5.3: `README.md` and `USAGE.md`

**Files:**
- Modify: `README.md:54`, `:72-74`, `:81`, `:137`
- Modify: `USAGE.md:83-90`, `:341-352`, `:661-662`

- [ ] **Step 1: Confirm the stale text**

Run: `grep -n -E 'three pairings|three compatible|accepts all three valid|three valid \(structure|all three index structures|hash-trie is omitted' README.md USAGE.md | cut -c1-70`

Expected: six lines (README.md 72 and 81, USAGE.md 89, 341, 345 and 661).

- [ ] **Step 2: Apply the edits.**

**Edit 1.** `README.md`, `kermit join` (line 54).

````text
and `hash-trie` with `hash-triejoin` (optionally `--ds-layout-hasher fxhash`); an incompatible pair is a usage error.
````

with

````text
`hash-trie` with `hash-triejoin` (optionally `--ds-layout-hasher fxhash`), and `hash-index` with `binary-hash-join` (a left-deep pipelined binary hash join, the baseline for what worst-case optimal joins buy; `--ds-layout-hasher` is its only `--ds-*` flag); an incompatible pair is a usage error.
````

The words before the quoted fragment, "`kermit join` accepts every valid cell: `tree-trie` / `column-trie` with `leapfrog-triejoin`, ", stay, so the sentence reads as a list of three cells.

**Edit 2.** `README.md`, the sweep comment (lines 72-74).

````text
# Only three pairings are valid — (tree-trie|column-trie, leapfrog-triejoin)
# and (hash-trie, hash-triejoin) — and `all` on either side skips the
# incompatible ones, announcing each skip on stderr.
````

with

````text
# Only four pairings are valid — (tree-trie|column-trie, leapfrog-triejoin),
# (hash-trie, hash-triejoin) and (hash-index, binary-hash-join) — and `all`
# on either side skips the incompatible ones, announcing each skip on stderr.
````

**Edit 3.** `README.md`, the flag list (line 81).

````text
Only the three compatible pairings above are supported;
````

with

````text
Only the four compatible pairings above are supported;
````

**Edit 4.** `README.md`, `bench join` (line 137).

````text
and `hash-trie` via `hash-triejoin`. Supported metrics
````

with

````text
`hash-trie` via `hash-triejoin`, and `hash-index` via `binary-hash-join`. Supported metrics
````

The words before it, "Supported cells (for `bench join`): `tree-trie` / `column-trie` via `leapfrog-triejoin`, ", stay.

**Edit 5.** `USAGE.md`, "Pick the index structure" (lines 83-90).

````text
`--indexstructure` accepts `tree-trie` (pointer-based), `column-trie`
(column-oriented), or `hash-trie` (hash-based). The two sorted tries implement
the same `Relation` + `TrieIterable` traits, pair with `-a leapfrog-triejoin`,
and are interchangeable from the CLI's perspective; benchmark to pick one.
`hash-trie` lives in a different trait family (`HashTrieIterable`) and pairs
only with `-a hash-triejoin`. Every command, including the one-shot
`kermit join` / `bench join`, accepts all three valid pairs; an incompatible
pair is rejected as a usage error.
````

with

````text
`--indexstructure` accepts `tree-trie` (pointer-based), `column-trie`
(column-oriented), `hash-trie` (hash-based) or `hash-index` (a chained hash
table keyed on its leading columns). The two sorted tries implement
the same `Relation` + `TrieIterable` traits, pair with `-a leapfrog-triejoin`,
and are interchangeable from the CLI's perspective; benchmark to pick one.
`hash-trie` lives in a different trait family (`HashTrieIterable`) and pairs
only with `-a hash-triejoin`. `hash-index` lives in a third (`KeyedIterable`)
and pairs only with `-a binary-hash-join`, a left-deep pipelined binary hash
join: the baseline for what the worst-case optimal joins buy. Every command,
including the one-shot `kermit join` / `bench join`, accepts all four valid
pairs; an incompatible pair is rejected as a usage error.

`--ds-layout-hasher sip|fxhash` picks the hash function of `hash-trie` and of
`hash-index`, so a comparison between the two can hold the hasher fixed. It is
the only `--ds-*` flag `hash-index` takes: every other one is refused, and,
as above, under `-i all -a all` each flag reaches the cells that have its
axis. `bench ds` has no query, so there a `hash-index` is keyed on every column.
````

**Edit 6.** `USAGE.md`, the `-i all` expansion (lines 341-346).

````text
`-i all` expands to all three index structures (`column-trie`, `hash-trie`,
`tree-trie`) and `-a all` expands to both concrete algorithms (`hash-triejoin`,
`leapfrog-triejoin`); the selectors are wired up so adding a new `IndexStructure`
or `JoinAlgorithm` variant automatically joins the sweep. The cross product is
filtered to the three valid (structure, algorithm) cells, and each skipped pair
is announced on stderr, so `-i all -a all` runs exactly:
````

with

````text
`-i all` expands to all four index structures (`column-trie`, `hash-index`,
`hash-trie`, `tree-trie`) and `-a all` expands to all three concrete algorithms
(`binary-hash-join`, `hash-triejoin`, `leapfrog-triejoin`); the selectors are
wired up so adding a new `IndexStructure` or `JoinAlgorithm` variant
automatically joins the sweep. The cross product is
filtered to the four valid (structure, algorithm) cells, and each skipped pair
is announced on stderr, so `-i all -a all` runs exactly:
````

**Edit 7.** `USAGE.md`, the cells that sweep runs (lines 350-352).

````text
kermit bench run triangle -i column-trie -a leapfrog-triejoin
kermit bench run triangle -i hash-trie -a hash-triejoin
````

with

````text
kermit bench run triangle -i column-trie -a leapfrog-triejoin
kermit bench run triangle -i hash-trie -a hash-triejoin
kermit bench run triangle -i hash-index -a binary-hash-join
````

**Edit 8.** `USAGE.md`, the Oxford example's comment (lines 661-662).

````text
#    implementations (tree-trie and column-trie); hash-trie is omitted because
#    it requires hash-triejoin rather than leapfrog-triejoin.
````

with

````text
#    implementations (tree-trie and column-trie); hash-trie and hash-index are
#    omitted because each requires its own algorithm (hash-triejoin and
#    binary-hash-join) rather than leapfrog-triejoin.
````

- [ ] **Step 3: Check**

Run: `grep -n -E 'three pairings|three compatible|accepts all three valid|three valid \(structure|all three index structures|hash-trie is omitted' README.md USAGE.md`

Expected: no output.

Run: `grep -c 'hash-index' README.md USAGE.md`

Expected: `README.md:3` and `USAGE.md:` a count of 6 or more.

- [ ] **Step 4: Commit**

```bash
git commit -m "docs(readme,usage): the hash-index and binary-hash-join cell and the hasher flag's reach" -- README.md USAGE.md
```

### Task 5.4: `kermit/README.md` and `kermit-ds/README.md`

**Files:**
- Modify: `kermit/README.md:14`, `:31`
- Modify: `kermit-ds/README.md:3`, `:7`, `:9`, `:15`, `:31`

- [ ] **Step 1: Confirm the text to extend**

Run: `grep -n -E 'three trie-based|hash_join`\]|Writes result tuples' kermit/README.md kermit-ds/README.md | cut -c1-80`

Expected: three lines.

- [ ] **Step 2: Apply the edits.**

**Edit 1.** `kermit/README.md`, the library surface (line 14): add the entry point after the hash-family one.

````text
[`db::hash_join`](src/db.rs) collects it the same way.
````

with

````text
[`db::hash_join`](src/db.rs) collects it the same way.
- [`db::binary_join_for_each`](src/db.rs) — keyed-family join over a `Catalog` under `BinaryHashJoin`, for a `KeyedIterable` structure such as `HashIndex`, streaming the same way; the query is prepared under `TrieLevels::ProbeKeys`, which puts the executed body in join order. [`db::binary_join`](src/db.rs) collects it the same way.
````

**Edit 2.** `kermit/README.md`, the CLI (line 31).

````text
Writes result tuples as CSV to stdout (or `--output` if given).
````

with

````text
Writes result tuples as CSV to stdout (or `--output` if given). Every valid cell runs from `kermit join`: `tree-trie` / `column-trie` with `leapfrog-triejoin`, `hash-trie` with `hash-triejoin`, and `hash-index` with `binary-hash-join`.
````

**Edit 3.** `kermit-ds/README.md`, the opening line (line 3).

````text
Provides three trie-based implementations that store tuples of `usize` keys:
````

with

````text
Provides four implementations that store tuples of `usize` keys, three tries and a hash index:
````

**Edit 4.** `kermit-ds/README.md`, the list (line 7).

````text
- [`HashTrie`](src/ds/hash_trie/implementation.rs) — a hash-based trie storing the relation as nested hash tables, generic over a `HashStrategy` (default `SipHashStrategy`).
````

with

````text
- [`HashTrie`](src/ds/hash_trie/implementation.rs) — a hash-based trie storing the relation as nested hash tables, generic over a `HashStrategy` (default `SipHashStrategy`).
- [`HashIndex`](src/ds/hash_index/implementation.rs) — a chained hash table keyed on the first `levels()` columns of the relation, the structure of the binary hash join baseline; it stores the rows once, beside a power-of-two directory of bucket heads and one link per row. Generic over a `HashStrategy` (default `SipHashStrategy`).
````

**Edit 5.** `kermit-ds/README.md`, the trait paragraph (line 9).

````text
so it is driven by `HashTriejoin` rather than the LFTJ path.
````

with

````text
so it is driven by `HashTriejoin` rather than the LFTJ path. `HashIndex` implements `Relation` and [`KeyedIterable`](../kermit-iters/src/keyed.rs) (a third, parallel family: `scan` and an exact-match `probe` on a key prefix, with no trie levels), so it is driven by `BinaryHashJoin`.
````

**Edit 6.** `kermit-ds/README.md`, the surface list (line 15).

````text
- **Data structures** — `TreeTrie`, `ColumnTrie`, `HashTrie`, plus the `IndexStructure` CLI enum.
````

with

````text
- **Data structures** — `TreeTrie`, `ColumnTrie`, `HashTrie`, `HashIndex`, plus the `IndexStructure` CLI enum.
````

**Edit 7.** `kermit-ds/README.md`, the recipe (line 31).

````text
or `HashTrieIterable` for a hash-based trie (see `HashTrie`).
````

with

````text
`HashTrieIterable` for a hash-based trie (see `HashTrie`), or `KeyedIterable` for an index keyed on its leading columns (see `HashIndex`).
````

The words before it, "Also implement an iterator trait: `TrieIterable` for a hierarchical (LFTJ-style) trie, ", stay, so the sentence lists three traits.

- [ ] **Step 3: Check**

Run: `grep -c -E 'HashIndex|KeyedIterable|binary-hash-join' kermit/README.md kermit-ds/README.md`

Expected: `kermit/README.md:2` and `kermit-ds/README.md:4`.

- [ ] **Step 4: Commit**

```bash
git commit -m "docs(kermit,kermit-ds): the HashIndex structure and the binary join entry points" -- kermit/README.md kermit-ds/README.md
```

### Task 5.5: `kermit-algos/README.md` and `kermit-iters/README.md`

**Files:**
- Modify: `kermit-algos/README.md:3`, `:7`, `:9-22`, `:31`, `:33`, `:40`
- Modify: `kermit-iters/README.md:11`, `:16`

`BinaryHashJoin` is the binary baseline, not a worst-case-optimal join; the intro keeps the count of worst-case-optimal joins at two.

- [ ] **Step 1: Confirm the stale text**

Run: `grep -n -E 'two worst-case-optimal|Two Iterator Families|neither algorithm|any .HashTrieIterable. \(in practice|backs the .HashTriejoin|TrieIterable., .HashTrieIterable.\)' kermit-algos/README.md kermit-iters/README.md | cut -c1-70`

Expected: lines 3, 7, 31 and 40 of `kermit-algos/README.md`, and lines 11 and 16 of `kermit-iters/README.md`.

- [ ] **Step 2: Apply the edits.** Each old fragment occurs exactly once in its file.

**Edit 1.** `kermit-algos/README.md`, the intro (line 3).

````text
and the Hash Trie Join (SIGMOD 2020), over any implementing [`HashTrieIterable`](../kermit-iters/src/hash_trie.rs).
````

with

````text
and the Hash Trie Join (SIGMOD 2020), over any implementing [`HashTrieIterable`](../kermit-iters/src/hash_trie.rs). It also holds the binary baseline they are measured against, a left-deep pipelined hash join (Free Join, section 2.2), over any data structure implementing [`KeyedIterable`](../kermit-iters/src/keyed.rs).
````

**Edit 2.** `kermit-algos/README.md`, the layout paragraph (line 7).

````text
The sorted and hash families share no code and never reference each other
````

with

````text
The sorted, hash and keyed families share no code and never reference one another
````

**Edit 3.** The same line: the heading it cites.

````text
§ "Two Iterator Families".
````

with

````text
§ "Three Iterator Families".
````

**Edit 4.** The source tree (lines 18-19): `join_order.rs`, after `index_plan.rs`.

````text
  index_plan.rs     turns a plan into the indexes a query builds (relation, filter,
                    column order)
````

with

````text
  index_plan.rs     turns a plan into the indexes a query builds (relation, filter,
                    column order)
  join_order.rs     turns a variable order into a left-deep join order (private;
                    index_plan.rs uses it under TrieLevels::ProbeKeys)
````

**Edit 5.** The source tree (line 22): the keyed family directory.

````text
  hash/             the HashTrieIterable family
````

with

````text
  hash/             the HashTrieIterable family
  keyed/            the KeyedIterable family (binary_hash_join.rs)
````

**Edit 6.** The public API list (line 31): `JoinAlgo`'s sentence.

````text
so neither algorithm materialises its result.
````

with

````text
so no algorithm materialises its result.
````

**Edit 7.** The public API list (line 33): the keyed-family entry point, inserted after `HashTriejoin`.

````text
- [`HashTriejoin`](src/hash/hash_triejoin.rs) — the hash-family entry point (`-a hash-triejoin`).
````

with

````text
- [`HashTriejoin`](src/hash/hash_triejoin.rs) — the hash-family entry point (`-a hash-triejoin`).
- [`BinaryHashJoin`](src/keyed/binary_hash_join.rs) — the keyed-family entry point (`-a binary-hash-join`): a left-deep pipelined binary hash join, the baseline for what the worst-case-optimal joins buy. It is not worst-case optimal.
````

**Edit 8.** The relationship paragraph (line 40).

````text
`HashTriejoin` any `HashTrieIterable` (in practice `HashTrie`).
````

with

````text
`HashTriejoin` any `HashTrieIterable` (in practice `HashTrie`), `BinaryHashJoin` any `KeyedIterable` (in practice `HashIndex`).
````

**Edit 9.** `kermit-iters/README.md`, the family traits (line 11): the keyed family, inserted after the hash-trie entry.

````text
it backs the `HashTriejoin` algorithm in `kermit-algos`.
````

with

````text
it backs the `HashTriejoin` algorithm in `kermit-algos`.
- [`KeyedIterable`](src/keyed.rs) — the keyed iterator family: an index keyed on its leading columns, read through `key_len`, `scan` (every row) and `probe` (exactly the rows with a given key, an exact-match lookup). It has no iterator type of its own, and is a separate trait for the same reason `HashTrieIterator` is; it backs the `BinaryHashJoin` algorithm in `kermit-algos`.
````

**Edit 10.** `kermit-iters/README.md`, the `JoinIterable` marker (line 16).

````text
(`LinearIterable`, `TrieIterable`, `HashTrieIterable`)
````

with

````text
(`LinearIterable`, `TrieIterable`, `HashTrieIterable`, `KeyedIterable`)
````

- [ ] **Step 3: Check**

Run: `grep -n -E 'two worst-case-optimal|Two Iterator Families|neither algorithm' kermit-algos/README.md kermit-iters/README.md | cut -c1-70`

Expected: one line, `kermit-algos/README.md:3` ("two worst-case-optimal" stays, since the two worst-case-optimal joins are unchanged).

Run: `grep -c -E 'KeyedIterable|BinaryHashJoin|keyed' kermit-algos/README.md kermit-iters/README.md`

Expected: `kermit-algos/README.md:7` or more, `kermit-iters/README.md:3` or more.

- [ ] **Step 4: Commit**

```bash
git commit -m "docs(readme): the keyed family and BinaryHashJoin in the kermit-algos and kermit-iters READMEs" -- kermit-algos/README.md kermit-iters/README.md
```

### Task 5.6: Two doc comments, `kermit/src/lib.rs` and `kermit-algos/src/sorted/mod.rs`

**Files:**
- Modify: `kermit/src/lib.rs:6`, `:36-37` (doc comments only)
- Modify: `kermit-algos/src/sorted/mod.rs:9` (a doc comment only)

Nothing but `//!` and `///` text changes. The new lines stay within `rustfmt.toml`'s `comment_width=80`, so `cargo fmt --check` still passes, and the intra-doc links to `db::binary_join` are checked by the story's `cargo doc`.

- [ ] **Step 1: Confirm the stale text**

Run: `grep -n -E 'running joins from raw tuple inputs|lftj_join.\] and \[.db::hash_join|Two Iterator Families' kermit/src/lib.rs kermit-algos/src/sorted/mod.rs`

Expected: three lines: `kermit/src/lib.rs:6`, `kermit/src/lib.rs:36` and `kermit-algos/src/sorted/mod.rs:9`.

- [ ] **Step 2: Apply the edits.** Each old fragment occurs exactly once in its file.

**Edit 1.** `kermit/src/lib.rs`, the crate doc (line 6): the keyed family's entry point, as a sentence after the existing text, so the existing lines keep their wrapping.

````text
//! running joins from raw tuple inputs.
````

with

````text
//! running joins from raw tuple inputs. A third entry point,
//! [`db::binary_join`], runs the keyed family's binary hash join.
````

**Edit 2.** `kermit/src/lib.rs`, the helper's doc comment (lines 36-37): the entry points that share its body.

````text
/// [`db::lftj_join`] and [`db::hash_join`] share, with `R` as the index
/// structure and `JA` as the algorithm. The query is therefore validated first,
````

with

````text
/// [`db::lftj_join`], [`db::hash_join`] and [`db::binary_join`] share, with
/// `R` as the index structure and `JA` as the algorithm. The query is
/// therefore validated first,
````

**Edit 3.** `kermit-algos/src/sorted/mod.rs`, the heading it cites (line 9).

````text
//! families are disjoint by construction (see "Two Iterator Families" in
````

with

````text
//! families are disjoint by construction (see "Three Iterator Families" in
````

- [ ] **Step 3: Check**

Run: `nix develop --command bash -c 'cargo fmt --all -- --check && RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit -p kermit-algos'`

Expected: no formatting diff, and the docs build with no warnings.

Run: `git diff --stat -- kermit/src/lib.rs kermit-algos/src/sorted/mod.rs`

Expected: the two files only, 6 insertions and 4 deletions in all, every changed line a `//!` or `///` line.

- [ ] **Step 4: Commit**

```bash
git commit -m "docs: the binary_join entry point in the kermit crate docs, the renamed family heading in the sorted family docs" -- kermit/src/lib.rs kermit-algos/src/sorted/mod.rs
```

### Task 5.7: `docs/data-structures/hash-trie.md`

**Files:**
- Modify: `docs/data-structures/hash-trie.md:397`, `:420-421`

Only the two statements this change makes false are corrected: the `levels n` suffix is no longer only a HashTrie matter, and `bench run` can now prepare under three policies.

- [ ] **Step 1: Confirm the stale text**

Run: `grep -n -E 'an .all. line reads as before|at most two' docs/data-structures/hash-trie.md | cut -c1-60`

Expected: two lines, 397 and 421.

- [ ] **Step 2: Apply the edits.** Each old fragment occurs exactly once in the file.

**Edit 1.** The bench axis value bullet (line 397).

````text
` levels n`; an `all` line reads as before.
````

with

````text
` levels n` (a HashIndex line ends the same way, with its key length as `n`); an `all` line reads as before.
````

**Edit 2.** The "Prepared per policy" bullet (lines 420-421).

````text
(at most two: the sorted cells' `all` and
    the HashTrie cell's), untimed;
````

with

````text
(at most three: the sorted cells' `all`,
    the HashTrie cell's and the HashIndex cell's `ProbeKeys`), untimed;
````

- [ ] **Step 3: Check**

Run: `grep -n -E 'at most two' docs/data-structures/hash-trie.md`

Expected: no output.

Run: `git diff --stat -- docs/data-structures/hash-trie.md`

Expected: the one file, 3 insertions and 3 deletions.

- [ ] **Step 4: Commit**

```bash
git commit -m "docs(hash-trie): the levels suffix and the prepared policies now include the HashIndex cell" -- docs/data-structures/hash-trie.md
```

### Task 5.8: `docs/specs/bench-report-schema.md`, and the story's gate

**Files:**
- Modify: `docs/specs/bench-report-schema.md:90-93`, `:119-120`, `:256-263`, the change-log table's last row (`:318`)

- [ ] **Step 1: Confirm the text to extend**

Run: `grep -n -E '"HashTrie"`\. The|"HashTriejoin"`\. The|A HashTrie index under' docs/specs/bench-report-schema.md | cut -c1-60`

Expected: three lines, 90, 119 and 120.

- [ ] **Step 2: Apply the edits.**

**Edit 1.** The `index` line's ` levels {n}` suffix (lines 90-93). The claim that it appears only under `--ds-layout-levels keys` is no longer true: `describe_index` appends it whenever an index has fewer levels than columns, and a HashIndex index has its key length as its levels.

````text
A HashTrie index under `--ds-layout-levels
keys` whose trie has levels for fewer columns than it keeps ends its columns
with ` levels {n}` (`#0 r filter - columns [0, 1] levels 1: 4 rows, 272
bytes`, #116); every other line reads as before.
````

with

````text
An index whose structure has levels for
fewer columns than it keeps ends its columns with ` levels {n}`. That is a
HashTrie index under `--ds-layout-levels keys` (`#0 r filter - columns [0, 1]
levels 1: 4 rows, 272 bytes`, #116) and a HashIndex index whose key is shorter
than its columns, where `n` is the key length (` levels 0` for the driver,
which no earlier atom keys); every other line reads as before.
````

**Edit 2.** The `data_structure` values (line 119).

````text
`"ColumnTrie"`, `"HashTrie"`. The `IndexStructure::axis_value` string.
````

with

````text
`"ColumnTrie"`, `"HashTrie"`, `"HashIndex"`. The `IndexStructure::axis_value` string.
````

**Edit 3.** The `algorithm` values (line 120).

````text
`"LeapfrogTriejoin"`, `"HashTriejoin"`. The `JoinAlgorithm::axis_value` string.
````

with

````text
`"LeapfrogTriejoin"`, `"HashTriejoin"`, `"BinaryHashJoin"`. The `JoinAlgorithm::axis_value` string.
````

**Edit 4.** The `ds_layout_hasher` axis, after the `ds_layout_levels` entry (lines 256-263).

````text
  kermit-lab back-fills `"all"` on HashTrie rows without it.
````

with

````text
  kermit-lab back-fills `"all"` on HashTrie rows without it.
- `ds_layout_hasher` (HashTrie and HashIndex, `"sip"` / `"fxhash"`, the `NAME` of the `HashStrategy` the structure was built over): the hash function. A HashIndex report always carries it, and carries no other `ds_*` axis (no `ds_layout_pruning`, `ds_layout_expansion`, `ds_layout_levels` or `ds_layout_seek`, no `ds_config_*` and no `ds_build_mode`), because the structure has none and the CLI refuses every other `--ds-*` flag on it. kermit-lab's back-fills are keyed by structure, so none applies to a HashIndex row, and `speedup_table` skips it, since it keeps only rows that carry a `ds_build_mode`.
````

**Edit 5.** The change log: append a row after the last one.

````text
`schema_version` stays 6. kermit-lab back-fills `all`. |
````

with

````text
`schema_version` stays 6. kermit-lab back-fills `all`. |
| 6 (no bump) | 2026-11-02 | A fourth cell, `HashIndex` × `BinaryHashJoin` (a left-deep pipelined binary hash join over a keyed hash index): new `data_structure` and `algorithm` axis values, and `ds_layout_hasher` on the new structure's reports. The `index` line's ` levels {n}` suffix, which this table's previous row introduced under `keys`, also appears on a HashIndex index, with its key length as `n` (` levels 0` for the driver). Additive: no metric changes meaning and no existing key changes type, so `schema_version` stays 6. kermit-lab needs no new back-fill. |
````

- [ ] **Step 3: Check the whole story**

Run: `nix develop --command bash -c 'RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps && cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings'`

Expected: both finish with no warnings. (Docs-only edits cannot add one; this confirms the tree is as Stories 1 to 4 left it.)

Run: `grep -rn -i -E 'exactly 3 valid|the 3 valid|three valid|three pairings|two iterator families|at most two' ARCHITECTURE.md CLAUDE.md README.md USAGE.md kermit/README.md kermit-ds/README.md kermit-algos/README.md kermit-iters/README.md kermit/src/lib.rs kermit-algos/src/sorted/mod.rs docs/data-structures/hash-trie.md docs/specs/bench-report-schema.md`

Expected: no output.

- [ ] **Step 4: Commit**

```bash
git commit -m "docs(bench-report-schema): HashIndex and BinaryHashJoin values and ds_layout_hasher on the new structure" -- docs/specs/bench-report-schema.md
```
