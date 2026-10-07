# HashTrie Build by Algorithm 2 (#107) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the paper's Algorithm 2 (group a list's tuples into a table's
buckets, then build each bucket's child from its list) the build process of
every HashTrie build mode, under the new default name `bulk`, keep the
per-tuple build unchanged as `incremental`, and add the Config value
`--ds-config child-capacity=grow|tuples`, which sizes every child once from
its list.

**Architecture:** There are five work packages; see "Work packages".

- **P1** (Tasks 1–6), `kermit-ds`, the process:
  - `HashTable` gains `map` and `map_in_runs`, Algorithm 2's "store `M_next`
    in `B`" for a whole table;
  - a new `bulk.rs` holds Algorithm 2 (`build`, `group`, `build_nested`,
    `child`);
  - `HashTrieBuildMode::Serial` becomes `Bulk` (default) and `Incremental`
    (today's code, unchanged);
  - lazy expansion, `radix:K`, `parallel:N` and `presized:N` all build their
    children through `child`;
  - `presized:N`'s two mirrored root steps fold into one "push onto the
    list" step plus a parallel children phase.

  Under every existing config the trie stays array-identical, so P1 changes
  build time only.
- **P2** (Task 7), `kermit-ds`, the Config: `ChildCapacity`, the
  `child_log2_capacity` sizing, and `incremental`'s prerequisite at the
  constructor.
- **P3** (Tasks 8–10), the `kermit` binary: the `--ds-config` key, the
  `Prerequisite` row, CLI and family tests, the join suites.
- **P4** (Task 11), kermit-lab: the rename map, the new back-fills, the
  per-structure speedup baseline.
- **P5** (Task 12), docs.
- **Controller** (Tasks 0, 13, 14): the merge, the gates, a smoke run,
  checkpoints, and landing on the user's word.

**Tech Stack:** Rust nightly (std scoped threads through `crate::morsel`),
the kermit-ds and kermit test macros, Python 3.13 + uv + pytest
(kermit-lab), Nix dev shell.

**Spec:** [`docs/specs/2026-10-07-hash-trie-algorithm-2-build-design.md`](../../specs/2026-10-07-hash-trie-algorithm-2-build-design.md) (approved 2026-10-07, commit 2293b99) · **Issues:** #107, #105 · **Parity rule:** every change moves HashTrie toward the paper; see the spec's "Parity with the paper".

---

## Preconditions

- **The dependent-optimisations design has landed on origin/master**
  (spec `docs/specs/2026-10-07-dependent-optimisations-design.md`, plan
  `docs/superpowers/plans/2026-10-07-dependent-optimisations.md`, branch
  `aidanb/dependent-optimisations`). It provides the following, and Task 0
  checks each name:
  - `HashTrieBuildMode::Presized(Threads)`, parsed from `presized:<threads>`;
  - in `from_tuples_with_config_and_build_mode`, a `Parallel` arm that
    always calls `parallel::fill_root`, and a `Presized` arm that asserts
    `root-capacity=tuples` and calls `parallel::fill_presized_root`;
  - `Prerequisite` (one variant, `PresizedBuildNeedsPresizedRoot`),
    `Prerequisite::ALL`, `Violation`, and the check at the end of
    `DsChoices::resolve`, in `kermit/src/options.rs`;
  - the tests `every_prerequisite_is_reachable`,
    `presized_build_reaches_its_own_path`,
    `parallel_build_merges_under_every_root_capacity` and
    `presized_build_requires_a_presized_root`;
  - the `HashPresized2` build-mode provider in `kermit/tests/join_tests.rs`
    and `kermit-ds/tests/hash_trie_tests.rs`;
  - `threads_of` reading `presized:N` in kermit-lab, `build_of` beside it,
    and `speedup_table` keying its arms by build mode and thread count;
  - the row-generic `every_prerequisite_is_reachable`, whose `match row`
    arms each return `(violating, satisfied, expected: Violation)`.

  Checked against the branch tip `fbe1a60` on 2026-10-07 (not yet on
  origin/master then); Tasks 8 and 11 are written against that code.

  If a name differs, use the landed one and record the difference in the P1
  report. **Do not start Task 1 before Task 0 is done.**
- **This branch** is `aidanb/hash-trie-paper-parity` (a loom worktree). It
  holds the spec. Commit on it; never switch branches.
- **No measurement claims.** Task 13's smoke run is a go/no-go for the
  checkpoint. The replicated measurement is a separate step ("After this
  plan").

## Ground rules for every task

- **Paths.**
  - `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/hash-trie-paper-parity_18dc25f8653dd5c4`.
  - `SCRATCH` is the executing session's scratchpad.
  - Never `cd`; use `git -C $WT`, `env -C`, `uv --directory`.
- **Cargo.** Run it in the foreground, as `CARGO_BUILD_JOBS=2 nix develop $WT
  --command cargo …`. A memory monitor kills cargo started with
  `run_in_background`. Run jobs over 10 minutes (kermit-ds Miri takes about
  35) with `setsid nohup … & disown`, and poll their log.
- **The host is shared.** Before any timing run, or any cargo run over a few
  minutes, run `pgrep -af "[k]ermit-bench-runs/.*/bin/kermit|kermit (bench|ds)|perf record"`.
  If another session is timing, wait, or run niced on one core
  (`nice -n 19 taskset -c 15`, `CARGO_BUILD_JOBS=1`).
- **Formatting.** Only `nix develop $WT --command cargo fmt --all`, before
  every commit.
- **Doc comments.** Backtick every identifier (clippy's `doc_markdown` runs
  under `-Dwarnings`).
- **Commits.** Conventional commits referencing `(#107)`. Stage files by
  name; never amend or push. Every message ends with these two lines; an
  executing session substitutes its own `Claude-Session` line:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  ```
- **Mutation checks** (the tasks name each mutant).
  1. Commit first.
  2. Apply the mutant as an exact edit.
  3. Confirm the named test fails, and that `git -C $WT diff` shows the mutant
     applied.
  4. Reverse the exact edit, never with `git checkout`.
  5. Re-run the test to green; `git -C $WT status` must be clean.
- **Scope (Priority 6).**
  - Untouched: the body of `HashTrie::insert_at`, `Relation::insert`,
    `HashTable::{bucket_index, entry_or_insert_with, grow}`, `BucketRun`,
    `with_runs`, `morsel.rs`; TreeTrie, ColumnTrie, the algorithms, the
    optimisers.
  - `incremental` executes exactly the code `from_tuples_with_config` ran
    before this plan; Task 13 checks it with a diff.
  - The default config's trie is unchanged, array for array; the identity
    tests are the check.
- **Parity.**
  - Three mechanisms are kermit's, not the paper's: the overflow tail of
    `presized:N`, how `presized:N` spreads the recursion over threads (each
    run's children on a worker), and the pending-list capacity fix in
    `child`. No doc or comment may attribute them to the paper.
  - A bucket list is a `Vec`. Umbra threads its lists through an 8-byte chain
    pointer in each tuple (§3.3.2); that is #101's, not this plan's.
- **Never write under `~/.cache/kermit`.** Reading `friendof.parquet` from it
  for the smoke run is fine.
- **Checkpoints with the user:** after P1 and P2 (identity holds, Miri is
  clean), after Task 13, and before any push.

---

## File map

| File | Responsibility in this plan |
|---|---|
| `kermit-ds/src/ds/hash_trie/hash_table.rs` | `map`, `map_in_runs`, `MapRun`; `log2_capacity_for` takes its floor; `PAPER_MIN_LOG2_CAPACITY` |
| `kermit-ds/src/ds/hash_trie/bulk.rs` (new) | Algorithm 2: `TupleList`, `build`, `group`, `build_nested`, `child`; its identity and sizing tests |
| `kermit-ds/src/ds/hash_trie/mod.rs` | `mod bulk;`; re-exports `ChildCapacity`, `ParseChildCapacityError` |
| `kermit-ds/src/ds/hash_trie/implementation.rs` | `from_tuples_in_bulk`, `from_tuples_incrementally`, `assert_arities`; the mode arms; `resolve` through `build`; `from_tuples_partitioned` fills by value; the mirrored root steps removed; doc credits fixed |
| `kermit-ds/src/ds/hash_trie/build_mode.rs` | `Bulk`, `Incremental`; `serial` rejected with a hint |
| `kermit-ds/src/ds/hash_trie/config.rs` | `ChildCapacity`, `ParseChildCapacityError`, `child_capacity`, `child_log2_capacity` |
| `kermit-ds/src/ds/hash_trie/radix.rs` | scratch roots by Algorithm 2; `config` instead of `load_factor` |
| `kermit-ds/src/ds/hash_trie/parallel.rs` | exact fill takes `config`; presized fill: group in runs, tail, children in runs; `push_in_run` |
| `kermit-ds/src/ds/hash_trie/identity.rs` | `configs()` loops child capacity; `incremental_configs()` |
| `kermit-ds/src/ds/mod.rs`, `kermit-ds/src/lib.rs` | re-exports |
| `kermit-ds/tests/hash_trie_tests.rs`, `kermit-ds/tests/parquet_tests.rs` | `HashIncremental` and `SizedChildren` aliases; lazy trace tests under sized children |
| `kermit/src/options.rs` | `child-capacity` key; `IncrementalBuildNeedsGrowingChildren`; renamed test strings |
| `kermit/src/execution.rs` | renamed mode; a family test for `child-capacity` |
| `kermit/tests/{cli_hash_trie_build_mode,cli_column_trie_build_mode,cli_hash_trie_config_choice,join_tests}.rs` | CLI and join coverage |
| `python/kermit-lab/kermit_lab/{defaults,frame,analysis,presets}.py` and tests | rename map, back-fills, per-structure baseline |
| `docs/data-structures/{hash-trie,parallel-build}.md`, `docs/specs/{optimization-standard,bench-report-schema}.md`, `CLAUDE.md`, the spec | docs |

---

## Work packages

One implementer per package. Each package is reviewed before the next
starts, and the controller merges `origin/master` at every boundary.

| Package | Tasks | Scope | Commits | Done when |
|---|---|---|---|---|
| *Controller* | 0 | Merge origin/master with dependent-optimisations, check names, record the baseline | merge | `$SCRATCH/baseline.txt`; workspace green |
| **P1 — process** | 1–6 | `map`; `bulk.rs`; modes; lazy; scratch roots; presized fold | 6 | `cargo test -p kermit-ds` and `-p kermit` green; mutants killed |
| **P2 — Config** | 7 | `ChildCapacity` and sizing in kermit-ds | 1 | `cargo test -p kermit-ds` green; kermit-ds Miri clean |
| **P3 — `kermit`** | 8–10 | CLI key and prerequisite; CLI and family tests; join suites | 3 | `cargo test -p kermit` green |
| **P4 — kermit-lab** | 11 | rename map, defaults, baseline | 1 | pytest green, contract test included |
| **P5 — docs** | 12 | the docs listed in the spec | 1 | `cargo doc` and clippy clean |
| *Controller* | 13, 14 | gate, scope diff, smoke run, landing | — | — |

Each implementer gets this plan, its task numbers, the ground rules, and a
report budget of about 40 lines. The report covers commit SHAs, test counts,
mutation outcomes, and any deviation with its reason.

---

## Task 0 [controller]: Merge and record the baseline

**Files:** a merge commit, if origin/master moved.

- [ ] **Step 1: Merge.**
  ```bash
  git -C $WT fetch origin
  git -C $WT merge origin/master
  ```
  This branch only adds the spec and this plan, so the merge is clean.

- [ ] **Step 2: Check the dependent-optimisations names exist.**
  ```bash
  git -C $WT grep -n -E 'Presized\(Threads\)|enum Prerequisite|PresizedBuildNeedsPresizedRoot|struct Violation|fn every_prerequisite_is_reachable|fn presized_build_reaches_its_own_path|fn parallel_build_merges_under_every_root_capacity|fn presized_build_requires_a_presized_root|HashPresized2' -- kermit-ds kermit
  ```
  Expected: hits in `build_mode.rs`, `implementation.rs`, `parallel.rs`,
  `options.rs`, `join_tests.rs` and `hash_trie_tests.rs`. If any is missing,
  stop: the precondition does not hold.

- [ ] **Step 3: Green, record the baseline.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo check --workspace --all-targets
  {
    echo "BASE $(git -C $WT rev-parse HEAD)"
    CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {print "cargo "p" passed, "f" failed, "i" ignored"}'
  } | tee $SCRATCH/baseline.txt
  git -C $WT rev-parse HEAD > $SCRATCH/base.sha
  git -C $WT show HEAD:kermit-ds/src/ds/hash_trie/implementation.rs > $SCRATCH/implementation.base.rs
  ```
  The last file is Task 13's reference for "`incremental` runs today's code".

---

# P1 — `kermit-ds`: Algorithm 2 becomes the build

## Task 1 [P1]: `HashTable::map` and `map_in_runs`

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/hash_table.rs` (after `into_buckets`; the
  new `MapRun` type after `VacantBucket`'s impl; tests at the end of `mod tests`)

- [ ] **Step 1: Write the failing tests.** Append to `mod tests`:

```rust
    /// A table of 40 hashes (two of which share a home bucket at 4
    /// buckets), each valued by the order it was inserted in.
    fn filled() -> HashTable<usize> {
        let mut t = HashTable::new();
        let (first, second) = colliding_pair(&t);
        t.entry_or_insert_with(first, LoadFactor::default(), || 0);
        t.entry_or_insert_with(second, LoadFactor::default(), || 1);
        let mut hash: u64 = 1;
        for k in 2..40 {
            t.entry_or_insert_with(hash, LoadFactor::default(), || k);
            hash = hash.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
        }
        t
    }

    /// `map` keeps the table's shape and every key in its bucket, and
    /// replaces each value.
    #[test]
    fn map_keeps_every_key_in_its_bucket() {
        let t = filled();
        let (cap, len) = (t.buckets_len(), t.len());
        let before: Vec<(Option<u64>, Option<usize>)> = (0..cap)
            .map(|i| (t.hash_at(i), t.value_at(i).copied()))
            .collect();
        let mapped: HashTable<String> = t.map(|v| format!("v{v}"));
        assert_eq!(mapped.buckets_len(), cap);
        assert_eq!(mapped.len(), len);
        assert_eq!(
            mapped.shell_heap_bytes(),
            cap * std::mem::size_of::<Option<Entry<String>>>()
        );
        for (i, (hash, value)) in before.into_iter().enumerate() {
            assert_eq!(mapped.hash_at(i), hash, "bucket {i}");
            assert_eq!(
                mapped.value_at(i).cloned(),
                value.map(|v| format!("v{v}")),
                "bucket {i}"
            );
        }
    }

    /// `map` calls `f` in bucket order, the order in which Algorithm 2's
    /// line 9 visits the populated buckets.
    #[test]
    fn map_calls_f_in_bucket_order() {
        let t = filled();
        let order: Vec<usize> = t.iter().map(|(_, &v)| v).collect();
        let mut seen = Vec::new();
        let _ = t.map(|v| seen.push(v));
        assert_eq!(seen, order);
    }

    /// Mapping in runs builds what `map` builds, however the table is cut.
    #[test]
    fn map_in_runs_matches_map() {
        let serial = filled().map(|v| v * 10);
        for parts in [1, 2, 4, 8, serial.buckets_len()] {
            let in_runs = filled().map_in_runs(parts, |runs| {
                assert_eq!(runs.len(), parts);
                for run in runs {
                    run.map(|v| v * 10);
                }
            });
            assert_eq!(in_runs.buckets_len(), serial.buckets_len(), "{parts} runs");
            assert_eq!(in_runs.len(), serial.len(), "{parts} runs");
            for i in 0..serial.buckets_len() {
                assert_eq!(in_runs.hash_at(i), serial.hash_at(i), "{parts} runs, bucket {i}");
                assert_eq!(in_runs.value_at(i), serial.value_at(i), "{parts} runs, bucket {i}");
            }
        }
    }

    /// A run left unmapped would lose its values, so `map_in_runs` refuses.
    #[test]
    #[should_panic(expected = "a run was left unmapped")]
    fn map_in_runs_refuses_to_lose_a_run() {
        let _ = filled().map_in_runs(2, |mut runs: Vec<MapRun<'_, usize, usize>>| {
            runs.pop().expect("two runs").map(|v| v);
        });
    }
```

- [ ] **Step 2: Run the tests to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib hash_table::tests::map
  ```
  Expected: compile error, `no method named map found for struct HashTable`
  (and `cannot find type MapRun`).

- [ ] **Step 3: Implement.** After `impl<'r, V> VacantBucket<'r, V> { … }`, add:

```rust
/// One contiguous run of a table that [`HashTable::map_in_runs`] is
/// mapping: its buckets, and the mapped table's buckets at the same
/// positions.
pub(super) struct MapRun<'a, V, W> {
    source: &'a mut [Option<Entry<V>>],
    target: &'a mut [Option<Entry<W>>],
}

impl<V, W> MapRun<'_, V, W> {
    /// Moves each value of the run, as `f(value)`, into the same bucket of
    /// the mapped table, in bucket order.
    pub(super) fn map(self, mut f: impl FnMut(V) -> W) {
        for (source, target) in self.source.iter_mut().zip(self.target.iter_mut()) {
            *target = source.take().map(|Entry { hash, value }| Entry {
                hash,
                value: f(value),
            });
        }
    }
}
```

and after `into_buckets`:

```rust
    /// The table with every value replaced by `f(value)`, called in bucket
    /// order: the same capacity, the same `len`, and every key in the bucket
    /// it occupied. Algorithm 2's "store `M_next` in `B`" (line 12) for a
    /// whole table: a table of tuple lists cannot hold the children built
    /// from them in place, since the two value types differ.
    pub(super) fn map<W>(self, mut f: impl FnMut(V) -> W) -> HashTable<W> {
        HashTable {
            log2_capacity: self.log2_capacity,
            len: self.len,
            buckets: self
                .buckets
                .into_iter()
                .map(|slot| {
                    slot.map(|Entry { hash, value }| Entry {
                        hash,
                        value: f(value),
                    })
                })
                .collect(),
        }
    }

    /// [`map`](Self::map) for a parallel caller: lends `map_runs` the table
    /// as `parts` contiguous [`MapRun`]s, one per worker, and returns the
    /// mapped table once `map_runs` has returned.
    ///
    /// # Panics
    ///
    /// Unless `parts` is a power of two no larger than the capacity, and if
    /// `map_runs` returns without mapping every run, whose values would
    /// otherwise be lost.
    pub(super) fn map_in_runs<W>(
        mut self, parts: usize, map_runs: impl FnOnce(Vec<MapRun<'_, V, W>>),
    ) -> HashTable<W> {
        let capacity = self.buckets.len();
        assert!(
            parts.is_power_of_two() && parts <= capacity,
            "{parts} runs do not split {capacity} buckets"
        );
        let mut buckets: Vec<Option<Entry<W>>> = (0..capacity).map(|_| None).collect();
        let run_len = capacity / parts;
        map_runs(
            self.buckets
                .chunks_mut(run_len)
                .zip(buckets.chunks_mut(run_len))
                .map(|(source, target)| MapRun { source, target })
                .collect(),
        );
        assert!(
            self.buckets.iter().all(Option::is_none),
            "map_in_runs: a run was left unmapped"
        );
        HashTable {
            log2_capacity: self.log2_capacity,
            len: self.len,
            buckets,
        }
    }
```

- [ ] **Step 4: Run the tests to verify they pass.** Same command. Expected:
  4 passed. (`mod hash_table` carries `#[allow(dead_code)]` in `mod.rs`, so
  the not-yet-used methods raise no warning.)

- [ ] **Step 5: Commit.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit-ds/src/ds/hash_trie/hash_table.rs
  git -C $WT commit -m "feat(hash-trie): HashTable::map and map_in_runs, Algorithm 2's store step (#107)"
  ```

---

## Task 2 [P1]: `bulk.rs`, Algorithm 2

**Files:**
- Create: `kermit-ds/src/ds/hash_trie/bulk.rs`
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs` (`mod bulk;` after `mod build_mode;`)
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (`is_leaf_depth`
  becomes `pub(super)`; new `assert_arities`, `from_tuples_in_bulk`,
  `from_tuples_incrementally`; `from_tuples_with_config` and
  `from_tuples_partitioned` delegate)

- [ ] **Step 1: Write the module with its failing tests.** Create
  `kermit-ds/src/ds/hash_trie/bulk.rs`:

```rust
//! The `bulk` build of a [`HashTrie`](super::HashTrie): Algorithm 2 of
//! Freitag et al., *Adopting Worst-Case Optimal Joins in Relational
//! Database Systems* (VLDB 2020, §3.2.2), issue #107.
//!
//! ```text
//!  1 function build(i, L)
//!  2   if i ≤ n then
//!  3     M ← allocateHashtable(2^⌈log2(1.25·|L|)⌉)
//!  4     while L is not empty do
//!  5       t ← pop next tuple from L
//!  6       B ← lookupBucket(M, h_i(π_vi(t)))
//!  7       push t onto the linked list stored in B
//!  8     i_next ← index of the next attribute in E_j
//!  9     foreach populated bucket B in M do
//! 10       L_next ← extract linked list stored in B
//! 11       M_next ← build(i_next, L_next)
//! 12       store M_next in B
//! 13     return M
//! 14   else
//! 15     return L
//! ```
//!
//! [`HashTrie::group`] is lines 3–7 and [`HashTrie::build_nested`] lines
//! 8–15. At the last attribute, lines 9–12 store each list itself (line 15),
//! so the table of lists is the leaf and its lists are the chains.
//! [`HashTrie::child`] decides what a bucket's list becomes: a pruned
//! `Singleton` (§3.3.1), an `Unexpanded` child under lazy expansion
//! (§3.3.1), or the table line 11 builds.
//!
//! Two differences from the paper, both kermit's:
//!
//! - A list is a `Vec` per bucket. Umbra threads its lists through an 8-byte
//!   chain pointer reserved in each materialised tuple (§3.3.2), which needs
//!   contiguous tuple storage (#101).
//! - Line 3's size: the root's comes from `root-capacity` (#88); every child
//!   starts at 4 buckets and grows, as under the per-tuple build.
//!
//! Algorithm 2 builds the trie the per-tuple build (`insert_at`, the
//! `incremental` mode) builds, array for array. A table's layout depends
//! only on the order its new keys arrive, and every list keeps input order,
//! so each table receives its keys in the order `insert_at` sends them.

use {
    super::{
        config::HashTrieConfig,
        expansion::{ExpansionPolicy, PendingChild},
        hash_table::{HashTable, INITIAL_LOG2_CAPACITY},
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::{PruningPolicy, SingletonPayload},
    },
    kermit_iters::HashStrategy,
};

/// A list of tuples in input order: Algorithm 2's `L`.
pub(super) type TupleList = Vec<Vec<usize>>;

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> HashTrie<H, P, E> {
    /// Algorithm 2: the table at `depth` over `list`, allocated at
    /// `2^log2_capacity` buckets.
    pub(super) fn build(
        depth: usize, arity: usize, list: TupleList, log2_capacity: u32, config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        let lists = Self::group(depth, list, log2_capacity, config);
        Self::build_nested(depth, arity, lists, config)
    }

    /// Lines 3–7: a table of `2^log2_capacity` buckets holding `list`'s
    /// tuples, each pushed onto the list in the bucket of its attribute at
    /// `depth`. The table grows under the load factor if its keys outgrow
    /// it, as `insert_at`'s tables do.
    pub(super) fn group(
        depth: usize, list: TupleList, log2_capacity: u32, config: HashTrieConfig,
    ) -> HashTable<TupleList> {
        let mut lists = HashTable::with_log2_capacity(log2_capacity);
        for tuple in list {
            let hash = H::hash(tuple[depth]);
            lists
                .entry_or_insert_with(hash, config.load_factor, Vec::new)
                .push(tuple);
        }
        lists
    }

    /// Lines 8–15: each bucket's list replaced by the node built from it,
    /// in bucket order (line 9). At the last attribute each list is stored
    /// itself (line 15), so the table of lists is the leaf.
    pub(super) fn build_nested(
        depth: usize, arity: usize, lists: HashTable<TupleList>, config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        if Self::is_leaf_depth(depth, arity) {
            return HashTrieNode::Leaf(lists);
        }
        HashTrieNode::Inner(lists.map(|list| Self::child(depth + 1, arity, list, config)))
    }

    /// The node a bucket's `list` becomes, at `depth`: a `Singleton` when
    /// pruning is on and one tuple lives below (§3.3.1), an `Unexpanded`
    /// child under lazy expansion (§3.3.1; `HashTrie::resolve` builds it by
    /// [`build`](Self::build) on the first probe), and otherwise the table
    /// line 11 builds.
    pub(super) fn child(
        depth: usize, arity: usize, mut list: TupleList, config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        if P::ENABLED && list.len() == 1 {
            let tuple = list.pop().expect("the list holds one tuple");
            return HashTrieNode::Singleton(P::Payload::from_tuple(tuple));
        }
        if E::LAZY {
            // `insert_at` starts a pending list as `vec![tuple]`, capacity
            // 1, or after an unprune as `Vec::with_capacity(2)`. A list grown
            // from empty reaches capacity 4 on its first push, and from there
            // both grow alike. Shrinking the two short cases keeps the trie
            // byte-identical to `incremental`'s, so `space` cannot move, and
            // keeps a one-tuple pending list at one slot.
            let first_capacity = if P::ENABLED { 2 } else { 1 };
            if list.len() == first_capacity {
                list.shrink_to(first_capacity);
            }
            return HashTrieNode::Unexpanded(E::Pending::from_tuples(list));
        }
        Self::build(depth, arity, list, INITIAL_LOG2_CAPACITY, config)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            ds::hash_trie::{
                expansion::{EagerExpansion, LazyExpansion},
                identity::{assert_same_trie, configs, inputs},
                pruning::{NoPruning, SingletonPruning},
            },
            test_support::{Lcg, Mod10HashStrategy},
        },
        kermit_iters::{FxHashStrategy, SipHashStrategy},
    };

    fn label<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
        arity: usize, config: HashTrieConfig, input: &str,
    ) -> String {
        format!(
            "{}/{}/{} arity {arity}, load {}%, root {}, {input}",
            H::NAME,
            P::NAME,
            E::NAME,
            config.load_factor.numerator(),
            config.root_capacity.axis_value(),
        )
    }

    /// Algorithm 2 builds the trie the per-tuple build makes, array for
    /// array: capacities, chains, singletons and pending lists included.
    /// The `interleaved` input gives an arity-2 trie a one-tuple and a
    /// two-tuple child, the two lengths whose pending lists `child` shrinks.
    fn check_identity<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for config in configs() {
                for (input, tuples) in inputs(arity) {
                    let incremental = HashTrie::<H, P, E>::from_tuples_incrementally(
                        arity.into(),
                        config,
                        tuples.clone(),
                    );
                    let bulk =
                        HashTrie::<H, P, E>::from_tuples_in_bulk(arity.into(), config, tuples);
                    assert_same_trie(&incremental, &bulk, &label::<H, P, E>(arity, config, input));
                }
            }
        }
    }

    #[test]
    fn bulk_builds_the_incremental_trie_under_siphash() {
        check_identity::<SipHashStrategy, NoPruning, EagerExpansion>();
        check_identity::<SipHashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<SipHashStrategy, NoPruning, LazyExpansion>();
        check_identity::<SipHashStrategy, SingletonPruning, LazyExpansion>();
    }

    #[test]
    fn bulk_builds_the_incremental_trie_under_fxhash() {
        check_identity::<FxHashStrategy, NoPruning, EagerExpansion>();
        check_identity::<FxHashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<FxHashStrategy, NoPruning, LazyExpansion>();
        check_identity::<FxHashStrategy, SingletonPruning, LazyExpansion>();
    }

    /// Distinct keys share full hashes, so one bucket's list holds several
    /// keys' tuples and leaf chains hold colliding tuples.
    #[test]
    fn bulk_builds_the_incremental_trie_under_colliding_hashes() {
        check_identity::<Mod10HashStrategy, NoPruning, EagerExpansion>();
        check_identity::<Mod10HashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<Mod10HashStrategy, NoPruning, LazyExpansion>();
        check_identity::<Mod10HashStrategy, SingletonPruning, LazyExpansion>();
    }

    /// Arity 4 and a first key holding half the tuples, which the shared
    /// inputs leave out.
    #[test]
    #[cfg_attr(miri, ignore = "thousands of inserts")]
    fn bulk_builds_the_incremental_trie_on_wide_and_skewed_inputs() {
        fn check<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
            let mut lcg = Lcg(0x107);
            for arity in 1..=4 {
                let random: Vec<Vec<usize>> = (0..4_000)
                    .map(|_| (0..arity).map(|_| lcg.next_usize() % 300).collect())
                    .collect();
                let skewed: Vec<Vec<usize>> = (0..4_000)
                    .map(|i| {
                        let first = if i % 2 == 0 {
                            7
                        } else {
                            lcg.next_usize() % 300
                        };
                        std::iter::once(first)
                            .chain((1..arity).map(|_| lcg.next_usize() % 20))
                            .collect()
                    })
                    .collect();
                for (input, tuples) in [("random", random), ("half one key", skewed)] {
                    for config in configs() {
                        let incremental = HashTrie::<H, P, E>::from_tuples_incrementally(
                            arity.into(),
                            config,
                            tuples.clone(),
                        );
                        let bulk = HashTrie::<H, P, E>::from_tuples_in_bulk(
                            arity.into(),
                            config,
                            tuples.clone(),
                        );
                        assert_same_trie(
                            &incremental,
                            &bulk,
                            &label::<H, P, E>(arity, config, input),
                        );
                    }
                }
            }
        }
        check::<SipHashStrategy, NoPruning, EagerExpansion>();
        check::<SipHashStrategy, SingletonPruning, LazyExpansion>();
        check::<FxHashStrategy, NoPruning, LazyExpansion>();
        check::<FxHashStrategy, SingletonPruning, EagerExpansion>();
    }

    #[test]
    #[should_panic(expected = "does not match header arity")]
    fn bulk_build_rejects_a_wrong_arity() {
        let _: HashTrie = HashTrie::from_tuples_in_bulk(
            2.into(),
            HashTrieConfig::default(),
            vec![vec![1, 2], vec![3]],
        );
    }
}
```

  In `kermit-ds/src/ds/hash_trie/mod.rs`, add `mod bulk;` after `mod build_mode;`.

- [ ] **Step 2: Run the tests to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib hash_trie::bulk
  ```
  Expected: compile errors: `is_leaf_depth` is private, and
  `from_tuples_incrementally` / `from_tuples_in_bulk` do not exist.

