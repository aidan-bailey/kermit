# HashTrie Chain Links (#101 layer 3, B2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every HashTrie list stops being a `Vec<RowId>`. Algorithm 2's per-bucket lists, the leaf chains, the lazy pending lists and the radix family's scratch lists each become a `Chain { head, tail, len }` threaded through one 4-byte link per row, kept in a `ChainLinks` array beside the tuple buffer: the paper's chain pointer (VLDB 2020 §3.3.2, Figure 3), in safe Rust. A build then allocates per table, never per list. `LeafRows` learns to walk a chain, and HashTriejoin's `emit_leaf` moves from indices to cursors. No answer, report field or flag changes.

**Architecture:**
- **One shared radix/parallel helper first (T1).** `radix:K` and `parallel:N` run one partition → scratch-root → merge sequence (`radix::partition_by_first_hash`, `radix::build_and_merge`), deferred from B1's final review. `radix:K` gains `parallel:N`'s early return on an empty batch and its k-way merge. The chain changes then touch one copy.
- **The links (T2).** `kermit_iters::ChainLinks` is `Vec<AtomicU32>`, read and written `Relaxed`, one link per row: the next row in that row's list, or `ChainLinks::NIL`. `ChainWalk` reads a row's link before it yields the row, so a caller may re-thread the rows it walks.
- **The readers (T3).** `LeafRows` gains a linked form beside the listed one, and a `LeafCursor`; `row(i)` goes and `ids()` becomes an iterator. Every reader migrates in the same commit: `emit_leaf` (one cursor per relation, an odometer that resets and carries), the selection view, the tests.
- **The list type (T4).** `hash_trie/chain.rs`: `Chain`, whose pushes append and write `NIL` into the pushed row's link, and `push_row`, Algorithm 2's line 7. A leaf bucket shrinks to 24 B.
- **The switch (T5).** Every HashTrie list becomes a `Chain` over the trie's `links`, in one commit: the eager and lazy builds, `insert`, `resolve`, the iterator, the identity checks, then the radix family.
- **The allocation bounds (T6)** in `kermit/tests/tuple_allocation.rs` are re-derived and tightened.
- **Docs (T7)** and the final gate.
- **Untouched:** TreeTrie, ColumnTrie, `morsel.rs`, the sorted family (`kermit-algos/src/sorted`) and the binary's plumbing (`kermit/src`). `kermit-algos`'s hash family changes in T3 only.

**Tech Stack:** Rust nightly (workspace `rust-toolchain.toml`), the nix dev shell for every cargo command and for rustfmt, `std::sync::atomic::AtomicU32` (no `unsafe`), `allocation_counter` for the allocation tests, Python kermit-lab (uv) for the gate's contract test.

**Spec:** `docs/specs/2026-10-14-hash-trie-chain-links-design.md` (committed at 0e94f30). Read it first. This plan implements it task by task, and every decision it records is settled. Where the drafted parts of this plan disagreed with each other or with the code, the choices are listed in the Merge notes at the end.

---

## Ground rules for the executor

- **Paths.** `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/hash-trie-paper-parity_18dc25f8653dd5c4`.
  - Every command runs from `$WT`; the commit steps that write `git -C $WT …` and those that write plain `git …` mean the same tree.
  - `SCRATCH` is the executing session's scratchpad.
  - `BASE=0e94f30`, the spec commit this plan starts from. The scope checks diff against it.
- **Branch:** work on the loom branch `aidanb/hash-trie-paper-parity`. Never switch branches, never amend, never push.
- **Before Task 1:** this plan is new and uncommitted. Unless the user has already committed it, commit it first, by name:
  - `git add docs/superpowers/plans/2026-10-14-hash-trie-chain-links.md`;
  - message `docs: plan #101 B2, chain links`, with the two trailer lines below.
- **Host:** before any compile or test run, check that no timing run is active:
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  ```
  It must print `0`. If not, wait: a compile under a timing run contaminates it.
- **Builds:** every cargo command runs inside the dev shell with two jobs, in the foreground: `nix develop --command env CARGO_BUILD_JOBS=2 cargo …`. Set `RUSTFLAGS` / `RUSTDOCFLAGS` inside the `env`, never outside `nix develop`:
  - clippy: `nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets`;
  - doc: `nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`.

  Long runs (the full workspace test) go through `setsid nohup … & disown`, with a `kill -0` watcher.
- **Formatting:** `nix develop --command cargo fmt --all` only. Stable rustfmt rewrites ~30 files.
- **Doc comments:** backtick every code identifier (clippy `doc_markdown` runs under `-Dwarnings`). `wrap_comments` wraps at 80 columns, so write comments that fit, and accept `cargo fmt`'s re-wrapping. Name the paper (VLDB 2020 §3.2.2 / §3.3.2, Algorithm 2's line numbers) where the code follows it, and say "kermit's" where it does not.
- **No `unsafe`.** The links are atomics precisely so that none is needed (spec, Decision 3).
- **Commits.** Conventional commits referencing `(#101)`. Stage files by name; a new file (`kermit-iters/src/chain_links.rs`, `kermit-ds/src/ds/hash_trie/chain.rs`) is staged by name like any other. Every message ends with these two lines:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_017PLoRDCeuYmtirxRTMXGJj
  ```
- **Mutation checks.** The spec names five (Testing § Mutation checks). Each is run by the task that can first see it fail:

  | # | Mutant | Must fail | Run in |
  | --- | --- | --- | --- |
  | 1 | `ChainWalk::next` reads a row's link after yielding the row | `chain.rs` `rethreading_a_chain_visits_every_row` | T4 |
  | 2 | `Chain::push` omits `links.set(row, ChainLinks::NIL);` | `bulk.rs` `bulk_and_incremental_thread_the_same_links` | C05b |
  | 3 | `advance_cursor` omits the reset on carry | `hash_triejoin.rs` `emit_leaf_resets_each_cursor_on_carry` | T3 |
  | 4 | `HashTrie::resolve` groups through a fresh `ChainLinks` instead of `&self.links` | `implementation.rs` `resolve_rethreads_the_pending_chain_through_a_shared_reference`, and the trace tests in `kermit-ds/tests/hash_trie_tests.rs` | C05b |
  | 5 | `presized:N`'s tail is sorted by new id instead of `origin` | `parallel.rs` `presized_tail_goes_in_input_order_for_every_n` (B1's pin) | C05b |

  The procedure, for each:
  1. Commit first.
  2. Apply the mutant as the exact edit its task gives.
  3. Confirm `git diff` shows it applied, and that the named test fails with the expected message.
  4. Reverse the exact edit, never with `git checkout`.
  5. Re-run the test to green; `git status` must be clean.
- **Never write under `~/.cache/kermit`.** No `--force`, `bench clean` or `bench gen` there, in any task or in the measurement, without asking the user first.
- **Green tree:** every task leaves the workspace compiling with all tests passing, and ends in one commit. **The exception is Task 5:** chunk C05a does not compile on its own and commits nothing; C05b makes it compile, runs everything and makes Task 5's one commit. The same implementer runs both chunks.
- **Scope (Priority 6):**
  - No task touches TreeTrie, ColumnTrie, `kermit-ds/src/morsel.rs`, the sorted family (`kermit-algos/src/sorted`) or the binary's plumbing (`kermit/src`). T7 edits no doc comment in `kermit/src` either.
  - `kermit-algos`'s hash family changes in T3 only: `emit_leaf`, the selection view's `ids()` use and the test call sites.
  - `kermit-iters` changes in T2 (`chain_links.rs`, `lib.rs`), T3 (`leaf_rows.rs`, `lib.rs`) and T7 (one doc comment in `hash_trie.rs`, the README).
- **Worker runs.** From T1 on, the worker-run pins are:
  - `radix:K`: `[1, 1, 1]` (`partition_rows`' two runs, then `dispatch`'s one; before T1 it was `[1, 1]`);
  - `parallel:N`: `[N, N, N]` (unchanged);
  - `presized:3`: `[3, 3, 3, 3]` (unchanged).

## File map

Derived from the contract the tasks were drafted against. Each task's own **Files** list is authoritative for its line numbers.

| File | Responsibility | Tasks |
| --- | --- | --- |
| `kermit-iters/src/chain_links.rs` (new) | `ChainLinks`, `ChainWalk`; unit tests and a doctest | T2 |
| `kermit-iters/src/lib.rs` | `mod chain_links`; re-exports `ChainLinks`, `ChainWalk` (T2) and `LeafCursor` (T3); one crate-doc sentence | T2, T3 |
| `kermit-iters/src/leaf_rows.rs` | `Ids::{Listed, Linked}`, `LeafRows::linked`, `LeafCursor`, `first` / `advance` / `row_at`; `ids()` an iterator; `row(i)` removed; `Debug` by hand; tests and doctest | T3 |
| `kermit-iters/src/hash_trie.rs` | `leaf_tuples`' doc names both forms | T7 |
| `kermit-iters/README.md` | "What lives here" names the tuple batch, `LeafRows` and `ChainLinks` | T7 |
| `kermit-algos/src/hash/hash_triejoin.rs` | `LeafScratch` holds cursors, `chain_lens` removed; `emit_leaf`, `advance_cursor`; tests, `emit_leaf_resets_each_cursor_on_carry` among them | T3 |
| `kermit-algos/src/hash/selection.rs` | `refresh_leaf` filters by `ids().zip(iter())`; tests | T3 |
| `kermit-algos/src/hash/singleton.rs` | its test's `ids()` | T3 |
| `kermit-ds/src/lib.rs` | re-exports `LeafCursor` beside `LeafRows` | T3 |
| `kermit-ds/tests/common/macros.rs` | the hash suite's `row(0)` read | T3 |
| `kermit-ds/src/ds/hash_trie/hash_trie_iter.rs` | iterator tests' `ids()` / rows (T3); `leaf_tuples` lends the linked form (T5) | T3, T5 |
| `kermit-ds/src/ds/hash_trie/radix.rs` | the shared helper, `Entries` moved here, `insert_in_first_appearance_order` deleted (T1); chains over `links` (T5) | T1, T5 |
| `kermit-ds/src/ds/hash_trie/parallel.rs` | calls the shared helper (T1); chains over `links`, `push_in_run`, the concurrency docs (T5) | T1, T5 |
| `kermit-ds/src/ds/hash_trie/chain.rs` (new) | `Chain`, `push_row`, unit tests, the 24-byte bucket pin (T4); the `dead_code` allows removed (T5) | T4, T5 |
| `kermit-ds/src/ds/hash_trie/mod.rs` | `mod chain;` | T4 |
| `kermit-ds/src/ds/hash_trie/expansion.rs` | `PendingChild` over `Chain`; `LazyChild { pending: Cell<Option<Chain>>, built }` | T5 |
| `kermit-ds/src/ds/hash_trie/node.rs` | `Leaf(HashTable<Chain>)` | T5 |
| `kermit-ds/src/ds/hash_trie/bulk.rs` | `TupleList` deleted, `links` threaded, the lazy `shrink_to` case deleted; `bulk_and_incremental_thread_the_same_links` (T5); the module doc, if T5 left it (T7) | T5, T7 |
| `kermit-ds/src/ds/hash_trie/implementation.rs` | the `links` field, `insert`, `insert_at`, `resolve`, the walks, `heap_size_bytes`; lazy test `resolve_rethreads_the_pending_chain_through_a_shared_reference` (T5); the `# Storage` paragraph (T7) | T5, T7 |
| `kermit-ds/src/ds/hash_trie/identity.rs` | `Rows`; chains compared through each trie's links | T5 |
| `kermit-ds/tests/*` | only where T5's switch needs it | T5 |
| `kermit/tests/tuple_allocation.rs` | the HashTrie bounds and comments re-derived | T6 |
| `docs/data-structures/hash-trie.md`, `docs/algorithms/hash-triejoin.md`, `docs/data-structures/parallel-build.md` | per-component docs | T7 |
| `CLAUDE.md`, `ARCHITECTURE.md`, `docs/specs/bench-report-schema.md` | workspace docs and the "4 (no bump)" row | T7 |
| `docs/specs/2026-10-14-hash-trie-chain-links-design.md` | status line; Amendment 1 (as built) | T7 |
| `docs/superpowers/plans/2026-10-14-hash-trie-chain-links.md` | this plan | before T1 |

## Execution chunks

The unit a subagent implements and reviewers check. Each chunk is given its task text with the ground rules, the file map and the merge notes. A second chunk is given its task's preamble (the text before Step 1) as well as its own steps.

- **C05a** switches the core files to `Chain` (`expansion.rs`, `node.rs`, `bulk.rs`, `implementation.rs`, `hash_trie_iter.rs`, `identity.rs` and their tests). The tree does not compile, because `radix.rs` and `parallel.rs` still build `Vec` lists; nothing is committed.
- **C05b** switches `radix.rs` and `parallel.rs`, runs everything, commits Task 5 and runs mutation checks 2, 4 and 5.
- **C07a** edits the Rust doc comments and `hash-trie.md`. The tree compiles; nothing is committed.
- **C07b** edits the other docs, sweeps, lints and makes Task 7's one commit, staging C07a's files too.

| Chunk | Plan text | Compiles and commits |
| --- | --- | --- |
| C01 | Task 1 | yes |
| C02 | Task 2 | yes |
| C03 | Task 3 | yes |
| C04 | Task 4 | yes (mutation check 1) |
| C05a | Task 5's preamble, then Chunk C05a: Steps 1–13 (the core files) | does not compile, no commit |
| C05b | Task 5's preamble, then Chunk C05b's preamble and Steps 14–29 (`radix.rs`, `parallel.rs`; run; commit; mutation checks 2, 4, 5) | yes, Task 5's one commit |
| C06 | Task 6 | yes |
| C07a | Task 7, preamble and Steps 1–2 (Rust doc comments; `hash-trie.md`) | compiles, no commit |
| C07b | Task 7, preamble and Steps 3–11 (`hash-triejoin.md`; `parallel-build.md`; `CLAUDE.md`; `ARCHITECTURE.md`; `bench-report-schema.md`; READMEs; the spec; sweep; lint; commit) | yes, Task 7's one commit |
| C08 | Final gate | runs the gate |

"After this plan" is not a chunk: every step in it waits for the user's word.

---

# Part T1–T3 of the B2 plan (#101 layer 3, chain links)

Drafted against HEAD 0e94f30. Every line number is as of HEAD unless a step says
"after Task N". `$WT` and `$SCRATCH` are as in the ground rules.

---

### Task 1: One partition-and-merge path for `radix:K` and `parallel:N`

A refactor deferred from B1's final review (minor 7). Today `radix::fill_root`
(`radix.rs:50-114`) and `parallel::fill_root_in_morsels` (`parallel.rs:188-250`)
each write the partition closure `(H::hash(row[0]) >> (64 - bits)) as usize`,
and each runs partition → scratch roots → merge, with two different merges: radix
sorts every root entry (`insert_in_first_appearance_order`, `radix.rs:169-181`),
parallel runs a k-way merge (`merge_in_first_appearance_order`,
`parallel.rs:252-283`). After this task:

- `radix.rs` holds the shared steps: `partition_by_first_hash` (step 1, the one
  copy of the partition closure) and `build_and_merge` (steps 2–3: `dispatch` over
  the non-empty partitions, `Entries::take_from`, free `origin` and `offsets`, the
  k-way merge). `Entries` and `merge_in_first_appearance_order` move there from
  `parallel.rs`; `take_in_arrival_order` becomes private (only
  `Entries::take_from` calls it). The sort-based
  `insert_in_first_appearance_order` is deleted.
- `radix:K` merges by the same k-way merge as `parallel:N`, over its `2^K`
  partitions' lists. The insertion order is the same (each partition's list is
  already in first-appearance order, since the copy is stable), so the root is
  bucket for bucket what it was: no test that compares tries changes. It is a
  process change for `radix:K`'s merge, O(D log P) heap operations for D root
  keys over P non-empty partitions in place of an O(D log D) sort, which the
  B1+B2 measurement covers.
- `radix:K` returns early on an empty batch, as `parallel:N` (`parallel.rs:192-196`)
  and `presized:N` (`parallel.rs:349-352`) already do.
- Worker runs: `radix:K` = `[1, 1, 1]` (`partition_rows`' histogram and scatter,
  then `dispatch`), `[]` on an empty batch; `parallel:N` = `[N, N, N]` and
  `presized:3` = `[3, 3, 3, 3]`, unchanged.

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/radix.rs`
  - the whole non-test part, lines 1–181 (module doc, `use`, `Arrival`,
    `fill_root`, `build_scratch_root`, `take_in_arrival_order`,
    `insert_in_first_appearance_order`);
  - `mod tests`: one new test after `radix_build_rejects_a_wrong_arity` (280–289).
- Modify: `kermit-ds/src/ds/hash_trie/parallel.rs`
  - module doc, lines 1–5 and 20–21;
  - the non-test `use`, lines 62–76;
  - `Entries` and its `impl`, lines 108–152 (deleted);
  - `fill_root_in_morsels` and `merge_in_first_appearance_order`, lines 186–283
    (the first replaced, the second deleted);
  - test-module `use`, lines 452–471 (`LoadFactor` added);
  - `build_modes_reach_their_builds`, lines 708–761 (its doc and the radix row).

**Unchanged, and why each still holds:**
- `radix_builds_the_bulk_trie_under_{siphash,fxhash,colliding_hashes}`
  (`radix.rs:254-278`): same insertion order into the root, so the same root.
- `radix_keeps_the_partitioned_copy_as_its_buffer` (`radix.rs:314-341`): its
  own partition closure stays (contract). On the `"empty"` input the build now
  returns the input batch itself: equal (empty, same arity) to the copy
  `partition_rows` makes, at the input's capacity.
- `parallel_build_of_nothing_starts_no_worker` (`parallel.rs:765-777`).
- `kermit/tests/tuple_allocation.rs`, the `radix:2` cell
  (`hash_trie_builds_allocate_per_list_not_per_row`, 383–436): the build gains
  `dispatch`'s and the k-way merge's allocations (the worker's result list, the
  results vector, the heap, the merge's list of lists, one `Entries` vector per
  partition). Each is sized by the 4 partitions or the root's keys, the same at
  both repetitions, so `many − once` does not move. The comment there ("So are
  the workers, the scratch roots' tables …, the merge") stays true.
- `kermit` (bin) `execution::tests::hash_trie_families_build_with_their_parallel_mode`
  reads only the parallel builds' hook records, which `radix:K` never writes.

- [ ] **Step 1: Check the host, then write the two failing pins.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  ```
  Expected: `0`. If not, wait; do not compile under a timing run.

  In `mod tests` of `kermit-ds/src/ds/hash_trie/radix.rs`, after
  `radix_build_rejects_a_wrong_arity` (280–289), insert:

```rust
    /// The radix build of nothing is the empty trie over the input batch,
    /// built without partitioning or starting a worker, as `parallel:N`'s
    /// is (`parallel_build_of_nothing_starts_no_worker`).
    #[test]
    fn radix_build_of_nothing_starts_no_worker() {
        crate::morsel::take_worker_runs();
        let built: HashTrie = HashTrie::from_tuples_with_build_mode(
            2.into(),
            HashTrieBuildMode::Radix(RadixBits::new(4).unwrap()),
            vec![],
        );
        assert_same_trie(&HashTrie::from_tuples(2.into(), vec![]), &built, "empty");
        assert!(crate::morsel::take_worker_runs().is_empty());
    }
```

  - `RadixBits` and `HashTrie` reach the test module through `super::*`;
    `HashTrieBuildMode`, `assert_same_trie` and `BuildModeRelation` are already in
    its `use` (`radix.rs:185-200`).
  - `take_worker_runs` is `#[cfg(test)] pub(crate)` (`morsel.rs:36-37`).

  In `mod tests` of `kermit-ds/src/ds/hash_trie/parallel.rs`, replace
  `build_modes_reach_their_builds` (708–761) with:

```rust
    /// Every mode builds the bulk trie's contents, so only the records show
    /// which build ran (`bulk` and `incremental` record nothing here;
    /// `each_build_mode_runs_its_own_build` tells them apart). They also
    /// show that `parallel:N` spreads its work:
    /// - N workers in every step: three `run_workers` calls, `partition_rows`'
    ///   histogram and scatter, then `dispatch`;
    /// - the tuples spread over the partitions.
    ///
    /// The radix build runs the same three steps on one thread (three runs
    /// of one worker: `partition_rows`' two, then `dispatch`), and records
    /// nothing. Eight first keys of eight tuples each fill the eight
    /// partitions of `parallel:2` evenly. `parallel:3` aims at 12 partitions,
    /// rounded up to 16, so key k lands in partition 2k: every other
    /// partition is empty, and the record lists those too. A second thread
    /// count is what shows that N itself reaches the build.
    #[test]
    fn build_modes_reach_their_builds() {
        let tuples: Vec<Vec<usize>> = (0..64).map(|i| vec![i % 8, i]).collect();
        let bulk: HashTrie<TopBitsHash> = HashTrie::from_tuples(2.into(), tuples.clone());
        for (mode, builds, worker_runs) in [
            (HashTrieBuildMode::Bulk, vec![], vec![]),
            (HashTrieBuildMode::Incremental, vec![], vec![]),
            (
                HashTrieBuildMode::Radix(RadixBits::new(3).unwrap()),
                vec![],
                vec![1, 1, 1],
            ),
            (
                HashTrieBuildMode::Parallel(threads(2)),
                vec![ParallelBuild {
                    threads: 2,
                    partition_sizes: vec![8; 8],
                    deferred: None,
                }],
                vec![2, 2, 2],
            ),
            (
                HashTrieBuildMode::Parallel(threads(3)),
                vec![ParallelBuild {
                    threads: 3,
                    partition_sizes: [8, 0].repeat(8),
                    deferred: None,
                }],
                vec![3, 3, 3],
            ),
        ] {
            PARALLEL_BUILDS.with(|builds| builds.borrow_mut().clear());
            crate::morsel::take_worker_runs();
            let built: HashTrie<TopBitsHash> =
                HashTrie::from_tuples_with_build_mode(2.into(), mode, tuples.clone());
            assert_eq!(PARALLEL_BUILDS.with(|b| b.take()), builds, "{mode:?}");
            assert_eq!(crate::morsel::take_worker_runs(), worker_runs, "{mode:?}");
            assert_same_contents(&bulk, &built, &format!("{mode:?}"));
        }
    }
```

  The only changes are the doc's radix sentence and the radix row's
  `vec![1, 1]` → `vec![1, 1, 1]`.

- [ ] **Step 2: Run the pins. Both fail.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds radix_build_of_nothing_starts_no_worker
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds build_modes_reach_their_builds
  ```
  Expected: both FAILED.
  - `radix_build_of_nothing_starts_no_worker` panics at
    `assertion failed: crate::morsel::take_worker_runs().is_empty()`: today's
    radix build runs `partition_rows` on the empty batch, two runs of one worker
    (`[1, 1]`). The `assert_same_trie` before it passes (both buffers are empty
    batches of arity 2 at capacity 0).
  - `build_modes_reach_their_builds` fails ``assertion `left == right` failed:
    Radix(RadixBits(3))`` with `left: [1, 1]`, `right: [1, 1, 1]`.

- [ ] **Step 3: `radix.rs` holds the shared steps.**

  Replace lines 1–181 of `kermit-ds/src/ds/hash_trie/radix.rs` (everything before
  `#[cfg(test)]`) with the following. `build_scratch_root` keeps its body and
  doc; `take_in_arrival_order` keeps its body and doc and loses `pub(super)`;
  `Entries` is `parallel.rs:108-152` with `radix::` dropped from its two calls and
  its doc link pointed at the local `build_scratch_root`;
  `merge_in_first_appearance_order` is `parallel.rs:262-283` with a new doc.

```rust
//! The `radix:K` build of a [`HashTrie`](super::HashTrie), and the steps it
//! shares with `parallel:N` (`parallel.rs`): copy the rows into partitions
//! on their first attribute's hash ([`partition_by_first_hash`], over
//! `morsel::partition_rows`), group each partition into a scratch root and
//! build its children by Algorithm 2 (`bulk.rs`), then merge the scratch
//! roots into the real one ([`build_and_merge`]) (SIGMOD 2020 §3.3.2;
//! issues #91, #101). `radix:K` cuts `2^K` partitions and runs every step on
//! the calling thread; `parallel:N` runs the first two on N threads.
//!
//! The copy is the paper's materialisation step: "the tuples are
//! partitioned based on the hash values of the first join key attribute"
//! (§3.3.2). It becomes the trie's buffer and the input is freed, so each
//! partition's rows are one contiguous range of the buffer, in input order,
//! and every row a partition's build reads is local. One round of `2^K`
//! partitions is kermit's choice; the paper adapts a two-pass radix
//! partitioning (Balkesen et al.).
//!
//! The result is equivalent to the bulk build's trie (Amendment 2): the
//! same tables bucket for bucket and capacity for capacity, and the same
//! tuples in every chain, singleton and pending list, under the copy's row
//! ids. A table's final layout depends only on the order in which its
//! *new* keys arrive — `HashTable::entry_or_insert_with` returns an
//! existing entry before its resize check — so the merge inserts the
//! distinct root keys in the order they first appear in the input, as the
//! bulk build does. The copy records each row's input position (`origin`),
//! and each key is tagged with the position of the row that introduced it.
//! A partition's keys leave its scratch root in that order, so a k-way
//! merge of the partitions' lists restores the input's. Below the root
//! nothing needs care: the copy is stable, so each root key's child is
//! built from the same tuples, in input order, as in the bulk build.

use {
    super::{
        build_mode::RadixBits,
        bulk::TupleList,
        config::{HashTrieConfig, LoadFactor},
        expansion::ExpansionPolicy,
        hash_table::HashTable,
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::PruningPolicy,
    },
    crate::morsel::{dispatch, partition_rows, Partitioned, Threads},
    kermit_iters::{HashStrategy, RowId, Tuples},
    std::{cmp::Reverse, collections::BinaryHeap},
};

/// A root entry ready to merge: the input position of the row whose key
/// first introduced it, its hash, and its finished value.
pub(super) type Arrival<V> = (RowId, u64, V);

/// One partition's root entries, each tagged with the input position at
/// which its key first appeared, in that order. The root is `Inner` for
/// arity ≥ 2, so its values are child nodes (subtries, or under pruning and
/// lazy expansion their `Singleton`s and pending lists), and `Leaf` for
/// arity 1, so its values are chains of row ids.
pub(super) enum Entries<P: PruningPolicy, E: ExpansionPolicy> {
    Children(Vec<Arrival<HashTrieNode<P, E>>>),
    Chains(Vec<Arrival<TupleList>>),
}

impl<P: PruningPolicy, E: ExpansionPolicy> Entries<P, E> {
    /// Moves every entry out of a finished scratch root, in the order its
    /// keys arrived (`first_seen`, from [`build_scratch_root`]).
    fn take_from(scratch: HashTrieNode<P, E>, first_seen: &[(RowId, u64)]) -> Self {
        match scratch {
            | HashTrieNode::Inner(table) => {
                let mut children = Vec::with_capacity(first_seen.len());
                take_in_arrival_order(table, first_seen, &mut children);
                Self::Children(children)
            },
            | HashTrieNode::Leaf(table) => {
                let mut chains = Vec::with_capacity(first_seen.len());
                take_in_arrival_order(table, first_seen, &mut chains);
                Self::Chains(chains)
            },
            | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
                unreachable!("a root is never pruned or unexpanded")
            },
        }
    }

    fn into_children(self) -> Vec<Arrival<HashTrieNode<P, E>>> {
        match self {
            | Self::Children(children) => children,
            | Self::Chains(_) => unreachable!("an Inner root's scratch roots are Inner"),
        }
    }

    fn into_chains(self) -> Vec<Arrival<TupleList>> {
        match self {
            | Self::Chains(chains) => chains,
            | Self::Children(_) => unreachable!("a Leaf root's scratch roots are Leaf"),
        }
    }
}

/// Fills the empty `root` with the rows of `tuples` by the `radix:bits`
/// build, every step on the calling thread, and returns the trie's buffer:
/// the rows copied into partition order. Every row must have `arity`
/// attributes; the caller checks.
pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    tuples: Tuples, root: &mut HashTrieNode<P, E>, arity: usize, bits: RadixBits,
    config: HashTrieConfig,
) -> Tuples {
    if tuples.is_empty() {
        // The bulk build of nothing is the empty root over the empty batch:
        // copy nothing and start no worker, as `parallel:N` does.
        return tuples;
    }
    // 1. Partition, on this thread: copy the rows into `2^bits` partitions on
    //    the top `bits` of their first attribute's hash. Each partition is one
    //    range of the copy, in input order. The whole batch is one morsel:
    //    `partition_rows` keeps a count and a range per (partition, morsel)
    //    pair, which one thread gains nothing from, and at `radix:16` over 10^7
    //    rows in `MORSEL_TUPLES`-row morsels would be about 40 million pairs.
    let one = Threads::new(1).expect("one thread is a valid count");
    let partitioned =
        partition_by_first_hash::<H>(one, &tuples, tuples.len(), u32::from(bits.get()));
    // The copy is the buffer from here on: free the input before the build,
    // so the build holds the rows once.
    drop(tuples);
    // 2–3. Build each partition into a scratch root, then merge, both on
    //      this thread.
    build_and_merge::<H, P, E>(partitioned, root, arity, one, config)
}

/// Step 1 of `radix:K` and `parallel:N`: copies the rows of `tuples` into
/// `2^bits` partitions on the top `bits` bits of their first attribute's
/// hash, each partition's rows in input order, with each copied row's input
/// position in `origin` (`morsel::partition_rows`, on `threads` workers over
/// morsels of `morsel_rows` rows). `bits` must be in `1..=16`.
pub(super) fn partition_by_first_hash<H: HashStrategy>(
    threads: Threads, tuples: &Tuples, morsel_rows: usize, bits: u32,
) -> Partitioned {
    debug_assert!(
        (1..=16).contains(&bits),
        "partition_by_first_hash: {bits} bits, not 1 to 16"
    );
    let shift = 64 - bits;
    partition_rows(threads, tuples, morsel_rows, 1 << bits, |row| {
        (H::hash(row[0]) >> shift) as usize
    })
}

/// Steps 2 and 3 of `radix:K` and `parallel:N`, over the rows `partitioned`
/// copied. On `threads` workers ([`dispatch`]), each non-empty partition's
/// id range is grouped into a scratch root of `root`'s kind and its
/// children are built by Algorithm 2 ([`build_scratch_root`]); its entries
/// then leave in the order their keys arrived, each tagged with its key's
/// first input position (`origin`). On the calling thread, a k-way merge
/// inserts every entry into the empty `root` in first-appearance order
/// ([`merge_in_first_appearance_order`]). Returns the copy, the trie's
/// buffer: every id in `root` names a row of it.
pub(super) fn build_and_merge<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    partitioned: Partitioned, root: &mut HashTrieNode<P, E>, arity: usize, threads: Threads,
    config: HashTrieConfig,
) -> Tuples {
    // An empty partition would build and drop an empty scratch root.
    let ranges: Vec<_> = (0..partitioned.partitions())
        .map(|p| partitioned.ids(p))
        .filter(|ids| !ids.is_empty())
        .collect();
    // 2. Build, a partition per task: each id range into a scratch root,
    //    whose entries then leave in the order their keys arrived, each
    //    tagged with its key's first input position (`origin`), not its id.
    let (rows, origin) = (&partitioned.tuples, &partitioned.origin);
    let entries = dispatch(threads, ranges, |ids| {
        let (scratch, first_seen) =
            build_scratch_root::<H, P, E>(rows, ids, origin, arity, config);
        Entries::take_from(scratch, &first_seen)
    });
    // Every key's first input position is in its entry now, so nothing reads
    // `origin` or `offsets` again: free them before the merge.
    drop(partitioned.origin);
    drop(partitioned.offsets);
    // 3. Merge, on this thread, in first-appearance order.
    match root {
        | HashTrieNode::Inner(table) => {
            let lists = entries.into_iter().map(Entries::into_children).collect();
            merge_in_first_appearance_order(table, lists, config.load_factor);
        },
        | HashTrieNode::Leaf(table) => {
            let lists = entries.into_iter().map(Entries::into_chains).collect();
            merge_in_first_appearance_order(table, lists, config.load_factor);
        },
        | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
            unreachable!("a root is never pruned or unexpanded")
        },
    }
    partitioned.tuples
}

/// Builds one partition, the rows `ids` of `tuples`, into a scratch root of
/// the real root's kind by Algorithm 2 (`bulk.rs`): the ids grouped at the
/// root, recording for each key the input position of the row that
/// introduced it (`origin[id]`) and the key's hash, in arrival order; then
/// each key's child built from its list, as the bulk build builds it.
/// `ids` must list the partition in input order.
pub(super) fn build_scratch_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    tuples: &Tuples, ids: impl IntoIterator<Item = RowId>, origin: &[RowId], arity: usize,
    config: HashTrieConfig,
) -> (HashTrieNode<P, E>, Vec<(RowId, u64)>) {
    // Lines 3–7 at the root. The scratch root starts at 4 buckets and
    // grows: its entries move into the real root, so its size is never
    // seen.
    let mut lists: HashTable<TupleList> = HashTable::new();
    let mut first_seen = Vec::new();
    for row in ids {
        let hash = H::hash(tuples.row(row)[0]);
        let keys_before = lists.len();
        lists
            .entry_or_insert_with(hash, config.load_factor, Vec::new)
            .push(row);
        if lists.len() > keys_before {
            first_seen.push((origin[row as usize], hash));
        }
    }
    // Lines 8–15.
    let scratch = HashTrie::<H, P, E>::build_nested(tuples, 0, arity, lists, config);
    (scratch, first_seen)
}

/// Moves every entry out of `scratch` into `out`, tagged with the input
/// position of the row whose key first introduced it. Bucket positions are
/// read before the table is consumed, and each bucket is taken exactly
/// once.
fn take_in_arrival_order<V>(
    scratch: HashTable<V>, first_seen: &[(RowId, u64)], out: &mut Vec<Arrival<V>>,
) {
    let positions: Vec<usize> = first_seen
        .iter()
        .map(|&(_, hash)| {
            scratch
                .index_of(hash)
                .expect("every arrival is in its scratch table")
        })
        .collect();
    let mut buckets = scratch.into_buckets();
    for (&(first, hash), position) in first_seen.iter().zip(positions) {
        let entry = buckets[position].take().expect("each bucket is taken once");
        debug_assert_eq!(entry.hash, hash);
        out.push((first, hash, entry.value));
    }
}

/// Inserts every partition's entries into the real root in the order their
/// keys first appeared in the input, the order the bulk build inserts
/// them, so the root's buckets, length and capacity are the bulk build's.
/// Each entry's tag is an input position (`origin`, through
/// [`build_scratch_root`]), not an id into the copy: ids run
/// partition-major, so merging on them would insert each partition's keys
/// in turn. Each list is already in that order, so a k-way merge suffices:
/// a heap pop and push per entry over at most P lists, O(D log P) for D
/// root keys. It runs on the calling thread: the build's sequential step.
fn merge_in_first_appearance_order<V>(
    root: &mut HashTable<V>, lists: Vec<Vec<Arrival<V>>>, load_factor: LoadFactor,
) {
    let mut lists: Vec<_> = lists
        .into_iter()
        .map(|list| list.into_iter().peekable())
        .collect();
    // Each list's next first-appearance position, smallest on top. Positions
    // are distinct, so no two heads tie.
    let mut heads: BinaryHeap<Reverse<(RowId, usize)>> = lists
        .iter_mut()
        .enumerate()
        .filter_map(|(list, entries)| entries.peek().map(|&(first, ..)| Reverse((first, list))))
        .collect();
    while let Some(Reverse((_, list))) = heads.pop() {
        let (_, hash, value) = lists[list].next().expect("a head names a non-empty list");
        root.entry_or_insert_with(hash, load_factor, || value);
        if let Some(&(first, ..)) = lists[list].peek() {
            heads.push(Reverse((first, list)));
        }
    }
}
```

  Notes on the code:
  - `Partitioned` has no `Drop` impl, so `build_and_merge` moves `origin` and
    `offsets` out once `dispatch` has returned (the borrows in `rows` and
    `origin` end there), and `tuples` last; the same partial moves
    `parallel.rs:233-249` makes today.
  - `dispatch` needs `Entries<P, E>: Send` and a `Sync` task; both hold under
    these bounds today, since `parallel.rs:226-230` makes the same call.
  - `fill_root`'s morsel is `tuples.len()`, no longer `tuples.len().max(1)`: the
    early return guarantees a non-empty batch, and `partition_rows` asserts only
    that a morsel holds a row (`morsel.rs:297-300`).
  - `RadixBits` lies in `1..=16` (`build_mode.rs:63-72`) and `partition_bits`
    gives 2 to 12 (`parallel.rs:154-161`), so the `debug_assert!` holds for both
    callers and `64 - bits` never shifts by 64.

- [ ] **Step 4: `parallel.rs` calls the shared steps.**

  All edits are in `kermit-ds/src/ds/hash_trie/parallel.rs`.

  **The module doc, lines 1–5.** Before:

```rust
//! The `parallel:N` and `presized:N` builds of a [`HashTrie`](super::HashTrie):
//! `parallel:N` is the radix build's three steps (`radix.rs`), with the
//! partition and build steps run on N threads by the morsel-driven helpers
//! of `crate::morsel` (Leis et al., SIGMOD 2014; SIGMOD 2020 §3.3.2; issue
//! #94).
```

  After:

```rust
//! The `parallel:N` and `presized:N` builds of a [`HashTrie`](super::HashTrie):
//! `parallel:N` is the radix build's three steps, through the helpers it
//! shares with `radix:K` (`radix::partition_by_first_hash` and
//! `radix::build_and_merge`), with the partition and build steps run on N
//! threads by the morsel-driven helpers of `crate::morsel` (Leis et al.,
//! SIGMOD 2014; SIGMOD 2020 §3.3.2; issue #94).
```

  **The module doc, lines 20–21** (step 3). Before:

```rust
//! 3. **Merge.** The calling thread inserts every entry into the real root in
//!    first-appearance order, by a k-way merge of the partitions' lists.
```

  After:

```rust
//! 3. **Merge.** The calling thread inserts every entry into the real root in
//!    first-appearance order, by a k-way merge of the partitions' lists, as
//!    `radix:K` merges its own.
```

  Steps 1 and 2 of the module doc stay: `[`partition_rows`]` and `[`dispatch`]`
  still resolve, since `presized:N` keeps both imports.

  **The non-test `use`, lines 62–76.** Before:

```rust
use {
    super::{
        bulk::TupleList,
        config::{HashTrieConfig, LoadFactor},
        expansion::ExpansionPolicy,
        hash_table::{home_bucket, BucketRun, HashTable, Overflow, RunEntry},
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::PruningPolicy,
        radix::{self, Arrival},
    },
    crate::morsel::{dispatch, partition_rows, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD},
    kermit_iters::{HashStrategy, RowId, Tuples},
    std::{cmp::Reverse, collections::BinaryHeap, ops::Range},
};
```

  After (`LoadFactor`, `Arrival`, `Reverse` and `BinaryHeap` were used only by
  `Entries` and the merge, which move; `presized:N` still uses `TupleList`,
  `dispatch`, `partition_rows`, `RowId` and `Range`):

```rust
use {
    super::{
        bulk::TupleList,
        config::HashTrieConfig,
        expansion::ExpansionPolicy,
        hash_table::{home_bucket, BucketRun, HashTable, Overflow, RunEntry},
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::PruningPolicy,
        radix,
    },
    crate::morsel::{dispatch, partition_rows, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD},
    kermit_iters::{HashStrategy, RowId, Tuples},
    std::ops::Range,
};
```

  **Delete `Entries` and its `impl`** (lines 108–152, from
  `/// One partition's root entries, each tagged with the input position at`
  through the `impl`'s closing `}`, and the blank line after it). It now lives in
  `radix.rs` (Step 3).

  **Replace `fill_root_in_morsels` and delete `merge_in_first_appearance_order`**
  (lines 186–283, from `/// [`fill_root`] with the morsel size as a parameter,`
  through the merge's closing `}`) with:

```rust
/// [`fill_root`] with the morsel size as a parameter, so tests can cut a
/// small input into many morsels.
fn fill_root_in_morsels<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    tuples: Tuples, root: &mut HashTrieNode<P, E>, arity: usize, threads: Threads,
    morsel_tuples: usize, config: HashTrieConfig,
) -> Tuples {
    if tuples.is_empty() {
        // The bulk build of nothing is the empty root over the empty batch:
        // copy nothing and start no worker.
        return tuples;
    }
    // 1. Partition, on `threads` workers, by the top bits of each row's first
    //    attribute's hash. The rows are copied into partition order, each
    //    partition's in input order, with each copy's input position in
    //    `origin`. The copy is the trie's buffer from here on, so the input is
    //    freed before the build steps.
    let partitioned = radix::partition_by_first_hash::<H>(
        threads,
        &tuples,
        morsel_tuples,
        partition_bits(threads),
    );
    drop(tuples);
    #[cfg(any(test, feature = "test-hooks"))]
    PARALLEL_BUILDS.with(|builds| {
        builds.borrow_mut().push(ParallelBuild {
            threads: threads.get(),
            partition_sizes: partition_sizes(&partitioned),
            deferred: None,
        });
    });
    // 2–3. Build each partition into a scratch root on `threads` workers,
    //      then merge on this thread.
    radix::build_and_merge::<H, P, E>(partitioned, root, arity, threads, config)
}
```

  `fill_root` (174–184), `partition_bits`, `partition_sizes` and everything from
  `REGION_BUCKETS` (285) on are unchanged.

  **The test-module `use`, lines 452–471:** `tuples_config` (866–872) names
  `LoadFactor::percent`, which reached the tests through `super::*` until now.
  Replace the line
  `                config::{ChildCapacity, HashTrieConfig, RootCapacity},`
  with
  `                config::{ChildCapacity, HashTrieConfig, LoadFactor, RootCapacity},`.
  No test names `Entries`, `take_in_arrival_order`,
  `merge_in_first_appearance_order`, `Reverse`, `BinaryHeap` or `Arrival`:
  `rg -n 'Entries|take_in_arrival_order|merge_in_first|Reverse|BinaryHeap|Arrival' kermit-ds/src/ds/hash_trie/parallel.rs`
  prints nothing after this step.

- [ ] **Step 5: Run the pins, then both modules' tests, then the crate.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds radix_build_of_nothing_starts_no_worker
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds build_modes_reach_their_builds
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_trie::radix
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_trie::parallel
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds
  ```
  Expected: every `test result: ok`, 0 failed. The radix run includes
  `radix_builds_the_bulk_trie_under_{siphash,fxhash,colliding_hashes}`,
  `radix_keeps_the_partitioned_copy_as_its_buffer`,
  `build_mode_relation_uses_the_default_config`,
  `radix_build_rejects_a_wrong_arity` and `radix_build_of_nothing_starts_no_worker`;
  the parallel run includes every `parallel_*` and `presized_*` test.

- [ ] **Step 6: Run what pins the modes outside `kermit-ds`, then the workspace.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests radix2
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests hashparallel2
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test tuple_allocation
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_hash_trie_build_mode
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit hash_trie_families_build_with_their_parallel_mode
  ```
  Expected: every `test result: ok`, 0 failed.

  Then the workspace, through the long-run procedure:
  ```bash
  setsid nohup nix develop --command env CARGO_BUILD_JOBS=2 cargo test --workspace > $SCRATCH/t1-test.log 2>&1 &
  echo $! > $SCRATCH/t1-test.pid; disown
  while kill -0 "$(cat $SCRATCH/t1-test.pid)" 2>/dev/null; do sleep 30; done
  grep -E '^test result|FAILED|panicked' $SCRATCH/t1-test.log | sort | uniq -c
  ```
  Expected: every `test result:` line reads `ok` with `0 failed`.

- [ ] **Step 7: Lint, format, document.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: all clean. An `unused import` here means a name in Step 4's `use`
  list is still needed (or no longer is); fix the list, not the code. Accept
  whatever `cargo fmt` rewraps.

- [ ] **Step 8: Commit.**
  ```bash
  git add kermit-ds/src/ds/hash_trie/radix.rs kermit-ds/src/ds/hash_trie/parallel.rs
  git commit -m "$(cat <<'EOF'
  refactor(hash-trie): one partition-and-merge path for radix:K and parallel:N (#101)

  radix:K and parallel:N each wrote the partition closure and the
  partition, scratch-root and merge sequence. radix.rs now holds one
  copy of each: partition_by_first_hash, the top-bits partition on the
  first attribute's hash, and build_and_merge, which dispatches every
  non-empty partition into a scratch root and merges the scratch roots
  in first-appearance order. parallel:N's fill_root_in_morsels calls
  both; Entries and merge_in_first_appearance_order moved to radix.rs.

  Two process changes for radix:K, which builds the same trie:
  - it merges by parallel:N's k-way merge, a heap over its 2^K
    partitions' lists, where it sorted every root entry; the insertion
    order, and so the root, is unchanged, and the B1+B2 measurement
    covers the cost;
  - it returns early on an empty batch, as parallel:N and presized:N
    do, so it starts no worker there. Its worker runs are now [1, 1, 1]:
    partition_rows' two and dispatch's one.

  Deferred from B1's final review (minor 7).

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_017PLoRDCeuYmtirxRTMXGJj
  EOF
  )"
  git status --short
  ```
  Expected: `git status --short` prints nothing.

---

### Task 2: `ChainLinks` and `ChainWalk` in `kermit-iters`

Additive: nothing outside `kermit-iters` uses them until Task 4 (`Chain`) and
Task 5 (the switch). `ChainLinks` is one 4-byte link per row of a `Tuples` batch,
the paper's chain pointer (VLDB 2020 §3.3.2, Figure 3) as an array beside the
batch (Decision 1). `ChainWalk` walks one chain, counted by its length, reading
each row's link before it yields the row, so a caller may re-thread the rows it
is given: Algorithm 2's step.

- **Atomics** (Decision 3): `Vec<AtomicU32>`, every access `Relaxed`, so lazy
  expansion can re-thread through `&self` without `unsafe`. `ChainLinks` is then
  `Send + Sync` automatically, which keeps eager tries `Sync` (Task 5's pins).
- **`NIL = RowId::MAX`.** A batch holds at most `u32::MAX` rows
  (`Tuples::from_flat` asserts `len <= RowId::MAX`, `tuples.rs`; `Tuples::push`
  refuses a row past it), so the largest id is `u32::MAX − 1`.
- **`new` zero-fills through the allocator** (`vec![0u32; rows]` is
  `alloc_zeroed`) and collects the `map` into atomics in place (same size and
  alignment). `with_capacity` is for the per-row builds, which `push` one link per
  row.
- **The walk's debug check.** When it reaches its last row it reads that row's
  link (in a debug build only) and asserts it is `NIL`, before yielding it. A
  release build reads `len − 1` links for a chain of `len` rows.

**Files:**
- Create: `kermit-iters/src/chain_links.rs`
- Modify: `kermit-iters/src/lib.rs` (crate doc, lines 12–14; `mod` list, lines
  41–49; `pub use`, lines 51–61)
- Test: `kermit-iters/src/chain_links.rs` (`mod tests`, and a doctest on
  `ChainLinks`)

- [ ] **Step 1: Check the host, then write the failing tests.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  ```
  Expected: `0`.

  Create `kermit-iters/src/chain_links.rs` with the module doc, the `use` and the
  test module only (Step 3 adds the types between the `use` and the tests):

```rust
//! [`ChainLinks`]: one link per row of a tuple batch, which threads a hash
//! trie's lists through the batch, and [`ChainWalk`], one list's rows in
//! chain order.

use {
    crate::tuples::RowId,
    std::sync::atomic::{AtomicU32, Ordering},
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows 2, 0 and 3 of a five-row batch threaded into one chain, as a
    /// list's pushes leave them: each link names the next row, and the
    /// tail's is `NIL`. Rows 1 and 4 are on no list and keep link 0.
    fn chain_2_0_3() -> ChainLinks {
        let links = ChainLinks::new(5);
        links.set(2, 0);
        links.set(0, 3);
        links.set(3, ChainLinks::NIL);
        links
    }

    /// `new` makes one zero link per row, in one allocation of exactly
    /// `rows` links.
    #[test]
    fn new_links_are_zero_one_per_row() {
        let links = ChainLinks::new(5);
        assert_eq!(links.len(), 5);
        assert!(!links.is_empty());
        assert_eq!(links.to_vec(), [0; 5]);
        assert_eq!(links.heap_size_bytes(), 5 * 4);
        let none = ChainLinks::new(0);
        assert!(none.is_empty());
        assert_eq!(none.heap_size_bytes(), 0);
    }

    /// `with_capacity` reserves room and holds no link until `push`; the
    /// per-row builds push one per row, so their links cost what `new`'s do.
    #[test]
    fn with_capacity_holds_no_link_until_pushed() {
        let mut links = ChainLinks::with_capacity(3);
        assert!(links.is_empty());
        assert_eq!(links.heap_size_bytes(), 3 * 4);
        links.push(ChainLinks::NIL);
        links.push(0);
        links.push(ChainLinks::NIL);
        assert_eq!(links.len(), 3);
        assert_eq!(links.to_vec(), [ChainLinks::NIL, 0, ChainLinks::NIL]);
        assert_eq!(
            links.heap_size_bytes(),
            ChainLinks::new(3).heap_size_bytes(),
            "three pushes fit the reserved room"
        );
        assert!(ChainLinks::default().is_empty());
    }

    /// `set` rewrites one link through a shared reference, and `get` reads
    /// it back; the other links do not move.
    #[test]
    fn set_rewrites_one_link_through_a_shared_reference() {
        let links = ChainLinks::new(3);
        let shared = &links;
        shared.set(1, 2);
        assert_eq!(links.get(1), 2);
        assert_eq!((links.get(0), links.get(2)), (0, 0));
        shared.set(1, ChainLinks::NIL);
        assert_eq!(links.to_vec(), [0, ChainLinks::NIL, 0]);
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn a_row_without_a_link_panics() { let _ = ChainLinks::new(2).get(2); }

    /// A walk yields its chain in chain order, exactly `len` rows, and stops.
    /// It never reads past the tail: the tail's link is `NIL`, and a walk that
    /// read on would index the links at `NIL` and panic.
    #[test]
    fn a_walk_yields_its_chain_in_chain_order() {
        let links = chain_2_0_3();
        assert_eq!(links.walk(2, 3).collect::<Vec<_>>(), [2, 0, 3]);
        let mut walk = links.walk(2, 3);
        assert_eq!(walk.len(), 3);
        assert_eq!(walk.next(), Some(2));
        assert_eq!(walk.size_hint(), (2, Some(2)));
        assert_eq!((walk.next(), walk.next()), (Some(0), Some(3)));
        assert_eq!(walk.len(), 0);
        assert_eq!((walk.next(), walk.next()), (None, None), "a finished walk stays finished");
        // A chain's tail alone is a chain of one row.
        assert_eq!(links.walk(3, 1).collect::<Vec<_>>(), [3]);
    }

    /// An empty walk reads no link at all, not even its head's: over no
    /// links, any read would panic.
    #[test]
    fn an_empty_walk_reads_nothing() {
        let none = ChainLinks::default();
        assert_eq!(none.walk(ChainLinks::NIL, 0).len(), 0);
        assert_eq!(none.walk(ChainLinks::NIL, 0).next(), None);
        assert_eq!(chain_2_0_3().walk(2, 0).count(), 0);
    }

    /// The rule the walk exists for: it reads a row's link before it yields
    /// the row, so the caller may re-thread each row as it gets it. Every row
    /// of the chain 0 → 1 → 2 → 3 → 4 → 5 moves, as it is yielded, onto one
    /// of two new chains by parity, the way Algorithm 2 moves a list's rows
    /// onto its children's lists: the moved row's link becomes `NIL`, and its
    /// new chain's old tail links to it. A walk that read a row's link only
    /// after yielding the row would follow the rewritten `NIL`.
    #[test]
    fn the_walk_reads_each_link_before_yielding_its_row() {
        let links = ChainLinks::new(6);
        for row in 0..5 {
            links.set(row, row + 1);
        }
        links.set(5, ChainLinks::NIL);
        // Each new chain's tail, by parity: `NIL` until it has a row.
        let mut tails = [ChainLinks::NIL; 2];
        let mut walked = Vec::new();
        for row in links.walk(0, 6) {
            assert!(
                (row as usize) < links.len(),
                "the walk yielded {row}, which is no row: it read a link after re-threading"
            );
            walked.push(row);
            let tail = &mut tails[row as usize % 2];
            if *tail != ChainLinks::NIL {
                links.set(*tail, row);
            }
            links.set(row, ChainLinks::NIL);
            *tail = row;
        }
        assert_eq!(walked, [0, 1, 2, 3, 4, 5]);
        assert_eq!(links.walk(0, 3).collect::<Vec<_>>(), [0, 2, 4]);
        assert_eq!(links.walk(1, 3).collect::<Vec<_>>(), [1, 3, 5]);
    }

    /// A walk given a `len` short of its chain ends on a row whose link is
    /// not `NIL`: a broken invariant, which a debug build catches before it
    /// yields that row.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "the tail, row 0, does not link to NIL")]
    fn a_tail_that_links_on_panics_in_debug() { let _ = chain_2_0_3().walk(2, 2).count(); }

    /// Two threads thread two halves of one batch through shared links.
    /// Each writes only its own rows' links, so relaxed stores race with
    /// nothing, and the scope's join orders them before the walks after it,
    /// as in the parallel builds.
    #[test]
    fn threads_thread_their_own_rows_through_shared_links() {
        let links = ChainLinks::new(8);
        std::thread::scope(|scope| {
            for half in [0..4, 4..8] {
                let links = &links;
                scope.spawn(move || {
                    let last = half.end - 1;
                    for row in half.start..last {
                        links.set(row, row + 1);
                    }
                    links.set(last, ChainLinks::NIL);
                });
            }
        });
        assert_eq!(links.walk(0, 4).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert_eq!(links.walk(4, 4).collect::<Vec<_>>(), [4, 5, 6, 7]);
    }
}
```

  In `kermit-iters/src/lib.rs`, add `mod chain_links;` as the first line of the
  `mod` list (before `mod hash_strategy;`, line 41). The re-export comes in Step 3.

- [ ] **Step 2: Run the tests to verify they fail.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-iters chain_links
  ```
  Expected: compile errors naming `ChainLinks`: ``error[E0412]: cannot find type
  `ChainLinks` in this scope`` (the helper's return type) and ``error[E0433]:
  failed to resolve: use of undeclared type `ChainLinks` `` (each call).

- [ ] **Step 3: Implement.** In `kermit-iters/src/chain_links.rs`, between the
  `use` block and `#[cfg(test)]`, insert:

```rust
/// One link per row of a tuple batch: the id of the next row in that row's
/// list, or [`NIL`](Self::NIL).
///
/// A hash trie's lists are chains threaded through these links: Algorithm
/// 2's per-bucket lists, the leaf chains and a lazy child's pending rows.
/// This is the paper's tuple chain pointer (Freitag et al., VLDB 2020,
/// §3.3.2 and Figure 3): a list names its first row, each row's link names
/// the next, and a row moves to another list when its link is rewritten, as
/// Algorithm 2 moves "each tuple in the input linked list L … to exactly one
/// of the linked lists that are processed recursively" (§3.2.2). A build
/// therefore allocates one array of links, never a list per bucket. The
/// paper reserves 8 bytes inside each tuple; a 4-byte [`RowId`] in an array
/// beside the batch is kermit's.
///
/// **Why atomics.** Only so that lazy child expansion can re-thread a
/// child's rows through a shared reference (`&self`, inside an iterator's
/// `open`) without `unsafe`. Every access is `Relaxed`. No two threads ever
/// write one link: every row is in exactly one list at a time, and one
/// thread threads each list. Where a parallel build's workers thread rows,
/// the scoped spawn and join that start and end them order every write
/// before the reads that follow. On x86-64 and AArch64 a relaxed load or
/// store compiles to a plain one.
///
/// **`NIL` is never a row.** A batch holds at most `u32::MAX` rows
/// ([`Tuples`](crate::Tuples)), so the largest id is `u32::MAX − 1`.
///
/// ```
/// use kermit_iters::{ChainLinks, RowId};
///
/// // Rows 0, 2 and 3 of a four-row batch, threaded into one chain through a
/// // shared reference.
/// let links = ChainLinks::new(4);
/// links.set(0, 2);
/// links.set(2, 3);
/// links.set(3, ChainLinks::NIL);
/// let chain: Vec<RowId> = links.walk(0, 3).collect();
/// assert_eq!(chain, [0, 2, 3]);
/// ```
#[derive(Debug, Default)]
pub struct ChainLinks(Vec<AtomicU32>);

impl ChainLinks {
    /// The link of a list's last row: there is no next row.
    pub const NIL: RowId = RowId::MAX;

    /// `rows` links, all zero, for a build that threads every row before it
    /// reads any link: Algorithm 2 pushes every row onto a list at the root
    /// first, so the starting values are never observed.
    ///
    /// `vec![0; rows]` asks the allocator for zeroed memory (`alloc_zeroed`),
    /// and the `map` into atomics is collected in place: `AtomicU32` has
    /// `u32`'s size and alignment, so the allocation is reused.
    pub fn new(rows: usize) -> Self {
        Self(vec![0u32; rows].into_iter().map(AtomicU32::new).collect())
    }

    /// No links, with room for `rows`: for the builds that place one row at
    /// a time and [`push`](Self::push) its link first.
    pub fn with_capacity(rows: usize) -> Self { Self(Vec::with_capacity(rows)) }

    /// The number of links: one per row of the batch they thread.
    pub fn len(&self) -> usize { self.0.len() }

    /// Whether there are no links.
    pub fn is_empty(&self) -> bool { self.0.is_empty() }

    /// Row `row`'s link: the next row of its list, or [`NIL`](Self::NIL).
    ///
    /// # Panics
    ///
    /// Panics if `row` has no link.
    #[inline]
    pub fn get(&self, row: RowId) -> RowId { self.0[row as usize].load(Ordering::Relaxed) }

    /// Sets row `row`'s link to `next`, through a shared reference.
    ///
    /// # Panics
    ///
    /// Panics if `row` has no link.
    #[inline]
    pub fn set(&self, row: RowId, next: RowId) {
        self.0[row as usize].store(next, Ordering::Relaxed);
    }

    /// Appends one link, `next`, for a row just appended to the batch.
    /// `insert` passes [`NIL`](Self::NIL): the row is on no list yet.
    pub fn push(&mut self, next: RowId) { self.0.push(AtomicU32::new(next)) }

    /// The `len` rows of the chain that starts at row `head`, in chain
    /// order (see [`ChainWalk`]). `len` must be the chain's length; `head` is
    /// not read when `len` is 0.
    #[inline]
    pub fn walk(&self, head: RowId, len: usize) -> ChainWalk<'_> {
        ChainWalk {
            links: self,
            next: head,
            left: len,
        }
    }

    /// Heap bytes the links hold: their capacity, not their length, at 4
    /// bytes each.
    pub fn heap_size_bytes(&self) -> usize {
        self.0.capacity() * std::mem::size_of::<AtomicU32>()
    }

    /// Every link, in row order, for tests and diagnostics.
    pub fn to_vec(&self) -> Vec<RowId> {
        self.0
            .iter()
            .map(|link| link.load(Ordering::Relaxed))
            .collect()
    }
}

/// The rows of one chain, in chain order: [`ChainLinks::walk`].
///
/// It reads each row's link before it yields the row, so its caller may
/// re-thread the rows it is given, rewriting their links, while the walk is
/// still running. That is Algorithm 2's step (VLDB 2020 §3.2.2): a list is
/// walked, and each of its rows moves onto a child's list on the way.
///
/// It is counted: it yields the `len` rows it was given and never reads a
/// link past the last one's. A debug build reads the last row's link too,
/// before yielding that row, and asserts it is [`ChainLinks::NIL`].
pub struct ChainWalk<'a> {
    links: &'a ChainLinks,
    /// The row to yield next; meaningless once `left` is 0.
    next: RowId,
    /// How many rows remain to yield.
    left: usize,
}

impl Iterator for ChainWalk<'_> {
    type Item = RowId;

    #[inline]
    fn next(&mut self) -> Option<RowId> {
        if self.left == 0 {
            return None;
        }
        let row = self.next;
        self.left -= 1;
        if self.left > 0 {
            // Read before `row` is yielded: the caller may re-thread `row`
            // (rewrite its link) before it asks for the next one.
            self.next = self.links.get(row);
        } else {
            debug_assert_eq!(
                self.links.get(row),
                ChainLinks::NIL,
                "ChainWalk: the tail, row {row}, does not link to NIL"
            );
        }
        Some(row)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) { (self.left, Some(self.left)) }
}

impl ExactSizeIterator for ChainWalk<'_> {}
```

  In `kermit-iters/src/lib.rs`:
  - The crate doc's third paragraph (lines 12–14). Before:
    ```rust
    //! A relation is built from a [`Tuples`] batch: its tuples row-major in one
    //! buffer, each row addressed by a [`RowId`]. [`LeafRows`] reads a hash-trie
    //! leaf chain's rows from such a buffer through their ids.
    ```
    After:
    ```rust
    //! A relation is built from a [`Tuples`] batch: its tuples row-major in one
    //! buffer, each row addressed by a [`RowId`]. [`LeafRows`] reads a hash-trie
    //! leaf chain's rows from such a buffer through their ids. [`ChainLinks`]
    //! gives each row of a batch a link to the next row of its list, so a hash
    //! trie can thread its lists through the batch rather than allocate one per
    //! list.
    ```
  - In the `pub use` block (line 52 at HEAD, 56 once the two edits above
    land), before `    hash_strategy::{FxHashStrategy, HashStrategy, SipHashStrategy},`,
    add:
    ```rust
        chain_links::{ChainLinks, ChainWalk},
    ```

  `#![deny(missing_docs)]` (line 39) holds: `ChainLinks`, `NIL`, every `pub fn`
  and `ChainWalk` carry docs; `ChainWalk`'s fields are private, and its trait
  methods need none.

- [ ] **Step 4: Run the tests and the doctest.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-iters chain_links
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-iters --doc ChainLinks
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-iters
  ```
  Expected: `test result: ok` each time. The first run lists the nine tests of
  `chain_links::tests`, `a_tail_that_links_on_panics_in_debug` included (a debug
  build; `should_panic` passes). The doctest run lists
  `src/chain_links.rs - chain_links::ChainLinks (line N)`.

  `the_walk_reads_each_link_before_yielding_its_row` is the walk's own pin of the
  read-before-yield rule (mutation 1, which Task 4 runs against its
  `rethreading_a_chain_visits_every_row`, fails it too; Task 4 Step 7 gives the exact
  mutant).

- [ ] **Step 5: Lint, format, document.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: all clean. `[`Tuples`](crate::Tuples)`, `[`NIL`](Self::NIL)`,
  `[`push`](Self::push)`, `[`ChainWalk`]` and `[`ChainLinks::walk`]` resolve; the
  crate doc's `[`ChainLinks`]` resolves through the re-export.

- [ ] **Step 6: Commit.**
  ```bash
  git add kermit-iters/src/chain_links.rs kermit-iters/src/lib.rs
  git commit -m "$(cat <<'EOF'
  feat(kermit-iters): ChainLinks threads a batch's rows into chains (#101)

  ChainLinks holds one 4-byte link per row of a Tuples batch: the id of
  the next row in that row's list, or NIL (RowId::MAX, never a row). It
  is the paper's tuple chain pointer (VLDB 2020 §3.3.2, Figure 3) as an
  array beside the batch, where the paper keeps 8 bytes in each tuple.
  The links are AtomicU32 accessed Relaxed, only so that lazy expansion
  can re-thread a child's rows through &self; no two threads ever write
  one link.

  ChainWalk walks one chain, counted by its length. It reads each row's
  link before it yields the row, so a caller may re-thread the rows it
  is given, as Algorithm 2 does, and a debug build asserts that the
  tail links to NIL.

  Nothing uses them yet: HashTrie's lists become chains later in #101.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_017PLoRDCeuYmtirxRTMXGJj
  EOF
  )"
  git status --short
  ```
  Expected: `git status --short` prints nothing.

---

### Task 3: `LeafRows` walks linked chains through a cursor

`LeafRows` (the contract between a hash trie and HashTriejoin) gains its second
form and loses random access, and every reader moves in this one commit.

- **Two forms.** `Listed(&[RowId])`, built by `new` and `from_parts` as today, for
  a view that holds its own ids (a pruned singleton, the constant view, the
  selection view's filtered ids). `Linked { links, head, len }`, built by the new
  `linked`, for a trie's leaf chain once its lists are chains (Task 5). In this
  task every trie still lends `Listed`: its chains are `Vec<RowId>` until Task 5.
- **A cursor replaces `row(i)`.** On a chain `row(i)` would be O(i), so it goes.
  `first()`, `advance(&mut LeafCursor) -> bool` and `row_at(LeafCursor)` are each
  O(1) in either form. `LeafCursor(usize)` is an index into a listed chain and the
  current row's id in a linked one; `advance` on a linked chain reads one link and
  reports `false` at `NIL`.
- **`ids()` becomes an iterator** (`ExactSizeIterator<Item = RowId> + 'a`): the
  slice for `Listed`, a `ChainWalk` for `Linked`, through a private two-variant
  iterator (`kermit-iters` has no dependencies, so no `either`). `iter()` maps it.
- **`Debug` by hand**, as the list of the chain's rows, so a linked view never
  prints its trie's whole links array.
- **`emit_leaf`** (Algorithm 3 lines 16–19) runs its mixed-base odometer on one
  `LeafCursor` per relation. `LeafScratch` loses `chain_lens`; its cursor vector is
  sized once per join (`Vec::with_capacity(relations)`; a cursor has no default
  position), so a result row still allocates nothing (`result_allocation.rs`).
- **The pin.** `emit_leaf_resets_each_cursor_on_carry`: a product of three leaf
  chains of lengths 1, 2 and 3 emits each of its 6 candidates once, in odometer
  order. It passes on today's index odometer too (Step 1): it pins what the
  rewrite must keep. Mutation 3 (Step 11) fails it.

**Files:**
- Modify: `kermit-iters/src/leaf_rows.rs` (the whole file)
- Modify: `kermit-iters/src/lib.rs` (after Task 2: the crate doc's third
  paragraph, lines 12–17; `leaf_rows::LeafRows,` in the `pub use` block, line 61)
- Modify: `kermit-iters/src/hash_trie.rs` (`leaf_tuples`' doc, lines 67–71)
- Modify: `kermit-ds/src/lib.rs` (line 50, the `kermit_iters::{…}` re-export)
- Modify: `kermit-algos/src/hash/hash_triejoin.rs` (`use`, lines 13–18;
  `LeafScratch`, 79–104; `emit_leaf`, 187–228; `advance_cursor`, 230–245; the
  test `verify_rejects_real_inner_level_collision`, 441–451; a new test after
  `join_for_each_streams_every_verified_row`, which ends at 538)
- Modify: `kermit-algos/src/hash/selection.rs` (`refresh_leaf`, 92–109; the
  `leaf_tuples` comment, 202–203; tests at 410 and 417)
- Modify: `kermit-algos/src/hash/singleton.rs` (test at 185)
- Modify: `kermit-ds/src/ds/hash_trie/hash_trie_iter.rs` (tests at 697 and 717)
- Modify: `kermit-ds/tests/common/macros.rs` (line 1255)

The reader list is the whole of it. `rg -n '\.row\(|\.ids\(\)' --type rust` at
HEAD, read past `Tuples::row` and the pruning payload's `row()`, gives exactly
these `LeafRows::row` / `LeafRows::ids` callers: `hash_triejoin.rs:215, 449`,
`selection.rs:102, 410, 417`, `singleton.rs:185`, `hash_trie_iter.rs:697, 717`,
`macros.rs:1255`, and `leaf_rows.rs`' own doctest and tests. No doctest or doc
outside `leaf_rows.rs` calls either (`hash_trie.rs` only names `LeafRows`).

- [ ] **Step 1: Check the host, then add the `emit_leaf` pin and run it on today's code.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  ```
  Expected: `0`.

  In `mod tests` of `kermit-algos/src/hash/hash_triejoin.rs`, after
  `join_for_each_streams_every_verified_row` (521–538), before the module's
  closing `}` (539), add:

```rust

    /// Algorithm 3 lines 16–19 over three leaf chains of lengths 1, 2 and 3:
    /// `R`'s, `S`'s and `T`'s, one for each of `A`, `B` and `C` below the
    /// shared `X`. `Mod10HashStrategy` puts `S`'s two `B` values (3 and 13)
    /// in one leaf bucket and `T`'s three `C` values (4, 14 and 24) in
    /// another, so the chains are that long, and every one of the 6
    /// candidates verifies: `X` is 1 throughout, and `A`, `B` and `C` each
    /// come from one relation. The odometer turns `R`'s cursor fastest.
    /// `R`'s chain has one row, so every advance carries out of it, and `S`'s
    /// every second advance: each carry must restart the cursor it leaves at
    /// its chain's first row. The candidates come out in odometer order,
    /// each exactly once, every chain in input order.
    #[test]
    fn emit_leaf_resets_each_cursor_on_carry() {
        use kermit_ds::Relation;
        let r = CollidingHashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let s = CollidingHashTrie::from_tuples(2.into(), vec![vec![1, 3], vec![1, 13]]);
        let t =
            CollidingHashTrie::from_tuples(2.into(), vec![vec![1, 4], vec![1, 14], vec![1, 24]]);
        let expected = vec![
            vec![1, 2, 3, 4],
            vec![1, 2, 13, 4],
            vec![1, 2, 3, 14],
            vec![1, 2, 13, 14],
            vec![1, 2, 3, 24],
            vec![1, 2, 13, 24],
        ];

        // `emit_leaf` itself, each iterator walked to its leaf as `enumerate`
        // walks it: `X` at the root, then the relation's own variable.
        let mut iters = vec![r.hash_trie_iter(), s.hash_trie_iter(), t.hash_trie_iter()];
        for it in &mut iters {
            assert!(it.open(), "the X level");
            assert!(it.open(), "the relation's own variable: its leaf");
        }
        let lens: Vec<usize> = iters
            .iter()
            .map(|it| it.leaf_tuples().expect("at its leaf").len())
            .collect();
        assert_eq!(lens, [1, 2, 3], "the chains the test needs");
        let predicate_variables = vec![vec![0, 1], vec![0, 2], vec![0, 3]];
        let mut scratch = LeafScratch::new(iters.len(), 4);
        let mut out = Vec::new();
        emit_leaf(
            &iters,
            &predicate_variables,
            &mut scratch,
            &mut |row: &[usize]| out.push(row.to_vec()),
        );
        assert_eq!(out, expected, "every candidate once, in odometer order");

        // The same product through the join. The lexicographic plan binds
        // `X`, `A`, `B`, `C` in that order, so `R`, `S` and `T` reach their
        // leaves at depths 1, 2 and 3, and `emit_leaf` runs once, at depth 4.
        let query: JoinQuery = "Q(X, A, B, C) :- R(X, A), S(X, B), T(X, C).".parse().unwrap();
        let plan = LexicographicOptimiser.plan(&query, &CatalogStats::default());
        assert_eq!(plan.variable_ordering, [0, 1, 2, 3]);
        let ds: HashMap<String, &CollidingHashTrie> = HashMap::from([
            ("R".to_string(), &r),
            ("S".to_string(), &s),
            ("T".to_string(), &t),
        ]);
        let mut joined = Vec::new();
        HashTriejoin::join_for_each(&plan, query, ds, |row| joined.push(row.to_vec()));
        assert_eq!(joined, expected);
    }
```

  Why the fixture is what the test needs (each fact is asserted, so a change to
  the hash or the plan fails loudly rather than passing vacuously):
  - `CollidingHashTrie` is `HashTrie<Mod10HashStrategy>` (418), whose defaults are
    `NoPruning` and `EagerExpansion` (`implementation.rs:127-131`), so `R`'s
    one-tuple subtrie is a table, not a `Singleton`, and its leaf is a chain.
  - Each root holds one key, `h(1)`, so the first `open` lands on it; the second
    lands on the child's only bucket (`h(2)`, `h(3) = h(13)`, `h(4) = h(14) =
    h(24)`), whose chain lists the relation's rows in input order (Algorithm 2's
    lists keep input order).
  - `analyse` numbers head variables in head order (`X = 0, A = 1, B = 2, C = 3`),
    and the lexicographic plan is the canonical order when the stored column
    orders allow it (`lexicographic.rs:17-26`): `X` precedes `A`, `B` and `C` in
    every atom.
  - Through the join, `X`'s scan picks the first of three size-1 roots (`R`), and
    the head is every variable, so the rows come out as `emit_leaf` emits them.

  Run it on today's code:
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-algos emit_leaf_resets_each_cursor_on_carry
  ```
  Expected: `test result: ok. 1 passed`. Today's index odometer emits the same 6
  rows in the same order; the cursor rewrite must keep that.

- [ ] **Step 2: Write `LeafRows`' failing tests.**

  Replace `mod tests` of `kermit-iters/src/leaf_rows.rs` (113–226, the
  `#[cfg(test)]` line through the end of the file) with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn batch() -> Tuples { Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6], vec![7, 8]]) }

    /// `ids` threaded into one chain over `rows` links, in that order, as a
    /// trie's pushes leave it: each link names the next id and the last is
    /// `NIL`. Rows not in `ids` keep link 0.
    fn thread(rows: usize, ids: &[RowId]) -> ChainLinks {
        let links = ChainLinks::new(rows);
        for pair in ids.windows(2) {
            links.set(pair[0], pair[1]);
        }
        if let Some(&tail) = ids.last() {
            links.set(tail, ChainLinks::NIL);
        }
        links
    }

    /// The chain `ids` of `tuples` in both forms: listed, and linked through
    /// `links` (`thread(tuples.len(), ids)`).
    fn both<'a>(
        tuples: &'a Tuples, ids: &'a [RowId], links: &'a ChainLinks,
    ) -> [LeafRows<'a>; 2] {
        let head = ids.first().copied().unwrap_or(ChainLinks::NIL);
        [
            LeafRows::new(tuples, ids),
            LeafRows::linked(tuples, links, head, ids.len()),
        ]
    }

    #[test]
    fn a_chain_reads_its_rows_in_chain_order() {
        let tuples = batch();
        // Not a palindrome, so a reversed `iter` shows.
        let ids: [RowId; 3] = [2, 0, 3];
        let links = thread(tuples.len(), &ids);
        for leaf in both(&tuples, &ids, &links) {
            assert_eq!(leaf.len(), 3);
            assert!(!leaf.is_empty());
            assert_eq!(leaf.ids().len(), 3);
            assert_eq!(leaf.ids().collect::<Vec<_>>(), [2, 0, 3]);
            assert_eq!(leaf.arity(), 2);
            assert_eq!(leaf.data(), tuples.as_flat());
            assert_eq!(leaf.iter().len(), 3);
            let rows: Vec<&[usize]> = leaf.iter().collect();
            assert_eq!(rows, [&[5, 6][..], &[1, 2][..], &[7, 8][..]]);
            assert_eq!(leaf.to_vecs(), vec![vec![5, 6], vec![1, 2], vec![7, 8]]);
            // A view is `Copy`: borrows and two words, nothing owned.
            let copy = leaf;
            assert_eq!(copy.to_vecs(), leaf.to_vecs());
        }
    }

    /// `first`, `advance` and `row_at` walk a many-row chain in chain order,
    /// in either form, and `first` restarts a cursor that ran past the end.
    #[test]
    fn a_cursor_walks_a_chain_of_many_rows() {
        let tuples = batch();
        let ids: [RowId; 3] = [2, 0, 3];
        let links = thread(tuples.len(), &ids);
        for leaf in both(&tuples, &ids, &links) {
            let mut at = leaf.first();
            assert_eq!(leaf.row_at(at), [5, 6]);
            assert!(leaf.advance(&mut at));
            assert_eq!(leaf.row_at(at), [1, 2]);
            assert!(leaf.advance(&mut at));
            assert_eq!(leaf.row_at(at), [7, 8]);
            assert!(!leaf.advance(&mut at), "past the last row");
            at = leaf.first();
            assert_eq!(at, leaf.first());
            assert_eq!(leaf.row_at(at), [5, 6]);
        }
    }

    /// On a one-row chain the first row is the last: `advance` reports the
    /// end at once.
    #[test]
    fn a_cursor_on_a_one_row_chain_starts_at_its_last_row() {
        let tuples = batch();
        let ids: [RowId; 1] = [1];
        let links = thread(tuples.len(), &ids);
        for leaf in both(&tuples, &ids, &links) {
            let mut at = leaf.first();
            assert_eq!(leaf.row_at(at), [3, 4]);
            assert!(!leaf.advance(&mut at));
            assert_eq!(leaf.to_vecs(), vec![vec![3, 4]]);
        }
    }

    #[test]
    fn an_empty_chain_lends_nothing() {
        let tuples = batch();
        let links = thread(tuples.len(), &[]);
        for leaf in both(&tuples, &[], &links) {
            assert_eq!(leaf.len(), 0);
            assert!(leaf.is_empty());
            assert_eq!(leaf.ids().len(), 0);
            assert_eq!(leaf.ids().next(), None);
            assert_eq!(leaf.iter().next(), None);
            assert!(leaf.to_vecs().is_empty());
        }
    }

    /// An empty chain's `first` is at no row: reading through it panics,
    /// so a reader checks `is_empty` first, as `emit_leaf` does.
    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn the_first_row_of_an_empty_listed_chain_panics() {
        let tuples = batch();
        let leaf = LeafRows::new(&tuples, &[]);
        let _ = leaf.row_at(leaf.first());
    }

    #[test]
    #[should_panic(expected = "out of range for slice of length 8")]
    fn the_first_row_of_an_empty_linked_chain_panics() {
        let tuples = batch();
        let links = ChainLinks::new(tuples.len());
        let leaf = LeafRows::linked(&tuples, &links, ChainLinks::NIL, 0);
        let _ = leaf.row_at(leaf.first());
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn a_row_past_a_listed_chain_panics() {
        let tuples = batch();
        let ids: [RowId; 2] = [0, 1];
        let leaf = LeafRows::new(&tuples, &ids);
        let mut at = leaf.first();
        assert!(leaf.advance(&mut at));
        assert!(!leaf.advance(&mut at));
        let _ = leaf.row_at(at);
    }

    /// Past its end a linked cursor is `NIL`, which names no row of the
    /// buffer.
    #[test]
    #[should_panic(expected = "out of range for slice of length 8")]
    fn a_row_past_a_linked_chain_panics() {
        let tuples = batch();
        let ids: [RowId; 2] = [0, 1];
        let links = thread(tuples.len(), &ids);
        let leaf = LeafRows::linked(&tuples, &links, 0, 2);
        let mut at = leaf.first();
        assert!(leaf.advance(&mut at));
        assert!(!leaf.advance(&mut at));
        let _ = leaf.row_at(at);
    }

    #[test]
    fn from_parts_views_the_same_rows() {
        let tuples = batch();
        let ids: [RowId; 2] = [3, 1];
        let whole = LeafRows::new(&tuples, &ids);
        let parts = LeafRows::from_parts(tuples.as_flat(), tuples.arity(), &ids);
        assert_eq!(parts.to_vecs(), whole.to_vecs());
        assert_eq!(
            (parts.data(), parts.arity()),
            (whole.data(), whole.arity())
        );
        assert!(parts.ids().eq(whole.ids()));
    }

    /// One value held inline, with no batch behind it (the constant view's
    /// leaf in `kermit-algos`).
    #[test]
    fn a_view_needs_no_batch() {
        let value = [7];
        let only: [RowId; 1] = [0];
        let leaf = LeafRows::from_parts(&value, 1, &only);
        assert_eq!(leaf.to_vecs(), vec![vec![7]]);
    }

    /// One links array threads every chain of a trie: two chains over one
    /// batch interleave, and each linked view follows its own.
    #[test]
    fn linked_chains_share_one_links_array() {
        let tuples = batch();
        let links = ChainLinks::new(tuples.len());
        for (row, next) in [(0, 2), (2, ChainLinks::NIL), (3, 1), (1, ChainLinks::NIL)] {
            links.set(row, next);
        }
        let first = LeafRows::linked(&tuples, &links, 0, 2);
        let second = LeafRows::linked(&tuples, &links, 3, 2);
        assert_eq!(first.to_vecs(), vec![vec![1, 2], vec![5, 6]]);
        assert_eq!(second.to_vecs(), vec![vec![7, 8], vec![3, 4]]);
        assert_eq!(second.ids().collect::<Vec<_>>(), [3, 1]);
    }

    /// `Debug` prints the chain's rows in chain order, in either form: not
    /// the ids, and never a linked view's whole links array.
    #[test]
    fn debug_prints_the_rows_of_the_chain() {
        let tuples = batch();
        let ids: [RowId; 2] = [2, 0];
        let links = thread(tuples.len(), &ids);
        for leaf in both(&tuples, &ids, &links) {
            assert_eq!(format!("{leaf:?}"), "[[5, 6], [1, 2]]");
        }
        let links = thread(tuples.len(), &[]);
        for leaf in both(&tuples, &[], &links) {
            assert_eq!(format!("{leaf:?}"), "[]");
        }
    }

    #[test]
    #[should_panic(expected = "range end index 4 out of range for slice of length 2")]
    fn an_id_past_the_buffer_panics() {
        let leaf = LeafRows::from_parts(&[1, 2], 2, &[1]);
        let _ = leaf.row_at(leaf.first());
    }

    /// Whole rows only: three values are not rows of arity 2. Asserted in
    /// debug builds only.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "3 values are not whole rows of arity 2")]
    fn a_buffer_of_partial_rows_panics_in_debug() {
        let _ = LeafRows::from_parts(&[1, 2, 3], 2, &[]);
    }

    /// A linked view reads its rows' links from an array of one link per row
    /// of its batch. Asserted in debug builds only.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "LeafRows::linked: 3 links for 4 rows")]
    fn a_linked_view_needs_a_link_per_row_in_debug() {
        let tuples = batch();
        let links = ChainLinks::new(3);
        let _ = LeafRows::linked(&tuples, &links, 0, 0);
    }

    /// At arity 0 every id reads the one empty row, in either form.
    #[test]
    fn a_nullary_chain_reads_empty_rows() {
        let leaf = LeafRows::from_parts(&[], 0, &[5, 9]);
        assert_eq!(leaf.to_vecs(), vec![Vec::<usize>::new(); 2]);
        let tuples = Tuples::from_flat(0, 3, Vec::new());
        let links = thread(tuples.len(), &[2, 0]);
        let linked = LeafRows::linked(&tuples, &links, 2, 2);
        assert_eq!(linked.to_vecs(), vec![Vec::<usize>::new(); 2]);
        assert_eq!(linked.ids().collect::<Vec<_>>(), [2, 0]);
    }

    /// Rows, ids and the buffer borrow the buffer (and a linked view's
    /// links), not the view: a caller keeps them after the view it read them
    /// through is gone, in either form. This compiles only if `row_at`,
    /// `iter`, `ids` and `data` lend `'a`, not a borrow of the view.
    #[test]
    fn rows_outlive_the_view() {
        fn check<'a>(view: impl Fn() -> LeafRows<'a>) {
            let row = {
                let leaf = view();
                leaf.row_at(leaf.first())
            };
            let all: Vec<&[usize]> = {
                let leaf = view();
                leaf.iter().collect()
            };
            let (data, mut ids) = {
                let leaf = view();
                (leaf.data(), leaf.ids())
            };
            let mut rows = {
                let leaf = view();
                leaf.iter()
            };
            assert_eq!(row, [7, 8]);
            assert_eq!(all, [row]);
            assert_eq!(ids.next(), Some(3));
            assert_eq!(rows.next(), Some(row));
            assert_eq!(
                LeafRows::from_parts(data, 2, &[3]).to_vecs(),
                vec![vec![7, 8]]
            );
        }
        let tuples = batch();
        let ids: [RowId; 1] = [3];
        let links = thread(tuples.len(), &ids);
        check(|| LeafRows::new(&tuples, &ids));
        check(|| LeafRows::linked(&tuples, &links, 3, 1));
    }
}
```

  What changed from today's tests (113–226):
  - Every test that read `row(i)` reads through a cursor; `a_row_past_the_chain_panics`
    is now `a_row_past_a_listed_chain_panics`, with a linked twin.
  - `a_chain_reads_its_rows_in_chain_order`, `an_empty_chain_lends_nothing`,
    `a_nullary_chain_reads_empty_rows` and `rows_outlive_the_view` run both forms;
    `ids()` is collected (it is an iterator now).
  - New: the cursor tests on many-row, one-row and empty chains, the two
    `first`-of-empty panics, `linked_chains_share_one_links_array`,
    `debug_prints_the_rows_of_the_chain` and the debug-only link-count check.

- [ ] **Step 3: Run the tests to verify they fail.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-iters leaf_rows
  ```
  Expected: compile errors: ``cannot find type `ChainLinks` in this scope``
  (E0412) and ``failed to resolve: use of undeclared type `ChainLinks` `` (E0433),
  since `leaf_rows.rs` does not import it yet; ``no function or associated item
  named `linked` `` (E0599); ``no method named `first` `` / `advance` / `row_at`
  (E0599); and ``no method named `collect` found for reference `&[u32]` `` (E0599,
  `ids()` is still a slice).

- [ ] **Step 4: Implement `LeafRows`' two forms and `LeafCursor`.**

  Replace lines 1–111 of `kermit-iters/src/leaf_rows.rs` (everything before
  `#[cfg(test)]`) with:

```rust
//! [`LeafRows`]: the rows of one hash-trie leaf chain, read from the batch
//! that holds them, and [`LeafCursor`], a position in one.

use {
    crate::{
        chain_links::{ChainLinks, ChainWalk},
        tuples::{RowId, Tuples},
    },
    std::{fmt, iter::Copied, slice},
};

/// A leaf chain's tuples: the chain's rows, in chain order, read from the
/// row-major buffer that holds them ([`Tuples`]'s layout).
///
/// A `HashTrie` keeps its relation's batch and names each tuple by its row
/// id (#111). A view names its chain's rows in one of two forms:
/// - **listed**: a slice of their ids ([`new`](Self::new),
///   [`from_parts`](Self::from_parts)), for a view that holds its own ids,
///   such as a filtered chain or one row held inline;
/// - **linked**: the chain's first row and length, the rest reached through
///   the batch's [`ChainLinks`] ([`linked`](Self::linked)), for a list
///   threaded through its trie's links (VLDB 2020 §3.3.2).
///
/// Either way it is a few borrows and two words, nothing copied. The rows it
/// lends borrow the buffer, not the view, so they outlive it.
///
/// Rows are read in chain order, by [`iter`](Self::iter) or through a
/// [`LeafCursor`]: [`first`](Self::first), [`advance`](Self::advance) and
/// [`row_at`](Self::row_at), each O(1). There is no random access, since the
/// `i`-th row of a linked chain is `i` links away. `Debug` prints the rows.
///
/// The accessors are `#[inline]`: this crate is separate from its callers and
/// the workspace builds without LTO, and a join's leaf loop calls
/// [`row_at`](Self::row_at) once per candidate.
///
/// ```
/// use kermit_iters::{ChainLinks, LeafRows, RowId, Tuples};
///
/// let tuples = Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6]]);
/// // Listed: the ids themselves.
/// let chain: [RowId; 2] = [2, 0];
/// let listed = LeafRows::new(&tuples, &chain);
/// // Linked: the same chain, threaded through one link per row.
/// let links = ChainLinks::new(tuples.len());
/// links.set(2, 0);
/// links.set(0, ChainLinks::NIL);
/// let linked = LeafRows::linked(&tuples, &links, 2, 2);
/// for leaf in [listed, linked] {
///     assert_eq!(leaf.len(), 2);
///     let mut at = leaf.first();
///     assert_eq!(leaf.row_at(at), [5, 6]);
///     assert!(leaf.advance(&mut at));
///     assert_eq!(leaf.row_at(at), [1, 2]);
///     assert!(!leaf.advance(&mut at));
///     assert_eq!(leaf.to_vecs(), vec![vec![5, 6], vec![1, 2]]);
/// }
/// ```
#[derive(Clone, Copy)]
pub struct LeafRows<'a> {
    data: &'a [usize],
    arity: usize,
    ids: Ids<'a>,
}

/// How a [`LeafRows`] names its chain's rows.
#[derive(Clone, Copy)]
enum Ids<'a> {
    /// The ids themselves, in chain order.
    Listed(&'a [RowId]),
    /// A chain threaded through `links`: `len` rows, the first `head`.
    Linked {
        links: &'a ChainLinks,
        head: RowId,
        len: usize,
    },
}

impl<'a> LeafRows<'a> {
    /// The rows `rows` of `tuples`, in the order the ids are listed.
    #[inline]
    pub fn new(tuples: &'a Tuples, rows: &'a [RowId]) -> Self {
        Self::from_parts(tuples.as_flat(), tuples.arity(), rows)
    }

    /// The rows `rows` of the row-major buffer `data`, whose rows hold
    /// `arity` values each: a listed view over a buffer that is not a whole
    /// [`Tuples`], such as one value held inline.
    ///
    /// `data.len()` must be a multiple of `arity` (empty when `arity` is 0),
    /// which a debug build asserts in O(1). The ids are not checked here:
    /// that would cost O(chain) per candidate. Reading a row whose id is past
    /// the end of `data` panics.
    #[inline]
    pub fn from_parts(data: &'a [usize], arity: usize, rows: &'a [RowId]) -> Self {
        // At arity 0 only the empty buffer is a multiple of it.
        debug_assert!(
            data.len().is_multiple_of(arity),
            "LeafRows::from_parts: {} values are not whole rows of arity {arity}",
            data.len()
        );
        Self {
            data,
            arity,
            ids: Ids::Listed(rows),
        }
    }

    /// The `len` rows of `tuples` on the chain that starts at row `head`,
    /// threaded through `links`, in chain order: a hash trie's leaf chain,
    /// read along its links. `links` must hold one link per row of `tuples`,
    /// which a debug build asserts in O(1), and the chain must end at its
    /// `len`-th row, whose link is [`ChainLinks::NIL`]. `head` is not read
    /// when `len` is 0.
    #[inline]
    pub fn linked(tuples: &'a Tuples, links: &'a ChainLinks, head: RowId, len: usize) -> Self {
        debug_assert_eq!(
            links.len(),
            tuples.len(),
            "LeafRows::linked: {} links for {} rows",
            links.len(),
            tuples.len()
        );
        Self {
            data: tuples.as_flat(),
            arity: tuples.arity(),
            ids: Ids::Linked {
                links,
                head,
                len,
            },
        }
    }

    /// The number of rows in the chain.
    #[inline]
    pub fn len(&self) -> usize {
        match self.ids {
            | Ids::Listed(ids) => ids.len(),
            | Ids::Linked {
                len, ..
            } => len,
        }
    }

    /// Whether the chain holds no rows.
    #[inline]
    pub fn is_empty(&self) -> bool {
        match self.ids {
            | Ids::Listed(ids) => ids.is_empty(),
            | Ids::Linked {
                len, ..
            } => len == 0,
        }
    }

    /// A cursor at the chain's first row. An empty chain's is at no row, so
    /// check [`is_empty`](Self::is_empty) before reading through it.
    #[inline]
    pub fn first(&self) -> LeafCursor {
        match self.ids {
            | Ids::Listed(_) => LeafCursor(0),
            | Ids::Linked {
                head, ..
            } => LeafCursor(head as usize),
        }
    }

    /// Moves `cursor` to the chain's next row and returns `true`, or, from
    /// the last row, past the end and returns `false`. One link read for a
    /// linked chain, which ends at the row whose link is
    /// [`ChainLinks::NIL`]. [`first`](Self::first) restarts a cursor past
    /// the end; advancing it again instead is a bug (on a linked chain it
    /// panics).
    #[inline]
    pub fn advance(&self, cursor: &mut LeafCursor) -> bool {
        match self.ids {
            | Ids::Listed(ids) => {
                cursor.0 += 1;
                cursor.0 < ids.len()
            },
            | Ids::Linked {
                links, ..
            } => {
                let next = links.get(cursor.0 as RowId);
                cursor.0 = next as usize;
                next != ChainLinks::NIL
            },
        }
    }

    /// The row `cursor` is at.
    ///
    /// # Panics
    ///
    /// Panics if `cursor` is past the end of a listed chain, or if the row it
    /// names is past the buffer's last row, which a linked chain's
    /// past-the-end cursor ([`ChainLinks::NIL`]) is at every arity from 1. At
    /// arity 0 every id reads the one empty row, so that cursor reads it too.
    #[inline]
    pub fn row_at(&self, cursor: LeafCursor) -> &'a [usize] {
        let id = match self.ids {
            | Ids::Listed(ids) => ids[cursor.0],
            | Ids::Linked {
                ..
            } => cursor.0 as RowId,
        };
        row_of(self.data, self.arity, id)
    }

    /// The chain's row ids, in chain order.
    #[inline]
    pub fn ids(&self) -> impl ExactSizeIterator<Item = RowId> + 'a {
        match self.ids {
            | Ids::Listed(ids) => IdIter::Listed(ids.iter().copied()),
            | Ids::Linked {
                links,
                head,
                len,
            } => IdIter::Linked(links.walk(head, len)),
        }
    }

    /// The buffer the ids index.
    #[inline]
    pub fn data(&self) -> &'a [usize] { self.data }

    /// The number of values in each row.
    #[inline]
    pub fn arity(&self) -> usize { self.arity }

    /// Every row of the chain, in chain order.
    #[inline]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &'a [usize]> + 'a {
        let (data, arity) = (self.data, self.arity);
        self.ids().map(move |r| row_of(data, arity, r))
    }

    /// Every row as its own `Vec`, for tests and diagnostics.
    pub fn to_vecs(&self) -> Vec<Vec<usize>> { self.iter().map(<[usize]>::to_vec).collect() }
}

/// The chain's rows, in chain order: a linked view prints its own rows,
/// never its trie's whole links array.
impl fmt::Debug for LeafRows<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// A position in a [`LeafRows`] chain: an index into a listed chain, the
/// current row's id in a linked one. [`LeafRows::first`] makes one,
/// [`LeafRows::advance`] moves it and [`LeafRows::row_at`] reads through it,
/// each in O(1). It is `Copy`, so a join keeps one per relation in its
/// scratch.
///
/// A cursor means something only for the chain it came from, through any
/// view of that chain: a trie lends a fresh `LeafRows` of the same chain each
/// time it is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafCursor(usize);

/// [`LeafRows::ids`]: a listed chain's ids, or a linked chain's walk.
enum IdIter<'a> {
    Listed(Copied<slice::Iter<'a, RowId>>),
    Linked(ChainWalk<'a>),
}

impl Iterator for IdIter<'_> {
    type Item = RowId;

    #[inline]
    fn next(&mut self) -> Option<RowId> {
        match self {
            | Self::Listed(ids) => ids.next(),
            | Self::Linked(walk) => walk.next(),
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            | Self::Listed(ids) => ids.size_hint(),
            | Self::Linked(walk) => walk.size_hint(),
        }
    }
}

impl ExactSizeIterator for IdIter<'_> {}

/// Row `r` of the row-major `data`, whose rows hold `arity` values.
#[inline]
fn row_of(data: &[usize], arity: usize, r: RowId) -> &[usize] {
    let r = r as usize;
    &data[r * arity..(r + 1) * arity]
}
```

  Notes on the code:
  - The workspace is edition 2021 (`Cargo.toml:16`), so `ids` and `iter`'s
    `impl … + 'a` capture `'a` only, not the `&self` borrow: `rows_outlive_the_view`
    keeps both iterators past the view. `match self.ids` copies `Ids` out (it is
    `Copy`), so the returned iterator borrows nothing of the view.
  - A linked cursor holds a `RowId` in a `usize`, so `cursor.0 as RowId` is
    lossless: every value came from a `RowId`.
  - `is_empty` matches on the form rather than calling `len() == 0`, which
    clippy's `len_zero` could flag.
  - A HashTrie is never nullary (`implementation.rs:55`), so a linked view at
    arity 0 exists only in the test that shows the documented behaviour.

  In `kermit-iters/src/lib.rs` (after Task 2):
  - The crate doc's third paragraph (12–17). Before:
    ```rust
    //! A relation is built from a [`Tuples`] batch: its tuples row-major in one
    //! buffer, each row addressed by a [`RowId`]. [`LeafRows`] reads a hash-trie
    //! leaf chain's rows from such a buffer through their ids. [`ChainLinks`]
    //! gives each row of a batch a link to the next row of its list, so a hash
    //! trie can thread its lists through the batch rather than allocate one per
    //! list.
    ```
    After:
    ```rust
    //! A relation is built from a [`Tuples`] batch: its tuples row-major in one
    //! buffer, each row addressed by a [`RowId`]. [`ChainLinks`] gives each row
    //! of a batch a link to the next row of its list, so a hash trie can thread
    //! its lists through the batch rather than allocate one per list.
    //! [`LeafRows`] reads a hash-trie leaf chain's rows from such a buffer,
    //! named by a list of their ids or reached along their links, in chain
    //! order through a [`LeafCursor`].
    ```
  - In the `pub use` block, `    leaf_rows::LeafRows,` becomes
    `    leaf_rows::{LeafCursor, LeafRows},`.

  In `kermit-iters/src/hash_trie.rs`, `leaf_tuples`' doc (67–71). Before:
  ```rust
      /// Tuple chain at the current leaf bucket, as a [`LeafRows`] view: the
      /// chain's row ids, in chain order, over the tuple buffer they index.
      /// Returns `Some` iff the current node is a leaf and the current bucket
      /// is occupied; otherwise `None`. The view borrows the iterator, so
      /// reading a chain copies and allocates nothing.
  ```
  After:
  ```rust
      /// Tuple chain at the current leaf bucket, as a [`LeafRows`] view: the
      /// chain's rows, in chain order, over the tuple buffer that holds them,
      /// named by a list of their ids or reached along their links
      /// ([`ChainLinks`](crate::ChainLinks)). Returns `Some` iff the current
      /// node is a leaf and the current bucket is occupied; otherwise `None`.
      /// The view borrows the iterator, so reading a chain copies and
      /// allocates nothing.
  ```

  In `kermit-ds/src/lib.rs`, line 50, `    kermit_iters::{LeafRows, RowId, Tuples},`
  becomes `    kermit_iters::{LeafCursor, LeafRows, RowId, Tuples},`.

- [ ] **Step 5: Run `kermit-iters`' tests.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-iters
  ```
  Expected: `test result: ok` for the unit tests (`leaf_rows::tests`' 17 and
  `chain_links::tests`' 9 among them) and for the doctests (`LeafRows`',
  `ChainLinks`', `Tuples`' and the crate's). The rest of the workspace does not
  compile until Steps 6–7 migrate its readers; do not run it yet.

- [ ] **Step 6: `emit_leaf` on cursors.**

  All edits are in `kermit-algos/src/hash/hash_triejoin.rs`.

  **The `use` (13–18).** `kermit_iters::{HashTrieIterable, HashTrieIterator},`
  becomes `kermit_iters::{HashTrieIterable, HashTrieIterator, LeafCursor, LeafRows},`.

  **`LeafScratch` (79–104).** Replace the struct and its `impl` with:

```rust
/// Per-join scratch for the leaf cross product (Algorithm 3 lines 16–19),
/// allocated once in `join_for_each` so that emitting a result tuple
/// allocates nothing (issue #65).
struct LeafScratch {
    /// The mixed-base odometer: `cursor[k]` is a position in the k-th
    /// iterator's current leaf chain.
    cursor: Vec<LeafCursor>,
    /// The candidate's output tuple, in canonical variable order.
    row: Vec<usize>,
    /// `bound[v]` records whether `row[v]` was written for this candidate.
    bound: Vec<bool>,
}

impl LeafScratch {
    /// Scratch for `relations` body predicates over `arity` distinct
    /// variables. A cursor has no default position, so `cursor` starts empty
    /// with room for every relation's, and `emit_leaf` fills it from each
    /// chain's `first` without allocating.
    fn new(relations: usize, arity: usize) -> Self {
        Self {
            cursor: Vec::with_capacity(relations),
            row: vec![0; arity],
            bound: vec![false; arity],
        }
    }
}
```

  **`emit_leaf` and `advance_cursor` (187–245).** Replace both functions with:

```rust
/// Algorithm 3 lines 16–19. Cross-product the leaf chains of every
/// iterator and emit each verified candidate.
///
/// The product is a mixed-base odometer of [`LeafCursor`]s, one per
/// relation, so each chain is read in chain order and no row is reached by
/// index: a linked chain's next row is one link away. Allocates nothing:
/// each chain is re-borrowed through `leaf_tuples()` as a `LeafRows` view
/// over its relation's tuple buffer rather than collected, a candidate's
/// tuples are slices of those buffers, and the cursors and output row live
/// in `scratch`, sized once per join.
fn emit_leaf<IT: HashTrieIterator, S: FnMut(&[usize])>(
    iters: &[IT], predicate_variables: &[Vec<usize>], scratch: &mut LeafScratch, emit: &mut S,
) {
    // Every cursor at its chain's first row. An empty chain leaves no
    // candidate. `cursor` has room for every relation, so `push` never
    // allocates.
    scratch.cursor.clear();
    for it in iters {
        let chain = leaf_chain(it);
        if chain.is_empty() {
            return;
        }
        scratch.cursor.push(chain.first());
    }
    loop {
        let candidate = iters
            .iter()
            .zip(&scratch.cursor)
            .map(|(it, &at)| leaf_chain(it).row_at(at));
        if verify_and_construct(
            candidate,
            predicate_variables,
            &mut scratch.row,
            &mut scratch.bound,
        ) {
            emit(&scratch.row);
        }
        if !advance_cursor(iters, &mut scratch.cursor) {
            break;
        }
    }
}

/// Advances the odometer by one candidate: the first relation's cursor steps
/// along its chain, and a cursor that steps past its chain's last row
/// restarts at the chain's first and carries into the next relation's.
/// Returns `false` once every cursor has carried, after the last candidate.
fn advance_cursor<IT: HashTrieIterator>(iters: &[IT], cursor: &mut [LeafCursor]) -> bool {
    for (it, at) in iters.iter().zip(cursor) {
        let chain = leaf_chain(it);
        if chain.advance(at) {
            return true;
        }
        *at = chain.first();
    }
    false
}

/// `it`'s current leaf chain: `emit_leaf` runs with every iterator on an
/// occupied bucket of its leaf level.
fn leaf_chain<IT: HashTrieIterator>(it: &IT) -> LeafRows<'_> {
    it.leaf_tuples()
        .expect("at leaf level for every participating iter")
}
```

  - The emptiness check and the cursors' start are one pass (the contract's
    "if any chain is empty, return; then each cursor at `first`"), so each chain
    is re-borrowed once before the product, where today's code re-borrowed it
    for `chain_lens` and again per candidate.
  - `scratch.cursor` holds exactly `iters.len()` cursors: `LeafScratch::new` is
    given `iters.len()` (`join_for_each`, 278), and `emit_leaf` pushes one per
    iterator.
  - `*at = chain.first();` is the reset on carry; mutation 3 (Step 11) deletes it.

  **The test `verify_rejects_real_inner_level_collision` (448–451).** Before:

```rust
        assert!(
            !verify_and_construct([r_chain.row(0), s_chain.row(0)], &pv, &mut row, &mut bound),
            "Y = 2 in R but 12 in S: the leaf-level check must reject it"
        );
```

  After:

```rust
        assert!(
            !verify_and_construct(
                [r_chain.row_at(r_chain.first()), s_chain.row_at(s_chain.first())],
                &pv,
                &mut row,
                &mut bound
            ),
            "Y = 2 in R but 12 in S: the leaf-level check must reject it"
        );
```

- [ ] **Step 7: Migrate the other readers.**

  **`kermit-algos/src/hash/selection.rs`, `refresh_leaf` (92–109).** Before:

```rust
            self.leaf_ids.extend(
                chain
                    .ids()
                    .iter()
                    .zip(chain.iter())
                    .filter(|(_, t)| equalities.iter().all(|e| t[e.source] == t[e.repeat]))
                    .map(|(&id, _)| id),
            );
```

  After (`ids()` yields `RowId`s by value now):

```rust
            self.leaf_ids.extend(
                chain
                    .ids()
                    .zip(chain.iter())
                    .filter(|(_, t)| equalities.iter().all(|e| t[e.source] == t[e.repeat]))
                    .map(|(id, _)| id),
            );
```

  **The same file, `leaf_tuples`' comment (202–203).** Before:

```rust
                // The filtered ids index the inner chain's buffer: lend
                // that buffer with them.
```

  After:

```rust
                // The filtered ids index the inner chain's buffer: lend
                // that buffer with them, as a listed view. The ids are the
                // view's own; it cannot thread them through the trie's
                // shared links.
```

  **The same file's tests.** Line 410, `        assert_eq!(rows.ids(), &[1]);`,
  becomes `        assert_eq!(rows.ids().collect::<Vec<_>>(), [1]);`. Line 417,
  `        assert_eq!(chain.ids(), &[0, 1, 2]);`, becomes
  `        assert_eq!(chain.ids().collect::<Vec<_>>(), [0, 1, 2]);`.

  **`kermit-algos/src/hash/singleton.rs`, line 185.**
  `        assert_eq!(chain.ids(), &[0]);` becomes
  `        assert_eq!(chain.ids().collect::<Vec<_>>(), [0]);`.

  **`kermit-ds/src/ds/hash_trie/hash_trie_iter.rs`.** Line 697,
  `        assert_eq!(rows.ids(), &[0, 2]);`, becomes
  `        assert_eq!(rows.ids().collect::<Vec<_>>(), [0, 2]);`. Line 717,
  `        assert_eq!(rows.ids(), &[2]);`, becomes
  `        assert_eq!(rows.ids().collect::<Vec<_>>(), [2]);`. The trie still lends
  `LeafRows::new` over its `Vec<RowId>` chains (`leaf_tuples`, 253–275, unchanged),
  so both tests keep their meaning.

  **`kermit-ds/tests/common/macros.rs`, line 1255.**
  `                        assert_eq!(h(chain.row(0)[0]), key);` becomes
  `                        assert_eq!(h(chain.row_at(chain.first())[0]), key);`.
  (`chain.len()` is asserted to be 1 on the line before.)

  The compiler is the real check (`LeafRows` has no `row` any more, and `ids()`
  is no slice), so a missed reader fails Step 8's build. As a sanity check:
  ```bash
  rg -n '\.row\([0-9i]\)' --type rust kermit-algos kermit-ds kermit-iters kermit
  rg -n 'chain_lens' --type rust .
  ```
  Expected: the first prints only `Tuples::row` calls in
  `kermit-iters/src/tuples.rs` (lines 34, 309, 338, 344 and 353 at HEAD: its
  doctest and tests, untouched); the second prints nothing.

- [ ] **Step 8: Run the readers' tests, the allocation check, then the workspace.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-iters
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-algos
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test result_allocation
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests hash
  ```
  Expected: every `test result: ok`, 0 failed. The `kermit-algos` run includes
  `emit_leaf_resets_each_cursor_on_carry`, the collision tests
  (`verify_rejects_real_inner_level_collision`,
  `join_algo_drops_{inner_level,leaf_chain}_collision_false_positive`) and the
  selection view's `selected_leaf_lends_the_inner_buffer_with_filtered_ids`;
  `result_allocation` holds every HashTriejoin cell at zero allocations per result
  row (one `Vec::with_capacity` per join replaces today's two `vec!`s).

  Then the workspace, through the long-run procedure:
  ```bash
  setsid nohup nix develop --command env CARGO_BUILD_JOBS=2 cargo test --workspace > $SCRATCH/t3-test.log 2>&1 &
  echo $! > $SCRATCH/t3-test.pid; disown
  while kill -0 "$(cat $SCRATCH/t3-test.pid)" 2>/dev/null; do sleep 30; done
  grep -E '^test result|FAILED|panicked' $SCRATCH/t3-test.log | sort | uniq -c
  ```
  Expected: every `test result:` line reads `ok` with `0 failed`. The `panicked`
  lines that appear come from `should_panic` tests only (each such test still
  reports `ok`).

- [ ] **Step 9: Lint, format, document.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: all clean. The doc links `[`ChainLinks`]`, `[`ChainLinks::NIL`]`,
  `[`LeafCursor`]`, `[`LeafRows::first`]` (and `advance`, `row_at`, `ids`),
  `[`linked`](Self::linked)` and `hash_trie.rs`' `[`ChainLinks`](crate::ChainLinks)`
  resolve; `LeafCursor` is public and documented (`#![deny(missing_docs)]`).

- [ ] **Step 10: Commit.**
  ```bash
  git add kermit-iters/src/leaf_rows.rs kermit-iters/src/lib.rs kermit-iters/src/hash_trie.rs \
    kermit-ds/src/lib.rs kermit-algos/src/hash/hash_triejoin.rs kermit-algos/src/hash/selection.rs \
    kermit-algos/src/hash/singleton.rs kermit-ds/src/ds/hash_trie/hash_trie_iter.rs \
    kermit-ds/tests/common/macros.rs
  git commit -m "$(cat <<'EOF'
  feat(kermit-iters): LeafRows walks linked chains through a cursor (#101)

  LeafRows names its rows in one of two forms: listed, a slice of ids,
  for the views that hold their own (a pruned singleton, the constant
  and selection views), or linked, a chain's head and length over a
  ChainLinks array, for a trie's leaf chains once they are threaded.
  row(i) is gone, since the i-th row of a linked chain is i links away:
  LeafCursor replaces it (first, advance, row_at, each O(1)), and ids()
  is an iterator. Debug prints the rows, so a linked view never prints
  its trie's links.

  HashTriejoin's emit_leaf runs its odometer (Algorithm 3 lines 16-19)
  on one cursor per relation, restarting each at first on a carry; the
  cursors live in LeafScratch, sized once per join, so a result row
  still allocates nothing, and chain_lens is gone. Every other reader
  moves with it: the selection view's filter, and the tests of the
  constant view, HashTrieIter and the hash-trie suite.

  The tries still lend the listed form until their lists become chains.
  emit_leaf_resets_each_cursor_on_carry pins the product over leaf
  chains of lengths 1, 2 and 3.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_017PLoRDCeuYmtirxRTMXGJj
  EOF
  )"
  git status --short
  ```
  Expected: `git status --short` prints nothing.

- [ ] **Step 11: Mutation check 3: the cursor does not reset on carry.**

  Follow the ground rules: the commit is made (Step 10); apply the exact edit;
  confirm it applied; see the test fail; reverse the exact edit, never with
  `git checkout`.
  - In `advance_cursor` (`kermit-algos/src/hash/hash_triejoin.rs`), delete the line
    ```rust
            *at = chain.first();
    ```
    (eight spaces of indent, inside the `for` loop, after the `if` block).
  - Check that it applied: `git diff -U0 kermit-algos/src/hash/hash_triejoin.rs`
    shows exactly one removed line, `-        *at = chain.first();`, and no added
    line. The file still compiles without a warning: `chain` is still used by
    `chain.advance(at)`.
  - Run:
    ```bash
    nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-algos emit_leaf_resets_each_cursor_on_carry
    ```
    Expected: FAILED, panicking in `LeafRows::row_at` (`kermit-iters/src/leaf_rows.rs`)
    with `index out of bounds: the len is 1 but the index is 1`. After the first
    candidate, `R`'s cursor carries out of its one-row chain and stays at index 1;
    `S`'s advance succeeds, and the second candidate reads `R` past its end. (Once
    Task 5 makes the tries lend linked chains, the same mutant fails the same test
    with `range end index … out of range for slice of length 2`: `R`'s cursor is
    then `NIL`.)
  - Reverse the exact edit: re-insert `        *at = chain.first();` as the line
    after the `if chain.advance(at) { … }` block's closing `}`. Re-run the test:
    `test result: ok. 1 passed`. Then `git status --short` must print nothing.

---

### Task 4: `Chain`, Algorithm 2's list threaded through links

Additive: nothing outside the new module's own tests calls it until Task 5.
- **The piece.** `kermit-ds/src/ds/hash_trie/chain.rs` defines `Chain { head, tail, len }`, a list of
  rows stored in its bucket, with each row's successor in a `ChainLinks` array (T2). It also
  defines `push_row`, Algorithm 2's line 7 over a `HashTable<Chain>`.
- **Order and `NIL`.** A push appends at the tail, so a list keeps input order (spec Decision 2).
  Every push writes `NIL` into the pushed row's link, so a chain always ends in `NIL` and the
  links are a function of the lists alone. Task 5's array-level identity relies on that.
- **Nominally `pub`.** `Chain` is `pub` inside the private module. From Task 5 on, `PendingChild`
  (`expansion.rs`, a public trait reachable through `ExpansionPolicy`) names it, and a
  `pub(super)` type there trips `private_interfaces` (warn, an error under `-Dwarnings`). That is
  the same reason the policy traits are generic over `N`.
- **Dead code until Task 5.** In the non-test lib target nothing calls `Chain` or `push_row`, so
  the struct, its `impl` and `push_row` carry `#[allow(dead_code)]` with the comment
  `// No caller until the lists become chains (Task 5).`. Task 5 (chunk C05a, Step 2) removes
  all three.
- **`#[inline]`.** Every function is `#[inline]`. Their callers (`group`, `insert_at`) are
  generic, so they are compiled in the crate that names a `HashTrie`, and the workspace builds
  without LTO. This is the reason `LeafRows`' accessors give.
- **Mutation 1 pins here.** `rethreading_a_chain_visits_every_row` walks a chain while it pushes
  every row onto a parity chain. It is the test that sees T2's read-before-yield rule (mutation
  check 1, Step 7).

**Files:**
- Create: `kermit-ds/src/ds/hash_trie/chain.rs`
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs` (the `mod` list, lines 11–24: `mod chain;` after
  `mod bulk;`, line 12)
- Test: `kermit-ds/src/ds/hash_trie/chain.rs` (`mod tests`)

**Preconditions (T2, T3):**
- `kermit_iters::{ChainLinks, ChainWalk}` exist with the contract's API: `new`, `get`, `set`,
  `walk`, `to_vec`, `NIL`. `ChainWalk` is an `ExactSizeIterator`.
- `kermit_iters::LeafRows::linked(tuples, links, head, len)` exists, and `LeafRows::ids()` returns
  an iterator.

- [ ] **Step 1: Check the host and the preconditions.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  rg -n 'pub const NIL|pub fn (new|set|get|walk|to_vec)\b' kermit-iters/src/chain_links.rs
  rg -n 'pub fn linked|pub fn ids' kermit-iters/src/leaf_rows.rs
  rg -n 'chain_links::\{ChainLinks, ChainWalk\}|LeafCursor' kermit-iters/src/lib.rs
  ```
  Expected:
  - the first command prints `0`. If not, wait: do not compile under a timing run;
  - one hit each for `NIL`, `new`, `set`, `get`, `walk` and `to_vec`;
  - `linked` and `ids` in `leaf_rows.rs`;
  - both re-exports in `lib.rs`.

  If a name differs, stop and reconcile with T2/T3 before going on.

- [ ] **Step 2: Create `kermit-ds/src/ds/hash_trie/chain.rs`.**

```rust
//! [`Chain`]: Algorithm 2's linked list `L` (Freitag et al., VLDB 2020,
//! §3.2.2), threaded through one link per row (§3.3.2, Figure 3; #101 B2).
//!
//! A chain is three values stored in its bucket: its first row, its last
//! row and its length. Each row's successor lives in the trie's
//! [`ChainLinks`], one 4-byte link per row of its buffer, so pushing a row
//! rewrites two links and allocates nothing. Algorithm 2 moves each row
//! from its parent's list to exactly one child list by rewriting its link,
//! so one link per row serves every level.
//!
//! Two things that are kermit's:
//!
//! - The links are row ids in an array beside the buffer, 4 bytes each,
//!   where the paper reserves an 8-byte chain pointer in each materialised
//!   tuple.
//! - A push appends at the tail, where line 7 ("push t onto the linked list
//!   stored in B"), read literally, prepends. Appending keeps every list in
//!   input order, so the bulk build stays array-identical to the per-tuple
//!   build (#107).
//!
//! Every push writes `NIL` into the pushed row's link, whatever it held, so
//! a chain always ends in `NIL` and the links are a function of the lists
//! alone.
//!
//! `Chain` is nominally `pub` in this private module: the lazy expansion
//! policy's public `PendingChild` trait (`expansion.rs`) holds one, and a
//! `pub(super)` type there would trip `private_interfaces`, the reason that
//! trait is generic over its node type.
//!
//! The functions are `#[inline]`: the builds that call them are generic, so
//! they are compiled in the crate that names a `HashTrie`, and the
//! workspace builds without LTO.

use {
    super::{config::LoadFactor, hash_table::HashTable},
    kermit_iters::{ChainLinks, ChainWalk, LeafRows, RowId, Tuples},
    std::num::NonZeroU32,
};

/// A list of rows threaded through the trie's `links`, in input order:
/// Algorithm 2's `L`. The rows are ids into the trie's buffer; each row's
/// successor is its link, and the tail's link is `NIL`.
// No caller until the lists become chains (Task 5).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chain {
    /// The first row, where a walk starts.
    head: RowId,
    /// The last row, where a push appends. Its link is `NIL`.
    tail: RowId,
    /// The number of rows: Algorithm 2 line 3's `|L|`, the pruning test and
    /// `LeafRows::len`. Never 0, so `Option<Entry<Chain>>`, a leaf bucket,
    /// uses it as its niche and stays 24 bytes.
    len: NonZeroU32,
}

// No caller until the lists become chains (Task 5).
#[allow(dead_code)]
impl Chain {
    /// The chain of the one row `row`. Writes `NIL` into `row`'s link.
    #[inline]
    pub(super) fn start(links: &ChainLinks, row: RowId) -> Self {
        links.set(row, ChainLinks::NIL);
        Self {
            head: row,
            tail: row,
            len: NonZeroU32::MIN,
        }
    }

    /// Appends `row`: the tail's link becomes `row`, and `row`'s link
    /// becomes `NIL`, whatever it held. `row` must be in no other chain; a
    /// build moves each row to its next list only after the walk of its old
    /// list has read its link (see [`walk`](Self::walk)).
    #[inline]
    pub(super) fn push(&mut self, links: &ChainLinks, row: RowId) {
        links.set(self.tail, row);
        links.set(row, ChainLinks::NIL);
        self.tail = row;
        self.len = self
            .len
            .checked_add(1)
            .expect("a chain holds at most `u32::MAX` rows");
    }

    /// The rows, in chain order: [`ChainLinks::walk`] from the head,
    /// counted by the length. The walk reads each row's link before it
    /// yields the row, so the caller may push each row it is given onto
    /// another chain over the same `links` while the walk runs: Algorithm
    /// 2's re-threading of a bucket's list into the next level's (lines 4–7
    /// over line 10's list).
    #[inline]
    pub(super) fn walk<'l>(&self, links: &'l ChainLinks) -> ChainWalk<'l> {
        links.walk(self.head, self.len())
    }

    /// The number of rows, at least 1: a chain is never empty, so it has no
    /// `is_empty`.
    #[inline]
    pub(super) fn len(&self) -> usize { self.len.get() as usize }

    /// The first row: a pruned `Singleton`'s row when the chain holds one.
    #[inline]
    pub(super) fn head(&self) -> RowId { self.head }

    /// The chain as a leaf's rows (`HashTrieIterator::leaf_tuples`): the
    /// linked `LeafRows` over `tuples`, walked through `links`.
    #[inline]
    pub(super) fn leaf_rows<'a>(&self, tuples: &'a Tuples, links: &'a ChainLinks) -> LeafRows<'a> {
        LeafRows::linked(tuples, links, self.head, self.len())
    }
}

/// Algorithm 2 line 7, and `insert_at`'s leaf step: pushes `row` onto the
/// chain in `hash`'s bucket of `lists`, starting that chain if the bucket is
/// new (the table first grows under `load_factor` if a new key would
/// overfill it). Returns whether it started one, i.e. whether `hash` is a
/// new key of `lists`.
// No caller until the lists become chains (Task 5).
#[allow(dead_code)]
#[inline]
pub(super) fn push_row(
    lists: &mut HashTable<Chain>, hash: u64, load_factor: LoadFactor, links: &ChainLinks,
    row: RowId,
) -> bool {
    let mut fresh = false;
    let chain = lists.entry_or_insert_with(hash, load_factor, || {
        fresh = true;
        Chain::start(links, row)
    });
    if !fresh {
        chain.push(links, row);
    }
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    const NIL: RowId = ChainLinks::NIL;

    /// The rows `chain` walks to, in order.
    fn walked(chain: &Chain, links: &ChainLinks) -> Vec<RowId> { chain.walk(links).collect() }

    /// `start` and `push` keep input order, and chains share one links
    /// array: two chains pushed in turn each walk their own rows.
    #[test]
    fn start_and_push_keep_input_order() {
        let links = ChainLinks::new(5);
        let mut chain = Chain::start(&links, 3);
        assert_eq!((chain.head(), chain.len()), (3, 1));
        for row in [1, 4, 0] {
            chain.push(&links, row);
        }
        assert_eq!((chain.head(), chain.len()), (3, 4));
        assert_eq!(walked(&chain, &links), vec![3, 1, 4, 0]);
        assert_eq!(chain.walk(&links).len(), 4, "an exact-size walk");
        let shared = ChainLinks::new(4);
        let mut a = Chain::start(&shared, 0);
        let mut b = Chain::start(&shared, 1);
        a.push(&shared, 2);
        b.push(&shared, 3);
        assert_eq!(walked(&a, &shared), vec![0, 2]);
        assert_eq!(walked(&b, &shared), vec![1, 3]);
    }

    /// `start` and every `push` write `NIL` into the new row's link,
    /// whatever it held, so every chain ends in `NIL` and the links are a
    /// function of the lists alone. Row 2's link is set to a stale
    /// successor first, as a row an earlier level threaded holds one.
    #[test]
    fn every_chain_ends_in_nil() {
        let links = ChainLinks::new(4);
        links.set(2, 3);
        let mut chain = Chain::start(&links, 1);
        assert_eq!(links.get(1), NIL, "start");
        chain.push(&links, 2);
        assert_eq!(links.get(1), 2, "the old tail links to the new row");
        assert_eq!(links.get(2), NIL, "push overwrote the stale link");
        chain.push(&links, 0);
        // Row 3 is in no chain, so its link keeps `new`'s zero.
        assert_eq!(links.to_vec(), vec![NIL, 2, 0, 0]);
        assert_eq!(walked(&chain, &links), vec![1, 2, 0]);
    }

    /// Algorithm 2's re-threading: a walk over one chain pushes every row
    /// it yields onto one of two chains over the same links, here by
    /// parity. Each push rewrites the yielded row's link, so the walk must
    /// have read that link before it yielded the row (`ChainWalk`'s rule);
    /// a walk that read it afterwards would follow the new chain, not the
    /// old one.
    #[test]
    fn rethreading_a_chain_visits_every_row() {
        let links = ChainLinks::new(6);
        let mut all = Chain::start(&links, 0);
        for row in 1..6 {
            all.push(&links, row);
        }
        let mut lists: HashTable<Chain> = HashTable::new();
        for row in all.walk(&links) {
            push_row(
                &mut lists,
                u64::from(row % 2),
                LoadFactor::DEFAULT,
                &links,
                row,
            );
        }
        let parity = |hash: u64| walked(lists.get(hash).expect("both parities"), &links);
        assert_eq!(parity(0), vec![0, 2, 4]);
        assert_eq!(parity(1), vec![1, 3, 5]);
        assert_eq!(links.to_vec(), vec![2, 3, 4, 5, NIL, NIL]);
    }

    #[test]
    fn push_row_returns_whether_it_started_a_chain() {
        let links = ChainLinks::new(3);
        let mut lists: HashTable<Chain> = HashTable::new();
        assert!(
            push_row(&mut lists, 5, LoadFactor::DEFAULT, &links, 0),
            "a new key"
        );
        assert!(
            !push_row(&mut lists, 5, LoadFactor::DEFAULT, &links, 1),
            "a key it has"
        );
        assert!(
            push_row(&mut lists, 6, LoadFactor::DEFAULT, &links, 2),
            "another new key"
        );
        assert_eq!(lists.len(), 2);
        assert_eq!(walked(lists.get(5).expect("pushed"), &links), vec![0, 1]);
        assert_eq!(walked(lists.get(6).expect("pushed"), &links), vec![2]);
    }

    /// A leaf bucket holds its hash and its chain inline: 8 + 12 bytes,
    /// padded to 24, with `len`'s `NonZeroU32` as the `Option`'s niche. A
    /// `Vec<RowId>` list made it 32, plus a heap allocation per list.
    #[test]
    fn a_leaf_bucket_is_24_bytes() {
        use {super::super::hash_table::Entry, std::mem::size_of};
        assert_eq!(size_of::<Chain>(), 12);
        assert_eq!(size_of::<Option<Entry<Chain>>>(), 24);
        assert_eq!(size_of::<Option<Entry<Vec<RowId>>>>(), 32);
    }

    /// A chain lends its rows as a linked `LeafRows` over the buffer, in
    /// chain order, the rows borrowed, not copied.
    #[test]
    fn leaf_rows_lends_the_chains_rows() {
        let tuples = Tuples::from(vec![vec![1, 2], vec![3, 4], vec![5, 6], vec![7, 8]]);
        let links = ChainLinks::new(tuples.len());
        let mut chain = Chain::start(&links, 2);
        chain.push(&links, 0);
        chain.push(&links, 3);
        let leaf = chain.leaf_rows(&tuples, &links);
        assert_eq!(leaf.len(), 3);
        assert_eq!(leaf.ids().collect::<Vec<_>>(), vec![2, 0, 3]);
        assert_eq!(leaf.to_vecs(), vec![vec![5, 6], vec![1, 2], vec![7, 8]]);
        assert_eq!(leaf.data().as_ptr(), tuples.as_flat().as_ptr());
    }
}
```

  Notes for the implementer:
  - **The closure in `push_row`.** It borrows `fresh` mutably and is moved into
    `entry_or_insert_with`, which consumes it. The returned `&mut Chain` borrows `lists`, not the
    closure, so reading `fresh` afterwards is accepted.
  - **The 24-byte pin.** `hash_table::Entry` is `pub(crate)` with `pub` fields, so the test names
    it through `super::super::hash_table::Entry`. `Chain` is 12 bytes, at alignment 4.
    `Entry<Chain>` is 8 + 12 = 20 bytes, padded to 24 at alignment 8. `NonZeroU32` gives the
    `Option` its niche. `Vec<RowId>` is 24 bytes, so `Entry<Vec<RowId>>` is 32, with `Vec`'s own
    niche.
  - **`len` without `is_empty`.** clippy's `len_without_is_empty` only checks exported methods, and
    these are `pub(super)`.
  - **The test's `1..6`** infers `RowId` (`u32`) from `push`'s parameter, and `row % 2` is a
    `u32`, hence `u64::from`.

- [ ] **Step 3: Register the module.** In `kermit-ds/src/ds/hash_trie/mod.rs`, lines 11–13.
  Before:

```rust
mod build_mode;
mod bulk;
mod config;
```

  After:

```rust
mod build_mode;
mod bulk;
mod chain;
mod config;
```

- [ ] **Step 4: Run the new tests, then the crate's unit tests.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l   # prints 0
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie::chain::
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib
  ```
  Expected:
  - the first run: `test result: ok. 6 passed; 0 failed`;
  - the second: `ok`, `0 failed`. Nothing else uses the module, so no other test can change.

- [ ] **Step 5: Lint, format, document.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: all clean.
  - If clippy names `Chain`, a field, a method or `push_row` as dead code, the item lacks its
    `#[allow(dead_code)]`. Add it with the same comment.
  - After `cargo fmt`, `git status --short` lists only `chain.rs` (new) and `mod.rs`.

- [ ] **Step 6: Commit.**
  ```bash
  git -C $WT add kermit-ds/src/ds/hash_trie/chain.rs kermit-ds/src/ds/hash_trie/mod.rs
  git -C $WT commit -m "$(cat <<'EOF'
  feat(hash-trie): Chain, Algorithm 2's list threaded through links (#101)

  A list of rows as three values in its bucket (head, tail and a
  NonZeroU32 length), with each row's successor in a ChainLinks array, one
  4-byte link per row: the paper's chain pointer (VLDB 2020 §3.3.2,
  Figure 3), beside the tuples rather than inside them. A push appends,
  so lists keep input order, and every push writes NIL into the pushed
  row's link, so a chain ends in NIL and the links are a function of the
  lists alone. push_row is Algorithm 2's line 7, and reports whether it
  started a chain.

  A leaf bucket of chains is 24 bytes (32 for a Vec<RowId>, plus its
  heap buffer), and a test pins it. The re-threading test pins
  ChainWalk's read-before-yield rule. Nothing calls the module until the
  lists become chains.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_017PLoRDCeuYmtirxRTMXGJj
  EOF
  )"
  ```

- [ ] **Step 7: Mutation check 1: the walk reads a row's link after yielding it.** Follow the ground
  rules: the commit is made, then apply the exact edit, confirm it with `git diff`, run the named
  test, reverse the exact edit (never `git checkout`), re-run to green, and check that
  `git status` is clean.

  The mutant is in T2's `kermit-iters/src/chain_links.rs`. It keeps the yielded row and reads its
  link only at the start of the next call.
  1. Print T2's committed text, which the reversal restores:
     ```bash
     git -C $WT show HEAD:kermit-iters/src/chain_links.rs | rg -n -B2 -A24 'impl Iterator for ChainWalk'
     git -C $WT show HEAD:kermit-iters/src/chain_links.rs | rg -n -A8 'pub fn walk'
     git -C $WT show HEAD:kermit-iters/src/chain_links.rs | rg -n -A6 'pub struct ChainWalk'
     ```
  2. Apply three edits:
     - (a) In `pub struct ChainWalk<'a> { … }`, after its last field, add the line
       `    pending: Option<RowId>,`.
     - (b) In `ChainLinks::walk`'s `ChainWalk { … }` literal, after its last field, add
       `            pending: None,`.
     - (c) Replace the whole body of `fn next(&mut self) -> Option<RowId>` (between its braces,
       including T2's debug assertion) with:

```rust
        // Mutant 1: the link of the row last yielded is read only now.
        if let Some(previous) = self.pending.take() {
            self.next = self.links.get(previous);
        }
        if self.left == 0 {
            return None;
        }
        let row = self.next;
        self.left -= 1;
        self.pending = Some(row);
        Some(row)
```

  3. Confirm: `git -C $WT diff --stat` lists only `kermit-iters/src/chain_links.rs`.
  4. Run:
     ```bash
     nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie::chain::
     ```
     Expected: `rethreading_a_chain_visits_every_row` FAILS. Pushing row 0 onto the even chain sets
     its link to `NIL`. The walk's next call then reads that link and yields `NIL` (`u32::MAX`).
     `push_row` then calls `ChainLinks::set` at index 4294967295, which panics with
     `index out of bounds: the len is 6 but the index is 4294967295`. If T2's `set` checks bounds
     differently, the failure is the `[0, 2, 4]` assertion instead.

     The other five tests pass: they rewrite no link while walking. That is the expected outcome,
     and it is why the re-threading test exists.
  5. With the mutant still applied, also run Task 2's own tests:
     ```bash
     nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-iters chain_links
     ```
     Expected: `the_walk_reads_each_link_before_yielding_its_row` FAILS (it panics on its
     second row: the walk yields `NIL`, because row 0's link was re-threaded before the walk
     read it), and `a_tail_that_links_on_panics_in_debug` FAILS (no panic: the mutant's `next`
     drops the tail assertion).
  6. Reverse: delete the two added lines (a) and (b). Restore `next`'s body exactly as Step 1
     printed it. `git -C $WT diff` must print nothing.
  7. Re-run the command in 4: `6 passed`, and the `kermit-iters` command in 5: all pass.
     `git -C $WT status --short` is empty.

---

### Task 5: Every HashTrie list becomes a `Chain`

Task 5 is one commit in two chunks, and one implementer runs both.
- **C05a** (this part) switches every list outside the radix family:
  - `expansion.rs`: the pending chain;
  - `node.rs`: the leaf;
  - `bulk.rs`: Algorithm 2's lists, with `TupleList` deleted;
  - `implementation.rs`: the links, the per-tuple build, `resolve`, the walk and `space`;
  - `hash_trie_iter.rs`: leaves lend `Linked`;
  - `identity.rs`: `Rows`, links on the id-level helpers, the links check;
  - these files' tests.

  It ends with the tree not compiling. `radix.rs` and `parallel.rs` still name `TupleList` and the
  old signatures, and the three fill calls in `implementation.rs` pass a `links` argument those
  files do not take yet. **No commit.**
- **C05b** (the next part) updates `radix.rs` and `parallel.rs` and their tests, then runs the
  full gate, makes Task 5's one commit and runs mutation checks 2, 4 and 5.

**The design in one paragraph.**
- The trie gains `links: ChainLinks` beside `tuples`, one link per row.
- Every list becomes a `Chain` in its bucket: a leaf chain (`Leaf(HashTable<Chain>)`), a lazy
  pending chain (`LazyChild { pending: Cell<Option<Chain>>, … }`) and Algorithm 2's per-bucket
  lists (`group` returns `HashTable<Chain>`).
- **The builds.**
  - `bulk` threads every row at the root (`ChainLinks::new`, zeroed: every link is written before
    it is read), then re-threads each row once per level.
  - `incremental` and `insert` push a `NIL` link per row and thread it once, by `insert_at`.
  - `resolve` re-threads a pending chain through `&self` (spec Decision 3).
- **Array identity.** Every push writes `NIL`, so `links` is a function of the lists, and `bulk`
  and `incremental` end with equal arrays. `assert_same_trie` now compares them whole.
- **`space`.** A chain lives in its bucket, so `heap_size_bytes` is the buffer, the links and the
  tables. Each list's heap buffer, and `child`'s lazy `shrink_to` special case, are gone.

**Files** (line numbers at HEAD 0e94f30; Tasks 1–3 do not touch these files except where noted):
- Modify: `kermit-ds/src/ds/hash_trie/chain.rs` (T4's file): the three `#[allow(dead_code)]`
  attributes and their comments go.
- Replace: `kermit-ds/src/ds/hash_trie/expansion.rs` (whole file, 1–214).
- Modify: `kermit-ds/src/ds/hash_trie/node.rs`:
  - module doc, 1–4;
  - `use`, 15–18;
  - `Leaf`, 27–31;
  - `Unexpanded`'s doc, 44–47.
- Modify: `kermit-ds/src/ds/hash_trie/bulk.rs`:
  - non-test code, 1–161 (replaced);
  - test `use`, 172;
  - `check_identity`'s doc, 196–199;
  - `bulk_keeps_chain_order_under_colliding_hashes`, 291–295;
  - a new test after 295;
  - `tuples_below`, 350–363.
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs`:
  - `use`, 21–43;
  - struct docs, 66–68 and 78–84;
  - fields, 133–137;
  - `with_config_for`, 159–178;
  - `tuples()` plus the new `links()`, 184–187;
  - `for_each_tuple` / `visit_at`, 197–239;
  - `insert_at`, 241–358;
  - `resolve`, 360–394;
  - `insert`, 408–436;
  - the three partitioned arms, 516–547;
  - `from_tuples_in_bulk`, 574–598;
  - `from_tuples_incrementally`, 600–625;
  - `from_tuples_partitioned`, 627–651;
  - `HeapSize`, 731–742;
  - `node_heap_bytes` and `tuple_list_heap_bytes`, 800–833;
  - tests:
    - `mod tests`, 1061–1111, 1211–1232, 1463–1466, 1657–1667, 1686–1687;
    - `lazy_tests`, 1851–1862, 1879–1893, 1910–1913, 2003, 2016–2019, and new items after 2030;
    - `root_capacity_tests`, 2135–2139 and 2249–2283.
- Modify: `kermit-ds/src/ds/hash_trie/hash_trie_iter.rs`:
  - `leaf_tuples`, 253–262;
  - tests `a_leaf_chain_lends_row_ids_into_the_trie_buffer` (684–701) and
    `singleton_frames_lend_their_row_id` (703–720), as T3 left them (T3 rewrote their `ids()`
    assertions).
- Replace: `kermit-ds/src/ds/hash_trie/identity.rs` (whole file, 1–650).
- Check, no change: `kermit-ds/tests/*.rs` and `kermit-ds/tests/common/macros.rs` (Step 11).
- Left for C05b: `kermit-ds/src/ds/hash_trie/radix.rs`, `kermit-ds/src/ds/hash_trie/parallel.rs`.

**Preconditions (T1–T4):**
- T1: `radix.rs` has `partition_by_first_hash`, `build_and_merge` and `Entries`.
  `radix::fill_root(tuples, root, arity, bits, config) -> Tuples` and
  `parallel::fill_root` / `fill_presized_root` keep their pre-T1 signatures. C05b adds `links`.
- T2: `kermit_iters::{ChainLinks, ChainWalk}` with the contract's API.
- T3: `LeafRows::linked`, `LeafCursor`, and `ids()` as an iterator. Every `LeafRows` reader is
  migrated to the cursor or the iterator, and tries still lend `Listed`.
- T4: `chain.rs`, with `Chain`, `push_row` and the three dead-code allows.

**Tests that pin behaviour and must pass at C05b's end, unchanged** (why each holds):
- `bulk::tests::bulk_builds_the_incremental_trie_*` (three) and
  `bulk_builds_the_incremental_trie_on_wide_and_skewed_inputs`. They pass through
  `assert_same_trie`, which now also requires equal links: both builds thread each row into the
  same final list, and every push writes `NIL`.
- `bulk::tests::tuples_sizes_every_child_from_its_list`. `build_child_table` sizes from
  `chain.len()`, which is the list length the `Vec` had.
- `implementation.rs`:
  - `partitioning_builds_keep_the_inputs_spare_capacity`: every mode's links are
    `ChainLinks::new(len)`, so the heap sizes stay equal;
  - `insert_after_a_partitioning_build_appends_to_the_copy`: `insert` pushes a link on both
    sides, so both grow alike;
  - `eager_tries_stay_sync`: `AtomicU32` is `Sync`;
  - `nodes_are_send_under_every_layout`: `Cell<Option<Chain>>` is `Send`;
  - `node_does_not_grow_under_*`, `each_build_mode_runs_its_own_build`, the pruning tests, and
    `lazy_tests` other than the two edited below.
- `kermit-ds/tests/hash_trie_tests.rs`:
  - the `lazy_expansion` trace tests (`assert_lazy_walks_like_eager`): an expanded child's leaf
    chains are the eager ones, walked through re-threaded links;
  - `hash_trie_test_suite!` and `parquet_test_suite!`: public API only.
- `kermit` (bin) `db::database::tests::column_distinct_leaves_a_lazy_trie_as_built`. The
  statistics walk is `TupleScan`, which reads the buffer, and `heap_size_bytes` reads no pending
  chain.
- `kermit/tests/result_allocation.rs`: walking a linked leaf allocates nothing.

#### Chunk C05a: the core files (Steps 1–13; the tree does not compile, no commit)

- [ ] **Step 1: Check the host and the preconditions.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  rg -n 'fn partition_by_first_hash|fn build_and_merge|enum Entries' kermit-ds/src/ds/hash_trie/radix.rs
  rg -n 'pub fn linked' kermit-iters/src/leaf_rows.rs
  rg -n 'allow\(dead_code\)' kermit-ds/src/ds/hash_trie/chain.rs
  git -C $WT status --short
  ```
  Expected:
  - the first command prints `0`;
  - one hit each in `radix.rs`;
  - one hit in `leaf_rows.rs`;
  - three hits in `chain.rs`;
  - a clean tree.

- [ ] **Step 2: `chain.rs`: the allows go.** In `kermit-ds/src/ds/hash_trie/chain.rs`, delete the
  three two-line pairs:

```rust
// No caller until the lists become chains (Task 5).
#[allow(dead_code)]
```

  - the pair above `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct Chain`;
  - the pair above `impl Chain`;
  - the pair above `#[inline] pub(super) fn push_row`.

  After this chunk every item has a non-test caller:
  - `start`: `push_row`, `insert_at`;
  - `push`: `push_row`, `LazyChild::push`, `insert_at`;
  - `walk`: `build_child_table`, `visit_chain`;
  - `len`: `child`, `build_child_table`;
  - `head`: `child`;
  - `leaf_rows`: `HashTrieIter::leaf_tuples`;
  - `push_row`: `group`, `insert_at`.

- [ ] **Step 3: `expansion.rs`: the pending chain.** Replace the whole of
  `kermit-ds/src/ds/hash_trie/expansion.rs` with:

```rust
//! Expansion policy — the third Layout dimension of
//! [`HashTrie`](super::HashTrie).
//!
//! Lazy child expansion (SIGMOD 2020 §3.3.1, Figure 6) builds only the root
//! table at construction. Every child below it keeps its tuples as a chain
//! of rows threaded through the trie's links (`chain.rs`) until a probe
//! first opens it, and only then is that child's table built, one level at
//! a time, re-threading the chain's rows into the child's lists. It is a
//! *shape*: an unexpanded child is a node state, so it is a Layout, not a
//! Config. Under [`EagerExpansion`] the state's payload is the uninhabited
//! [`Never`], the `Unexpanded` variant vanishes from the node's layout, and
//! the instantiation compiles to the eager code. Bench axis:
//! `ds_layout_expansion`.
//!
//! The payload is generic over the node type `N` rather than naming
//! `HashTrieNode`. That keeps these two traits, public but in a private
//! module like `pruning.rs`'s, free of the crate-private node type, which
//! `private_interfaces` would reject. `Chain`, which `PendingChild` does
//! name, is nominally `pub` for the same reason.

use {
    super::{chain::Chain, pruning::Never},
    kermit_iters::{ChainLinks, LayoutOption, RowId},
    std::cell::{Cell, OnceCell},
};

/// What a `HashTrieNode::Unexpanded` holds: `Box<LazyChild<N>>` under
/// [`LazyExpansion`], [`Never`] under [`EagerExpansion`].
pub trait PendingChild<N>: Sized {
    /// An unexpanded child holding `chain`: its tuples' rows in the trie's
    /// buffer, in insertion order, threaded through the trie's links.
    fn from_chain(chain: Chain) -> Self;
    /// The built table, once a probe has expanded this child.
    fn built(&self) -> Option<&N>;
    /// [`built`](Self::built) for `insert`, which holds the trie mutably.
    fn built_mut(&mut self) -> Option<&mut N>;
    /// The chain of rows not yet built into a table; `None` once expanded.
    fn pending(&self) -> Option<Chain>;
    /// Appends `row` to the pending chain, through the trie's `links`.
    /// Callers check [`built_mut`](Self::built_mut) first.
    fn push(&mut self, links: &ChainLinks, row: RowId);
    /// The built table, building it first from the pending chain if no
    /// probe has yet. The chain is taken, so an expanded child keeps no
    /// list, and `build` re-threads its rows through the trie's links,
    /// under the `&self` a probe holds, which the links' atomics allow.
    fn expand(&self, build: impl FnOnce(Chain) -> N) -> &N;
    /// Heap bytes of the payload itself (the box), excluding the built
    /// table, which `heap_size_bytes` counts separately. The pending chain
    /// lives in the box, so it adds nothing.
    fn own_heap_bytes(&self) -> usize;
}

/// Compile-time expansion policy of a [`HashTrie`](super::HashTrie).
///
/// As with `PruningPolicy`, the associated type and the constant must
/// agree: `LAZY == false` requires an uninhabited `Pending`, so no
/// `Unexpanded` node can exist. `PendingChild` lives in this private
/// module, so [`EagerExpansion`] and [`LazyExpansion`] are the only
/// policies and the pairing is checked here, once.
pub trait ExpansionPolicy: LayoutOption + Copy + Default + 'static {
    /// What a `HashTrieNode::Unexpanded` holds under this policy, for a
    /// node type `N`. `Send` whenever `N` is, so a parallel build can move
    /// finished subtries between threads: `LazyChild`'s cells are `Send`,
    /// only `!Sync`.
    type Pending<N: Send>: PendingChild<N> + Send;
    /// Folded by the compiler: `insert_at`'s lazy branches are
    /// `if E::LAZY { … }`.
    const LAZY: bool;
}

impl<N> PendingChild<N> for Never {
    fn from_chain(_chain: Chain) -> Self {
        unreachable!("EagerExpansion never constructs an Unexpanded child (E::LAZY is false)")
    }

    fn built(&self) -> Option<&N> { match *self {} }

    fn built_mut(&mut self) -> Option<&mut N> { match *self {} }

    fn pending(&self) -> Option<Chain> { match *self {} }

    fn push(&mut self, _links: &ChainLinks, _row: RowId) { match *self {} }

    fn expand(&self, _build: impl FnOnce(Chain) -> N) -> &N { match *self {} }

    fn own_heap_bytes(&self) -> usize { match *self {} }
}

/// An unexpanded child: the chain of its tuples' rows until a probe
/// reaches it, the table built from them afterwards.
///
/// `Cell` and `OnceCell` make lazy tries `!Sync` (still `Send`). Nothing
/// in the workspace shares a relation across threads; a parallel prober
/// would need a different cell.
pub struct LazyChild<N> {
    /// The rows below this bucket, in insertion order, threaded through the
    /// trie's links: a `Copy` value, which expansion takes, leaving `None`.
    pending: Cell<Option<Chain>>,
    /// The table this level would have held, once a probe has reached it.
    built: OnceCell<N>,
}

impl<N> PendingChild<N> for Box<LazyChild<N>> {
    fn from_chain(chain: Chain) -> Self {
        Box::new(LazyChild {
            pending: Cell::new(Some(chain)),
            built: OnceCell::new(),
        })
    }

    fn built(&self) -> Option<&N> { self.built.get() }

    fn built_mut(&mut self) -> Option<&mut N> { self.built.get_mut() }

    fn pending(&self) -> Option<Chain> { self.pending.get() }

    fn push(&mut self, links: &ChainLinks, row: RowId) {
        debug_assert!(self.built.get().is_none(), "push into an expanded child");
        self.pending
            .get_mut()
            .as_mut()
            .expect("an unexpanded child holds its pending chain")
            .push(links, row);
    }

    // The chain is taken before `build` re-threads its rows, so no later
    // `pending` (nor `for_each_tuple`) reads it again; until `build`
    // returns, the child is neither pending nor built.
    fn expand(&self, build: impl FnOnce(Chain) -> N) -> &N {
        self.built.get_or_init(|| {
            build(
                self.pending
                    .take()
                    .expect("an unexpanded child holds its pending chain until it is built"),
            )
        })
    }

    fn own_heap_bytes(&self) -> usize { std::mem::size_of::<LazyChild<N>>() }
}

/// Every level built at construction: today's structure. Bench axis value
/// `"eager"`. The default `E`.
#[derive(Copy, Clone, Default, Debug)]
pub struct EagerExpansion;

impl LayoutOption for EagerExpansion {
    const NAME: &'static str = "eager";
}

impl ExpansionPolicy for EagerExpansion {
    type Pending<N: Send> = Never;

    const LAZY: bool = false;
}

/// Lazy child expansion (paper Figure 6). Bench axis value `"lazy"`.
#[derive(Copy, Clone, Default, Debug)]
pub struct LazyExpansion;

impl LayoutOption for LazyExpansion {
    const NAME: &'static str = "lazy";
}

impl ExpansionPolicy for LazyExpansion {
    type Pending<N: Send> = Box<LazyChild<N>>;

    const LAZY: bool = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lazy child over a stand-in node type: the row ids it was built from.
    type Lazy = Box<LazyChild<Vec<RowId>>>;

    #[test]
    fn policy_names_are_the_axis_values() {
        assert_eq!(<EagerExpansion as LayoutOption>::NAME, "eager");
        assert_eq!(<LazyExpansion as LayoutOption>::NAME, "lazy");
    }

    #[test]
    fn lazy_matches_the_marker() {
        const { assert!(!EagerExpansion::LAZY) };
        const { assert!(LazyExpansion::LAZY) };
    }

    /// The pending chain grows by `push`, then `expand` takes it into the
    /// build, once: the child is built and holds no chain.
    #[test]
    fn expand_takes_the_pending_chain_once() {
        let links = ChainLinks::new(2);
        let mut child = Lazy::from_chain(Chain::start(&links, 0));
        child.push(&links, 1);
        assert!(child.built().is_none());
        let chain = child.pending().expect("not yet expanded");
        assert_eq!(chain.walk(&links).collect::<Vec<_>>(), vec![0, 1]);
        let mut calls = 0;
        let built = child.expand(|chain| {
            calls += 1;
            chain.walk(&links).collect()
        });
        assert_eq!(built, &vec![0, 1]);
        // A second expand returns the same table without building again.
        let again = child.expand(|_| unreachable!("already built"));
        assert!(std::ptr::eq(built, again));
        assert_eq!(calls, 1);
        assert_eq!(child.pending(), None, "an expanded child keeps no chain");
    }

    #[test]
    fn built_mut_reaches_the_expanded_table() {
        let links = ChainLinks::new(1);
        let mut child = Lazy::from_chain(Chain::start(&links, 0));
        assert!(child.built_mut().is_none());
        child.expand(|chain| chain.walk(&links).collect());
        child.built_mut().unwrap().push(1);
        assert_eq!(child.built(), Some(&vec![0, 1]));
    }

    #[test]
    fn own_heap_bytes_is_the_box() {
        let links = ChainLinks::new(1);
        let child = Lazy::from_chain(Chain::start(&links, 0));
        assert_eq!(
            child.own_heap_bytes(),
            std::mem::size_of::<LazyChild<Vec<RowId>>>()
        );
    }

    /// The pending chain is stored in the box itself, 12 bytes with `len`'s
    /// niche for the `Option`, so a lazy child allocates only its box.
    #[test]
    fn the_pending_chain_lives_in_the_box() {
        assert_eq!(std::mem::size_of::<Cell<Option<Chain>>>(), 12);
    }
}
```

  Notes:
  - **The cells.** `Cell::get` needs `Option<Chain>: Copy`, which it is. `Cell::take` needs
    `Default`, which `Option` has. `push` reaches the chain through `Cell::get_mut` (it holds
    `&mut self`), and `expand` through `Cell::take` (it holds `&self`).
  - **Send and Sync.** `LazyChild<N>` stays `Send` (`Cell<Option<Chain>>` and `OnceCell<N>` are
    `Send` for `Send` contents) and `!Sync`.
  - **The renamed test.** `expand_moves_the_pending_rows_once` becomes
    `expand_takes_the_pending_chain_once`. Its old `capacity() == 0` check has no counterpart: a
    taken `Cell` holds `None`, which the new last assertion checks.

- [ ] **Step 4: `node.rs`: a leaf holds chains.** All edits are in
  `kermit-ds/src/ds/hash_trie/node.rs`.

  Module doc, lines 1–4. Before:

```rust
//! Recursive node type for the hash trie. Inner levels carry child node
//! tables; the deepest (leaf) level carries chains of row ids; a pruned
//! subtrie carries its single tuple's row id directly. Every row id indexes
//! the trie's buffer (`HashTrie::tuples`, #111).
```

  After:

```rust
//! Recursive node type for the hash trie. Inner levels carry child node
//! tables; the deepest (leaf) level carries chains, lists of rows threaded
//! through the trie's links (`chain.rs`, #101 B2); a pruned subtrie carries
//! its single tuple's row id directly. Every row id indexes the trie's
//! buffer (`HashTrie::tuples`, #111).
```

  `use`, lines 15–18. Before:

```rust
use {
    super::{expansion::ExpansionPolicy, hash_table::HashTable, pruning::PruningPolicy},
    kermit_iters::RowId,
};
```

  After (`RowId` was used only by `Leaf`):

```rust
use super::{
    chain::Chain, expansion::ExpansionPolicy, hash_table::HashTable, pruning::PruningPolicy,
};
```

  `Leaf`, lines 27–31. Before:

```rust
    /// Leaf level: hash table whose values are chains. Each chain holds the
    /// row ids, in input order, of the tuples whose attribute hashes match
    /// the path of hashes from the root to this bucket; the tuples
    /// themselves live in the trie's buffer.
    Leaf(HashTable<Vec<RowId>>),
```

  After:

```rust
    /// Leaf level: hash table whose values are chains. Each chain lists the
    /// rows, in input order, of the tuples whose attribute hashes match the
    /// path of hashes from the root to this bucket: its first and last rows
    /// and its length, with each row's successor in the trie's links. The
    /// tuples themselves live in the trie's buffer.
    Leaf(HashTable<Chain>),
```

  `Unexpanded`'s doc, lines 44–47. Before:

```rust
    /// Unexpanded child (lazy child expansion, SIGMOD 2020 Figure 6): the
    /// row ids of the tuples below this bucket, kept as a list until a
    /// probe first opens it, then the table built from them. Never the
    /// root. Holds `E::Pending<Self>`: `Box<LazyChild<Self>>` when lazy, the
```

  After:

```rust
    /// Unexpanded child (lazy child expansion, SIGMOD 2020 Figure 6): the
    /// chain of the tuples below this bucket, kept until a probe first
    /// opens it, then the table built from its rows. Never the root. Holds
    /// `E::Pending<Self>`: `Box<LazyChild<Self>>` when lazy, the
```

  Lines 48–51 (`uninhabited \`Never\` when eager, …`) stay as they are.

- [ ] **Step 5: `bulk.rs`: Algorithm 2 over chains.** In `kermit-ds/src/ds/hash_trie/bulk.rs`,
  replace lines 1–161 (everything before `#[cfg(test)]`) with:

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
//! A list `L` is a `Chain` (`chain.rs`): its first and last rows and its
//! length, in its bucket, with each row's successor in the trie's links,
//! one per row (§3.3.2, Figure 3). Line 5 pops a row by reading its link,
//! and line 7 pushes it by rewriting two, so each row moves from its
//! parent's list to exactly one child list (§3.2.2), and a build allocates
//! per table, never per list (#101 B2).
//!
//! Two things that are kermit's:
//!
//! - The links are 4-byte row ids in an array beside the trie's buffer
//!   (`ChainLinks`), where Umbra reserves an 8-byte chain pointer in each
//!   materialised tuple (§3.3.2); and a push appends at a list's tail, where
//!   line 7, read literally, prepends.
//! - Line 3's size is a Config value whose default (`grow`) is not the paper's;
//!   `child-capacity=tuples` at `load-factor=0.8` is the paper's sizing
//!   exactly. The root's is `root-capacity` (#88), and each child's is
//!   `child-capacity`: 4 buckets that grow (`grow`), or sized once from its
//!   list (`tuples`).
//!
//! Under `child-capacity=grow`, Algorithm 2 builds the trie the per-tuple
//! build (`insert_at`, the `incremental` mode, which requires `grow`)
//! builds, array for array, links included. A table's layout depends only
//! on the order its new keys arrive. Here every list keeps input order,
//! because a push appends, so each table receives its keys in the order
//! `insert_at` sends them. Every push also writes `NIL` into the pushed
//! row's link, so whichever build ran, a row's final link is its successor
//! in the list that ends up holding it, or `NIL`.

use {
    super::{
        chain::{push_row, Chain},
        config::HashTrieConfig,
        expansion::{ExpansionPolicy, PendingChild},
        hash_table::HashTable,
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::{PruningPolicy, SingletonPayload},
    },
    crate::morsel::row_id,
    kermit_iters::{ChainLinks, HashStrategy, RowId, Tuples},
};

/// Every row id of `tuples`, in input order: Algorithm 2's first list,
/// which is a range, never a chain, so it threads nothing. `row_id` (in
/// `morsel.rs`) converts the length; a batch never holds more than
/// `RowId::MAX` rows.
pub(super) fn all_rows(tuples: &Tuples) -> std::ops::Range<RowId> { 0..row_id(tuples.len()) }

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> HashTrie<H, P, E> {
    /// Algorithm 2: the table at `depth` over the rows of `tuples` that
    /// `list` names, allocated at `2^log2_capacity` buckets, its lists
    /// threaded through `links`.
    pub(super) fn build(
        tuples: &Tuples, links: &ChainLinks, depth: usize, arity: usize,
        list: impl IntoIterator<Item = RowId>, log2_capacity: u32, config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        let lists = Self::group(tuples, links, depth, list, log2_capacity, config);
        Self::build_nested(tuples, links, depth, arity, lists, config)
    }

    /// Lines 3–7: a table of `2^log2_capacity` buckets holding `list`'s
    /// rows, each pushed onto the chain in the bucket of its row's attribute
    /// hash at `depth` (`push_row`). The table grows under the load factor
    /// if its keys outgrow it, as `insert_at`'s tables do.
    ///
    /// `list` may be a chain's walk over these same `links` (line 10's list,
    /// from [`build_child_table`](Self::build_child_table)). The walk reads
    /// each row's link before it yields the row, so pushing the row onto its
    /// new chain, which rewrites that link, cannot derail it.
    pub(super) fn group(
        tuples: &Tuples, links: &ChainLinks, depth: usize, list: impl IntoIterator<Item = RowId>,
        log2_capacity: u32, config: HashTrieConfig,
    ) -> HashTable<Chain> {
        let mut lists = HashTable::with_log2_capacity(log2_capacity);
        for row in list {
            let hash = H::hash(tuples.row(row)[depth]);
            push_row(&mut lists, hash, config.load_factor, links, row);
        }
        lists
    }

    /// Lines 8–15: each bucket's chain replaced by the node built from it,
    /// in bucket order (line 9). At the last attribute each chain is stored
    /// itself (line 15), so the table of chains is the leaf.
    pub(super) fn build_nested(
        tuples: &Tuples, links: &ChainLinks, depth: usize, arity: usize, lists: HashTable<Chain>,
        config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        // `>=`, as `make_root_sized`'s `arity <= 1`, so the unsupported nullary
        // root is a leaf under both builds.
        if depth + 1 >= arity {
            return HashTrieNode::Leaf(lists);
        }
        HashTrieNode::Inner(
            lists.map(|chain| Self::child(tuples, links, depth + 1, arity, chain, config)),
        )
    }

    /// The node a bucket's `chain` becomes, at `depth`: a `Singleton` of its
    /// one row when pruning is on and one tuple lives below (§3.3.1), an
    /// `Unexpanded` child holding the chain under lazy expansion (§3.3.1;
    /// `HashTrie::resolve` builds its table by
    /// [`build_child_table`](Self::build_child_table) on the first probe),
    /// and otherwise the table line 11 builds.
    ///
    /// A one-row chain was started, not pushed onto, so a `Singleton`'s row
    /// holds `NIL`, as `insert_at` leaves it.
    pub(super) fn child(
        tuples: &Tuples, links: &ChainLinks, depth: usize, arity: usize, chain: Chain,
        config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        if P::ENABLED && chain.len() == 1 {
            return HashTrieNode::Singleton(P::Payload::from_row(chain.head()));
        }
        if E::LAZY {
            return HashTrieNode::Unexpanded(E::Pending::from_chain(chain));
        }
        Self::build_child_table(tuples, links, depth, arity, chain, config)
    }

    /// The table a bucket's `chain` becomes at `depth` when it is built
    /// (line 11): eagerly by [`child`](Self::child), or on the first probe
    /// by `HashTrie::resolve` under lazy expansion, which re-threads the
    /// chain's rows through the `&self` a probe holds. One definition, so a
    /// child expanded from a chain is the eager child built from that chain,
    /// size and links included.
    pub(super) fn build_child_table(
        tuples: &Tuples, links: &ChainLinks, depth: usize, arity: usize, chain: Chain,
        config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        let log2_capacity = config.child_log2_capacity(chain.len());
        Self::build(tuples, links, depth, arity, chain.walk(links), log2_capacity, config)
    }
}
```

  Notes:
  - **`TupleList` is deleted.** Its users in `radix.rs` and `parallel.rs` are C05b's.
  - **The `shrink_to` special case** in `child` (old lines 126–145) is deleted. It kept the lazy
    pending `Vec`s' capacities equal to `incremental`'s, and a chain has no capacity.
  - **Why `ChainLinks::new`'s zeroes are never read** (the starting values `bulk` and the
    partitioned builds rely on). The root's `group` walks `all_rows`, a range, and its first push
    of each row writes the row's link (`start`) or writes it, after the previous tail's
    (`push`). Every later `group` walks a chain whose links the level above wrote.

- [ ] **Step 6: `bulk.rs` tests.** All edits are in `mod tests` of
  `kermit-ds/src/ds/hash_trie/bulk.rs`.

  **a. The test `use`, line 172.** Before:

```rust
                identity::{assert_same_trie, incremental_configs, inputs},
```

  After:

```rust
                identity::{assert_same_trie, incremental_configs, inputs, rows},
```

  **b. `check_identity`'s doc, lines 196–199.** Before:

```rust
    /// Algorithm 2 builds the trie the per-tuple build makes, array for
    /// array: capacities, chains, singletons and pending lists included.
    /// The `interleaved` input gives an arity-2 trie a one-tuple and a
    /// two-tuple child, the two lengths whose pending lists `child` shrinks.
```

  After:

```rust
    /// Algorithm 2 builds the trie the per-tuple build makes, array for
    /// array: capacities, chains, links, singletons and pending chains
    /// included (`assert_same_trie`).
```

  **c. The end of `bulk_keeps_chain_order_under_colliding_hashes`, lines 291–295.** Before:

```rust
        let Some((_, chain)) = leaf.iter().next() else {
            panic!("one chain")
        };
        assert_eq!(chain, &vec![0, 1, 2, 3, 4]);
    }
```

  After:

```rust
        let Some((_, chain)) = leaf.iter().next() else {
            panic!("one chain")
        };
        assert_eq!(
            chain.walk(bulk.links()).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
        assert_eq!(
            bulk.links().to_vec(),
            vec![1, 2, 3, 4, ChainLinks::NIL],
            "each row links to the next, the tail to `NIL`"
        );
    }
```

  **d. A new test**, right after that function (after line 295). It is mutation 2's pin, run in
  C05b:

```rust
    /// The rows of the `Singleton`s a build left as nodes below `node`. A
    /// pending chain is a list, not a node, so a lazy trie's are its root's.
    fn singleton_rows<P: PruningPolicy, E: ExpansionPolicy>(
        node: &HashTrieNode<P, E>, out: &mut Vec<RowId>,
    ) {
        match node {
            | HashTrieNode::Inner(table) => {
                for (_, child) in table.iter() {
                    singleton_rows(child, out);
                }
            },
            | HashTrieNode::Singleton(payload) => out.push(*payload.row()),
            | HashTrieNode::Leaf(_) | HashTrieNode::Unexpanded(_) => {},
        }
    }

    /// The links are a function of the lists (`Chain::push` writes `NIL`
    /// into every pushed row's link), so Algorithm 2, which re-threads each
    /// row once per level, and the per-tuple build, which threads it once,
    /// end with equal arrays, compared whole. The fixture (arity 3,
    /// colliding hashes) has a leaf chain of colliding distinct tuples and a
    /// duplicate (rows 0, 1, 4), a two-row chain (2, 5) and one-row chains
    /// (6, 7, 3). Under pruning, rows 3 (depth 1) and 7 (depth 2) are
    /// `Singleton`s, whose rows end no chain and so hold `NIL`. Under lazy
    /// expansion the root's children are pending chains, threaded once, and
    /// row 3 is still a `Singleton` under pruning.
    #[test]
    fn bulk_and_incremental_thread_the_same_links() {
        let tuples = rows(3, &[
            [1, 2, 3],
            [11, 2, 13],
            [1, 4, 5],
            [7, 8, 9],
            [1, 2, 3],
            [21, 4, 15],
            [1, 4, 6],
            [1, 9, 9],
        ]);
        fn check<P: PruningPolicy, E: ExpansionPolicy>(tuples: &Tuples) {
            const NIL: RowId = ChainLinks::NIL;
            // Eager: each row's successor in its leaf chain. Lazy: in its
            // root key's pending chain, `[0, 1, 2, 4, 5, 6, 7]` or `[3]`.
            let expected = if E::LAZY {
                vec![1, 2, 4, NIL, 5, 6, 7, NIL]
            } else {
                vec![1, 4, 5, NIL, NIL, NIL, NIL, NIL]
            };
            let pruned: &[RowId] = match (P::ENABLED, E::LAZY) {
                | (false, _) => &[],
                | (true, false) => &[3, 7],
                | (true, true) => &[3],
            };
            for config in incremental_configs() {
                let label = label::<Mod10HashStrategy, P, E>(3, config, "links fixture");
                let incremental = HashTrie::<Mod10HashStrategy, P, E>::from_tuples_incrementally(
                    3.into(),
                    config,
                    tuples.clone(),
                );
                let bulk = HashTrie::<Mod10HashStrategy, P, E>::from_tuples_in_bulk(
                    3.into(),
                    config,
                    tuples.clone(),
                );
                assert_eq!(
                    bulk.links().to_vec(),
                    incremental.links().to_vec(),
                    "{label}: links"
                );
                assert_eq!(bulk.links().to_vec(), expected, "{label}: the threading");
                let mut singletons = Vec::new();
                singleton_rows(bulk.root(), &mut singletons);
                singletons.sort_unstable();
                assert_eq!(singletons, pruned, "{label}: the premise: the singletons");
                for row in singletons {
                    assert_eq!(
                        bulk.links().get(row),
                        NIL,
                        "{label}: bulk's singleton row {row}"
                    );
                    assert_eq!(
                        incremental.links().get(row),
                        NIL,
                        "{label}: incremental's singleton row {row}"
                    );
                }
                assert_same_trie(&incremental, &bulk, &label);
            }
        }
        check::<NoPruning, EagerExpansion>(&tuples);
        check::<SingletonPruning, EagerExpansion>(&tuples);
        check::<NoPruning, LazyExpansion>(&tuples);
        check::<SingletonPruning, LazyExpansion>(&tuples);
    }
```

  The fixture under `Mod10HashStrategy` (`hash(k) = k mod 10`), so the expectations can be
  checked by hand:
  - **Depth 0.** Key 1 holds rows 0, 1, 2, 4, 5, 6, 7 (1 ≡ 11 ≡ 21), and key 7 holds row 3.
  - **Depth 1, under key 1.** Key 2 holds 0, 1, 4; key 4 holds 2, 5, 6; key 9 holds 7.
  - **Depth 2.** Under (1, 2), key 3 holds 0, 1, 4 (3 ≡ 13). Under (1, 4), key 5 holds 2, 5
    (5 ≡ 15) and key 6 holds 6. Under (1, 9), key 9 holds 7. Under key 7, row 3 is alone at every
    level.
  - **The leaf chains** are `[0, 1, 4]`, `[2, 5]`, `[6]`, `[7]` and `[3]`, so the eager links are
    0→1, 1→4, 2→5 and `NIL` for the rest. Under eager pruning, rows 3 and 7 are `Singleton`s,
    also `NIL`.
  - **Under lazy expansion**, the root's lists are the pending chains: 0→1→2→4→5→6→7, and row 3
    alone.
  - `singletons` is a `Vec<RowId>` compared with a `&[RowId]` (std's
    `impl PartialEq<&[U]> for Vec<T>`).
  - `payload.row()` needs `SingletonPayload`, which the non-test `use` imports and `super::*`
    brings in. `ChainLinks` and `RowId` come the same way.

  **e. `tuples_below`, lines 350–363.** Before:

```rust
    /// The tuples stored below `node`, built or pending.
    fn tuples_below<P: PruningPolicy, E: ExpansionPolicy>(node: &HashTrieNode<P, E>) -> usize {
        match node {
            | HashTrieNode::Inner(table) => {
                table.iter().map(|(_, child)| tuples_below(child)).sum()
            },
            | HashTrieNode::Leaf(table) => table.iter().map(|(_, chain)| chain.len()).sum(),
            | HashTrieNode::Singleton(_) => 1,
            | HashTrieNode::Unexpanded(pending) => match pending.built() {
                | Some(built) => tuples_below(built),
                | None => pending.pending().len(),
            },
        }
    }
```

  After:

```rust
    /// The tuples stored below `node`, built or pending.
    fn tuples_below<P: PruningPolicy, E: ExpansionPolicy>(node: &HashTrieNode<P, E>) -> usize {
        match node {
            | HashTrieNode::Inner(table) => {
                table.iter().map(|(_, child)| tuples_below(child)).sum()
            },
            | HashTrieNode::Leaf(table) => table.iter().map(|(_, chain)| chain.len()).sum(),
            | HashTrieNode::Singleton(_) => 1,
            | HashTrieNode::Unexpanded(pending) => match pending.built() {
                | Some(built) => tuples_below(built),
                | None => pending
                    .pending()
                    .expect("an unexpanded child holds its pending chain")
                    .len(),
            },
        }
    }
```

- [ ] **Step 7: `implementation.rs`: the links, the per-tuple build, `resolve`, the walk and
  `space`.** All edits are in the non-test code of
  `kermit-ds/src/ds/hash_trie/implementation.rs`.

  **a. `use`, lines 21–43.** Before:

```rust
use {
    super::{
        build_mode::HashTrieBuildMode,
        bulk::{all_rows, TupleList},
        config::{ChildCapacity, HashTrieConfig, LoadFactor, RootCapacity},
        expansion::{EagerExpansion, ExpansionPolicy, PendingChild},
        node::HashTrieNode,
        parallel,
        pruning::{NoPruning, PruningPolicy, SingletonPayload},
        radix,
    },
    crate::{
        morsel::row_id,
        relation::{
            BuildModeRelation, ConfigurableRelation, ConfiguredBuildModeRelation, Relation,
            RelationHeader,
        },
    },
    kermit_iters::{
        ConfigOption, HashStrategy, JoinIterable, LayoutOption, RowId, SipHashStrategy, Tuples,
    },
    std::marker::PhantomData,
};
```

  After:

```rust
use {
    super::{
        build_mode::HashTrieBuildMode,
        bulk::all_rows,
        chain::{push_row, Chain},
        config::{ChildCapacity, HashTrieConfig, LoadFactor, RootCapacity},
        expansion::{EagerExpansion, ExpansionPolicy, PendingChild},
        node::HashTrieNode,
        parallel,
        pruning::{NoPruning, PruningPolicy, SingletonPayload},
        radix,
    },
    crate::{
        morsel::row_id,
        relation::{
            BuildModeRelation, ConfigurableRelation, ConfiguredBuildModeRelation, Relation,
            RelationHeader,
        },
    },
    kermit_iters::{
        ChainLinks, ConfigOption, HashStrategy, JoinIterable, LayoutOption, RowId,
        SipHashStrategy, Tuples,
    },
    std::marker::PhantomData,
};
```

  **b. The struct doc's last invariant, lines 66–68.** Before:

```rust
/// - Every [`RowId`] in the trie (chain entry, `Singleton`, pending list)
///   indexes a row of the buffer, and every row of the buffer is named by
///   exactly one of them.
```

  After:

```rust
/// - Every row of the buffer is in exactly one of the trie's lists: a leaf
///   chain, an unexpanded child's pending chain, or a pruned `Singleton`,
///   which holds its [`RowId`].
/// - Once a build has finished, the links hold one entry per row of the
///   buffer: the row's successor in the chain that holds it, or
///   [`ChainLinks::NIL`]. A `Singleton`'s row, and every chain's tail, hold
///   `NIL`. So the links are a function of the trie's lists, and `bulk` and
///   `incremental` build equal arrays.
```

  **c. Storage, lines 78–84.** Before:

```rust
/// order. Below the tables everything names a tuple by its [`RowId`]: a
/// leaf chain is a `Vec<RowId>`, a pruned `Singleton` one `RowId`, an
/// unexpanded child's pending list a `Vec<RowId>`. The paper's leaves
/// likewise point into a materialised buffer, partitioned as it is
/// materialised (VLDB 2020 §3.3.2); its pointers are 8 bytes where these
/// ids are 4, and it threads each list through the tuples where these are a
/// `Vec` per list (both kermit's; the second until #101 B2).
```

  After:

```rust
/// order. Below the tables everything names a tuple by its [`RowId`]: a
/// leaf chain, like an unexpanded child's pending chain, is its first and
/// last rows and its length, with each row's successor in the links, one
/// [`ChainLinks`] entry per row beside the buffer; a pruned `Singleton` is
/// one `RowId`. The paper's leaves likewise point into a materialised
/// buffer, partitioned as it is materialised, and thread each list through
/// a chain pointer reserved in each tuple (VLDB 2020 §3.3.2, Figure 3). Its
/// pointers are 8 bytes inside each tuple where these links are 4-byte ids
/// in an array beside the buffer, and it pushes onto a list where these
/// lists are appended to, in input order (both kermit's).
```

  **d. The fields, lines 133–137.** Before:

```rust
    /// Every stored tuple: the build's batch, or its partitioned copy (see
    /// Storage), then every inserted row. Its length is the multiset count
    /// (duplicates count). Chains, singletons and pending lists hold ids of
    /// its rows.
    tuples: Tuples,
```

  After:

```rust
    /// Every stored tuple: the build's batch, or its partitioned copy (see
    /// Storage), then every inserted row. Its length is the multiset count
    /// (duplicates count). Its rows are threaded into the trie's chains
    /// through `links`, and a `Singleton` holds one row's id.
    tuples: Tuples,
    /// One link per row of `tuples`: the row's successor in the chain that
    /// holds it, or `ChainLinks::NIL` (see Invariants). Atomic, so that lazy
    /// expansion can re-thread a child's rows through the `&self` a probe
    /// holds (`resolve`).
    links: ChainLinks,
```

  **e. `with_config_for`, lines 159–178.** Before:

```rust
    /// A trie holding `config` and owning `tuples`, with an empty root sized
    /// for a build from them by [`HashTrieConfig::root_log2_capacity`]: 4
    /// buckets under `RootCapacity::Grow`, and under `Tuples` a capacity at
    /// which `tuples.len()` keys never make it grow. The per-tuple build
    /// passes its batch and then inserts every row (until it has, the buffer
    /// holds rows the root does not); [`ConfigurableRelation::with_config`]
    /// passes an empty batch. The bulk and partitioned builds size their
    /// roots by the same `root_log2_capacity`.
    pub(super) fn with_config_for(
        header: RelationHeader, config: HashTrieConfig, tuples: Tuples,
    ) -> Self {
        let root = Self::make_root_sized(header.arity(), config.root_log2_capacity(tuples.len()));
        Self {
            header,
            tuples,
            root,
            config,
            _layout: PhantomData,
        }
    }
```

  After:

```rust
    /// A trie holding `config` and owning `tuples`, with an empty root sized
    /// for a build from them by [`HashTrieConfig::root_log2_capacity`], and
    /// no links yet, with room for one per row of `tuples`. The root is 4
    /// buckets under `RootCapacity::Grow`, and under `Tuples` a capacity at
    /// which `tuples.len()` keys never make it grow. The per-tuple build
    /// passes its batch, then pushes each row's link and inserts the row
    /// (until it has, the buffer holds rows the root and the links do not);
    /// [`ConfigurableRelation::with_config`] passes an empty batch. The bulk
    /// and partitioned builds size their roots by the same
    /// `root_log2_capacity`.
    pub(super) fn with_config_for(
        header: RelationHeader, config: HashTrieConfig, tuples: Tuples,
    ) -> Self {
        let root = Self::make_root_sized(header.arity(), config.root_log2_capacity(tuples.len()));
        let links = ChainLinks::with_capacity(tuples.len());
        Self {
            header,
            tuples,
            links,
            root,
            config,
            _layout: PhantomData,
        }
    }
```

  **f. `tuples()`, lines 184–187, and the new `links()` after it.** Before:

```rust
    /// The trie's buffer: every stored tuple, in the order Storage
    /// describes. Chains, singletons and pending lists hold ids of its rows;
    /// `HashTrieIter` lends a chain's rows from it (`LeafRows`).
    pub(crate) fn tuples(&self) -> &Tuples { &self.tuples }
```

  After:

```rust
    /// The trie's buffer: every stored tuple, in the order Storage
    /// describes. Its rows are threaded into the trie's chains through
    /// [`links`](Self::links), and a `Singleton` holds one row's id;
    /// `HashTrieIter` lends a chain's rows from it (`LeafRows`).
    pub(crate) fn tuples(&self) -> &Tuples { &self.tuples }

    /// One link per row of the buffer: the row's successor in the chain
    /// that holds it, or `ChainLinks::NIL` (see Invariants). `HashTrieIter`
    /// lends a leaf chain by walking it (`LeafRows::linked`), and `resolve`
    /// re-threads a pending chain through it.
    pub(crate) fn links(&self) -> &ChainLinks { &self.links }
```

  **g. `for_each_tuple` and `visit_at`, lines 197–239.** Replace with:

```rust
    /// Walk the trie depth-first, lending every stored tuple to `visit`:
    /// each leaf chain's rows in chain order, walked through the trie's
    /// links, and each pruned `Singleton`'s row, read from the trie's buffer
    /// through its row id.
    ///
    /// The walk allocates nothing per tuple: O(n) time, O(arity) stack.
    /// The CLI's `bench ds` `iteration` and `end_to_end` metrics time this
    /// walk (issue #79), so it traverses the structure; scanning the buffer
    /// instead would time an array scan. `TupleScan::scan_tuples` is that
    /// scan, for callers that need only the multiset (#111).
    ///
    /// Under `LazyExpansion` an unexpanded child lends its pending chain's
    /// rows in insertion order, and the walk expands nothing. A `visit` that
    /// expands a child whose chain this walk is still reading, by opening an
    /// iterator on this same trie and reaching that child, re-threads the
    /// chain's rows under the walk, which may then lend wrong rows or panic
    /// (before #101 B2 it panicked with `BorrowMutError`). No caller in the
    /// workspace does that.
    pub fn for_each_tuple<V: FnMut(&[usize])>(&self, mut visit: V) {
        Self::visit_at(&self.tuples, &self.links, &self.root, &mut visit);
    }

    fn visit_at<V: FnMut(&[usize])>(
        tuples: &Tuples, links: &ChainLinks, node: &HashTrieNode<P, E>, visit: &mut V,
    ) {
        match node {
            | HashTrieNode::Inner(table) => {
                for (_, child) in table.iter() {
                    Self::visit_at(tuples, links, child, visit);
                }
            },
            | HashTrieNode::Leaf(table) => {
                for (_, chain) in table.iter() {
                    Self::visit_chain(tuples, links, chain, visit);
                }
            },
            | HashTrieNode::Singleton(payload) => visit(tuples.row(*payload.row())),
            | HashTrieNode::Unexpanded(pending) => match (pending.built(), pending.pending()) {
                | (Some(built), _) => Self::visit_at(tuples, links, built, visit),
                | (None, Some(chain)) => Self::visit_chain(tuples, links, &chain, visit),
                | (None, None) => {
                    unreachable!("an unexpanded child holds its pending chain until it is built")
                },
            },
        }
    }

    /// Lends the rows of `chain` to `visit`, in chain order.
    fn visit_chain<V: FnMut(&[usize])>(
        tuples: &Tuples, links: &ChainLinks, chain: &Chain, visit: &mut V,
    ) {
        for row in chain.walk(links) {
            visit(tuples.row(row));
        }
    }
```

  **The `(None, None)` arm.** Only an expansion that is still running leaves a child neither
  pending nor built. Nothing reaches `visit_at` from inside an expansion: `resolve`'s build calls
  no visitor. A visitor that expands a child returns only after the expansion has finished.

  **h. `insert_at`, lines 241–358.** Replace with:

```rust
    /// Insert the row `row` of `tuples` at the appropriate depth, descending
    /// at once, and thread it through `links`. It is used by the per-tuple
    /// build (`incremental`) and by `Relation::insert`, which both push
    /// `row`'s link as `NIL` first, so a row that stays a pruned `Singleton`
    /// holds `NIL`, as under `bulk`. The paper's build, Algorithm 2, groups
    /// before it recurses (`bulk.rs`); under every config both builds
    /// accept, they build the identical trie, links included. The
    /// singleton-pruning extension of §3.3.1 (Figure 5) applies when the
    /// policy `P` enables it. `P::ENABLED` is a constant, so under
    /// `NoPruning` both pruning branches are compiled out and this is the
    /// pre-pruning insert. An unprune is a single extra O(arity) chain, not
    /// a fan-out: the evicted tuple stops as a new `Singleton` where the two
    /// diverge while only the new tuple keeps descending.
    pub(super) fn insert_at(
        tuples: &Tuples, links: &ChainLinks, node: &mut HashTrieNode<P, E>, depth: usize,
        arity: usize, row: RowId, load_factor: LoadFactor,
    ) {
        let key = tuples.row(row)[depth];
        let hash = H::hash(key);
        match node {
            | HashTrieNode::Inner(table) => {
                if P::ENABLED && table.get(hash).is_none() {
                    // Fresh bucket: the subtrie below holds exactly one tuple,
                    // so store its row id instead of one table per remaining
                    // level. The closure always runs — absence was just
                    // proven — so this is two O(1) probes, kept over a
                    // special-cased insert for readability.
                    table.entry_or_insert_with(hash, load_factor, || {
                        HashTrieNode::Singleton(P::Payload::from_row(row))
                    });
                    return;
                }
                if E::LAZY && table.get(hash).is_none() {
                    // Fresh bucket, pruning off: defer the child's table
                    // (Figure 6). The row waits as a one-row pending chain
                    // until a probe opens the child; `resolve` then builds
                    // the table. With pruning on, the block above stored a
                    // `Singleton` first.
                    table.entry_or_insert_with(hash, load_factor, || {
                        HashTrieNode::Unexpanded(E::Pending::from_chain(Chain::start(links, row)))
                    });
                    return;
                }
                // The child lives at `depth + 1`; it is the leaf when that is
                // the last attribute.
                let child_is_leaf = Self::is_leaf_depth(depth + 1, arity);
                let child = table.entry_or_insert_with(hash, load_factor, || {
                    HashTrieNode::new_table(child_is_leaf)
                });
                if E::LAZY {
                    // The bucket existed (absence returned above), so the
                    // closure did not run: under lazy expansion an `Inner`
                    // bucket holds a `Singleton` or an `Unexpanded` child,
                    // never a table.
                    match child {
                        | HashTrieNode::Unexpanded(pending) => match pending.built_mut() {
                            | Some(built) => Self::insert_at(
                                tuples,
                                links,
                                built,
                                depth + 1,
                                arity,
                                row,
                                load_factor,
                            ),
                            | None => pending.push(links, row),
                        },
                        | HashTrieNode::Singleton(payload) => {
                            // A second tuple below a pruned bucket: the child
                            // becomes the pending chain of both. The evicted
                            // row goes first, as an eager unprune re-inserts it
                            // first, so expansion later builds the eager table
                            // (under `child-capacity=grow`; see `resolve`).
                            let mut chain = Chain::start(links, *payload.row());
                            chain.push(links, row);
                            *child = HashTrieNode::Unexpanded(E::Pending::from_chain(chain));
                        },
                        | HashTrieNode::Inner(_) | HashTrieNode::Leaf(_) => unreachable!(
                            "a lazy Inner bucket holds a Singleton or an Unexpanded child"
                        ),
                    }
                    return;
                }
                if P::ENABLED && matches!(child, HashTrieNode::Singleton(_)) {
                    // Unprune: a second tuple has arrived, so the subtrie no
                    // longer holds exactly one. Swap in the table this level
                    // would have had and re-insert the evicted row ahead of
                    // the new one; the recursion re-prunes wherever the two
                    // diverge.
                    let replacement = HashTrieNode::new_table(child_is_leaf);
                    let HashTrieNode::Singleton(evicted) = std::mem::replace(child, replacement)
                    else {
                        unreachable!("matched Singleton above")
                    };
                    Self::insert_at(
                        tuples,
                        links,
                        child,
                        depth + 1,
                        arity,
                        evicted.into_row(),
                        load_factor,
                    );
                }
                Self::insert_at(tuples, links, child, depth + 1, arity, row, load_factor);
            },
            | HashTrieNode::Leaf(table) => {
                push_row(table, hash, load_factor, links, row);
            },
            | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
                unreachable!(
                    "insert_at descends through Inner/Leaf only; singletons are unpruned and \
                     unexpanded children appended to by the parent"
                )
            },
        }
    }
```

  Borrow notes:
  - **The lazy unprune arm.** `match child` on the `&mut HashTrieNode` binds `payload` as a
    reborrow of `*child`. `*payload.row()` copies the id out, and that is `payload`'s last use, so
    NLL accepts the assignment `*child = …` after it. The `Singleton` that the assignment drops is
    a `RowId` (or the uninhabited `Never`): no drop glue.
  - **`pending.built_mut()`.** The `Some` arm uses the borrow and the `None` arm calls
    `pending.push`: the same shape as today's code, and accepted for the same reason.
  - **The links.** `links` is a shared borrow throughout: `Chain::start`, `Chain::push` and
    `PendingChild::push` all take `&ChainLinks`.

  **i. `resolve`, lines 360–394.** Replace with:

```rust
    /// `node` as a probe sees it. An `Unexpanded` child is first built
    /// (once; later calls return the same table), and any other node is
    /// returned as is. `depth` is `node`'s depth.
    ///
    /// The table is built by Algorithm 2 (`bulk.rs`) from the pending chain,
    /// which keeps insertion order, so it is the eager table at this
    /// position, bucket for bucket, for a trie no `insert` has changed since
    /// its build (or under `child-capacity=grow`, always): linear probing
    /// places keys by insertion order under the same load factor, from the
    /// same starting capacity. The build re-threads the chain's rows into
    /// the child's lists through `&self`, which the links' relaxed atomics
    /// allow (`ChainLinks`), so the links end as the eager trie's. Its own
    /// children come out `Unexpanded` (or `Singleton`), so each expansion
    /// builds exactly one level (§3.3.1; SIGMOD 2020 Figure 6). The
    /// trace-equivalence tests in `kermit-ds/tests/hash_trie_tests.rs` and
    /// `resolve_rethreads_the_pending_chain_through_a_shared_reference` pin
    /// this.
    ///
    /// After an `insert` under `child-capacity=tuples`, an unexpanded child and
    /// its eager twin can differ in capacity. The eager child started at the
    /// size of its build-time list, or at 4 buckets if `insert` created or
    /// unpruned it, and grew from there. This one is sized from its whole
    /// pending chain. Joins and benches never insert after building, so no
    /// measurement sees the difference.
    ///
    /// `HashTrieIter::open` is the only caller, so only a probe expands
    /// anything. `for_each_tuple` walks the pending chain, `heap_size_bytes`
    /// reads the tables and the links' capacity, and `collect_tuples` and
    /// `scan_tuples` the buffer, so no read-only call expands a child.
    pub(crate) fn resolve<'t>(
        &'t self, node: &'t HashTrieNode<P, E>, depth: usize,
    ) -> &'t HashTrieNode<P, E> {
        match node {
            | HashTrieNode::Unexpanded(pending) => pending.expand(|chain| {
                Self::build_child_table(
                    &self.tuples,
                    &self.links,
                    depth,
                    self.header.arity(),
                    chain,
                    self.config,
                )
            }),
            | other => other,
        }
    }
```

  **j. `Relation::insert`, lines 408–436.** Replace with:

```rust
    /// Appends `tuple` to the buffer and a `NIL` link for it, then places
    /// the new row by `insert_at`, which threads it into its chain.
    ///
    /// # Panics
    ///
    /// Panics if `tuple`'s length is not the relation's arity, or if the
    /// trie already holds `RowId::MAX` rows, the most a row id can name
    /// ([`Tuples::push`]).
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
        self.tuples.push(tuple);
        self.links.push(ChainLinks::NIL);
        let row = row_id(self.tuples.len() - 1);
        Self::insert_at(
            &self.tuples,
            &self.links,
            &mut self.root,
            0,
            arity,
            row,
            self.config.load_factor,
        );
    }
```
  `&self.tuples`, `&self.links` and `&mut self.root` are disjoint fields of `self`, so the three
  borrows are accepted together. `self.config.load_factor` is a `Copy` read of a fourth field.

  **k. The three partitioned arms of `from_tuples_with_config_and_build_mode`, lines 516–547.**
  Before:

```rust
            | HashTrieBuildMode::Radix(bits) => {
                Self::from_tuples_partitioned(header, config, tuples, |tuples, log2, arity| {
                    let mut root = Self::make_root_sized(arity, log2);
                    let copy = radix::fill_root::<H, P, E>(tuples, &mut root, arity, bits, config);
                    (copy, root)
                })
            },
            | HashTrieBuildMode::Parallel(threads) => {
                Self::from_tuples_partitioned(header, config, tuples, |tuples, log2, arity| {
                    let mut root = Self::make_root_sized(arity, log2);
                    let copy =
                        parallel::fill_root::<H, P, E>(tuples, &mut root, arity, threads, config);
                    (copy, root)
                })
            },
```

  and, inside the `Presized` arm, after its `assert_eq!`:

```rust
                Self::from_tuples_partitioned(header, config, tuples, |tuples, log2, arity| {
                    parallel::fill_presized_root::<H, P, E>(tuples, log2, arity, threads, config)
                })
```

  After:

```rust
            | HashTrieBuildMode::Radix(bits) => Self::from_tuples_partitioned(
                header,
                config,
                tuples,
                |tuples, links, log2, arity| {
                    let mut root = Self::make_root_sized(arity, log2);
                    let copy =
                        radix::fill_root::<H, P, E>(tuples, links, &mut root, arity, bits, config);
                    (copy, root)
                },
            ),
            | HashTrieBuildMode::Parallel(threads) => Self::from_tuples_partitioned(
                header,
                config,
                tuples,
                |tuples, links, log2, arity| {
                    let mut root = Self::make_root_sized(arity, log2);
                    let copy = parallel::fill_root::<H, P, E>(
                        tuples, links, &mut root, arity, threads, config,
                    );
                    (copy, root)
                },
            ),
```

  and:

```rust
                Self::from_tuples_partitioned(
                    header,
                    config,
                    tuples,
                    |tuples, links, log2, arity| {
                        parallel::fill_presized_root::<H, P, E>(
                            tuples, links, log2, arity, threads, config,
                        )
                    },
                )
```

  `cargo fmt` (C05b) settles the layout. The `links` argument comes right after `tuples` in all
  three calls, as the contract gives C05b's signatures.

  **l. `from_tuples_in_bulk`, `from_tuples_incrementally` and `from_tuples_partitioned`, lines
  574–651.** Replace the three functions with:

```rust
    /// The `bulk` build: Algorithm 2 from the root (`bulk.rs`) over every
    /// row id of `tuples`, in input order, the root sized as every build
    /// sizes it, by [`HashTrieConfig::root_log2_capacity`] (#88). The trie
    /// keeps `tuples` as its buffer and threads its rows through one link
    /// each. The links start zeroed (`ChainLinks::new`): the root's grouping
    /// writes every row's link before any is read.
    ///
    /// # Panics
    ///
    /// Panics if `tuples` holds rows of another arity than `header.arity()`.
    pub(super) fn from_tuples_in_bulk(
        header: RelationHeader, config: HashTrieConfig, tuples: Tuples,
    ) -> Self {
        #[cfg(test)]
        BULK_BUILDS.with(|n| n.set(n.get() + 1));
        let arity = header.arity();
        let tuples = Self::batch_for_header(arity, tuples);
        let log2_capacity = config.root_log2_capacity(tuples.len());
        let links = ChainLinks::new(tuples.len());
        let root = Self::build(
            &tuples,
            &links,
            0,
            arity,
            all_rows(&tuples),
            log2_capacity,
            config,
        );
        Self {
            header,
            tuples,
            links,
            root,
            config,
            _layout: PhantomData,
        }
    }

    /// The `incremental` build: one [`insert_at`](Self::insert_at) per row,
    /// in input order. This is the build `from_tuples_with_config` ran
    /// before #107; since #111 it places ids of rows of the batch it keeps,
    /// and checks the batch's arity once, before building.
    ///
    /// # Panics
    ///
    /// Panics if `tuples` holds rows of another arity than `header.arity()`.
    pub(super) fn from_tuples_incrementally(
        header: RelationHeader, config: HashTrieConfig, tuples: Tuples,
    ) -> Self {
        let arity = header.arity();
        let tuples = Self::batch_for_header(arity, tuples);
        let mut trie = Self::with_config_for(header, config, tuples);
        for row in all_rows(&trie.tuples) {
            // As `insert` does: a row's link is `NIL` until a chain threads
            // it, so a row that stays a pruned `Singleton` holds `NIL`, as
            // `bulk` leaves it. (Zeroed links would leave it at 0.)
            trie.links.push(ChainLinks::NIL);
            Self::insert_at(
                &trie.tuples,
                &trie.links,
                &mut trie.root,
                0,
                arity,
                row,
                config.load_factor,
            );
        }
        trie
    }

    /// What the partitioned builds share (`radix:K`, `parallel:N`,
    /// `presized:N`): the batch's arity check, then `fill`, which builds the
    /// root over the batch at the capacity every build gives it
    /// (`HashTrieConfig::root_log2_capacity`, presized under
    /// `root-capacity=tuples`) and returns it with the trie's buffer: the
    /// batch's rows copied into partition order (`morsel::partition_rows`,
    /// #101), the batch then freed. `fill` threads the rows of that buffer
    /// through `links`, one per row (the copy has the batch's row count),
    /// zeroed: every fill writes a row's link before it reads it. Tests in
    /// `parallel.rs` call it with small morsels and regions.
    pub(super) fn from_tuples_partitioned(
        header: RelationHeader, config: HashTrieConfig, tuples: Tuples,
        fill: impl FnOnce(Tuples, &ChainLinks, u32, usize) -> (Tuples, HashTrieNode<P, E>),
    ) -> Self {
        let arity = header.arity();
        let tuples = Self::batch_for_header(arity, tuples);
        let log2_capacity = config.root_log2_capacity(tuples.len());
        let links = ChainLinks::new(tuples.len());
        let (tuples, root) = fill(tuples, &links, log2_capacity, arity);
        debug_assert_eq!(links.len(), tuples.len(), "one link per row of the buffer");
        Self {
            header,
            tuples,
            links,
            root,
            config,
            _layout: PhantomData,
        }
    }
```
  - **Borrowck.** `all_rows(&trie.tuples)` returns a `Range<RowId>` that borrows nothing, so
    `trie.links.push` inside the loop is accepted. The three borrows passed to `insert_at` are
    disjoint fields of the local `trie`.
  - **`fill`'s bound.** `impl FnOnce(Tuples, &ChainLinks, u32, usize) -> …` is higher-ranked over
    the elided reference lifetime, and closures passed straight to the call infer it.

  **m. `HeapSize`, lines 731–742.** Replace with:

```rust
impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> crate::heap_size::HeapSize
    for HashTrie<H, P, E>
{
    /// The buffer (capacity × 8 bytes), the links (capacity × 4 bytes, one
    /// per row), every table's bucket array, and each lazy child's box. A
    /// chain is three values in its bucket and a pruned `Singleton`'s id
    /// lives in its parent's bucket, so neither adds anything: each tuple is
    /// counted once, in the buffer, and each link once, in the links.
    fn heap_size_bytes(&self) -> usize {
        self.tuples.heap_size_bytes() + self.links.heap_size_bytes() + node_heap_bytes(&self.root)
    }
}
```

  **n. `node_heap_bytes` and `tuple_list_heap_bytes`, lines 800–833.** Replace both functions
  with the one below. `tuple_list_heap_bytes` is deleted, and its `#[allow(clippy::ptr_arg)]`
  goes with it.

```rust
/// Heap bytes below the buffer and the links: every table's bucket array,
/// and each lazy child's box.
fn node_heap_bytes<P: PruningPolicy, E: ExpansionPolicy>(node: &HashTrieNode<P, E>) -> usize {
    match node {
        | HashTrieNode::Inner(table) => {
            let shell = table.shell_heap_bytes();
            let children: usize = table.iter().map(|(_, child)| node_heap_bytes(child)).sum();
            shell + children
        },
        // Each chain lives in its bucket; its rows are in the buffer and its
        // successors in the links.
        | HashTrieNode::Leaf(table) => table.shell_heap_bytes(),
        // The id is stored in the bucket; its row is in the buffer.
        | HashTrieNode::Singleton(_) => 0,
        // A pending chain lives in the box, so only a built table adds
        // anything below it.
        | HashTrieNode::Unexpanded(pending) => {
            pending.own_heap_bytes() + pending.built().map_or(0, node_heap_bytes)
        },
    }
}
```
  `map_or(0, node_heap_bytes)` passes the generic function item. `P` and `E` are inferred from
  `Option<&HashTrieNode<P, E>>`. A closure there would trip clippy's `redundant_closure`.

- [ ] **Step 8: `implementation.rs` tests.**

  **a. `heap_size_counts_the_buffer_the_tables_and_four_bytes_per_id`, lines 1061–1092.** Replace
  the function and its doc with:

```rust
    /// `space` after #101 B2: the buffer, every table's bucket array, and
    /// the links, four bytes per row. A chain lives in its bucket, so no
    /// list allocates; a tuple is counted once, in the buffer.
    #[test]
    fn heap_size_counts_the_buffer_the_tables_and_four_bytes_per_id() {
        use crate::HeapSize;
        let trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![4, 5]]);
        let HashTrieNode::Inner(root) = &trie.root else {
            panic!("arity 2 has an Inner root")
        };
        let mut tables = root.shell_heap_bytes();
        for (_, child) in root.iter() {
            let HashTrieNode::Leaf(leaf) = child else {
                panic!("depth 1 of arity 2 is the leaf")
            };
            tables += leaf.shell_heap_bytes();
        }
        // One link per row, allocated once, at the batch's size.
        assert_eq!(trie.links().len(), 3);
        assert_eq!(
            trie.links().heap_size_bytes(),
            3 * std::mem::size_of::<RowId>()
        );
        assert!(trie.tuples().heap_size_bytes() >= 6 * std::mem::size_of::<usize>());
        assert_eq!(
            trie.heap_size_bytes(),
            trie.tuples().heap_size_bytes() + tables + trie.links().heap_size_bytes()
        );
    }
```
  The `links().heap_size_bytes()` call is `ChainLinks`' inherent method, and `HeapSize` is not
  implemented for `ChainLinks`, so there is no ambiguity. `ChainLinks::new(3)` has capacity
  exactly 3: `vec![0u32; 3]` allocates exactly, and the in-place collect into atomics keeps the
  allocation.

  **b. `insert_appends_its_row_and_chains_the_new_id`, lines 1094–1111.** Replace with:

```rust
    /// `insert` appends its tuple to the buffer and a `NIL` link to the
    /// links, then threads the new row: a duplicate joins its twin's chain,
    /// after it.
    #[test]
    fn insert_appends_its_row_and_chains_the_new_id() {
        let mut trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        trie.insert(vec![1, 2]);
        assert_eq!(trie.tuples().to_vecs(), vec![vec![1, 2], vec![1, 2]]);
        let HashTrieNode::Inner(root) = &trie.root else {
            panic!("arity 2 has an Inner root")
        };
        let Some((_, HashTrieNode::Leaf(leaf))) = root.iter().next() else {
            panic!("one root key, whose child is the leaf")
        };
        let Some((_, chain)) = leaf.iter().next() else {
            panic!("one leaf key")
        };
        assert_eq!(chain.walk(trie.links()).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(trie.links().to_vec(), vec![1, ChainLinks::NIL]);
    }
```

  **c. B1's insert-after-build comments** (wording only), lines 1211–1214, 1224–1225 and
  1231–1232. Three before/after pairs.

  Before:

```rust
    /// the same inserts, under Layout `H`, `P`, `E`. Neither comparison
    /// expands a lazy trie: both read a pending list as it stands.
```

  After:

```rust
    /// the same inserts, under Layout `H`, `P`, `E`. Neither comparison
    /// expands a lazy trie: both read a pending chain as it stands.
```

  Before:

```rust
        // first key with a new second value, an append to a pending list
        // under lazy expansion; and an exact duplicate of the first row.
```

  After:

```rust
        // first key with a new second value, an append to a pending chain
        // under lazy expansion; and an exact duplicate of the first row.
```

  Before:

```rust
            // The premise: `LONE_KEY`'s one row is a `Singleton` under
            // pruning, and a pending list under lazy expansion.
```

  After:

```rust
            // The premise: `LONE_KEY`'s one row is a `Singleton` under
            // pruning, and a pending chain under lazy expansion.
```

  **d. `walked_sorted`'s doc, lines 1463–1466.** Before:

```rust
    /// The tuples the trie walk (`for_each_tuple`) lends, sorted: the
    /// multiset stored in the trie's chains, singletons and pending lists.
```

  After:

```rust
    /// The tuples the trie walk (`for_each_tuple`) lends, sorted: the
    /// multiset stored in the trie's chains, singletons and pending chains.
```

  **e. The mirrors in `node_does_not_grow_under_the_expansion_policy`, lines 1657–1667.**
  Before:

```rust
        #[allow(dead_code)]
        enum Mirror {
            Inner(HashTable<()>),
            Leaf(HashTable<Vec<RowId>>),
        }
        #[allow(dead_code)]
        enum PrunedMirror {
            Inner(HashTable<()>),
            Leaf(HashTable<Vec<RowId>>),
            Singleton(RowId),
        }
```

  After (`Chain` comes in through `super::*`):

```rust
        #[allow(dead_code)]
        enum Mirror {
            Inner(HashTable<()>),
            Leaf(HashTable<Chain>),
        }
        #[allow(dead_code)]
        enum PrunedMirror {
            Inner(HashTable<()>),
            Leaf(HashTable<Chain>),
            Singleton(RowId),
        }
```

  **f. `eager_tries_stay_sync`'s doc, lines 1686–1687.** Before:

```rust
    /// Eager tries stay `Sync`: their `Unexpanded` payload is `Never` as a
    /// whole type, so no cell type appears in them. A compile-time pin.
```

  After:

```rust
    /// Eager tries stay `Sync`: their `Unexpanded` payload is `Never` as a
    /// whole type, so no cell type appears in them, and their links are
    /// atomics (`ChainLinks`), which are `Sync`. A compile-time pin.
```

  **g. `lazy_tests`' `use`, lines 1851–1862.** Before:

```rust
    use {
        super::{
            super::{
                expansion::{LazyChild, LazyExpansion, PendingChild},
                pruning::SingletonPruning,
            },
            tests::walked_sorted,
            *,
        },
        crate::{cardinality::Cardinality, heap_size::HeapSize, relation::Projectable},
        kermit_iters::HasOptimizationAxes,
    };
```

  After:

```rust
    use {
        super::{
            super::{
                expansion::{LazyChild, LazyExpansion, PendingChild},
                identity::{configs, inputs},
                pruning::SingletonPruning,
            },
            tests::walked_sorted,
            *,
        },
        crate::{
            cardinality::Cardinality,
            heap_size::HeapSize,
            relation::{ConfigurableRelation, Projectable},
        },
        kermit_iters::HasOptimizationAxes,
    };
```

  **h. `pending_of`, lines 1879–1893.** Before:

```rust
    /// The pending tuples of an unexpanded child, read through `trie`'s
    /// buffer, or `None` for any other node or an expanded child.
    fn pending_of<P: PruningPolicy>(
        trie: &HashTrie<SipHashStrategy, P, LazyExpansion>, node: &HashTrieNode<P, LazyExpansion>,
    ) -> Option<Vec<Vec<usize>>> {
        match node {
            | HashTrieNode::Unexpanded(p) if p.built().is_none() => Some(
                p.pending()
                    .iter()
                    .map(|&row| trie.tuples().row(row).to_vec())
                    .collect(),
            ),
            | _ => None,
        }
    }
```

  After (`pending()` is `None` once expanded, so the guard goes):

```rust
    /// The pending tuples of an unexpanded child, read through `trie`'s
    /// buffer and links, or `None` for any other node or an expanded child.
    fn pending_of<P: PruningPolicy>(
        trie: &HashTrie<SipHashStrategy, P, LazyExpansion>, node: &HashTrieNode<P, LazyExpansion>,
    ) -> Option<Vec<Vec<usize>>> {
        match node {
            | HashTrieNode::Unexpanded(p) => p.pending().map(|chain| {
                chain
                    .walk(trie.links())
                    .map(|row| trie.tuples().row(row).to_vec())
                    .collect()
            }),
            | _ => None,
        }
    }
```

  **i. The end of `lazy_build_leaves_every_root_child_unexpanded`, lines 1910–1913.** Before:

```rust
        let HashTrieNode::Unexpanded(pending) = child(&trie, 1) else {
            panic!("a lazy root bucket holds an Unexpanded child")
        };
        assert_eq!(*pending.pending(), vec![0, 1, 2], "row ids, in input order");
```

  After:

```rust
        let HashTrieNode::Unexpanded(pending) = child(&trie, 1) else {
            panic!("a lazy root bucket holds an Unexpanded child")
        };
        let chain = pending.pending().expect("not yet expanded");
        assert_eq!(
            chain.walk(trie.links()).collect::<Vec<_>>(),
            vec![0, 1, 2],
            "row ids, in input order"
        );
```

  **j. A comment in `walks_read_pending_tuples_without_expanding`, line 2003.** Before:

```rust
        // The trie walk reads each unexpanded child's pending row ids.
```

  After:

```rust
        // The trie walk reads each unexpanded child's pending chain.
```

  **k. `expanded_heap_is_the_eager_heap_plus_one_box_per_child`'s doc, lines 2016–2019.**
  Before:

```rust
    /// Fully expanded, a lazy arity-2 trie is the eager trie plus one
    /// `LazyChild` box per root child. The tables are identical, the
    /// pending ids are moved rather than copied, and the emptied pending lists
    /// hold no heap.
```

  After:

```rust
    /// Fully expanded, a lazy arity-2 trie is the eager trie plus one
    /// `LazyChild` box per root child. The tables and the links are
    /// identical: expansion takes each pending chain and re-threads its rows,
    /// and a chain holds no heap.
```

  **l. The re-threading pin**, new, inserted after the closing `}` of
  `expanded_heap_is_the_eager_heap_plus_one_box_per_child` (line 2030), before
  `projection_of_a_lazy_trie_holds_the_projected_tuples`. It is mutation 4's pin, run in C05b:

```rust
    /// Expands every child below `node`, depth-first, by `resolve` through
    /// `trie` alone, as probes reaching every bucket would.
    fn expand_everything<P: PruningPolicy>(
        trie: &HashTrie<SipHashStrategy, P, LazyExpansion>, node: &HashTrieNode<P, LazyExpansion>,
        depth: usize,
    ) {
        if let HashTrieNode::Inner(table) = trie.resolve(node, depth) {
            for (_, child) in table.iter() {
                expand_everything(trie, child, depth + 1);
            }
        }
    }

    /// `l`, a node of the fully expanded `lazy`, against `e`, `eager`'s
    /// node at the same position: the same tables bucket for bucket, the
    /// same `Singleton` rows, and leaf chains that are the same `Chain`
    /// (head, tail and length) and walk the same rows, each through its own
    /// trie's links. An expanded child is compared as the table it built.
    fn assert_expanded_like_eager<P: PruningPolicy>(
        eager: &HashTrie<SipHashStrategy, P>, e: &HashTrieNode<P, EagerExpansion>,
        lazy: &HashTrie<SipHashStrategy, P, LazyExpansion>, l: &HashTrieNode<P, LazyExpansion>,
        path: &str,
    ) {
        let l = match l {
            | HashTrieNode::Unexpanded(pending) => pending
                .built()
                .unwrap_or_else(|| panic!("{path}: left unexpanded")),
            | other => other,
        };
        match (e, l) {
            | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
                assert_eq!(x.buckets_len(), y.buckets_len(), "{path}: capacity");
                for idx in 0..x.buckets_len() {
                    assert_eq!(x.hash_at(idx), y.hash_at(idx), "{path}: bucket {idx}");
                    if let (Some(a), Some(b)) = (x.value_at(idx), y.value_at(idx)) {
                        assert_expanded_like_eager(eager, a, lazy, b, &format!("{path}/{idx}"));
                    }
                }
            },
            | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
                assert_eq!(x.buckets_len(), y.buckets_len(), "{path}: capacity");
                for idx in 0..x.buckets_len() {
                    assert_eq!(x.hash_at(idx), y.hash_at(idx), "{path}: bucket {idx}");
                    if let (Some(a), Some(b)) = (x.value_at(idx), y.value_at(idx)) {
                        assert_eq!(a, b, "{path}/{idx}: chain");
                        assert_eq!(
                            a.walk(eager.links()).collect::<Vec<_>>(),
                            b.walk(lazy.links()).collect::<Vec<_>>(),
                            "{path}/{idx}: chain rows"
                        );
                    }
                }
            },
            | (HashTrieNode::Singleton(x), HashTrieNode::Singleton(y)) => {
                assert_eq!(x.row(), y.row(), "{path}: singleton")
            },
            | _ => panic!("{path}: node variants differ"),
        }
    }

    /// `resolve` builds a child from its pending chain through `&self`
    /// alone, re-threading the chain's rows into the child's lists in the
    /// trie's links: the case the links' atomics exist for (Decision 3 of
    /// the chain-links design). Expanded everywhere, the lazy trie's links
    /// equal the eager trie's, entry for entry, and every leaf chain walks
    /// the eager trie's rows. Before expansion its links were its root's
    /// pending chains, so the expansions rewrote them.
    fn check_resolve_rethreads<P: PruningPolicy>() {
        for config in configs() {
            for arity in 1..=3 {
                for (input, tuples) in inputs(arity) {
                    let label = format!(
                        "{} arity {arity}, load {}%, root {}, children {}, {input}",
                        P::NAME,
                        config.load_factor.numerator(),
                        config.root_capacity.axis_value(),
                        config.child_capacity.axis_value(),
                    );
                    let eager = HashTrie::<SipHashStrategy, P, EagerExpansion>::from_tuples_with_config(
                        arity.into(),
                        config,
                        tuples.clone(),
                    );
                    // Not `mut`: every expansion below goes through `&lazy`.
                    let lazy = HashTrie::<SipHashStrategy, P, LazyExpansion>::from_tuples_with_config(
                        arity.into(),
                        config,
                        tuples,
                    );
                    if arity == 3 && input == "interleaved" {
                        assert_ne!(
                            lazy.links().to_vec(),
                            eager.links().to_vec(),
                            "{label}: the premise: expansion has rows to re-thread"
                        );
                    }
                    expand_everything(&lazy, lazy.root(), 0);
                    assert_eq!(
                        lazy.links().to_vec(),
                        eager.links().to_vec(),
                        "{label}: links"
                    );
                    assert_expanded_like_eager(&eager, eager.root(), &lazy, lazy.root(), &label);
                }
            }
        }
    }

    #[test]
    fn resolve_rethreads_the_pending_chain_through_a_shared_reference() {
        check_resolve_rethreads::<NoPruning>();
        check_resolve_rethreads::<SingletonPruning>();
    }
```
  - **Why the premise holds.** The `interleaved` rows at arity 3 are `[1,2,3]`, `[2,3,4]`,
    `[1,5,6]`, `[3,1,1]`, `[2,3,9]` and `[1,2,7]`.
    - Unexpanded, the lazy trie's root chains are `[0, 2, 5]`, `[1, 4]` and `[3]`, so row 0's link
      is 2.
    - Eager, every leaf chain holds one row (all six tuples differ in some attribute under
      SipHash), so row 0's link is `NIL`.
    - That holds under both pruning policies and every config.
  - **Why equality holds.** Both builds are `bulk` from the same batch, so the ids agree.
    `child-capacity=tuples` sizes an expanded child from `chain.len()`, the list length the eager
    child was sized from. No `insert` intervenes.
  - **Under mutation 4.** `resolve` groups `chain.walk` over a fresh, zeroed `ChainLinks`. In a
    debug build the walk's tail assertion panics on the first chain of two or more rows. Without
    that assertion, the `: links` assertion fails, because the trie's own links still hold the
    root's threading.
  - **Visibility.** `trie.resolve` is `pub(crate)`. `Chain::walk` is `pub(super)` in
    `hash_trie::chain`, so it is visible throughout `hash_trie`, `lazy_tests` included.

  **m. `root_capacity_tests::check_default_identity`, lines 2135–2139.** Before:

```rust
                // `insert` grows the buffer by amortised doubling, so the two
                // buffers hold the same rows at different capacities. The
                // trie, which holds only ids, must be identical.
                assert_eq!(built.tuples(), inserted.tuples(), "{label}: buffer");
                assert_same_node(built.root(), inserted.root(), &label);
```

  After:

```rust
                // `insert` grows the buffer and the links by amortised
                // doubling, so the two tries hold the same rows and links at
                // different capacities. The trie, which holds only ids, must
                // be identical.
                assert_eq!(built.tuples(), inserted.tuples(), "{label}: buffer");
                assert_eq!(
                    built.links().to_vec(),
                    inserted.links().to_vec(),
                    "{label}: links"
                );
                assert_same_node(
                    built.links(),
                    built.root(),
                    inserted.links(),
                    inserted.root(),
                    &label,
                );
```

  **n. `root_capacity_tests::check_children_unchanged`, lines 2249–2283.** Three edits.

  The doc, lines 2249–2250. Before:

```rust
    /// `tuples` sizes the root only. Under it, every root entry holds the
    /// subtrie (or chain) it holds under `grow`, array for array.
```

  After:

```rust
    /// `tuples` sizes the root only. Under it, every root entry holds the
    /// subtrie (or chain) it holds under `grow`, array for array, and the
    /// links, a function of the lists alone, are equal.
```

  After the `tuple_count` assertion (lines 2266–2270), insert:

```rust
                    assert_eq!(
                        grow.links().to_vec(),
                        presized.links().to_vec(),
                        "{label}: links"
                    );
```

  The `Inner` arm, lines 2279–2283. Before:

```rust
                            | (HashTrieNode::Inner(a), HashTrieNode::Inner(b)) => assert_same_node(
                                a.value_at(idx).unwrap(),
                                b.value_at(at).unwrap(),
                                &format!("{label}/{hash:#x}"),
                            ),
```

  After:

```rust
                            | (HashTrieNode::Inner(a), HashTrieNode::Inner(b)) => assert_same_node(
                                grow.links(),
                                a.value_at(idx).unwrap(),
                                presized.links(),
                                b.value_at(at).unwrap(),
                                &format!("{label}/{hash:#x}"),
                            ),
```

  The `Leaf` arm (2284–2288) is unchanged. It compares `Option<&Chain>` values (head, tail and
  length), which together with the equal links just asserted is the whole chain. `Chain` derives
  `PartialEq` and `Debug`.

  The grep the contract names, after these edits:
  ```bash
  rg -n 'capacity\(\)|\.pending\(\)|from_rows|TupleList|Vec<RowId>|assert_same_node\(' kermit-ds/src/ds/hash_trie/implementation.rs
  ```
  Expected matches, all intended:
  - `value_capacity()` in B1's tests (the buffer);
  - `.pending()` in `visit_at`, `pending_of`, `lazy_build_leaves_every_root_child_unexpanded`;
  - the two `assert_same_node(` calls in `root_capacity_tests`;
  - no `from_rows`, `TupleList` or `Vec<RowId>`.

- [ ] **Step 9: `hash_trie_iter.rs`: a leaf lends `Linked`.** All edits are in
  `kermit-ds/src/ds/hash_trie/hash_trie_iter.rs`.

  **a. `leaf_tuples`, lines 253–262.** Before:

```rust
    fn leaf_tuples(&self) -> Option<LeafRows<'_>> {
        let tuples = self.trie.tuples();
        match self.stack.last()? {
            | Frame::Table {
                node,
                idx,
            } => match *node {
                | HashTrieNode::Leaf(t) => {
                    t.value_at(*idx).map(|chain| LeafRows::new(tuples, chain))
                },
```

  After:

```rust
    fn leaf_tuples(&self) -> Option<LeafRows<'_>> {
        // A leaf bucket lends its chain linked, walked through the trie's
        // links; a singleton frame lends its one row id as a listed chain.
        let (tuples, links) = (self.trie.tuples(), self.trie.links());
        match self.stack.last()? {
            | Frame::Table {
                node,
                idx,
            } => match *node {
                | HashTrieNode::Leaf(t) => {
                    t.value_at(*idx).map(|chain| chain.leaf_rows(tuples, links))
                },
```

  The rest of the function (the `Inner` and unreachable arms, and the `Frame::Singleton` arm's
  `LeafRows::new(tuples, std::slice::from_ref(s.row()))`) is unchanged. `self.trie` is a
  `&'a HashTrie`, so both borrows outlive `&self`, as `tuples` already did.

  **b. `a_leaf_chain_lends_row_ids_into_the_trie_buffer`** (lines 684–701 at HEAD; T3 rewrote its
  `ids()` line). Replace the whole function with:

```rust
    #[test]
    fn a_leaf_chain_lends_row_ids_into_the_trie_buffer() {
        // The two copies of (1, 2) share one chain. Its ids are their input
        // positions, in input order, linked through the trie's links (row
        // 0's link names row 2, whose link is `NIL`), and its rows are read
        // from the trie's own buffer, never copied.
        use kermit_iters::ChainLinks;
        let trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4], vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open());
        assert!(it.lookup(h(1)));
        assert!(it.open());
        assert!(it.lookup(h(2)));
        let rows = it.leaf_tuples().expect("the chain of (1, 2)");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.ids().collect::<Vec<_>>(), vec![0, 2]);
        assert_eq!(rows.arity(), 2);
        assert_eq!(rows.to_vecs(), vec![vec![1, 2], vec![1, 2]]);
        assert_eq!(rows.data().as_ptr(), trie.tuples().as_flat().as_ptr());
        assert_eq!(trie.links().to_vec(), vec![
            2,
            ChainLinks::NIL,
            ChainLinks::NIL
        ]);
    }
```

  **c. `singleton_frames_lend_their_row_id`** (lines 703–720 at HEAD; T3 rewrote its `ids()`
  line). Replace the whole function with:

```rust
    #[test]
    fn singleton_frames_lend_their_row_id() {
        // (4, 5, 6) is alone below 4, so under pruning both levels below
        // the root are singleton frames. `frame_for` builds the first and
        // `open`'s `Descent::Deeper` the second. The leaf one lends its
        // payload's row id, 2, as a listed chain over the trie's buffer. The
        // row ends no chain, so its link is `NIL`.
        use kermit_iters::ChainLinks;
        let trie = Pruned::from_tuples(3.into(), vec![vec![1, 2, 3], vec![1, 2, 7], vec![4, 5, 6]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open());
        assert!(it.lookup(h(4)));
        assert!(it.open());
        assert!(it.open());
        assert!(matches!(it.stack.last(), Some(Frame::Singleton(_))));
        let rows = it.leaf_tuples().expect("singleton at leaf depth");
        assert_eq!(rows.ids().collect::<Vec<_>>(), vec![2]);
        assert_eq!(rows.to_vecs(), vec![vec![4, 5, 6]]);
        assert_eq!(rows.data().as_ptr(), trie.tuples().as_flat().as_ptr());
        assert_eq!(trie.links().get(2), ChainLinks::NIL);
    }
```
  The rest of the iterator's tests read `to_vecs()`, `is_empty()` or `iter()`, which T3 made
  form-agnostic, so they need nothing.

- [ ] **Step 10: `identity.rs`: `Rows`, links on the id-level helpers, and the links check.**
  Replace the whole of `kermit-ds/src/ds/hash_trie/identity.rs` with:

```rust
//! Identity checks for the HashTrie builds (#91, #94, #88, #101). Every
//! build mode must build the trie the bulk build does, the root-capacity
//! Config must leave every subtrie as it was, and the `presized:N` build
//! must be equivalent (Amendment 2), so the radix, parallel and
//! root-capacity tests share these assertions and inputs.
//!
//! A trie's singletons hold row ids into its buffer (#111), and its leaf and
//! pending chains are lists of such rows threaded through its links (#101
//! B2), so two tries compare at one of two levels:
//!
//! - **Array level**, [`assert_same_trie`] and [`assert_same_node`]: equal ids.
//!   Every chain is the same `Chain` (head, tail and length) and walks the
//!   same ids, each through its own trie's links. Equal ids name equal tuples
//!   only over equal buffers, so `assert_same_trie` compares the buffers
//!   first, then the links arrays whole. For builds that keep their input
//!   batch as their buffer, as `bulk` and `incremental` do.
//! - **Content level**, [`assert_same_contents`] and
//!   [`assert_same_node_contents`]: every table bucket for bucket, as at the
//!   array level, and every chain and singleton naming the same tuples in the
//!   same order, each read through its own trie's buffer and links
//!   ([`Rows`]); the buffers hold the same rows at the same capacity. For
//!   builds that may store their rows in another order than they arrived in
//!   (#101 layer 3's partitioned copy), whose ids, and so links, differ.
//!
//! `presized:N` may place root keys in other buckets (Amendment 2):
//! [`assert_equivalent_root`] checks its root rule over roots built from
//! one input batch, with children compared by id, and
//! [`assert_equivalent_trie`] checks whole tries, with children compared
//! by contents.

use {
    super::{
        chain::Chain,
        config::{ChildCapacity, HashTrieConfig, LoadFactor, RootCapacity},
        expansion::{ExpansionPolicy, PendingChild},
        hash_table::{home_bucket, HashTable},
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::{PruningPolicy, SingletonPayload},
    },
    crate::{cardinality::Cardinality, heap_size::HeapSize, test_support::Lcg},
    kermit_iters::{ChainLinks, HashStrategy, RowId, Tuples},
};

/// A trie's rows as its chains name them: its buffer and its links. A
/// content-level comparison reads each side's chains through its own pair.
#[derive(Clone, Copy)]
pub(super) struct Rows<'t> {
    tuples: &'t Tuples,
    links: &'t ChainLinks,
}

impl<'t> Rows<'t> {
    /// The rows of `tuples`, as the chains threaded through `links` name
    /// them.
    pub(super) fn new(tuples: &'t Tuples, links: &'t ChainLinks) -> Self {
        Self {
            tuples,
            links,
        }
    }

    /// `trie`'s buffer and links.
    pub(super) fn of<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
        trie: &'t HashTrie<H, P, E>,
    ) -> Self {
        Self::new(trie.tuples(), trie.links())
    }

    /// The rows `chain` names, in chain order.
    fn of_chain(self, chain: &Chain) -> Vec<&'t [usize]> {
        let Rows {
            tuples,
            links,
        } = self;
        chain.walk(links).map(|row| tuples.row(row)).collect()
    }

    /// The row `row` names.
    fn row(self, row: RowId) -> &'t [usize] { self.tuples.row(row) }
}

/// Asserts that two tables hold the same buckets: the same capacity
/// (array length and allocation), the same length, the same hash in
/// every bucket, and values that `same_value` accepts.
fn assert_same_table<V>(
    a: &HashTable<V>, b: &HashTable<V>, path: &str, same_value: &dyn Fn(&V, &V, &str),
) {
    assert_eq!(a.buckets_len(), b.buckets_len(), "{path}: capacity");
    assert_eq!(
        a.shell_heap_bytes(),
        b.shell_heap_bytes(),
        "{path}: bucket allocation"
    );
    assert_eq!(a.len(), b.len(), "{path}: len");
    for idx in 0..a.buckets_len() {
        assert_eq!(a.hash_at(idx), b.hash_at(idx), "{path}: bucket {idx}");
        if let (Some(x), Some(y)) = (a.value_at(idx), b.value_at(idx)) {
            same_value(x, y, &format!("{path}/{idx}"));
        }
    }
}

/// Two chains, each walked through its own trie's links: the same `Chain`
/// (head, tail and length), and the same row ids in chain order.
fn assert_same_chain(
    a_links: &ChainLinks, a: &Chain, b_links: &ChainLinks, b: &Chain, path: &str,
) {
    assert_eq!(a, b, "{path}: chain");
    assert_eq!(
        a.walk(a_links).collect::<Vec<_>>(),
        b.walk(b_links).collect::<Vec<_>>(),
        "{path}: chain ids"
    );
}

/// Two nodes at the array level: every table bucket for bucket, every chain
/// the same `Chain` walking the same ids (`a`'s through `a_links`, `b`'s
/// through `b_links`), every singleton the same row id, and every pending
/// chain the same.
pub(super) fn assert_same_node<P: PruningPolicy, E: ExpansionPolicy>(
    a_links: &ChainLinks, a: &HashTrieNode<P, E>, b_links: &ChainLinks, b: &HashTrieNode<P, E>,
    path: &str,
) {
    match (a, b) {
        | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
            assert_same_table(x, y, path, &|x, y, path| {
                assert_same_node(a_links, x, b_links, y, path)
            })
        },
        | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
            assert_same_table(x, y, path, &|x, y, path| {
                assert_same_chain(a_links, x, b_links, y, path)
            })
        },
        | (HashTrieNode::Singleton(x), HashTrieNode::Singleton(y)) => {
            assert_eq!(x.row(), y.row(), "{path}: singleton");
        },
        | (HashTrieNode::Unexpanded(x), HashTrieNode::Unexpanded(y)) => {
            // Building expands nothing, so both children still hold the
            // pending chains `insert_at` or `child` left them.
            let (Some(x), Some(y)) = (x.pending(), y.pending()) else {
                panic!("{path}: expanded during the build")
            };
            assert_same_chain(a_links, &x, b_links, &y, &format!("{path}: pending"));
        },
        | _ => panic!("{path}: node variants differ"),
    }
}

/// The premise of every id comparison: the two tries own equal buffers,
/// row for row and at the same capacity.
fn assert_same_buffer<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_eq!(a.tuples(), b.tuples(), "{label}: buffer");
    assert_eq!(
        a.tuples().heap_size_bytes(),
        b.tuples().heap_size_bytes(),
        "{label}: buffer capacity"
    );
}

/// The array-level identity the standard requires of a BuildMode: the
/// buffer, the links array whole, every table's buckets and capacity, every
/// chain, singleton and pending chain, the heap size and the tuple count.
/// The links are compared before any node, so a build that threads a row
/// differently fails at `: links`.
pub(super) fn assert_same_trie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_same_buffer(a, b, label);
    assert_eq!(a.links().to_vec(), b.links().to_vec(), "{label}: links");
    assert_same_node(a.links(), a.root(), b.links(), b.root(), label);
    assert_eq!(
        a.heap_size_bytes(),
        b.heap_size_bytes(),
        "{label}: heap size"
    );
    assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
}

/// Two chains, each read through its own trie's buffer and links: the same
/// rows, in the same order. The ids may differ.
fn assert_same_chain_contents(a: Rows<'_>, x: &Chain, b: Rows<'_>, y: &Chain, path: &str) {
    assert_eq!(a.of_chain(x), b.of_chain(y), "{path}: chain rows");
}

/// [`assert_same_node`] by contents: every table bucket for bucket, as
/// there, but every chain, singleton and pending chain compared by the
/// tuples it names, `x`'s read through `a` and `y`'s through `b`.
pub(super) fn assert_same_node_contents<P: PruningPolicy, E: ExpansionPolicy>(
    a: Rows<'_>, x: &HashTrieNode<P, E>, b: Rows<'_>, y: &HashTrieNode<P, E>, path: &str,
) {
    match (x, y) {
        | (HashTrieNode::Inner(s), HashTrieNode::Inner(t)) => {
            assert_same_table(s, t, path, &|s, t, path| {
                assert_same_node_contents(a, s, b, t, path)
            })
        },
        | (HashTrieNode::Leaf(s), HashTrieNode::Leaf(t)) => {
            assert_same_table(s, t, path, &|s, t, path| {
                assert_same_chain_contents(a, s, b, t, path)
            })
        },
        | (HashTrieNode::Singleton(s), HashTrieNode::Singleton(t)) => {
            assert_eq!(
                a.row(*s.row()),
                b.row(*t.row()),
                "{path}: singleton row"
            );
        },
        | (HashTrieNode::Unexpanded(s), HashTrieNode::Unexpanded(t)) => {
            // As in `assert_same_node`: building expands nothing.
            let (Some(s), Some(t)) = (s.pending(), t.pending()) else {
                panic!("{path}: expanded during the build")
            };
            assert_same_chain_contents(a, &s, b, &t, &format!("{path}: pending"));
        },
        | _ => panic!("{path}: node variants differ"),
    }
}

/// Two content-equal tries' buffers: the same rows as a multiset, at the
/// same capacity. The order of their rows, and so every id, may differ.
fn assert_same_buffer_contents(a: &Tuples, b: &Tuples, label: &str) {
    let sorted = |tuples: &Tuples| {
        let mut copy = tuples.clone();
        copy.sort();
        copy
    };
    assert_eq!(sorted(a), sorted(b), "{label}: buffer rows");
    assert_eq!(
        a.value_capacity(),
        b.value_capacity(),
        "{label}: buffer capacity"
    );
}

/// The content-level identity a build that stores its rows in another
/// order owes `bulk` (#101 layer 3):
///
/// - every table bucket for bucket, the root included, so this is also the
///   check that a build keeps `bulk`'s root;
/// - every chain, singleton and pending chain naming the same tuples in the
///   same order, each read through its own trie's buffer and links;
/// - buffers holding the same rows at the same capacity;
/// - and the heap size and tuple count.
///
/// The walk runs first, so a row that differs is reported where it sits.
pub(super) fn assert_same_contents<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_same_node_contents(Rows::of(a), a.root(), Rows::of(b), b.root(), label);
    assert_same_buffer_contents(a.tuples(), b.tuples(), label);
    assert_eq!(
        a.heap_size_bytes(),
        b.heap_size_bytes(),
        "{label}: heap size"
    );
    assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
}

/// Rows of up to three columns, cut to `arity`, as a batch whose capacity
/// is exactly its rows, so a batch and its clone have equal capacities.
pub(super) fn rows(arity: usize, rows: &[[usize; 3]]) -> Tuples {
    let mut tuples = Tuples::with_capacity(arity, rows.len());
    for row in rows {
        tuples.push(&row[..arity]);
    }
    tuples
}

/// Enough distinct first values to double the root several times (three
/// times under Miri), with repeats so subtries and chains hold several
/// tuples. The builds are safe Rust, so Miri runs a small matrix: the full
/// one took almost five minutes there.
const RANDOM_TUPLES: usize = if cfg!(miri) {
    48
} else {
    3_000
};

/// Load factors under test, in percent. Miri runs the default only.
pub(super) const LOAD_PERCENTS: &[u8] = if cfg!(miri) {
    &[70]
} else {
    &[70, 50]
};

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

/// The shared inputs, each a batch of `arity` whose capacity is exactly its
/// rows (see [`rows`]). The random rows are the draws made before #111, in
/// the same order.
pub(super) fn inputs(arity: usize) -> Vec<(&'static str, Tuples)> {
    let mut lcg = Lcg(0x91);
    let mut random = Tuples::with_capacity(arity, RANDOM_TUPLES);
    for _ in 0..RANDOM_TUPLES {
        let row = [
            lcg.next_usize() % (RANDOM_TUPLES / 4),
            lcg.next_usize() % 50,
            lcg.next_usize() % 7,
        ];
        random.push(&row[..arity]);
    }
    vec![
        ("empty", Tuples::new(arity)),
        ("one tuple", rows(arity, &[[1, 2, 3]])),
        (
            "duplicates",
            rows(arity, &[[1, 2, 3], [1, 2, 3], [1, 2, 3]]),
        ),
        (
            "interleaved",
            rows(arity, &[
                [1, 2, 3],
                [2, 3, 4],
                [1, 5, 6],
                [3, 1, 1],
                [2, 3, 9],
                [1, 2, 7],
            ]),
        ),
        ("random", random),
    ]
}

/// The equivalence Amendment 2 asks of a build that may place root keys in
/// other buckets. The root has the same capacity, allocation and length,
/// occupies the same buckets, and has the same total probe displacement.
/// Every key's child or chain is array-identical, `a`'s walked through
/// `a_links` and `b`'s through `b_links`.
pub(super) fn assert_equivalent_root<P: PruningPolicy, E: ExpansionPolicy>(
    a_links: &ChainLinks, a: &HashTrieNode<P, E>, b_links: &ChainLinks, b: &HashTrieNode<P, E>,
    label: &str,
) {
    match (a, b) {
        | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
            assert_equivalent_table(x, y, label, &|x, y, path| {
                assert_same_node(a_links, x, b_links, y, path)
            })
        },
        | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
            assert_equivalent_table(x, y, label, &|x, y, path| {
                assert_same_chain(a_links, x, b_links, y, path)
            })
        },
        | _ => panic!("{label}: root variants differ"),
    }
}

/// [`assert_equivalent_root`]'s root rule with every key's child or chain
/// compared by contents ([`assert_same_node_contents`]), `x`'s read through
/// `a` and `y`'s through `b`: the rule for builds that may also store their
/// rows in another order (#101 layer 3).
fn assert_equivalent_root_contents<P: PruningPolicy, E: ExpansionPolicy>(
    a: Rows<'_>, x: &HashTrieNode<P, E>, b: Rows<'_>, y: &HashTrieNode<P, E>, label: &str,
) {
    match (x, y) {
        | (HashTrieNode::Inner(s), HashTrieNode::Inner(t)) => {
            assert_equivalent_table(s, t, label, &|s, t, path| {
                assert_same_node_contents(a, s, b, t, path)
            })
        },
        | (HashTrieNode::Leaf(s), HashTrieNode::Leaf(t)) => {
            assert_equivalent_table(s, t, label, &|s, t, path| {
                assert_same_chain_contents(a, s, b, t, path)
            })
        },
        | _ => panic!("{label}: root variants differ"),
    }
}

fn assert_equivalent_table<V>(
    a: &HashTable<V>, b: &HashTable<V>, path: &str, same_value: &dyn Fn(&V, &V, &str),
) {
    assert_eq!(a.buckets_len(), b.buckets_len(), "{path}: capacity");
    assert_eq!(
        a.shell_heap_bytes(),
        b.shell_heap_bytes(),
        "{path}: bucket allocation"
    );
    assert_eq!(a.len(), b.len(), "{path}: len");
    let cap = a.buckets_len();
    let log2 = cap.trailing_zeros();
    let occupied = |t: &HashTable<V>| {
        (0..cap)
            .filter(|&i| t.hash_at(i).is_some())
            .collect::<Vec<_>>()
    };
    assert_eq!(occupied(a), occupied(b), "{path}: occupied buckets");
    let displacement = |t: &HashTable<V>| {
        (0..cap)
            .filter_map(|i| t.hash_at(i).map(|h| (i + cap - home_bucket(h, log2)) % cap))
            .sum::<usize>()
    };
    assert_eq!(
        displacement(a),
        displacement(b),
        "{path}: total displacement"
    );
    for idx in 0..cap {
        if let (Some(hash), Some(x)) = (a.hash_at(idx), a.value_at(idx)) {
            let j = b
                .index_of(hash)
                .unwrap_or_else(|| panic!("{path}: hash {hash:#x} missing"));
            same_value(
                x,
                b.value_at(j).expect("index_of found it"),
                &format!("{path}/{hash:#x}"),
            );
        }
    }
}

/// `presized:N`'s rule for whole tries: [`assert_equivalent_root`]'s root
/// rule with every child compared by contents, buffers holding the same
/// rows at the same capacity, and the same heap size and tuple count.
pub(super) fn assert_equivalent_trie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_same_buffer_contents(a.tuples(), b.tuples(), label);
    assert_equivalent_root_contents(Rows::of(a), a.root(), Rows::of(b), b.root(), label);
    assert_eq!(
        a.heap_size_bytes(),
        b.heap_size_bytes(),
        "{label}: heap size"
    );
    assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            ds::hash_trie::{
                bulk::all_rows,
                expansion::{EagerExpansion, LazyExpansion},
                pruning::{NoPruning, SingletonPruning},
            },
            relation::{ConfigurableRelation, Relation},
            test_support::Mod10HashStrategy,
        },
        kermit_iters::SipHashStrategy,
    };

    /// The rows of [`inputs`]' "interleaved" batch, at arity 3.
    const INTERLEAVED: [[usize; 3]; 6] = [[1, 2, 3], [2, 3, 4], [1, 5, 6], [3, 1, 1], [2, 3, 9], [
        1, 2, 7,
    ]];

    /// Two builds of one input are the same at the content level. Every
    /// node variant meets the comparison: `Inner` and `Leaf` at every arity
    /// above 1, `Singleton` under pruning, `Unexpanded` under lazy
    /// expansion.
    fn check_accepts_the_same_trie<P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for (input, tuples) in inputs(arity) {
                let label = format!("{}/{} arity {arity}, {input}", P::NAME, E::NAME);
                let a =
                    HashTrie::<SipHashStrategy, P, E>::from_tuples(arity.into(), tuples.clone());
                let b = HashTrie::<SipHashStrategy, P, E>::from_tuples(arity.into(), tuples);
                assert_same_contents(&a, &b, &label);
            }
        }
    }

    #[test]
    fn same_contents_accepts_the_same_trie() {
        check_accepts_the_same_trie::<NoPruning, EagerExpansion>();
        check_accepts_the_same_trie::<SingletonPruning, EagerExpansion>();
        check_accepts_the_same_trie::<NoPruning, LazyExpansion>();
        check_accepts_the_same_trie::<SingletonPruning, LazyExpansion>();
    }

    /// `tuples`' rows in reverse order, and the ids that name `tuples' rows
    /// in the reversed batch, in input order.
    fn reverse_rows(tuples: &Tuples) -> (Tuples, Vec<RowId>) {
        let mut reversed = Tuples::with_capacity(tuples.arity(), tuples.len());
        for row in all_rows(tuples).rev() {
            reversed.push(tuples.row(row));
        }
        (reversed, all_rows(tuples).rev().collect())
    }

    /// The bulk build's root (Algorithm 2 from depth 0, as
    /// `from_tuples_in_bulk` builds it) over the rows of `tuples` that `ids`
    /// names, in that order, with the links it threads them through.
    fn root_over<P: PruningPolicy, E: ExpansionPolicy>(
        tuples: &Tuples, arity: usize, ids: impl IntoIterator<Item = RowId>,
    ) -> (ChainLinks, HashTrieNode<P, E>) {
        let config = HashTrieConfig::default();
        let log2_capacity = config.root_log2_capacity(tuples.len());
        let links = ChainLinks::new(tuples.len());
        let root = HashTrie::<SipHashStrategy, P, E>::build(
            tuples,
            &links,
            0,
            arity,
            ids,
            log2_capacity,
            config,
        );
        (links, root)
    }

    /// Calls `check` on every input's root and on the root built over the
    /// reversed batch from its rows in input order: the same tuples under
    /// other ids, at every arity, each with its own buffer and links.
    fn for_each_reversed_root<P: PruningPolicy, E: ExpansionPolicy>(
        check: impl Fn(Rows<'_>, &HashTrieNode<P, E>, Rows<'_>, &HashTrieNode<P, E>, &str),
    ) {
        for arity in 1..=3 {
            for (input, tuples) in inputs(arity) {
                let label = format!("{}/{} arity {arity}, {input}", P::NAME, E::NAME);
                let (reversed, ids) = reverse_rows(&tuples);
                let (forward_links, forward) = root_over::<P, E>(&tuples, arity, all_rows(&tuples));
                let (backward_links, backward) = root_over::<P, E>(&reversed, arity, ids);
                check(
                    Rows::new(&tuples, &forward_links),
                    &forward,
                    Rows::new(&reversed, &backward_links),
                    &backward,
                    &label,
                );
            }
        }
    }

    #[test]
    fn same_node_contents_accepts_a_reversed_buffer() {
        for_each_reversed_root::<NoPruning, EagerExpansion>(assert_same_node_contents);
        for_each_reversed_root::<SingletonPruning, EagerExpansion>(assert_same_node_contents);
        for_each_reversed_root::<NoPruning, LazyExpansion>(assert_same_node_contents);
        for_each_reversed_root::<SingletonPruning, LazyExpansion>(assert_same_node_contents);
    }

    /// `assert_equivalent_trie`'s root rule reads each child through its
    /// own buffer and links, so a reversed buffer's root is equivalent to
    /// the forward one.
    #[test]
    fn equivalent_root_contents_accepts_a_reversed_buffer() {
        for_each_reversed_root::<NoPruning, EagerExpansion>(assert_equivalent_root_contents);
        for_each_reversed_root::<SingletonPruning, EagerExpansion>(assert_equivalent_root_contents);
        for_each_reversed_root::<NoPruning, LazyExpansion>(assert_equivalent_root_contents);
        for_each_reversed_root::<SingletonPruning, LazyExpansion>(assert_equivalent_root_contents);
    }

    /// Guards the test above. Compared by id, the reversed root differs,
    /// so that test passes on the tuples, not because the ids stayed put.
    #[test]
    #[should_panic(expected = ": chain")]
    fn array_level_rejects_a_reversed_buffer() {
        let tuples = rows(2, &INTERLEAVED);
        let (reversed, ids) = reverse_rows(&tuples);
        let (forward_links, forward) =
            root_over::<NoPruning, EagerExpansion>(&tuples, 2, all_rows(&tuples));
        let (backward_links, backward) = root_over::<NoPruning, EagerExpansion>(&reversed, 2, ids);
        assert_same_node(
            &forward_links,
            &forward,
            &backward_links,
            &backward,
            "reversed",
        );
    }

    /// `assert_same_trie` compares the links whole, before any node. Two
    /// builds of one input, with one link of the second rewritten (through
    /// `&`, as lazy expansion writes), fail there.
    #[test]
    #[should_panic(expected = ": links")]
    fn same_trie_rejects_another_link() {
        let a: HashTrie = HashTrie::from_tuples(2.into(), rows(2, &INTERLEAVED));
        let b: HashTrie = HashTrie::from_tuples(2.into(), rows(2, &INTERLEAVED));
        // Rows 0 and 5 are both `[1, 2]`, so they share a leaf chain.
        assert_eq!(b.links().get(0), 5, "the premise");
        b.links().set(0, ChainLinks::NIL);
        assert_same_trie(&a, &b, "rewritten");
    }

    /// [`INTERLEAVED`] against a copy whose row `index` has its last value
    /// raised by 10. `Mod10HashStrategy` hashes both values alike, so every
    /// table matches bucket for bucket and only the rows differ.
    fn check_rows_that_hash_alike<P: PruningPolicy, E: ExpansionPolicy>(index: usize) {
        let mut changed = INTERLEAVED;
        changed[index][2] += 10;
        let a = HashTrie::<Mod10HashStrategy, P, E>::from_tuples(3.into(), rows(3, &INTERLEAVED));
        let b = HashTrie::<Mod10HashStrategy, P, E>::from_tuples(3.into(), rows(3, &changed));
        assert_same_contents(&a, &b, "hash-alike");
    }

    /// Row 5, `[1, 2, 7]`, sits in a leaf chain.
    #[test]
    #[should_panic(expected = "chain rows")]
    fn same_contents_rejects_a_chain_row_that_hashes_alike() {
        check_rows_that_hash_alike::<NoPruning, EagerExpansion>(5);
    }

    /// Row 3, `[3, 1, 1]`, is the only tuple under key 3, so pruning makes
    /// it a `Singleton`.
    #[test]
    #[should_panic(expected = "singleton row")]
    fn same_contents_rejects_a_singleton_row_that_hashes_alike() {
        check_rows_that_hash_alike::<SingletonPruning, EagerExpansion>(3);
    }

    /// Under lazy expansion every root child is a pending chain.
    #[test]
    #[should_panic(expected = "pending: chain rows")]
    fn same_contents_rejects_a_pending_row_that_hashes_alike() {
        check_rows_that_hash_alike::<NoPruning, LazyExpansion>(3);
    }

    /// One chain holds the same two rows in the other order. The buffers
    /// hold the same multiset and every hash matches, so only the chain's
    /// order tells the tries apart.
    #[test]
    #[should_panic(expected = "chain rows")]
    fn same_contents_rejects_a_chain_in_another_order() {
        let a: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples(2.into(), rows(2, &[[1, 2, 0], [1, 12, 0]]));
        let b: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples(2.into(), rows(2, &[[1, 12, 0], [1, 2, 0]]));
        assert_same_contents(&a, &b, "reordered");
    }

    /// `assert_equivalent_trie` reads each side's chains through its own
    /// buffer and links. The pair of
    /// `same_contents_rejects_a_chain_in_another_order` has the same buffer
    /// multiset, capacity and root buckets, and equal ids in the chain, so
    /// only the rows those ids name tell the tries apart. (The hash-alike
    /// pair would fail the buffer check first.)
    #[test]
    #[should_panic(expected = "chain rows")]
    fn equivalent_trie_rejects_a_chain_in_another_order() {
        let a: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples(2.into(), rows(2, &[[1, 2, 0], [1, 12, 0]]));
        let b: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples(2.into(), rows(2, &[[1, 12, 0], [1, 2, 0]]));
        assert_equivalent_trie(&a, &b, "reordered");
    }

    /// `equivalent_trie_rejects_a_chain_in_another_order` at arity 1, where
    /// the root is a `Leaf`: `[1]` and `[11]` share hash 1, so they share
    /// the root's one chain, in the other order.
    #[test]
    #[should_panic(expected = "chain rows")]
    fn equivalent_trie_rejects_an_arity_1_chain_in_another_order() {
        let a: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples(1.into(), rows(1, &[[1, 0, 0], [11, 0, 0]]));
        let b: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples(1.into(), rows(1, &[[11, 0, 0], [1, 0, 0]]));
        assert_equivalent_trie(&a, &b, "reordered");
    }

    /// The same trie over a buffer with spare capacity: equal nodes and
    /// rows, another capacity.
    #[test]
    #[should_panic(expected = "buffer capacity")]
    fn same_contents_rejects_another_buffer_capacity() {
        let exact = rows(2, &INTERLEAVED);
        let mut spare = Tuples::with_capacity(2, 64);
        for row in exact.rows() {
            spare.push(row);
        }
        let a: HashTrie = HashTrie::from_tuples(2.into(), exact);
        let b: HashTrie = HashTrie::from_tuples(2.into(), spare);
        assert_same_contents(&a, &b, "spare");
    }

    /// The root is compared bucket for bucket, too. The same rows under a
    /// root of another capacity (8 buckets grown, 16 sized for six tuples
    /// at 70 %) fail, so `assert_same_contents` is also the check that a
    /// build keeps `bulk`'s root.
    #[test]
    #[should_panic(expected = "root capacity: capacity")]
    fn same_contents_compares_the_root_bucket_for_bucket() {
        let build = |root_capacity| {
            let config = HashTrieConfig {
                root_capacity,
                ..HashTrieConfig::default()
            };
            HashTrie::<SipHashStrategy>::from_tuples_with_config(
                2.into(),
                config,
                rows(2, &INTERLEAVED),
            )
        };
        assert_same_contents(
            &build(RootCapacity::Grow),
            &build(RootCapacity::Tuples),
            "root capacity",
        );
    }
}
```

  What changed, against HEAD:
  - **`Rows`** is new (the contract's type), with two private helpers, `of_chain` and `row`. It
    replaces `rows_by_id` and every `a_tuples` / `b_tuples` pair.
  - **`assert_same_chain`** takes `(a_links, a, b_links, b, path)`. It compares `Chain` values
    (message `: chain`) and the walked ids. The capacity check is gone, since a chain has none.
  - **`assert_same_node` and `assert_equivalent_root`** take each side's links before its node.
  - **`assert_same_trie`** adds `"{label}: links"`, after the buffer check and before the nodes.
  - **The content-level functions** take `Rows`. Their messages keep the substrings the
    `should_panic` tests expect:
    - `chain rows` from `assert_same_chain_contents`;
    - `pending: chain rows` through the `"{path}: pending"` path;
    - `singleton row`, `buffer capacity` and `root capacity: capacity` unchanged.
  - **The self-tests.**
    - `root_over` returns `(ChainLinks, HashTrieNode)`.
    - `for_each_reversed_root`'s `check` takes `Rows`.
    - `array_level_rejects_a_reversed_buffer` passes both links. The reversed root's first leaf
      chain has another head, so the first assertion to fail is `: chain`.
    - `same_trie_rejects_another_link` is new: it checks that the links comparison is not vacuous.
      Row 0's link is 5, because rows 0 and 5 (`[1, 2]`) share the chain `[0, 5]`.
  - **Fn items as `check`.** Passing `assert_same_node_contents` and
    `assert_equivalent_root_contents` works because their `Rows<'_>` and `&` parameters are
    late-bound, so the items satisfy the higher-ranked `impl Fn(Rows<'_>, …)`.
  - **Callers C05b must update.** `assert_equivalent_root` is called only by `parallel.rs`'s
    `check_one_run`. No other file calls `assert_same_node_contents` or `Rows` directly.

- [ ] **Step 11: Check `kermit-ds/tests` (no change expected).**
  ```bash
  rg -n 'TupleList|from_rows|\.pending\(\)|insert_at|build_nested|assert_same_node|links\(\)|ChainLinks|\.row\(|ids\(\)' kermit-ds/tests
  ```
  Expected:
  - one comment hit, `hash_trie_tests.rs:168` (`… one \`insert_at\` per tuple …`);
  - at most the leaf-view lines T3 migrated in `common/macros.rs` (around line 1255, where
    `chain.row(0)` stood at HEAD). They read a `LeafRows`, whose API T3 fixed, not a trie
    internal.

  Nothing else matches. The integration tests use only the public API:
  `from_tuples*`, `for_each_tuple`, `collect_tuples`, `hash_trie_iter` and `leaf_tuples`, whose
  `LeafRows` T3 made form-agnostic. C05a changes no public signature. Nothing in
  `kermit-ds/tests` changes. The same holds for `kermit/tests` and `kermit-algos`, which name no
  `hash_trie` internals.

- [ ] **Step 12: Sweep C05a's files for leftovers.**
  ```bash
  rg -n 'TupleList|from_rows|tuple_list_heap_bytes|RefCell|shrink_to|allow\(dead_code\)' \
    kermit-ds/src/ds/hash_trie/{chain,expansion,node,bulk,implementation,hash_trie_iter,identity}.rs
  rg -n 'Vec<RowId>' \
    kermit-ds/src/ds/hash_trie/{chain,expansion,node,bulk,implementation,hash_trie_iter,identity}.rs
  ```
  Expected:
  - the first command: two hits, both in `implementation.rs`'s
    `node_does_not_grow_under_the_expansion_policy` (the mirrors' `#[allow(dead_code)]`). Nothing
    else;
  - the second: only test code, all intended:
    - `chain.rs`: the 24-byte pin's `Option<Entry<Vec<RowId>>>`, and its doc line;
    - `expansion.rs` tests: `type Lazy = Box<LazyChild<Vec<RowId>>>`, its doc, and
      `own_heap_bytes_is_the_box`'s `size_of`.

- [ ] **Step 13: Confirm that only C05b's files fail to compile.** C05a ends with the tree not
  compiling, so this step checks where it fails.
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l   # prints 0
  nix develop --command env CARGO_BUILD_JOBS=2 cargo check -p kermit-ds --all-targets --message-format short 2>&1 \
    | rg ': error' | rg -o '^[^:]+\.rs:[0-9]+' | sort -u
  ```
  Expected: every location is in one of these places:
  - `kermit-ds/src/ds/hash_trie/radix.rs` (`bulk::TupleList`, `HashTable<TupleList>`, the old
    `build_nested` call);
  - `kermit-ds/src/ds/hash_trie/parallel.rs` (`TupleList`, `child`'s and `build_nested`'s new
    `links` argument, `push_in_run`, the test closures of `from_tuples_partitioned`, the
    `assert_equivalent_root` call);
  - `kermit-ds/src/ds/hash_trie/implementation.rs`, at the three fill calls of Step 7k only
    (E0061: the `links` argument C05b's signatures add).

  rustc may stop before type-checking some bodies, so fewer locations is fine. An error anywhere
  else (`bulk.rs`, `expansion.rs`, `node.rs`, `identity.rs`, `hash_trie_iter.rs`, `chain.rs`, or
  `implementation.rs` outside the fill calls) is a C05a mistake: fix it before starting C05b. If C05b's
  first build shows more errors in C05a's files, those were hidden here, and C05b fixes them as
  part of Task 5.

  **C05a ends here, without a commit.** Left for C05b, which the same implementer runs next:
  `kermit-ds/src/ds/hash_trie/radix.rs` and `kermit-ds/src/ds/hash_trie/parallel.rs`, their
  tests, then the full kermit-ds test run, clippy, fmt, doc, Task 5's one commit (staging every
  file C05a touched as well) and mutation checks 2, 4 and 5.

---

#### Chunk C05b: `radix.rs` and `parallel.rs` thread their lists through the links; run, lint, commit, mutants (Steps 14–29)

C05a left the tree not compiling: `bulk.rs`, `implementation.rs`, `identity.rs`,
`expansion.rs`, `node.rs` and `hash_trie_iter.rs` speak `Chain` and `&ChainLinks`, and
`TupleList` is gone, but `radix.rs` and `parallel.rs` still name `TupleList` and call
`build_nested`, `child` and the identity helpers without links. This chunk finishes the switch
in those two files, adds the cross-partition re-threading pin for `presized:N`, runs everything,
and makes Task 5's one commit. The same implementer ran C05a.

**State assumed (T1–T4 and C05a):**
- T2: `kermit_iters::ChainLinks` (`new`, `get`, `NIL`, …), `Sync` (a `Vec<AtomicU32>`).
- T4: `kermit-ds/src/ds/hash_trie/chain.rs`: `Chain::{start, push, walk, len, head, leaf_rows}`
  and `chain::push_row(lists, hash, load_factor, links, row) -> bool`, each item still under a
  `#[allow(dead_code)]` with the comment "No caller until the lists become chains (Task 5)."
- C05a:
  - `HashTrie::links(&self) -> &ChainLinks` (`pub(crate)`);
  - `HashTrie::build_nested(tuples, links, depth, arity, lists: HashTable<Chain>, config)` and
    `HashTrie::child(tuples, links, depth, arity, chain: Chain, config)` (`bulk.rs`);
  - `from_tuples_partitioned`'s fill is
    `FnOnce(Tuples, &ChainLinks, u32, usize) -> (Tuples, HashTrieNode<P, E>)`, called with
    `ChainLinks::new(tuples.len())`;
  - `HashTrieNode::Leaf(HashTable<Chain>)`;
  - `identity::assert_same_node(a_links, a, b_links, b, path)`,
    `identity::assert_equivalent_root(a_links, a, b_links, b, label)`,
    `identity::Rows::{new, of}` and
    `identity::assert_same_node_contents(a: Rows, x, b: Rows, y, path)`, all `pub(super)`;
    `assert_same_trie`, `assert_same_contents` and `assert_equivalent_trie` keep their
    `(trie, trie, label)` signatures.
- T1: `radix.rs` holds `Entries` (with `take_from`, `into_children`, `into_chains`),
  `fill_root`, `partition_by_first_hash`, `build_and_merge`, `build_scratch_root`,
  `take_in_arrival_order` and `merge_in_first_appearance_order`; `parallel.rs`'s
  `fill_root_in_morsels` calls `radix::partition_by_first_hash` and `radix::build_and_merge`;
  `insert_in_first_appearance_order` is deleted. Worker runs: `radix:K` `[1, 1, 1]`,
  `parallel:N` `[N, N, N]`, `presized:3` `[3, 3, 3, 3]`. The "Before" blocks below that say
  "post-T1" are that shape as the contract fixes it; every edit replaces a whole item, so locate
  each by its name, not by byte.

The "Before (post-T1)" blocks below are Task 1's items with its comments trimmed; Task 1's Steps 3–4 hold the exact text. Every edit replaces a whole item, so locate each by its name and replace it whole.

Line numbers are at HEAD (0e94f30); T1 moves `Entries` and `merge_in_first_appearance_order`
from `parallel.rs` (HEAD 108–152, 252–283) into `radix.rs`, so `parallel.rs`'s later items sit
about 80 lines higher and `radix.rs`'s about 60 lines lower after T1.

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/radix.rs`
  - module doc (HEAD 1–26) and the non-test `use` (28–41);
  - `Entries` (moved here by T1): its doc and its two `TupleList` types;
  - `fill_root` (HEAD 47–114, rewritten by T1), `build_and_merge` (T1),
    `build_scratch_root` (HEAD 116–144);
  - test `check_contents`' doc (HEAD 210–216).
- Modify: `kermit-ds/src/ds/hash_trie/parallel.rs`
  - module doc (HEAD 1–60) and the non-test `use` (62–76);
  - `fill_root` and `fill_root_in_morsels` (HEAD 174–250, rewritten by T1);
  - `fill_presized_root` (302–323), `fill_presized_root_in` (325–406), `push_in_run` (408–421);
  - tests: the `use` (452–471), `parallel_in_small_morsels` (483–506), `check_identity`'s doc
    (508–520), `check_one_run` (790–837), `presized_in_small_regions` (1076–1098), and one new
    pin after `keys_that_cannot_fit_their_region_go_to_the_tail` (1262–1284).
- Modify (verify, or write): `kermit-ds/src/ds/hash_trie/implementation.rs`, the three
  partitioning arms of `from_tuples_with_config_and_build_mode` (HEAD 516–547).
- Modify: `kermit-ds/src/ds/hash_trie/chain.rs` (T4's `#[allow(dead_code)]` markers go).

**Tests that pin behaviour and stay unchanged** (each still holds, and each compiles as is):
- `radix.rs`: `radix_builds_the_bulk_trie_under_{siphash,fxhash,colliding_hashes}`,
  `radix_build_rejects_a_wrong_arity`, `build_mode_relation_uses_the_default_config` (its
  `assert_same_trie` now also compares two `bulk` tries' links, which are equal) and
  `radix_keeps_the_partitioned_copy_as_its_buffer` (its own partition closure stays, per T1).
  They build through the public constructors and compare through `assert_same_contents` /
  `assert_same_trie`, whose signatures did not change.
- `parallel.rs`: `parallel_builds_the_bulk_trie_under_*`, `partition_bits_*`,
  `parallel_stores_its_partitioned_copy`, `parallel_builds_the_bulk_trie_on_large_and_skewed_inputs`,
  `build_modes_reach_their_builds` (T1's `[1, 1, 1]` radix row: T5 adds no worker run),
  `parallel_build_of_nothing_starts_no_worker` (`assert_same_trie` over two empty tries, empty
  links), `parallel_build_rejects_a_wrong_arity`, `one_run_of_grouping_builds_the_bulk_root`
  (its helper changes, Step 19), every `presized_*` test, `keys_that_cannot_fit_their_region_go_to_the_tail`,
  `presized_build_reaches_its_own_path` (`[3, 3, 3, 3]`), `parallel_build_merges_under_every_root_capacity`,
  `presized_build_requires_a_presized_root`. Every mode still threads one link per row of its
  buffer, so heap sizes stay equal across modes, and every content comparison now walks each
  chain through its own trie's links.

- [ ] **Step 14: Check the state C05b builds on.**

  Host check first: no timing run may be active.
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l   # prints 0
  rg -n 'TupleList' kermit-ds/src
  rg -n 'fn (fill_root|fill_root_in_morsels|partition_by_first_hash|build_and_merge|build_scratch_root|take_in_arrival_order|merge_in_first_appearance_order)\b|enum Entries' \
    kermit-ds/src/ds/hash_trie/radix.rs kermit-ds/src/ds/hash_trie/parallel.rs
  rg -n 'pub\(crate\) fn links|fn from_tuples_partitioned|fill: impl FnOnce' kermit-ds/src/ds/hash_trie/implementation.rs
  rg -n 'pub\(super\) (fn|struct) (assert_same_node|assert_equivalent_root|assert_same_node_contents|Rows)\b' \
    kermit-ds/src/ds/hash_trie/identity.rs
  ```
  Expected:
  - `TupleList`: hits in `radix.rs` and `parallel.rs` only (C05a deleted the alias and every
    other use).
  - The second grep: `radix.rs` holds `enum Entries`, `fill_root`, `partition_by_first_hash`,
    `build_and_merge`, `build_scratch_root`, `take_in_arrival_order` and
    `merge_in_first_appearance_order`; `parallel.rs` holds only `fill_root` and
    `fill_root_in_morsels`.
  - `links` and `from_tuples_partitioned` exist, the fill bound reads
    `fill: impl FnOnce(Tuples, &ChainLinks, u32, usize) -> (Tuples, HashTrieNode<P, E>)`.
  - The four identity helpers exist.

  If any differs, stop: an earlier task or chunk did not land as the contract says.

- [ ] **Step 15: `radix.rs`, the non-test code.**

  All edits are in `kermit-ds/src/ds/hash_trie/radix.rs`.

  **The module doc.** Replace every `//!` line at the top of the file (HEAD 1–26; T1 may have
  reworded it) with:
  ```rust
  //! The `radix:K` build of a [`HashTrie`](super::HashTrie): copy the rows
  //! into `2^K` partitions on their first attribute's hash
  //! ([`partition_by_first_hash`]), group each partition into a scratch root
  //! and build its children by Algorithm 2 (`bulk.rs`), then merge the scratch
  //! roots into the real one ([`build_and_merge`]; SIGMOD 2020 §3.3.2; issues
  //! #91, #101). `parallel:N` (`parallel.rs`) runs the same steps on N
  //! threads; `radix:K` runs them on the calling thread, with the whole batch
  //! as one morsel.
  //!
  //! The copy is the paper's materialisation step: "the tuples are
  //! partitioned based on the hash values of the first join key attribute"
  //! (§3.3.2). It becomes the trie's buffer and the input is freed, so each
  //! partition's rows are one contiguous range of the buffer, in input order,
  //! and every row a partition's build reads is local. One round of `2^K`
  //! partitions is kermit's choice; the paper adapts a two-pass radix
  //! partitioning (Balkesen et al.).
  //!
  //! Every list is a [`Chain`] threaded through the trie's links, one per row
  //! of the copy ([`ChainLinks`]): the paper's chain pointer (§3.3.2, Figure
  //! 3), kept as a 4-byte id in an array beside the buffer rather than 8 bytes
  //! inside each tuple (kermit's). A partition's scratch root threads the
  //! partition's rows into its root chains, and Algorithm 2 re-threads each
  //! chain into the next level's, so building a partition writes the links of
  //! its own rows and of no others: partitions build on separate threads with
  //! one writer per link. The merge moves `Chain` values and finished
  //! children, whose ids name rows of the copy, and writes no link.
  //!
  //! The result is equivalent to the bulk build's trie (Amendment 2): the
  //! same tables bucket for bucket and capacity for capacity, and the same
  //! tuples in every chain, singleton and pending chain, under the copy's row
  //! ids. A table's final layout depends only on the order in which its
  //! *new* keys arrive — `HashTable::entry_or_insert_with` returns an
  //! existing entry before its resize check — so the merge inserts the
  //! distinct root keys in the order they first appear in the input, as the
  //! bulk build does. The copy records each row's input position (`origin`),
  //! each key is tagged with the position of the row that introduced it, and
  //! [`merge_in_first_appearance_order`] merges the partitions' tagged lists
  //! k-way on that tag. Below the root nothing needs care: the copy is
  //! stable, so each root key's child is built from the same tuples, in input
  //! order, as in the bulk build.
  ```

  **The non-test `use`** (HEAD 28–41; T1 added `dispatch`, `Partitioned`, `Reverse` and
  `BinaryHeap` for the moved helpers). Replace the whole `use {…};` block with:
  ```rust
  use {
      super::{
          build_mode::RadixBits,
          chain::{self, Chain},
          config::{HashTrieConfig, LoadFactor},
          expansion::ExpansionPolicy,
          hash_table::HashTable,
          implementation::HashTrie,
          node::HashTrieNode,
          pruning::PruningPolicy,
      },
      crate::morsel::{dispatch, partition_rows, Partitioned, Threads},
      kermit_iters::{ChainLinks, HashStrategy, RowId, Tuples},
      std::{cmp::Reverse, collections::BinaryHeap},
  };
  ```
  `bulk::TupleList` goes (C05a deleted it); `chain` (for `chain::push_row`), `Chain` and
  `ChainLinks` come in.

  **`Entries`** (moved here by T1). Replace its doc comment, which T1 carried over from
  `parallel.rs` HEAD 108–112, with:
  ```rust
  /// One partition's root entries, each tagged with the input position at
  /// which its key first appeared, in that order. The root is `Inner` for
  /// arity ≥ 2, so its values are child nodes (subtries, or under pruning and
  /// lazy expansion their `Singleton`s and pending chains), and `Leaf` for
  /// arity 1, so its values are chains. Either way every id in them names a
  /// row of the copy, so they move into the real root as they are.
  ```
  Then replace the type `Vec<Arrival<TupleList>>` with `Vec<Arrival<Chain>>` at both of its
  occurrences in `radix.rs`: the variant (post-T1: `    Chains(Vec<Arrival<TupleList>>),`) and
  `into_chains`' return type (post-T1: `    fn into_chains(self) -> Vec<Arrival<TupleList>> {`,
  or `pub(super) fn` if T1 widened it; keep its visibility). After:
  ```rust
      Chains(Vec<Arrival<Chain>>),
  ```
  ```rust
      fn into_chains(self) -> Vec<Arrival<Chain>> {
  ```
  `take_from`, `into_children` and the rest of `into_chains` are unchanged: they move values.

  **`fill_root`.** Before (post-T1):
  ```rust
  /// Fills the empty `root` with the rows of `tuples` by the `radix:bits`
  /// build and returns the trie's buffer: the rows copied into partition
  /// order. Every row must have `arity` attributes; the caller checks.
  pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: Tuples, root: &mut HashTrieNode<P, E>, arity: usize, bits: RadixBits,
      config: HashTrieConfig,
  ) -> Tuples {
      if tuples.is_empty() {
          return tuples;
      }
      let one = Threads::new(1).expect("one thread is a valid count");
      let partitioned =
          partition_by_first_hash::<H>(one, &tuples, tuples.len(), u32::from(bits.get()));
      drop(tuples);
      build_and_merge::<H, P, E>(partitioned, root, arity, one, config)
  }
  ```
  After (the whole item, doc included):
  ```rust
  /// Fills the empty `root` with the rows of `tuples` by the `radix:bits`
  /// build, threading every list through `links` (one link per row of
  /// `tuples`), and returns the trie's buffer: the rows copied into partition
  /// order. Every row must have `arity` attributes; the caller checks.
  pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: Tuples, links: &ChainLinks, root: &mut HashTrieNode<P, E>, arity: usize,
      bits: RadixBits, config: HashTrieConfig,
  ) -> Tuples {
      if tuples.is_empty() {
          // The bulk build of nothing is the empty root over the empty batch:
          // copy nothing and start no worker, as `parallel:N` does.
          return tuples;
      }
      // 1. Partition, on this thread: copy the rows into `2^bits` partitions on
      //    the top `bits` of their first attribute's hash. Each partition is one
      //    range of the copy, in input order. The whole batch is one morsel:
      //    `partition_rows` keeps a count and a range per (partition, morsel)
      //    pair, which one thread gains nothing from, and at `radix:16` over 10^7
      //    rows in `MORSEL_TUPLES`-row morsels would be about 40 million pairs.
      let one = Threads::new(1).expect("one thread is a valid count");
      let one_morsel = tuples.len();
      let partitioned =
          partition_by_first_hash::<H>(one, &tuples, one_morsel, u32::from(bits.get()));
      // The copy is the buffer from here on: free the input before the build,
      // so the build holds the rows once.
      drop(tuples);
      // 2 and 3, on this thread.
      build_and_merge::<H, P, E>(partitioned, links, root, arity, one, config)
  }
  ```
  If T1's body differs in comments only, the text above still replaces it: the calls are the
  contract's.

  **`build_and_merge`.** Before (post-T1):
  ```rust
  pub(super) fn build_and_merge<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      partitioned: Partitioned, root: &mut HashTrieNode<P, E>, arity: usize, threads: Threads,
      config: HashTrieConfig,
  ) -> Tuples {
      let ranges: Vec<_> = (0..partitioned.partitions())
          .map(|p| partitioned.ids(p))
          .filter(|ids| !ids.is_empty())
          .collect();
      let (rows, origin) = (&partitioned.tuples, &partitioned.origin);
      let entries = dispatch(threads, ranges, |ids| {
          let (scratch, first_seen) =
              build_scratch_root::<H, P, E>(rows, ids, origin, arity, config);
          Entries::take_from(scratch, &first_seen)
      });
      drop(partitioned.origin);
      drop(partitioned.offsets);
      match root {
          | HashTrieNode::Inner(table) => {
              let lists = entries.into_iter().map(Entries::into_children).collect();
              merge_in_first_appearance_order(table, lists, config.load_factor);
          },
          | HashTrieNode::Leaf(table) => {
              let lists = entries.into_iter().map(Entries::into_chains).collect();
              merge_in_first_appearance_order(table, lists, config.load_factor);
          },
          | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
              unreachable!("a root is never pruned or unexpanded")
          },
      }
      partitioned.tuples
  }
  ```
  After (the whole item, with its doc comment replacing T1's):
  ```rust
  /// Steps 2 and 3 of `radix:K` and `parallel:N`. Each non-empty partition
  /// of `partitioned` is built into a scratch root ([`build_scratch_root`])
  /// on one of `threads` workers, which threads that partition's rows, and
  /// only those, through `links`; then the calling thread merges the scratch
  /// roots' entries into the empty `root` in first-appearance order
  /// ([`merge_in_first_appearance_order`]), writing no link. Returns the
  /// copy, the trie's buffer: every id in `root` names a row of it, and
  /// `links` holds one link per row of it.
  pub(super) fn build_and_merge<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      partitioned: Partitioned, links: &ChainLinks, root: &mut HashTrieNode<P, E>, arity: usize,
      threads: Threads, config: HashTrieConfig,
  ) -> Tuples {
      // An empty partition would build and drop an empty scratch root.
      let ranges: Vec<_> = (0..partitioned.partitions())
          .map(|p| partitioned.ids(p))
          .filter(|ids| !ids.is_empty())
          .collect();
      // 2. Build: each partition's id range into a scratch root, whose entries
      //    then leave in the order their keys arrived, each tagged with its
      //    key's first input position (`origin`), not its id. A partition's
      //    rows are in exactly one of these builds, so each link has one
      //    writer, and the relaxed atomics need no lock between the workers.
      let (rows, origin) = (&partitioned.tuples, &partitioned.origin);
      let entries = dispatch(threads, ranges, |ids| {
          let (scratch, first_seen) =
              build_scratch_root::<H, P, E>(rows, links, ids, origin, arity, config);
          Entries::take_from(scratch, &first_seen)
      });
      // Every key's first input position is in its entry now, so nothing reads
      // `origin` or `offsets` again: free them before the merge.
      drop(partitioned.origin);
      drop(partitioned.offsets);
      // 3. Merge, on this thread, in first-appearance order. The workers were
      //    joined inside `dispatch`, so every link they wrote is visible here.
      match root {
          | HashTrieNode::Inner(table) => {
              let lists = entries.into_iter().map(Entries::into_children).collect();
              merge_in_first_appearance_order(table, lists, config.load_factor);
          },
          | HashTrieNode::Leaf(table) => {
              let lists = entries.into_iter().map(Entries::into_chains).collect();
              merge_in_first_appearance_order(table, lists, config.load_factor);
          },
          | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
              unreachable!("a root is never pruned or unexpanded")
          },
      }
      partitioned.tuples
  }
  ```
  The closure captures `links: &ChainLinks`, which is `Send` because `ChainLinks` is `Sync`
  (`dispatch`'s task is `Fn + Sync`).

  **`build_scratch_root`.** Before (HEAD 116–144; T1 left it unchanged):
  ```rust
  /// Builds one partition, the rows `ids` of `tuples`, into a scratch root of
  /// the real root's kind by Algorithm 2 (`bulk.rs`): the ids grouped at the
  /// root, recording for each key the input position of the row that
  /// introduced it (`origin[id]`) and the key's hash, in arrival order; then
  /// each key's child built from its list, as the bulk build builds it.
  /// `ids` must list the partition in input order.
  pub(super) fn build_scratch_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: &Tuples, ids: impl IntoIterator<Item = RowId>, origin: &[RowId], arity: usize,
      config: HashTrieConfig,
  ) -> (HashTrieNode<P, E>, Vec<(RowId, u64)>) {
      // Lines 3–7 at the root. The scratch root starts at 4 buckets and
      // grows: its entries move into the real root, so its size is never
      // seen.
      let mut lists: HashTable<TupleList> = HashTable::new();
      let mut first_seen = Vec::new();
      for row in ids {
          let hash = H::hash(tuples.row(row)[0]);
          let keys_before = lists.len();
          lists
              .entry_or_insert_with(hash, config.load_factor, Vec::new)
              .push(row);
          if lists.len() > keys_before {
              first_seen.push((origin[row as usize], hash));
          }
      }
      // Lines 8–15.
      let scratch = HashTrie::<H, P, E>::build_nested(tuples, 0, arity, lists, config);
      (scratch, first_seen)
  }
  ```
  After:
  ```rust
  /// Builds one partition, the rows `ids` of `tuples`, into a scratch root of
  /// the real root's kind by Algorithm 2 (`bulk.rs`): each row pushed onto
  /// the chain in its key's bucket, threaded through `links` (lines 3–7),
  /// recording for each key the input position of the row that introduced it
  /// (`origin[id]`) and the key's hash, in arrival order; then each key's
  /// child built from its chain, as the bulk build builds it (lines 8–15).
  /// `ids` must list the partition in input order. Only the links of `ids`'
  /// rows are written, so partitions can build on separate threads.
  pub(super) fn build_scratch_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: &Tuples, links: &ChainLinks, ids: impl IntoIterator<Item = RowId>,
      origin: &[RowId], arity: usize, config: HashTrieConfig,
  ) -> (HashTrieNode<P, E>, Vec<(RowId, u64)>) {
      // Lines 3–7 at the root. The scratch root starts at 4 buckets and
      // grows: its entries move into the real root, so its size is never
      // seen. `push_row` starts a chain exactly when the row's key is new.
      let mut lists: HashTable<Chain> = HashTable::new();
      let mut first_seen = Vec::new();
      for row in ids {
          let hash = H::hash(tuples.row(row)[0]);
          if chain::push_row(&mut lists, hash, config.load_factor, links, row) {
              first_seen.push((origin[row as usize], hash));
          }
      }
      // Lines 8–15: each chain re-threaded into its child's chains.
      let scratch = HashTrie::<H, P, E>::build_nested(tuples, links, 0, arity, lists, config);
      (scratch, first_seen)
  }
  ```

  `partition_by_first_hash`, `take_in_arrival_order` and `merge_in_first_appearance_order` are
  unchanged: the first never sees a link, and the other two move values of any type.

- [ ] **Step 16: `radix.rs`, the tests.**

  `check_contents`' doc (HEAD 210–216). Before:
  ```rust
      /// `radix:K` builds a trie equivalent to `bulk`'s, for every arity, load
      /// factor, root capacity, child capacity, bit count and input.
      /// `assert_same_contents` compares every table bucket for bucket, so the
      /// root is `bulk`'s (Decision 3 of the partitioned-materialisation
      /// design), and every chain, singleton and pending list holds the same
      /// tuples in the same order under the copy's row ids. The heap size is
      /// compared too: `space` must not move with the mode.
  ```
  After:
  ```rust
      /// `radix:K` builds a trie equivalent to `bulk`'s, for every arity, load
      /// factor, root capacity, child capacity, bit count and input.
      /// `assert_same_contents` compares every table bucket for bucket, so the
      /// root is `bulk`'s (Decision 3 of the partitioned-materialisation
      /// design), and every chain, singleton and pending chain holds the same
      /// tuples in the same order under the copy's row ids, each chain walked
      /// through its own trie's links. The heap size is compared too: `space`
      /// must not move with the mode, and every mode keeps one link per row.
  ```

  Every other HEAD test in `radix.rs` compiles and holds unchanged (see the preamble). T1 may
  have added tests; check whether any calls a function whose signature this chunk changed:
  ```bash
  rg -n 'build_scratch_root|build_and_merge|fill_root\(' kermit-ds/src/ds/hash_trie/radix.rs
  ```
  Expected: hits in the non-test code only (definitions, the two calls in `fill_root` and
  `build_and_merge`, and doc links). For any hit inside `mod tests`, insert the links of the
  batch that call builds over as the new argument, exactly where the signatures above put it:
  - `build_scratch_root(rows, ids, …)` becomes `build_scratch_root(rows, &links, ids, …)`;
  - `build_and_merge(partitioned, root, …)` becomes `build_and_merge(partitioned, &links, root, …)`;
  - `fill_root(tuples, root, …)` becomes `fill_root(tuples, &links, root, …)`;

  with `let links = ChainLinks::new(<that batch>.len());` on the line before the call, where
  `<that batch>` is the `Tuples` the call's ids name (`rows`, `partitioned.tuples`, or the
  `tuples` passed in). `ChainLinks` reaches the test module through `super::*`.

- [ ] **Step 17: `parallel.rs`, the non-test code.**

  All edits are in `kermit-ds/src/ds/hash_trie/parallel.rs`.

  **The module doc.** Replace every `//!` line at the top of the file (HEAD 1–60; T1 may have
  reworded steps 1–3) with:
  ```rust
  //! The `parallel:N` and `presized:N` builds of a [`HashTrie`](super::HashTrie):
  //! `parallel:N` is the radix build's three steps (`radix.rs`), with the
  //! partition and build steps run on N threads by the morsel-driven helpers
  //! of `crate::morsel` (Leis et al., SIGMOD 2014; SIGMOD 2020 §3.3.2; issue
  //! #94).
  //!
  //! 1. **Partition.** `radix::partition_by_first_hash` copies every row
  //!    ([`partition_rows`]) into one of P partitions by the top log₂P bits of
  //!    the row's first attribute's hash, and records each copied row's input
  //!    position (`origin`); P is four per thread, rounded up to a power of
  //!    two. The copy is the paper's materialisation step (§3.3.2, #101): it
  //!    becomes the trie's buffer, the input batch is freed, and each
  //!    partition's rows are a contiguous range of it, so every later read of a
  //!    partition's rows is local. One round of partitioning is kermit's
  //!    choice; the paper adapts a two-pass radix partitioning (Balkesen et
  //!    al.).
  //! 2. **Build.** `radix::build_and_merge` [`dispatch`]es each non-empty
  //!    partition's id range to a worker, which threads the partition's rows
  //!    into the chains of a scratch root and builds its children, as `bulk`
  //!    does (`radix::build_scratch_root`), then takes the scratch root's
  //!    entries out, each tagged with the input position at which its key
  //!    first appeared.
  //! 3. **Merge.** The calling thread inserts every entry into the real root in
  //!    first-appearance order, by a k-way merge of the partitions' lists.
  //!
  //! The result holds the bulk trie's contents, and its root is the bulk
  //! build's, bucket for bucket and capacity for capacity, for the reasons
  //! the radix build's is.
  //! - **Children.** A partition keeps its rows in input order (`partition_rows`
  //!   is stable), so each child is built from the tuples `bulk` builds it from,
  //!   in the same order, under the copy's row ids.
  //! - **Root.** The root receives its new keys in the bulk order, because the
  //!   merge orders them by `origin`, not by id.
  //! - **Collisions.** Equal 64-bit hashes share a root entry and also a
  //!   partition, because the partition is a function of the hash.
  //!
  //! Only the buffer's row order, and so the ids, differ, so the trie is
  //! equivalent to `bulk`'s rather than array-identical (Amendment 2).
  //!
  //! `presized:N`
  //! (`docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`;
  //! spelled `parallel:N` under `root-capacity=tuples` until 2026-10-07;
  //! [`fill_presized_root`]) follows the paper where the paper describes it:
  //! - the input is partitioned by its first attribute's hash as it is copied
  //!   (§3.3.2);
  //! - Algorithm 2 groups it into a root sized before any tuple arrives (line 3;
  //!   `root-capacity=tuples`, #88) and builds each bucket's child.
  //!
  //! How this runs on threads is kermit's:
  //! 1. Each partition is a contiguous run of the root's fixed-size regions, its
  //!    rows a contiguous range of the copy. One worker threads it into the
  //!    chains of its run of a presized table.
  //! 2. A row whose probe would cross its region's end is deferred to the calling
  //!    thread, which pushes the deferred rows in input order (by `origin`).
  //! 3. Each worker builds the children of one run's buckets at a time,
  //!    re-threading every row of their chains, a deferred key's rows included,
  //!    which may live in another partition's range of the copy.
  //!
  //! Root keys may then sit in other buckets than the bulk build's, so this
  //! trie is equivalent rather than identical (Amendment 2): the root has the
  //! same capacity, occupied buckets and total displacement, and every subtrie
  //! holds the same tuples in the same order. Regions, not threads, decide
  //! which tuples are deferred, and the tail goes in input order, so the
  //! trie's contents are the same for every N, bucket for bucket. Only its
  //! buffer, each N's partitioned copy, differs.
  //!
  //! **Links.** Every list in both builds is a [`Chain`] threaded through the
  //! trie's one links array, a link per row of the copy ([`ChainLinks`]; the
  //! paper's chain pointer, §3.3.2, Figure 3), and workers write it
  //! concurrently. Every row is in exactly one list at a time, so each link
  //! has exactly one writer at a time:
  //! - under `parallel:N`, the worker building the row's partition;
  //! - under `presized:N`, the worker filling the row's run in step 1, the
  //!   calling thread if step 2 deferred the row, then in step 3 the worker of
  //!   the run whose bucket holds the row's chain. For a deferred key that can
  //!   be another partition's run: its rows stay in their home partition's
  //!   range of the copy, but the tail may place its chain in the next run's
  //!   bucket, whose worker re-threads them.
  //!
  //! The links are relaxed atomics, so these writes are race-free without
  //! proving which partition owns which links, and the scoped spawn and join
  //! of every worker run (`crate::morsel`) order one step's writes before the
  //! next step's reads. No lock and no `split_at_mut` are needed. Atomics,
  //! rather than a plain array split by partition, are kermit's choice (#101
  //! B2, Decision 3): lazy expansion re-threads a child's rows through a
  //! shared reference.
  ```

  **The non-test `use`** (HEAD 62–76, as T1 left it). Replace the whole `use {…};` block with:
  ```rust
  use {
      super::{
          chain::{self, Chain},
          config::HashTrieConfig,
          expansion::ExpansionPolicy,
          hash_table::{home_bucket, BucketRun, HashTable, Overflow, RunEntry},
          implementation::HashTrie,
          node::HashTrieNode,
          pruning::PruningPolicy,
          radix,
      },
      crate::morsel::{dispatch, partition_rows, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD},
      kermit_iters::{ChainLinks, HashStrategy, RowId, Tuples},
      std::ops::Range,
  };
  ```
  - `bulk::TupleList` goes; `chain`, `Chain` and `ChainLinks` come in.
  - `LoadFactor`, `radix::Arrival`, `Reverse` and `BinaryHeap` have no non-test use here after
    T1 (the merge lives in `radix.rs`). If T1 kept any of them here, this block drops it. The
    tests' `tuples_config` names `LoadFactor`; Step 19 imports it in the test module.

  **`fill_root` and `fill_root_in_morsels`.** Before (post-T1):
  ```rust
  /// Fills the empty `root` with the rows of `tuples` by the
  /// `parallel:threads` build, and returns the partitioned copy of `tuples`
  /// that the trie keeps as its buffer: every id in `root` names a row of
  /// it. `tuples` itself is freed once copied. Every row must have `arity`
  /// attributes; the caller checks.
  pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: Tuples, root: &mut HashTrieNode<P, E>, arity: usize, threads: Threads,
      config: HashTrieConfig,
  ) -> Tuples {
      fill_root_in_morsels::<H, P, E>(tuples, root, arity, threads, MORSEL_TUPLES, config)
  }

  /// [`fill_root`] with the morsel size as a parameter, so tests can cut a
  /// small input into many morsels.
  fn fill_root_in_morsels<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: Tuples, root: &mut HashTrieNode<P, E>, arity: usize, threads: Threads,
      morsel_tuples: usize, config: HashTrieConfig,
  ) -> Tuples {
      if tuples.is_empty() {
          return tuples;
      }
      let partitioned = radix::partition_by_first_hash::<H>(
          threads,
          &tuples,
          morsel_tuples,
          partition_bits(threads),
      );
      drop(tuples);
      #[cfg(any(test, feature = "test-hooks"))]
      PARALLEL_BUILDS.with(|builds| {
          builds.borrow_mut().push(ParallelBuild {
              threads: threads.get(),
              partition_sizes: partition_sizes(&partitioned),
              deferred: None,
          });
      });
      radix::build_and_merge::<H, P, E>(partitioned, root, arity, threads, config)
  }
  ```
  After (both items, docs included):
  ```rust
  /// Fills the empty `root` with the rows of `tuples` by the
  /// `parallel:threads` build, threading every list through `links` (one link
  /// per row of `tuples`), and returns the partitioned copy of `tuples` that
  /// the trie keeps as its buffer: every id in `root` names a row of it.
  /// `tuples` itself is freed once copied. Every row must have `arity`
  /// attributes; the caller checks.
  pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: Tuples, links: &ChainLinks, root: &mut HashTrieNode<P, E>, arity: usize,
      threads: Threads, config: HashTrieConfig,
  ) -> Tuples {
      fill_root_in_morsels::<H, P, E>(tuples, links, root, arity, threads, MORSEL_TUPLES, config)
  }

  /// [`fill_root`] with the morsel size as a parameter, so tests can cut a
  /// small input into many morsels.
  fn fill_root_in_morsels<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: Tuples, links: &ChainLinks, root: &mut HashTrieNode<P, E>, arity: usize,
      threads: Threads, morsel_tuples: usize, config: HashTrieConfig,
  ) -> Tuples {
      if tuples.is_empty() {
          // The bulk build of nothing is the empty root over the empty batch:
          // copy nothing and start no worker.
          return tuples;
      }
      // 1. Partition, on `threads` workers, by the top bits of each row's first
      //    attribute's hash. The rows are copied into partition order, each
      //    partition's in input order, with each copy's input position in
      //    `origin`. The copy is the trie's buffer from here on, so the input is
      //    freed before the build steps.
      let partitioned = radix::partition_by_first_hash::<H>(
          threads,
          &tuples,
          morsel_tuples,
          partition_bits(threads),
      );
      drop(tuples);
      #[cfg(any(test, feature = "test-hooks"))]
      PARALLEL_BUILDS.with(|builds| {
          builds.borrow_mut().push(ParallelBuild {
              threads: threads.get(),
              partition_sizes: partition_sizes(&partitioned),
              deferred: None,
          });
      });
      // 2 and 3: each partition into a scratch root on `threads` workers, each
      //    threading only its own partition's rows through `links`, then the
      //    merge on this thread.
      radix::build_and_merge::<H, P, E>(partitioned, links, root, arity, threads, config)
  }
  ```
  Seven parameters for `fill_root_in_morsels`: at clippy's `too_many_arguments` limit, not over.

  **`fill_presized_root`.** Before (HEAD 302–323):
  ```rust
  /// Builds the root of `2^log2_capacity` buckets over the rows of `tuples` by
  /// `presized:threads`, and returns it with the partitioned copy of `tuples`
  /// that the trie keeps as its buffer: every id in the root names a row of
  /// it. `tuples` itself is freed once copied. This is the partitioned build
  /// of a presized root: the paper's partitioning and Algorithm 2, with
  /// kermit's regions, tail and per-run children
  /// (`docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`).
  /// Every child is built by Algorithm 2 (#107). Every row must have `arity`
  /// attributes; the caller checks.
  pub(super) fn fill_presized_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: Tuples, log2_capacity: u32, arity: usize, threads: Threads, config: HashTrieConfig,
  ) -> (Tuples, HashTrieNode<P, E>) {
      fill_presized_root_in::<H, P, E>(
          tuples,
          log2_capacity,
          arity,
          threads,
          MORSEL_TUPLES,
          REGION_BUCKETS,
          config,
      )
  }
  ```
  After:
  ```rust
  /// Builds the root of `2^log2_capacity` buckets over the rows of `tuples` by
  /// `presized:threads`, threading every list through `links` (one link per
  /// row of `tuples`), and returns it with the partitioned copy of `tuples`
  /// that the trie keeps as its buffer: every id in the root names a row of
  /// it. `tuples` itself is freed once copied. This is the partitioned build
  /// of a presized root: the paper's partitioning and Algorithm 2, with
  /// kermit's regions, tail and per-run children
  /// (`docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`).
  /// Every child is built by Algorithm 2 (#107). Every row must have `arity`
  /// attributes; the caller checks.
  pub(super) fn fill_presized_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: Tuples, links: &ChainLinks, log2_capacity: u32, arity: usize, threads: Threads,
      config: HashTrieConfig,
  ) -> (Tuples, HashTrieNode<P, E>) {
      fill_presized_root_in::<H, P, E>(
          tuples,
          links,
          log2_capacity,
          arity,
          threads,
          MORSEL_TUPLES,
          REGION_BUCKETS,
          config,
      )
  }
  ```

  **`fill_presized_root_in`.** Before (HEAD 325–406):
  ```rust
  /// [`fill_presized_root`] with the morsel and region sizes as parameters,
  /// so tests can cut a small input into many morsels and regions.
  ///
  /// 1. **Partition**: [`partition_rows`] copies the rows by the top bits of each
  ///    row's home bucket, so partition k is a contiguous run of regions. Its
  ///    rows are a contiguous range of the copy, in input order, with each row's
  ///    input position in `origin`. The copy becomes the buffer; the input is
  ///    freed.
  /// 2. **Group** (Algorithm 2, lines 4–7): each worker takes a partition's id
  ///    range with its run of a presized table of row-id lists, and pushes every
  ///    row's id onto the list in its bucket. Rows whose key's probe runs off its
  ///    region are deferred.
  /// 3. **Tail**: the calling thread pushes the deferred rows in input order
  ///    (ascending `origin`), with ordinary probing. The paper does not say how a
  ///    probe crossing a partition's end is handled; this is kermit's answer.
  /// 4. **Children** (lines 8–12): each worker builds the children of its run's
  ///    buckets, by the bulk build's `child`, reading its partition's rows, and
  ///    those of any deferred key the tail placed in its run from another
  ///    partition. The paper does not say how the recursion is spread over
  ///    threads; one run per worker is kermit's choice.
  fn fill_presized_root_in<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: Tuples, log2_capacity: u32, arity: usize, threads: Threads, morsel_tuples: usize,
      region_buckets: usize, config: HashTrieConfig,
  ) -> (Tuples, HashTrieNode<P, E>) {
      if tuples.is_empty() {
          let root = HashTrie::<H, P, E>::make_root_sized(arity, log2_capacity);
          return (tuples, root);
      }
      let parts = presized_parts(log2_capacity, threads, region_buckets);
      let shift = log2_capacity - parts.trailing_zeros();
      // 1. Partition by the home bucket's top bits: whole runs of regions. The
      //    copy is the trie's buffer from here on, so the input is freed.
      let partitioned = partition_rows(threads, &tuples, morsel_tuples, parts, |row| {
          home_bucket(H::hash(row[0]), log2_capacity) >> shift
      });
      drop(tuples);
      let rows = &partitioned.tuples;
      // 2. Group each run of regions in parallel, partition k into run k.
      let mut lists: HashTable<TupleList> = HashTable::with_log2_capacity(log2_capacity);
      let ranges = (0..partitioned.partitions())
          .map(|p| partitioned.ids(p))
          .collect();
      let mut deferred = fill_runs(&mut lists, threads, ranges, region_buckets, |run, row| {
          push_in_run::<H>(rows, run, row)
      });
      #[cfg(any(test, feature = "test-hooks"))]
      PARALLEL_BUILDS.with(|builds| {
          builds.borrow_mut().push(ParallelBuild {
              threads: threads.get(),
              partition_sizes: partition_sizes(&partitioned),
              deferred: Some(deferred.len()),
          });
      });
      // 3. The tail, in input order: by each row's input position, `origin`. Its
      //    new id would order the tail partition-major, and the partition count
      //    depends on N, so tail keys would take buckets that vary with N. The
      //    table is presized for every row, so it does not grow here.
      let origin = &partitioned.origin;
      deferred.sort_unstable_by_key(|&row| origin[row as usize]);
      for row in deferred {
          lists
              .entry_or_insert_with(H::hash(rows.row(row)[0]), config.load_factor, Vec::new)
              .push(row);
      }
      // Nothing reads `origin` or `offsets` past the tail: free them before the
      // children are built, the build's most allocation-heavy phase.
      drop(partitioned.origin);
      drop(partitioned.offsets);
      // 4. Every bucket's child, a run per worker. At arity 1 the lists are the
      //    chains, and the table of lists is the root. `arity <= 1`, as
      //    `build_nested` decides at depth 0 (`depth + 1 >= arity`).
      let root = if arity <= 1 {
          HashTrieNode::Leaf(lists)
      } else {
          HashTrieNode::Inner(lists.map_in_runs(parts, |runs| {
              dispatch(threads, runs, |run| {
                  run.map(|list| HashTrie::<H, P, E>::child(rows, 1, arity, list, config));
              });
          }))
      };
      (partitioned.tuples, root)
  }
  ```
  After:
  ```rust
  /// [`fill_presized_root`] with the morsel and region sizes as parameters,
  /// so tests can cut a small input into many morsels and regions.
  ///
  /// 1. **Partition**: [`partition_rows`] copies the rows by the top bits of each
  ///    row's home bucket, so partition k is a contiguous run of regions. Its
  ///    rows are a contiguous range of the copy, in input order, with each row's
  ///    input position in `origin`. The copy becomes the buffer; the input is
  ///    freed.
  /// 2. **Group** (Algorithm 2, lines 4–7): each worker takes a partition's id
  ///    range with its run of a presized table of chains, and pushes every row
  ///    onto the chain in its key's bucket ([`push_in_run`]), threading it
  ///    through `links`. Rows whose key's probe runs off its region are
  ///    deferred.
  /// 3. **Tail**: the calling thread pushes the deferred rows in input order
  ///    (ascending `origin`), with ordinary probing. The paper does not say how a
  ///    probe crossing a partition's end is handled; this is kermit's answer.
  /// 4. **Children** (lines 8–12): each worker builds the children of its run's
  ///    buckets, by the bulk build's `child`, re-threading each bucket's chain
  ///    into its child's chains. A run's chains hold its partition's rows, and
  ///    those of any deferred key the tail placed in its run from another
  ///    partition. The paper does not say how the recursion is spread over
  ///    threads; one run per worker is kermit's choice.
  ///
  /// Each link has one writer at a time (see the module doc): the worker of
  /// the row's run in step 2, this thread in step 3 if the row was deferred,
  /// and in step 4 the worker of the run whose bucket holds the row's chain,
  /// which for a deferred key may be another partition's run. Every row is in
  /// exactly one chain, and the scoped joins between the steps order them, so
  /// the relaxed atomics need no lock and no `split_at_mut`.
  // Eight parameters: `fill_presized_root`'s six and the two sizes tests set.
  #[allow(clippy::too_many_arguments)]
  fn fill_presized_root_in<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: Tuples, links: &ChainLinks, log2_capacity: u32, arity: usize, threads: Threads,
      morsel_tuples: usize, region_buckets: usize, config: HashTrieConfig,
  ) -> (Tuples, HashTrieNode<P, E>) {
      if tuples.is_empty() {
          let root = HashTrie::<H, P, E>::make_root_sized(arity, log2_capacity);
          return (tuples, root);
      }
      let parts = presized_parts(log2_capacity, threads, region_buckets);
      let shift = log2_capacity - parts.trailing_zeros();
      // 1. Partition by the home bucket's top bits: whole runs of regions. The
      //    copy is the trie's buffer from here on, so the input is freed.
      let partitioned = partition_rows(threads, &tuples, morsel_tuples, parts, |row| {
          home_bucket(H::hash(row[0]), log2_capacity) >> shift
      });
      drop(tuples);
      let rows = &partitioned.tuples;
      // 2. Group each run of regions in parallel, partition k into run k: each
      //    worker threads its own partition's rows into its run's chains.
      let mut lists: HashTable<Chain> = HashTable::with_log2_capacity(log2_capacity);
      let ranges = (0..partitioned.partitions())
          .map(|p| partitioned.ids(p))
          .collect();
      let mut deferred = fill_runs(&mut lists, threads, ranges, region_buckets, |run, row| {
          push_in_run::<H>(rows, links, run, row)
      });
      #[cfg(any(test, feature = "test-hooks"))]
      PARALLEL_BUILDS.with(|builds| {
          builds.borrow_mut().push(ParallelBuild {
              threads: threads.get(),
              partition_sizes: partition_sizes(&partitioned),
              deferred: Some(deferred.len()),
          });
      });
      // 3. The tail, in input order: by each row's input position, `origin`. Its
      //    new id would order the tail partition-major, and the partition count
      //    depends on N, so tail keys would take buckets that vary with N. The
      //    table is presized for every row, so it does not grow here. Each row
      //    goes onto its key's chain as `bulk`'s `group` puts it (line 7). A
      //    key's rows are deferred all together, so the tail starts every
      //    deferred key's chain, and writes only deferred rows' links.
      let origin = &partitioned.origin;
      deferred.sort_unstable_by_key(|&row| origin[row as usize]);
      for row in deferred {
          let hash = H::hash(rows.row(row)[0]);
          chain::push_row(&mut lists, hash, config.load_factor, links, row);
      }
      // Nothing reads `origin` or `offsets` past the tail: free them before the
      // children are built, the build's most allocation-heavy phase.
      drop(partitioned.origin);
      drop(partitioned.offsets);
      // 4. Every bucket's child, a run per worker, each chain re-threaded into
      //    its child's chains through the shared `links`. A run's chains may
      //    hold another partition's rows (a deferred key the tail placed here),
      //    but every row is in one chain, so one worker writes its link. At
      //    arity 1 the chains are the leaf, and the table of chains is the
      //    root. `arity <= 1`, as `build_nested` decides at depth 0
      //    (`depth + 1 >= arity`).
      let root = if arity <= 1 {
          HashTrieNode::Leaf(lists)
      } else {
          HashTrieNode::Inner(lists.map_in_runs(parts, |runs| {
              dispatch(threads, runs, |run| {
                  run.map(|chain| HashTrie::<H, P, E>::child(rows, links, 1, arity, chain, config));
              });
          }))
      };
      (partitioned.tuples, root)
  }
  ```
  - Eight parameters trip clippy's `too_many_arguments` (limit 7) under `-Dwarnings`; the
    `allow` follows the precedent in `kermit/src/main.rs` (see Contract issues).
  - If T4 marked `push_row` `#[must_use]`, the tail's call warns (`unused_must_use`): write it
    `let _new_key = chain::push_row(&mut lists, hash, config.load_factor, links, row);` here and
    in `check_one_run` (Step 19).
  - The `fill_runs` step closure captures `rows: &Tuples` and `links: &ChainLinks`, both
    `Sync`, as its `Fn + Sync` bound requires.

  **`push_in_run`.** Before (HEAD 408–421):
  ```rust
  /// Algorithm 2's lines 6–7 inside one run of a presized table of lists:
  /// `presized:N`'s root step, the same for every arity. Returns the row's id
  /// if its key's probe ran off its region, for the calling thread to push
  /// afterwards.
  fn push_in_run<H: HashStrategy>(
      tuples: &Tuples, run: &mut BucketRun<'_, TupleList>, row: RowId,
  ) -> Result<(), RowId> {
      match run.entry(H::hash(tuples.row(row)[0])) {
          | Ok(RunEntry::Occupied(list)) => list.push(row),
          | Ok(RunEntry::Vacant(bucket)) => bucket.insert(Vec::new()).push(row),
          | Err(Overflow) => return Err(row),
      }
      Ok(())
  }
  ```
  After:
  ```rust
  /// Algorithm 2's lines 6–7 inside one run of a presized table of chains:
  /// `presized:N`'s root step, the same for every arity. Pushes `row` onto the
  /// chain in its key's bucket, starting the chain in an empty bucket, so it
  /// writes the links of `row` and of the chain's old tail, both rows of the
  /// run's own partition. Returns the row's id if its key's probe ran off its
  /// region, for the calling thread to push afterwards.
  fn push_in_run<H: HashStrategy>(
      tuples: &Tuples, links: &ChainLinks, run: &mut BucketRun<'_, Chain>, row: RowId,
  ) -> Result<(), RowId> {
      match run.entry(H::hash(tuples.row(row)[0])) {
          | Ok(RunEntry::Occupied(chain)) => chain.push(links, row),
          | Ok(RunEntry::Vacant(bucket)) => {
              bucket.insert(Chain::start(links, row));
          },
          | Err(Overflow) => return Err(row),
      }
      Ok(())
  }
  ```

  `fill_runs` (HEAD 423–448) is unchanged: it is generic over the bucket value, and its caller's
  `step` closure carries the links.

- [ ] **Step 18: The three partitioning arms pass the links.**

  In `kermit-ds/src/ds/hash_trie/implementation.rs`, `from_tuples_with_config_and_build_mode`'s
  `Radix`, `Parallel` and `Presized` arms (HEAD 516–547) call the functions this chunk just
  changed. C05a may already have written them; either way they must read exactly as follows
  (the `Presized` arm's prerequisite `assert_eq!` and its comment stay as they are, above the
  call):
  ```rust
              | HashTrieBuildMode::Radix(bits) => {
                  Self::from_tuples_partitioned(header, config, tuples, |tuples, links, log2, arity| {
                      let mut root = Self::make_root_sized(arity, log2);
                      let copy =
                          radix::fill_root::<H, P, E>(tuples, links, &mut root, arity, bits, config);
                      (copy, root)
                  })
              },
              | HashTrieBuildMode::Parallel(threads) => {
                  Self::from_tuples_partitioned(header, config, tuples, |tuples, links, log2, arity| {
                      let mut root = Self::make_root_sized(arity, log2);
                      let copy = parallel::fill_root::<H, P, E>(
                          tuples, links, &mut root, arity, threads, config,
                      );
                      (copy, root)
                  })
              },
  ```
  and, in the `Presized` arm, the call after the `assert_eq!`:
  ```rust
                  Self::from_tuples_partitioned(header, config, tuples, |tuples, links, log2, arity| {
                      parallel::fill_presized_root::<H, P, E>(
                          tuples, links, log2, arity, threads, config,
                      )
                  })
  ```
  `cargo fmt` re-wraps them; what matters is that `links` is the closure's second parameter and
  each fill's second argument. The `links` C05a's `from_tuples_partitioned` passes is
  `ChainLinks::new(tuples.len())`: the copy has the batch's row count, so the array indexes the
  copy's ids.

- [ ] **Step 19: `parallel.rs`, the existing tests.**

  All edits are in `mod tests` of `kermit-ds/src/ds/hash_trie/parallel.rs`.

  **The test-module `use`** (HEAD 452–471). Replace the whole block with:
  ```rust
      use {
          super::*,
          crate::{
              ds::hash_trie::{
                  build_mode::{HashTrieBuildMode, RadixBits},
                  bulk::all_rows,
                  config::{ChildCapacity, HashTrieConfig, LoadFactor, RootCapacity},
                  expansion::{EagerExpansion, LazyExpansion},
                  identity::{
                      assert_equivalent_root, assert_equivalent_trie, assert_same_contents,
                      assert_same_node_contents, assert_same_trie, configs, inputs, Rows,
                      LOAD_PERCENTS,
                  },
                  implementation::HashTrie,
                  pruning::{NoPruning, SingletonPruning},
              },
              relation::{BuildModeRelation, ConfigurableRelation, Relation},
              test_support::{Lcg, Mod10HashStrategy},
          },
          kermit_iters::{FxHashStrategy, LayoutOption, SipHashStrategy},
      };
  ```
  - `LoadFactor` moves here from the parent's imports (`tuples_config` names it).
  - `assert_same_node_contents` and `Rows` are for the new pin (Step 20).
  - `Chain`, `chain`, `ChainLinks`, `HashTable`, `RowId`, `push_in_run` and the other parent items
    still arrive through `super::*`.

  **`parallel_in_small_morsels`** (HEAD 483–506). Before:
  ```rust
      /// A `parallel:t` trie built through `HashTrie::from_tuples_partitioned`
      /// with morsels of 7 tuples, which make the partition step cut a small
      /// input many times.
      fn parallel_in_small_morsels<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
          arity: usize, config: HashTrieConfig, tuples: &Tuples, t: usize,
      ) -> HashTrie<H, P, E> {
          HashTrie::<H, P, E>::from_tuples_partitioned(
              arity.into(),
              config,
              tuples.clone(),
              |tuples, log2, arity| {
                  let mut root = HashTrie::<H, P, E>::make_root_sized(arity, log2);
                  let copy = fill_root_in_morsels::<H, P, E>(
                      tuples,
                      &mut root,
                      arity,
                      threads(t),
                      7,
                      config,
                  );
                  (copy, root)
              },
          )
      }
  ```
  After:
  ```rust
      /// A `parallel:t` trie built through `HashTrie::from_tuples_partitioned`
      /// with morsels of 7 tuples, which make the partition step cut a small
      /// input many times.
      fn parallel_in_small_morsels<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
          arity: usize, config: HashTrieConfig, tuples: &Tuples, t: usize,
      ) -> HashTrie<H, P, E> {
          HashTrie::<H, P, E>::from_tuples_partitioned(
              arity.into(),
              config,
              tuples.clone(),
              |tuples, links, log2, arity| {
                  let mut root = HashTrie::<H, P, E>::make_root_sized(arity, log2);
                  let copy = fill_root_in_morsels::<H, P, E>(
                      tuples,
                      links,
                      &mut root,
                      arity,
                      threads(t),
                      7,
                      config,
                  );
                  (copy, root)
              },
          )
      }
  ```

  **`check_identity`'s doc** (HEAD 508–520). In the sentence
  `/// same hashes in the same buckets at the same capacity. Every chain,` /
  `/// singleton and pending list holds the same tuples in the same order,` /
  `/// each id read through its own trie's buffer (`assert_same_contents`).`,
  the three lines become:
  ```rust
      /// same hashes in the same buckets at the same capacity. Every chain,
      /// singleton and pending chain holds the same tuples in the same order,
      /// each chain walked through its own trie's links and each id read
      /// through its own trie's buffer (`assert_same_contents`).
  ```
  The function body is unchanged.

  **`check_one_run`** (HEAD 790–837). Before:
  ```rust
      /// Grouping alone, without threads: one run over the whole presized
      /// table of lists, the overflow pushed afterwards in input order, then
      /// `build_nested`. The root must be equivalent to bulk's (Amendment 2),
      /// and every child identical.
      fn check_one_run<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
          for arity in 1..=3 {
              for &percent in LOAD_PERCENTS {
                  for child_capacity in [ChildCapacity::Grow, ChildCapacity::Tuples] {
                      let config = tuples_config(percent, child_capacity);
                      for (input, tuples) in inputs(arity) {
                          let label = format!(
                              "{}/{}/{} arity {arity}, load {percent}%, children {}, {input}",
                              H::NAME,
                              P::NAME,
                              E::NAME,
                              child_capacity.axis_value(),
                          );
                          let bulk = HashTrie::<H, P, E>::from_tuples_with_config(
                              arity.into(),
                              config,
                              tuples.clone(),
                          );
                          let log2 = config.root_log2_capacity(tuples.len());
                          let mut lists: HashTable<TupleList> = HashTable::with_log2_capacity(log2);
                          let tail: Vec<RowId> = lists.with_runs(1, 1 << log2, |mut runs| {
                              all_rows(&tuples)
                                  .filter_map(|row| {
                                      push_in_run::<H>(&tuples, &mut runs[0], row).err()
                                  })
                                  .collect()
                          });
                          for row in tail {
                              lists
                                  .entry_or_insert_with(
                                      H::hash(tuples.row(row)[0]),
                                      config.load_factor,
                                      Vec::new,
                                  )
                                  .push(row);
                          }
                          let root =
                              HashTrie::<H, P, E>::build_nested(&tuples, 0, arity, lists, config);
                          assert_equivalent_root(bulk.root(), &root, &label);
                      }
                  }
              }
          }
      }
  ```
  After:
  ```rust
      /// Grouping alone, without threads: one run over the whole presized
      /// table of chains, the overflow pushed afterwards in input order, then
      /// `build_nested`, every list threaded through one links array over the
      /// input. The root must be equivalent to bulk's (Amendment 2), and every
      /// child array-identical, each chain walked through its own trie's
      /// links. One run covers the whole table, so the tail holds only keys
      /// whose probe would wrap past the last bucket.
      fn check_one_run<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
          for arity in 1..=3 {
              for &percent in LOAD_PERCENTS {
                  for child_capacity in [ChildCapacity::Grow, ChildCapacity::Tuples] {
                      let config = tuples_config(percent, child_capacity);
                      for (input, tuples) in inputs(arity) {
                          let label = format!(
                              "{}/{}/{} arity {arity}, load {percent}%, children {}, {input}",
                              H::NAME,
                              P::NAME,
                              E::NAME,
                              child_capacity.axis_value(),
                          );
                          let bulk = HashTrie::<H, P, E>::from_tuples_with_config(
                              arity.into(),
                              config,
                              tuples.clone(),
                          );
                          let log2 = config.root_log2_capacity(tuples.len());
                          let links = ChainLinks::new(tuples.len());
                          let mut lists: HashTable<Chain> = HashTable::with_log2_capacity(log2);
                          let tail: Vec<RowId> = lists.with_runs(1, 1 << log2, |mut runs| {
                              all_rows(&tuples)
                                  .filter_map(|row| {
                                      push_in_run::<H>(&tuples, &links, &mut runs[0], row).err()
                                  })
                                  .collect()
                          });
                          for row in tail {
                              let hash = H::hash(tuples.row(row)[0]);
                              chain::push_row(&mut lists, hash, config.load_factor, &links, row);
                          }
                          let root = HashTrie::<H, P, E>::build_nested(
                              &tuples, &links, 0, arity, lists, config,
                          );
                          assert_equivalent_root(bulk.links(), bulk.root(), &links, &root, &label);
                      }
                  }
              }
          }
      }
  ```
  `bulk` keeps the input batch as its buffer, so both roots' ids name the same rows; each side's
  chains are walked through its own links.

  **`presized_in_small_regions`** (HEAD 1076–1098). Before:
  ```rust
      /// A `presized:t` trie built through `HashTrie::from_tuples_partitioned`
      /// with morsels of 7 tuples and `region_buckets`-bucket regions, so small
      /// inputs span many morsels and regions and overflow.
      fn presized_in_small_regions<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
          arity: usize, config: HashTrieConfig, tuples: &Tuples, t: usize, region_buckets: usize,
      ) -> HashTrie<H, P, E> {
          HashTrie::<H, P, E>::from_tuples_partitioned(
              arity.into(),
              config,
              tuples.clone(),
              |tuples, log2, arity| {
                  fill_presized_root_in::<H, P, E>(
                      tuples,
                      log2,
                      arity,
                      threads(t),
                      7,
                      region_buckets,
                      config,
                  )
              },
          )
      }
  ```
  After:
  ```rust
      /// A `presized:t` trie built through `HashTrie::from_tuples_partitioned`
      /// with morsels of 7 tuples and `region_buckets`-bucket regions, so small
      /// inputs span many morsels and regions and overflow.
      fn presized_in_small_regions<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
          arity: usize, config: HashTrieConfig, tuples: &Tuples, t: usize, region_buckets: usize,
      ) -> HashTrie<H, P, E> {
          HashTrie::<H, P, E>::from_tuples_partitioned(
              arity.into(),
              config,
              tuples.clone(),
              |tuples, links, log2, arity| {
                  fill_presized_root_in::<H, P, E>(
                      tuples,
                      links,
                      log2,
                      arity,
                      threads(t),
                      7,
                      region_buckets,
                      config,
                  )
              },
          )
      }
  ```

  Every other test in the module compiles and holds unchanged (the preamble lists them). Check
  that no other test calls a changed signature:
  ```bash
  rg -n 'fill_root_in_morsels|fill_presized_root_in|push_in_run|build_nested|build_scratch_root|build_and_merge|assert_equivalent_root\(|assert_same_node\(|TupleList|Vec::new\)' \
    kermit-ds/src/ds/hash_trie/parallel.rs
  ```
  Expected: the definitions, the non-test calls of Step 17, and in `mod tests` only the
  calls in `parallel_in_small_morsels`, `check_one_run` and `presized_in_small_regions` as
  rewritten above. No `TupleList`, no `Vec::new)`. A T1-added test that calls
  `radix::build_scratch_root` or `radix::build_and_merge` takes the rule of Step 16.

- [ ] **Step 20: The pin: `presized:N` re-threads a deferred key's rows in another partition's run.**

  In `mod tests` of `parallel.rs`, after `keys_that_cannot_fit_their_region_go_to_the_tail`
  (HEAD 1262–1284), add:
  ```rust
      /// Homes in a 32-bucket root: first keys 0 to 2 at bucket 7, the last
      /// bucket of the first 8-bucket region, and key 3 at bucket 8, the first
      /// of the second. Every value from 16 up shares one hash, so distinct
      /// last values meet in one leaf chain; other distinct values keep
      /// distinct hashes.
      #[derive(Copy, Clone, Default, Debug)]
      struct CrossRunHash;

      impl LayoutOption for CrossRunHash {
          const NAME: &'static str = "cross-run";
      }

      impl HashStrategy for CrossRunHash {
          fn hash(key: usize) -> u64 {
              let (home, low) = match key {
                  | 0..=2 => (7, key),
                  | 16.. => (8, 16),
                  | _ => (8, key),
              };
              super::super::hash_table::hash_with_home(home, 5, low as u64)
          }
      }

      /// Every row of `trie` with the row its link names, or `None` for `NIL`,
      /// sorted: the trie's threading read as rows rather than ids, so two
      /// tries over buffers in different row orders compare.
      fn successors<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
          trie: &HashTrie<H, P, E>,
      ) -> Vec<(Vec<usize>, Option<Vec<usize>>)> {
          let (tuples, links) = (trie.tuples(), trie.links());
          let mut pairs: Vec<_> = all_rows(tuples)
              .map(|row| {
                  let next = links.get(row);
                  let next = (next != ChainLinks::NIL).then(|| tuples.row(next).to_vec());
                  (tuples.row(row).to_vec(), next)
              })
              .collect();
          pairs.sort();
          pairs
      }

      /// The child in `root`'s bucket for the first key `key`.
      fn child_of<P: PruningPolicy, E: ExpansionPolicy>(
          root: &HashTrieNode<P, E>, key: usize,
      ) -> &HashTrieNode<P, E> {
          let HashTrieNode::Inner(table) = root else {
              panic!("arity 3 has an Inner root")
          };
          let bucket = table
              .index_of(CrossRunHash::hash(key))
              .unwrap_or_else(|| panic!("key {key} is in the root"));
          table.value_at(bucket).expect("index_of found an occupied bucket")
      }

      /// `presized:N`'s run workers re-thread a deferred key's rows that live
      /// in another partition. The fixture:
      /// - Sixteen rows `[i % 4, (i / 4) % 2, 16 + i]` give a 32-bucket root of
      ///   8-bucket regions: four regions, so every N here cuts four
      ///   partitions, a region (and a run) each.
      /// - First keys 0, 1 and 2 home at bucket 7, so their twelve rows are
      ///   partition 0, ids 0 to 11 of the copy; key 3 homes at bucket 8, so
      ///   its four rows are partition 1, ids 12 to 15.
      /// - Key 0 takes bucket 7 and key 3 bucket 8. Keys 1 and 2 overflow
      ///   region 0, and the tail places them, in input order, at buckets 9 and
      ///   10: in run 1.
      ///
      /// So run 1's worker builds keys 1's and 2's children from partition 0's
      /// rows, while run 0's worker re-threads key 0's rows of the same
      /// partition: two writers in one partition's links, one per row. Each
      /// `(first, second)` pair holds two distinct rows whose last values share
      /// a hash, so every leaf chain has a link between rows to check. The trie
      /// must be equivalent to `bulk`'s; the deferred keys' subtries must hold
      /// `bulk`'s rows in `bulk`'s order, each chain walked through its own
      /// trie's links; and every row's link must name the row it names in
      /// `bulk`, read as rows: the chains thread exactly their rows, whichever
      /// worker threaded them. Under lazy expansion the root's chains stay
      /// pending, and the same checks read them.
      #[test]
      #[cfg_attr(miri, ignore = "compares thread counts; Miri runs one")]
      fn presized_rethreads_deferred_rows_in_another_partitions_run() {
          fn check<E: ExpansionPolicy>() {
              let config = tuples_config(70, ChildCapacity::Grow);
              let tuples = Tuples::from(
                  (0..16)
                      .map(|i| vec![i % 4, (i / 4) % 2, 16 + i])
                      .collect::<Vec<Vec<usize>>>(),
              );
              assert_eq!(
                  config.root_log2_capacity(tuples.len()),
                  5,
                  "a 32-bucket root"
              );
              let bulk = HashTrie::<CrossRunHash, NoPruning, E>::from_tuples_with_config(
                  3.into(),
                  config,
                  tuples.clone(),
              );
              for t in [1, 2, 4] {
                  let label = format!("{} presized:{t}", E::NAME);
                  PARALLEL_BUILDS.with(|b| b.borrow_mut().clear());
                  let built = presized_in_small_regions::<CrossRunHash, NoPruning, E>(
                      3, config, &tuples, t, 8,
                  );
                  // The premise, so the test cannot pass vacuously: four
                  // partitions, keys 1 and 2 deferred, their rows in partition
                  // 0 and their buckets in run 1.
                  let builds = PARALLEL_BUILDS.with(|b| b.take());
                  assert_eq!(builds.len(), 1, "{label}");
                  assert_eq!(
                      builds[0].partition_sizes,
                      vec![12, 4, 0, 0],
                      "{label}: partitions"
                  );
                  assert_eq!(
                      builds[0].deferred,
                      Some(8),
                      "{label}: keys 1 and 2's rows"
                  );
                  assert_ne!(
                      built.tuples(),
                      bulk.tuples(),
                      "{label}: the copy moves key 3's rows last"
                  );
                  for row in all_rows(built.tuples()) {
                      let key = built.tuples().row(row)[0];
                      assert_eq!(
                          key <= 2,
                          row < 12,
                          "{label}: row {row}, key {key}, sits in its home partition"
                      );
                  }
                  let HashTrieNode::Inner(root) = built.root() else {
                      panic!("{label}: arity 3 has an Inner root")
                  };
                  for (key, bucket) in [(0, 7), (3, 8), (1, 9), (2, 10)] {
                      assert_eq!(
                          root.index_of(CrossRunHash::hash(key)),
                          Some(bucket),
                          "{label}: key {key}'s bucket"
                      );
                  }
                  // The pin.
                  assert_equivalent_trie(&bulk, &built, &label);
                  for key in [1, 2] {
                      assert_same_node_contents(
                          Rows::of(&bulk),
                          child_of(bulk.root(), key),
                          Rows::of(&built),
                          child_of(built.root(), key),
                          &format!("{label}: key {key}"),
                      );
                  }
                  assert_eq!(
                      successors(&bulk),
                      successors(&built),
                      "{label}: every row's link"
                  );
              }
          }
          check::<EagerExpansion>();
          check::<LazyExpansion>();
      }
  ```
  Why the premises hold (checked against the code, not run):
  - `root_log2_capacity(16)` at 70 % is ⌈1600 / 70⌉ = 23 buckets, rounded to 32: log₂ 5.
  - `presized_parts(5, threads(t), 8)` is `min(next_power_of_two(4t), 32 / 8)` = 4 for t = 1, 2,
    4, so `shift` = 3 and a row's partition is its home bucket `>> 3`: 0 for keys 0–2 (home 7),
    1 for key 3 (home 8). `partition_sizes` is `[12, 4, 0, 0]`.
  - Run 0 is buckets 0–7, region 0. Key 0 (rows 0, 4, 8, 12) starts its chain at bucket 7; keys 1
    and 2 then find bucket 7 taken and reach the region's end: all 8 of their rows are deferred.
    Run 1 places key 3 at bucket 8.
  - The tail, by `origin`: key 1 (first at input position 1) probes 7, 8, takes 9; key 2
    (position 2) takes 10. `bulk` inserts in first-appearance order 0, 1, 2, 3 into buckets 7, 8,
    9, 10. Both occupy {7, 8, 9, 10} with total displacement 0 + 1 + 2 + 2 = 5 (`bulk`) and
    0 + 0 + 2 + 3 = 5 (here), so `assert_equivalent_trie`'s root rule holds.
  - Key k's rows are i = k, k + 4, k + 8, k + 12; the second value `(i / 4) % 2` gives the pairs
    {k, k + 8} and {k + 4, k + 12}, and their last values `16 + i` share one hash, so each leaf
    chain holds two distinct rows in input order.

  The eager case re-threads at two levels below the root (the second attribute's table, then the
  leaf chains), in run 1's worker for partition 0's rows.

- [ ] **Step 21: Remove Task 4's `#[allow(dead_code)]` markers.**

  Every `chain.rs` item now has a caller: `start`, `push` and `push_row` in `bulk.rs`,
  `implementation.rs`, `radix.rs` and `parallel.rs`; `walk`, `len` and `head` in `bulk.rs`,
  `implementation.rs` and `identity.rs`; `leaf_rows` in `hash_trie_iter.rs`.
  ```bash
  rg -n 'No caller until the lists become chains' kermit-ds/src kermit-iters/src
  ```
  Delete each `#[allow(dead_code)]` attribute that carries this comment. For T4's one-line form
  (the B1 precedent),
  ```rust
  #[allow(dead_code)] // No caller until the lists become chains (Task 5).
  ```
  delete the whole line:
  ```bash
  sed -i '/^[[:space:]]*#\[allow(dead_code)\] \/\/ No caller until the lists become chains (Task 5)\.$/d' \
    kermit-ds/src/ds/hash_trie/chain.rs
  ```
  If T4 put the comment on a line of its own above the attribute, delete both lines of each
  pair by hand. Then:
  ```bash
  rg -n 'No caller until the lists become chains' kermit-ds/src kermit-iters/src   # prints nothing
  rg -n 'allow\(dead_code\)' kermit-ds/src kermit-iters/src
  ```
  Expected for the second: exactly the three pre-existing markers: `kermit-ds/src/ds/hash_trie/mod.rs`
  (`mod hash_table`, HEAD line 15) and two lines in `implementation.rs`'s test fixture (HEAD
  1657 and 1662; C05a shifts them). If T4 put its marker on `mod chain;` in `mod.rs`, that line
  appears in the first grep and goes too.

- [ ] **Step 22: Sweep the two files for list wording.**

  ```bash
  rg -n 'TupleList|row-id list|lists of row ids|table of lists|pending list|Vec<RowId>' \
    kermit-ds/src/ds/hash_trie/radix.rs kermit-ds/src/ds/hash_trie/parallel.rs
  ```
  Expected hits, all true after this chunk:
  - `parallel.rs`'s `fill_runs`: its return type and its workers' `deferred` lists
    (`Vec<RowId>`), the presized tail's queue of deferred ids. It is sorted by `origin`, so it is
    not a chain, and it is not a list of the trie (see Contract issues).
  - `parallel.rs`'s tests: `check_one_run`'s `tail: Vec<RowId>`.

  Any other hit is stale wording the steps above missed: reword it to name chains (a list of
  the trie is a chain; a lazy child holds a pending chain).

- [ ] **Step 23: Build, then the radix and parallel tests, then the whole crate.**

  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l   # prints 0
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --no-run
  ```
  Expected: it compiles with no warning. This is the first build since C05a began, so it
  rebuilds `kermit-ds` and its four test targets: a few minutes at two jobs. A
  `dead_code` warning on a `chain.rs` item means a caller C05a or this chunk should have
  written is missing; an `unused_must_use` warning on `push_row` takes Step 17's second
  bullet.

  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_trie::radix
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_trie::parallel
  ```
  Expected: `test result: ok`, 0 failed, in each. Under `hash_trie::parallel`:
  `presized_rethreads_deferred_rows_in_another_partitions_run`, `one_run_of_grouping_builds_the_bulk_root`,
  `presized_tail_goes_in_input_order_for_every_n`, `keys_that_cannot_fit_their_region_go_to_the_tail`,
  the `parallel_builds_the_bulk_trie_*` and `presized_parallel_builds_are_*` matrices and
  `build_modes_reach_their_builds`. The presized and radix matrices are the slow ones: about a
  minute each in a debug build.

  The new pin passes on its first run: it pins behaviour this chunk implements, and it could not
  compile before. Its premise assertions (partition sizes, deferred count, buckets 9 and 10, rows
  0–11) are what keep it from passing vacuously; if one fails, the fixture's reasoning in Step
  Step 20 is wrong, not the build. Re-derive it from the code rather than editing the expected
  values to match.

  Then the whole crate (unit tests, `tests/hash_trie_tests.rs`, `tests/parquet_tests.rs`,
  `tests/trie_tests.rs`, doctests):
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds
  ```
  Expected: every `test result:` line `ok`, 0 failed; 3–6 minutes. It includes C05a's pins
  (`bulk_and_incremental_thread_the_same_links`,
  `resolve_rethreads_the_pending_chain_through_a_shared_reference`), T4's `chain` tests, the
  `lazy_expansion` trace tests in `hash_trie_tests.rs`, `eager_tries_stay_sync`,
  `nodes_are_send_under_every_layout`, and B1's
  `partitioning_builds_keep_the_inputs_spare_capacity` and
  `insert_after_a_partitioning_build_appends_to_the_copy`.

- [ ] **Step 24: The workspace's tests, detached.**

  The 16-pattern suites in `kermit/tests/join_tests.rs` (8 HashTrie aliases × both optimisers,
  under `AnyOrders`, and the build-mode cells), `result_allocation.rs`, `tuple_allocation.rs`,
  the `cli_*` tests, the LUBM mini oracle and `kermit-algos`'s hash family all run here. The
  build links every `kermit` integration-test binary: budget 10–15 minutes at two jobs, then a
  few minutes of tests. Run it detached (the harness kills long foreground jobs, and a
  background job on low free memory):
  ```bash
  setsid nohup nix develop --command env CARGO_BUILD_JOBS=2 cargo test --workspace > $SCRATCH/t5-test.log 2>&1 &
  echo $! > $SCRATCH/t5-test.pid; disown
  while kill -0 "$(cat $SCRATCH/t5-test.pid)" 2>/dev/null; do sleep 30; done
  grep -E '^test result' $SCRATCH/t5-test.log \
    | awk '{p+=$4; f+=$6; i+=$8} END {print "cargo "p" passed, "f" failed, "i" ignored"}'
  grep -nE 'FAILED|panicked at|error(\[E[0-9]+\])?:' $SCRATCH/t5-test.log | head -20
  ```
  If the session blocks a foreground `sleep`, wait on the same `kill -0` condition with its
  Monitor tool instead.

  Expected: `0 failed`, and the second grep prints nothing.
  - `tuple_allocation.rs`' two HashTrie cells pass at their old bounds (777 and 770): every
    allocation T5 removes only shrinks `many − once`. Task 6 tightens them.
  - `result_allocation.rs` passes unchanged: `emit_leaf` advances cursors (T3), and a walk of a
    linked chain allocates nothing.
  - `e2e_watdiv` is known to fail intermittently under the parallel run (issue #63's
    clock-seeded data). If it is the only failure, re-run it alone
    (`nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-rdf --test e2e_watdiv`)
    before suspecting this task.

- [ ] **Step 25: Lint, format, document.**

  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: all clean. Clippy rebuilds every target under new flags: 5–10 minutes.
  - `fill_presized_root_in`'s `#[allow(clippy::too_many_arguments)]` is the only new allow.
  - In the module docs, `[`Chain`]` and `[`ChainLinks`]` resolve through the new imports, and
    `[`partition_by_first_hash`]`, `[`build_and_merge`]` and `[`merge_in_first_appearance_order`]`
    to `radix.rs`'s own items.
  - If `fmt --check` disagrees with CI later, suspect the flake's pinned nightly first
    (CLAUDE.md, "The flake's nightly must track CI's").

- [ ] **Step 26: Commit Task 5 (C05a and C05b together).**

  ```bash
  git status --short
  ```
  Expected: modified, and nothing else:
  - `kermit-ds/src/ds/hash_trie/{bulk,chain,expansion,hash_trie_iter,identity,implementation,node,parallel,radix}.rs`;
  - `kermit-ds/src/ds/hash_trie/mod.rs` only if T4's marker sat on `mod chain;`;
  - any `kermit-ds/tests/` file C05a's chunk names as modified.

  Stage those by name, then commit:
  ```bash
  git add kermit-ds/src/ds/hash_trie/bulk.rs kermit-ds/src/ds/hash_trie/chain.rs \
    kermit-ds/src/ds/hash_trie/expansion.rs kermit-ds/src/ds/hash_trie/hash_trie_iter.rs \
    kermit-ds/src/ds/hash_trie/identity.rs kermit-ds/src/ds/hash_trie/implementation.rs \
    kermit-ds/src/ds/hash_trie/node.rs kermit-ds/src/ds/hash_trie/parallel.rs \
    kermit-ds/src/ds/hash_trie/radix.rs
  git status --short   # add mod.rs or a kermit-ds/tests file listed as modified, by name
  git commit -m "$(cat <<'EOF'
  feat(hash-trie): thread every list through per-row chain links (#101)

  Every HashTrie list is now a Chain { head, tail, len } in its bucket,
  threaded through one ChainLinks array beside the tuple buffer, a 4-byte
  link per row: the paper's chain pointer (VLDB 2020 §3.3.2, Figure 3),
  kept in an array of relaxed atomics so lazy expansion can re-thread a
  child through a shared reference. Algorithm 2's per-bucket lists, the
  leaf chains, the lazy pending lists and the radix family's scratch
  lists all become Chains. Algorithm 2 moves each row to exactly one
  child list by rewriting its link, so a build allocates per table and
  never per list. TupleList, tuple_list_heap_bytes and child's lazy
  shrink_to special case are gone; heap_size_bytes counts the links.

  Lists append and keep input order, and every push writes NIL into the
  pushed row's link, so the links are a function of the lists: bulk and
  incremental stay array-identical, links included, and assert_same_trie
  now compares the arrays. radix:K, parallel:N and presized:N stay
  equivalent to bulk as B1 left them. Each partition's worker threads
  only its own rows, and presized:N's run workers re-thread a deferred
  key's rows across partitions, one writer per link; a new test pins
  that. Tries lend linked LeafRows.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_017PLoRDCeuYmtirxRTMXGJj
  EOF
  )"
  git status --short   # prints nothing
  ```

- [ ] **Step 27: Mutation check 2: `Chain::push` omits the pushed row's `NIL`.**

  Follow the ground rules: the commit above comes first; apply the exact edit; confirm it
  applied; see the test fail; reverse the exact edit, never with `git checkout`.
  ```bash
  rg -n 'links.set\(row, ChainLinks::NIL\);' kermit-ds/src/ds/hash_trie/chain.rs
  ```
  Expected: two lines, one in `Chain::start` and one in `Chain::push`.
  - **Mutant:** in `Chain::push` only, delete the line `links.set(row, ChainLinks::NIL);` (the
    store that follows `links.set(self.tail, row);`). Never the one in `start`.
  - **Applied?** `git diff --stat` shows one file, one deletion; the `rg` above now prints one
    line, inside `start`.
  - **Run:**
    ```bash
    nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds bulk_and_incremental_thread_the_same_links
    ```
    Expected: FAILED (about two minutes, most of it the rebuild). Under the mutant a pushed row
    keeps a stale link: 0 from `ChainLinks::new`'s zeroed array at the root, or its previous
    level's successor below it. `incremental` pushes `NIL` for every row up front, so its tails
    stay `NIL` while `bulk`'s do not. The panic is either `ChainWalk`'s debug assertion that a
    chain's tail links to `NIL` (it fires during `bulk`'s own build, when Algorithm 2 walks a
    root chain of two or more rows at arity 2 or more), or the test's links comparison
    (`…: links`). Either counts. T4's chain tests fail too.
  - **Reverse:** put the line back exactly where it was, re-run the same command (`ok`), and
    check that `git status --short` prints nothing.

- [ ] **Step 28: Mutation check 4: `resolve` groups the pending chain over fresh links.**

  - **Mutant:** in `HashTrie::resolve` (`implementation.rs`), inside the `pending.expand(|chain| …)`
    closure, replace the argument `&self.links` of the `Self::build_child_table(…)` call with
    `&ChainLinks::new(self.tuples.len())`. Only that occurrence: `&self.links` appears elsewhere
    in the file.
  - **Applied?** `git diff` shows one changed line, inside `fn resolve`;
    `rg -c 'ChainLinks::new\(self.tuples.len\(\)\)' kermit-ds/src/ds/hash_trie/implementation.rs`
    prints `1`.
  - **Run:**
    ```bash
    nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds resolve_rethreads_the_pending_chain_through_a_shared_reference
    nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --test hash_trie_tests lazy_expansion
    ```
    Expected: both FAILED. Over a zeroed array, the walk of the pending chain reads link 0 after
    its head, so it yields row 0 for every later position, and it reads a non-`NIL` link at its
    tail: `ChainWalk`'s debug assertion panics on the first expansion. Even without it, the
    child's chains would be threaded through a temporary array that is dropped, so the trie's
    own links would still describe the pending chain. In `lazy_expansion` the failure is a
    panic inside a `step`, or an `Obs` mismatch (`arity …, seed …, step …`).
  - **Reverse:** restore `&self.links`, re-run both commands (`ok`), and check that
    `git status --short` prints nothing.

- [ ] **Step 29: Mutation check 5: the presized tail ordered by new id (B1's pin, re-run).**

  - **Mutant:** in `fill_presized_root_in` (`parallel.rs`), replace the line
    `    deferred.sort_unstable_by_key(|&row| origin[row as usize]);` with
    `    deferred.sort_unstable();`.
  - **Applied?** `git diff --stat` shows one file changed;
    `rg -c 'deferred.sort_unstable\(\);' kermit-ds/src/ds/hash_trie/parallel.rs` prints `1`.
    The build warns that `origin` is unused; that is expected and harmless here.
  - **Run:**
    ```bash
    nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds presized_tail_goes_in_input_order_for_every_n
    ```
    Expected: FAILED at `presized:4`, with `presized:4: X, first in input order, takes bucket 8`:
    the copy gives Y's rows (key 4) smaller ids than X's, so a tail by id pushes Y first and Y
    takes bucket 8. `presized:1` passes first, because its copy is the input. The chains do not
    change this: the tail's order decides only which bucket each deferred key's chain starts in.
    (The new pin of Step 20 passes under this mutant: its deferred rows all sit in
    partition 0, where id order is input order.)
  - **Reverse:** restore the line, re-run the command (`ok`), and check that
    `git status --short` prints nothing.

---

### Task 6: `tuple_allocation.rs`: no HashTrie list allocates once lists are chains

**Files:**
- Modify: `kermit/tests/tuple_allocation.rs` (T5 did not touch it, so HEAD lines hold):
  - module doc, lines 1–2 and the HashTrie bullet (21–25);
  - `hash_trie_builds_allocate_per_list_not_per_row`: doc 364–382, body 383–436;
  - `hash_trie_presized_build_allocates_per_list_not_per_row`: doc 438–453, body 454–482.
- Not modified: `kermit/tests/result_allocation.rs` (Step 6 says why and runs it).

**State assumed:** Task 5 is committed. Every HashTrie list is a `Chain`; `links` is built by
`ChainLinks::new(tuples.len())` (`bulk` and the three partitioning builds) or
`ChainLinks::with_capacity(tuples.len())` plus one `push` per row (`incremental`).

**The allocations, re-derived from the code after Task 5.** Each HashTrie cell is
differential: one build of the 100 distinct tuples once (`once`, 100 rows) against one of the
same 100 tuples repeated 100 times (`many`, 10 000 rows), both of `repeated(…)`, built before
`allocation_counter::measure` starts. An allocation affects the difference only if its count
differs between the two sizes. A reallocation counts as one allocation (the counter keeps
`GlobalAlloc`'s default `realloc` and `alloc_zeroed`, which call its `alloc`).

| Source | Count at 100 rows / at 10 000 | Why |
| --- | --- | --- |
| Tables, root and children, every Layout (`grow`) | equal | Sized and grown by keys: 10 first keys, 10 second keys under each. |
| Lazy children's boxes (`LazyChild`) | equal | One per root key. |
| Lists: Algorithm 2's, the leaf chains, the pending chains, the scratch roots' | 0 / 0 | `Chain`s in their buckets (T5). Before T5, 111 (`grow` cells) or 110 (`presized:1`) `Vec`s grew by up to ⌈log₂ 100⌉ = 7 steps each: the 777 and 770 bounds. |
| `links`, `bulk` and the partitioning builds | 1 / 1 | `ChainLinks::new`: one zeroed `Vec<u32>` collected in place into atomics. |
| `links`, `incremental` | 1 / 1 | `ChainLinks::with_capacity(rows)`, then one `push` per row into that room: no growth. |
| The buffer | 0 / 0 (`bulk`, `incremental`); 1 / 1 (partitioning) | The input batch is kept; a partitioning build's copy is one buffer sized from the batch. |
| `partition_rows` (`radix:2`, `parallel:1`, `presized:1`) | equal | One morsel at both sizes (`radix:2` passes its batch as one; 10 000 rows fit one 16 384-row morsel), so the histogram, the partition numbers (sized from the morsel), the offsets, `origin`, the ranges and the per-worker results are one allocation each. |
| Scratch roots, `first_seen`, the `Entries` lists, the merge heap | equal | Sized or grown by keys. |
| `presized:1`'s root, run lists, `dispatch` results, test-hook record | equal | The root is one allocation at either size. 1 partition at 100 rows and 4 at 10 000, but every list of runs or partitions is sized once, and a worker's result list holds four entries in its first allocation. Nothing is deferred at either size, so the tail queue never allocates. |
| Workers (`thread::scope` at one thread) and the parallel test-hook record | equal | Per call, not per row. |

So nothing grows with the row count any more: the bound becomes **0 extra allocations**,
`many <= once`. The counts should be equal in every cell. The check stays one-sided, like every
other differential cell in the file: a one-time initialisation on a code path's first run would
fall in the smaller build, which runs first, and must not fail the test. Bounds only tighten:
777 and 770 become 0.

- [ ] **Step 1: Host check, and the premise.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l   # prints 0
  git log --oneline -1   # Task 5's commit: feat(hash-trie): thread every list through per-row chain links (#101)
  rg -n 'TupleList' kermit-ds/src   # prints nothing
  ```

- [ ] **Step 2: Measure first: print every cell's counts (temporary).**

  In `hash_trie_builds_allocate_per_list_not_per_row`, after the line
  ```rust
          let (once, many) = (allocations(mode, 1), allocations(mode, REPEATS));
  ```
  add:
  ```rust
          eprintln!("TEMP HashTrie {layout} {mode:?}: once {once}, many {many}");
  ```
  In `hash_trie_presized_build_allocates_per_list_not_per_row`, after the line
  ```rust
      let ((once, once_partitions), (many, many_partitions)) = (measure(1), measure(REPEATS));
  ```
  add:
  ```rust
      eprintln!("TEMP HashTrie presized:1: once {once}, many {many}");
  ```
  Run:
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test tuple_allocation hash_trie -- --nocapture 2>&1 | grep -E 'TEMP|test result'
  ```
  The first run after Task 5 rebuilds `kermit` and this test binary: 3–5 minutes. The tests
  themselves take seconds. Expected: both tests `ok` (at their old bounds), and eight `TEMP`
  lines. Copy them into this table, which goes into the commit message (Step 8):

  | Cell | once (100 rows) | many (10 000 rows) | many − once |
  | --- | --- | --- | --- |
  | eager `Bulk` | | | |
  | pruned `Bulk` | | | |
  | lazy `Bulk` | | | |
  | pruned lazy `Bulk` | | | |
  | eager `Incremental` | | | |
  | eager `Radix(2)` | | | |
  | eager `Parallel(1)` | | | |
  | `presized:1` (`root-capacity=tuples`) | | | |

  Expected: `many − once` is 0 in every row (the derivation above).

- [ ] **Step 3: If any `many − once` is above 0, find its source before going on.**

  Do not set the bound to the measured value: a difference means the derivation missed an
  allocation that grows with the rows, and the bound must name it.
  1. Measure that cell at 1, 2, 4, 10 and 100 repetitions (a temporary loop over
     `[1, 2, 4, 10, 100]` calling the cell's `allocations(mode, repeats)` or `measure(repeats)`,
     printing each count). A difference that grows linearly with the rows is a per-row
     allocation; one that grows by about one per doubling of the rows is a `Vec` grown one push
     at a time; one that appears only between 1 and 2 repetitions and is flat after it is a
     capacity threshold (for example a list that fits a first allocation of four entries at one
     size only).
  2. Find it on that cell's build path:
     ```bash
     rg -n 'Vec::new\(\)|Vec::with_capacity|vec!\[|\.push\(|collect' \
       kermit-ds/src/ds/hash_trie/bulk.rs kermit-ds/src/ds/hash_trie/chain.rs \
       kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/radix.rs \
       kermit-ds/src/ds/hash_trie/parallel.rs kermit-ds/src/morsel.rs \
       kermit-iters/src/chain_links.rs
     ```
     Read only the functions the cell's mode reaches (`from_tuples_in_bulk` →
     `build` / `group` / `build_nested` / `child`; `from_tuples_incrementally` → `insert_at`;
     `from_tuples_partitioned` → `radix::fill_root` or `parallel::fill_root` /
     `fill_presized_root` → `partition_rows`, `build_and_merge`, `fill_runs`, `map_in_runs`).
  3. If it is a list of the trie, or any per-row allocation, it is a Task 5 regression against
     the spec's "no allocation per list": fix the code in its own commit,
     `fix(hash-trie): … (#101)` with the two trailer lines, re-run Task 5's Step 23 for
     `kermit-ds`, and return to Step 2.
  4. If it is bounded and legitimate, keep it: set that test's bound to exactly the count the
     source accounts for, name the source and its derivation in the test's doc comment in Step
     4's text, and never set it above the old bound (777, or 770 for `presized:1`).

- [ ] **Step 4: Remove the temporary lines; rewrite the docs, names and bounds.**

  Delete the two `eprintln!("TEMP …")` lines added in Step 2.

  **Module doc, lines 1–2.** Before:
  ```rust
  //! Issue #111: no tuple costs a heap allocation of its own between reading a
  //! file and building any structure, except HashTrie's chains.
  ```
  After:
  ```rust
  //! Issue #111: no tuple costs a heap allocation of its own between reading a
  //! file and building any structure. HashTrie's lists were the one exception
  //! until #101's layer 3 threaded them through one link per row.
  ```

  **The HashTrie bullet, lines 21–25.** Before:
  ```rust
  //! - **HashTrie builds** keep a chain `Vec` per distinct tuple, by design until
  //!   #101's layer 3, so they are measured on the same 100 distinct tuples
  //!   repeated once and 100 times: the tables and chains are the same, and only
  //!   the row-id lists grow, by at most ⌈log₂ 100⌉ = 7 growth steps each. A
  //!   per-row allocation adds 9 900.
  ```
  After:
  ```rust
  //! - **HashTrie builds** thread every list through one 4-byte link per row
  //!   (#101's layer 3): Algorithm 2's lists, the leaf chains and the lazy
  //!   pending lists are `Chain`s stored in their buckets, so a build allocates
  //!   per table, never per list. They are measured on the same 100 distinct
  //!   tuples repeated once and 100 times: the tables are the same, and the
  //!   links and a partitioning build's copy are one allocation each at either
  //!   size, so the larger build may allocate no more than the smaller. A
  //!   per-row allocation adds 9 900, and a list grown one push per row, as
  //!   each was before, up to 7.
  ```

  **The first HashTrie test** (lines 364–436, doc and function). Replace both with:
  ```rust
  /// Under the default `root-capacity=grow` every table's size depends on its
  /// keys alone (`tuples` would size the root from the row count), so the
  /// tables, and under lazy expansion each root child's box, are the same at
  /// both repetitions. No list allocates (#101's layer 3): Algorithm 2's lists,
  /// the leaf chains and the lazy pending lists are `Chain`s, a head, a tail and
  /// a length in their bucket, threaded through the trie's links, one 4-byte
  /// link per row. The links are one allocation at either size: `bulk` and the
  /// partitioning builds allocate them at their full length
  /// (`ChainLinks::new`), and `incremental` reserves them once
  /// (`ChainLinks::with_capacity`) and pushes one link per row into that room.
  /// The buffer is the input batch, or a partitioning build's copy, one
  /// allocation sized from the batch.
  ///
  /// The partitioning builds, `radix:2` and `parallel:1`, grow no list per
  /// partition (#101). `partition_rows` counts each partition's rows in a
  /// histogram pass, keeping each row's partition number in a `u16` list sized
  /// from its morsel, then copies the rows and their input positions
  /// (`origin`) into zeroed buffers sized once from the batch, and a partition
  /// is a range of the copy. `radix:2` passes its whole batch as one morsel,
  /// and both inputs fit one of `parallel:1`'s 16 384-row morsels, so the
  /// histogram, the partition numbers, the offsets and the morsel's ranges are
  /// one allocation each at both repetitions. So are the workers, the scratch
  /// roots' tables (sized by their keys), the merge, and `parallel:1`'s
  /// test-hook record.
  ///
  /// Nothing grows with the row count, so the larger build may make no more
  /// allocations than the smaller: 0 extra, where the bound was 777 (111 row-id
  /// lists of up to ⌈log₂ 100⌉ = 7 growth steps each) while the lists were
  /// `Vec`s. Every cell measured equal when the bound was set (the commit
  /// records the counts). The check is one-sided, like the other differential
  /// cells: a one-time initialisation on a path's first run would fall in the
  /// smaller build, which runs first.
  #[test]
  fn hash_trie_builds_allocate_independently_of_their_row_count() {
      let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
      type Measure = fn(HashTrieBuildMode, usize) -> u64;
      let cells: [(&str, Measure, HashTrieBuildMode); 7] = [
          (
              "eager",
              hash_build_allocations::<NoPruning, EagerExpansion>,
              HashTrieBuildMode::Bulk,
          ),
          (
              "pruned",
              hash_build_allocations::<SingletonPruning, EagerExpansion>,
              HashTrieBuildMode::Bulk,
          ),
          (
              "lazy",
              hash_build_allocations::<NoPruning, LazyExpansion>,
              HashTrieBuildMode::Bulk,
          ),
          (
              "pruned lazy",
              hash_build_allocations::<SingletonPruning, LazyExpansion>,
              HashTrieBuildMode::Bulk,
          ),
          (
              "eager",
              hash_build_allocations::<NoPruning, EagerExpansion>,
              HashTrieBuildMode::Incremental,
          ),
          (
              "eager",
              hash_build_allocations::<NoPruning, EagerExpansion>,
              radix,
          ),
          (
              "eager",
              hash_build_allocations::<NoPruning, EagerExpansion>,
              HashTrieBuildMode::Parallel(one_thread()),
          ),
      ];
      for (layout, allocations, mode) in cells {
          let (once, many) = (allocations(mode, 1), allocations(mode, REPEATS));
          assert!(
              many <= once,
              "HashTrie {layout} {mode:?}: {once} allocations for {} rows, {many} for {}: the \
               larger build allocates more, so some row or list allocates",
              KEYS * KEYS,
              REPEATS * KEYS * KEYS
          );
      }
  }
  ```
  The cells are unchanged; the `lists` and `bound` lines go.

  **The presized test** (lines 438–482, doc and function). Replace both with:
  ```rust
  /// `presized:1` under `root-capacity=tuples`, its prerequisite. The root is
  /// sized once from the row count, one allocation at either repetition, and
  /// the children's tables by their keys. No list allocates (#101's layer 3):
  /// the root's chains, the children's and the leaf chains are `Chain`s
  /// threaded through the trie's links, which `from_tuples_partitioned`
  /// allocates once at the row count. Its partitions are whole runs of the
  /// root's 4 096-bucket regions, at most [`ONE_THREAD_PARTITIONS`]: the 100
  /// rows' 256-bucket root is one region and one partition, the 10 000 rows'
  /// 16 384-bucket root four.
  ///
  /// The three extra partitions cost no allocation (#101). `partition_rows`
  /// sizes its histogram, partition numbers, offsets, copy and `origin` once,
  /// and a partition is a range of the copy. `with_runs` and `map_in_runs` size
  /// their run lists once, and each worker's result list holds four entries in
  /// its first allocation, so one partition or four cost the same. The
  /// fixture's ten keys never probe past a region's end, so no row is deferred
  /// to the tail: its queue of deferred ids, the one `Vec` here grown a push at
  /// a time, never allocates.
  ///
  /// So the larger build may allocate no more than the smaller: 0 extra, where
  /// the bound was 770 (110 row-id lists of up to 7 growth steps each) while
  /// the lists were `Vec`s. It measured equal when the bound was set (the
  /// commit records the counts).
  #[test]
  fn hash_trie_presized_build_allocates_independently_of_its_row_count() {
      let config = HashTrieConfig {
          root_capacity: RootCapacity::Tuples,
          ..HashTrieConfig::default()
      };
      let mode = HashTrieBuildMode::Presized(one_thread());
      let measure = |repeats| {
          let (allocations, builds) =
              measure_hash_build::<NoPruning, EagerExpansion>(config, mode, repeats);
          let [build] = builds.as_slice() else {
              panic!("presized:1 ran one build: {builds:?}");
          };
          assert_eq!(build.threads, 1, "presized:1 ran on one thread");
          assert_eq!(build.deferred, Some(0), "no row is deferred to the tail");
          (allocations, build.partition_sizes.len())
      };
      let ((once, once_partitions), (many, many_partitions)) = (measure(1), measure(REPEATS));
      assert_eq!((once_partitions, many_partitions), (1, 4));
      let rows = REPEATS * KEYS * KEYS;
      assert!(
          many <= once,
          "HashTrie presized:1: {once} allocations for {} rows, {many} for {rows}: the larger \
           build allocates more, so some row or list allocates",
          KEYS * KEYS,
      );
  }
  ```
  - The names change because the old ones (`…_per_list_not_per_row`) describe the `Vec` lists;
    no file but this one names them (`rg -n 'allocate_per_list_not_per_row' -g '!target' .`
    prints only these two definitions before the edit, nothing after).
  - `ceil_log2`, `doublings` and `ONE_THREAD_PARTITIONS` stay in use: the CSV reader cell, the
    TreeTrie cells and the presized doc link.
  - If Step 3 kept a legitimate source, its test's `many <= once` becomes `many <= once + N`
    with `let bound = N;` named in its doc, as Step 3 item 4 says.

- [ ] **Step 5: Run.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test tuple_allocation
  ```
  Expected: `test result: ok. 9 passed`, among them
  `hash_trie_builds_allocate_independently_of_their_row_count` and
  `hash_trie_presized_build_allocates_independently_of_its_row_count`. If one fails, its message
  prints both counts: go back to Step 3; never raise the bound past what Step 3 derived.

- [ ] **Step 6: `result_allocation.rs` needs no edit; run it.**

  It measures only joins and scans, over relations built before `allocation_counter::measure`
  starts (`relations::<…>(fan_out)`, `database_with_copies`, `hash_scan_allocations`' trie), and
  asserts equal counts at two result sizes. Task 5 changed what a HashTrie build allocates, which
  it never measures. On its measured paths, `emit_leaf` advances one cursor per relation in a
  `Vec` sized once in `LeafScratch::new` (T3), and `for_each_tuple` walks each chain through
  `ChainLinks::walk`, which allocates nothing. Its lazy cells still measure a second join: a cold
  one expands children, allocating each child's tables, as before. Its doc still holds.
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test result_allocation
  ```
  Expected: `test result: ok`, 0 failed.

- [ ] **Step 7: Lint and format.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  ```
  Expected: clean. Only a test file changed, so `cargo doc` has nothing new to check.

- [ ] **Step 8: Commit, with the measured counts.**

  Fill each `<n>` from the Step 2 table before running the command.
  ```bash
  git add kermit/tests/tuple_allocation.rs
  git commit -m "$(cat <<'EOF'
  test(hash-trie): no list allocates once lists are chains (#101)

  Every HashTrie list is a Chain threaded through the trie's links since
  the chain-links commit, so a build allocates per table and never per
  list: the tables are sized by their keys, the links and a partitioning
  build's copy are one allocation each, and partition_rows' and
  presized:1's run lists are sized once. tuple_allocation.rs's two
  HashTrie cells drop their per-list terms (777 and 770 extra
  allocations allowed) and now require the 10 000-row build to allocate
  no more than the 100-row one. Both are renamed for what they now pin.

  Measured before tightening (allocations, 100 rows / 10 000 rows):
    eager bulk               <n> / <n>
    pruned bulk              <n> / <n>
    lazy bulk                <n> / <n>
    pruned lazy bulk         <n> / <n>
    eager incremental        <n> / <n>
    eager radix:2            <n> / <n>
    eager parallel:1         <n> / <n>
    presized:1 (tuples)      <n> / <n>

  result_allocation.rs needs no change: it measures joins and scans over
  relations built before measuring.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_017PLoRDCeuYmtirxRTMXGJj
  EOF
  )"
  git status --short   # prints nothing
  ```
  If Step 3 found and kept a source, add one paragraph naming it and its bound.

---

### Task 7: Documentation

The spec's Documentation list, plus every doc and doc comment that T1–T6 left naming a
`Vec` per list, a slice of chain ids, `row(i)`, a sort in `radix:K`'s merge (T1 replaced it
with `parallel:N`'s k-way merge) or a `BorrowMutError` in a lazy walk.
- **The parity wording** the HashTrie direction requires (#105):
  - the link per row is the paper's chain pointer (§3.3.2, Figure 3), and Algorithm 2's
    re-threading is its line 5–7 move of each tuple to exactly one child list;
  - two departures are kermit's: 4-byte ids in an array beside the tuples rather than
    8-byte pointers inside them (spec Decision 1), and lists that append, keeping input
    order, where line 7's push read literally would prepend (Decision 2);
  - the 24-byte leaf bucket is kermit's (the paper's bucket is 16 B).
- **No outward-facing step here.** The #105 board and the issue comments are in "After this
  plan", as text for the user to approve.

Each edit below is an exact before/after. Where a line is long, "before" is the exact
substring to replace, or the step names the whole line by its opening words. Check each is
unique with `grep -cF '<before>' <file>` (expect `1`) before editing; where a "before" is
not unique, the step says what anchors it.

Line numbers are at 0e94f30. No earlier task edits a Markdown file, so every Markdown line
number holds. Rust doc comments are mostly rewritten inside T1–T6; Step 1 lists the ones
they may have left, each guarded:
- `grep -cF` on the first line of its "before" prints `1`: no earlier task touched it, so
  apply the edit.
- It prints `0`: an earlier task rewrote it. Read the current text. If it states every fact
  of the "after" text, leave it; if it misses one, replace the current paragraph with the
  "after" text in full.

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (the `# Storage` paragraph, 72–84; doc only)
- Modify: `kermit-ds/src/ds/hash_trie/bulk.rs` (module doc, 30–47, if T5 left it; doc only)
- Modify: `kermit-iters/src/hash_trie.rs` (`leaf_tuples`' doc, 67–71)
- Modify: `kermit-iters/README.md` (the "What lives here" list, after line 12)
- Modify: `docs/data-structures/hash-trie.md` (7, 9–37, 41, 43, 45, 52, after 58, 60, 81, 82, 84, 90–98, after 107, 117–132, 140–154, 158, 163, 164, 167, 234–235, 250–251, 257–258, 261–269, after 337, after 440, 530–535, 561–563, 568–570, after 600, after 677, 685–686, after 706, after 754, after 789)
- Modify: `docs/algorithms/hash-triejoin.md` (37, 49, after 49, 63)
- Modify: `docs/data-structures/parallel-build.md` (45–49, 175–194, 204–214, 245, 260, 272–276, 287–312 (one sentence after 312), 321–339, 341–350, 390, 408–419, after 615)
- Modify: `CLAUDE.md` (80–81, 142, 148, 153, 335)
- Modify: `ARCHITECTURE.md` (147, 149, 215–227, 232–233, 309)
- Modify: `docs/specs/bench-report-schema.md` (after 266)
- Modify: `docs/specs/2026-10-14-hash-trie-chain-links-design.md` (status, 4; a new Amendment 1 at the end)

- [ ] **Step 1: The Rust doc comments the code tasks may have left.** Each is guarded as
  the preamble says, and each is wrapped at 80 columns.

  **`implementation.rs`, the `# Storage` paragraph** (72–84). T5 adds the `links` field and
  the links invariant to the struct doc's `# Invariants`; this paragraph is T7's. Before:
  ```rust
  /// The trie owns its tuples as one row-major buffer ([`Tuples`], #111): the
  /// batch a build is given, then every row `insert` appends. `bulk` and
  /// `incremental` keep the batch as is, in arrival order. `radix:K`,
  /// `parallel:N` and `presized:N` copy it into partition order on the first
  /// attribute's hash (`morsel::partition_rows`, #101) and keep the copy
  /// instead, so each partition's rows are one contiguous range, in input
  /// order. Below the tables everything names a tuple by its [`RowId`]: a
  /// leaf chain is a `Vec<RowId>`, a pruned `Singleton` one `RowId`, an
  /// unexpanded child's pending list a `Vec<RowId>`. The paper's leaves
  /// likewise point into a materialised buffer, partitioned as it is
  /// materialised (VLDB 2020 §3.3.2); its pointers are 8 bytes where these
  /// ids are 4, and it threads each list through the tuples where these are a
  /// `Vec` per list (both kermit's; the second until #101 B2).
  ```
  After:
  ```rust
  /// The trie owns its tuples as one row-major buffer ([`Tuples`], #111): the
  /// batch a build is given, then every row `insert` appends. `bulk` and
  /// `incremental` keep the batch as is, in arrival order. `radix:K`,
  /// `parallel:N` and `presized:N` copy it into partition order on the first
  /// attribute's hash (`morsel::partition_rows`, #101) and keep the copy
  /// instead, so each partition's rows are one contiguous range, in input
  /// order. Below the tables everything names a tuple by its [`RowId`]: a
  /// pruned `Singleton` is one `RowId`, and every list (a leaf chain, an
  /// unexpanded child's pending list, and each of Algorithm 2's per-bucket
  /// lists while a build runs) is a `Chain` threaded through `links`, one
  /// link per row beside the buffer (#101 B2). The paper's leaves likewise
  /// point into a materialised buffer, partitioned as it is materialised, and
  /// thread each list through a chain pointer reserved in each tuple (VLDB
  /// 2020 §3.3.2). Its pointers are 8 bytes inside the tuple where these ids
  /// and links are 4 bytes beside it, and its push prepends where these lists
  /// append, keeping input order (both kermit's).
  ```

  **`bulk.rs`, the module doc's "kermit's" list and identity paragraph** (30–47). Before:
  ```rust
  //! Two things that are kermit's:
  //!
  //! - A list is a `Vec<RowId>` per bucket: 4-byte ids of rows in the trie's
  //!   buffer (`Tuples`, #111). Umbra threads its lists through an 8-byte chain
  //!   pointer reserved in each materialised tuple (§3.3.2); that needs the
  //!   partitioned copy of the buffer (#101 layer 3).
  //! - Line 3's size is a Config value whose default (`grow`) is not the paper's;
  //!   `child-capacity=tuples` at `load-factor=0.8` is the paper's sizing
  //!   exactly. The root's is `root-capacity` (#88), and each child's is
  //!   `child-capacity`: 4 buckets that grow (`grow`), or sized once from its
  //!   list (`tuples`).
  //!
  //! Under `child-capacity=grow`, Algorithm 2 builds the trie the per-tuple
  //! build (`insert_at`, the `incremental` mode, which requires `grow`)
  //! builds, array for array. A table's layout depends only on the order its
  //! new keys arrive. Here every list keeps input order, because a `Vec` push
  //! appends, so each table receives its keys in the order `insert_at` sends
  //! them.
  ```
  After:
  ```rust
  //! Two things that are kermit's:
  //!
  //! - A list is a `Chain` threaded through the trie's `links`, one 4-byte
  //!   link per row of its buffer (`Tuples`, #111; #101 B2). That is the
  //!   paper's chain pointer (§3.3.2), but as an id in an array beside the
  //!   buffer rather than an 8-byte pointer reserved in each tuple. Line 7
  //!   appends at the chain's tail where a literal push would prepend, so
  //!   every list keeps input order. Line 10's extract moves nothing: the next
  //!   level is grouped from the chain's walk, which rewrites each row's link,
  //!   so a row moves to exactly one child list and no list allocates.
  //! - Line 3's size is a Config value whose default (`grow`) is not the paper's;
  //!   `child-capacity=tuples` at `load-factor=0.8` is the paper's sizing
  //!   exactly. The root's is `root-capacity` (#88), and each child's is
  //!   `child-capacity`: 4 buckets that grow (`grow`), or sized once from its
  //!   list (`tuples`).
  //!
  //! Under `child-capacity=grow`, Algorithm 2 builds the trie the per-tuple
  //! build (`insert_at`, the `incremental` mode, which requires `grow`)
  //! builds, array for array, `links` included. A table's layout depends only
  //! on the order its new keys arrive. Here every list keeps input order,
  //! because a push appends, so each table receives its keys in the order
  //! `insert_at` sends them; and every push writes `NIL` into the pushed row's
  //! link, so `links` is a function of the lists alone.
  ```

  **`kermit-iters/src/hash_trie.rs`, `leaf_tuples`' doc** (67–71). Before:
  ```rust
      /// Tuple chain at the current leaf bucket, as a [`LeafRows`] view: the
      /// chain's row ids, in chain order, over the tuple buffer they index.
      /// Returns `Some` iff the current node is a leaf and the current bucket
      /// is occupied; otherwise `None`. The view borrows the iterator, so
      /// reading a chain copies and allocates nothing.
  ```
  After:
  ```rust
      /// Tuple chain at the current leaf bucket, as a [`LeafRows`] view: the
      /// chain's rows, in chain order, over the tuple buffer their ids index.
      /// A hash trie lends its chain linked, walked through its
      /// [`ChainLinks`](crate::ChainLinks); a view that holds its own ids lends
      /// them listed. Returns `Some` iff the current node is a leaf and the
      /// current bucket is occupied; otherwise `None`. The view borrows the
      /// iterator, so reading a chain copies and allocates nothing.
  ```

  Then confirm the code tasks left no other stale doc comment in the files they changed:
  ```bash
  rg -n -e 'Vec<RowId>' -e 'TupleList' -e 'chain_lens' -e 'RefCell<Vec<RowId>>' -e 'shrink_to\(' \
    -e 'BorrowMutError' -e '`Vec` per' -e 'a sort of' \
    kermit-ds/src/ds/hash_trie kermit-iters/src kermit-algos/src/hash kermit/tests/tuple_allocation.rs
  ```
  Expected hits, all true after B2:
  - `kermit-iters/src/tuples.rs`: `Tuples::sort`'s `order: Vec<RowId>`, and the docs of the
    conversion from one `Vec` per tuple (#111's test-fixture form);
  - `kermit-algos/src/hash/selection.rs`: `leaf_ids: Vec<RowId>`, the selection view's own
    filtered ids, lent in the listed form;
  - `kermit-ds/src/ds/hash_trie/parallel.rs`: `fill_runs`' deferred ids (`-> Vec<RowId>`) and
    its test's `tail`, scratch vectors of ids, not lists of the trie;
  - `kermit-ds/src/ds/hash_trie/identity.rs`: the `reverse_rows` test helper's id map;
  - any test that collects a walk or spells out expected ids (`collect::<Vec<RowId>>()`,
    `let expected: Vec<RowId> = …`) in `chain.rs`, `bulk.rs`, `implementation.rs`,
    `hash_trie_iter.rs` or `kermit-iters`;
  - `implementation.rs`: the walks' doc may name `BorrowMutError` as what a re-entrant visit
    did before B2 (T5's wording), and a heap-size test's doc may say no chain holds a `Vec`
    per tuple;
  - `kermit/tests/tuple_allocation.rs`: TreeTrie's "children `Vec` per inner node".

  Any other hit in a doc comment is a leftover: fix it in the wording of this task. A hit in
  code (a type, a call) is a code task's leftover: report it with the task's name, and do not
  edit code here.

- [ ] **Step 2: `docs/data-structures/hash-trie.md`.**

  Line 7, before (substring):
  ```text
  A chain holds the row ids of its tuples; the tuples themselves live once, in the trie's buffer.
  ```
  After:
  ```text
  A chain is a list of its tuples' row ids threaded through the trie's links, one 4-byte link per row beside the buffer; the tuples themselves live once, in the trie's buffer.
  ```

  The representation block (9–37): the whole ```` ```rust ```` block becomes:
  ```rust
  HashTrie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> {
      header:      RelationHeader,
      tuples:      Tuples,            // every stored tuple: arrival or partition order (below); len() = multiset size
      links:       ChainLinks,        // one link per row of `tuples`: the next row in its list, or NIL
      root:        HashTrieNode<P, E>,
      config:      HashTrieConfig,
      _layout:     PhantomData<(H, P, E)>,
  }

  enum HashTrieNode<P: PruningPolicy, E: ExpansionPolicy> {
      Inner(HashTable<HashTrieNode<P, E>>),     // depths 0..arity-1
      Leaf(HashTable<Chain>),                   // depth arity-1: the chains
      Singleton(P::Payload),                    // pruned subtrie, depths 1..arity
      Unexpanded(E::Pending<HashTrieNode<P, E>>), // lazy child, depths 1..arity
  }

  struct ChainLinks(Vec<AtomicU32>);           // kermit-iters; Relaxed loads and stores

  struct Chain {                                // a list threaded through `links`, in input order
      head: RowId,
      tail: RowId,
      len:  NonZeroU32,
  }

  struct LazyChild<N> {                         // E::Pending<N> under LazyExpansion (boxed)
      pending: Cell<Option<Chain>>,             // the rows below the bucket, until a probe takes them
      built:   OnceCell<N>,                     // the table, once a probe reached it
  }

  struct HashTable<V> {
      log2_capacity: u32,
      len: usize,
      buckets: Vec<Option<Entry<V>>>,     // 2^log2_capacity buckets, open addressing
  }

  struct Entry<V> { hash: u64, value: V }
  ```

  Line 41, before (substring):
  ```text
  every child below it keeps its tuples' row ids in `pending` until `HashTrieIter::open` first enters it, and `HashTrie::resolve` then moves them into a table built by Algorithm 2's `build` (`bulk.rs`), one level deep (its own children start unexpanded). `for_each_tuple` and `heap_size_bytes` read `built` if present and `pending` otherwise; `collect_tuples` and `scan_tuples` read the buffer; none of them expands anything.
  ```
  After:
  ```text
  every child below it keeps its tuples as a pending `Chain` until `HashTrieIter::open` first enters it, and `HashTrie::resolve` then takes the chain (`Cell::take`) and re-threads its rows into a table built by Algorithm 2's `build` (`bulk.rs`), one level deep (its own children start unexpanded). The re-threading writes links through `&self`, which is why the links are atomics (spec Decision 3). `for_each_tuple` reads `built` if present and walks `pending` otherwise; `heap_size_bytes` counts the `Box` and `built`, since a pending chain owns no heap; `collect_tuples` and `scan_tuples` read the buffer; none of them expands anything.
  ```

  Line 43, the whole paragraph beginning `**The tuple buffer** (#111, #101).`, becomes:
  ```markdown
  **The tuple buffer** (#111, #101). The trie owns its relation's tuples as one row-major `Tuples` batch: the batch a build is given, then every row `insert` appends. `bulk` and `incremental` keep that batch without a copy, in arrival order. `radix:K`, `parallel:N` and `presized:N` copy it into partition order on the first attribute's hash (`morsel::partition_rows`, the paper's materialisation step, §3.3.2) and keep the copy, so each partition's rows are one contiguous range of the buffer, in input order; the input batch is freed once copied. Below the tables every tuple is named by its `RowId` (`u32`, an index into the buffer): a pruned `Singleton` is one `RowId`, and every list (a leaf chain, a lazy pending list, and each of Algorithm 2's per-bucket lists while a build runs) is a `Chain` threaded through `links`, in input order (#101 B2). `HashTrieIterator::leaf_tuples` lends a chain as a `LeafRows` view over the buffer and the links. Two builds that keep their input own equal buffers, so the `bulk`/`incremental` identity tests compare ids and `links` arrays, after checking the buffers are equal; the partitioning modes are compared by contents, each chain walked through its own trie's links and read through its own buffer (`identity::assert_same_contents`). Reading back: `for_each_tuple` walks the trie depth-first, each chain through `links` (`bench ds` times it, #79); `TupleScan::scan_tuples` (statistics, which need only the multiset) scans the buffer in its row order; `collect_tuples` copies the buffer.

  **The chains** (#101 B2). `links: ChainLinks` (`kermit-iters`) holds one link per row: the id of the next row in that row's list, or `ChainLinks::NIL` (`u32::MAX`, never a row, since a batch holds at most `u32::MAX` rows). A list is a `Chain { head, tail, len }` stored in its bucket ([`chain.rs`](../../kermit-ds/src/ds/hash_trie/chain.rs)). `Chain::start` writes `NIL` into its row's link, and `Chain::push` appends a row by writing two links, the old tail's and the row's own `NIL`. Algorithm 2 moves each row from its parent's list to exactly one child list by rewriting its link, so a row's link is rewritten once per level and ends as its leaf-chain successor. A chain's walk (`ChainWalk`) reads a row's link before it yields the row, so the build may re-thread the rows it is walking, and it stops after `len` rows. `len` serves line 3's `|L|`, the pruning test and `LeafRows::len`; as a `NonZeroU32` it gives `Option<Entry<Chain>>` a niche, so a leaf bucket is 24 B (8 for the hash, 12 for the chain, padded), where it was 32 B plus a heap list. A build therefore allocates per table, never per list: the tables, the buffer (and the radix family's copy), one `links` array, and the scratch structures sized up front.
  ```

  The parity table (45–52): its header row and its last row change; the four rows between
  stay. Before (the header):
  ```text
  | Element (VLDB 2020 §3.3.1–3.3.2, Fig. 3) | kermit since #101 B1 (2026-10-08) |
  ```
  After:
  ```text
  | Element (VLDB 2020 §3.2.2, §3.3.1–3.3.2, Fig. 3) | kermit since #101 B2 (2026-10-14) |
  ```
  Before (the last row, line 52):
  ```text
  | A leaf is a list threaded through an 8-byte chain pointer per tuple | ✗ a `Vec<RowId>` per chain (kermit's), until #101 B2 |
  ```
  After (five rows, the spec's "Parity with the paper"):
  ```text
  | A leaf is a list threaded through a chain pointer per tuple | ✓ a link per tuple (#101 B2), with two departures, both kermit's: a 4-byte id in an array beside the tuple rather than an 8-byte pointer inside it (the design's Decision 1); lists in input order, appended rather than pushed (Decision 2). Before B2, a `Vec<RowId>` per chain |
  | Algorithm 2 moves each tuple to exactly one child list | ✓ by rewriting its link; before B2 it copied the tuple's id into a new `Vec` |
  | No allocation per list | ✓ since #101 B2 |
  | A bucket is 16 B (8-byte hash, 8-byte child slot) | ✗ a leaf bucket is 24 B: the hash and a `Chain` (head, tail and `len`) behind an `Option` (kermit's; 32 B plus a heap list before B2). An inner bucket holds its child's table header inline (#106) |
  | Lazy expansion re-threads a child's tuples on its first probe | ✓ through relaxed atomics (Decision 3); before B2 it moved a `Vec` |
  ```

  Below the parity table, B1's paragraph on the copy's cost ends in a bullet list whose last
  bullet (line 58) ends `so each row is hashed once.`. After that bullet, insert a blank line
  and:
  ```markdown
  The chains cost 4 bytes per row for the trie's life (`links`, counted by `heap_size_bytes`), and one store per push beyond the paper's: the `NIL` written into the pushed row's link, which keeps `links` a function of the lists (kermit's). In return no build allocates per list. A chain step reads two places, the link and then the row, where the paper's in-row pointer reads one cache line; the links are dense, and in the partitioning modes a chain's rows are partition-local (#101 B1). The links are `AtomicU32`, read and written `Relaxed`, so that a lazy trie can re-thread a child's rows inside `HashTrieIter::open`, which holds only `&self`. On x86-64 and AArch64 a relaxed load or store compiles to a plain one; eager tries use the same atomics, which keeps them `Sync`, and what that costs them is measured once (the design's § Measurement).
  ```

  Line 60, before:
  ```text
  Design: [`docs/specs/2026-10-08-hash-trie-partitioned-materialisation-design.md`](../specs/2026-10-08-hash-trie-partitioned-materialisation-design.md).
  ```
  After:
  ```text
  Design: [`docs/specs/2026-10-08-hash-trie-partitioned-materialisation-design.md`](../specs/2026-10-08-hash-trie-partitioned-materialisation-design.md) (#101 B1) and [`docs/specs/2026-10-14-hash-trie-chain-links-design.md`](../specs/2026-10-14-hash-trie-chain-links-design.md) (#101 B2).
  ```

  The Construction table (76–84). Line 81, before:
  ```text
  | lines 4–7, push each tuple onto its bucket's list | `HashTrie::group` |
  ```
  After:
  ```text
  | lines 4–7, push each tuple onto its bucket's list | `HashTrie::group`, through `chain::push_row`: a fresh bucket starts a chain at the row (one link write), any other appends the row at its chain's tail (two). Line 7's push, read literally, prepends; kermit's lists append, so they keep input order |
  ```
  Line 82, before:
  ```text
  | lines 8–12, build each bucket's child from its list | `HashTrie::build_nested`, through `HashTable::map` and `HashTrie::child` |
  ```
  After:
  ```text
  | lines 8–12, build each bucket's child from its list | `HashTrie::build_nested`, through `HashTable::map` and `HashTrie::child`. Line 10's extract moves nothing: the child is grouped from the chain's walk, which reads each row's link before the row is re-threaded |
  ```
  Line 84, the whole row beginning `| the lists themselves |`, becomes:
  ```text
  | the lists themselves | a `Chain { head, tail, len }` per bucket, threaded through `links`, one 4-byte link per row of the trie's tuple buffer (#101 B2): the paper's chain pointer per tuple (§3.3.2), as a 4-byte id in an array beside the buffer rather than an 8-byte pointer inside each tuple (kermit's, after #111's 4-byte ids). Moving a row to a child list rewrites its link, so no list allocates |
  ```

  "Against the per-tuple build" (90–98), before:
  ```markdown
  **Against the per-tuple build.** `insert`, and the `incremental` build
  mode, place one tuple at a time by `insert_at` instead. Under every config
  both accept (`incremental` accepts only `child-capacity=grow`), the two
  builds give the identical trie, because a table's layout depends only on the
  order its new keys arrive and every list keeps input order. One detail keeps
  it byte-identical: under lazy expansion `child` shrinks a one-tuple pending
  list (pruning off) and a two-tuple one (pruning on) to the capacities
  `insert_at` gives them. That is kermit's, for `space`'s sake, not the
  paper's.
  ```
  After:
  ```markdown
  **Against the per-tuple build.** `insert`, and the `incremental` build
  mode, place one tuple at a time by `insert_at` instead. Under every config
  both accept (`incremental` accepts only `child-capacity=grow`), the two
  builds give the identical trie, `links` included. A table's layout depends
  only on the order its new keys arrive, and every list keeps input order,
  because a push appends. Every push writes `NIL` into the pushed row's link,
  so `links` is a function of the lists alone (#101 B2), and a chain owns no
  heap, so no capacity can differ either.
  ```

  Invariants: after the bullet beginning `- **Leaf chains preserve hash collisions.**`
  (line 107), insert:
  ```markdown
  - **Links are a function of the lists** (#101 B2). `links.len() == tuples.len()`, and every row's link is its successor in the one list that holds it, or `NIL`: a `Singleton`'s row and every list's tail hold `NIL`, because every push writes `NIL` into the pushed row's link. Two builds that make the same lists therefore make the same `links`, so `bulk` and `incremental` are array-identical, links included. `insert` pushes one `NIL` link per row it appends, and the per-row build does the same before each `insert_at`; zeroed links would leave a pruned `Singleton`'s row at 0 where `bulk` writes `NIL`. Pinned by `bulk_and_incremental_thread_the_same_links` in [`bulk.rs`](../../kermit-ds/src/ds/hash_trie/bulk.rs), and by `identity::assert_same_trie`, which compares the two `links` arrays whole.
  ```

  The Complexity table (117–132). Each row below is named by its first cell, which is unique
  in the file; replace the whole row.

  The row beginning `` | `insert(tuple)` | O(a) amortized | ``:
  ```text
  | `insert(tuple)` | O(a) amortized | O(a) | per-level: one hash + one probe + at most one resize; amortized O(1) per level. The row is appended to the buffer with a `NIL` link, and the leaf step starts a chain at it or appends it to one: one or two link writes, no allocation (#101 B2) |
  ```
  The row beginning `` | `from_tuples(n)` | ``:
  ```text
  | `from_tuples(n)` | O(n · a) | O(n · a) | Algorithm 2, the default `bulk` build: per level, n hashes and n chain pushes of one or two link writes each (each push moves a row from its parent's list to one child list; the tuples stay in the buffer), no allocation per list (#101 B2), one `links` array of 4 B per row allocated zeroed, and one transient table of chains per inner table (24 B a bucket, the root's the largest; the last attribute's table of chains stays as the `Leaf`). Expected cost holds for input in another `HashTrie`'s iteration order, or any subset of it; the bucket-index invariant names the one order it does not cover |
  ```
  The row beginning `` | `from_tuples` under `radix:K` | ``:
  ```text
  | `from_tuples` under `radix:K` | O(n · a + D · K) | O(n · a) | the bulk build over a partitioned copy: a histogram pass and a scatter copying every row (`partition_rows`, one first-attribute hash per tuple, kept for the scatter), a second first-attribute hash per tuple in the scratch roots, and a k-way merge of the D distinct root keys over the 2^K partitions' entry lists (`radix::build_and_merge`, shared with `parallel:N`: O(D · K) heap operations, where a sort, O(D log D), ran until 2026-10-14); transiently the copy beside the input while it is made, and 4 B per tuple of `origin` and 2 B of partition number; builds an equivalent trie, the bulk trie's contents and root with the rows in partition order |
  ```
  The row beginning `` | `HashTrieIterator::open()` into an unexpanded child (lazy) | ``:
  ```text
  | `HashTrieIterator::open()` into an unexpanded child (lazy) | O(k) first time, O(1) after | O(k) | builds the child's one-level table from its `k` pending tuples, re-threading each row's link into the table's chains through `&self` (`HashTrie::resolve`); later `open`s find it built |
  ```
  The row beginning `` | `insert(tuple)` under `LazyExpansion` | ``:
  ```text
  | `insert(tuple)` under `LazyExpansion` | O(1) amortized | O(a) | one root-level hash and probe, then the row added to the child's pending chain (one or two link writes); recurses only into a child a probe has already expanded |
  ```
  The row beginning `` | `HashTrieIterator::leaf_tuples()` | ``:
  ```text
  | `HashTrieIterator::leaf_tuples()` | O(1) | | a `LeafRows` view of the current bucket's chain in its linked form, the chain's head and length over the trie's buffer and links (a pruned level lends the listed form, its one row id); copies nothing. Reading it walks the chain, one link load and one row per step |
  ```
  The row beginning `` | `HeapSize::heap_size_bytes()` | ``:
  ```text
  | `HeapSize::heap_size_bytes()` | O(node count) | | the buffer (capacity × 8 B) and the links (capacity × 4 B), plus a recursive walk summing each `HashTable` shell (a chain lives in its bucket and owns no heap), and under lazy expansion each unexpanded child's `Box<LazyChild>` (`own_heap_bytes`) and, once built, its table. A tuple is counted once, in the buffer, plus its 4-byte link: at arity 2 that is 16 B plus 4, where a `Vec` per tuple cost 40 B before #111 and each chain added a heap list of 4-byte ids before #101 B2 |
  ```
  The row beginning `` | `for_each_tuple(visit)` | ``:
  ```text
  | `for_each_tuple(visit)` | O(n) | O(a) stack | depth-first walk of the trie lending each stored tuple: each chain walked through `links` (a lazy child's pending chain likewise), each pruned `Singleton` through its id, every row read from the buffer; allocates nothing per tuple, so `bench ds` times it (issue #79). `collect_tuples()` copies the buffer, in its row order, and `TupleScan::scan_tuples` scans it in place: neither walks the trie |
  ```

  The worked micro-example (140–154). The whole ```` ``` ```` block under "Tuples `(1, 2),
  (1, 3), (2, 4)`" becomes:
  ```text
  HashTrie {
    arity: 2,
    tuples: [1, 2, 1, 3, 2, 4],          // rows 0, 1, 2, row-major
    links:  [NIL, NIL, NIL],             // every row is the tail of its chain
    root: Inner(HashTable {
      bucket[i₁]: Some(Entry { hash: h(1), value: Leaf(HashTable {
        bucket[j₁]: Some(Entry { hash: h(2), value: Chain { head: 0, tail: 0, len: 1 } }),   // row 0 = (1, 2)
        bucket[j₂]: Some(Entry { hash: h(3), value: Chain { head: 1, tail: 1, len: 1 } }),   // row 1 = (1, 3)
      })}),
      bucket[i₂]: Some(Entry { hash: h(2), value: Leaf(HashTable {
        bucket[j₃]: Some(Entry { hash: h(4), value: Chain { head: 2, tail: 2, len: 1 } }),   // row 2 = (2, 4)
      })}),
    })
  }
  ```
  Then, directly after the block's closing fence (before `That is the unpruned shape`),
  insert a blank line and:
  ```markdown
  The build re-threads row 0. Grouping the root starts `h(1)`'s chain at row 0 (link 0 = `NIL`), appends row 1 to it (link 0 = 1, link 1 = `NIL`) and starts `h(2)`'s at row 2 (link 2 = `NIL`). Building `h(1)`'s child walks that chain: it reads row 0's link before regrouping row 0, then starts one chain per second value, which writes `NIL` into links 0 and 1. No list was allocated.
  ```
  Line 158, before (substring):
  ```text
  bucket `i₁` holds `Unexpanded { pending: [0, 1] }` and bucket `i₂` holds `Unexpanded { pending: [2] }`. Step 2 below builds the first child's `Leaf` table from its two rows (exactly the table shown above) before pushing the frame,
  ```
  After:
  ```text
  bucket `i₁` holds `Unexpanded { pending: Chain { head: 0, tail: 1, len: 2 } }` and bucket `i₂` holds `Unexpanded { pending: Chain { head: 2, tail: 2, len: 1 } }`, with links `[1, NIL, NIL]`. Step 2 below takes the first child's chain and re-threads its two rows into its `Leaf` table (exactly the table shown above; links become `[NIL, NIL, NIL]`) before pushing the frame,
  ```
  Step 2 (line 163), before (substring):
  ```text
  `leaf_tuples()` returns a `LeafRows` over ids `[0]`, whose one row is `(1, 2)`.
  ```
  After:
  ```text
  `leaf_tuples()` returns a linked `LeafRows` over the chain that starts at row 0, of length 1, whose one row is `(1, 2)`.
  ```
  Step 3 (line 164), before (substring):
  ```text
  `leaf_tuples()` returns a `LeafRows` over ids `[1]`, whose one row is `(1, 3)`.
  ```
  After:
  ```text
  `leaf_tuples()` returns a linked `LeafRows` over the chain that starts at row 1, whose one row is `(1, 3)`.
  ```
  Step 6 (line 167), before (substring):
  ```text
  `leaf_tuples()` returns a `LeafRows` over ids `[2]`, whose one row is `(2, 4)`. (Under `SingletonPruning`: a `Singleton` frame at depth 1 over row 2, with the same `key()` and `leaf_tuples()`.)
  ```
  After:
  ```text
  `leaf_tuples()` returns a linked `LeafRows` over the chain that starts at row 2, whose one row is `(2, 4)`. (Under `SingletonPruning`: a `Singleton` frame at depth 1 over row 2, with the same `key()`, and a `leaf_tuples()` lending the same row in the listed form.)
  ```

  Singleton pruning's **Measured effect** (225–235), before (substring, lines 234–235):
  ```text
      untested; use the `ds_layout_hasher × ds_layout_pruning` pivot in
      kermit-lab.
  ```
  After:
  ```text
      untested; use the `ds_layout_hasher × ds_layout_pruning` pivot in
      kermit-lab. *(2026-10-14, #101 B2: measured while every leaf chain was
      a `Vec`, so the levels pruning removes included a list allocation that
      no build makes since B2; these savings predate it.)*
  ```

  Lazy child expansion, **Type-level** (250–251), before:
  ```text
      - under `lazy`, a `Box<LazyChild>` with the pending row ids in a
        `RefCell` and the built table in a `OnceCell`;
  ```
  After:
  ```text
      - under `lazy`, a `Box<LazyChild>` with the pending chain in a `Cell`
        (taken when the child is built) and the built table in a `OnceCell`;
  ```
  Then (257–258), before:
  ```text
      Lazy tries are `!Sync` (still `Send`): a parallel prober would need a
      different cell. The payload is generic over the node type rather than
  ```
  After:
  ```text
      Lazy tries are `!Sync` (still `Send`): a parallel prober would need a
      different cell. Both kinds keep their links as atomics (#101 B2), so
      eager tries stay `Sync`. The payload is generic over the node type rather than
  ```
  **Who expands** and **With pruning** (261–269), before:
  ```text
    - **Who expands:** only `HashTrieIter::open`, through `HashTrie::resolve`.
      `for_each_tuple` and `heap_size_bytes` read the pending list;
      `collect_tuples`, `scan_tuples`, `project` and the Parquet round-trip
      read the buffer; none of them expands anything. A `for_each_tuple`
      visitor that opens an iterator on the same trie and reaches a child
      being visited panics (`BorrowMutError`).
    - **With pruning:** a bucket with one tuple below it is a `Singleton` and
      never expands; two or more make an `Unexpanded` list, the evicted
      singleton tuple first.
  ```
  After:
  ```text
    - **Who expands:** only `HashTrieIter::open`, through `HashTrie::resolve`,
      which takes the pending chain and re-threads its rows into the child's
      table through `&self`.
      `for_each_tuple` walks the pending chain, and `heap_size_bytes` counts
      the `Box` (a chain owns no heap); `collect_tuples`, `scan_tuples`,
      `project` and the Parquet round-trip read the buffer; none of them
      expands anything. A `for_each_tuple` visitor that opens an iterator on
      the same trie and expands a child the walk is still reading re-threads
      rows under the walk, which may then lend wrong rows or panic (before
      #101 B2 it panicked with `BorrowMutError`); nothing in kermit does that.
    - **With pruning:** a bucket with one tuple below it is a `Singleton` and
      never expands; two or more make an `Unexpanded` pending chain, the
      evicted singleton tuple first.
  ```
  Lazy **Measured effect**, after the bullet `      - The numbers above predate #107.`
  (line 337), insert:
  ```text
      - *Before #101 B2* (2026-10-14): every number above was measured while
        each pending list and leaf chain was a `Vec`. A pending list is now a
        chain in its child's `LazyChild`, threaded through the trie's links,
        and expansion re-threads it, so lazy `insertion`, `space` and
        `iteration` all predate B2.
  ```

  Child capacity's **Measured effect**, after the bullet ending
  `child's |L| equals its D.` (line 440), insert:
  ```text
      - *(2026-10-14, #101 B2: measured while every list was a `Vec`; no
        list allocates since B2, so these `insertion` ratios predate it.)*
  ```

  The radix build's **Cost** (530), before:
  ```text
  - a sort of the D merge entries;
  ```
  After:
  ```text
  - the k-way merge of the D merge entries over the 2^K partitions' entry
    lists, `parallel:N`'s merge (`radix::build_and_merge` serves both; a sort
    of all D entries until 2026-10-14);
  ```
  Same list (531–535), before:
  ```text
  - transient memory: the copy beside the input while it is made, 2 bytes
    per tuple of partition number (between the two passes), 4 bytes per
    tuple of `origin` (freed before the merge, once the scratch roots hold
    each key's first input position; `presized:N` frees it after its tail),
    and the scratch tables.
  ```
  After:
  ```text
  - transient memory: the copy beside the input while it is made, 2 bytes
    per tuple of partition number (between the two passes), 4 bytes per
    tuple of `origin` (freed before the merge, once the scratch roots hold
    each key's first input position; `presized:N` frees it after its tail),
    and the scratch tables. The scratch roots' chains are threaded through
    the trie's own `links` and move into the real root unchanged, since
    their ids are global (#101 B2).
  ```

  The presized build (561–563), before (substring):
  ```text
  one partition per contiguous run of regions of the presized root, and each
  worker pushes the id of every row of its partition, one contiguous range
  of the copy, onto its bucket's list.
  ```
  After:
  ```text
  one partition per contiguous run of regions of the presized root, and each
  worker appends every row of its partition, one contiguous range of the
  copy, to its bucket's chain, threading the row's link.
  ```
  Same paragraph (568–570), before (substring):
  ```text
  built by Algorithm 2 a run of buckets at a time, the runs handed to
  whichever worker is free (how the recursion is spread over threads is
  kermit's choice; the paper is silent).
  ```
  After:
  ```text
  built by Algorithm 2 a run of buckets at a time, the runs handed to
  whichever worker is free (how the recursion is spread over threads is
  kermit's choice; the paper is silent). A run's worker re-threads the rows
  of its buckets' chains, including a deferred key's rows, which may lie in
  another run's partition; a row is in one list at a time, so each link
  still has one writer (#101 B2).
  ```

  Build modes **Tests**: after the bullet that ends
  `` `SizedChildren` and `PaperSizing` join suites. `` (line 600), insert:
  ```markdown
    - `bulk_and_incremental_thread_the_same_links` in `bulk.rs` (the two
      builds' `links` arrays are equal, #101 B2); the `chain.rs` tests
      (`start` and `push` keep input order, every chain ends in `NIL`,
      `rethreading_a_chain_visits_every_row`) and its size pin of the 24-byte
      leaf bucket; `resolve_rethreads_the_pending_chain_through_a_shared_reference`
      in `implementation.rs` (lazy expansion re-threads through `&self`); and
      `emit_leaf_resets_each_cursor_on_carry` in `hash_triejoin.rs` (the leaf
      product over chains of lengths 1, 2 and 3).
  ```

  **Measured effect of `bulk`** (670–677), after the bullet
  `  - The geomean over 13 relations is 1.02×. `space` is identical.` (line 677), insert:
  ```markdown
    - *(2026-10-14, #101 B2: measured while every list was a `Vec`. B2
      threads the lists through per-row links, which removes the per-list
      allocations #107's reading blames for the `price` ratio, so these
      ratios predate it.)*
  ```

  **Measured effect, earlier modes** (678–686), before (substring):
  ```text
    `radix:K`, `parallel:N` and `presized:N` copy the rows into partition
    order first.)* On inputs that arrive grouped
  ```
  After:
  ```text
    `radix:K`, `parallel:N` and `presized:N` copy the rows into partition
    order first. 2026-10-14, #101 B2: and every chain was a `Vec` of its own,
    where today's are threaded through per-row links.)* On inputs that arrive grouped
  ```

  `#### Radix build A/B` (702–706): insert after the `> **2026-10-08 (#101 B1):**` note
  directly below that heading (line 706, beginning
  `> **2026-10-08 (#101 B1):** These arms also partitioned row ids and left the rows in arrival order. Today's `radix:K` copies`),
  separated by a blank line:
  ```markdown
  > **2026-10-14 (#101 B2):** These arms also kept every leaf chain as a `Vec` of its own. Today's chains are threaded through one 4-byte link per row (`ChainLinks`), so no list allocates and `space` counts the links instead; these numbers do not measure a post-B2 build.
  ```

  `#### Radix build A/B, shuffled input` (750–754): insert after the
  `> **2026-10-08 (#101 B1):**` note directly below that heading (line 754, beginning
  `> **2026-10-08 (#101 B1):** These arms also partitioned row ids and left the rows in arrival order. #101's locality run`),
  separated by a blank line:
  ```markdown
  > **2026-10-14 (#101 B2):** These arms also kept every leaf chain as a `Vec` of its own. Today's chains are threaded through per-row links beside the buffer, so a build allocates per table, never per list; these numbers do not measure a post-B2 build.
  ```

  `#### Algorithm 2 A/B (#107)` (789): insert directly below the heading, before
  `The replicated measurement of #107`, with a blank line on each side:
  ```markdown
  > **2026-10-14 (#101 B2):** Every list in this record was a `Vec<RowId>`: each bucket's list at every level of `bulk`'s grouping, each leaf chain and each lazy pending list was a heap allocation of its own, the cost the *All keys distinct* reading below names. Today's lists are chains threaded through one 4-byte link per row, so `bulk` allocates per table, never per list. These `insertion`, `space` and `iteration` numbers do not measure a post-B2 HashTrie; TreeTrie's do.
  ```

  Check that the three tables this step edited still render: read the parity table, the
  Construction table and the Complexity table once in a Markdown preview, or count each
  edited row's `|` against its header's. A stray `|` breaks a table silently.

**Chunk boundary (C07a ends here; C07b starts).** C07a edits doc comments and `hash-trie.md`
only, so the tree still compiles; it commits nothing. C07b continues in the same working tree
and makes Task 7's one commit (Step 11), staging C07a's files too.

- [ ] **Step 3: `docs/algorithms/hash-triejoin.md`.**

  Line 37, before:
  ```text
  - Lines 16–19 → `fn emit_leaf(iters, predicate_variables, scratch, emit)`.
  ```
  After:
  ```text
  - Lines 16–19 → `fn emit_leaf(iters, predicate_variables, scratch, emit)`, an odometer of one `LeafCursor` per relation, advanced by `fn advance_cursor(iters, cursor)`.
  ```

  Line 49, before (substring):
  ```text
  as a `LeafRows` view (the chain's row ids over its relation's tuple buffer, #111) whose `row(i)` is a slice of that buffer, and the cross-product cursor and output row are scratch buffers allocated once per join, so emitting a tuple allocates nothing.
  ```
  After:
  ```text
  as a `LeafRows` view (the chain over its relation's tuple buffer, #111, walked through the trie's links since #101 B2), whose rows are slices of that buffer, and the cross-product cursors and output row are scratch buffers allocated once per join, so emitting a tuple allocates nothing.
  ```

  After line 49 (the bullet beginning `- **The result is streamed, not materialised.**`),
  insert:
  ```markdown
  - **The leaf product walks cursors, never indices** (#101 B2). A trie's leaf chain is threaded through its links, so it has no random access: `row(i)` would cost O(i). `LeafScratch` therefore holds one `LeafCursor` per relation, an index into a view's own list of ids or the current row's id in a trie's chain, and `emit_leaf` runs Algorithm 3's lines 17–19 as an odometer over them:
    - if any relation's chain `is_empty()`, the product is empty and it returns;
    - otherwise every cursor starts at its chain's `first()`;
    - a candidate is the row `row_at(cursor[k])` of each relation k;
    - `advance_cursor` advances `cursor[0]`; when a chain's `advance` runs past its last row, that cursor is reset to `first()` and the carry moves to the next relation; the product ends when every digit has overflowed.

    So every combination is visited once, in the order the index odometer visited it, and the first relation's chain is walked again for each step of the others. The cursors are sized once in `LeafScratch::new`, so the walk allocates nothing per result row (`kermit/tests/result_allocation.rs`). Pinned by `emit_leaf_resets_each_cursor_on_carry` (three relations with chains of lengths 1, 2 and 3): without the reset on carry, candidates are dropped.
  ```

  The Complexity table's `emit_leaf` row (63), before:
  ```text
  | `emit_leaf` | O(∏ chain_size · arity) | cross-product over leaf chains, verify each candidate in O(arity) |
  ```
  After:
  ```text
  | `emit_leaf` | O(∏ chain_size · arity) | cross-product over leaf chains, verify each candidate in O(arity); each cursor step is one link load and one row read |
  ```

- [ ] **Step 4: `docs/data-structures/parallel-build.md`.**

  The shared-steps pointer (45–49), before:
  ```markdown
  The TreeTrie build is `TreeTrie::build_parallel` in
  `kermit-ds/src/ds/tree_trie/implementation.rs`.
  The HashTrie builds are `parallel::fill_root` (`parallel:N`) and
  `parallel::fill_presized_root` (`presized:N`), both in
  `kermit-ds/src/ds/hash_trie/parallel.rs`.
  ```
  After:
  ```markdown
  The TreeTrie build is `TreeTrie::build_parallel` in
  `kermit-ds/src/ds/tree_trie/implementation.rs`.
  The HashTrie builds are `parallel::fill_root` (`parallel:N`) and
  `parallel::fill_presized_root` (`presized:N`), both in
  `kermit-ds/src/ds/hash_trie/parallel.rs`. `parallel:N` shares its
  partition, scratch-root and merge steps with `radix:K`
  (`radix::partition_by_first_hash`, `radix::build_and_merge`).
  ```

  The exact build's pseudocode (175–194): the whole ```` ```text ```` block becomes:
  ```text
  parallel_build(tuples, N):
      check arities                                     // the serial check, on the caller
      links = n zeroed links                            // one per row, shared by every worker (#101 B2)
      if no tuples: return the empty root               // no worker started
      P = 4·N rounded up to a power of two; b = log₂ P
      copy = partition_rows(tuples, morsels of 16 384)  // step 1, N workers, two passes
          // row t goes to partition H(t[0]) >> (64 − b): a histogram pass keeps
          // each row's partition, then a scatter copies the rows into partition
          // order, with origin[id] = the copied row's input position
      free tuples                                       // the copy is the trie's buffer
      lists = dispatch(non-empty partitions' id ranges): // step 2, N workers
          group into a scratch root in id order (input order within a partition),
              appending each id to its key's chain through links,
              noting (origin[id], hash) whenever it gains a key
          build each key's child from its chain (Algorithm 2), re-threading its ids
          take its entries out in that order
          -> [(first input position, hash, subtrie or chain)]
      root = empty                                       // step 3, the caller
      k-way merge the lists by first input position:
          insert each (hash, value) into root
  ```

  The **Build** and **Merge** bullets (204–214), before:
  ```markdown
  - **Build.** A worker builds a whole partition, one contiguous range of the
    copy, into a scratch root of the real root's kind, by Algorithm 2, as
    the bulk build does. It then moves the scratch root's entries out in the
    order their keys arrived, each tagged with its key's first input
    position.
  - **Merge.** The calling thread inserts every entry into the real root in
    the order its key first appeared in the input, by `origin`. Ids in the
    copy run partition by partition, so merging on them would insert each
    partition's keys in turn. Each list is already in that order, so a k-way
    merge over at most P lists suffices. (The radix build, single-threaded,
    sorts its entries instead.)
  ```
  After:
  ```markdown
  - **Build.** A worker builds a whole partition, one contiguous range of the
    copy, into a scratch root of the real root's kind, by Algorithm 2, as
    the bulk build does. It then moves the scratch root's entries out in the
    order their keys arrived, each tagged with its key's first input
    position. Grouping threads each row onto its key's chain, and each
    child's build re-threads it (#101 B2). Only this worker writes those
    rows' links: a row is in one list at a time, so each link has one
    writer. The links are `AtomicU32`, accessed `Relaxed`, which makes that
    race-free without proving which partition owns which links, and the
    scoped threads' spawn and join order every worker's writes before
    anything reads the chains again.
  - **Merge.** The calling thread inserts every entry into the real root in
    the order its key first appeared in the input, by `origin`. Ids in the
    copy run partition by partition, so merging on them would insert each
    partition's keys in turn. Each list is already in that order, so a k-way
    merge over at most P lists suffices. It moves `Chain` values and
    subtries unchanged: their ids are global. (The radix build,
    single-threaded, runs the same merge over its 2^K partitions' lists;
    `radix::build_and_merge` serves both.)
  ```

  Line 245, before (substring):
  ```text
  still `!Sync`, and no trie is ever shared between threads.
  ```
  After:
  ```text
  still `!Sync`, and no trie is ever shared between threads (the workers
  share only the build's `links` array, by reference; `AtomicU32` is `Sync`).
  ```

  The exact build's complexity, Build row (260), before:
  ```text
  | Build | O(n · a) expected probes and inserts, split across partitions, plus O(D) to take the entries out | N workers |
  ```
  After:
  ```text
  | Build | O(n · a) expected probes, inserts and link writes, split across partitions, plus O(D) to take the entries out | N workers |
  ```

  Memory (272–276), before:
  ```markdown
  Memory: while the copy is made, the input and the copy are both held, so
  for that stretch the tuples are held twice. `origin` adds 4 bytes per
  tuple until the scratch roots are built (it is freed before the merge,
  since each key's first input position is already in its entry), and the
  partition numbers 2 bytes per tuple between the two passes.
  ```
  After:
  ```markdown
  Memory: while the copy is made, the input and the copy are both held, so
  for that stretch the tuples are held twice. `origin` adds 4 bytes per
  tuple until the scratch roots are built (it is freed before the merge,
  since each key's first input position is already in its entry), and the
  partition numbers 2 bytes per tuple between the two passes. The trie's
  `links` add 4 bytes per tuple for its life, allocated zeroed once before
  the copy (#101 B2); no list allocates.
  ```

  The exact build's worked example: after the paragraph ending
  `Had the merge keyed on ids, it would have inserted 2 (id 0) first.` (line 312), insert a
  blank line and:
  ```markdown
  Partition 2's worker alone writes links 1–3. Grouping starts key 3's chain
  at id 1 and key 1's at id 2, then appends id 3 to key 1's (link 2 = 3,
  link 3 = `NIL`); building key 1's child re-threads ids 2 and 3 into one
  chain per second value (link 2 = `NIL`). Partition 1's worker writes link
  0 only.
  ```

  The presized pseudocode (321–339): the whole ```` ```text ```` block becomes:
  ```text
  presized:N (requires root-capacity=tuples):
      check arities; no tuples → the empty root, no worker started
      links = n zeroed links                             // one per row, shared by every worker (#101 B2)
      root = 2^p buckets, p = config.root_log2_capacity(n)    // #88; never grows here
      P = 4·N rounded up to a power of two, capped at the region count
      copy = partition_rows(tuples, morsels of 16 384)   // step 1, N workers, two passes
          // row t goes to partition home_bucket(H(t[0]), p) >> (p − log₂ P):
          // partition k is run k of the root, and ids offsets[k]..offsets[k+1]
          // of the copy; origin[id] is the copied row's input position
      free tuples                                        // the copy is the trie's buffer
      root.with_runs(P, REGION_BUCKETS, |runs|           // step 2, N workers
          dispatch over (partition k's id range, run k):
              for each id, in order (input order within the partition):
                  push_in_run(run k, id)           lines 6–7, onto its bucket's chain
                      off the region's end → defer
      tail: sort the deferred ids by origin, push each by ordinary probing
      free origin and offsets                            // nothing reads them past the tail
      children: dispatch over the P runs: map a run's chains to children (`child`, lines 8–12),
          re-threading their ids                         // a tail key's ids may lie in another partition
  ```

  The paragraph after it (341–350): after its last sentence, `…and does not say how a probe
  that crosses a partition's end is handled.` (line 350), append (same paragraph):
  ```markdown
  Every row is in one list at a time, so each link has one writer however
  the runs are spread (#101 B2). A run's worker re-threads its buckets'
  chains, and a deferred key's chain, which the tail may have placed in a
  bucket of the next run's region, holds rows of the key's home partition:
  that run's worker re-threads rows of a partition it did not group. The
  links are relaxed atomics, and the scoped threads' spawn and join order
  each step's writes before the next step reads them.
  ```

  The presized complexity table's Group row (390), before:
  ```text
  | Group (lines 4–7) | O(n) expected, one push per tuple | N workers |
  ```
  After:
  ```text
  | Group (lines 4–7) | O(n) expected, one chain push (one or two link writes) per tuple | N workers |
  ```

  The presized worked example (408–419). Line 410, before (substring):
  ```text
  | key 1 is new: its list starts with id 0 | 3 |
  ```
  After:
  ```text
  | key 1 is new: its chain starts at id 0 (link 0 = `NIL`) | 3 |
  ```
  Line 412, before (substring):
  ```text
  | key 1 is found at 3: id 2 is pushed onto its list | 3 |
  ```
  After:
  ```text
  | key 1 is found at 3: id 2 is appended to its chain (link 0 = 2, link 2 = `NIL`) | 3 |
  ```
  Lines 417–419, before:
  ```markdown
  lands at 4. The children are then built a run at a time: run 0's task
  builds key 1's from ids `[0, 2]` (`[1,a] [1,d]`), and run 1's builds key
  2's and key 3's.
  ```
  After:
  ```markdown
  lands at 4. The children are then built a run at a time: run 0's task
  builds key 1's by walking its chain, ids 0 then 2 (`[1,a] [1,d]`), and
  run 1's builds key 2's and key 3's. Key 2's chain sits in bucket 4, in
  run 1's region, but holds id 1, a row of partition 0: run 1's worker
  re-threads it, and no other task touches link 1 while the children are
  built.
  ```

  The HashTrie scaling record (611–615): insert after the `> **2026-10-08 (#101 B1):**` note
  directly below `### Scaling result: HashTrie, presized and grown (2026-10-06)` (line 615),
  separated by a blank line:
  ```markdown
  > **2026-10-14 (#101 B2):** These arms also kept every list as a `Vec` of its own. Today's lists are chains threaded through one 4-byte link per row of the trie, which the workers write through a shared reference, so no list allocates and `space` counts the links; these numbers do not measure a post-B2 build either.
  ```

- [ ] **Step 5: `CLAUDE.md`.**

  The workspace map (80–81), before:
  ```text
  kermit-iters    → Core iterator traits (LinearIterator, TrieIterator) and the tuple batch
                    (Tuples, RowId, LeafRows; #111). Zero dependencies.
  ```
  After:
  ```text
  kermit-iters    → Core iterator traits (LinearIterator, TrieIterator) and the tuple batch
                    (Tuples, RowId, LeafRows, ChainLinks; #111, #101). Zero dependencies.
  ```

  Line 142, the whole bullet beginning `- **Tuples** / **RowId** / **LeafRows**`, becomes:
  ```markdown
  - **Tuples** / **RowId** / **LeafRows** / **ChainLinks** (`kermit-iters/src/{tuples,leaf_rows,chain_links}.rs`; `kermit-ds` re-exports `Tuples`, `RowId`, `LeafRows` and `LeafCursor`; #111, #101): a relation's tuples as one row-major buffer of one arity, with an explicit row count (a nullary batch keeps its count) and at most `u32::MAX` rows, each addressed by a 4-byte `RowId`. `read_csv` / `read_parquet` return one at its exact size; `Tuples::sort` is the one sort of both sorted tries; `HashTrie` keeps its batch, or under `radix:K` / `parallel:N` / `presized:N` its partition-ordered copy (#101 B1), and beside it one `ChainLinks` array: a 4-byte link per row, the next row in that row's list or `ChainLinks::NIL` (#101 B2). Every HashTrie list (Algorithm 2's per-bucket lists, the leaf chains, lazy pending lists, the radix family's scratch lists) is a `Chain { head, tail, len }` threaded through those links in input order, so no build allocates per list; a pruned `Singleton` holds one row id. The links are `AtomicU32`, accessed `Relaxed`, only so that a lazy trie can re-thread a child's rows inside `HashTrieIter::open` through `&self`; each link has one writer, in the parallel builds too, and no `unsafe` is involved. `LeafRows<'a>` is the `Copy` view of one chain that `HashTrieIterator::leaf_tuples` returns, in one of two forms: linked (a trie's chain: its head and length over the buffer and the links) or listed (a slice of ids: a singleton frame, the constant view, and the selection view, which holds its own filtered ids). It is read by walking (`len`, `iter()`, `ids()`, an iterator, or a `LeafCursor` through `first` / `advance` / `row_at`, as `emit_leaf`'s odometer does); there is no random access, since `row(i)` would be O(i) on a chain. Its rows borrow the buffer, so the leaf product allocates nothing. `to_vecs()` (both `Tuples` and `LeafRows`) is for tests and diagnostics.
  ```

  Line 148 (the `BuildModeRelation` bullet), before (substring):
  ```text
  HashTrie's `bulk` and `incremental` are array-identical.
  ```
  After:
  ```text
  HashTrie's `bulk` and `incremental` are array-identical, their `links` arrays included (#101 B2).
  ```

  Line 153 (the `TupleScan` bullet), before (substring):
  ```text
  `HashTrie::for_each_tuple`, which `bench ds` times (#79), still walks the trie, reading rows through their ids.
  ```
  After:
  ```text
  `HashTrie::for_each_tuple`, which `bench ds` times (#79), still walks the trie, each chain through its links.
  ```

  Line 335 (the lazy-joins gotcha), before (substring):
  ```text
  `HashTrieIter::open` builds each child it first reaches (`OnceCell` through `&self`),
  ```
  After:
  ```text
  `HashTrieIter::open` builds each child it first reaches (`OnceCell` through `&self`, re-threading the child's rows through the trie's atomic links, #101 B2),
  ```

  The two `tuple_allocation.rs` mentions (lines 182 and 257) say a build "allocates nothing
  per input row". That stays true, and neither says HashTrie's chains allocate, so they are
  unchanged; the stale claim, "except HashTrie's chains", is in that file's own module doc,
  which T6 rewrote. Confirm:
  ```bash
  grep -n 'tuple_allocation' CLAUDE.md
  grep -n 'except HashTrie' kermit/tests/tuple_allocation.rs
  ```
  Expected: the two `CLAUDE.md` lines as described, and no hit in `tuple_allocation.rs`. A
  hit there is T6's to fix; report it rather than editing a test file here.

- [ ] **Step 6: `ARCHITECTURE.md`.**

  Line 147, before (substring):
  ```text
  HashTrie keeps the batch and stores row ids.
  ```
  After:
  ```text
  HashTrie keeps the batch, stores row ids, and threads its lists through one 4-byte link per row (`ChainLinks`, `kermit-iters/src/chain_links.rs`, #101).
  ```

  Line 149, the whole paragraph beginning `` `LeafRows<'a>` (`kermit-iters/src/leaf_rows.rs`) ``,
  becomes:
  ```markdown
  `LeafRows<'a>` (`kermit-iters/src/leaf_rows.rs`) is a `Copy` view of one hash-trie leaf chain over the buffer its row ids index, in one of two forms: *linked*, a trie's chain (its head and length, walked through the trie's `ChainLinks`), or *listed*, a slice of ids (a pruned level's one id, and the constant and selection views, which hold their own). `HashTrieIterator::leaf_tuples` returns it. It is read by walking, never by index: `iter()`, `ids()`, or a `LeafCursor` (`first`, `advance`, `row_at`), one per relation in HashTriejoin's leaf product. The rows it lends borrow the buffer, not the view, so the leaf product reads its candidates without copying or allocating.
  ```
  (The heading `#### Tuple batches: `Tuples` and `LeafRows`` stays: line 139 links to its
  anchor.)

  The HashTrie sketch (215–227): the whole ```` ```rust ```` block becomes:
  ```rust
  struct HashTrie<H: HashStrategy = SipHashStrategy> {
      header: RelationHeader,
      tuples: Tuples,         // every stored tuple, arrival or partition order (below); len() is the multiset count
      links: ChainLinks,      // one 4-byte link per row: the next row in its list, or NIL
      root: HashTrieNode,
      _hasher: PhantomData<H>,
  }

  enum HashTrieNode {
      Inner(HashTable<HashTrieNode>),    // child nodes
      Leaf(HashTable<Chain>),            // chains: lists of rows in `tuples`, threaded through `links`
  }
  ```

  The **Leaves reach whole tuples** bullet (232), before (substring):
  ```text
  The trie keeps its relation's `Tuples` batch, and a chain holds the row ids of its tuples, read through `LeafRows` (#111). The ids are 4 bytes where the paper's tuple pointers are 8.
  ```
  After:
  ```text
  The trie keeps its relation's `Tuples` batch, and a chain threads its tuples' row ids through the trie's links, one 4-byte link per row beside the buffer, read through `LeafRows` (#111, #101). The ids and links are 4 bytes where the paper's tuple pointers and chain pointers are 8, and the links sit in an array beside the tuples rather than inside them.
  ```

  The **Construction** bullet (233), after its sentence ending
  `and their tries hold the same contents under other row ids.` and before
  `See [`docs/data-structures/hash-trie.md`]`, insert:
  ```text
  Every list is a `Chain { head, tail, len }` threaded through `links`, in input order; moving a row to the next level's list rewrites its link, as Algorithm 2 moves each tuple to exactly one child list, so a build allocates per table, never per list (#101 B2).
  ```
  (One space separates it from the sentences on either side.)

  Hash Triejoin step 4 (309), before (substring):
  ```text
  4. **Emit**: at the leaf, cross-product the participating tuple chains, then
  ```
  After:
  ```text
  4. **Emit**: at the leaf, cross-product the participating tuple chains (one `LeafCursor` per chain, an odometer whose overflow resets a cursor and carries), then
  ```

- [ ] **Step 7: `docs/specs/bench-report-schema.md`.** After the row beginning
  `| 4 (no bump) | 2026-10-08 |` (line 266), insert:
  ```text
  | 4 (no bump) | 2026-10-14 | Every HashTrie list is a chain threaded through one 4-byte link per row (`ChainLinks`) instead of a `Vec<RowId>` (#101 B2), in every build mode and Layout. HashTrie's `insertion` and `end_to_end` time builds that allocate per table, never per list, and rewrite each row's link once per level; its `space` counts the links (capacity × 4 B) and 24-byte leaf buckets where it counted a heap list per chain and pending list and 32-byte leaf buckets; its `iteration`, the join's leaf product and `bench ds`'s trie walk, walks each chain through its links, one link and one row per step, and under `ds_layout_expansion=lazy` times an expansion that re-threads links; and under `--column-orders any` its `copies` time copies built the same way. The trie's contents, its tables' capacities and every join's output order are unchanged, and no field or axis changed, so `schema_version` stays `4`, as for #107 and #101 B1; compare HashTrie timings and `space` within one binary, or across B2 only through a measured A/B. TreeTrie and ColumnTrie reports are unchanged. |
  ```

- [ ] **Step 8: `kermit-iters/README.md` and the spec.**

  `kermit-iters/README.md`: after the bullet beginning
  `` - [`HashStrategy`](src/hash_strategy.rs) `` (line 12), insert:
  ```markdown
  - [`Tuples`](src/tuples.rs) / `RowId`, [`LeafRows`](src/leaf_rows.rs) / `LeafCursor` and [`ChainLinks`](src/chain_links.rs) / `ChainWalk` — a relation's tuples as one row-major batch, each row addressed by a 4-byte `RowId` (#111); a hash-trie leaf chain's rows read from such a batch, listed or linked, and walked with a cursor; and the per-row link array a `HashTrie` threads its lists through, in place of a `Vec` per list (#101).
  ```

  `docs/specs/2026-10-14-hash-trie-chain-links-design.md`, the status (line 4), before:
  ```text
  **Status:** Design approved in conversation (2026-10-14), section by section. Not yet planned.
  ```
  After (`<today>` is `date +%F` on the day this task runs, the same date in both places):
  ```text
  **Status:** Design approved in conversation (2026-10-14), section by section. Planned in
  `docs/superpowers/plans/2026-10-14-hash-trie-chain-links.md` and implemented (<today>); see
  Amendment 1 for where the code differs from this text.
  ```
  At the end of the same file, after the `## Documentation` list, append a blank line and:
  ```markdown
  ## Amendment 1 (<today>): as built

  Only where the code differs from the text above:

  - **`Chain` is nominally `pub`.** Its module is private, but
    `PendingChild::from_chain` names it in a public trait reachable through
    `ExpansionPolicy`, and a `pub(super)` type there trips
    `private_interfaces`, an error under `-Dwarnings`. The same reason makes
    the policy traits generic over the node type.
  - **`Chain::walk` returns a named `ChainWalk`** (`kermit-iters`), counted by
    `len`: it never reads past the tail, and a debug build asserts the tail's
    link is `NIL`.
  - **The per-row builds push their links.** `ChainLinks::new` allocates
    zeroed links for the builds from tuples, as designed. `insert` and the
    `incremental` build start from `ChainLinks::with_capacity` and push one
    `NIL` link per row before placing it: zeroed links would leave a pruned
    `Singleton`'s row at 0 where `bulk` writes `NIL`, and the array identity
    with `bulk` would fail.
  - **`radix:K` merges as `parallel:N` does.** The shared helper
    (`radix::build_and_merge`) gives `radix:K` the k-way merge over its 2^K
    partitions' entry lists, O(D log P) heap operations, in place of a sort
    of all D entries, as well as the early return on an empty batch. Its
    worker runs are `[1, 1, 1]`.
  - **`LeafRows`' `Debug` is written by hand**, as the list of its rows, so a
    linked view never prints its trie's whole links array.
  - **A re-entrant walk is no longer caught.** A `for_each_tuple` visitor that
    expands a child the walk is still reading (by opening an iterator on the
    same trie) re-threads rows under the walk, which may then lend wrong
    rows or panic; it was a `BorrowMutError` before. Nothing in kermit does
    that, and the walks' docs say so.
  ```

- [ ] **Step 9: Sweep.**
  ```bash
  rg -n -e 'TupleList' -e 'tuple_list_heap_bytes' -e 'chain_lens' -e 'from_rows' -e 'shrink_to\(' \
    -e 'HashTable<Vec<RowId>>' -e 'RefCell<Vec<RowId>>' -e 'until #101 B2' -e "until #101's layer 3" \
    -e 'chain capacity' -e 'sorts its entries' -e 'a sort of the D' \
    --glob '!docs/superpowers/**' --glob '!docs/specs/20*' --glob '!**/CHANGELOG.md' .
  rg -n '\.row\(' kermit-algos/src kermit-ds/tests kermit/tests kermit-iters/src/leaf_rows.rs \
    kermit-iters/src/hash_trie.rs
  rg -n -e 'Vec<RowId>' -e 'a `Vec` per' -e '32 B' -e 'BorrowMutError' -e 'row\(i\)' \
    -e '8-byte chain pointer' --glob '!docs/superpowers/**' --glob '!docs/specs/20*' \
    --glob '!**/CHANGELOG.md' .
  ```
  Expected:
  - **The first grep** prints nothing. (`from_rows` was `PendingChild`'s constructor before
    `from_chain`; no other item has the name.) A hit in Rust code, or in
    `kermit/tests/tuple_allocation.rs`'s comments, is a code task's leftover (T5's or T6's):
    report it with the task's name rather than editing code here.
  - **The second grep** prints no call of a `LeafRows` method named `row`. A `tuples.row(`
    receiver, `Tuples::row`, is fine wherever it appears.
  - **The third grep.** Every hit is true after B2. Each one:
    - is one of Step 1's named id vectors (`Tuples::sort`'s order, the selection view's
      `leaf_ids`, `fill_runs`' deferred ids and its test's `tail`, `reverse_rows`), or
      `kermit-ds/src/morsel.rs`, which is TreeTrie's and untouched;
    - is a "before #101 B2" clause, a `2026-10-14 (#101 B2)` note, or the history in a dated
      record (hash-trie.md's records, among them the #107 record's reading on Umbra's 8-byte
      chain pointer; parallel-build.md's; the `bench-report-schema.md` rows);
    - is about #111's `Vec` per tuple (`BENCHMARKING.md` 89–90, `kermit/src/bench_report.rs`
      85, parallel-build.md's `2026-10-08 (#111)` note);
    - says why a chain has no random access (`row(i)` would be O(i)): hash-triejoin.md's new
      invariant and `CLAUDE.md`'s bullet;
    - names `BorrowMutError` as what a re-entrant walk did before B2.

  Fix any other hit in the wording of this task. Then read once, rendered, the tables this
  task edited (hash-trie.md's parity, Construction and Complexity tables; parallel-build.md's
  complexity tables and presized worked example): a stray `|` breaks a table silently.

- [ ] **Step 10: Lint and document** (three of the edited files are Rust, doc comments only).
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: `0`, then all clean. `cargo doc` resolves the new
  [`ChainLinks`](crate::ChainLinks) link in `kermit-iters/src/hash_trie.rs`. Then
  `git status --short` lists exactly this task's files: the three Rust files (fewer if Step 1
  found an item already rewritten), `kermit-iters/README.md`, `hash-trie.md`,
  `hash-triejoin.md`, `parallel-build.md`, `CLAUDE.md`, `ARCHITECTURE.md`,
  `bench-report-schema.md` and the spec.

- [ ] **Step 11: Commit.** Stage by name; drop from the list any Rust file Step 1 left
  unchanged.
  ```bash
  git add kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/bulk.rs \
    kermit-iters/src/hash_trie.rs kermit-iters/README.md \
    docs/data-structures/hash-trie.md docs/algorithms/hash-triejoin.md \
    docs/data-structures/parallel-build.md CLAUDE.md ARCHITECTURE.md \
    docs/specs/bench-report-schema.md docs/specs/2026-10-14-hash-trie-chain-links-design.md
  git commit -m "$(cat <<'EOF'
  docs: HashTrie chains are threaded through per-row links (#101)

  hash-trie.md, hash-triejoin.md, parallel-build.md, CLAUDE.md,
  ARCHITECTURE.md and kermit-iters' README describe #101 B2: every
  HashTrie list is a Chain threaded through one 4-byte link per row
  (ChainLinks), so a build allocates per table, never per list, and
  Algorithm 2 moves each row to exactly one child list by rewriting its
  link (VLDB 2020 §3.2.2, §3.3.2). The parity rows close the chain-pointer
  gap with two departures that are kermit's (4-byte links beside the
  tuples; lists that append, in input order) and keep the 24-byte leaf
  bucket as one. emit_leaf's cursors, LeafRows' two forms, the workers'
  one-writer-per-link argument and the presized children across runs are
  documented. bench-report-schema gains a "4 (no bump)" row; records
  measured before B2 carry dated notes; radix:K's merge is the shared
  k-way merge. The spec is marked planned and implemented, with Amendment
  1 recording where the code differs from it.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_017PLoRDCeuYmtirxRTMXGJj
  EOF
  )"
  ```
  The #105 board and the issues are not edited here: they are outward-facing, so they wait
  for the user's word after landing. The texts are in "After this plan", Step 0.

---

## Final gate

Run after Task 7, on a quiet host. This is CI's PR gate plus the binary's private-item
docs.

- [ ] **Step 1: The host.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  ```
  Expected: `0`. If a timing run is active, wait for it (its chain log prints
  `CHAIN_EXIT=`); never compile under it.

- [ ] **Step 2: The whole workspace's tests**, detached, with a watcher.
  ```bash
  setsid nohup nix develop --command env CARGO_BUILD_JOBS=2 cargo test --workspace > $SCRATCH/gate-test.log 2>&1 &
  echo $! > $SCRATCH/gate-test.pid; disown
  while kill -0 "$(cat $SCRATCH/gate-test.pid)" 2>/dev/null; do sleep 30; done
  grep -E '^test result' $SCRATCH/gate-test.log \
    | awk '{p+=$4; f+=$6; i+=$8} END {print "cargo "p" passed, "f" failed, "i" ignored"}'
  grep -nE 'FAILED|panicked at|error(\[E[0-9]+\])?:' $SCRATCH/gate-test.log | head -20
  ```
  If the session blocks a foreground `sleep`, wait on the same `kill -0` condition with its
  Monitor tool instead.

  Expected: `0 failed`, and the second grep prints nothing. B1's gate passed 6035 at
  a589c64, and the spec commit 0e94f30 adds no test. The count grows by the tests T1–T6 add,
  less any they remove; each task's report states its own delta, and their sum is the
  expected growth. Cross-check it from the diff:
  ```bash
  git diff $BASE -U0 -- '*.rs' | grep -E '^[+-][[:space:]]*#\[test\]' | cut -c1 | sort | uniq -c
  grep -A1 'Doc-tests kermit_iters' $SCRATCH/gate-test.log
  ```
  The first prints how many `#[test]` attributes were added (`+`) and removed (`-`). The
  passed count should be 6035 plus that difference, plus the one doctest T2 adds in
  `chain_links.rs` (T3 rewrites `leaf_rows.rs`' doctest in place, which changes no count);
  the second shows `kermit_iters`' doctest run. A `#[test]` inside a suite macro
  (`kermit-ds/tests/common/macros.rs`) counts once per invocation, so where a task edited one,
  its own report is authoritative. Any shortfall is a lost test: find it before going on.

- [ ] **Step 3: The acceptance tests by name**, which Step 2 ran, re-run alone so their
  output is readable:
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds -- \
    bulk_and_incremental_thread_the_same_links bulk_builds_the_incremental_trie \
    radix_builds_the_bulk_trie parallel_builds_the_bulk_trie presized_parallel_builds_are \
    rethreading_a_chain_visits_every_row resolve_rethreads_the_pending_chain_through_a_shared_reference \
    eager_tries_stay_sync nodes_are_send_under_every_layout presized_tail_goes_in_input_order_for_every_n
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-algos -- emit_leaf_resets_each_cursor_on_carry
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test tuple_allocation --test result_allocation
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit column_distinct_leaves_a_lazy_trie_as_built
  ```
  Expected: `0 failed` in every `test result` line, and every name above appears in the
  output with `... ok` at least once (`bulk_builds_the_incremental_trie`, `radix_builds_the_bulk_trie`,
  `parallel_builds_the_bulk_trie` and `presized_parallel_builds_are` are prefixes, each
  matching several tests). A name that matches nothing has been renamed: find the test and
  re-run it by its name. `tuple_allocation` and `result_allocation` pass in full.

- [ ] **Step 4: Lints, format and docs.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
  nix develop --command cargo fmt --all -- --check
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  nix develop --command env CARGO_BUILD_JOBS=2 cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings
  ```
  Expected:
  - **clippy, fmt and `cargo doc`:** clean.
    - If `fmt --check` disagrees with CI later, suspect the flake's pinned nightly first
      (CLAUDE.md, "The flake's nightly must track CI's").
    - The fix is `nix flake update rust-overlay` and a re-run. Commit `flake.lock` separately,
      and only with the user's agreement.
  - **`cargo rustdoc --bin kermit`:** exactly the 2 errors known before this plan, at
    `kermit/src/execution.rs:6` and `kermit/src/options.rs:241`. The binary's private docs are
    never linted by CI, since the lib and the bin share the name `kermit`.
    - B2 changes nothing in `kermit/src`, so the count cannot move through this plan's
      edits. If in doubt, run the same command in a `git archive $BASE` copy under `$SCRATCH`
      and compare.
    - Any further error: find which task's change reaches it, fix it in a commit of its own
      (`docs: … (#101)`, with the two trailer lines), and report it.

- [ ] **Step 5: kermit-lab against a release binary.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo build --release -p kermit
  uv --directory python/kermit-lab sync --group test
  KERMIT_BIN=$WT/target/release/kermit uv --directory python/kermit-lab run pytest -q
  ```
  Expected: `207 passed`, as at B1's gate, the real-binary contract test included. No
  report field changed (spec Decision 4), and no Python file changed, so nothing in
  kermit-lab should move. If numpy or matplotlib fail to load, run the pytest line inside
  `nix develop --command` (the flake exports their `LD_LIBRARY_PATH`).

- [ ] **Step 6: Scope (Priority 6).**
  ```bash
  git diff $BASE --stat -- kermit-ds/src/ds/tree_trie kermit-ds/src/ds/column_trie \
    kermit-ds/src/morsel.rs kermit-ds/src/seek.rs kermit-algos/src/sorted kermit-parser \
    kermit-bench kermit-rdf kermit/src python
  git diff $BASE --stat -- kermit-algos
  git log --oneline $BASE..HEAD -- kermit-algos
  git diff $BASE --stat -- kermit-iters
  git diff $BASE --stat -- kermit
  git diff $BASE --stat
  ```
  Expected:
  - **TreeTrie, ColumnTrie, `morsel.rs`, the sorted family, the binary's plumbing and the
    rest:** the first command prints nothing.
  - **`kermit-algos`.** The second lists only `kermit-algos/src/hash/hash_triejoin.rs`,
    `selection.rs` and `singleton.rs`, and the third exactly one commit, Task 3's.
  - **`kermit-iters`.** The fourth lists `src/chain_links.rs`, `src/leaf_rows.rs`,
    `src/lib.rs`, `src/hash_trie.rs` and `README.md`.
  - **`kermit`.** The fifth lists only `kermit/tests/tuple_allocation.rs`.
  - **Every file.** The last lists exactly the file map's files plus this plan.

- [ ] **Step 7: Acceptance** (the spec's § Acceptance, item by item).
  ```bash
  rg -n -e 'TupleList' -e 'HashTable<Vec<RowId>>' -e 'RefCell<Vec<RowId>>' -e 'Arrival<Vec<RowId>>' \
    -e 'Vec<Vec<RowId>>' -e 'Cell<Vec' kermit-ds/src/ds/hash_trie
  rg -n 'Vec<RowId>' kermit-ds/src/ds/hash_trie
  rg -n -e 'Listed' -e 'Linked' kermit-iters/src/leaf_rows.rs | head -4
  rg -n -e 'chain_lens' -e '\.row\(' kermit-algos/src/hash
  rg -n 'LeafCursor' kermit-algos/src/hash/hash_triejoin.rs | head -3
  ```
  Expected, against each acceptance item:
  1. **Every HashTrie list is a `Chain`.** The first grep prints nothing. The second prints
     only vectors of ids that are no list of the trie: `fill_runs`' deferred ids and the tail
     test's `tail` (`parallel.rs`), `reverse_rows`' id map (`identity.rs`), and test locals
     that collect a walk or spell out expected ids. A field, a node payload or a type alias
     of `Vec<RowId>` is a list that B2 missed: the gate fails.
  2. **`LeafRows` walks both forms, and `emit_leaf` uses cursors.** The third grep shows both
     variants; the fourth prints nothing; the fifth shows `LeafScratch`'s cursors.
  3. **`bulk` against `incremental` is array-identical, `links` included, and the radix
     family is equivalent to `bulk`:** Step 3's `bulk_*`, `radix_*`, `parallel_*` and
     `presized_*` tests passed.
  4. **The workspace gate:** Steps 2, 4 and 5.
  5. **The scope check:** Step 6.
  6. **The re-derived `tuple_allocation.rs` bounds pass:** Step 3.

- [ ] **Step 8: Report to the user.** Include:
  - the commit SHAs (the plan's, then T1–T7, and any fix commit from Step 4);
  - the gate's counts (cargo passed / failed / ignored against the expected count; pytest);
  - the five mutation checks, each with the test that failed and its message;
  - the rustdoc error count;
  - anything done differently from this plan, with reasons.

**Landing is the user's call.** On their word only:
1. `git fetch origin`;
2. `git merge origin/master` (never rebase);
3. if anything came in, re-run Steps 1–5;
4. `git push origin HEAD:master`.

Never push without it.

---

## After this plan: the board, the issues, and the replicated measurement

Everything here is outward-facing or a timing run, so each step waits for the user's word.
B1's own board update, issue comments and measurement (its plan's "After this plan") were
never made, so this section covers B1 and B2 together.

- [ ] **Step 0: The #105 board and #101 (only on the user's word, after landing).** The
  board's rows still describe the trie before #111 ("One heap `Vec` per tuple; chains are
  `Vec<Vec<usize>>`"). Show the user the rows as they stand and the replacements below, and
  edit only once they approve.
  ```bash
  gh issue view 105 --repo aidan-bailey/kermit --json body -q .body > $SCRATCH/105-before.md
  cp $SCRATCH/105-before.md $SCRATCH/105-after.md
  ```
  - Find the rows below in `$SCRATCH/105-after.md` and replace the kermit and Tracking cells
    of each with the text given, keeping the row's own markup (its checkbox cell and its
    Paper cell).
  - Then `diff -u $SCRATCH/105-before.md $SCRATCH/105-after.md`, and show the diff to the
    user.

  `<B2 landing SHA>` is `git -C $WT rev-parse --short HEAD` at the push; the user confirms
  it before the edit. B1 landed at a589c64.

  The rows:
  - **Representation, the row whose Paper cell begins "Tuples materialised contiguously at
    a fixed length, with an 8-byte chain pointer"**: checkbox `[x]`.
    - kermit: "One row-major buffer per relation (#111). `radix:K`, `parallel:N` and
      `presized:N` copy it into first-key-hash partition order and keep the copy (#101 B1);
      `bulk` and `incremental` keep arrival order. Every list is threaded through one link per
      row, so no list allocates (#101 B2). Departures, all kermit's: 4-byte ids and links in
      an array beside the tuples, not 8-byte pointers inside them; lists that append, in input
      order; one partitioning round, not two passes."
    - Tracking: "#111; #101 B1 (a589c64) and B2 (`<B2 landing SHA>`); the measurement of both
      follows."
  - **Representation, the row whose Paper cell begins "16-byte buckets"**: checkbox stays
    `[ ]`. kermit: append to the cell "A leaf bucket is 24 B since #101 B2: the hash and a
    chain's head, tail and length."
  - **Build, the row whose Paper cell begins "Algorithm 2: put tuples into bucket lists"**:
    in the Tracking cell, replace "The lists are a `Vec` per bucket, not chain pointers in the
    tuples (#101)" with "Since #101 B2 the lists are chains threaded through one 4-byte link
    per row, appended in input order (kermit's; the paper's push prepends)."
  - **Build, the row whose Paper cell begins "Parallel build"**: in the Tracking cell,
    replace "Still open: tuples stay in file order, partitioned in one pass into per-morsel
    buffers rather than two passes into contiguous storage (#101)" with "Since #101 B1 the
    tuples are copied into contiguous partitions on N threads as they are materialised; one
    partitioning round, not two passes (kermit's: #101 found no fan-out cost at this scale).
    Since #101 B2 the workers thread every list through the trie's links, one writer per
    link."
  - **The Status line**: replace "Open rows: 2 representation (plus the #66 revisit, now
    unblocked)" with "Open rows: 1 representation (#106, with the pruned-pointer row it
    subsumes; plus the #66 revisit, now unblocked)", and "#101 has work in flight" with
    "#101's layer 3 landed (B1 a589c64, B2 `<B2 landing SHA>`); its measurement is next".

  On their word:
  ```bash
  gh issue edit 105 --repo aidan-bailey/kermit --body-file $SCRATCH/105-after.md
  gh issue comment 105 --repo aidan-bailey/kermit --body "#101 layer 3 landed in two halves. B1 (a589c64): radix:K, parallel:N and presized:N copy their rows into first-key-hash partition order and keep the copy as the trie's buffer (VLDB 2020 §3.3.2). B2 (<B2 landing SHA>): every HashTrie list is a chain threaded through one 4-byte link per row, so a build allocates per table, never per list, and Algorithm 2 moves each row to one child list by rewriting its link (§3.2.2). Departures, recorded in hash-trie.md: links are 4-byte ids beside the tuples, lists append in input order, a leaf bucket is 24 B. Rows updated: contiguous storage (done), 16-byte buckets (note), Algorithm 2 and the parallel build (tracking). Specs: docs/specs/2026-10-08-hash-trie-partitioned-materialisation-design.md, docs/specs/2026-10-14-hash-trie-chain-links-design.md. One replicated measurement of both follows."
  gh issue comment 101 --repo aidan-bailey/kermit --body "Layer 3 has landed. B1 (a589c64): the radix family copies its rows into partition order (morsel::partition_rows) and keeps the copy, so every read of a partition's rows is local. B2 (<B2 landing SHA>): every list (Algorithm 2's per-bucket lists, leaf chains, lazy pending lists, the radix family's scratch lists) is a chain threaded through one 4-byte link per row, so no list allocates. bulk and incremental keep arrival order, so one binary separates the copy's effect from the order's. Neither half has been measured yet: the next step is one replicated run of both (bulk, incremental, radix:12, parallel:N, presized:N, eager and lazy, on price and friendof grouped and shuffled, plus this issue's locality harness), with TreeTrie as the control."
  ```
  The user may edit any of these texts. Use their version, and post nothing they have not
  seen.

**The measurement.** The spec's § Measurement, which also runs B1's never-run Parts A–C, in
the #107 style:
- after landing, and only once the user says to run it;
- in its own run directory, `RUN=/tb/Source/Academia/kermit-bench-runs/hash-trie-chain-links-<date>/`
  (`<date>` is the day the run starts, `date +%F`), with a README written before the run that
  states the predictions below and a log section kept as the run goes.

- [ ] **Step 1: Binaries.**
  - **The SHAs:**
    - `d746b97`, #111's landing, flat tuple batches;
    - `a589c64`, B1's landing;
    - `<B2 landing SHA>`, B2's;
    - `89771a9`, before #111, for Part C only (#111's own A/B, folded in from B1's plan).
  - **The build, per SHA:**
    - `git -C $WT archive <sha> | tar -x -C $RUN/src/<sha>`;
    - `Cargo.lock` is gitignored, so `git archive` leaves it out. Copy the worktree's into each
      tree, as the locality run did, and record its sha256 in `$RUN/env.txt`;
    - build in its own `CARGO_TARGET_DIR` (`$RUN/target-<sha>`) through the worktree flake,
      `nix develop $WT --command env CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=$RUN/target-<sha> cargo build --release -p kermit`
      from `$RUN/src/<sha>`, so all use one toolchain (default features: jemalloc);
    - copy the binary to `$RUN/bin/kermit-<sha>` and record its sha256 in `$RUN/env.txt`.
  - **The sha256s must all differ.** A shared target dir once copied one binary out twice:
    `git archive` dates files at the commit time, so cargo saw the second tree as up to date.

- [ ] **Step 2: Precautions.** Copy `run.sh`, `chain.sh` and `host_sampler.sh` from
  `../hash-trie-algorithm-2-2026-10-07/` and `../hash-trie-locality-2026-10-08/`, and adapt
  them; `run.sh` sources `../build-2026-10-05/lib.sh` for its quiet gate and `step`.
  - **Before every step:** `lib.sh`'s quiet gate (at most 1.5 busy cores over 10 s), then a
    memory guard sized from a probed peak (`need_mib`, as in the #107 `run.sh`). Probe the
    partitioning modes, which hold the tuples twice while they copy, and lazy `presized:8`
    on `binary-1e7`, not `bulk`. B2's links add 4 bytes per row to every peak.
  - **The host sampler**, run-local, logs to `logs/host-samples.tsv` until the chain prints
    `CHAIN_EXIT=`. Rows that overlap another session's work are left out and re-run.
  - **Replicates:** 5, outermost, with the arm order rotated per replicate and the binary
    order alternating (`d746b97 a589c64 B2`, then reversed).
  - **A distinct `--name` per step** (#98: Criterion overwrites a group shared across runs),
    and a `--report-json` per step.
  - **Criterion output:** `KERMIT_WORKSPACE=$RUN`, so it lands in `$RUN/target/criterion`.
  - **A deadline.** Before launching, time one replicate of each part (`REPS=1`), multiply,
    and show the user the estimate; launch with the `DEADLINE` guard the 2026-10-07 full
    sweep's `chain.sh` used, set to what the user approves.
  - **Inputs, read-only:**
    - `friendof` and `price` from `~/.cache/kermit/benchmarks/watdiv-stress-100-test-1/`.
      Read them, never write there: no `--force`, `bench clean` or `bench gen` under
      `~/.cache/kermit` without asking the user.
    - The file-shuffled `friendof` of `../radix-shuffle-2026-10-05/data/`.
    - `binary-1e7.csv` and the other 2026-10-06 relations from
      `../tree-trie-scaling-2026-10-05/data/`, in file order.
    - A stable sort of `binary-1e7.csv` on the first column, written once to
      `$RUN/data/binary-1e7-sorted.csv`: `(head -1 F; tail -n +2 F | sort -t, -k1,1n -s)`.
    - `oxford-uniform-s3`'s YAML, copied into `$RUN/benchmarks`.
  - **Report schemas.** 89771a9 writes schema-3 reports and the others schema 4, so Part C's
    analysis passes `allow_mixed_schema=True` deliberately and compares like with like only.

- [ ] **Step 3: Part A, B1's and B2's effects** (binaries `d746b97`, `a589c64`, B2).
  `bench ds -m insertion iteration` in every replicate, plus `space` in replicate 1 (it is
  deterministic), with `--sample-size 10 --measurement-time 3 --warm-up-time 1`. A step:
  ```bash
  step "A/$bin/$rel/$cell/r$i" $RUN/bin/kermit-$bin $RUN/reports/A-$bin-$rel-$cell-r$i.json \
      --name b2A-$bin-$rel-$cell-r$i \
      ds -r "${REL[$rel]}" $(cell_args $cell) -m "${metrics[@]}"
  ```
  - **HashTrie cells** (SipHash, pruning off), each under `--ds-layout-expansion eager` and
    `lazy`:
    - `bulk`, `incremental`, `radix:12` and `parallel:8` under the default config;
    - `bulk` and `presized:8` under `--ds-config root-capacity=tuples`, so `presized:8`'s
      `space` is compared like with like;
    - `radix:16`, eager only (B1's Part A arm).
  - **The control:** TreeTrie `serial` (`-i tree-trie`). Its code is identical in all three
    binaries, so its ratio is the codegen bound.
  - **Inputs:** `friendof` grouped and file-shuffled, `price`, and `binary-1e7` sorted and in
    file order.
  - **`space`** must be identical across the modes of one binary, config and expansion: the
    BuildMode rule, since `links` is sized from the row count in every mode. Any difference
    is a bug, not a result: stop and report it.
  - **Comparisons:** `a589c64` over `d746b97` is B1's effect, B2 over `a589c64` B2's;
    `bulk` over `incremental` within B2 is #107's ratio after B2. `bench ds`'s `iteration` is
    the trie walk (`for_each_tuple`), which reads chains through links after B2; Part C's is
    the join.

- [ ] **Step 4: Part B, the #101 locality harness** (B1's Part B), at `a589c64` and B2.
  - **The harness.** Copy `../hash-trie-locality-2026-10-08/harness/` to
    `$RUN/harness-<sha>/` per SHA, and point its `kermit-ds` path dependency at
    `$RUN/src/<sha>/kermit-ds`. Run its E1 block, `friendof` × cells GG/SS/GS/SG, for `bulk`
    and `radix:12`.
  - **The prediction (B1's spec):** `radix:12`'s GS and SS cells fall to about GG plus the
    copy, because the copy undoes any allocation order.
  - **What #111 changed.** The harness passes `Vec<Vec<usize>>` into the timed constructor.
    On a post-#111 tree the conversion into one `Tuples` batch happens inside the timed build,
    and it is the only step that reads the allocation order. GS therefore keeps that
    conversion's random reads under every mode.
    - Time the conversion alone per cell, untimed for the build, so the copy's effect is
      attributed separately.
    - Say so in the README before the run.
  - **If the harness does not compile** against a post-#111 tree, port only its constructor
    call and record the port's diff in `$RUN/patches/`. Change nothing it times.

- [ ] **Step 5: Part C, across #111, B1 and B2** (B1's Part C, with B2 added; binaries
  `89771a9`, `d746b97`, `a589c64`, B2).
  - **C1:** `bench ds -m insertion space iteration` on the 12 relations of the 2026-10-06 runs
    and the shuffled `friendof`: HashTrie `bulk`, `radix:12`, `parallel:16` and
    `presized:16` (with `root-capacity=tuples`), eager, and TreeTrie `serial` as the control.
  - **C2:** `bench run oxford-uniform-s3 -q triangle -m insertion iteration space end-to-end`,
    HashTrie pruning {off, on} × expansion {eager, lazy}, with TreeTrie/LFTJ as the control,
    at the lazy-expansion record's Criterion settings (`lib.sh`'s `CRIT`). This is where
    `end_to_end` means a query: build plus join (`--queries-per-build 1`).
  - **Comparisons:** `d746b97` over `89771a9` is #111's effect, `a589c64` over `d746b97`
    B1's, B2 over `a589c64` B2's. `bulk`'s B1-over-`d746b97` ratio, like TreeTrie's, is the
    codegen bound: B1 does not change `bulk`.
  - #111's own Part B (glibc builds of TreeTrie) is not folded in: it involves no HashTrie.

- [ ] **Step 6: Part D, the eager atomics check.** A one-off, never landed (spec § Measurement
  and Decision 3). No `unsafe`, in the scratch tree too.
  - **The scratch tree:** `$RUN/src/<B2 SHA>-plain`, another `git archive` of B2's tip. In its
    `kermit-iters/src/chain_links.rs`, replace the type and the bodies of `new`,
    `with_capacity`, `get`, `set`, `push`, `heap_size_bytes` and `to_vec`, leaving `len`,
    `is_empty`, `walk`, `ChainWalk` and every doc comment as they are:
    ```rust
    use std::cell::Cell;

    #[derive(Debug, Default)]
    pub struct ChainLinks(Vec<Cell<u32>>);

    impl ChainLinks {
        pub const NIL: RowId = RowId::MAX;
        pub fn new(rows: usize) -> Self { Self(vec![0u32; rows].into_iter().map(Cell::new).collect()) }
        pub fn with_capacity(rows: usize) -> Self { Self(Vec::with_capacity(rows)) }
        #[inline]
        pub fn get(&self, row: RowId) -> RowId { self.0[row as usize].get() }
        #[inline]
        pub fn set(&self, row: RowId, next: RowId) { self.0[row as usize].set(next) }
        pub fn push(&mut self, next: RowId) { self.0.push(Cell::new(next)) }
        pub fn heap_size_bytes(&self) -> usize { self.0.capacity() * std::mem::size_of::<u32>() }
        pub fn to_vec(&self) -> Vec<RowId> { self.0.iter().map(Cell::get).collect() }
    }
    ```
    The block shows the items without their doc comments: keep each item's existing doc
    comment above it (`#![deny(missing_docs)]` applies). Remove the `std::sync::atomic` import
    the file no longer uses, and if `ChainWalk::next` reads `self.links.0` directly rather
    than through `get`, change that read to `self.links.get(row)`. A `Cell<u32>`'s `get` and
    `set` are plain loads and stores, with none of the optimiser's caution around atomics:
    the closest a scratch can come to a plain `Vec<u32>` without changing every build's
    signature, since every build re-threads through a shared `&ChainLinks`.
  - **The radix family goes from the scratch.** `Cell` is not `Sync`, so the parallel builds,
    whose workers share `&ChainLinks`, no longer compile. Part D times only serial eager cells,
    so the scratch drops them rather than reach for `unsafe`:
    - in `kermit-ds/src/ds/hash_trie/mod.rs`, delete `mod parallel;`, `mod radix;` and the two
      `parallel::` re-exports (`take_parallel_builds`, `ParallelBuild`);
    - in `kermit-ds/src/ds/hash_trie/implementation.rs`, replace the bodies of the `Radix`,
      `Parallel` and `Presized` arms of `from_tuples_with_config_and_build_mode` with
      `unimplemented!("Part D scratch: bulk and incremental only")`, and drop the `parallel`
      and `radix` imports;
    - if anything else then fails to compile (for instance a `HashTrie: Sync` bound in the
      binary), stop and report it to the user rather than patch around it.

    Record the whole diff in `$RUN/patches/plain-links.diff`, and say in the README that the
    scratch binary has no radix family and no `unsafe`.
  - **Build** it like the others (release, default features, as the other binaries), into
    `$RUN/target-plain`, and copy the binary to `$RUN/bin/kermit-<B2 SHA>-plain`.
  - **Time it against B2's binary**, eager cells only (lazy is not run on the scratch
    binary):
    - `bench ds -m insertion iteration` on `friendof` grouped, `price` and `binary-1e7` in
      file order, for `bulk` and `incremental`, pruning off and on;
    - `bench run oxford-uniform-s3 -q triangle -m iteration`, eager, pruning off and on.
  - **The rule.** A real cost is the plain build faster than B2's by more than the codegen
    bound (about ±3 % across a rebuild, `BENCHMARKING.md`'s "Codegen precision bound"), with
    the 95 % bootstrap CI of the mean ratio excluding 1. Report any such cell to the user: it
    reopens Decision 3's declined per-policy link type. Nothing in the repo changes from this
    check.

- [ ] **Step 7: Predictions** (written into the README before the run).
  - **B2, against `a589c64`:**
    - `insertion` falls in every HashTrie mode and on every input, most for `bulk` on `price`
      (every list held one tuple, so the build allocated and freed a list per key). The
      #107 `bulk` ÷ `incremental` ratios, 1.62× on `price` and 1.19× on grouped `friendof`,
      shrink.
    - `space` falls on every HashTrie cell: each leaf bucket shrinks from 32 to 24 B, occupied
      or not, and each chain's heap list goes, for 4 B of link per row.
    - `iteration` holds within a few percent where chains are short, as on these sets: a
      one-row chain's walk reads one link where it read a `Vec`'s heap. It may cost where
      leaf products restart long inner chains. Lazy `iteration` should fall, since expansion
      no longer allocates lists.
    - TreeTrie is unchanged within the codegen bound.
  - **B1, against `d746b97`** (B1's predictions, from its plan):
    - on shuffled input `radix:12` falls from its 2.13× order penalty to near its grouped time
      plus the copy, and beats `bulk`, which pays 2.32×;
    - on grouped input the copy makes the radix family slightly slower than `bulk`;
    - `space` is unchanged by B1 in every mode;
    - `bulk` and `incremental` are unchanged by B1 within the codegen bound.
  - **#111, against `89771a9`** (Part C): the #111 spec's § Measurement expectations: HashTrie
    `space` about halves per tuple, every `insertion` gets faster, and eager `iteration`
    holds within the codegen bound.
  - **Part D:** no cell outside the codegen bound.

- [ ] **Step 8: Afterwards.**
  - Record the results as dated records: B2's and B1's in `docs/data-structures/hash-trie.md`
    (a `#### Chain links and partitioned materialisation A/B (#101)` record under Build
    modes, in the #107 record's form), and the parallel cells in `parallel-build.md`. Commit
    them as `docs(hash-trie): record #101's replicated A/B`, with the two trailer lines.
  - Draft the results' comments for #101 and #105 in the form of Step 0's, each leading with
    the headline ratios and the run directory, and show them to the user. Post only on their
    word, in their version.
  - Making a radix-family mode the default, and reopening Decision 1 (the in-row layout) or
    Decision 3 (per-policy links), are decided from these numbers, not before.

---

## Merge notes

The plan was drafted in four parts against one shared contract (the coordinator's, in the
planning session's scratchpad): Tasks 1–3; Task 4 and chunk C05a; chunk C05b and Task 6; the
front matter, Task 7, the gate and "After this plan". Where the parts met, these are the choices:

1. **Mutation 1's edit.** Task 4 Step 7's three-edit mutant targets Task 2's `ChainWalk::next`.
   It fails Task 4's `rethreading_a_chain_visits_every_row`, and also Task 2's own
   `the_walk_reads_each_link_before_yielding_its_row` and `a_tail_that_links_on_panics_in_debug`
   (sub-step 5 runs them).
2. **C05b's "Before (post-T1)" blocks** were written from the contract, with Task 1's comments
   trimmed. Every C05b edit replaces a whole item, located by name; Task 1's Steps 3–4 hold the
   exact text. C05b's module doc for `radix.rs` replaces Task 1's, keeping its content (the
   shared steps, the k-way merge) and adding the links paragraph.
3. **Step numbers.** Task 5 is one task: C05a is Steps 1–13, C05b Steps 14–29.
4. **`fill_presized_root_in`** reaches eight parameters with `links`, one past clippy's
   `too_many_arguments`. It takes `#[allow(clippy::too_many_arguments)]` with a reason, as
   `kermit/src/main.rs` and `kermit/src/materialize.rs` do. Bundling the morsel and region sizes
   would change two test helpers for no reader's benefit.
5. **The three partitioning arms** in `implementation.rs` are written in C05a (Step 7, item k)
   and verified in C05b (Step 18).
6. **Acceptance's "no `Vec<RowId>` list remains".** Every list of the trie is a `Chain`. One
   `Vec<RowId>` stays in `hash_trie/`: `presized:N`'s queue of deferred ids (`fill_runs`), which
   is sorted by `origin` before the tail pushes it, so it cannot be a chain, and it is no list of
   the trie. The gate's Step 7 says so.
7. **`LeafRows::row_at` at arity 0.** A linked chain's past-the-end cursor is `NIL`; at arity 0
   it reads the one empty row instead of panicking. That is documented rather than checked: a
   `NIL` test would add a branch to every candidate of the leaf product, and a HashTrie is never
   nullary.
8. **`LeafRows::linked` debug-asserts `links.len() == tuples.len()`**, so every test that builds
   a linked view sizes its links to its batch.
9. **`incremental` pushes a `NIL` link per row** (`ChainLinks::with_capacity`, then `push`, as
   `insert` does) rather than starting from zeroed links. With zeroed links a pruned
   `Singleton`'s row would hold 0 where `bulk` writes `NIL`, and `bulk` and `incremental` would
   not be array-identical. `bulk` and the radix family start from `ChainLinks::new` (zeroed),
   since they thread every row at the root before reading any link.
10. **`radix:K`'s merge is the k-way merge** (Task 1), which `parallel:N` already ran: one
    partition-and-merge path, as B1's final review asked. That is a process change for
    `radix:K` (O(D log P) heap operations where it sorted), which the B1+B2 measurement covers;
    its worker runs become `[1, 1, 1]`.
11. **Doc comments owned twice.** C05a rewrites `implementation.rs`'s `# Storage` and `bulk.rs`'s
    module doc, and Task 3 rewrites `kermit-iters/src/hash_trie.rs`'s `leaf_tuples` doc. Task 7
    Step 1's items for those are guarded fallbacks: their "before" is gone, so they are skipped.
12. **Part D without `unsafe`.** The drafted scratch used `Vec<Cell<u32>>` with a scratch-only
    `unsafe impl Sync`. The workspace rule is no `unsafe`, so the scratch instead drops the
    radix family (whose workers need `Sync`) and times the serial eager cells only.
13. **`morsel.rs:284–286`** says the radix build passes its batch as one morsel,
    "(`tuples.len().max(1)`)"; after Task 1 it passes `tuples.len()`, after its early return.
    The sentence stays true in substance, and the scope check keeps `morsel.rs` unchanged, so it
    is left.
14. **`for_each_tuple`'s reentrancy.** A visit that expands a child whose pending chain the walk
    is still reading (by opening an iterator on the same trie) re-threads rows under the walk,
    which may then lend wrong rows or panic; before B2 it panicked with `BorrowMutError`. The doc
    says so; no caller does it. A probe in a self-join is safe: expansion only moves the rows of
    a pending chain, and no `LeafRows` ever lends one.
15. **Mutation 3 after Task 5** fails the same test with a slice-range panic (`R`'s cursor is
    `NIL`), not the index panic it gives in Task 3. The gate's report lists the message seen.
16. **Task 6 renames** the two HashTrie allocation tests (`…_independently_of_their_row_count`,
    `…_independently_of_its_row_count`); nothing outside `tuple_allocation.rs` names them.