- [ ] **Step 3: Implement in `implementation.rs`.**

  1. `fn is_leaf_depth` → `pub(super) fn is_leaf_depth`.
  2. In the `impl … HashTrie<H, P, E>` block that holds
     `from_tuples_with_config_and_build_mode`, add before
     `from_tuples_partitioned`:

```rust
    /// The serial build's arity check, with its message, for the builds
    /// that read their whole input before building any of it.
    fn assert_arities(arity: usize, tuples: &[Vec<usize>]) {
        for tuple in tuples {
            assert_eq!(
                tuple.len(),
                arity,
                "from_tuples: tuple arity {} does not match header arity {}",
                tuple.len(),
                arity,
            );
        }
    }

    /// The `bulk` build: Algorithm 2 from the root (`bulk.rs`), the root
    /// sized as every build sizes it, by
    /// [`HashTrieConfig::root_log2_capacity`] (#88).
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    pub(super) fn from_tuples_in_bulk(
        header: RelationHeader, config: HashTrieConfig, tuples: Vec<Vec<usize>>,
    ) -> Self {
        let arity = header.arity();
        Self::assert_arities(arity, &tuples);
        let tuple_count = tuples.len();
        let root = Self::build(
            0,
            arity,
            tuples,
            config.root_log2_capacity(tuple_count),
            config,
        );
        Self {
            header,
            root,
            tuple_count,
            config,
            _layout: PhantomData,
        }
    }

    /// The `incremental` build: one [`insert_at`](Self::insert_at) per
    /// tuple, in input order. This is the build `from_tuples_with_config`
    /// ran before #107, unchanged.
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    pub(super) fn from_tuples_incrementally(
        header: RelationHeader, config: HashTrieConfig, tuples: Vec<Vec<usize>>,
    ) -> Self {
        let arity = header.arity();
        let mut trie = Self::with_config_for(header, config, tuples.len());
        for tuple in tuples {
            assert_eq!(
                tuple.len(),
                arity,
                "from_tuples: tuple arity {} does not match header arity {}",
                tuple.len(),
                arity,
            );
            Self::insert_at(&mut trie.root, 0, arity, tuple, config.load_factor);
            // from_tuples bypasses insert(), so count here. If this loop is
            // ever refactored to route through insert(), drop this increment
            // or the counter double-counts.
            trie.tuple_count += 1;
        }
        trie
    }
```

  3. `ConfigurableRelation::from_tuples_with_config` becomes
     `Self::from_tuples_incrementally(header, config, tuples)` (its old body
     moved verbatim into step 2's function; Task 3 switches it to bulk).
  4. In `from_tuples_partitioned`, replace its `for tuple in &tuples {
     assert_eq!(…) }` loop with `Self::assert_arities(arity, &tuples);`.
  5. `with_config_for`'s doc: replace "Every constructor creates its root
     here, so no build path computes a capacity itself." with "Every
     constructor that starts from an empty root creates it here; the bulk
     build sizes its root by the same `root_log2_capacity`."

- [ ] **Step 4: Run the tests to verify they pass.** Same command, then the
  whole hash-trie module:
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib hash_trie
  ```
  Expected: the 5 bulk tests pass; the rest of `hash_trie` still passes.

- [ ] **Step 5: Mutants.** Commit first (Step 6), then for each, apply,
  confirm the named test fails, reverse:
  - **M2a, a bucket list reversed:** in `group`, `.push(tuple)` →
    `.insert(0, tuple)`. Fails: `bulk_builds_the_incremental_trie_under_siphash`
    (a chain differs).
  - **M2b, the capacity fix dropped:** delete the `if list.len() ==
    first_capacity { … }` block. Fails:
    `bulk_builds_the_incremental_trie_under_siphash` (`pending: chain
    capacity`).

- [ ] **Step 6: Commit.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit-ds/src/ds/hash_trie/bulk.rs kermit-ds/src/ds/hash_trie/mod.rs kermit-ds/src/ds/hash_trie/implementation.rs
  git -C $WT commit -m "feat(hash-trie): Algorithm 2 as bulk.rs, array-identical to the per-tuple build (#107)"
  ```

---

## Task 3 [P1]: `bulk` and `incremental` modes; `bulk` is the default

The variant `Serial` disappears, so every crate that names it changes in this
one commit.

> **As executed** (718a55d, fcf2f54, e2ed4ff): the rejection message became
> "serial was renamed incremental in #107 (the per-tuple build); the default
> is now bulk (Algorithm 2)" after review, superseding the text below; a
> test-only `BULK_BUILDS` record and `each_build_mode_runs_its_own_build`
> pin which build each mode runs (the two build the identical trie, so
> nothing else can); the sed was scoped to `kermit` and `kermit-ds`; the
> struct doc names `insert_at` without an intra-doc link (it is
> `pub(super)`).

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/build_mode.rs`
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (mode arms;
  `from_tuples_with_config`; the struct and `insert_at` docs; the
  `expect(dead_code)` on `from_tuples_in_bulk`)
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs` (the `expect(dead_code)` on
  `mod bulk;`)
- Modify: `kermit-ds/src/ds/hash_trie/{radix,parallel,identity}.rs` (renames)
- Modify: `kermit-ds/tests/hash_trie_tests.rs`, `kermit-ds/tests/parquet_tests.rs`
  (an `incremental` alias)
- Modify: `kermit/src/options.rs`, `kermit/src/execution.rs`,
  `kermit/tests/cli_hash_trie_build_mode.rs`,
  `kermit/tests/cli_column_trie_build_mode.rs` (renamed values)

- [ ] **Step 1: Update `build_mode.rs`'s tests to the new names (they fail).**
  In `mod tests`:
  - `axis_values_round_trip_through_from_str`: `let mut modes =
    vec![HashTrieBuildMode::Serial];` → `let mut modes =
    vec![HashTrieBuildMode::Bulk, HashTrieBuildMode::Incremental];`.
  - `axis_values_and_default_are_pinned`: its doc becomes "The labels name
    every HashTrie report's `ds_build_mode`. kermit-lab reads a missing axis,
    and the pre-#107 `"serial"`, as `"incremental"`."; replace
    `assert_eq!(HashTrieBuildMode::Serial.axis_value(), "serial");` with
    ```rust
        assert_eq!(HashTrieBuildMode::Bulk.axis_value(), "bulk");
        assert_eq!(HashTrieBuildMode::Incremental.axis_value(), "incremental");
    ```
    and the default assertion with
    `assert_eq!(HashTrieBuildMode::default(), HashTrieBuildMode::Bulk);`.
  - `malformed_modes_are_rejected_with_the_accepted_forms`: replace the rows
    `("bulk", "unknown hash-trie build mode \"bulk\"")` and
    `("Serial", "unknown hash-trie build mode \"Serial\"")` with
    ```rust
            ("serial", "hash-trie has no serial build since #107"),
            ("Bulk", "unknown hash-trie build mode \"Bulk\""),
    ```
    and the accepted-forms assertion string with
    `"expected bulk, incremental, radix:<bits>, parallel:<threads> or presized:<threads>"`.
  - Add:
    ```rust
    /// `serial` named the per-tuple build until #107; its rejection names
    /// the two builds that replaced the name.
    #[test]
    fn serial_is_rejected_with_its_replacements() {
        let msg = "serial".parse::<HashTrieBuildMode>().unwrap_err().to_string();
        assert!(msg.contains("the default is bulk (Algorithm 2)"), "{msg}");
        assert!(msg.contains("the per-tuple build is incremental"), "{msg}");
    }
    ```

- [ ] **Step 2: Run them to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib build_mode::tests
  ```
  Expected: compile error, `no variant named Bulk`.

- [ ] **Step 3: The variants, parser and labels.** In `build_mode.rs`:
  - Replace the `Serial` variant (with its doc) by:
    ```rust
    /// Algorithm 2 of the paper (VLDB 2020, §3.2.2): a table's tuples are
    /// grouped into its buckets, then each bucket's child is built from its
    /// list (`bulk.rs`, #107). The default.
    #[default]
    Bulk,
    /// One `insert_at` per tuple, in input order: the only build before
    /// #91, and the default, spelled `serial`, until #107.
    Incremental,
    ```
  - The enum's doc: "`Bulk`, `Incremental`, `Radix` and `Parallel` also put
    each key in the same bucket" (replacing "`Serial`, `Radix` and
    `Parallel`"), and "(issues #91, #94, #107)".
  - The `FromStr` doc: "returns: `bulk`, `incremental`, `radix:<bits>`,
    `parallel:<threads>` and `presized:<threads>`."
  - The `error` closure's clause:
    `"{why}; expected bulk, incremental, radix:<bits>, parallel:<threads> or presized:<threads>; bits in {}..={}, threads in 1..={}"`.
  - Replace `| None if s == "serial" => Ok(Self::Serial),` by
    ```rust
            | None if s == "bulk" => Ok(Self::Bulk),
            | None if s == "incremental" => Ok(Self::Incremental),
            | None if s == "serial" => Err(error(
                "hash-trie has no serial build since #107: the default is bulk (Algorithm 2), \
                 and the per-tuple build is incremental"
                    .to_owned(),
            )),
    ```
  - In `axis_value`, replace the `Serial` arm by
    ```rust
            | Self::Bulk => "bulk".to_owned(),
            | Self::Incremental => "incremental".to_owned(),
    ```

- [ ] **Step 4: Route the modes.** In `implementation.rs`:
  - `ConfigurableRelation::from_tuples_with_config` →
    `Self::from_tuples_in_bulk(header, config, tuples)`.
  - Delete the two `#[cfg_attr(not(test), expect(dead_code, reason = "tests
    reach the bulk build until it is the default build"))]` attributes Task 2
    added: on `mod bulk;` in `mod.rs`, and on `from_tuples_in_bulk`. The bulk
    build is now reached outside tests, so the expectations would go
    unfulfilled and fail clippy under `-Dwarnings` (plain `cargo test` only
    warns).
  - In `from_tuples_with_config_and_build_mode`, replace the `Serial` arm by
    ```rust
            | HashTrieBuildMode::Bulk => Self::from_tuples_in_bulk(header, config, tuples),
            | HashTrieBuildMode::Incremental => {
                Self::from_tuples_incrementally(header, config, tuples)
            },
    ```
    and in its doc replace "`Serial` is [`ConfigurableRelation::from_tuples_with_config`],
    unchanged;" with "`Bulk` is [`ConfigurableRelation::from_tuples_with_config`]
    (Algorithm 2); `Incremental` is the per-tuple build;".
  - The struct doc's `# Construction`: replace "Both funnel through `insert`
    for a single tuple, faithful to Algorithm 2 from the paper." with
    "`from_tuples` builds by Algorithm 2 of the paper (`bulk.rs`): a table's
    tuples are grouped into its buckets, then each bucket's child is built
    from its list. `insert` places one tuple by
    [`insert_at`](Self::insert_at), which the `incremental` build mode runs
    once per tuple; both build the identical trie." In the same section's
    list of BuildModes, name `incremental` first.
  - `insert_at`'s doc: replace its first two sentences ("Insert one tuple at
    the appropriate depth. Algorithm 2 of the paper, plus …") with "Insert
    one tuple at the appropriate depth, descending at once: the per-tuple
    build (`incremental`) and `Relation::insert`. The paper's build,
    Algorithm 2, groups before it recurses (`bulk.rs`); under every config
    both builds accept, they build the identical trie. The singleton-pruning
    extension of §3.3.1 (Figure 5) applies when the policy `P` enables it."
    Keep the rest of the doc.

- [ ] **Step 5: The workspace rename.**
  ```bash
  git -C $WT grep -l 'HashTrieBuildMode::Serial' | sed "s|^|$WT/|" | xargs sed -i 's/HashTrieBuildMode::Serial/HashTrieBuildMode::Bulk/g'
  ```
  Then, by hand:
  - `kermit-ds/src/ds/hash_trie/radix.rs`: the tests `radix_builds_the_serial_trie_under_{siphash,fxhash,colliding_hashes}`
    become `radix_builds_the_bulk_trie_under_…`; `check_identity`'s doc:
    "`radix:K` builds the trie `bulk` builds"; the module doc's "The result
    is the trie the serial build makes" → "the bulk build makes".
  - `kermit-ds/src/ds/hash_trie/parallel.rs`: the tests
    `parallel_builds_the_serial_trie_*` become `parallel_builds_the_bulk_trie_*`;
    in `build_modes_reach_their_builds`, after the `Bulk` row add
    `(HashTrieBuildMode::Incremental, vec![], vec![]),`.
  - `kermit-ds/src/ds/hash_trie/identity.rs` module doc: "the trie the
    serial build does" → "the trie the bulk build does".
  - `kermit/src/options.rs`:
    - the `BuildChoices` field doc's hash-trie clause:
      `` `hash-trie=bulk|incremental|radix:<bits>|parallel:<threads>|presized:<threads>` (default `bulk`; bits in 1..=16, threads in 1..=1024; `presized` requires `--ds-config root-capacity=tuples`) ``;
    - `build_choices_resolve_keyed_pairs_per_structure`: `build(&["hash-trie=serial"])` → `build(&["hash-trie=bulk"])`;
    - `build_choices_reject_malformed_pairs`:
      `(&["incremental"], "did you mean --ds-build column-trie=incremental?")` →
      `(&["incremental"], "did you mean --ds-build column-trie=incremental or hash-trie=incremental?")`;
      `(&["serial"], "did you mean --ds-build tree-trie=serial or hash-trie=serial?")` →
      `(&["serial"], "did you mean --ds-build tree-trie=serial?")`;
      `(&["hash-trie=serial", "hash-trie=radix:4"], "hash-trie given more than once")` →
      `(&["hash-trie=bulk", "hash-trie=radix:4"], "hash-trie given more than once")`;
      `(&["hash-trie=bulk"], "unknown hash-trie build mode \"bulk\"")` →
      `(&["hash-trie=serial"], "hash-trie has no serial build since #107")`;
      and add `(&["bulk"], "did you mean --ds-build column-trie=bulk or hash-trie=bulk?"),`;
    - `build_choices_resolve_to_each_structures_labels`'s `TABLE`, HashTrie
      row: `"serial"` → `"bulk", "incremental"`.
  - `kermit/src/execution.rs`: the assertion on
    `HashTrieFamily::<SipHashStrategy, NoPruning, EagerExpansion>::default().build_mode_axes()`
    expects `build_mode_axis("bulk")`.
  - `kermit/tests/cli_hash_trie_build_mode.rs`: `cli_bench_ds_hash_trie_records_serial_build_mode`
    becomes `cli_bench_ds_hash_trie_records_bulk_build_mode`, expecting
    `"bulk"`; the module doc's "`serial` by default" → "`bulk` by default
    (`incremental` is the per-tuple build)".
  - `kermit/tests/cli_column_trie_build_mode.rs`: in both sweep tests, split
    the `| Some("TreeTrie" | "HashTrie")` arm into
    ```rust
            | Some("TreeTrie") => assert_eq!(axes["ds_build_mode"], "serial", "{axes}"),
            | Some("HashTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
    ```

- [ ] **Step 6: An `incremental` alias in the structure suites.** In
  `kermit-ds/tests/hash_trie_tests.rs`, after the radix block:
  ```rust
  // ── BuildMode: the per-tuple build ──────────────────────────────────────
  // `incremental` is the build before #107, one `insert_at` per tuple. It
  // builds the trie `bulk` builds, so the contract holds unchanged.
  define_build_mode_provider!(
      HashIncremental,
      HashTrieBuildMode,
      HashTrieBuildMode::Incremental
  );

  type HashTrieSipIncremental = BuiltWith<HashTrieSip, HashIncremental>;
  type HashTrieSipPrunedLazyIncremental = BuiltWith<HashTrieSipPrunedLazy, HashIncremental>;

  hash_trie_test_suite!(HashTrieSipIncremental, SipHashStrategy);

  hash_trie_test_suite!(HashTrieSipPrunedLazyIncremental, SipHashStrategy);
  ```
  In `kermit-ds/tests/parquet_tests.rs`, after the radix block:
  ```rust
  // …and under the per-tuple build, which must load the same trie (#107).
  define_build_mode_provider!(
      HashIncremental,
      HashTrieBuildMode,
      HashTrieBuildMode::Incremental
  );

  type HashTrieSipIncremental = BuiltWith<HashTrieSip, HashIncremental>;

  fn sorted_tuples_incremental(relation: &HashTrieSipIncremental) -> Vec<Vec<usize>> {
      sorted_tuples(relation)
  }

  parquet_test_suite!(HashTrieSipIncremental, sorted_tuples_incremental);
  ```

- [ ] **Step 7: Run everything this touched.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit options:: execution::
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_hash_trie_build_mode --test cli_column_trie_build_mode
  git -C $WT grep -n -E '"serial"|`serial`|hash-trie=serial' -- kermit kermit-ds
  ```
  Expected: all pass. The grep may list only TreeTrie sites, the `serial`
  rejection, its tests, and doc lines Task 12 rewrites. Fix any other
  HashTrie site now.

- [ ] **Step 8: Commit.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add -u kermit-ds kermit
  git -C $WT commit -m "feat(hash-trie): bulk (Algorithm 2) is the default build, incremental the per-tuple one (#107)"
  ```
  (`add -u` stages only tracked files this task changed; check
  `git -C $WT status --short` shows nothing else first.)

---

## Task 4 [P1]: Lazy expansion through `build`

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (`resolve`; remove
  `expand_level`; the struct invariant that names it)

- [ ] **Step 1: The test already exists.** The `lazy_expansion` trace tests in
  `kermit-ds/tests/hash_trie_tests.rs` require an expanded child to walk
  exactly like the eager table at its position. Run them once for the
  baseline:
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --test hash_trie_tests lazy_expansion
  ```
  Expected: 8 passed.

- [ ] **Step 2: Implement.** Delete `expand_level` and replace `resolve`'s
  `Unexpanded` arm:
  ```rust
            | HashTrieNode::Unexpanded(pending) => pending.expand(|tuples| {
                // One level of Algorithm 2: the child's own children come out
                // unexpanded (or pruned), as the paper's lazy expansion builds
                // a nested table only when a probe first reaches it.
                Self::build(
                    depth,
                    self.header.arity(),
                    tuples,
                    INITIAL_LOG2_CAPACITY,
                    self.config,
                )
            }),
  ```
  `resolve`'s doc keeps its text; add "The table is built by Algorithm 2
  (`bulk.rs`) from the pending list, which keeps insertion order, so it is the
  eager table at this position." Now that it is true, `child`'s doc in
  `bulk.rs` names it: "(§3.3.1; `HashTrie::resolve` builds its table by
  [`build`](Self::build) on the first probe)". In the struct's `# Invariants`, "(see
  `expand_level`)" → "(see `resolve`)". If `INITIAL_LOG2_CAPACITY` is no
  longer imported in `implementation.rs`, keep its import (`make_root` uses it).

- [ ] **Step 3: Run.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds
  ```
  Expected: all pass, the trace tests and `bulk_builds_the_incremental_trie_*`
  included.

- [ ] **Step 4: Commit, then a mutant.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit-ds/src/ds/hash_trie/implementation.rs
  git -C $WT commit -m "refactor(hash-trie): lazy expansion builds its level by Algorithm 2 (#107)"
  ```
  **M4, an expanded child at the wrong size:** `INITIAL_LOG2_CAPACITY` →
  `INITIAL_LOG2_CAPACITY + 1` in `resolve`. Fails:
  `sip_lazy_walks_like_eager` (the keys walk in another order).

---

## Task 5 [P1]: `radix:K` and `parallel:N` build their scratch roots by Algorithm 2

> **As executed** (b136e9d, d0d45b1): `HashTrie::make_root` lost its last
> caller (the old scratch root) and was deleted, with its doc moved to
> `make_root_sized`; `implementation.rs` no longer imports
> `INITIAL_LOG2_CAPACITY` (Task 4's note to keep it is superseded). The
> exact-merge path's docs and test bindings call the reference build `bulk`.

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/radix.rs` (`fill_root`, `build_scratch_root`, module doc)
- Modify: `kermit-ds/src/ds/hash_trie/parallel.rs` (`fill_root`, `fill_root_in_morsels`, module doc, one test call)
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (the `Radix` and `Parallel` arms)

- [ ] **Step 1: The tests already exist.** `radix_builds_the_bulk_trie_*`,
  `parallel_builds_the_bulk_trie_*`, `build_modes_reach_their_builds` and
  `parallel_build_merges_under_every_root_capacity` check the result
  array for array against `bulk`. They pass now, with the scratch roots built by
  `insert_at`, and must still pass.

- [ ] **Step 2: Implement `radix.rs`.**
  - Imports: add `bulk::TupleList` and `config::HashTrieConfig` to the
    `super::{…}` list.
  - `fill_root`'s last parameter `load_factor: LoadFactor` → `config:
    HashTrieConfig`; inside, `build_scratch_root::<H, P, E>(partition, arity,
    config)`, and the final `insert_in_first_appearance_order(…,
    config.load_factor)` (both arms).
  - Replace `build_scratch_root`:
    ```rust
    /// Builds one partition into a scratch root of the real root's kind by
    /// Algorithm 2 (`bulk.rs`): the partition's tuples grouped at the root,
    /// recording for each key the input index of the tuple that introduced it
    /// and the key's hash, in arrival order; then each key's child built from
    /// its list, as the bulk build builds it.
    pub(super) fn build_scratch_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
        partition: impl IntoIterator<Item = Indexed>, arity: usize, config: HashTrieConfig,
    ) -> (HashTrieNode<P, E>, Vec<(usize, u64)>) {
        // Lines 3–7 at the root. The scratch root starts at 4 buckets and
        // grows: its entries move into the real root, so its size is never
        // seen.
        let mut lists: HashTable<TupleList> = HashTable::new();
        let mut first_seen = Vec::new();
        for (index, tuple) in partition {
            let hash = H::hash(tuple[0]);
            let keys_before = lists.len();
            lists
                .entry_or_insert_with(hash, config.load_factor, Vec::new)
                .push(tuple);
            if lists.len() > keys_before {
                first_seen.push((index, hash));
            }
        }
        // Lines 8–15.
        let scratch = HashTrie::<H, P, E>::build_nested(0, arity, lists, config);
        (scratch, first_seen)
    }
    ```
  - The module doc: "build each partition into a scratch root" → "group each
    partition into a scratch root and build its children by Algorithm 2";
    "each root key's subtrie is built by the same `insert_at` calls, on the
    same tuples in the same order, as in the serial build" → "each root key's
    child is built from the same list, in input order, as in the bulk build".

- [ ] **Step 3: Implement `parallel.rs`.**
  - Add `config::HashTrieConfig` to the imports (keep `LoadFactor`:
    `merge_in_first_appearance_order` takes one).
  - `fill_root` and `fill_root_in_morsels`: the last parameter `load_factor:
    LoadFactor` → `config: HashTrieConfig`; `radix::build_scratch_root::<H,
    P, E>(partition.into_tuples(), arity, config)`; both
    `merge_in_first_appearance_order(table, lists, config.load_factor)`.
  - In the test `check_identity`, the morsels-of-7 call passes `config`
    instead of `config.load_factor`.
  - The module doc's step 2: "builds each non-empty partition into a scratch
    root by the serial build's own `insert_at`" → "groups each non-empty
    partition into a scratch root and builds its children, as `bulk` does
    (`radix::build_scratch_root`)"; "each subtrie receives the serial build's
    `insert_at` calls" → "each child is built from the list `bulk` builds it
    from".

- [ ] **Step 4: The constructor arms.** In
  `from_tuples_with_config_and_build_mode`, the `Radix` and `Parallel` arms
  pass `config` instead of `config.load_factor`.

- [ ] **Step 5: Run.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib hash_trie
  ```
  Expected: all pass.

- [ ] **Step 6: Commit, then a mutant.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit-ds/src/ds/hash_trie/radix.rs kermit-ds/src/ds/hash_trie/parallel.rs kermit-ds/src/ds/hash_trie/implementation.rs
  git -C $WT commit -m "refactor(hash-trie): radix and parallel scratch roots by Algorithm 2 (#107)"
  ```
  **M5, a scratch list that starts too big:** in `build_scratch_root`,
  `Vec::new` → `|| Vec::with_capacity(1)`. Fails:
  `radix_builds_the_bulk_trie_under_siphash` (`chain capacity` at arity 1,
  where the scratch lists are the chains).

---

## Task 6 [P1]: The presized fold: group in runs, then children in runs

> **As executed** (e24ccc9, fbe3fb7): after review, `from_tuples_partitioned`'s
> `fill` receives the root's log2 capacity rather than a root
> (`FnOnce(u32, usize, Vec<Vec<usize>>) -> HashTrieNode`), so `presized:N`
> never allocates a presized root only to drop it; radix and parallel build
> theirs with `make_root_sized` in their closures, and `fill_presized_root(_in)`
> take `log2_capacity` (tests pass `log2`). The docs credit the paper only
> with hash partitioning (§3.3.2) and Algorithm 2; regions, the tail and the
> per-run children are kermit's.

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (`from_tuples_partitioned`
  fills by value; the three partitioned arms; delete
  `insert_at_leaf_root_in_run`, `insert_at_inner_root_in_run`, and the
  now-unused `BucketRun`, `RunEntry` imports)
- Modify: `kermit-ds/src/ds/hash_trie/parallel.rs` (`fill_presized_root`,
  `fill_presized_root_in`, new `push_in_run`; tests)

- [ ] **Step 1: Write the failing test changes in `parallel.rs`.**
  - Replace `check_root_step` and `the_root_step_builds_the_serial_trie_below_the_root`
    (the mirror they guard is going) with:
    ```rust
    /// Grouping alone, without threads: one run over the whole presized
    /// table of lists, the overflow pushed afterwards in input order, then
    /// `build_nested`. The root must be equivalent to bulk's (Amendment 2),
    /// and every child identical.
    fn check_one_run<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for &percent in LOAD_PERCENTS {
                let config = tuples_config(percent);
                for (input, tuples) in inputs(arity) {
                    let label = format!(
                        "{}/{}/{} arity {arity}, load {percent}%, {input}",
                        H::NAME,
                        P::NAME,
                        E::NAME
                    );
                    let bulk = HashTrie::<H, P, E>::from_tuples_with_config(
                        arity.into(),
                        config,
                        tuples.clone(),
                    );
                    let log2 = config.root_log2_capacity(tuples.len());
                    let mut lists: HashTable<TupleList> = HashTable::with_log2_capacity(log2);
                    let tail: Vec<Vec<usize>> = lists.with_runs(1, 1 << log2, |mut runs| {
                        tuples
                            .iter()
                            .cloned()
                            .filter_map(|t| push_in_run::<H>(&mut runs[0], t).err())
                            .collect()
                    });
                    for t in tail {
                        lists
                            .entry_or_insert_with(H::hash(t[0]), config.load_factor, Vec::new)
                            .push(t);
                    }
                    let root = HashTrie::<H, P, E>::build_nested(0, arity, lists, config);
                    assert_equivalent_root(bulk.root(), &root, &label);
                }
            }
        }
    }

    #[test]
    fn one_run_of_grouping_builds_the_bulk_root() {
        check_one_run::<SipHashStrategy, NoPruning, EagerExpansion>();
        check_one_run::<SipHashStrategy, SingletonPruning, EagerExpansion>();
        check_one_run::<SipHashStrategy, NoPruning, LazyExpansion>();
        check_one_run::<SipHashStrategy, SingletonPruning, LazyExpansion>();
        check_one_run::<FxHashStrategy, SingletonPruning, LazyExpansion>();
        check_one_run::<Mod10HashStrategy, SingletonPruning, EagerExpansion>();
    }
    ```
    Add `bulk::TupleList` and `hash_table::HashTable` to the test module's
    `use` list if `super::*` does not bring them.
  - `check_presized`: replace
    ```rust
                        let mut small = HashTrie::<H, P, E>::make_root_sized(arity, log2);
                        fill_presized_root_in::<H, P, E>(
                            &mut small,
                            arity,
                            tuples.clone(),
                            threads(t),
                            7,
                            8,
                            config.load_factor,
                        );
    ```
    with
    ```rust
                        let small = fill_presized_root_in::<H, P, E>(
                            HashTrie::<H, P, E>::make_root_sized(arity, log2),
                            arity,
                            tuples.clone(),
                            threads(t),
                            7,
                            8,
                            config,
                        );
    ```
  - `keys_that_cannot_fit_their_region_go_to_the_tail`: the same change,
    `let root = fill_presized_root_in::<EndOfRegionHash, NoPruning,
    EagerExpansion>(HashTrie::<EndOfRegionHash>::make_root_sized(2, 5), 2,
    tuples, threads(2), 7, 8, config);` (drop the `let mut root` line).
  - `presized_build_reaches_its_own_path`: its doc becomes "three worker runs
    (scatter, the regions' lists, then the regions' children)", and it expects
    `vec![3, 3, 3]`.

  - In the presized tests (`check_presized`, `keys_that_cannot_fit_their_region_go_to_the_tail`
    and the rest of the presized block), rename the `serial` bindings to
    `bulk` and "equivalent to serial" / "serial's" in their docs and labels
    to `bulk`, as Task 3 did for the exact-build tests.

- [ ] **Step 2: Run them to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib hash_trie::parallel
  ```
  Expected: compile errors (`push_in_run` not found, wrong argument types to
  `fill_presized_root_in`).

- [ ] **Step 3: `from_tuples_partitioned` fills by value.** In
  `implementation.rs`:
  ```rust
    /// What the partitioned builds share (`radix:K`, `parallel:N`,
    /// `presized:N`): the per-tuple build's arity check, with its message, then
    /// `fill` on the empty root (presized under `root-capacity=tuples`, as
    /// the bulk build's is), which returns the filled root, then the multiset
    /// count the other builds keep.
    fn from_tuples_partitioned(
        header: RelationHeader, config: HashTrieConfig, tuples: Vec<Vec<usize>>,
        fill: impl FnOnce(HashTrieNode<P, E>, usize, Vec<Vec<usize>>) -> HashTrieNode<P, E>,
    ) -> Self {
        let arity = header.arity();
        Self::assert_arities(arity, &tuples);
        let tuple_count = tuples.len();
        let mut trie = Self::with_config_for(header, config, tuple_count);
        trie.root = fill(trie.root, arity, tuples);
        trie.tuple_count = tuple_count;
        trie
    }
  ```
  and the three arms (keep the `Presized` arm's prerequisite `assert_eq!` as
  dependent-optimisations wrote it):
  ```rust
            | HashTrieBuildMode::Radix(bits) => {
                Self::from_tuples_partitioned(header, config, tuples, |mut root, arity, tuples| {
                    radix::fill_root::<H, P, E>(&mut root, arity, tuples, bits, config);
                    root
                })
            },
            | HashTrieBuildMode::Parallel(threads) => {
                Self::from_tuples_partitioned(header, config, tuples, |mut root, arity, tuples| {
                    parallel::fill_root::<H, P, E>(&mut root, arity, tuples, threads, config);
                    root
                })
            },
            | HashTrieBuildMode::Presized(threads) => {
                // (the existing prerequisite assert_eq!, unchanged)
                Self::from_tuples_partitioned(header, config, tuples, |root, arity, tuples| {
                    parallel::fill_presized_root::<H, P, E>(root, arity, tuples, threads, config)
                })
            },
  ```
  Delete `insert_at_leaf_root_in_run` and `insert_at_inner_root_in_run`, and
  drop `BucketRun, RunEntry` from the `hash_table::{…}` import.

- [ ] **Step 4: The presized fill in `parallel.rs`.** Imports: `hash_table::{home_bucket,
  BucketRun, HashTable, Overflow, RunEntry}` and `bulk::TupleList`. Replace
  `fill_presized_root` and `fill_presized_root_in`, and add `push_in_run`
  after them (`fill_runs` stays as it is):
  ```rust
  /// Fills the presized, empty `root` with `tuples` by `presized:threads`,
  /// the paper's partitioned build
  /// (`docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`),
  /// with every child built by Algorithm 2 (#107), and returns it. Every
  /// tuple must have `arity` attributes; the caller checks.
  pub(super) fn fill_presized_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      root: HashTrieNode<P, E>, arity: usize, tuples: Vec<Vec<usize>>, threads: Threads,
      config: HashTrieConfig,
  ) -> HashTrieNode<P, E> {
      fill_presized_root_in::<H, P, E>(
          root,
          arity,
          tuples,
          threads,
          MORSEL_TUPLES,
          REGION_BUCKETS,
          config,
      )
  }

  /// [`fill_presized_root`] with the morsel and region sizes as parameters,
  /// so tests can cut a small input into many morsels and regions.
  ///
  /// 1. **Partition**: `scatter` by the top bits of each tuple's home bucket,
  ///    so partition k is a contiguous run of regions, in input order.
  /// 2. **Group** (Algorithm 2, lines 4–7): each worker takes a partition with
  ///    its run of a presized table of tuple lists, and pushes every tuple onto
  ///    the list in its bucket. Tuples whose key's probe runs off its region
  ///    are deferred.
  /// 3. **Tail**: the calling thread pushes the deferred tuples in input order,
  ///    with ordinary probing. The paper does not say how a probe crossing a
  ///    partition's end is handled; this is kermit's answer.
  /// 4. **Children** (lines 8–12): each worker builds the children of its
  ///    run's buckets, by the bulk build's `child`. The paper does not say how
  ///    the recursion is spread over threads; one run per worker is kermit's
  ///    choice.
  fn fill_presized_root_in<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      root: HashTrieNode<P, E>, arity: usize, tuples: Vec<Vec<usize>>, threads: Threads,
      morsel_tuples: usize, region_buckets: usize, config: HashTrieConfig,
  ) -> HashTrieNode<P, E> {
      if tuples.is_empty() {
          return root;
      }
      // Only the root's capacity is used: its tuples are grouped into a table
      // of lists of that size, which step 4 turns into the root. The empty
      // root is dropped before that table is allocated.
      let log2 = root.buckets_len().trailing_zeros();
      drop(root);
      let capacity = 1usize << log2;
      let region_buckets = region_buckets.min(capacity);
      let parts = (PARTITIONS_PER_THREAD * threads.get())
          .next_power_of_two()
          .min(capacity / region_buckets);
      let shift = log2 - parts.trailing_zeros();
      // 1. Partition by the home bucket's top bits: whole runs of regions.
      let partitions = scatter(threads, tuples, morsel_tuples, parts, |tuple| {
          home_bucket(H::hash(tuple[0]), log2) >> shift
      });
      #[cfg(any(test, feature = "test-hooks"))]
      let partition_sizes: Vec<usize> = partitions.iter().map(Partition::len).collect();
      // 2. Group each run of regions in parallel.
      let mut lists: HashTable<TupleList> = HashTable::with_log2_capacity(log2);
      let mut deferred = fill_runs(&mut lists, threads, partitions, region_buckets, |run, tuple| {
          push_in_run::<H>(run, tuple)
      });
      #[cfg(any(test, feature = "test-hooks"))]
      PARALLEL_BUILDS.with(|builds| {
          builds.borrow_mut().push(ParallelBuild {
              threads: threads.get(),
              partition_sizes,
              deferred: Some(deferred.len()),
          });
      });
      // 3. The tail, in input order. The table is presized for every tuple, so
      //    it does not grow here.
      deferred.sort_unstable_by_key(|&(position, _)| position);
      for (_, tuple) in deferred {
          lists
              .entry_or_insert_with(H::hash(tuple[0]), config.load_factor, Vec::new)
              .push(tuple);
      }
      // 4. Every bucket's child, a run per worker. At arity 1 the lists are the
      //    chains, and the table of lists is the root. `arity <= 1`, as
      //    `build_nested` decides at depth 0 (`depth + 1 >= arity`).
      if arity <= 1 {
          return HashTrieNode::Leaf(lists);
      }
      HashTrieNode::Inner(lists.map_in_runs(parts, |runs| {
          dispatch(threads, runs, |run| {
              run.map(|list| HashTrie::<H, P, E>::child(1, arity, list, config));
          });
      }))
  }

  /// Algorithm 2's lines 6–7 inside one run of a presized table of lists:
  /// `presized:N`'s root step, the same for every arity. Returns the tuple if
  /// its key's probe ran off its region, for the calling thread to push
  /// afterwards.
  fn push_in_run<H: HashStrategy>(
      run: &mut BucketRun<'_, TupleList>, tuple: Vec<usize>,
  ) -> Result<(), Vec<usize>> {
      match run.entry(H::hash(tuple[0])) {
          | Ok(RunEntry::Occupied(list)) => list.push(tuple),
          | Ok(RunEntry::Vacant(bucket)) => bucket.insert(Vec::new()).push(tuple),
          | Err(Overflow) => return Err(tuple),
      }
      Ok(())
  }
  ```
  The module doc's presized paragraph: "each worker inserts every tuple of its
  partition once, straight into its run of the root" → "each worker pushes
  every tuple of its partition onto its bucket's list in its run of a presized
  table of lists; after the tail, each worker builds the children of its run's
  buckets by Algorithm 2"; "every subtrie is identical" stays.

- [ ] **Step 5: Run.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit execution::
  ```
  Expected: all pass, including every `presized_parallel_builds_are_*` test,
  `keys_that_cannot_fit_their_region_go_to_the_tail` and
  `hash_trie_families_build_with_their_parallel_mode`.

- [ ] **Step 6: Commit, then a mutant.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/parallel.rs
  git -C $WT commit -m "feat(hash-trie): presized:N groups in runs then builds children in runs; the mirrored root steps go (#107)"
  ```
  **M6, the tail out of order:** `deferred.sort_unstable_by_key(|&(position,
  _)| position)` → `deferred.sort_unstable_by_key(|&(position, _)|
  std::cmp::Reverse(position))`. Fails:
  `keys_that_cannot_fit_their_region_go_to_the_tail` (each deferred key's
  chain comes out reversed).

**Checkpoint 1 (controller → user):** report the SHAs, the test delta against
`$SCRATCH/baseline.txt`, the mutants, and the kermit-ds Miri result (run it
now, `setsid nohup` per the ground rules). Before going on, run Miri on
`kermit-ds` and record its time:
```bash
setsid nohup env -C $WT nix develop $WT --command cargo miri test -p kermit-ds > $SCRATCH/miri-p1.log 2>&1 & disown
```

---

# P2 — `kermit-ds`: `child-capacity`

## Task 7 [P2]: `ChildCapacity`, sized children, and `incremental`'s prerequisite

> **As executed** (346603a, ab795e5, 4eb2080): sizing lives in
> `build_child_table` (Task 4); all three bulk-vs-incremental tests use
> `incremental_configs()`. After review: the docs qualify "an expanded child is
> the eager child" (it holds for a trie no `insert` has changed since its build,
> and always under `grow`; under `tuples` a lazily expanded child is sized from
> its pending list, an eagerly inserted one starts at 4); `check_presized` takes
> a `child_capacities` slice (`BOTH_CHILD_CAPACITIES` for the shared inputs),
> and the large presized test runs Sip/NoPruning/Eager × `tuples` and
> Fx/Pruned/Lazy × `grow` (a lazy build leaves children pending, so only the
> eager layout sizes them), keeping `hash_trie`'s lib tests at ~28 s.

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/config.rs`
- Modify: `kermit-ds/src/ds/hash_trie/hash_table.rs` (`log2_capacity_for`'s floor; `PAPER_MIN_LOG2_CAPACITY`; tests)
- Modify: `kermit-ds/src/ds/hash_trie/bulk.rs` (`child` sizes; tests)
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (`resolve` sizes; the `Incremental` assert; `root_capacity_tests::config`)
- Modify: `kermit-ds/src/ds/hash_trie/build_mode.rs` (the `Incremental` doc)
- Modify: `kermit-ds/src/ds/hash_trie/{identity,radix,parallel}.rs` (configs and labels)
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs`, `kermit-ds/src/ds/mod.rs`, `kermit-ds/src/lib.rs` (re-exports)
- Modify: `kermit-ds/tests/hash_trie_tests.rs`, `kermit-ds/tests/parquet_tests.rs`

- [ ] **Step 1: Write the failing tests.**

  In `config.rs`'s tests:
  ```rust
    #[test]
    fn default_child_capacity_is_grow() {
        assert_eq!(HashTrieConfig::default().child_capacity, ChildCapacity::Grow);
        assert_eq!(ChildCapacity::default(), ChildCapacity::Grow);
    }

    /// What a report's `ds_config_child_capacity` says is what
    /// `--ds-config child-capacity=…` parses back.
    #[test]
    fn child_capacity_axis_values_round_trip() {
        assert_eq!(ChildCapacity::Grow.axis_value(), "grow");
        assert_eq!(ChildCapacity::Tuples.axis_value(), "tuples");
        for value in [ChildCapacity::Grow, ChildCapacity::Tuples] {
            assert_eq!(value.axis_value().parse::<ChildCapacity>(), Ok(value));
        }
    }

    #[test]
    fn malformed_child_capacities_name_both_values() {
        for bad in ["", "Grow", "keys", "1024"] {
            let msg = bad.parse::<ChildCapacity>().unwrap_err().to_string();
            assert!(msg.contains("expected grow or tuples"), "{bad:?}: {msg}");
        }
    }

    #[test]
    fn axes_report_child_capacity_as_a_string() {
        let tuples = HashTrieConfig {
            child_capacity: ChildCapacity::Tuples,
            ..HashTrieConfig::default()
        };
        assert!(tuples
            .axes()
            .contains(&("child_capacity", Value::from("tuples"))));
        assert!(HashTrieConfig::default()
            .axes()
            .contains(&("child_capacity", Value::from("grow"))));
    }

    /// Under `grow` a child starts at 4 buckets; under `tuples` it is sized
    /// for its list with the paper's 2-bucket minimum, so a one-tuple list
    /// gets 2 buckets.
    #[test]
    fn child_log2_capacity_is_four_buckets_under_grow_and_sized_under_tuples() {
        let grow = HashTrieConfig::default();
        let tuples = HashTrieConfig {
            child_capacity: ChildCapacity::Tuples,
            ..grow
        };
        for n in [1, 2, 3, 1_000] {
            assert_eq!(grow.child_log2_capacity(n), INITIAL_LOG2_CAPACITY);
            assert_eq!(
                tuples.child_log2_capacity(n),
                log2_capacity_for(n, grow.load_factor, PAPER_MIN_LOG2_CAPACITY)
            );
        }
        assert_eq!(tuples.child_log2_capacity(1), 1);
        // 3 tuples at 70 % need ⌈300 / 70⌉ = 5 buckets: 2^3.
        assert_eq!(tuples.child_log2_capacity(3), 3);
    }
  ```
  (Import `PAPER_MIN_LOG2_CAPACITY` in the test module beside the existing
  `INITIAL_LOG2_CAPACITY` / `log2_capacity_for`.)

  In `hash_table.rs`'s tests, add:
  ```rust
    /// With the paper's 2-bucket minimum, the capacity at an 80 % cap is the
    /// paper's `2^⌈log2(1.25·|L|)⌉` for every list length, one tuple
    /// included.
    #[test]
    fn log2_capacity_for_with_the_papers_minimum_is_the_papers_sizing() {
        let lf = LoadFactor::percent(80).unwrap();
        for keys in 1..2_000usize {
            let paper = (5 * keys).div_ceil(4).next_power_of_two().trailing_zeros();
            assert_eq!(
                log2_capacity_for(keys, lf, PAPER_MIN_LOG2_CAPACITY),
                paper,
                "{keys} keys"
            );
        }
    }
  ```

  In `bulk.rs`'s tests, add (with `crate::{morsel::Threads,
  relation::ConfigurableRelation}`, `ds::hash_trie::{build_mode::{HashTrieBuildMode,
  RadixBits}, config::{ChildCapacity, LoadFactor, RootCapacity}}` in its
  `use`):
  ```rust
    /// The tuples stored below `node`, built or pending.
    fn tuples_below<P: PruningPolicy, E: ExpansionPolicy>(node: &HashTrieNode<P, E>) -> usize {
        match node {
            | HashTrieNode::Inner(table) => table.iter().map(|(_, child)| tuples_below(child)).sum(),
            | HashTrieNode::Leaf(table) => table.iter().map(|(_, chain)| chain.len()).sum(),
            | HashTrieNode::Singleton(_) => 1,
            | HashTrieNode::Unexpanded(pending) => match pending.built() {
                | Some(built) => tuples_below(built),
                | None => pending.pending().len(),
            },
        }
    }

    /// Every table below `node` has the capacity `child_log2_capacity` gives
    /// the tuples below it, expanding lazy children on the way by `resolve`,
    /// as a probe would. A table starts at that capacity and growth only
    /// enlarges it, so equality also shows that it never grew.
    fn assert_children_sized<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
        trie: &HashTrie<H, P, E>, node: &HashTrieNode<P, E>, depth: usize, label: &str,
    ) {
        let HashTrieNode::Inner(table) = node else {
            return;
        };
        for (hash, child) in table.iter() {
            let child = trie.resolve(child, depth + 1);
            if matches!(child, HashTrieNode::Inner(_) | HashTrieNode::Leaf(_)) {
                assert_eq!(
                    child.buckets_len(),
                    1 << trie.config().child_log2_capacity(tuples_below(child)),
                    "{label}: the child at depth {} under {hash:#x}",
                    depth + 1
                );
                assert_children_sized(trie, child, depth + 1, label);
            }
        }
    }

    /// Under `child-capacity=tuples` every mode sizes every child once from
    /// its list, eager or lazy, pruned or not. Miri leaves out the threaded
    /// modes.
    fn check_children_sized<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        let percents: &[u8] = if cfg!(miri) {
            &[70]
        } else {
            &[50, 70, 80]
        };
        for &percent in percents {
            for root_capacity in [RootCapacity::Grow, RootCapacity::Tuples] {
                let config = HashTrieConfig {
                    load_factor: LoadFactor::percent(percent).unwrap(),
                    root_capacity,
                    child_capacity: ChildCapacity::Tuples,
                };
                let mut modes = vec![
                    HashTrieBuildMode::Bulk,
                    HashTrieBuildMode::Radix(RadixBits::new(2).unwrap()),
                ];
                if !cfg!(miri) {
                    modes.push(HashTrieBuildMode::Parallel(Threads::new(2).unwrap()));
                    if root_capacity == RootCapacity::Tuples {
                        modes.push(HashTrieBuildMode::Presized(Threads::new(2).unwrap()));
                    }
                }
                for arity in 1..=3 {
                    for (input, tuples) in inputs(arity) {
                        for &mode in &modes {
                            let trie = HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
                                arity.into(),
                                config,
                                mode,
                                tuples.clone(),
                            );
                            let label = format!(
                                "{} {mode:?}",
                                label::<H, P, E>(arity, config, input)
                            );
                            assert_children_sized(&trie, trie.root(), 0, &label);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tuples_sizes_every_child_from_its_list() {
        check_children_sized::<SipHashStrategy, NoPruning, EagerExpansion>();
        check_children_sized::<SipHashStrategy, SingletonPruning, EagerExpansion>();
        check_children_sized::<SipHashStrategy, NoPruning, LazyExpansion>();
        check_children_sized::<SipHashStrategy, SingletonPruning, LazyExpansion>();
        check_children_sized::<FxHashStrategy, SingletonPruning, LazyExpansion>();
        check_children_sized::<Mod10HashStrategy, NoPruning, EagerExpansion>();
    }

    /// `incremental` creates a child on its first tuple, before its list is
    /// known, so it cannot size it. The CLI rejects the pair first; here it
    /// is a broken invariant, like a wrong arity.
    #[test]
    #[should_panic(expected = "hash-trie=incremental requires child-capacity=grow")]
    fn incremental_requires_growing_children() {
        let _: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            HashTrieConfig {
                child_capacity: ChildCapacity::Tuples,
                ..HashTrieConfig::default()
            },
            HashTrieBuildMode::Incremental,
            vec![vec![1, 2]],
        );
    }
  ```
  `bulk.rs`'s `check_identity` and `…_on_wide_and_skewed_inputs` switch from
  `configs()` to `incremental_configs()` (imported from `identity`), and
  `label` adds `", children {}"` with `config.child_capacity.axis_value()`.

  In `kermit-ds/tests/hash_trie_tests.rs`, `assert_lazy_walks_like_eager`
  gains a second parameter:
  ```rust
    fn assert_lazy_walks_like_eager<H: HashStrategy, P: PruningPolicy>(
        load_factor: u8, child_capacity: ChildCapacity,
    ) {
        let config = HashTrieConfig {
            load_factor: LoadFactor::percent(load_factor).unwrap(),
            child_capacity,
            ..HashTrieConfig::default()
        };
  ```
  Every existing call passes `ChildCapacity::Grow` as the second argument;
  add `ChildCapacity` to the file's `kermit_ds::{…}` import; and add:
  ```rust
    /// Under `child-capacity=tuples` an expanded child is sized from its
    /// pending list, as the eager build sizes it from the same list.
    #[test]
    fn lazy_walks_like_eager_with_children_sized_from_their_lists() {
        assert_lazy_walks_like_eager::<SipHashStrategy, NoPruning>(70, ChildCapacity::Tuples);
        assert_lazy_walks_like_eager::<FxHashStrategy, SingletonPruning>(80, ChildCapacity::Tuples);
        assert_lazy_walks_like_eager::<Mod10HashStrategy, SingletonPruning>(
            90,
            ChildCapacity::Tuples,
        );
    }
  ```
  After the lazy aliases (the `HashTrieSipDenseLazy` line), add:
  ```rust
  // ── Config variant: children sized from their lists ─────────────────────
  // Under `child-capacity=tuples` every table below the root is sized once
  // from its list (#107), so the contract must hold over children sparser
  // than grown ones, eager or lazy.
  define_config_provider!(SizedChildren, HashTrieConfig, HashTrieConfig {
      child_capacity: ChildCapacity::Tuples,
      ..HashTrieConfig::default()
  });

  type HashTrieSipSizedChildren = Configured<HashTrieSip, SizedChildren>;
  type HashTrieSipLazySizedChildren = Configured<HashTrieSipLazy, SizedChildren>;
  type HashTrieMod10PrunedSizedChildren = Configured<HashTrieMod10Pruned, SizedChildren>;
  ```
  and, with the other `hash_trie_test_suite!` lines:
  ```rust
  hash_trie_test_suite!(HashTrieSipSizedChildren, SipHashStrategy);

  hash_trie_test_suite!(HashTrieSipLazySizedChildren, SipHashStrategy);

  hash_trie_test_suite!(HashTrieMod10PrunedSizedChildren, Mod10HashStrategy);
  ```
  In `kermit-ds/tests/parquet_tests.rs`, after the presized-root block:
  ```rust
  // …and with children sized from their lists (#107).
  define_config_provider!(SizedChildren, HashTrieConfig, HashTrieConfig {
      child_capacity: ChildCapacity::Tuples,
      ..HashTrieConfig::default()
  });

  type HashTrieSipSizedChildren = Configured<HashTrieSip, SizedChildren>;

  fn sorted_tuples_sized_children(relation: &HashTrieSipSizedChildren) -> Vec<Vec<usize>> {
      sorted_tuples(relation)
  }

  parquet_test_suite!(HashTrieSipSizedChildren, sorted_tuples_sized_children);
  ```
  (add `ChildCapacity` to that file's `kermit_ds::{…}` import).

- [ ] **Step 2: Run them to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds
  ```
  Expected: compile errors, `ChildCapacity` not found.

- [ ] **Step 3: The Config.** In `config.rs`, after `impl FromStr for
  RootCapacity`, add:
  ```rust
  /// How large a build makes each table below the root (#107).
  ///
  /// A value, not a shape: it replaces a child table's starting capacity, the
  /// constant 4 buckets, on the path every `bulk` build takes, and is read
  /// once per child. The root's is [`RootCapacity`].
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
  pub enum ChildCapacity {
      /// Start at 4 buckets and double as keys arrive: the only behaviour
      /// before #107.
      #[default]
      Grow,
      /// Size each child once, from the number of tuples in the list it is
      /// built from, so that it never grows during the build: Algorithm 2,
      /// line 3, with the paper's own minimum of 2 buckets. Only a build that
      /// groups before it recurses knows that number, so the `incremental`
      /// build rejects this value. A child that `Relation::insert` creates
      /// after the build starts at 4 buckets.
      Tuples,
  }

  impl ChildCapacity {
      /// The value the bench axis reports and `--ds-config child-capacity=`
      /// parses.
      pub fn axis_value(self) -> &'static str {
          match self {
              | Self::Grow => "grow",
              | Self::Tuples => "tuples",
          }
      }
  }

  /// A string that names no [`ChildCapacity`]. Its message names both values.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct ParseChildCapacityError(String);

  impl fmt::Display for ParseChildCapacityError {
      fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
          write!(f, "expected grow or tuples, got {:?}", self.0)
      }
  }

  impl std::error::Error for ParseChildCapacityError {}

  /// Parses the strings [`ChildCapacity::axis_value`] returns.
  impl FromStr for ChildCapacity {
      type Err = ParseChildCapacityError;

      fn from_str(s: &str) -> Result<Self, Self::Err> {
          match s {
              | "grow" => Ok(Self::Grow),
              | "tuples" => Ok(Self::Tuples),
              | other => Err(ParseChildCapacityError(other.to_owned())),
          }
      }
  }
  ```
  `HashTrieConfig` gains, after `root_capacity`:
  ```rust
      /// How large a build makes every table below the root. Bench axis
      /// `ds_config_child_capacity` (`"grow"` or `"tuples"`).
      pub child_capacity: ChildCapacity,
  ```
  `root_log2_capacity`'s `Tuples` arm becomes
  `log2_capacity_for(tuple_count, self.load_factor, INITIAL_LOG2_CAPACITY)`,
  and after it:
  ```rust
      /// A child's log2 capacity for a build that gives it a list of
      /// `list_len` tuples: 4 buckets under [`ChildCapacity::Grow`], and under
      /// [`ChildCapacity::Tuples`] the smallest capacity, of at least 2
      /// buckets, at which `list_len` keys never make it grow. At a load factor
      /// of 0.8 that is the paper's `2^⌈log2(1.25·|L|)⌉` exactly (Algorithm 2,
      /// line 3), one-tuple lists included.
      pub(crate) fn child_log2_capacity(self, list_len: usize) -> u32 {
          match self.child_capacity {
              | ChildCapacity::Grow => INITIAL_LOG2_CAPACITY,
              | ChildCapacity::Tuples => {
                  log2_capacity_for(list_len, self.load_factor, PAPER_MIN_LOG2_CAPACITY)
              },
          }
      }
  ```
  `ConfigOption::axes` gains
  `("child_capacity", Value::from(self.child_capacity.axis_value())),`.
  Import `PAPER_MIN_LOG2_CAPACITY` beside `log2_capacity_for`.

- [ ] **Step 4: The floor.** In `hash_table.rs`, after `INITIAL_LOG2_CAPACITY`:
  ```rust
  /// The smallest log2 capacity the paper's sizing gives a table: 2 buckets,
  /// for one tuple (`2^⌈log2(1.25)⌉`). Children sized from their lists start
  /// here; the root keeps [`INITIAL_LOG2_CAPACITY`] (#88).
  pub(crate) const PAPER_MIN_LOG2_CAPACITY: u32 = 1;
  ```
  `log2_capacity_for` takes `min_log2: u32` as its third parameter and ends
  `.max(min_log2)` instead of `.max(INITIAL_LOG2_CAPACITY)`; its doc's first
  line becomes "The smallest log2 capacity, at least `min_log2`, at which a
  table holds `keys` keys without its load-factor cap firing", and "except
  that it never goes below 4 buckets" becomes "except below `min_log2`". The
  existing tests that call it pass `INITIAL_LOG2_CAPACITY` as the third
  argument; `a_presized_table_never_grows_for_its_keys` loops `for min_log2
  in [PAPER_MIN_LOG2_CAPACITY, INITIAL_LOG2_CAPACITY]` around its key loop.
  Find every call with `git -C $WT grep -n 'log2_capacity_for(' -- kermit-ds`.

- [ ] **Step 5: Sizing in the build.**
  - `bulk.rs`, the body of `build_child_table` (Task 4's fix-loop added it as
    the one definition of a child's table, called by `child`'s eager branch
    and by `resolve`, so this is the only sizing site):
    ```rust
        let log2_capacity = config.child_log2_capacity(list.len());
        Self::build(depth, arity, list, log2_capacity, config)
    ```
    and drop `INITIAL_LOG2_CAPACITY` from its imports. In `child`'s doc, the
    lazy parenthesis names the function `resolve` calls: "(§3.3.1;
    `HashTrie::resolve` builds its table by
    [`build_child_table`](Self::build_child_table) on the first probe)". In
    the module doc,
    replace the second bullet with: "Line 3's size is a Config value: the
    root's is `root-capacity` (#88), and each child's is `child-capacity`:
    4 buckets that grow (`grow`, the default), or sized once from its list
    (`tuples`), the paper's sizing at a load factor of 0.8."
  - `implementation.rs`, `resolve`: no change; it calls `build_child_table`,
    which now sizes.
  - `implementation.rs`, the `Incremental` arm:
    ```rust
            | HashTrieBuildMode::Incremental => {
                // The prerequisite: `insert_at` creates a child on its first
                // tuple, before the child's list is known, so it cannot size
                // it. The CLI rejects the pair first; here it is a broken
                // invariant, like a wrong arity.
                assert_eq!(
                    config.child_capacity,
                    ChildCapacity::Grow,
                    "hash-trie=incremental requires child-capacity=grow; got child-capacity={}",
                    config.child_capacity.axis_value(),
                );
                Self::from_tuples_incrementally(header, config, tuples)
            },
    ```
    (import `ChildCapacity` from `super::config`).
  - `build_mode.rs`, the `Incremental` doc gains: "It creates a child on its
    first tuple, before the child's list is known, so it **requires
    `child-capacity=grow`**; the constructor panics without it, and the CLI
    rejects it first."

- [ ] **Step 6: Every config in the identity tests.**
  - `identity.rs`:
    ```rust
    /// The configs a build-mode identity test runs under: each load factor in
    /// [`LOAD_PERCENTS`] with each root capacity and each child capacity.
    /// Every build mode must build the same trie under every config (#88,
    /// #107).
    pub(super) fn configs() -> Vec<HashTrieConfig> {
        let mut configs = Vec::new();
        for &percent in LOAD_PERCENTS {
            for root_capacity in [RootCapacity::Grow, RootCapacity::Tuples] {
                for child_capacity in [ChildCapacity::Grow, ChildCapacity::Tuples] {
                    configs.push(HashTrieConfig {
                        load_factor: LoadFactor::percent(percent).unwrap(),
                        root_capacity,
                        child_capacity,
                    });
                }
            }
        }
        configs
    }

    /// The [`configs`] the `incremental` build accepts: children that grow.
    pub(super) fn incremental_configs() -> Vec<HashTrieConfig> {
        configs()
            .into_iter()
            .filter(|config| config.child_capacity == ChildCapacity::Grow)
            .collect()
    }
    ```
  - `radix.rs`, `check_identity`'s label gains `", children {}"` with
    `config.child_capacity.axis_value()`.
  - `parallel.rs`:
    - `check_identity`: replace the `for root_capacity in […] { for &percent
      in LOAD_PERCENTS { let config = HashTrieConfig { … };` header (and its
      two closing braces) with `for config in configs() {`, and build the
      label from `config` (`config.root_capacity.axis_value()`,
      `config.load_factor.numerator()`, `config.child_capacity.axis_value()`);
      import `configs` from `identity`.
    - `tuples_config` takes the child capacity:
      ```rust
      fn tuples_config(percent: u8, child_capacity: ChildCapacity) -> HashTrieConfig {
          HashTrieConfig {
              load_factor: LoadFactor::percent(percent).unwrap(),
              root_capacity: RootCapacity::Tuples,
              child_capacity,
          }
      }
      ```
      `check_presized` and `check_one_run` loop `for child_capacity in
      [ChildCapacity::Grow, ChildCapacity::Tuples]` around their `config` and
      add it to their labels. The other callers
      (`keys_that_cannot_fit_their_region_go_to_the_tail`,
      `presized_build_reaches_its_own_path`) pass `ChildCapacity::Grow`.
  - `implementation.rs`, `root_capacity_tests::config` adds
    `child_capacity: ChildCapacity::Grow,` to its literal.
  - Re-exports: in `hash_trie/mod.rs`, `config::{ChildCapacity,
    HashTrieConfig, InvalidLoadFactor, LoadFactor, ParseChildCapacityError,
    ParseRootCapacityError, RootCapacity}`; in `ds/mod.rs` and `lib.rs` add
    `ChildCapacity` and `ParseChildCapacityError` beside `RootCapacity` and
    `ParseRootCapacityError`.
  - The two key-list tests in `implementation.rs`
    (`optimization_axes_default_strategy_reports_sip`,
    `optimization_axes_fxhash_strategy_reports_fxhash`) list
    `"ds_config_child_capacity"` first in their expected `keys`.

- [ ] **Step 7: Run.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo check --workspace --all-targets
  ```
  Expected: all pass, and the workspace compiles. The `kermit` crate uses
  `..HashTrieConfig::default()` in its literals, so it needs no change; fix
  any literal the check reports by adding that.

- [ ] **Step 8: Commit, then mutants.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add -u kermit-ds
  git -C $WT commit -m "feat(hash-trie): child-capacity=grow|tuples sizes every child from its list (#107)"
  ```
  - **M7a, children ignore the config:** `config.child_log2_capacity(list.len())`
    → `INITIAL_LOG2_CAPACITY` in `build_child_table`. Fails:
    `tuples_sizes_every_child_from_its_list`.
  - **M7b, expansion bypasses the shared definition:** in `resolve`, call
    `Self::build(depth, self.header.arity(), tuples, INITIAL_LOG2_CAPACITY,
    self.config)` instead of `build_child_table`. Fails:
    `lazy_walks_like_eager_with_children_sized_from_their_lists` and the lazy
    cases of `tuples_sizes_every_child_from_its_list`.
  - **M7c, the prerequisite dropped:** delete the `assert_eq!` in the
    `Incremental` arm. Fails: `incremental_requires_growing_children`.

**Checkpoint 2 (controller → user):** SHAs, the test delta, the mutants, and
kermit-ds Miri with its time against Checkpoint 1's. If Miri exceeds 45
minutes locally, propose which matrix to trim before going on.

---

# P3 — the `kermit` binary

## Task 8 [P3]: The `--ds-config` key and the `Prerequisite` row

**Files:**
- Modify: `kermit/src/options.rs`

- [ ] **Step 1: Write the failing tests.** In `options.rs`'s `tests`:
  ```rust
    #[test]
    fn config_choices_parse_child_capacity() {
        let tuples = ConfigChoices {
            ds_config: vec!["child-capacity=tuples".into()],
        };
        assert_eq!(
            tuples.hash_trie_config_resolved().unwrap().child_capacity,
            ChildCapacity::Tuples
        );
        let all = ConfigChoices {
            ds_config: vec![
                "load-factor=0.8".into(),
                "root-capacity=tuples".into(),
                "child-capacity=tuples".into(),
            ],
        };
        let resolved = all.hash_trie_config_resolved().unwrap();
        assert_eq!(resolved.child_capacity, ChildCapacity::Tuples);
        assert_eq!(resolved.root_capacity, RootCapacity::Tuples);
        assert_eq!(resolved.load_factor, LoadFactor::percent(80).unwrap());
        assert_eq!(
            ConfigChoices::default()
                .hash_trie_config_resolved()
                .unwrap()
                .child_capacity,
            ChildCapacity::Grow
        );
    }

    #[test]
    fn config_choices_reject_bad_child_capacities() {
        for bad in ["", "Grow", "keys", "1024"] {
            let choices = ConfigChoices {
                ds_config: vec![format!("child-capacity={bad}")],
            };
            let msg = choices.hash_trie_config_resolved().unwrap_err().to_string();
            assert!(msg.contains("child-capacity"), "{bad:?}: {msg}");
            assert!(msg.contains("expected grow or tuples"), "{bad:?}: {msg}");
        }
    }

    /// `incremental` under sized children is rejected with the flag to add.
    #[test]
    fn ds_choices_resolve_rejects_incremental_with_sized_children() {
        let err = DsChoices::resolve(
            IndexStructureSelector::HashTrie,
            &LayoutChoices::default(),
            &ConfigChoices {
                ds_config: vec!["child-capacity=tuples".into()],
            },
            &build(&["hash-trie=incremental"]),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "--ds-build hash-trie=incremental requires --ds-config child-capacity=grow; got \
             child-capacity=tuples"
        );
        let ok = DsChoices::resolve(
            IndexStructureSelector::HashTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build(&["hash-trie=incremental"]),
        )
        .unwrap();
        assert_eq!(ok.build.hash_trie, HashTrieBuildMode::Incremental);
    }
  ```
  and give the row-generic guard `every_prerequisite_is_reachable` (as landed
  by dependent-optimisations: each arm of its `match row` returns
  `(violating, satisfied, expected: Violation)`) the new row's arm, after
  the `PresizedBuildNeedsPresizedRoot` arm:
  ```rust
                | Prerequisite::IncrementalBuildNeedsGrowingChildren => {
                    let mut violating = DsChoices::default();
                    violating.build.hash_trie = HashTrieBuildMode::Incremental;
                    violating.config.child_capacity = ChildCapacity::Tuples;
                    let mut satisfied = violating;
                    satisfied.config.child_capacity = ChildCapacity::Grow;
                    (violating, satisfied, Violation {
                        dependent: "--ds-build hash-trie=incremental".to_owned(),
                        requires: "--ds-config child-capacity=grow",
                        actual: "child-capacity=tuples".to_owned(),
                    })
                },
  ```
  (The match is exhaustive, so until Step 3 adds the variant this arm is a
  compile error; that is the failing test.)

- [ ] **Step 2: Run them to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit options::
  ```
  Expected: compile errors (`ChildCapacity` not imported,
  `IncrementalBuildNeedsGrowingChildren` not found).

- [ ] **Step 3: Implement.**
  - Import `ChildCapacity` in the top-level `kermit_ds::{…}` list.
  - `ConfigChoices`'s field doc: "`HashTrie` accepts `load-factor=<decimal in
    (0, 1)>`, `root-capacity=grow|tuples` and `child-capacity=grow|tuples`."
  - `HASH_TRIE_KEYS`: `&["load-factor", "root-capacity", "child-capacity"]`.
  - In `hash_trie_config_resolved`'s `match key`, after `"root-capacity"`:
    ```rust
                | "child-capacity" => {
                    config.child_capacity = value
                        .parse::<ChildCapacity>()
                        .map_err(|why| anyhow::anyhow!("--ds-config {key}: {why}"))?;
                },
    ```
  - `Prerequisite`: add the variant after `PresizedBuildNeedsPresizedRoot`,
    ```rust
    /// `--ds-build hash-trie=incremental` creates each child on its first
    /// tuple, before the child's list is known, so it cannot size it: it
    /// needs `--ds-config child-capacity=grow` (#107).
    IncrementalBuildNeedsGrowingChildren,
    ```
    extend `ALL` to
    `&[Self::PresizedBuildNeedsPresizedRoot, Self::IncrementalBuildNeedsGrowingChildren]`,
    and add the arm to `violated`:
    ```rust
            | Self::IncrementalBuildNeedsGrowingChildren => match choices.build.hash_trie {
                | HashTrieBuildMode::Incremental
                    if choices.config.child_capacity != ChildCapacity::Grow =>
                {
                    Some(Violation {
                        dependent: "--ds-build hash-trie=incremental".to_owned(),
                        requires: "--ds-config child-capacity=grow",
                        actual: format!(
                            "child-capacity={}",
                            choices.config.child_capacity.axis_value()
                        ),
                    })
                },
                | _ => None,
            },
    ```
    (Only an explicit flag makes `child_capacity` non-default, so the actual
    value never needs "(the default)".)
  - `BuildChoices`'s field doc ends: "…; `presized` requires `--ds-config
    root-capacity=tuples`, and `incremental` requires `--ds-config
    child-capacity=grow`)".

- [ ] **Step 4: Run.** Same command. Expected: all pass.

- [ ] **Step 5: Commit, then a mutant.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit/src/options.rs
  git -C $WT commit -m "feat(cli): --ds-config child-capacity, and incremental requires child-capacity=grow (#107)"
  ```
  **M8, the row left out of `ALL`:** remove
  `Self::IncrementalBuildNeedsGrowingChildren` from `ALL`. Fails:
  `ds_choices_resolve_rejects_incremental_with_sized_children`.

---

## Task 9 [P3]: CLI and family tests

**Files:**
- Modify: `kermit/tests/cli_hash_trie_config_choice.rs`
- Modify: `kermit/tests/cli_hash_trie_build_mode.rs`
- Modify: `kermit/src/execution.rs` (tests)

- [ ] **Step 1: Write the tests.** In `cli_hash_trie_config_choice.rs` (and its
  module doc's key list: "`load-factor`, `root-capacity`, `child-capacity`"):
  ```rust
  #[test]
  fn cli_bench_ds_with_child_capacity_records_axis() {
      let (output, report) = bench_ds("hash-trie", &["--ds-config", "child-capacity=tuples"]);
      assert!(
          output.status.success(),
          "{}",
          String::from_utf8_lossy(&output.stderr)
      );
      assert_eq!(axes_of(&report)["ds_config_child_capacity"], "tuples");
  }

  #[test]
  fn cli_bench_ds_default_child_capacity_is_grow() {
      let (output, report) = bench_ds("hash-trie", &[]);
      assert!(
          output.status.success(),
          "{}",
          String::from_utf8_lossy(&output.stderr)
      );
      assert_eq!(axes_of(&report)["ds_config_child_capacity"], "grow");
  }

  #[test]
  fn cli_bench_ds_rejects_a_malformed_child_capacity() {
      let (output, _) = bench_ds("hash-trie", &["--ds-config", "child-capacity=keys"]);
      assert!(!output.status.success());
      let stderr = String::from_utf8_lossy(&output.stderr);
      assert!(stderr.contains("expected grow or tuples"), "{stderr}");
  }
  ```
  In `cli_hash_trie_build_mode.rs`:
  ```rust
  /// `serial` named the per-tuple build until #107; the CLI rejects it and
  /// names what replaced it.
  #[test]
  fn cli_rejects_the_retired_serial_build() {
      let (output, _) = bench_ds("hash-trie", &["--ds-build", "hash-trie=serial"]);
      assert!(!output.status.success());
      let stderr = String::from_utf8_lossy(&output.stderr);
      assert!(stderr.contains("serial was renamed incremental in #107"), "{stderr}");
      assert!(stderr.contains("the default is now bulk (Algorithm 2)"), "{stderr}");
  }

  #[test]
  fn cli_bench_ds_records_the_incremental_build() {
      let (output, report) = bench_ds("hash-trie", &["--ds-build", "hash-trie=incremental"]);
      assert!(
          output.status.success(),
          "{}",
          String::from_utf8_lossy(&output.stderr)
      );
      assert_eq!(axes_of(&report)["ds_build_mode"], "incremental");
  }

  /// `incremental` cannot size a child it creates on its first tuple.
  #[test]
  fn cli_rejects_incremental_with_sized_children() {
      let (output, _) = bench_ds("hash-trie", &[
          "--ds-config",
          "child-capacity=tuples",
          "--ds-build",
          "hash-trie=incremental",
      ]);
      assert!(!output.status.success());
      let stderr = String::from_utf8_lossy(&output.stderr);
      assert!(
          stderr.contains(
              "--ds-build hash-trie=incremental requires --ds-config child-capacity=grow; got \
               child-capacity=tuples"
          ),
          "{stderr}"
      );
  }

  /// `--verify` checks the answers of the closest-to-paper configuration,
  /// pruned, eager and lazy (#107).
  #[test]
  fn cli_bench_run_verifies_the_papers_configuration() {
      for expansion in ["eager", "lazy"] {
          let (output, report) = bench_run("triangle", &[
              "-i",
              "hash-trie",
              "-a",
              "hash-triejoin",
              "-m",
              "iteration",
              "--verify",
              "--ds-layout-pruning",
              "on",
              "--ds-layout-expansion",
              expansion,
              "--ds-config",
              "root-capacity=tuples,child-capacity=tuples,load-factor=0.8",
              "--ds-build",
              "hash-trie=presized:3",
          ]);
          assert!(
              output.status.success(),
              "{expansion}: {}",
              String::from_utf8_lossy(&output.stderr)
          );
          let axes = axes_of(&report);
          assert_eq!(axes["verified"], true, "{expansion}: {axes}");
          assert_eq!(axes["ds_config_child_capacity"], "tuples", "{expansion}: {axes}");
          assert_eq!(axes["ds_build_mode"], "presized:3", "{expansion}: {axes}");
      }
  }
  ```
  In `kermit/src/execution.rs`'s tests, beside
  `hash_family_build_relation_presizes_the_root_under_tuples` (import
  `kermit_ds::ChildCapacity` in the test module):
  ```rust
    /// Under `child-capacity=tuples` the family's build sizes children from
    /// their lists, so a report labelled `"tuples"` timed sized children.
    /// Three equal tuples share one child: grown, it keeps 4 buckets; sized
    /// for its 3 tuples at 70 % it has 8.
    #[test]
    fn hash_family_build_relation_sizes_children_under_tuples() {
        let heap = |child_capacity| {
            let family = HashHtj::<SipHashStrategy, NoPruning, EagerExpansion>::new(
                HashTrieConfig {
                    child_capacity,
                    ..HashTrieConfig::default()
                },
                HashTrieBuildMode::Bulk,
                Planner::stored(LexicographicOptimiser),
            );
            let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
            family
                .build_relation(header, vec![vec![1, 1]; 3])
                .heap_size_bytes()
        };
        assert!(heap(ChildCapacity::Tuples) > heap(ChildCapacity::Grow));
    }
  ```

- [ ] **Step 2: Run.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_hash_trie_config_choice --test cli_hash_trie_build_mode
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit execution::tests::hash_family
  ```
  Expected: all pass. They pass on first run, because Tasks 7–8 already built
  what they check. So check them against a mutant: **M9**, in
  `HashTrieConfig::axes`, report `"grow"` for `child_capacity` whatever its
  value. `cli_bench_ds_with_child_capacity_records_axis` must fail. Reverse it.

- [ ] **Step 3: Commit.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit/tests/cli_hash_trie_config_choice.rs kermit/tests/cli_hash_trie_build_mode.rs kermit/src/execution.rs
  git -C $WT commit -m "test(cli): child-capacity, incremental and the paper's configuration end to end (#107)"
  ```

---

## Task 10 [P3]: The join suites

**Files:**
- Modify: `kermit/tests/join_tests.rs`

- [ ] **Step 1: Add the blocks.** Add `ChildCapacity` to the `kermit_ds::{…}`
  import. After the root-capacity Config block:
  ```rust
  // ── Config axis: child capacity ─────────────────────────────────────────
  // The default-config invocations above are the `ds_config_child_capacity:
  // "grow"` baseline; these are the alternate (#107). Under `tuples` every
  // child is sized once from its list, as Algorithm 2's line 3 sizes it.
  define_config_provider!(SizedChildren, HashTrieConfig, HashTrieConfig {
      child_capacity: ChildCapacity::Tuples,
      ..HashTrieConfig::default()
  });
  ```
  followed by eight `define_multiway_join_test_suite_with_config!`
  invocations with `SizedChildren`: `HashTrieSip` and `HashTrieFx`, each under
  `LexicographicOptimiser`, `CardinalityOptimiser` and `CostBasedOptimiser`,
  and `HashTrieSipLazy` under `LexicographicOptimiser` and
  `CostBasedOptimiser`. These are the `PresizedRoot` block's eight
  invocations with the provider swapped. Each is written out, e.g.
  ```rust
  define_multiway_join_test_suite_with_config!(
      HashTrieSip,
      HashTriejoin,
      LexicographicOptimiser,
      SizedChildren
  );
  ```
  Before the lazy pair, keep the comment "Expansion sizes each child from its
  pending list, so a lazy trie's children are sized too."

  After the radix BuildMode block:
  ```rust
  // ── BuildMode axis: HashTrie's per-tuple build ──────────────────────────
  // The plain HashTrie invocations above build `bulk` (Algorithm 2, the
  // default); these run `incremental`, the build before #107, which builds
  // the identical trie. The same three Layouts as the radix rows.
  // ColumnTrie's provider is `Incremental`, so this one is `HashIncremental`.
  define_build_mode_provider!(
      HashIncremental,
      HashTrieBuildMode,
      HashTrieBuildMode::Incremental
  );
  ```
  followed by nine `define_multiway_join_test_suite_for_build_mode!`
  invocations with `HashIncremental`: `HashTrieSip`, `HashTrieFxPruned` and
  `HashTrieSipPrunedLazy`, each under the three optimisers (the `Radix2`
  block's nine, provider swapped).

  After the presized "BuildMode × Config" block:
  ```rust
  // ── BuildMode × Config: sized children under every partitioned build ────
  // Under `child-capacity=tuples` (#107) the partitioned builds size every
  // child as `bulk` does. The last three are the closest-to-paper
  // configuration: root and children sized at a load factor of 0.8, pruned
  // and lazy, under `presized:2`.
  define_config_provider!(PaperSizing, HashTrieConfig, HashTrieConfig {
      load_factor: LoadFactor::percent(80).unwrap(),
      root_capacity: RootCapacity::Tuples,
      child_capacity: ChildCapacity::Tuples,
  });
  type HashTrieSipSizedChildren = Configured<HashTrieSip, SizedChildren>;
  type HashTrieSipLazySizedChildren = Configured<HashTrieSipLazy, SizedChildren>;
  type HashTrieSipPrunedLazyPaper = Configured<HashTrieSipPrunedLazy, PaperSizing>;
  ```
  followed by nine `define_multiway_join_test_suite_for_build_mode!`
  invocations: `HashTrieSipSizedChildren` with `Radix2`,
  `HashTrieSipLazySizedChildren` with `HashParallel2`, and
  `HashTrieSipPrunedLazyPaper` with `HashPresized2`, each under the three
  optimisers.

  In the `define_multiway_join_test_suite_with_column_orders!` block (the
  "copy is built the way its relation type builds" one), add the aliases
  `type HashTrieSipIncremental = BuiltWith<HashTrieSip, HashIncremental>;`
  beside the other `BuiltWith` aliases, and append rows for
  `HashTrieSipSizedChildren`, `HashTrieSipIncremental` and
  `HashTrieSipPrunedLazyPaper`, each under the three optimisers with
  `AnyOrders`.

- [ ] **Step 2: Run.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests 2>&1 | grep -E '^test result'
  ```
  Expected: all pass; the count rises by 16 × (8 + 9 + 9 + 9) = 560.

- [ ] **Step 3: Commit.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit/tests/join_tests.rs
  git -C $WT commit -m "test(join): the 16 patterns under child-capacity, incremental and the paper's sizing (#107)"
  ```

---

# P4 — kermit-lab

## Task 11 [P4]: The rename, the back-fills, the per-structure baseline

**Files:**
- Modify: `python/kermit-lab/kermit_lab/defaults.py`
- Modify: `python/kermit-lab/kermit_lab/frame.py` (`_summary_from_reports`, `threads_of` doc)
- Modify: `python/kermit-lab/kermit_lab/analysis.py` (`speedup_table`, `BASELINE_BUILD_MODES`)
- Modify: `python/kermit-lab/kermit_lab/presets.py` (`speedup`)
- Modify: `python/kermit-lab/README.md` (the `speedup` row, ~line 128)
- Test: `python/kermit-lab/tests/{test_defaults,test_analysis,test_render_all,test_contract}.py`

- [ ] **Step 1: Write the failing tests.**
  - `test_defaults.py`: in `test_build_mode_backfills_each_structures_pre_axis_build`,
    the HashTrie `pd.NA` row back-fills `"incremental"` (expected list
    `["incremental", "bulk", "serial", "parallel:4", "incremental", "radix:8"]`)
    and `SCOPED_AXIS_DEFAULTS[("ds_build_mode", "HashTrie")] == "incremental"`;
    its docstring names "HashTrie's pre-#91 per-tuple build, ``incremental``
    since #107". Add (importing `apply_renamed_axis_values`):
    ```python
    def test_renamed_values_rewrite_hash_tries_old_serial_only() -> None:
        """HashTrie's per-tuple build was ``serial`` until #107 and is
        ``incremental`` since; TreeTrie's ``serial`` is another build."""
        df = pd.DataFrame({
            "data_structure": ["HashTrie", "HashTrie", "TreeTrie", "HashTrie"],
            "ds_build_mode": ["serial", "bulk", "serial", pd.NA],
        })
        out = apply_renamed_axis_values(df)
        assert out["ds_build_mode"].tolist()[:3] == ["incremental", "bulk", "serial"]
        assert pd.isna(out["ds_build_mode"].iloc[3])
        assert df["ds_build_mode"].iloc[0] == "serial", "the caller's frame is untouched"


    def test_child_capacity_backfills_grow_on_hash_trie_only() -> None:
        df = pd.DataFrame({
            "data_structure": ["HashTrie", "TreeTrie"],
            "ds_config_child_capacity": [pd.NA, pd.NA],
        })
        out = apply_axis_defaults(df)
        assert out["ds_config_child_capacity"].iloc[0] == "grow"
        assert pd.isna(out["ds_config_child_capacity"].iloc[1])
    ```
  - `test_analysis.py`: `_build_mode_rows` gains a keyword
    `data_structure: str = "TreeTrie"` used for the row's `data_structure` and
    in `criterion_function` (`f"{data_structure}/insertion"`). Add:
    ```python
    def test_speedup_table_divides_a_hash_trie_by_its_bulk_build() -> None:
        df = _build_mode_rows(
            {"bulk": [100.0], "incremental": [90.0], "parallel:2": [50.0]},
            data_structure="HashTrie",
        )
        assert speedup_table(df).iloc[0]["speedup"] == pytest.approx(2.0)


    def test_speedup_table_baseline_overrides_the_structures_default() -> None:
        df = _build_mode_rows(
            {"bulk": [100.0], "incremental": [80.0], "parallel:2": [40.0]},
            data_structure="HashTrie",
        )
        assert speedup_table(df, baseline="incremental").iloc[0]["speedup"] == pytest.approx(2.0)


    def test_speedup_table_names_the_structures_baseline_when_unpaired() -> None:
        df = _build_mode_rows({"serial": [100.0], "parallel:2": [50.0]}, data_structure="HashTrie")
        with pytest.warns(UserWarning, match="no 'bulk' row"), pytest.raises(
            ValueError, match="no case"
        ):
            speedup_table(df)
    ```
  - `test_render_all.py`: in `test_ablation_preset_lets_build_mode_through_on_iteration`,
    the expected set is `{"incremental", "bulk"}` and the comment "ColumnTrie's
    two builds, and HashTrie's back-filled pre-#91 build (``incremental``)";
    in `test_build_mode_ablation_leaves_out_structures_without_the_axis`, the
    docstring says the HashTrie report "loads as ``incremental``" and the
    assertion compares with `"incremental"`. In `conftest.py`,
    `fixture_build_mode_tree`'s docstring: "so it loads as its pre-#91
    per-tuple build, ``incremental``".
  - `test_contract.py`:
    ```python
    def test_bench_ds_hash_trie_reports_the_bulk_build_and_its_child_capacity(
        tmp_path: Path,
    ) -> None:
        report = tmp_path / "ds.json"
        _run(
            tmp_path, report,
            "ds", "--relation", str(FIXTURES / "edge.csv"), "-i", "hash-trie", "-m", "space",
        )
        # Without the back-fill, so the binary's own labels are what is read.
        df = kl.load(report, criterion_root=tmp_path / "target" / "criterion", apply_defaults=False)
        assert len(df) == 1
        assert df.iloc[0]["ds_build_mode"] == "bulk"
        assert df.iloc[0]["ds_config_child_capacity"] == "grow"
    ```

- [ ] **Step 2: Run them to verify they fail.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit
  KERMIT_BIN=$WT/target/debug/kermit uv --directory $WT/python/kermit-lab run pytest -q
  ```
  Expected: the new and changed tests fail (`apply_renamed_axis_values`
  missing, back-fill still `serial`, baseline still `serial`).

- [ ] **Step 3: Implement.**
  - `defaults.py`: the HashTrie build-mode default becomes
    ```python
    # HashTrie built one insert per tuple before issue #91: the build named
    # `serial` until #107 and `incremental` since. Every HashTrie report since
    # #91 carries the axis (`bulk`, Algorithm 2, by default since #107).
    ("ds_build_mode", "HashTrie"): "incremental",
    ```
    and add after the root-capacity entry:
    ```python
    # Every HashTrie child grew from 4 buckets before issue #107.
    ("ds_config_child_capacity", "HashTrie"): "grow",
    ```
    After `BINARY_AXIS_DEFAULTS`:
    ```python
    # (axis column, data_structure, old value) -> new value, for an axis value a
    # later change renamed. Unlike a default this rewrites a value the report
    # carries, and the loader applies it whether or not it back-fills defaults:
    # the old value named the same build, so the new name is ground truth.
    RENAMED_AXIS_VALUES: dict[tuple[str, str, object], object] = {
        # HashTrie's per-tuple build was `serial` until #107, which made
        # Algorithm 2 (`bulk`) the default and renamed it `incremental`.
        ("ds_build_mode", "HashTrie", "serial"): "incremental",
    }


    def apply_renamed_axis_values(df: pd.DataFrame) -> pd.DataFrame:
        """Return ``df`` with every renamed axis value replaced by its new
        name, on rows of the structure that renamed it. Mutates a copy, leaving
        the caller's frame unchanged."""
        out = df.copy()
        if "data_structure" not in out.columns:
            return out
        for (col, data_structure, old), new in RENAMED_AXIS_VALUES.items():
            if col in out.columns:
                hit = out["data_structure"].isin([data_structure]) & out[col].isin([old])
                out.loc[hit, col] = new
        return out
    ```
    The module docstring's paragraph on the registries gains: "A third,
    `RENAMED_AXIS_VALUES`, rewrites a value a later change renamed; the loader
    applies it even without back-filling."
  - `frame.py`, `_summary_from_reports`: import `apply_renamed_axis_values`
    beside `apply_axis_defaults`, and before `if apply_defaults:` add
    ```python
        # A renamed value names the build that ran, so it is rewritten whether
        # or not defaults are back-filled.
        df = apply_renamed_axis_values(df)
    ```
    `threads_of`'s docstring: "TreeTrie's ``serial``, ColumnTrie's and
    HashTrie's ``bulk`` / ``incremental``, …".
  - `analysis.py`: before `speedup_table`,
    ```python
    # The default single-threaded build of each structure with a parallel one:
    # what `speedup_table` divides by when it is given no `baseline`.
    BASELINE_BUILD_MODES: dict[str, str] = {
        "TreeTrie": "serial",
        "HashTrie": "bulk",
    }
    ```
    `speedup_table`'s signature: `baseline: str | None = None`. Its first
    docstring line: "Speedup of every threaded build over the baseline build:
    ``baseline`` if given, else the structure's default single-threaded build
    (:data:`BASELINE_BUILD_MODES`: TreeTrie ``serial``, HashTrie ``bulk``)."
    In the body, after `rows = …`:
    ```python
        def baseline_of(data_structure: object) -> str:
            if baseline is not None:
                return baseline
            return BASELINE_BUILD_MODES.get(data_structure, "serial")

        def baselines_named(frame: pd.DataFrame) -> str:
            structures = frame["data_structure"] if "data_structure" in frame.columns else [None]
            return " or ".join(repr(m) for m in sorted({baseline_of(s) for s in structures}))
    ```
    The loop as dependent-optimisations landed it selects `base` before it
    computes `identity`; compute `identity` first and select the case's
    baseline by it (the rest of the loop, `threaded` / `build_of` arms
    included, is unchanged):
    ```python
        for key, case in rows.groupby(case_keys, dropna=False, sort=True):
            identity = dict(zip(case_keys, key if isinstance(key, tuple) else (key,)))
            base = case[case["ds_build_mode"] == baseline_of(identity.get("data_structure"))]
            if base.empty:
                continue
    ```
    In the orphan warning, `have no {baseline!r} row in their case` becomes
    `have no {baselines_named(orphans)} row in their case`, and in the final
    error `no case has both a {baseline!r} row` becomes `no case has both a
    {baselines_named(rows)} row`; the rest of both messages is unchanged. The
    existing tests that match `"no 'serial' row"` use TreeTrie rows, so they
    still pass.
  - `README.md`, the `speedup` row: "build speedup over each structure's
    default single-threaded build (TreeTrie `serial`, HashTrie `bulk`) vs
    `threads` …", and its requirement column: "that baseline's and
    `parallel:N` / `presized:N` `ds_build_mode` rows of one case".
  - `presets.py`, `speedup`: `baseline: str | None = None`, docstring "over the
    baseline build (each structure's default single-threaded build unless
    ``baseline`` is given)". The y-axis label, `f"speedup over {baseline}
    ({phase})"`, names the defaults when none is given:
    ```python
        over = baseline
        if over is None:
            structures = table["data_structure"] if "data_structure" in table.columns else [None]
            over = " / ".join(sorted({BASELINE_BUILD_MODES.get(s, "serial") for s in structures}))
        ax.set_ylabel(f"speedup over {over} ({phase})")
    ```
    (import `BASELINE_BUILD_MODES` from `.analysis` beside `speedup_table`).

- [ ] **Step 4: Run.** Same commands as Step 2. Expected: all pass, the
  contract test included.

- [ ] **Step 5: Commit.**
  ```bash
  git -C $WT add python/kermit-lab/kermit_lab/defaults.py python/kermit-lab/kermit_lab/frame.py python/kermit-lab/kermit_lab/analysis.py python/kermit-lab/kermit_lab/presets.py python/kermit-lab/tests/test_defaults.py python/kermit-lab/tests/test_analysis.py python/kermit-lab/tests/test_render_all.py python/kermit-lab/tests/test_contract.py python/kermit-lab/tests/conftest.py
  git -C $WT commit -m "feat(kermit-lab): HashTrie's serial reads as incremental; child-capacity back-fill; per-structure speedup baseline (#107)"
  ```

---

# P5 — docs

## Task 12 [P5]: Document the build

The dependent-optimisations landing has reworded parts of these files. Apply
each change below to the text as it stands after Task 0, keeping its wording
where these instructions do not replace it. No doc may attribute the
overflow tail, the per-run recursion or the pending-list capacity fix to the
paper.

**Files:**
- Modify: `docs/data-structures/hash-trie.md`
- Modify: `docs/data-structures/parallel-build.md`
- Modify: `docs/specs/optimization-standard.md`
- Modify: `docs/specs/bench-report-schema.md`
- Modify: `CLAUDE.md`
- Modify: `USAGE.md`, `BENCHMARKING.md`, `ARCHITECTURE.md`
- Modify: `docs/specs/2026-10-07-hash-trie-algorithm-2-build-design.md` (status line)

- [ ] **Step 1: `hash-trie.md`.**
  1. **Representation**, the `Unexpanded` paragraph: "moves them into a table
     built by the same `insert_at`, one level deep" → "moves them into a
     table built by Algorithm 2's `build` (`bulk.rs`), one level deep".
     Wherever `hash-trie.md` says an expanded child *is* the eager table
     (~57, ~216), qualify it as the code docs now do: for a trie no `insert`
     has changed since its build, and always under `child-capacity=grow`.
  2. **A new `### Construction` subsection** at the end of Representation:
     ```markdown
     ### Construction

     A trie built from a known set of tuples is built by the paper's
     Algorithm 2 (VLDB 2020, §3.2.2; [`bulk.rs`](../../kermit-ds/src/ds/hash_trie/bulk.rs)):
     a table is allocated once for a list of tuples, every tuple is pushed onto
     the list in its bucket, and each bucket's list is then built into the
     bucket's child, recursively. This is the `bulk` build mode, the default.

     | Algorithm 2 | kermit |
     |---|---|
     | line 3, allocate `M` | `HashTable::with_log2_capacity`, sized by `root_log2_capacity` (the root) or `child_log2_capacity` (a child) |
     | lines 4–7, push each tuple onto its bucket's list | `HashTrie::group` |
     | lines 8–12, build each bucket's child from its list | `HashTrie::build_nested`, through `HashTable::map` and `HashTrie::child` |
     | line 15, return the list | the last attribute's table of lists is the `Leaf`, its lists the chains |
     | the lists themselves | a `Vec` per bucket; Umbra threads them through an 8-byte chain pointer in each tuple (§3.3.2), which needs #101's contiguous storage |

     `child` makes a one-tuple list a `Singleton` under pruning, keeps a list
     as an `Unexpanded` child under lazy expansion, and otherwise builds the
     next table. `insert`, and the `incremental` build mode, place one tuple at
     a time by `insert_at` instead; under every config both accept, the two
     builds give the identical trie, because a table's layout depends only on
     the order its new keys arrive and every list keeps input order. One detail
     keeps it byte-identical: `child` shrinks a one-tuple pending list (pruning
     off) and a two-tuple one (pruning on) to the capacities `insert_at` gives
     them. That is kermit's, for `space`'s sake, not the paper's.
     ```
  3. **Invariants**, "Path depth = arity": "Enforced at construction time by
     `HashTrie::make_root` and `insert_at`" → "Enforced at construction time
     by `make_root_sized` and the builds (`build_nested`, `insert_at`)"
     (`make_root` was deleted in Task 5). "Load factor cap", last sentence → "Every table starts
     at 4 buckets except in a build from a known set of tuples, where
     `--ds-config root-capacity=tuples` sizes the root once for the tuple
     count, and `child-capacity=tuples` sizes every child once for its list
     (see [Config flags](#config-flags))." "Lazy buckets hold no tables":
     "its tuples are re-inserted in insertion order, the order eager
     construction inserted them, under the same load factor" → "it is built
     by Algorithm 2 from its pending list, which keeps insertion order, under
     the same load factor and child capacity as the eager build".
  4. **Complexity**: the `from_tuples(n)` row's notes → "Algorithm 2, the
     default `bulk` build: n hashes and moves per level, a list allocation per
     inner bucket (a one-tuple list too, freed again when pruning makes it a
     `Singleton`), and one transient table of lists per
     table (about 32 B a bucket, the root's the largest). Expected cost holds
     for input in another `HashTrie`'s iteration order, or any subset of it;
     the bucket-index invariant names the one order it does not cover". Add a
     row after it: `` | `from_tuples` under `incremental` | O(n · a) | O(n · a) | loops `insert` over the input: the build before #107 | ``. The radix
     row: "the serial inserts" → "the bulk build".
  5. **Config flags**: in the root-capacity bullet, replace "Child tables keep
     growing from 4 under both values, since the serial build creates a child
     before it knows how many tuples the child will hold." with "Child tables
     are sized by `child-capacity`, below." Then add the bullet:
     ```markdown
     - **Child capacity** (`ds_config_child_capacity`): how large a build makes
       every table below the root. Under `grow` (the default) a child starts at
       4 buckets and doubles as keys arrive. Under `tuples` each child is sized
       once from |L|, the length of the list Algorithm 2 builds it from: the
       smallest power of two ≥ 2 with `|L| · 100 ≤ capacity · percent`, which at
       `load-factor=0.8` is the paper's `2^⌈log2(1.25·|L|)⌉` exactly, one-tuple
       lists included. Distinct keys cannot outnumber tuples, so no child grows
       during the build. It is a *value* on a path every `bulk` build takes (a
       child's starting capacity, read once per child), so the default pays
       nothing for it. Lazy expansion sizes an expanded child from its pending
       list the same way.
       - **CLI:** `-i hash-trie --ds-config child-capacity=tuples`. The paper's
         sizing throughout is `--ds-config
         root-capacity=tuples,child-capacity=tuples,load-factor=0.8`.
       - **Default:** `grow` (the only behaviour before #107).
       - **Prerequisite:** `--ds-build hash-trie=incremental` requires
         `child-capacity=grow`: the per-tuple build creates a child on its first
         tuple, before the child's list is known.
       - **Rust:** `HashTrieConfig { child_capacity: ChildCapacity::Tuples, ..HashTrieConfig::default() }`;
         `HashTrieConfig::child_log2_capacity`. After the build, a child that
         `insert` creates starts at 4 buckets under eager expansion; under lazy
         expansion it is sized from its pending list when first probed.
       - **Bench axis value:** the JSON string `"grow"` / `"tuples"`.
       - **Expected effect:** `insertion` falls where children would rehash as
         they grow. `space` rises where a child's list holds more tuples than
         distinct keys (|L| > D), the tuples-versus-keys question #113 raises
         for the root. Unmeasured as of this writing.
     ```
  6. **Build modes**: the intro: "every mode here builds the identical trie"
     stays, with "under every config it accepts" added. Replace the `Serial`
     row with:
     ```markdown
     | `Bulk` (default) | `hash-trie=bulk` | Algorithm 2 (§3.2.2): each table's tuples grouped into its buckets, then each bucket's child built from its list ([Construction](#construction)) |
     | `Incremental` | `hash-trie=incremental` | one `insert_at` per tuple, in input order: the build named `serial` before #107; requires `child-capacity=grow` |
     ```
     In the radix row, "build each partition into a scratch root" → "group
     each partition into a scratch root and build its children by Algorithm
     2". In **The radix build**, step 2 becomes "**Scratch roots.** Each
     non-empty partition's tuples are grouped into a scratch root of the real
     root's kind (Algorithm 2, lines 4–7), recording the input index of the
     tuple that introduced each key; then each key's child is built from its
     list, as the bulk build builds it." In **Why it is identical**, the third
     bullet → "Each root key's child is built from the same list, in input
     order, because the partition is stable. That covers every `Singleton`,
     every chain, every capacity and, under `LazyExpansion`, every pending
     list." In the presized paragraph, "each worker inserts every tuple of its
     regions once" → "each worker pushes every tuple of its regions onto its
     bucket's list; after the calling thread's tail, each worker builds the
     children of its regions by Algorithm 2 (spreading the recursion one run
     per worker is kermit's choice; the paper is silent)", and name the
     closest-to-paper configuration as `--ds-config
     root-capacity=tuples,child-capacity=tuples,load-factor=0.8 --ds-build
     hash-trie=presized:N`, with pruning and lazy expansion on. In the
     **Axis** bullet: the values are `bulk` / `incremental` / `radix:K` /
     `parallel:N` / `presized:N`, and "kermit-lab reads a HashTrie row
     without the axis, or with the pre-#107 `serial`, as `incremental`". In
     **API**: "`Relation::from_tuples` uses `Bulk`." In **Tests**, add the
     first item "`bulk_builds_the_incremental_trie_*` and
     `tuples_sizes_every_child_from_its_list` in `bulk.rs`, the
     `HashTrieSipIncremental` aliases, and the `HashIncremental`,
     `SizedChildren` and `PaperSizing` join suites", rename
     `radix_builds_the_serial_trie_*` / `parallel_builds_the_serial_trie_*`
     to `…_bulk_trie_*`, and replace "`the_root_step_builds_the_serial_trie_below_the_root`,
     which guards the root step's mirror of `insert_at`" with
     "`one_run_of_grouping_builds_the_bulk_root`, the grouping step without
     threads". Add a bullet **When `incremental` is useful**: "reproducing
     pre-#107 `insertion` numbers, and measuring what Algorithm 2's order
     buys over the per-tuple descent (the locality #101 is about)."

- [ ] **Step 2: `parallel-build.md`.**
  - The mode table near the top: replace the `HashTrieBuildMode::Serial` row
    with two rows, `` | `HashTrieBuildMode::Bulk` | `hash-trie=bulk` | `"bulk"` | ✓ | ``
    and `` | `HashTrieBuildMode::Incremental` | `hash-trie=incremental` | `"incremental"` | ✓ | ``.
    Wherever the HashTrie text names `serial` as its baseline, it names
    `bulk`.
  - **The three steps (exact build)**: the pseudocode line "build a scratch
    root by insert_at, in input order," → "group into a scratch root in input
    order, then build each key's child from its list (Algorithm 2),"; the
    prose "by the serial build's own `insert_at`" → "by Algorithm 2, as the
    bulk build does".
  - **Invariant (exact build)**: "is built by the serial build's `insert_at`
    calls, on the same tuples in" → "is built from the same list, the same
    tuples in".
  - **The presized build**: replace the pseudocode's root-step lines
    (`insert_at_{leaf,inner}_root_in_run(run k, tuple)` and the two after it)
    with
    ```text
                push_in_run(run k, tuple)        lines 6–7, onto its bucket's list
                    off the region's end → defer
        tail: push each deferred tuple, in input order, by ordinary probing
        children: each worker maps its run's lists to children (`child`, lines 8–12)
    ```
    and the paragraph on "The root step (`insert_at_leaf_root_in_run` /
    `insert_at_inner_root_in_run` …) is `insert_at`'s depth-0 arm, decision
    for decision …" with "The root step is `push_in_run`: Algorithm 2's lines
    6–7 inside a run, the same for every arity. Children are built after the
    tail, by the bulk build's `child`, one run of buckets per worker; the paper
    does not say how the recursion is spread over threads, so this is
    kermit's." The invariant bullet "**Subtries are serial's.** A key's tuples
    reach `insert_at` in input order …" becomes "**Subtries are bulk's.** A
    key's tuples reach its bucket's list in input order, all inside its region
    or all in the tail, so its child is built from the list `bulk` builds it
    from."

- [ ] **Step 3: `optimization-standard.md`.**
  - Config section, after the root-capacity example:
    ```markdown
    > **Third concrete example (implemented).** The child capacity (#107). A child table's starting capacity was the constant 4 buckets; `HashTrieConfig::child_capacity` replaces it with a value read once per child: 4 under `grow`, or under `tuples` the smallest power of two that holds the child's list under the load factor (Algorithm 2, line 3). Only a build that groups before it recurses knows a list's length, so the per-tuple `incremental` build has the prerequisite `child-capacity=grow`.
    ```
  - The BuildMode "Examples" row: "HashTrie bulk / incremental (requires
    `child-capacity=grow`) / radix:K / parallel:N / presized:N (requires
    `root-capacity=tuples`) ✓".
  - "What's implemented today": the two HashTrie build rows read "(bulk /
    incremental / radix:K)" and "(bulk / parallel:N / presized:N)", and add
    rows `` | HashTrie Algorithm 2 build (bulk, the default; incremental is the per-tuple one) | BuildMode | `ds_build_mode` | §3.2.2 (issue #107) | `` and
    `` | HashTrie child capacity (grow / tuples) | Config | `ds_config_child_capacity` | §3.2.2, Algorithm 2 line 3 (issue #107) | ``.
  - If the dependent-optimisations landing added a list of prerequisites,
    add `incremental requires child-capacity=grow` to it.

- [ ] **Step 4: `bench-report-schema.md`.**
  - The `ds_build_mode` bullet's HashTrie values: "`bulk` (default, Algorithm
    2), `incremental` (the per-tuple build, `serial` before #107),
    `radix:<bits>`, `parallel:N` and `presized:N`".
  - The back-fill bullet: on HashTrie rows add `ds_config_child_capacity ==
    "grow"` and `ds_build_mode == "incremental"`, and "a HashTrie
    `ds_build_mode` of `serial` reads as `incremental` (`RENAMED_AXIS_VALUES`
    in kermit-lab's `defaults.py`)".
  - A history row:
    ```markdown
    | 3 (no bump) | 2026-10-07 | HashTrie builds by Algorithm 2 (#107): its default `ds_build_mode` is `bulk`, and the per-tuple build is `incremental`, spelled `serial` until now; kermit-lab reads old HashTrie `serial` rows as `incremental`, so they stay correctly labelled. Every HashTrie report gains `ds_config_child_capacity` (`grow` by default). The trie each existing config builds is unchanged, so `space` and `iteration` keep their meaning; `insertion` and `end_to_end` of the default and of `radix:K` / `parallel:N` / `presized:N` now time Algorithm 2, so compare them within one binary, as every timing already is. Additive, so `schema_version` stays `3`. |
    ```

- [ ] **Step 5: `CLAUDE.md`.**
  - In Build Commands, after the `root-capacity=tuples` line:
    `cargo run -- bench run triangle -i hash-trie -a hash-triejoin --ds-config root-capacity=tuples,child-capacity=tuples,load-factor=0.8  # Config axis: every table sized once, the paper's Algorithm 2 sizing (#107)`.
  - The `BuildModeRelation` bullet's HashTrie clause: "`HashTrie`
    (`HashTrieBuildMode`: `bulk` default = Algorithm 2, group then recurse,
    #107; `incremental` = one `insert_at` per tuple, `serial` before #107,
    requires `child-capacity=grow`; `radix:<bits>` …" (keep the rest).
  - The "Config values are build-time for `HashTrie`" gotcha: add
    "`HashTrieConfig::child_capacity` (`--ds-config child-capacity=tuples`,
    axis `ds_config_child_capacity`, default `grow`; #107), read by the bulk
    build's `child` and by lazy `resolve`" to its list of Config values.

- [ ] **Step 5b: `USAGE.md`, `BENCHMARKING.md`, `ARCHITECTURE.md`.** Each
  still gives `hash-trie=serial` as the default, a spelling the binary now
  rejects.
  - `USAGE.md` (~202–206, the `--ds-build` paragraph): the HashTrie clause
    becomes "`hash-trie=bulk` (the default, Algorithm 2), `incremental` (the
    per-tuple build, `serial` before #107; requires `--ds-config
    child-capacity=grow`), `radix:<bits>`, `parallel:<threads>` or
    `presized:<threads>` (…the rest as it stands)". Wherever `USAGE.md`
    lists the `--ds-config` keys (`git grep -n root-capacity USAGE.md`), add
    `child-capacity=grow|tuples`.
  - `BENCHMARKING.md`:
    - ~215: "HashTrie's (`hash-trie=bulk` by default, Algorithm 2;
      `hash-trie=incremental` for the per-tuple build, `serial` before
      #107; …".
    - The Scaling section: "The trie is identical to the serial build's" →
      "to the structure's single-threaded build's (TreeTrie `serial`,
      HashTrie `bulk`)"; the presized baseline "is `serial` under the same
      `root-capacity=tuples` config" → "is `bulk` under the same
      `root-capacity=tuples` config"; "plot the speedup over `serial`" →
      "plot the speedup over the single-threaded build (`kl.speedup_table`
      picks TreeTrie `serial`, HashTrie `bulk`)"; in the HashTrie
      paragraph, "`parallel:1` against `serial`" → "`parallel:1` against
      `bulk`". The TreeTrie shell loop stays as it is.
    - The kermit-lab table row for `speedup_table`: signature
      `speedup_table(df, *, phase="insertion", baseline=None)`, "over each
      structure's default build (TreeTrie `serial`, HashTrie `bulk`)".
    - Wherever it lists the `--ds-config` keys, add `child-capacity`.
  - `ARCHITECTURE.md` ~228: the sentence on `HashTrie`'s axes, already stale
    (it predates #92 and #88), becomes "`HashTrie<H, P, E>` has three Layout
    axes — the hasher `H` (`SipHashStrategy` default, `FxHashStrategy`), the
    pruning policy `P` and the expansion policy `E` — and three Config axes,
    the load factor, the root capacity and the child capacity; its
    `HasOptimizationAxes` impl reports `ds_layout_hasher`,
    `ds_layout_pruning`, `ds_layout_expansion`, `ds_config_load_factor`,
    `ds_config_root_capacity` and `ds_config_child_capacity`." Keep the rest
    of the paragraph.

- [ ] **Step 6: The spec's status line.** In
  `docs/specs/2026-10-07-hash-trie-algorithm-2-build-design.md`: "**Status:**
  Design approved in conversation (2026-10-07); implemented
  (`docs/superpowers/plans/2026-10-07-hash-trie-algorithm-2-build.md`)." Note
  any deviation from the spec that the plan made: Algorithm 2 lives in
  `bulk.rs` (as the spec says), `HashTable::map_in_runs` is the run-wise form,
  and the presized fill takes and returns its root by value.

- [ ] **Step 7: Check and commit.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command env RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  CARGO_BUILD_JOBS=2 nix develop $WT --command env RUSTFLAGS=-Dwarnings cargo clippy --all-targets
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings
  git -C $WT add docs/data-structures/hash-trie.md docs/data-structures/parallel-build.md docs/specs/optimization-standard.md docs/specs/bench-report-schema.md CLAUDE.md docs/specs/2026-10-07-hash-trie-algorithm-2-build-design.md
  git -C $WT commit -m "docs: HashTrie builds by Algorithm 2; child-capacity; bulk and incremental (#107)"
  ```
  The binary rustdoc has two known errors from before this plan; it must not
  have more.

---

# Controller

## Task 13 [controller]: Gate, scope diff, smoke run, checkpoint 3

- [ ] **Step 1: The gate.**
  ```bash
  nix flake update rust-overlay --flake $WT   # CI floats nightly; a stale lock skews fmt
  nix develop $WT --command cargo fmt --all --check
  CARGO_BUILD_JOBS=2 nix develop $WT --command env RUSTFLAGS=-Dwarnings cargo clippy --all-targets --verbose
  CARGO_BUILD_JOBS=2 nix develop $WT --command env RUSTDOCFLAGS=-Dwarnings cargo doc --workspace
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {print "cargo "p" passed, "f" failed, "i" ignored"}'
  KERMIT_BIN=$WT/target/debug/kermit uv --directory $WT/python/kermit-lab run pytest -q
  # No Miri: the user dropped it, CI job included, on 2026-10-07 (no unsafe in the workspace).
  ```
  If `flake.lock` changed, commit it separately ("chore(nix): refresh
  rust-overlay") only if the user agrees; otherwise revert it before the
  checkpoint. Compare the test count with `$SCRATCH/baseline.txt`. (Miri was
  dropped on 2026-10-07; P1 and P2 ran it clean at e24ccc9 and 346603a.)

- [ ] **Step 2: Scope diff.**
  ```bash
  git -C $WT diff $(cat $SCRATCH/base.sha) --stat -- kermit-ds/src/ds/tree_trie kermit-ds/src/ds/column_trie kermit-algos kermit-ds/src/morsel.rs
  ```
  Expected: empty. Then check that `incremental` runs today's code: in
  `$SCRATCH/implementation.base.rs`, the body of `from_tuples_with_config`
  and of `insert_at` must equal `from_tuples_incrementally`'s and
  `insert_at`'s now, apart from the function name and doc. Diff the two
  function bodies by eye or with a short script; report the result.

- [ ] **Step 3: Smoke run (one run per arm; a go/no-go, not a measurement).**
  First check the host is quiet (ground rules). Then:
  ```bash
  S=$SCRATCH/smoke; mkdir -p $S
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build --release -p kermit
  cp $WT/target/release/kermit $S/kermit
  D=/tb/Source/Academia/kermit-bench-runs/tree-trie-scaling-2026-10-05/data
  C=$HOME/.cache/kermit/benchmarks/watdiv-stress-100-test-1
  CRIT=(--sample-size 10 --measurement-time 3 --warm-up-time 1)
  for rel in $D/unary-1e7.csv $D/binary-1e6.csv $C/friendof.parquet; do
    r=$(basename $rel); r=${r%%.*}
    for mode in bulk incremental; do
      KERMIT_WORKSPACE=$S env -C $S ./kermit bench "${CRIT[@]}" --report-json $S/$r-$mode.json \
        --name smoke-$r-$mode ds -r $rel -i hash-trie -m insertion --ds-build hash-trie=$mode
    done
  done
  for cc in grow tuples; do
    KERMIT_WORKSPACE=$S env -C $S ./kermit bench "${CRIT[@]}" --report-json $S/friendof-bulk-$cc.json \
      --name smoke-friendof-bulk-$cc ds -r $C/friendof.parquet -i hash-trie -m insertion space \
      --ds-config child-capacity=$cc
  done
  uv --directory $WT/python/kermit-lab run python -c "
  import kermit_lab as kl, glob
  df = kl.load(sorted(glob.glob('$S/*.json')), criterion_root='$S/target/criterion')
  print(df[['relation_path','ds_build_mode','ds_config_child_capacity','phase','mean_ns']].to_string())
  "
  ```
  Report `bulk` ÷ `incremental` `insertion` per relation, and `tuples` ÷ `grow`
  for `insertion` and `space` on `friendof`. One run per arm cannot tell a
  ±5 % change from noise; a `bulk` ratio above about 1.2 on any relation is
  a go/no-go question for the user.

- [ ] **Step 4: Checkpoint 3 (controller → user).** Report: the SHAs, the gate
  (counts), the scope diff, the mutants (M2a–M9), the smoke ratios,
  and anything the plan did differently from the spec, with reasons. Ask
  whether to land.

## Task 14 [controller]: Landing (only on the user's word)

- [ ] **Step 1:** `git -C $WT fetch origin && git -C $WT merge origin/master`;
  if anything came in, re-run Task 13's gate (Step 1).
- [ ] **Step 2:** `git -C $WT push origin HEAD:master` (the user's established
  path for loom work; never force).
- [ ] **Step 3a (user decision, 2026-10-07):** on #105, reword the Build table's
  parallel-build row so it credits the paper only with hash partitioning
  (§3.3.2): "Input partitioned by the first attribute's hash, morsel-driven
  (§3.3.2) ✓; partitions as contiguous root regions filled in parallel, and the
  deferred tail, are kermit's". The 2026-10-06 presized spec carries a dated
  note to the same effect (d0a6df6's sibling commit).
- [ ] **Step 3:** On #107, comment the landing SHA, the closest-to-paper
  configuration and the smoke ratios (labelled as a smoke run). On #105, tick
  "each table sized once" (with the configuration) and "Algorithm 2", and mark
  the #66 row as unblocked. Close #107 only once the user says the replicated
  measurement is not required for closing.

---

## After this plan: the replicated measurement

Use the perf comparison recipe: one binary, a distinct `--name` per run, ≥ 5
replicates in alternating arm order, jemalloc, and the 2026-10-06 run's
scripts as the template
(`kermit-bench-runs/hash-trie-presized-scaling-2026-10-06/`):

1. `bulk` vs `incremental` `insertion` on the 12 relations, plus a shuffled
   `friendof` (which bears on #101's locality hypothesis);
2. `child-capacity=tuples` vs `grow`: `insertion`, `space` and `iteration`, at
   load factors 0.8 and 0.7, under `bulk` and `presized:16`;
3. lazy `iteration`, across binaries (before and after this plan), with
   TreeTrie as the control, since expansion changed process in every mode.

Costs to look for, from the Task 2 review (build time only; the finished trie
is unchanged): under pruning, each single-tuple key allocates and frees a
4-slot list that `insert_at` never makes; under lazy expansion, each 1- or
2-tuple pending list pays one extra reallocation for the capacity fix; `map`
holds two bucket arrays at once; and the input buffer and the root's lists
coexist through the root's `group`.

Record the results in `docs/data-structures/hash-trie.md` and post them to
#107 and #105.
