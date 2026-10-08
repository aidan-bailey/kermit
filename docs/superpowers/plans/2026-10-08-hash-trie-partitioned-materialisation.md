# HashTrie Partitioned Materialisation (#101 layer 3, B1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** HashTrie's three partitioning build modes, `radix:K`, `parallel:N` and `presized:N`, copy their rows into first-key-hash partition order and keep the copy as the trie's buffer, the paper's materialisation step (VLDB 2020 §3.3.2). Every later read of a partition's rows is then local. `bulk` and `incremental` keep arrival order, so one binary measures the effect.

**Architecture:**
- **One new helper.** `morsel::partition_rows` (T1) is a classic histogram-and-scatter radix partition, morsel-driven like `scatter_rows`.
  - It hashes each row once and keeps the partition number for the scatter.
  - It writes into disjoint `&mut` ranges of one output buffer: no unsafe, no atomics.
  - It returns a `Partitioned`: the copy, the partition offsets, and each copied row's input position (`origin`).
- **Content-level identity.** `identity::assert_same_contents` (T2) compares two tries by the tuples their ids name, each id read through its own trie's buffer. A partitioned build's ids differ from `bulk`'s.
- **The partitioned fill** takes the batch by value and returns the buffer (T3).
- **The modes switch one at a time:** `radix:K` (T3, on one thread, as one morsel), `parallel:N` (T4, on N threads) and `presized:N` (T5, by home-bucket run). The merge and the presized tail order by `origin`, so each root stays `bulk`'s, or as equivalent as before.
- **Cleanup and docs.** T6 re-derives the allocation bounds, and T7 updates the docs.
- **Untouched:** TreeTrie, which keeps `scatter_rows`, ColumnTrie, `kermit-algos` and the binary's plumbing.

**Tech Stack:** Rust nightly (workspace `rust-toolchain.toml`), the nix dev shell for every cargo command and for rustfmt, `allocation_counter` for the allocation tests, Python kermit-lab (uv) for the gate's contract test.

**Spec:** `docs/specs/2026-10-08-hash-trie-partitioned-materialisation-design.md` (committed at a85c263). Read it first. This plan implements it task by task, and every decision it records is settled. Where the drafted parts of this plan disagreed with each other or with the code, the choices are listed in the Merge notes at the end.

---

## Ground rules for the executor

- **Paths.** `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/hash-trie-paper-parity_18dc25f8653dd5c4`.
  - Every command runs from `$WT`; the commit steps that write `git -C $WT …` and those that write plain `git …` mean the same tree.
  - `SCRATCH` is the executing session's scratchpad.
- **Branch:** work on the loom branch `aidanb/hash-trie-paper-parity`. Never switch branches, never amend, never push.
- **Before Task 1:** this plan is new and uncommitted. Unless the user has already committed it, commit it first, by name:
  - `git add docs/superpowers/plans/2026-10-08-hash-trie-partitioned-materialisation.md`;
  - message `docs: plan #101 B1, partitioned materialisation`, with the two trailer lines below.
- **Host:** before any compile or test run, check that no timing run is active:
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  ```
  It must print `0`. If not, wait: a compile under a timing run contaminates it.
- **Builds:** every cargo command runs inside the dev shell with two jobs, in the foreground: `nix develop --command env CARGO_BUILD_JOBS=2 cargo …`. Set `RUSTFLAGS` / `RUSTDOCFLAGS` inside the `env`, never outside `nix develop`. Long runs (the full workspace test) go through `setsid nohup … & disown`, with a `kill -0` watcher.
- **Formatting:** `nix develop --command cargo fmt --all` only. Stable rustfmt rewrites ~30 files.
- **Doc comments:** backtick every code identifier (clippy `doc_markdown` runs under `-Dwarnings`). `wrap_comments` wraps at 80 columns, so write comments that fit, and accept `cargo fmt`'s re-wrapping.
- **Commits.** Conventional commits referencing `(#101)`. Stage files by name. Every message ends with these two lines:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  ```
- **Mutation checks** (the tasks name each mutant):
  1. Commit first.
  2. Apply the mutant as an exact edit.
  3. Confirm `git diff` shows it applied, and that the named test fails.
  4. Reverse the exact edit, never with `git checkout`.
  5. Re-run the test to green; `git status` must be clean.
- **Never write under `~/.cache/kermit`.**
- **Green tree:** every task leaves the workspace compiling with all tests passing, and ends in one commit.
- **Scope (Priority 6):**
  - Do not touch TreeTrie, ColumnTrie, `kermit-algos`, or the binary's plumbing (`kermit/src`).
  - The exceptions are the doc-comment fixes T7 lists in `kermit/src/execution.rs`.
  - `morsel::scatter_rows` and `RowPartition` stay: TreeTrie's `parallel:N` uses both.
- **Worker runs.** `partition_rows` calls `run_workers` exactly twice, the histogram then the scatter, even for an empty batch. The worker-run pins follow from that:
  - `radix:K`: `[1, 1]` (T3);
  - `parallel:N`: `[N, N, N]` (T4);
  - `presized:3`: `[3, 3, 3, 3]` (T5).

  T1's `partition_rows_starts_workers_twice` pins the count.

## File map

| File | Responsibility | Tasks |
| --- | --- | --- |
| `kermit-iters/src/tuples.rs` | `Tuples::value_capacity` | T1 |
| `kermit-ds/src/morsel.rs` | `Partitioned`, `partition_rows` (`scatter_rows` unchanged) | T1, T3 (allows removed) |
| `kermit-ds/src/ds/hash_trie/identity.rs` | `assert_same_contents`, `assert_same_node_contents`; `assert_equivalent_trie` by contents below the root | T2 |
| `kermit-ds/src/ds/hash_trie/radix.rs` | `radix:K` materialises; `radix::partition` deleted; `build_scratch_root` takes `origin` | T3 |
| `kermit-ds/src/ds/hash_trie/implementation.rs` | `from_tuples_partitioned`'s fill returns the buffer; the mode arms; storage docs | T3, T4, T5, T7 |
| `kermit-ds/src/ds/hash_trie/parallel.rs` | `parallel:N` (T4) and `presized:N` (T5) materialise; `partition_sizes`, `presized_parts`; tests | T3, T4, T5 |
| `kermit-ds/src/ds/hash_trie/build_mode.rs` | mode docs | T7 |
| `kermit-ds/src/relation.rs` | one test-helper comment | T6 |
| `kermit-ds/tests/hash_trie_tests.rs`, `kermit/tests/join_tests.rs` | comments that said "identical" | T3, T4 |
| `kermit/tests/tuple_allocation.rs` | HashTrie bounds re-derived (805 → 777, 819 → 770) | T6 |
| `kermit/src/execution.rs`, `kermit/tests/cli_hash_trie_build_mode.rs` | doc comments only | T7 |
| `docs/data-structures/hash-trie.md`, `docs/data-structures/parallel-build.md`, `CLAUDE.md`, `ARCHITECTURE.md` | docs | T7 |

## Execution chunks

The unit a subagent implements and reviewers check. Each chunk is given its task text with the ground rules, the file map and the merge notes. Every task fits one chunk except Tasks 3 and 7 (each over ~900 lines), which run as two:
- **C03a** writes the failing pin and makes the radix tests content-level. The tree compiles and the pin fails by design; nothing is committed.
- **C03b** implements, runs, commits, and runs the mutation checks.
- **C07a** edits the Rust doc comments and `hash-trie.md`. The tree compiles; nothing is committed.
- **C07b** edits the other docs, sweeps, lints and makes Task 7's one commit.

A second chunk is given its task's preamble (the text before Step 1) as well as its own steps.

| Chunk | Plan text | Compiles and commits |
| --- | --- | --- |
| C01 | Task 1 | yes |
| C02 | Task 2 | yes |
| C03a | Task 3, preamble and Steps 0–4 (premise check; failing pin; content-level radix tests; run on today's code) | compiles, no commit |
| C03b | Task 3, preamble and Steps 5–10 (implement; run; comments; lint; commit; mutants) | yes, Task 3's one commit |
| C04 | Task 4 | yes |
| C05 | Task 5 | yes |
| C06 | Task 6 | yes |
| C07a | Task 7, preamble and Steps 1–2 (Rust doc comments; `hash-trie.md`) | compiles, no commit |
| C07b | Task 7, preamble and Steps 3–9 (`parallel-build.md`; `CLAUDE.md`; `ARCHITECTURE.md`; sweep; lint; commit) | yes, Task 7's one commit |
| C08 | Final gate | runs the gate |

### Task 1: `Tuples::value_capacity`; `morsel::partition_rows` copies rows into partition order

Additive: nothing calls `partition_rows` until T3.
- **The pieces.** `Tuples::value_capacity` reports the buffer's capacity in values. `morsel::partition_rows` is the classic histogram-and-scatter radix partition, morsel-driven like `scatter_rows`:
  - per-morsel histograms on `threads` workers;
  - partition-major prefix sums over (partition, morsel);
  - a scatter in which each worker copies its morsel's rows into disjoint `&mut` ranges of one output buffer.
- **No unsafe, no atomics.** The output is the same byte for byte for every thread count.
- **One hash per row** (coordinator decision, RECONCILE 10(b)). The histogram pass keeps each row's partition number in a per-morsel `Vec<u32>`, sized up front from the morsel's row count, and the scatter reads it back instead of calling `partition_of` again.
  - Today's `radix::partition` caches a `Vec<u16>` of partition numbers for the same reason.
  - So a partitioning build hashes each row's first attribute as often as it did before B1, and the B1 measurement compares like with like.
- **The zero-fill** (RECONCILE 10(c)). The copy is zero-filled before the scatter overwrites it. Safe Rust cannot hand out uninitialised `&mut` ranges, and the fill runs at memset speed; the doc names it as a known cost.
- **Dead code until T3.** In the non-test lib target `partition_rows`, `Partitioned` and its `impl` have no caller, so they carry `#[allow(dead_code)]`, which T3 removes.
- **One-thread callers.** The bookkeeping is one count and one range per (partition, morsel) pair. At `radix:16` on 10⁷ rows in 16 384-row morsels that would be 611 × 65 536 pairs, so the doc tells a one-thread caller to pass the whole batch as one morsel.

**Files:**
- Modify: `kermit-iters/src/tuples.rs` (`impl Tuples`, lines 200–206; `mod tests`, lines 430–439)
- Modify: `kermit-ds/src/morsel.rs` (module doc, lines 4–14; `use`, lines 20–23; new items after `scatter_rows`, which ends at line 168; `mod tests`, after line 284)
- Test: `kermit-iters/src/tuples.rs` and `kermit-ds/src/morsel.rs` (`mod tests`)

- [ ] **Step 1: Check the host, then write the failing `value_capacity` test.**
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  ```
  Expected: `0`. If not, wait; do not compile under a timing run.

  In `kermit-iters/src/tuples.rs`, the last test and the module's closing brace (lines 430–439). Before:

```rust
    #[test]
    fn capacity_is_counted_and_released() {
        let mut tuples = Tuples::with_capacity(3, 10);
        assert!(tuples.heap_size_bytes() >= 30 * std::mem::size_of::<usize>());
        tuples.push(&[1, 2, 3]);
        tuples.push(&[4, 5, 6]);
        tuples.shrink_to_fit();
        assert_eq!(tuples.heap_size_bytes(), 6 * std::mem::size_of::<usize>());
    }
}
```

  After:

```rust
    #[test]
    fn capacity_is_counted_and_released() {
        let mut tuples = Tuples::with_capacity(3, 10);
        assert!(tuples.heap_size_bytes() >= 30 * std::mem::size_of::<usize>());
        tuples.push(&[1, 2, 3]);
        tuples.push(&[4, 5, 6]);
        tuples.shrink_to_fit();
        assert_eq!(tuples.heap_size_bytes(), 6 * std::mem::size_of::<usize>());
    }

    /// `value_capacity` is the buffer's capacity in values, not rows, spare
    /// capacity included: what a copy reserves to hold the same heap bytes.
    #[test]
    fn value_capacity_counts_values_spare_capacity_included() {
        let mut tuples = Tuples::with_capacity(3, 10);
        tuples.push(&[1, 2, 3]);
        assert!(tuples.value_capacity() >= 30);
        assert_eq!(
            tuples.heap_size_bytes(),
            tuples.value_capacity() * std::mem::size_of::<usize>()
        );
        tuples.shrink_to_fit();
        assert_eq!(tuples.value_capacity(), 3);
        assert_eq!(Tuples::from(vec![vec![], vec![]]).value_capacity(), 0);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-iters tuples::
  ```
  Expected: compile error ``error[E0599]: no method named `value_capacity` found for struct `Tuples` in the current scope`` (one per call).

- [ ] **Step 3: Implement.** In `kermit-iters/src/tuples.rs` (lines 200–206). Before:

```rust
    /// Releases the buffer's spare capacity, so that
    /// [`heap_size_bytes`](Self::heap_size_bytes) counts exactly the rows.
    pub fn shrink_to_fit(&mut self) { self.data.shrink_to_fit() }

    /// Heap bytes the buffer holds: its capacity, not its length, in
    /// `usize`s.
    pub fn heap_size_bytes(&self) -> usize { self.data.capacity() * std::mem::size_of::<usize>() }
```

  After:

```rust
    /// Releases the buffer's spare capacity, so that
    /// [`heap_size_bytes`](Self::heap_size_bytes) counts exactly the rows.
    pub fn shrink_to_fit(&mut self) { self.data.shrink_to_fit() }

    /// How many values the buffer has room for before it grows: its
    /// capacity in `usize`s, not rows, spare capacity included. A copy that
    /// reserves it holds as many heap bytes
    /// ([`heap_size_bytes`](Self::heap_size_bytes)) as this batch.
    #[inline]
    pub fn value_capacity(&self) -> usize { self.data.capacity() }

    /// Heap bytes the buffer holds: its capacity, not its length, in
    /// `usize`s.
    pub fn heap_size_bytes(&self) -> usize { self.data.capacity() * std::mem::size_of::<usize>() }
```

- [ ] **Step 4: Run the tests to verify they pass.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-iters tuples::
  ```
  Expected: `test result: ok. 17 passed; 0 failed` (the 16 existing `tuples::tests` plus the new one), and the `tuples::Tuples` doctest passes.

- [ ] **Step 5: Write the failing `partition_rows` tests.** In `kermit-ds/src/morsel.rs`, the end of `scatter_rows_sends_nullary_rows_by_id` and the start of the next test (lines 273–287). Before:

```rust
    /// A nullary row is an empty slice but still has an id, so a batch of
    /// them scatters like any other.
    #[test]
    fn scatter_rows_sends_nullary_rows_by_id() {
        let mut input = Tuples::new(0);
        for _ in 0..5 {
            input.push(&[]);
        }
        let partitions = scatter_rows(threads(2), &input, 2, 2, |row| row.len());
        assert_eq!(partitions[0].ids().collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        assert_eq!(partitions[1].len(), 0);
    }

    #[test]
    fn dispatch_returns_results_in_item_order() {
```

  After: the same lines, with the following inserted between the closing `}` of `scatter_rows_sends_nullary_rows_by_id` and `#[test] fn dispatch_returns_results_in_item_order`:

```rust
    /// `n` rows of `arity` values (at most 3): `[i % 7, i, n - i]`, cut to
    /// `arity`. At arity 1 rows repeat; at arity 3 every row is distinct.
    fn rows_of_arity(n: usize, arity: usize) -> Tuples {
        let mut tuples = Tuples::with_capacity(arity, n);
        for i in 0..n {
            tuples.push(&[i % 7, i, n - i][..arity]);
        }
        tuples
    }

    /// Partitions in the `partition_rows` tests. [`first_mod_3`] never sends
    /// a row to the last one, so an empty partition is always among them.
    const PARTITIONS: usize = 4;

    /// The partition the `partition_rows` tests send a row to.
    fn first_mod_3(row: &[usize]) -> usize { row[0] % 3 }

    /// `partition_rows` at arities 1 and 3, on 1 to 8 threads, with morsels
    /// from one row to more than the whole batch: each case's label, input
    /// and output.
    fn partitioned_cases() -> Vec<(String, Tuples, Partitioned)> {
        let n = 100;
        let mut cases = Vec::new();
        for arity in [1, 3] {
            let input = rows_of_arity(n, arity);
            for t in [1, 2, 3, 8] {
                for morsel in [1, 3, 5, n, n + 1] {
                    let out = partition_rows(threads(t), &input, morsel, PARTITIONS, first_mod_3);
                    let case = format!("arity {arity}, threads {t}, morsel {morsel}");
                    cases.push((case, input.clone(), out));
                }
            }
        }
        cases
    }

    /// `offsets` are the prefix sums of the partition sizes, the empty last
    /// partition included, and the copy and `origin` hold every row.
    #[test]
    fn partition_rows_offsets_are_the_prefix_sums_of_the_partition_sizes() {
        for (case, input, out) in partitioned_cases() {
            let mut expected = vec![0];
            let mut end = 0;
            for p in 0..PARTITIONS {
                end += input.rows().filter(|&row| first_mod_3(row) == p).count();
                expected.push(end);
            }
            assert_eq!(out.offsets, expected, "{case}");
            assert_eq!(out.partitions(), PARTITIONS, "{case}");
            assert_eq!(out.tuples.arity(), input.arity(), "{case}");
            assert_eq!(out.tuples.len(), input.len(), "{case}");
            assert_eq!(out.origin.len(), input.len(), "{case}");
        }
    }

    /// Each copied row lies in the range of the partition `partition_of`
    /// names. With the sizes above and `origin` a permutation, partition
    /// `p` holds exactly the rows sent to `p`.
    #[test]
    fn partition_rows_puts_each_row_in_its_partition() {
        for (case, _, out) in partitioned_cases() {
            for p in 0..out.partitions() {
                for id in out.ids(p) {
                    assert_eq!(first_mod_3(out.tuples.row(id)), p, "{case}: row {id}");
                }
            }
        }
    }

    /// Within a partition, rows keep input order: their input positions
    /// ascend, across morsels as within one.
    #[test]
    fn partition_rows_keeps_input_order_within_a_partition() {
        for (case, _, out) in partitioned_cases() {
            for (p, bounds) in out.offsets.windows(2).enumerate() {
                let from = &out.origin[bounds[0]..bounds[1]];
                assert!(
                    from.windows(2).all(|pair| pair[0] < pair[1]),
                    "{case}: partition {p} is out of input order: {from:?}"
                );
            }
        }
    }

    /// `origin` maps each copied row to the input row it was copied from,
    /// and names every input position exactly once.
    #[test]
    fn partition_rows_origin_maps_each_copied_row_to_its_input_row() {
        for (case, input, out) in partitioned_cases() {
            for (id, &from) in (0..).zip(&out.origin) {
                assert_eq!(out.tuples.row(id), input.row(from), "{case}: row {id}");
            }
            let mut positions = out.origin.clone();
            positions.sort_unstable();
            let every: Vec<RowId> = (0..).take(input.len()).collect();
            assert_eq!(positions, every, "{case}: every input row exactly once");
        }
    }

    /// The copy is the same, byte for byte, on every thread count and with
    /// every morsel size: which worker took which morsel cannot show.
    #[test]
    fn partition_rows_is_the_same_on_every_thread_count() {
        for arity in [1, 3] {
            let input = rows_of_arity(100, arity);
            let one = partition_rows(threads(1), &input, 3, PARTITIONS, first_mod_3);
            for t in [2, 3, 8] {
                for morsel in [1, 3, 7, 101] {
                    assert_eq!(
                        partition_rows(threads(t), &input, morsel, PARTITIONS, first_mod_3),
                        one,
                        "arity {arity}, threads {t}, morsel {morsel}"
                    );
                }
            }
        }
    }

    /// `partition_of` runs exactly once per row: the histogram pass keeps
    /// each row's partition number and the scatter reads it back, so a
    /// caller that hashes in `partition_of` hashes each row once here.
    #[test]
    fn partition_rows_calls_partition_of_once_per_row() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let input = rows_of_arity(100, 3);
        for t in [1, 3] {
            for morsel in [1, 7, 101] {
                let calls = AtomicUsize::new(0);
                let out = partition_rows(threads(t), &input, morsel, PARTITIONS, |row| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    first_mod_3(row)
                });
                assert_eq!(calls.into_inner(), 100, "threads {t}, morsel {morsel}");
                assert_eq!(out.offsets[PARTITIONS], 100, "threads {t}, morsel {morsel}");
            }
        }
    }

    /// Two worker runs, the histogram then the scatter, each on `threads`
    /// workers, an empty batch included. The radix family's worker-run pins
    /// (`build_modes_reach_their_builds`, `presized_build_reaches_its_own_path`
    /// and `parallel_build_merges_under_every_root_capacity` in
    /// `hash_trie/parallel.rs`) count on it.
    #[test]
    fn partition_rows_starts_workers_twice() {
        for t in [1, 3] {
            take_worker_runs();
            let _ = partition_rows(threads(t), &rows_of_arity(10, 2), 4, PARTITIONS, first_mod_3);
            assert_eq!(take_worker_runs(), vec![t, t], "threads {t}");
            let _ = partition_rows(threads(t), &Tuples::new(2), 4, PARTITIONS, first_mod_3);
            assert_eq!(take_worker_runs(), vec![t, t], "threads {t}, empty batch");
        }
    }

    #[test]
    fn partition_rows_of_nothing_is_empty() {
        let out = partition_rows(threads(4), &Tuples::new(2), 8, 3, |_| 0);
        assert_eq!(out.offsets, vec![0, 0, 0, 0]);
        assert_eq!((out.tuples.arity(), out.tuples.len()), (2, 0));
        assert!(out.origin.is_empty());
        assert!((0..3).all(|p| out.ids(p).is_empty()));
    }

    /// The copy reserves the input buffer's capacity, spare capacity
    /// included, so a trie that keeps the copy holds the bytes the input
    /// held.
    #[test]
    fn partition_rows_reserves_the_inputs_capacity() {
        let mut input = Tuples::with_capacity(3, 40);
        for i in 0..10 {
            input.push(&[i, i, i]);
        }
        assert!(
            input.value_capacity() > input.as_flat().len(),
            "the input has spare capacity"
        );
        for t in [1, 3] {
            let out = partition_rows(threads(t), &input, 4, PARTITIONS, first_mod_3);
            assert_eq!(out.tuples.value_capacity(), input.value_capacity(), "threads {t}");
        }
        let empty = Tuples::with_capacity(2, 5);
        let out = partition_rows(threads(2), &empty, 4, PARTITIONS, first_mod_3);
        assert_eq!(out.tuples.value_capacity(), empty.value_capacity());
    }

    /// With one partition the copy is the input, `origin` is the identity
    /// and the partition is every row.
    #[test]
    fn one_partition_is_the_input_order() {
        for arity in [1, 3] {
            let input = rows_of_arity(50, arity);
            for t in [1, 3] {
                let case = format!("arity {arity}, threads {t}");
                let out = partition_rows(threads(t), &input, 7, 1, |_| 0);
                assert_eq!(out.tuples, input, "{case}");
                assert_eq!(out.origin, (0..50).collect::<Vec<RowId>>(), "{case}");
                assert_eq!(out.offsets, vec![0, 50], "{case}");
                assert_eq!(out.ids(0), 0..50, "{case}");
            }
        }
    }

    /// A nullary row is an empty slice but still has an input position, so
    /// a batch of them partitions like any other.
    #[test]
    fn partition_rows_copies_nullary_rows_by_position() {
        let input = Tuples::from_flat(0, 5, Vec::new());
        let out = partition_rows(threads(2), &input, 2, 2, |row| row.len());
        assert_eq!((out.tuples.arity(), out.tuples.len()), (0, 5));
        assert_eq!(out.offsets, vec![0, 5, 5]);
        assert_eq!(out.origin, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    #[should_panic(expected = "partition_rows: a morsel holds at least one row")]
    fn partition_rows_rejects_empty_morsels() {
        let _ = partition_rows(threads(1), &rows_of_arity(4, 1), 0, PARTITIONS, first_mod_3);
    }
```

- [ ] **Step 6: Run the tests to verify they fail.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds morsel::
  ```
  Expected: compile errors ``error[E0425]: cannot find function `partition_rows` in this scope`` and ``error[E0412]: cannot find type `Partitioned` in this scope``.

- [ ] **Step 7: Implement.** Three edits in `kermit-ds/src/morsel.rs`.

  The module doc (lines 4–14). Before:

```rust
//! The input is cut into small fixed-size *morsels*. The *dispatcher*, a
//! mutex-guarded queue, hands the next unit of work to whichever worker is
//! free, so a slow worker never stalls the rest. A build uses it twice:
//!
//! 1. [`scatter_rows`] sends row ids into partitions, one morsel at a time;
//! 2. [`dispatch`] then runs one task per partition.
//!
//! Neither result depends on scheduling. A partition lists its row ids in
//! input order, and task results come back in task order, so a parallel
//! build can reproduce its serial counterpart exactly
//! (`docs/data-structures/parallel-build.md`).
```

  After:

```rust
//! The input is cut into small fixed-size *morsels*. The *dispatcher*, a
//! mutex-guarded queue, hands the next unit of work to whichever worker is
//! free, so a slow worker never stalls the rest. A build uses it twice:
//!
//! 1. [`scatter_rows`] sends row ids into partitions, one morsel at a time,
//!    or [`partition_rows`] copies the rows themselves into partition order
//!    (the paper's materialisation step, #101);
//! 2. [`dispatch`] then runs one task per partition.
//!
//! No result depends on scheduling. A partition lists its rows in input
//! order, and task results come back in task order, so a parallel build
//! can reproduce its serial counterpart exactly
//! (`docs/data-structures/parallel-build.md`).
```

  The `use` (lines 20–23). Before:

```rust
use {
    kermit_iters::{RowId, Tuples},
    std::{num::NonZeroUsize, panic, sync::Mutex, thread},
};
```

  After:

```rust
use {
    kermit_iters::{RowId, Tuples},
    std::{mem, num::NonZeroUsize, ops::Range, panic, sync::Mutex, thread},
};
```

  The new items go after `scatter_rows`, between its closing `}` (line 168) and `dispatch`'s doc comment (line 170). Before (lines 159–171):

```rust
    let mut out: Vec<RowPartition> = (0..partitions).map(|_| RowPartition::default()).collect();
    for (_, buckets) in morsels {
        for (partition, bucket) in out.iter_mut().zip(buckets) {
            if !bucket.is_empty() {
                partition.segments.push(bucket);
            }
        }
    }
    out
}

/// Runs `task` on every item of `items` and returns the results in item
/// order. `threads` workers take the next item from a shared queue whenever
```

  After: the same lines, with this inserted between `out\n}` and `/// Runs \`task\``:

```rust
/// A batch's rows copied into partition order by [`partition_rows`]: the
/// paper's materialisation step, which partitions the tuples so "that
/// tuples with similar join key hash values reside in physically close
/// memory locations" (Freitag et al., VLDB 2020, §3.3.2). Partition `p`'s
/// rows are the rows `offsets[p]..offsets[p + 1]` of `tuples`, in input
/// order.
#[allow(dead_code)] // No caller until the radix family copies its rows (#101).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Partitioned {
    /// The partition-ordered copy. Its buffer reserves the input buffer's
    /// capacity ([`Tuples::value_capacity`]), so a trie that keeps it in
    /// place of the input holds the same bytes.
    pub(crate) tuples: Tuples,
    /// `partitions + 1` prefix sums of the partition sizes: `offsets[0]` is
    /// 0 and the last is the row count.
    pub(crate) offsets: Vec<usize>,
    /// `origin[r]` is the input position of the copy's row `r`.
    pub(crate) origin: Vec<RowId>,
}

#[allow(dead_code)] // No caller until the radix family copies its rows (#101).
impl Partitioned {
    /// How many partitions the rows were copied into.
    pub(crate) fn partitions(&self) -> usize { self.offsets.len() - 1 }

    /// Partition `p`'s rows in the copy: consecutive ids, whose rows keep
    /// input order.
    pub(crate) fn ids(&self, p: usize) -> Range<RowId> {
        row_id(self.offsets[p])..row_id(self.offsets[p + 1])
    }
}

/// The unwritten front of one range of [`partition_rows`]' copy: its values
/// and the matching `origin` slots. Ranges are disjoint `&mut` borrows of
/// the two buffers, so workers write them without a lock.
struct Cursor<'a> {
    values: &'a mut [usize],
    origin: &'a mut [RowId],
}

impl<'a> Cursor<'a> {
    /// Cuts the next `rows` rows of `arity` values off the front, as a range
    /// of their own.
    fn split_off_front(&mut self, rows: usize, arity: usize) -> Cursor<'a> {
        let (values, rest) = mem::take(&mut self.values).split_at_mut(rows * arity);
        self.values = rest;
        let (origin, rest) = mem::take(&mut self.origin).split_at_mut(rows);
        self.origin = rest;
        Cursor {
            values,
            origin,
        }
    }

    /// Writes `row`, copied from input position `id`, at the front, and
    /// moves the front past it.
    fn write(&mut self, row: &[usize], id: RowId) {
        let (slot, rest) = mem::take(&mut self.origin)
            .split_first_mut()
            .expect("the histogram counted every row a morsel sends to this range");
        *slot = id;
        self.origin = rest;
        let (values, rest) = mem::take(&mut self.values).split_at_mut(row.len());
        values.copy_from_slice(row);
        self.values = rest;
    }
}

/// Copies the rows of `tuples` into partition order: partition `p` holds,
/// in input order, the rows that `partition_of` sends to `p`, which must be
/// below `partitions`, and the partitions follow one another in order. A
/// classic histogram-and-scatter radix partition, in three steps:
///
/// 1. **Histogram.** `threads` workers take morsels of `morsel_rows`
///    consecutive rows from a shared queue, as in [`scatter_rows`]. For each
///    morsel they call `partition_of` once per row, keep the row's
///    partition number, and count the morsel's rows per partition.
/// 2. **Offsets.** Prefix sums over (partition, morsel) pairs, partition by
///    partition, give every pair its own range of the copy: a partition's
///    ranges lie side by side, in morsel order.
/// 3. **Scatter.** The copy and `origin` are allocated once and cut into
///    those ranges with `split_at_mut`, grouped by morsel. Workers take the
///    morsels again, each with its ranges and its rows' partition numbers,
///    and write every row at the front of its partition's range, its input
///    position into `origin` alongside. The ranges are disjoint, so no
///    write needs a lock or an atomic.
///
/// `partition_of` runs exactly once per row: the scatter reads the numbers
/// the histogram kept (4 bytes a row, freed as each morsel is scattered)
/// instead of calling it again. A build whose `partition_of` hashes
/// therefore hashes each row once here, as it did when it partitioned ids.
///
/// The copy does not depend on the thread count or on which worker took
/// which morsel: a partition's ranges are in morsel order, and a morsel's
/// rows are written in input order. Its buffer reserves the input buffer's
/// capacity ([`Tuples::value_capacity`]). Any arity works, nullary
/// included.
///
/// It starts workers exactly twice, one `run_workers` call per pass
/// (histogram, then scatter), each on `threads` workers, even for an empty
/// batch; a caller that must start none checks for that first.
///
/// Two costs are known and accepted:
///
/// - **The zero-fill.** The copy is zero-filled before the scatter
///   overwrites it, since safe Rust cannot hand out uninitialised `&mut`
///   ranges. The fill runs at memset speed.
/// - **The bookkeeping** is one count and one range per (partition, morsel)
///   pair. A one-thread caller gains nothing from several morsels and
///   passes the whole batch as one (`tuples.len().max(1)`): the classic
///   one-thread radix partition, one histogram and one scatter.
///
/// # Panics
///
/// Panics if `morsel_rows` is zero, if `partitions` exceeds `u32::MAX`, or
/// if `partition_of` returns `partitions` or more, or panics itself.
#[allow(dead_code)] // No caller until the radix family copies its rows (#101).
pub(crate) fn partition_rows(
    threads: Threads, tuples: &Tuples, morsel_rows: usize, partitions: usize,
    partition_of: impl Fn(&[usize]) -> usize + Sync,
) -> Partitioned {
    assert!(morsel_rows > 0, "partition_rows: a morsel holds at least one row");
    // A row's partition number is kept as a `u32` between the two passes.
    assert!(
        u32::try_from(partitions).is_ok(),
        "partition_rows: at most `u32::MAX` partitions"
    );
    let (len, arity) = (tuples.len(), tuples.arity());
    let morsels = len.div_ceil(morsel_rows);
    let morsel_ids = |m: usize| {
        let first = m * morsel_rows;
        row_id(first)..row_id(first + morsel_rows.min(len - first))
    };

    // 1. Histogram: `counts[m][p]` of morsel `m`'s rows go to partition `p`,
    //    and `numbers[m]` holds each of its rows' partition, in row order.
    let queue = Mutex::new(0..morsels);
    let mut counts: Vec<Vec<usize>> = vec![Vec::new(); morsels];
    let mut numbers: Vec<Vec<u32>> = vec![Vec::new(); morsels];
    let counted = run_workers(threads, || {
        let mut counted = Vec::new();
        while let Some(m) = take_next(&queue) {
            let ids = morsel_ids(m);
            let mut histogram = vec![0; partitions];
            // Sized from the morsel's row count, so it never grows.
            let mut morsel_numbers = Vec::with_capacity(ids.len());
            for id in ids {
                let p = partition_of(tuples.row(id));
                histogram[p] += 1;
                // Lossless: the index above checked `p < partitions`, and
                // the assertion at the top `partitions <= u32::MAX`.
                morsel_numbers.push(p as u32);
            }
            counted.push((m, histogram, morsel_numbers));
        }
        counted
    });
    for (m, histogram, morsel_numbers) in counted.into_iter().flatten() {
        counts[m] = histogram;
        numbers[m] = morsel_numbers;
    }

    // 2. Offsets, and the copy cut into one range per (partition, morsel)
    //    pair, partition by partition: `ranges[m][p]` is where morsel `m`
    //    writes its rows of partition `p`.
    let mut values = Vec::with_capacity(tuples.value_capacity());
    values.resize(len * arity, 0);
    let mut origin: Vec<RowId> = vec![0; len];
    let mut offsets = Vec::with_capacity(partitions + 1);
    offsets.push(0);
    {
        let mut unassigned = Cursor {
            values: values.as_mut_slice(),
            origin: origin.as_mut_slice(),
        };
        let mut ranges: Vec<Vec<Cursor<'_>>> =
            (0..morsels).map(|_| Vec::with_capacity(partitions)).collect();
        let mut end = 0;
        for p in 0..partitions {
            for (m, morsel_ranges) in ranges.iter_mut().enumerate() {
                let rows = counts[m][p];
                morsel_ranges.push(unassigned.split_off_front(rows, arity));
                end += rows;
            }
            offsets.push(end);
        }
        debug_assert_eq!(end, len, "the histograms count every row once");

        // 3. Scatter: each worker takes a morsel with its ranges and its
        //    rows' partition numbers, and writes every row at the front of
        //    its partition's range. `partition_of` is not called again.
        let queue = Mutex::new(ranges.into_iter().zip(numbers).enumerate());
        run_workers(threads, || {
            while let Some((m, (mut ranges, morsel_numbers))) = take_next(&queue) {
                for (id, p) in morsel_ids(m).zip(morsel_numbers) {
                    ranges[p as usize].write(tuples.row(id), id);
                }
            }
        });
        // The ranges borrow `values` and `origin` until this block ends.
    }
    Partitioned {
        tuples: Tuples::from_flat(arity, len, values),
        offsets,
        origin,
    }
}
```

  **Why it compiles (borrow checker):**
  - **`run_workers` bound.** `run_workers` takes `work: impl Fn() -> R + Sync`.
    - The scatter closure captures `&queue`, `&morsel_ids` and `&tuples`; it does not call `partition_of`.
    - It mutates only `ranges`, a `Vec<Cursor<'a>>` it owns once `take_next` has moved it out of the queue, so it is `Fn`. It consumes `morsel_numbers`, a `Vec<u32>` it owns the same way.
    - It is `Sync` because every capture is a shared reference to a `Sync` value.
      - `Mutex<T>` is `Sync` when `T: Send`. `T = Enumerate<Zip<vec::IntoIter<Vec<Cursor<'a>>>, vec::IntoIter<Vec<u32>>>>` is `Send`, because `&mut [usize]`, `&mut [RowId]` and `u32` are.
      - `Tuples` is `Sync`.
      - `morsel_ids` borrows two `usize`s.
    - The histogram closure is the same case, with a `Mutex<Range<usize>>`, plus `&partition_of`, which is `Sync` by its bound. It returns `Vec<(usize, Vec<usize>, Vec<u32>)>`, which is `Send`.
  - **The partition numbers.** `ids.len()` is `ExactSizeIterator::len` on `Range<u32>` (in the prelude), read before `ids` is consumed. `p as u32` cannot truncate: `histogram[p]` has already bounds-checked `p < partitions`, and the assertion at the top bounds `partitions` by `u32::MAX`. `numbers` is moved into the scatter's queue, so each morsel's list is freed once its worker has scattered it.
  - **Scoped threads.** `run_workers` spawns `&work` inside `thread::scope`, which lets helpers borrow non-`'static` data. The ranges borrow `values` and `origin`, which outlive the call.
  - **`mem::take`.** It moves each `&'a mut` slice out of its cursor and leaves `&mut []` behind, so `split_at_mut` splits the full-lifetime borrow and both halves keep `'a`: std's `ChunksMut` idiom. Splitting through `&mut self` directly would tie the halves to the method's borrow.
  - **The block.** `queue` owns the iterator, whose drop glue runs at the end of `queue`'s scope, so the borrows of `values` and `origin` last until then. Moving the buffers into `Tuples::from_flat` after the block is accepted; inside it, it would be E0505.
  - **Capacity.** `Vec::with_capacity(n)` has exactly capacity `n` (std's documented guarantee). `resize(len * arity, 0)` cannot reallocate, since `len * arity` is the input's length, which is at most its capacity.
  - **Worker runs.** There are exactly two `run_workers` calls, both unconditional; `partition_rows_starts_workers_twice` pins the count. The T3/T4/T5 tasks' `take_worker_runs` pins (`[1, 1]` for `radix:3`, `[N, N, N]` for `parallel:N`, `[3, 3, 3, 3]` for `presized:3`) rely on it.
  - **Dead code.** The three `#[allow(dead_code)]` attributes make those items live roots for the dead-code pass.
    - That keeps `Cursor` (used only by `partition_rows`) and the fields read by `ids` live in the non-test lib target.
    - `tuples` and `origin` are read only by tests, hence the allow on the struct.
    - The derived `Debug`/`PartialEq` impls are ignored by that pass.

- [ ] **Step 8: Run the tests to verify they pass.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds morsel::
  ```
  Expected: `test result: ok. 21 passed; 0 failed` (the 9 existing `morsel::tests` and the 12 new ones). Then confirm the call count the later pins rely on:
  ```bash
  rg -n 'run_workers\(' kermit-ds/src/morsel.rs
  ```
  Expected: four lines, `scatter_rows`', `dispatch`'s and two in `partition_rows` (the definition reads `run_workers<R: Send>(` and does not match).

- [ ] **Step 9: Lint, format, document.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  ```
  Expected: all clean. In particular, no `dead_code` warning from the non-test `kermit-ds` lib target. If one names `Cursor` or a field of `Partitioned`, the allow it needs is missing: add it to that item, with the same comment. `git diff --stat` after `cargo fmt` lists only the two files of this task.

- [ ] **Step 10: Commit.**
  ```bash
  git -C $WT add kermit-iters/src/tuples.rs kermit-ds/src/morsel.rs
  git -C $WT commit -m "feat(ds): morsel::partition_rows copies rows into partition order (#101)

  The paper's materialisation step (VLDB 2020 §3.3.2) as a classic
  histogram-and-scatter radix partition: per-morsel histograms on N
  workers, partition-major prefix sums over (partition, morsel), then a
  scatter into disjoint &mut ranges of one buffer, with origin recording
  each row's input position. partition_of runs once per row: the
  histogram keeps each row's partition number for the scatter. No
  unsafe, no atomics. Stable, and the same byte for byte for every
  thread count. The copy reserves the input's capacity, read through the
  new Tuples::value_capacity. Unused until the radix family's builds
  call it.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee"
  ```

- [ ] **Step 11: Mutation checks** (ground rules: commit first, exact edit, confirm with `git diff`, reverse the exact edit, re-run to green, `git status` clean).
  1. **Within-partition order broken** (the spec's third mutant). In `partition_rows` step 2:
     - Change `for (m, morsel_ranges) in ranges.iter_mut().enumerate() {` to `for (m, morsel_ranges) in ranges.iter_mut().enumerate().rev() {`.
     - The ranges of a partition then run in reverse morsel order; the offsets and the membership stay right.
     - Run `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds morsel::tests::partition_rows_keeps_input_order_within_a_partition`. Expected: FAILED, "partition 0 is out of input order".
  2. **Capacity not carried.** Change `let mut values = Vec::with_capacity(tuples.value_capacity());` to `let mut values = Vec::with_capacity(len * arity);`.
     - Run `… cargo test -p kermit-ds morsel::tests::partition_rows_reserves_the_inputs_capacity`. Expected: FAILED, `left: 30` / `right: 120`.
  3. **The scatter hashes again** (RECONCILE 10(b)). In the scatter loop, change `ranges[p as usize].write(tuples.row(id), id);` to `ranges[partition_of(tuples.row(id))].write(tuples.row(id), id);`.
     - The copy is unchanged, but every row now calls `partition_of` twice. An unused-variable warning for `p` is expected.
     - Run `… cargo test -p kermit-ds morsel::tests::partition_rows_calls_partition_of_once_per_row`. Expected: FAILED, `left: 200` / `right: 100`.

---

### Task 2: `identity::assert_same_contents`, the content-level trie comparison

**Why.** From T3 on, the radix family stores its rows in partition order, so its ids differ from `bulk`'s for the same tuples.

**The content-level helpers (Part A).** The new helpers in `kermit-ds/src/ds/hash_trie/identity.rs` compare the tuples the ids name, not the ids, each id read through its own trie's buffer:
- every table bucket for bucket: capacity, allocation, length and the hash in every bucket. That covers the root, so `assert_same_contents` is also the check T3 and T4 need that a build keeps `bulk`'s root;
- every chain, singleton and pending list: the same rows, in the same order, at the same capacity;
- the buffers: the same multiset of rows at the same `value_capacity()`;
- the heap size and the tuple count.

**`presized:N`'s rule (Part B).** `assert_equivalent_trie` compares below the root by contents, as T5 needs.
- **A new private helper.** `assert_equivalent_root_contents` applies `assert_equivalent_root`'s root rule with content-level children. `assert_equivalent_trie` uses it, and checks the buffers as a multiset at the same capacity.
- **`assert_equivalent_root` is unchanged.** Same signature, id-level children, so `parallel.rs` is not touched. `check_one_run` keeps using it on roots built over the input batch, and so does T5.
- **Green today.** Every buffer still equals its input, so the content-level `assert_equivalent_trie` holds wherever the id-level one did.

`identity` is `#[cfg(test)]` (`hash_trie/mod.rs:18–19`), so nothing here needs a dead-code allowance.

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/identity.rs`:
  - module doc, lines 1–13;
  - `use`, line 26;
  - new helpers after `assert_same_trie`, which ends at line 113;
  - a new helper after `assert_equivalent_root`, which ends at line 222;
  - `assert_equivalent_trie`, lines 266–279;
  - a new `mod tests` at the end.
- Test: `kermit-ds/src/ds/hash_trie/identity.rs` (`mod tests`)

- [ ] **Step 1: Check the host (`find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l` prints `0`), then write the failing self-tests.** Append to `kermit-ds/src/ds/hash_trie/identity.rs`, after `assert_equivalent_trie`'s closing `}` (line 279):

```rust

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
    const INTERLEAVED: [[usize; 3]; 6] = [
        [1, 2, 3],
        [2, 3, 4],
        [1, 5, 6],
        [3, 1, 1],
        [2, 3, 9],
        [1, 2, 7],
    ];

    /// Two builds of one input are the same at the content level. Every
    /// node variant meets the comparison: `Inner` and `Leaf` at every arity
    /// above 1, `Singleton` under pruning, `Unexpanded` under lazy
    /// expansion.
    fn check_accepts_the_same_trie<P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for (input, tuples) in inputs(arity) {
                let label = format!("{}/{} arity {arity}, {input}", P::NAME, E::NAME);
                let a = HashTrie::<SipHashStrategy, P, E>::from_tuples(arity.into(), tuples.clone());
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

    /// `tuples`' rows in reverse order, and the ids that name `tuples`' rows
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
    /// names, in that order.
    fn root_over<P: PruningPolicy, E: ExpansionPolicy>(
        tuples: &Tuples, arity: usize, ids: impl IntoIterator<Item = RowId>,
    ) -> HashTrieNode<P, E> {
        let config = HashTrieConfig::default();
        let log2_capacity = config.root_log2_capacity(tuples.len());
        HashTrie::<SipHashStrategy, P, E>::build(tuples, 0, arity, ids, log2_capacity, config)
    }

    /// Calls `check` on every input's root and on the root built over the
    /// reversed batch from its rows in input order: the same tuples under
    /// other ids, at every arity.
    fn for_each_reversed_root<P: PruningPolicy, E: ExpansionPolicy>(
        check: impl Fn(&Tuples, &HashTrieNode<P, E>, &Tuples, &HashTrieNode<P, E>, &str),
    ) {
        for arity in 1..=3 {
            for (input, tuples) in inputs(arity) {
                let label = format!("{}/{} arity {arity}, {input}", P::NAME, E::NAME);
                let (reversed, ids) = reverse_rows(&tuples);
                let forward = root_over::<P, E>(&tuples, arity, all_rows(&tuples));
                let backward = root_over::<P, E>(&reversed, arity, ids);
                check(&tuples, &forward, &reversed, &backward, &label);
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

    /// Guards the test above. Compared by id, the reversed root differs,
    /// so that test passes on the tuples, not because the ids stayed put.
    #[test]
    #[should_panic(expected = ": chain")]
    fn array_level_rejects_a_reversed_buffer() {
        let tuples = rows(2, &INTERLEAVED);
        let (reversed, ids) = reverse_rows(&tuples);
        let forward = root_over::<NoPruning, EagerExpansion>(&tuples, 2, all_rows(&tuples));
        let backward = root_over::<NoPruning, EagerExpansion>(&reversed, 2, ids);
        assert_same_node(&forward, &backward, "reversed");
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

    /// Under lazy expansion every root child is a pending list.
    #[test]
    #[should_panic(expected = "pending")]
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

  **Why each should_panic fixture fails where it says.** Each is traced against `bulk.rs`' `group`/`child`.
  - **`array_level_rejects_a_reversed_buffer`.** Every root child is a `Leaf`, and the reversed ids differ in every chain: 6 rows, no fixed point of `i ↦ 5 − i`.
  - **Chain row.** Row 5 is chain `(1, 2, 7)`'s only id under `NoPruning`/eager.
  - **Singleton row.** Key 3 holds one tuple, so pruning makes it `Singleton(3)` at depth 1.
  - **Pending row.** Under lazy expansion with `NoPruning`, key 3's child is `Unexpanded([3])`.
  - **Chain order.** `[1, 2]` and `[1, 12]` share hash 2 under `Mod10HashStrategy`, so they share the chain at `(1, 2)`.
  - **Root capacity.** Six tuples at 70 %:
    - `root_log2_capacity` gives 16 buckets under `Tuples` (⌈600/70⌉ = 9, rounded up to 16);
    - the grown root doubles to 8 at its third key, since 3 · 100 > 4 · 70;
    - the first assertion of `assert_same_table`, `"{path}: capacity"`, fails.

  The walk runs before the buffer check in `assert_same_contents`, so the hash-alike fixtures are caught by the walk's row comparison (`chain rows`, `singleton row`, `pending: chain rows`), not by the buffer's multiset.

- [ ] **Step 2: Run the tests to verify they fail.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds identity::
  ```
  Expected: compile errors ``error[E0425]: cannot find function `assert_same_contents` in this scope`` and ``… `assert_same_node_contents` …``.

- [ ] **Step 3: Implement.** Three edits in `kermit-ds/src/ds/hash_trie/identity.rs`.

  The module doc (lines 1–13). Before:

```rust
//! Array-level identity checks for the HashTrie builds (#91, #94, #88).
//! Every build mode must build the trie the bulk build does, the
//! root-capacity Config must leave every subtrie as it was, and the
//! `presized:N` build must be equivalent (Amendment 2), so the
//! radix, parallel and root-capacity tests share these assertions and
//! inputs.
//!
//! A trie keeps the batch it is built from as its buffer, unchanged, and its
//! chains, singletons and pending lists hold row ids into it (#111). Two
//! builds of one input therefore own equal buffers, so equal id lists name
//! equal tuples, and comparing ids is as strict as comparing tuples was.
//! [`assert_same_trie`] and [`assert_equivalent_trie`] compare the buffers
//! first, so that premise is checked, not assumed.
```

  After:

```rust
//! Identity checks for the HashTrie builds (#91, #94, #88, #101). Every
//! build mode must build the trie the bulk build does, the root-capacity
//! Config must leave every subtrie as it was, and the `presized:N` build
//! must be equivalent (Amendment 2), so the radix, parallel and
//! root-capacity tests share these assertions and inputs.
//!
//! A trie's chains, singletons and pending lists hold row ids into its
//! buffer (#111), so two tries compare at one of two levels:
//!
//! - **Array level**, [`assert_same_trie`] and [`assert_same_node`]: equal
//!   ids. Equal ids name equal tuples only over equal buffers, so
//!   `assert_same_trie` compares the buffers first. For builds that keep
//!   their input batch as their buffer, as `bulk` and `incremental` do.
//! - **Content level**, [`assert_same_contents`] and
//!   [`assert_same_node_contents`]: every table bucket for bucket, as at
//!   the array level, and every chain, singleton and pending list naming
//!   the same tuples in the same order, each id read through its own
//!   trie's buffer; the buffers hold the same rows at the same capacity.
//!   For builds that may store their rows in another order than they
//!   arrived in (#101 layer 3's partitioned copy).
```

  The `use` (line 26). Before:

```rust
    kermit_iters::{HashStrategy, Tuples},
```

  After:

```rust
    kermit_iters::{HashStrategy, RowId, Tuples},
```

  The new helpers go after `assert_same_trie` (lines 98–113), between its closing `}` and `/// Rows of up to three columns` (line 115). Before (lines 107–116):

```rust
    assert_eq!(
        a.heap_size_bytes(),
        b.heap_size_bytes(),
        "{label}: heap size"
    );
    assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
}

/// Rows of up to three columns, cut to `arity`, as a batch whose capacity
/// is exactly its rows, so a batch and its clone have equal capacities.
```

  After: the same lines, with this inserted between `assert_same_trie`'s closing `}` and `/// Rows of up to three columns`:

```rust
/// The rows `ids` name in `tuples`, in id order.
fn rows_by_id<'t>(tuples: &'t Tuples, ids: &[RowId]) -> Vec<&'t [usize]> {
    ids.iter().map(|&id| tuples.row(id)).collect()
}

/// Two chains or pending lists, each read through its own trie's buffer:
/// the same rows, in the same order, at the same capacity. The ids may
/// differ.
// `&Vec`, not a slice: the comparison reads `capacity()`.
#[allow(clippy::ptr_arg)]
fn assert_same_chain_contents(
    a_tuples: &Tuples, a: &TupleList, b_tuples: &Tuples, b: &TupleList, path: &str,
) {
    assert_eq!(
        rows_by_id(a_tuples, a),
        rows_by_id(b_tuples, b),
        "{path}: chain rows"
    );
    assert_eq!(a.capacity(), b.capacity(), "{path}: chain capacity");
}

/// [`assert_same_node`] by contents: every table bucket for bucket, as
/// there, but every chain, singleton and pending list compared by the
/// tuples its ids name, `a`'s read through `a_tuples` and `b`'s through
/// `b_tuples`.
pub(super) fn assert_same_node_contents<P: PruningPolicy, E: ExpansionPolicy>(
    a_tuples: &Tuples, a: &HashTrieNode<P, E>, b_tuples: &Tuples, b: &HashTrieNode<P, E>,
    path: &str,
) {
    match (a, b) {
        | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
            assert_same_table(x, y, path, &|x, y, path| {
                assert_same_node_contents(a_tuples, x, b_tuples, y, path)
            })
        },
        | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
            assert_same_table(x, y, path, &|x, y, path| {
                assert_same_chain_contents(a_tuples, x, b_tuples, y, path)
            })
        },
        | (HashTrieNode::Singleton(x), HashTrieNode::Singleton(y)) => {
            assert_eq!(
                a_tuples.row(*x.row()),
                b_tuples.row(*y.row()),
                "{path}: singleton row"
            );
        },
        | (HashTrieNode::Unexpanded(x), HashTrieNode::Unexpanded(y)) => {
            // As in `assert_same_node`: building expands nothing.
            assert!(
                x.built().is_none() && y.built().is_none(),
                "{path}: expanded during the build"
            );
            assert_same_chain_contents(
                a_tuples,
                &x.pending(),
                b_tuples,
                &y.pending(),
                &format!("{path}: pending"),
            );
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
/// - every chain, singleton and pending list naming the same tuples in the
///   same order, each read through its own trie's buffer;
/// - buffers holding the same rows at the same capacity;
/// - and the heap size and tuple count.
///
/// The walk runs first, so a row that differs is reported where it sits.
pub(super) fn assert_same_contents<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_same_node_contents(a.tuples(), a.root(), b.tuples(), b.root(), label);
    assert_same_buffer_contents(a.tuples(), b.tuples(), label);
    assert_eq!(
        a.heap_size_bytes(),
        b.heap_size_bytes(),
        "{label}: heap size"
    );
    assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
}
```

  Notes:
  - **Accessors.** Each helper reuses the accessors `identity.rs` already uses: `assert_same_table`, which reads `buckets_len`, `shell_heap_bytes`, `len`, `hash_at` and `value_at` per bucket; `SingletonPayload::row`; and `PendingChild::{built, pending}`.
  - **Pending lists.** `&x.pending()` is a `&Ref<Vec<RowId>>`, which deref-coerces to `&TupleList`, as at line 79.
  - **The fn items in Step 1.** The tests pass `assert_same_node_contents` (and, in Step 5, `assert_equivalent_root_contents`) as fn items to `impl Fn(&Tuples, &HashTrieNode<P, E>, &Tuples, &HashTrieNode<P, E>, &str)`. `P` and `E` are inferred from the bound, and the elided lifetimes are late-bound, so the higher-ranked bound holds.

- [ ] **Step 4: Run the tests to verify they pass.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds identity::
  ```
  Expected: `test result: ok. 9 passed; 0 failed` (all in `ds::hash_trie::identity::tests`; the six `should_panic` ones report `ok`).

- [ ] **Step 5: Content comparison below the equivalent root (Part B; T5 relies on it).** `assert_equivalent_root` stays as it is, signature and id-level children both.

  In `identity.rs`, insert after `assert_equivalent_root`, between its closing `}` (line 222) and `fn assert_equivalent_table` (line 224):

```rust
/// [`assert_equivalent_root`]'s root rule with every key's child or chain
/// compared by contents ([`assert_same_node_contents`]), `a`'s read through
/// `a_tuples` and `b`'s through `b_tuples`: the rule for builds that may
/// also store their rows in another order (#101 layer 3).
fn assert_equivalent_root_contents<P: PruningPolicy, E: ExpansionPolicy>(
    a_tuples: &Tuples, a: &HashTrieNode<P, E>, b_tuples: &Tuples, b: &HashTrieNode<P, E>,
    label: &str,
) {
    match (a, b) {
        | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
            assert_equivalent_table(x, y, label, &|x, y, path| {
                assert_same_node_contents(a_tuples, x, b_tuples, y, path)
            })
        },
        | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
            assert_equivalent_table(x, y, label, &|x, y, path| {
                assert_same_chain_contents(a_tuples, x, b_tuples, y, path)
            })
        },
        | _ => panic!("{label}: root variants differ"),
    }
}
```

  `assert_equivalent_trie` (lines 266–279). Before:

```rust
/// [`assert_equivalent_root`] for whole tries, plus the buffer, heap size
/// and tuple count.
pub(super) fn assert_equivalent_trie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_same_buffer(a, b, label);
    assert_equivalent_root(a.root(), b.root(), label);
```

  After (the heap-size and tuple-count assertions below are unchanged):

```rust
/// `presized:N`'s rule for whole tries: [`assert_equivalent_root`]'s root
/// rule with every child compared by contents, buffers holding the same
/// rows at the same capacity, and the same heap size and tuple count.
pub(super) fn assert_equivalent_trie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_same_buffer_contents(a.tuples(), b.tuples(), label);
    assert_equivalent_root_contents(a.tuples(), a.root(), b.tuples(), b.root(), label);
```

  `assert_same_buffer` is still used by `assert_same_trie`, so no dead code results.

  The module doc gains a closing paragraph. After the content-level bullet from Step 3 (ending `(#101 layer 3's partitioned copy).`), append:

```rust
//!
//! `presized:N` may place root keys in other buckets (Amendment 2):
//! [`assert_equivalent_root`] checks its root rule over roots built from
//! one input batch, with children compared by id, and
//! [`assert_equivalent_trie`] checks whole tries, with children compared
//! by contents.
```

  The self-test, in `identity.rs`' `mod tests`, after `same_node_contents_accepts_a_reversed_buffer`:

```rust
    /// `assert_equivalent_trie`'s root rule reads each child through its
    /// own buffer, so a reversed buffer's root is equivalent to the forward
    /// one.
    #[test]
    fn equivalent_root_contents_accepts_a_reversed_buffer() {
        for_each_reversed_root::<NoPruning, EagerExpansion>(assert_equivalent_root_contents);
        for_each_reversed_root::<SingletonPruning, EagerExpansion>(assert_equivalent_root_contents);
        for_each_reversed_root::<NoPruning, LazyExpansion>(assert_equivalent_root_contents);
        for_each_reversed_root::<SingletonPruning, LazyExpansion>(assert_equivalent_root_contents);
    }
```

  `assert_equivalent_root_contents` is private to `identity`. The child module `tests` reaches it through `use super::*`.

- [ ] **Step 6: Run the tests.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds identity::
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_trie::
  ```
  Expected:
  - `identity::`: `test result: ok. 10 passed; 0 failed`.
  - `hash_trie::`: `test result: ok.` with 0 failed. That includes the 17 `hash_trie::parallel::tests`:
    - `check_presized`'s `assert_equivalent_trie` now compares children by contents, and passes because every buffer is still the input batch;
    - its `assert_equivalent_root` calls (lines 697, 786, 974) are unchanged.

- [ ] **Step 7: Lint and format.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  ```
  Expected: clean. `cargo doc` is unaffected: `identity` is `cfg(test)`. `git diff --stat` lists only `identity.rs`.

- [ ] **Step 8: Commit.**
  ```bash
  git -C $WT add kermit-ds/src/ds/hash_trie/identity.rs
  git -C $WT commit -m "test(hash-trie): content-level identity helpers (#101)

  assert_same_contents compares two tries node by node: every table
  bucket for bucket (the root included), every chain, singleton and
  pending list by the tuples its ids name in its own trie's buffer, the
  buffers as a multiset at the same capacity, the heap size and the
  tuple count. A build that copies its rows into partition order can
  only be compared this way. assert_equivalent_trie now compares below
  the root by contents, so presized:N's rule survives that copy;
  assert_equivalent_root stays id-level for roots built over one input.
  Self-tests cover every node variant, a reversed buffer, and rows that
  hash alike.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee"
  ```

- [ ] **Step 9: Mutation checks** (ground rules as in Task 1).
  1. **Chains compared by id.** In `assert_same_chain_contents`, change `rows_by_id(a_tuples, a),` / `rows_by_id(b_tuples, b),` to `a,` / `b,`.
     - Run `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds identity::tests::same_node_contents_accepts_a_reversed_buffer`. Expected: FAILED, `chain rows`.
     - `same_contents_rejects_a_chain_row_that_hashes_alike` also fails: the test did not panic. An unused-function warning for `rows_by_id` is expected under the mutant.
  2. **A singleton read through the wrong buffer.** In `assert_same_node_contents`, change `b_tuples.row(*y.row()),` to `a_tuples.row(*y.row()),`.
     - Run `… cargo test -p kermit-ds identity::tests::same_contents_rejects_a_singleton_row_that_hashes_alike`. Expected: FAILED, "test did not panic as expected".
  3. **The equivalent trie's children compared by id again.** In `assert_equivalent_root_contents`'s `Inner` arm, change `assert_same_node_contents(a_tuples, x, b_tuples, y, path)` to `assert_same_node(x, y, path)`.
     - Run `… cargo test -p kermit-ds identity::tests::equivalent_root_contents_accepts_a_reversed_buffer`. Expected: FAILED, `: chain`.

---

### Task 3: `radix:K` materialises its partitions

`radix:K` copies its rows into first-key-hash partition order through `morsel::partition_rows`
(T1). The copy becomes the trie's buffer and the input batch is freed. `radix::partition` is
deleted. Each partition's scratch root reads one contiguous range of the copy. `first_seen`
records each key's *input* position through `origin`, so the merge still inserts root keys in
first-appearance order and the root stays bucket for bucket `bulk`'s (spec Decision 3).
`parallel:N` and `presized:N` do not materialise yet: their closures return the batch
unchanged, and `parallel:N` passes an identity `origin` (see "Why an identity `Vec`").

**One morsel** (coordinator decision, RECONCILE 10(a)). `radix:K` runs `partition_rows` on one
thread with the whole batch as one morsel (`tuples.len().max(1)`), not `MORSEL_TUPLES`.
- `partition_rows` keeps a count and a range per (partition, morsel) pair.
- At `radix:16` on 10⁷ rows in 16 384-row morsels that is 611 × 65 536 ≈ 40 M pairs, about
  1.6 GB.
- One thread gains nothing from several morsels.

`parallel:N` and `presized:N` keep `MORSEL_TUPLES`: they cut at most 4N partitions, rounded
up to a power of two. `radix.rs` documents the choice where it calls `partition_rows`.

**State assumed:** T1 (`Tuples::value_capacity`, `morsel::Partitioned`, `morsel::partition_rows`
with `ids(p)` and `partitions()`) and T2 (`identity::assert_same_contents`) have landed.

**Why an identity `Vec` for `parallel:N` in this task.** `build_scratch_root` gains
`origin: &[RowId]` (the contract's final signature). Until T4, `parallel:N` still scatters ids
with `scatter_rows`, so a row's id *is* its input position and `origin` is the identity. The
options were:
- **A `Vec` of `0..len` (chosen).** It costs one 4-byte-per-row allocation and one sequential
  write per `parallel:N` build, for one commit only: T4 replaces it with `&partitioned.origin`
  and changes nothing else at the call. `tuple_allocation.rs`'s `parallel:1` cell is
  differential (`many <= once + bound`), and one more allocation at both sizes cancels out.
  No timing is taken between T3 and T4.
- **A closure (`impl Fn(RowId) -> RowId`).** It is free at run time, but it either stays,
  which changes the contract's signature and makes `radix:K` pass `|id| origin[id as usize]`,
  or T4 has to change the signature back.
- **`Option<&[RowId]>`.** This adds a branch per new key, and an API that lives for one commit.

**Why no separate root assertion.** T2's `assert_same_contents` compares every table,
depth 0 included, bucket by bucket. It checks the same capacity, the same occupied
positions, and the same hash at every position. At depth 0 that is exactly "the root is
`bulk`'s, bucket for bucket". A separate root check would repeat it word for word. The
`origin` mutant in Step 10 is the evidence: it changes only the root's insertion order, and
`assert_same_contents` must reject it. **Step 0 checks the premise.**

**Why `space` stays identical** (no test in `kermit/tests` or `kermit-ds/tests` compares
`space` across modes; the radix tests below assert `heap_size_bytes` equality against `bulk`).
`HashTrie::heap_size_bytes` (implementation.rs:721–723) is the buffer's
`data.capacity() × 8` plus every table's bucket array plus every id list's
`capacity() × 4`. The three parts compare as follows:
- **The buffer.** The copy reserves `tuples.value_capacity()`, which equals the input's
  capacity, and `bulk` keeps the input.
- **The tables.** The root is `bulk`'s. Every child is built from the same tuples, in the same
  order, with the same hashes and config.
- **The id lists.** They receive the same number of pushes in the same order, so they reach
  the same capacities.

`origin` and `offsets` are dropped when `fill_root` returns.

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/radix.rs` (the whole file: module doc, imports, `Arrival`, `fill_root`; delete `partition`; `build_scratch_root`, `take_in_arrival_order`, `insert_in_first_appearance_order`, `mod tests`)
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (Storage doc 70–80, Construction doc 93–97, field doc 127–129, `tuples()`/`collect_tuples` docs 177–186, the three closures 505–534, `from_tuples_partitioned` 615–631, `project` comment 701–702, `TupleScan` doc 727)
- Modify: `kermit-ds/src/ds/hash_trie/parallel.rs` (imports 53–55, the dispatch 187–193, test imports 391–394, `build_modes_reach_their_builds` 576–625)
- Modify: `kermit-ds/src/morsel.rs` (only T1's `#[allow(dead_code)]` markers)
- Modify: `kermit-ds/tests/hash_trie_tests.rs` (comment 149–153)
- Modify: `kermit/tests/join_tests.rs` (comment 338–344)
- Not edited here, handed to T7 (RECONCILE 5 and 8; T7 has the texts):
  - `kermit-ds/src/ds/hash_trie/build_mode.rs`, the enum and variant docs and `RadixBits`' docs;
  - `kermit/tests/cli_hash_trie_build_mode.rs`, module doc 5–7;
  - `kermit/src/execution.rs`, doc comments 1846–1848 and 1863–1865.
- Not edited: `kermit-ds/src/ds/hash_trie/identity.rs`'s module doc, which T2 already rewrote.

**Other callers checked (no edit needed):**
- `radix::partition` has one caller, `radix::fill_root` (radix.rs:46).
- `build_scratch_root` has two callers, `radix::fill_root` (radix.rs:50) and
  `parallel::fill_root_in_morsels` (parallel.rs:191). No test calls either function, and
  both are `pub(super)`, so nothing outside `hash_trie` can.
- **Radix tests that stay valid unchanged:**
  - `bulk.rs:405–425` (`check_children_sized`, capacities only).
  - `hash_table.rs:920` (probe costs).
  - `kermit-ds/tests/hash_trie_tests.rs` `HashTrieSipRadix2` / `HashTrieSipLazyRadix2` (iterator contract).
  - `kermit-ds/tests/parquet_tests.rs:153` (sorted round trip).
  - `kermit-ds/src/relation.rs:945` (`hash_rows` sorts).
  - `kermit/tests/join_tests.rs` `Radix2` suites (answers).
  - `kermit/tests/cli_hash_trie_build_mode.rs` (axes only; no cross-mode `space` comparison).
  - `kermit/src/execution.rs:1866` `hash_families_build_with_their_mode` (`radixed > bulk` still
    holds: the radix build hashes every row's first attribute in `partition_rows` and again in
    its scratch root).
  - `kermit/src/execution.rs:1927` (thread records; radix records none).
- **Radix tests that would fail unchanged, both fixed in Step 5:**
  - `parallel.rs:588` `build_modes_reach_their_builds` asserts `assert_same_trie` against
    `bulk` and `worker_runs == []` for `radix:3`.
  - `radix.rs:259` `build_mode_relation_uses_the_default_config` asserts `assert_same_trie`
    between `from_tuples` and `radix:2`.

- [ ] **Step 0: Host check, and the premise of the root identity.**

  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  rg -n -A3 "fn assert_same_contents" kermit-ds/src/ds/hash_trie/identity.rs | rg "assert_same_node_contents"
  rg -n -A14 "fn assert_same_node_contents" kermit-ds/src/ds/hash_trie/identity.rs | rg "assert_same_table"
  rg -n -A16 "fn assert_same_table" kermit-ds/src/ds/hash_trie/identity.rs | rg "hash_at"
  ```
  Expected:
  - the first command prints `0`;
  - the second shows `assert_same_contents` calling
    `assert_same_node_contents(a.tuples(), a.root(), b.tuples(), b.root(), label)`, so the walk
    starts at the root;
  - the third prints two lines, the `Inner` and `Leaf` arms calling `assert_same_table`;
  - the fourth prints `assert_eq!(a.hash_at(idx), b.hash_at(idx), "{path}: bucket {idx}");`,
    inside the loop over every bucket index.

  Together: `assert_same_contents` compares the root position by position. If any of these
  prints nothing, or the table check compares only a set of occupied buckets, stop and report:
  this task's root identity would need its own assertion.

- [ ] **Step 1: Write the failing pin test.** In `kermit-ds/src/ds/hash_trie/radix.rs`, add
  the T1 imports to the module's `use` block (lines 17–29). Before:

  ```rust
          pruning::PruningPolicy,
      },
      kermit_iters::{HashStrategy, RowId, Tuples},
  };
  ```
  After:
  ```rust
          pruning::PruningPolicy,
      },
      crate::morsel::{partition_rows, Threads},
      kermit_iters::{HashStrategy, RowId, Tuples},
  };
  ```
  Append this test to `mod tests`, after `build_mode_relation_uses_the_default_config`
  (line 271). The tests reach the two imports through `use super::*`. The expected copy is
  computed as the build computes it, one thread and one morsel; `partition_rows`' output does
  not depend on either (T1's `partition_rows_is_the_same_on_every_thread_count`).

  ```rust
      /// The copy is what `radix:K` adds, and the content tests cannot see
      /// it: a build that left the rows in arrival order would build the same
      /// contents. So the trie's buffer must be `partition_rows`' copy of its
      /// input, partitioned on the top `K` bits of the first attribute's hash,
      /// row for row and at the input's capacity. The random input spreads
      /// over every bit count's partitions, so its copy is not the input and
      /// the test cannot pass vacuously.
      #[test]
      fn radix_keeps_the_partitioned_copy_as_its_buffer() {
          let one = Threads::new(1).expect("one thread is a valid count");
          for arity in 1..=3 {
              for &bits in BITS {
                  let shift = 64 - u32::from(bits);
                  let radix = HashTrieBuildMode::Radix(RadixBits::new(bits).unwrap());
                  for (input, tuples) in inputs(arity) {
                      let label = format!("arity {arity}, radix:{bits}, {input}");
                      let one_morsel = tuples.len().max(1);
                      let expected = partition_rows(one, &tuples, one_morsel, 1 << bits, |row| {
                          (SipHashStrategy::hash(row[0]) >> shift) as usize
                      });
                      if input == "random" {
                          assert_ne!(expected.tuples, tuples, "{label}: the copy reorders");
                      }
                      let built: HashTrie =
                          HashTrie::from_tuples_with_build_mode(arity.into(), radix, tuples.clone());
                      assert_eq!(built.tuples(), &expected.tuples, "{label}: buffer");
                      assert_eq!(
                          built.tuples().value_capacity(),
                          tuples.value_capacity(),
                          "{label}: capacity"
                      );
                  }
              }
          }
      }
  ```

- [ ] **Step 2: Run it to verify it fails.**

  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds radix_keeps_the_partitioned_copy
  ```
  Expected: 1 test, FAILED. The panic is the `assert_eq!` labelled `…: buffer`, at the
  `interleaved` or `random` input. Today the trie keeps the input in arrival order, while
  `expected` is in partition order. The non-test library build warns that `partition_rows`
  and `Threads` are unused; Step 5 uses them.

- [ ] **Step 3: Make the radix tests content-level.** They must pass on today's code, since an
  array-identical trie is content-identical; they are the pinning tests for Step 4. In
  `radix.rs` `mod tests`, replace the `use` block (lines 163–177). Before:

  ```rust
      use {
          super::*,
          crate::{
              ds::hash_trie::{
                  build_mode::HashTrieBuildMode,
                  config::HashTrieConfig,
                  expansion::{EagerExpansion, LazyExpansion},
                  identity::{assert_same_trie, configs, inputs, rows},
                  pruning::{NoPruning, SingletonPruning},
              },
              relation::{BuildModeRelation, ConfigurableRelation, Relation},
              test_support::Mod10HashStrategy,
          },
          kermit_iters::{FxHashStrategy, SipHashStrategy},
      };
  ```
  After:
  ```rust
      use {
          super::*,
          crate::{
              ds::hash_trie::{
                  build_mode::HashTrieBuildMode,
                  config::HashTrieConfig,
                  expansion::{EagerExpansion, LazyExpansion},
                  identity::{assert_same_contents, assert_same_trie, configs, inputs, rows},
                  pruning::{NoPruning, SingletonPruning},
              },
              heap_size::HeapSize,
              relation::{BuildModeRelation, ConfigurableRelation, Relation},
              test_support::Mod10HashStrategy,
          },
          kermit_iters::{FxHashStrategy, SipHashStrategy},
      };
  ```
  Replace `check_identity` (lines 187–218) with `check_contents`:

  ```rust
      /// `radix:K` builds a trie equivalent to `bulk`'s, for every arity, load
      /// factor, root capacity, child capacity, bit count and input.
      /// `assert_same_contents` compares every table bucket for bucket, so the
      /// root is `bulk`'s (Decision 3 of the partitioned-materialisation
      /// design), and every chain, singleton and pending list holds the same
      /// tuples in the same order under the copy's row ids. The heap size is
      /// compared too: `space` must not move with the mode.
      fn check_contents<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
          for arity in 1..=3 {
              for config in configs() {
                  for &bits in BITS {
                      let radix = HashTrieBuildMode::Radix(RadixBits::new(bits).unwrap());
                      for (input, tuples) in inputs(arity) {
                          let build = |mode| {
                              HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
                                  arity.into(),
                                  config,
                                  mode,
                                  tuples.clone(),
                              )
                          };
                          let label = format!(
                              "{}/{}/{} arity {arity}, load {}%, root {}, children {}, \
                               radix:{bits}, {input}",
                              H::NAME,
                              P::NAME,
                              E::NAME,
                              config.load_factor.numerator(),
                              config.root_capacity.axis_value(),
                              config.child_capacity.axis_value(),
                          );
                          let (bulk, built) = (build(HashTrieBuildMode::Bulk), build(radix));
                          assert_same_contents(&bulk, &built, &label);
                          assert_eq!(
                              bulk.heap_size_bytes(),
                              built.heap_size_bytes(),
                              "{label}: heap size"
                          );
                      }
                  }
              }
          }
      }
  ```
  In the three `radix_builds_the_bulk_trie_*` tests (lines 220–244), replace every
  `check_identity::<` with `check_contents::<` (12 calls). The test names stay.
  In `build_mode_relation_uses_the_default_config` (lines 259–271), the last line becomes
  content-level. Before:
  ```rust
          assert_same_trie(&plain, &built, "from_tuples vs radix:2");
  ```
  After:
  ```rust
          assert_same_contents(&plain, &built, "from_tuples vs radix:2");
  ```
  The line before it (`assert_same_trie(&plain, &default, …)`) stays array-level:
  `from_tuples` and the default mode are both `bulk`.

- [ ] **Step 4: Run the radix tests on today's code.**

  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_trie::radix::tests
  ```
  Expected: 6 tests. 5 pass: the three `radix_builds_the_bulk_trie_*`,
  `radix_build_rejects_a_wrong_arity`, and `build_mode_relation_uses_the_default_config`
  (if it fails here, the old radix build was not content-equal to bulk: stop). 1 fails:
  `radix_keeps_the_partitioned_copy_as_its_buffer`, as in Step 2.

**Chunk boundary (C03a ends here; C03b starts).** C03a leaves the tree compiling, with the pin
failing by design and nothing committed. C03b starts from that working tree: it re-runs Step 4's
command first and expects the same result (5 pass, 1 fails).

- [ ] **Step 5: Implement.**

  **5a. `radix.rs`, everything above `mod tests`.** Replace lines 1–159 (module doc through
  `insert_in_first_appearance_order`) by the following, which keeps Step 1's import. The
  `all_rows` import goes, since only `partition` used it, and `fn partition` is deleted.
  `fill_root` passes the whole batch as one morsel (RECONCILE 10(a)) and says why:

  ```rust
  //! The `radix:K` build of a [`HashTrie`](super::HashTrie): copy the rows
  //! into `2^K` partitions on their first attribute's hash
  //! (`morsel::partition_rows`), group each partition into a scratch root and
  //! build its children by Algorithm 2 (`bulk.rs`), then merge the scratch
  //! roots into the real one (SIGMOD 2020 §3.3.2; issues #91, #101).
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
  //! Below the root nothing needs care: the copy is stable, so each root
  //! key's child is built from the same tuples, in input order, as in the
  //! bulk build.

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
      crate::morsel::{partition_rows, Threads},
      kermit_iters::{HashStrategy, RowId, Tuples},
  };

  /// A root entry ready to merge: the input position of the row whose key
  /// first introduced it, its hash, and its finished value.
  pub(super) type Arrival<V> = (RowId, u64, V);

  /// Fills the empty `root` with the rows of `tuples` by the `radix:bits`
  /// build and returns the trie's buffer: the rows copied into partition
  /// order. Every row must have `arity` attributes; the caller checks.
  pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
      tuples: Tuples, root: &mut HashTrieNode<P, E>, arity: usize, bits: RadixBits,
      config: HashTrieConfig,
  ) -> Tuples {
      // 1. Partition, on this thread: copy the rows into `2^bits` partitions
      //    on the top `bits` of their first attribute's hash. Each partition
      //    is one range of the copy, in input order. The whole batch is one
      //    morsel: `partition_rows` keeps a count and a range per (partition,
      //    morsel) pair, which one thread gains nothing from, and at
      //    `radix:16` over 10^7 rows in `MORSEL_TUPLES`-row morsels would be
      //    about 40 million pairs.
      let shift = 64 - u32::from(bits.get());
      let one = Threads::new(1).expect("one thread is a valid count");
      let one_morsel = tuples.len().max(1);
      let partitioned = partition_rows(one, &tuples, one_morsel, 1 << bits.get(), |row| {
          (H::hash(row[0]) >> shift) as usize
      });
      // The copy is the buffer from here on: free the input before the build,
      // so the build holds the rows once.
      drop(tuples);
      // 2. Build each partition into a scratch root. One of the two lists
      //    stays empty: the root is `Inner` for arity ≥ 2 (values are
      //    subtries, or under lazy expansion their pending lists) and `Leaf`
      //    for arity 1 (values are chains).
      let mut subtries = Vec::new();
      let mut chains = Vec::new();
      for partition in 0..partitioned.partitions() {
          let ids = partitioned.ids(partition);
          if ids.is_empty() {
              continue;
          }
          let (scratch, first_seen) = build_scratch_root::<H, P, E>(
              &partitioned.tuples,
              ids,
              &partitioned.origin,
              arity,
              config,
          );
          match scratch {
              | HashTrieNode::Inner(table) => {
                  take_in_arrival_order(table, &first_seen, &mut subtries)
              },
              | HashTrieNode::Leaf(table) => take_in_arrival_order(table, &first_seen, &mut chains),
              | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
                  unreachable!("a root is never pruned or unexpanded")
              },
          }
      }
      // 3. Merge, in first-appearance order.
      match root {
          | HashTrieNode::Inner(table) => {
              insert_in_first_appearance_order(table, subtries, config.load_factor)
          },
          | HashTrieNode::Leaf(table) => {
              insert_in_first_appearance_order(table, chains, config.load_factor)
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
  pub(super) fn take_in_arrival_order<V>(
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

  /// Inserts the merged entries into the real root in the order their keys
  /// first appeared in the input — the order the bulk build inserts them —
  /// so the root's buckets, length and capacity are the bulk build's.
  fn insert_in_first_appearance_order<V>(
      root: &mut HashTable<V>, mut arrivals: Vec<Arrival<V>>, load_factor: LoadFactor,
  ) {
      // Input positions are distinct (`origin` is a permutation), so the
      // unstable sort is deterministic.
      arrivals.sort_unstable_by_key(|&(first, ..)| first);
      for (_, hash, value) in arrivals {
          root.entry_or_insert_with(hash, load_factor, || value);
      }
  }
  ```

  **5b. `implementation.rs`, `from_tuples_partitioned`** (lines 615–631). Before:

  ```rust
      /// What the partitioned builds share (`radix:K`, `parallel:N`,
      /// `presized:N`): the batch's arity check, then `fill`, which builds the
      /// root over the batch at the capacity every build gives it
      /// (`HashTrieConfig::root_log2_capacity`, presized under
      /// `root-capacity=tuples`). The trie keeps the batch as its buffer.
      fn from_tuples_partitioned(
          header: RelationHeader, config: HashTrieConfig, tuples: Tuples,
          fill: impl FnOnce(&Tuples, u32, usize) -> HashTrieNode<P, E>,
      ) -> Self {
          let arity = header.arity();
          let tuples = Self::batch_for_header(arity, tuples);
          let root = fill(&tuples, config.root_log2_capacity(tuples.len()), arity);
          Self {
  ```
  After:
  ```rust
      /// What the partitioned builds share (`radix:K`, `parallel:N`,
      /// `presized:N`): the batch's arity check, then `fill`, which builds the
      /// root over the batch at the capacity every build gives it
      /// (`HashTrieConfig::root_log2_capacity`, presized under
      /// `root-capacity=tuples`) and returns it with the trie's buffer: the
      /// batch itself, or, from a build that materialises its partitions, the
      /// batch's rows copied into partition order (`morsel::partition_rows`,
      /// #101), the batch then freed.
      fn from_tuples_partitioned(
          header: RelationHeader, config: HashTrieConfig, tuples: Tuples,
          fill: impl FnOnce(Tuples, u32, usize) -> (Tuples, HashTrieNode<P, E>),
      ) -> Self {
          let arity = header.arity();
          let tuples = Self::batch_for_header(arity, tuples);
          let log2_capacity = config.root_log2_capacity(tuples.len());
          let (tuples, root) = fill(tuples, log2_capacity, arity);
          Self {
  ```
  The struct literal after it (`header, tuples, root, config, _layout: PhantomData`) is
  unchanged.

  **5c. `implementation.rs`, the three closures** (lines 505–534). Before:

  ```rust
              | HashTrieBuildMode::Radix(bits) => {
                  Self::from_tuples_partitioned(header, config, tuples, |tuples, log2, arity| {
                      let mut root = Self::make_root_sized(arity, log2);
                      radix::fill_root::<H, P, E>(tuples, &mut root, arity, bits, config);
                      root
                  })
              },
              | HashTrieBuildMode::Parallel(threads) => {
                  Self::from_tuples_partitioned(header, config, tuples, |tuples, log2, arity| {
                      let mut root = Self::make_root_sized(arity, log2);
                      parallel::fill_root::<H, P, E>(tuples, &mut root, arity, threads, config);
                      root
                  })
              },
  ```
  After:
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
                      parallel::fill_root::<H, P, E>(&tuples, &mut root, arity, threads, config);
                      (tuples, root)
                  })
              },
  ```
  And in the `Presized` arm (lines 532–534). Before:
  ```rust
                  Self::from_tuples_partitioned(header, config, tuples, |tuples, log2, arity| {
                      parallel::fill_presized_root::<H, P, E>(tuples, log2, arity, threads, config)
                  })
  ```
  After:
  ```rust
                  Self::from_tuples_partitioned(header, config, tuples, |tuples, log2, arity| {
                      let root = parallel::fill_presized_root::<H, P, E>(
                          &tuples,
                          log2,
                          arity,
                          threads,
                          config,
                      );
                      (tuples, root)
                  })
  ```
  (`cargo fmt` in Step 8 may reflow these call arguments; accept its layout.)

  **5d. `parallel.rs`, the identity `origin`.** The import (lines 53–55). Before:
  ```rust
      crate::morsel::{
          dispatch, scatter_rows, RowPartition, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD,
      },
  ```
  After:
  ```rust
      crate::morsel::{
          dispatch, row_id, scatter_rows, RowPartition, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD,
      },
  ```
  The dispatch (lines 187–193). Before:
  ```rust
      // 2. Build, on `threads` workers: each partition into a scratch root, whose
      //    entries then leave in the order their keys arrived.
      let entries = dispatch(threads, partitions, |partition| {
          let (scratch, first_seen) =
              radix::build_scratch_root::<H, P, E>(tuples, partition.ids(), arity, config);
          Entries::take_from(scratch, &first_seen)
      });
  ```
  After:
  ```rust
      // This build partitions row ids, not rows, so a row's id is its input
      // position: `origin` is the identity.
      let origin: Vec<RowId> = (0..row_id(tuples.len())).collect();
      // 2. Build, on `threads` workers: each partition into a scratch root, whose
      //    entries then leave in the order their keys arrived.
      let entries = dispatch(threads, partitions, |partition| {
          let (scratch, first_seen) =
              radix::build_scratch_root::<H, P, E>(tuples, partition.ids(), &origin, arity, config);
          Entries::take_from(scratch, &first_seen)
      });
  ```
  T4 deletes the two `origin` lines and the `row_id` import, and passes `&partitioned.origin`.

  **5e. `parallel.rs`, `build_modes_reach_their_builds`.** `radix:3` now runs
  `partition_rows`, so its worker runs are no longer empty, and its buffer is no longer
  `bulk`'s. The test-module import (lines 391–394). Before:
  ```rust
                  identity::{
                      assert_equivalent_root, assert_equivalent_trie, assert_same_node,
                      assert_same_trie, configs, inputs, LOAD_PERCENTS,
                  },
  ```
  After:
  ```rust
                  identity::{
                      assert_equivalent_root, assert_equivalent_trie, assert_same_contents,
                      assert_same_node, assert_same_trie, configs, inputs, LOAD_PERCENTS,
                  },
  ```
  The doc (lines 576–586). Before:
  ```rust
      /// Every mode builds the same trie, so only the records show which build
      /// ran (`bulk` and `incremental` record nothing here;
      /// `each_build_mode_runs_its_own_build` tells them apart), and that
      /// `parallel:N` spreads its work: N workers in both steps (two
      /// `run_workers` calls), and the tuples spread over the partitions.
      /// Eight first keys of eight tuples each fill the eight partitions of
      /// `parallel:2` evenly. `parallel:3` aims at 12 partitions, rounded up to
      /// 16, so key k lands in partition 2k: every other partition is empty,
      /// and the record lists those too. A second thread count is what shows
      /// that N itself reaches the build. The radix build partitions too, but
      /// serially.
  ```
  After:
  ```rust
      /// Every mode builds the same contents, so only the records show which
      /// build ran (`bulk` and `incremental` record nothing here;
      /// `each_build_mode_runs_its_own_build` tells them apart), and that
      /// `parallel:N` spreads its work: N workers in both steps (two
      /// `run_workers` calls), and the tuples spread over the partitions.
      /// Eight first keys of eight tuples each fill the eight partitions of
      /// `parallel:2` evenly. `parallel:3` aims at 12 partitions, rounded up to
      /// 16, so key k lands in partition 2k: every other partition is empty,
      /// and the record lists those too. A second thread count is what shows
      /// that N itself reaches the build. The radix build partitions too, on
      /// the calling thread alone: `partition_rows`' histogram and scatter
      /// passes, one worker each, and no record.
  ```
  The radix row (lines 594–598). Before:
  ```rust
              (
                  HashTrieBuildMode::Radix(RadixBits::new(3).unwrap()),
                  vec![],
                  vec![],
              ),
  ```
  After:
  ```rust
              (
                  HashTrieBuildMode::Radix(RadixBits::new(3).unwrap()),
                  vec![],
                  vec![1, 1],
              ),
  ```
  `vec![1, 1]` assumes T1's `partition_rows` makes two `run_workers` calls, the histogram
  pass and the scatter pass, as the contract describes. If T1's `partition_rows` makes a
  different number, use that count of `1`s; T1's own worker-run test pins the number.
  The last line of the loop (line 624). Before:
  ```rust
              assert_same_trie(&bulk, &built, &format!("{mode:?}"));
  ```
  After:
  ```rust
              assert_same_contents(&bulk, &built, &format!("{mode:?}"));
  ```
  (`bulk`, `incremental` and `parallel:N` keep their own array-level identity tests:
  `bulk.rs` `check_identity`, and `parallel.rs` `check_identity`.)

  **5f. `morsel.rs`: T1's dead-code markers.**
  ```bash
  rg -n -A1 "allow\(dead_code\)" kermit-ds/src/morsel.rs
  ```
  T1 put exactly three markers, each with the comment `// No caller until the radix family
  copies its rows (#101).`: on `struct Partitioned`, on `impl Partitioned` and on
  `partition_rows`. Remove all three lines; this task uses every item they cover (`Cursor` is
  live through `partition_rows`). Afterwards the `rg` prints nothing. If clippy (Step 8) then
  reports dead code in `morsel.rs`, an item T1 added has no caller here: report it rather than
  re-adding a marker.

- [ ] **Step 6: Run the tests.**

  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_trie::radix::tests
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds build_modes_reach_their_builds
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds
  ```
  Expected: 6 passed (the pin included), then 1 passed, then every `kermit-ds` test result
  `ok`. Unchanged and passing: the `parallel.rs` and `bulk.rs` identity tests (parallel
  still scatters ids, with an identity `origin`), and `tuples_sizes_every_child_from_its_list`.
  Then the `kermit` tests that build `radix:K`:
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test tuple_allocation --test join_tests --test cli_hash_trie_build_mode
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit hash_famil
  ```
  Expected: all `ok`.
  - `hash_trie_builds_allocate_per_list_not_per_row` still holds its bound: `radix:2`'s
    `partition_rows` allocations are the same at both repetitions (one morsel), and
    `parallel:1`'s identity `origin` is one allocation at both.
  - `hash_families_build_with_their_mode` and `hash_trie_families_build_with_their_parallel_mode`
    pass.

- [ ] **Step 7: Make the comments that this task falsifies true.** Each edit below is
  wrapped at 80 columns, as `wrap_comments` requires.

  `implementation.rs`, the Storage section (lines 72–74). Before:
  ```rust
  /// The trie owns its tuples as one row-major buffer ([`Tuples`], #111), in
  /// arrival order: the batch a build is given, kept as is, then every row
  /// `insert` appends. Below the tables everything names a tuple by its
  ```
  After:
  ```rust
  /// The trie owns its tuples as one row-major buffer ([`Tuples`], #111): the
  /// batch a build is given, then every row `insert` appends. `bulk` and
  /// `incremental` keep the batch as is, in arrival order; `radix:K` keeps
  /// its copy of the batch instead, in partition order, so each partition's
  /// rows are contiguous (#101). Below the tables everything names a tuple
  /// by its
  ```
  `cargo fmt` re-wraps the paragraph's tail; accept it.

  The Construction section (lines 93–97). Before:
  ```rust
  /// can also be built by the `incremental`, `radix:K`, `parallel:N` and
  /// `presized:N` BuildModes
  /// ([`from_tuples_with_config_and_build_mode`](Self::from_tuples_with_config_and_build_mode),
  /// or [`BuildModeRelation`]), which build the identical trie (`presized:N`
  /// an equivalent one, Amendment 2).
  ```
  After:
  ```rust
  /// can also be built by the `incremental`, `radix:K`, `parallel:N` and
  /// `presized:N` BuildModes
  /// ([`from_tuples_with_config_and_build_mode`](Self::from_tuples_with_config_and_build_mode),
  /// or [`BuildModeRelation`]), which build an equivalent trie (Amendment 2):
  /// the same contents and capacities. `presized:N` may place root keys in
  /// other buckets, and `radix:K` stores its rows in partition order, under
  /// other ids.
  ```
  The `tuples` field (lines 127–129). Before:
  ```rust
      /// Every stored tuple, in arrival order; its length is the multiset
      /// count (duplicates count). Chains, singletons and pending lists hold
      /// ids of its rows.
  ```
  After:
  ```rust
      /// Every stored tuple: the build's batch, or its partitioned copy (see
      /// Storage), then every inserted row. Its length is the multiset count
      /// (duplicates count). Chains, singletons and pending lists hold ids of
      /// its rows.
  ```
  `tuples()` (lines 177–179). Before:
  ```rust
      /// The trie's buffer: every stored tuple, in arrival order. Chains,
      /// singletons and pending lists hold ids of its rows; `HashTrieIter`
      /// lends a chain's rows from it (`LeafRows`).
  ```
  After:
  ```rust
      /// The trie's buffer: every stored tuple, in the order Storage
      /// describes. Chains, singletons and pending lists hold ids of its rows;
      /// `HashTrieIter` lends a chain's rows from it (`LeafRows`).
  ```
  `collect_tuples` (lines 182–185). Before:
  ```rust
      /// A copy of every stored tuple, in arrival order: the trie's buffer,
      /// cloned (#111). O(n · arity) time and space; to visit the tuples
      /// without copying them, use [`for_each_tuple`](Self::for_each_tuple)
      /// (a walk of the trie) or `TupleScan::scan_tuples` (the buffer).
  ```
  After:
  ```rust
      /// A copy of every stored tuple, in the buffer's order (arrival order,
      /// or partition order after a build that copies its rows, #101): the
      /// trie's buffer, cloned (#111). O(n · arity) time and space; to visit
      /// the tuples without copying them, use
      /// [`for_each_tuple`](Self::for_each_tuple) (a walk of the trie) or
      /// `TupleScan::scan_tuples` (the buffer).
  ```
  The `project` comment (lines 701–702). Before:
  ```rust
          // One buffer for the projection, in arrival order; one scratch row,
          // reused, so nothing is allocated per tuple.
  ```
  After:
  ```rust
          // One buffer for the projection, in the trie's row order; one
          // scratch row, reused, so nothing is allocated per tuple.
  ```
  The `TupleScan` impl doc (line 727). Before:
  ```rust
  /// The trie's buffer, in arrival order (#111): statistics need only the
  ```
  After:
  ```rust
  /// The trie's buffer, in its row order (#111): statistics need only the
  ```

  `kermit-ds/tests/hash_trie_tests.rs` lines 150–153. Before:
  ```rust
  // Every build mode builds the identical trie (issue #91), so the iterator
  // contract must hold unchanged, eager or lazy. Two bits make four
  // partitions, so the 3–5 tuple fixtures spread over several partitions
  // with several keys in each.
  ```
  After:
  ```rust
  // Every build mode builds an equivalent trie (issue #91; `radix:K` stores
  // its rows in partition order since #101), so the iterator contract must
  // hold unchanged, eager or lazy. Two bits make four partitions, so the 3–5
  // tuple fixtures spread over several partitions with several keys in each.
  ```

  `kermit/tests/join_tests.rs` lines 339–344. Before:
  ```rust
  // The plain HashTrie invocations above build `bulk` (Algorithm 2, the
  // default); these run the radix build, which must build the identical trie
  // (issue #91). Two bits make four partitions, so the 3–5 tuple fixtures
  // spread over several partitions with several keys in each. Sip/off/eager,
  // Fx/on/eager and Sip/on/lazy cover both hashers, both pruning policies and
  // both expansion policies, each under every optimiser.
  ```
  After:
  ```rust
  // The plain HashTrie invocations above build `bulk` (Algorithm 2, the
  // default); these run the radix build, which must build an equivalent trie
  // (issue #91): the same contents, its rows stored in partition order
  // (#101). Two bits make four partitions, so the 3–5 tuple fixtures spread
  // over several partitions with several keys in each. Sip/off/eager,
  // Fx/on/eager and Sip/on/lazy cover both hashers, both pruning policies and
  // both expansion policies, each under every optimiser.
  ```

  The `implementation.rs` doc edits above (Storage, Construction, `tuples`, `tuples()`,
  `collect_tuples`) sit in the file this task already edits, so they are made here rather than
  in T7. Their wording names `radix:K` only, which is true at this commit; T7 extends the two
  `radix:K` sentences (Storage, Construction) to name all three modes once T4 and T5 have landed.
  `build_mode.rs`'s `Radix` and `RadixBits` docs (the latter still says partition numbers fit a
  `u16`) stay stale until T7, which rewrites that file's docs for all three modes at once
  (RECONCILE 8). `identity.rs`'s module doc needs nothing here: T2 already wrote it for both
  levels of comparison.

- [ ] **Step 8: Lint, format, document, then the workspace.**

  ```bash
  nix develop --command env RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc --workspace --no-deps
  ```
  Expected: no warnings and no fmt diff after the formatting run. If `fmt` touched more than
  whitespace, re-run Step 6's `kermit-ds` commands. Then the workspace, through the long-run
  procedure:
  ```bash
  setsid nohup nix develop --command env CARGO_BUILD_JOBS=2 cargo test --workspace > $SCRATCH/b1-t3-test.log 2>&1 &
  echo $! > $SCRATCH/b1-t3-test.pid; disown
  while kill -0 "$(cat $SCRATCH/b1-t3-test.pid)" 2>/dev/null; do sleep 30; done
  grep -E '^test result|FAILED|panicked' $SCRATCH/b1-t3-test.log | sort | uniq -c
  ```
  Expected: every `test result:` line reads `ok` with `0 failed`.

- [ ] **Step 9: Commit.**

  ```bash
  git add kermit-ds/src/ds/hash_trie/radix.rs kermit-ds/src/ds/hash_trie/implementation.rs \
    kermit-ds/src/ds/hash_trie/parallel.rs kermit-ds/src/morsel.rs \
    kermit-ds/tests/hash_trie_tests.rs kermit/tests/join_tests.rs
  git commit -m "$(cat <<'EOF'
  perf(hash-trie): radix:K copies its rows into partition order (#101)

  radix:K partitions the rows, not their ids: morsel::partition_rows
  copies the batch into 2^K first-key-hash partitions on one thread, as
  one morsel (per-morsel bookkeeping would cost ~1.6 GB at radix:16 over
  10^7 rows), the copy becomes the trie's buffer, and the input is freed
  before the build.
  Each partition's scratch root reads one contiguous range of the copy,
  and first_seen records each key's input position through origin, so the
  merge still gives the root bulk's layout, bucket for bucket.
  radix::partition is gone.

  The partitioned fill takes the batch by value and returns the buffer.
  parallel:N and presized:N return the batch unchanged; parallel:N passes
  an identity origin until it materialises too.

  radix's tests compare contents (assert_same_contents) and heap size
  instead of arrays, and a new test pins the buffer to partition_rows'
  output, the one difference no content test can see.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  EOF
  )"
  ```
  (`morsel.rs` is staged for Step 5f's removed markers.)

- [ ] **Step 10: Mutation checks.** For each mutant, follow these steps in order:
  1. Apply it as the exact edit.
  2. Confirm `git diff` shows it.
  3. Run the named tests.
  4. Reverse the exact edit, never with `git checkout`.
  5. Re-run the tests to green.

  Run with `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_trie::radix::tests`.
  The mutants leave an unused variable, so expect warnings.

  - **M1, the copy skipped (the spec's mutant).** In `radix::fill_root`, change the line
    `        (H::hash(row[0]) >> shift) as usize` to `        0`.
    - This is the one-line way to keep the rows in arrival order with a consistent trie.
      Every row goes to partition 0, so the copy equals the input and `origin` is the identity.
    - **Fails:** `radix_keeps_the_partitioned_copy_as_its_buffer`, at `…: buffer`. The latest
      it can fail is the `random` input.
    - **Passes:** the three `radix_builds_the_bulk_trie_*` tests and
      `build_mode_relation_uses_the_default_config`. One partition builds the bulk contents.
      This is why the pin exists, and the expected outcome.
  - **M2, `origin` ignored.** In `build_scratch_root`, change
    `first_seen.push((origin[row as usize], hash));` to `first_seen.push((row, hash));`.
    - The merge then inserts root keys partition by partition, not in first-appearance order.
    - **Fails:** `radix_builds_the_bulk_trie_under_siphash` and
      `radix_builds_the_bulk_trie_under_fxhash`. They fail inside `assert_same_contents`, at a
      bucket of the depth-0 table (the root's path is the bare label). This is the evidence
      that the content check holds the root bucket for bucket, so no separate root assertion
      is needed.
    - **Passes:** `radix_builds_the_bulk_trie_under_colliding_hashes`. Every `Mod10` hash is
      below 10, so its top bits are 0, every row is in partition 0, and new ids follow input
      order. The pin test passes too, since the buffer is unchanged.

  Expected afterwards: `git status` is clean.

---

### Task 4: `parallel:N` copies its rows into partition order

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/parallel.rs`. Line numbers are at a85c263; T3 shifts
  them by a few lines.
  - module doc, the `parallel:N` part (1–22);
  - the non-test `use` (42–58);
  - a new `partition_sizes` helper after `partition_bits` (136–143);
  - `fill_root` (145–153) and `fill_root_in_morsels` (155–208);
  - `merge_in_first_appearance_order`'s doc (210–216);
  - test module `use` (383–402);
  - `check_identity` (414–469);
  - a new pin, `parallel_stores_its_partitioned_copy`, after
    `partition_bits_give_four_partitions_per_thread_rounded_up` (505–512);
  - `parallel_builds_the_bulk_trie_on_large_and_skewed_inputs` (the assert at 554);
  - `build_modes_reach_their_builds` (576–626);
  - `parallel_build_merges_under_every_root_capacity` (999–1034).
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs`:
  - the `HashTrieBuildMode::Parallel(threads)` arm (512–518);
  - `from_tuples_partitioned` (615–634): it becomes `pub(super)`, because the
    small-morsel tests build through it.
- Modify (comments only):
  - `kermit-ds/tests/hash_trie_tests.rs` 183–185;
  - `kermit/tests/join_tests.rs` 472–474.

  Both say `parallel:2` builds "the identical trie", which stops being true here.

**Preconditions (T1–T3):**
- `morsel::partition_rows` and `morsel::Partitioned` exist (T1).
  - `partition_rows` runs its histogram and its scatter as two `run_workers` calls, so a
    `parallel:N` build now records three worker runs, not two.
- `identity::assert_same_contents` exists, and `assert_equivalent_trie` compares
  contents below the root (T2).
- From T3:
  - `radix::build_scratch_root` takes `origin: &[RowId]` and records `(origin[id], hash)`;
  - `from_tuples_partitioned`'s fill is `FnOnce(Tuples, u32, usize) -> (Tuples, HashTrieNode<P, E>)`;
  - `fill_root_in_morsels` passes `build_scratch_root` an identity `origin` (`0..len`);
  - `build_modes_reach_their_builds`' `radix:3` row records `vec![1, 1]` worker runs:
    `partition_rows` on one thread, two runs.

**Tests that pin behaviour and stay unchanged** (why each holds):
- `ds::hash_trie::bulk::tests::tuples_sizes_every_child_from_its_list` runs `parallel:2`.
  - It checks each table's capacity against the tuples below it.
  - `parallel:N` builds every child from the same list, so the capacities do not change.
- `kermit-ds/tests/hash_trie_tests.rs`, the three `*Parallel2` suites: the iterator contract.
  - Sibling orders are checked as sets, and leaf rows as sorted rows.
  - `leaf_tuples` resolves ids through the trie's own buffer.
- `kermit-ds/tests/parquet_tests.rs`, `HashTrieSipParallel2`: it compares sorted
  `collect_tuples()`. The buffer is a permutation of the input, so the sorted rows are equal.
- `kermit/tests/join_tests.rs`, the `HashParallel2` cells, compare answers. HashTriejoin
  reads leaf rows from the trie's own buffer, so the answers do not depend on ids.
- `kermit/tests/cli_hash_trie_build_mode.rs` checks the `ds_build_mode` axes and the
  `--verify` answer counts. Neither depends on where rows sit.
- `kermit` (bin) `execution::tests::hash_trie_families_build_with_their_parallel_mode`
  reads only `threads` and `deferred` from the hook record, whose shape does not change.
- `kermit/tests/tuple_allocation.rs`, the `parallel:1` cell, bounds `many − once`.
  - `partition_rows` allocates the same number of times at 100 and 10 000 rows, because
    both fit one morsel. The per-partition id pushes it replaces were the only per-row
    growth.
  - So the difference falls and the bound holds. T6 re-derives the stale comment.

- [ ] **Step 1: Write the failing pin.**

  Check the T1 precondition first:
  ```bash
  rg -n 'run_workers\(' kermit-ds/src/morsel.rs
  ```
  Expected: four call lines: `scatter_rows`', `dispatch`'s and two in `partition_rows`.
  The definition does not match, because it reads `run_workers<R: Send>(`. If
  `partition_rows` makes one call or three, change the `vec![t, t, t]` (Step 5) and
  `vec![3, 3, 3, 3]` (Task 5 Step 4) expectations to match before going on.

  In `mod tests` of `kermit-ds/src/ds/hash_trie/parallel.rs`, after
  `partition_bits_give_four_partitions_per_thread_rounded_up` (505–512), add:

  ```rust
      /// `parallel:N` stores the partitioned copy as its buffer: exactly what
      /// `partition_rows` makes of the input at `parallel:N`'s partition count,
      /// which `partition_bits` derives from N, at the input's capacity. A
      /// build that skipped the copy would pass every content test, so this is
      /// the test that sees it: the random input spreads over every partition,
      /// so there the copy must differ from the input.
      fn check_copy<H: HashStrategy>() {
          for arity in 1..=3 {
              for (input, tuples) in inputs(arity) {
                  for &t in THREADS {
                      let label = format!("{} arity {arity}, parallel:{t}, {input}", H::NAME);
                      let bits = partition_bits(threads(t));
                      let copy =
                          partition_rows(threads(t), &tuples, MORSEL_TUPLES, 1 << bits, |row| {
                              (H::hash(row[0]) >> (64 - bits)) as usize
                          });
                      let built: HashTrie<H> = HashTrie::from_tuples_with_build_mode(
                          arity.into(),
                          HashTrieBuildMode::Parallel(threads(t)),
                          tuples.clone(),
                      );
                      assert_eq!(built.tuples(), &copy.tuples, "{label}: buffer");
                      assert_eq!(
                          built.tuples().value_capacity(),
                          tuples.value_capacity(),
                          "{label}: buffer capacity"
                      );
                      if input == "random" {
                          assert_ne!(
                              built.tuples(),
                              &tuples,
                              "{label}: the copy is in partition order"
                          );
                      }
                  }
              }
          }
      }

      #[test]
      fn parallel_stores_its_partitioned_copy() {
          check_copy::<SipHashStrategy>();
          check_copy::<FxHashStrategy>();
      }
  ```
  - `partition_rows` and `MORSEL_TUPLES` reach the test module through `super::*`.
  - For this step, add `partition_rows` to the parent's `crate::morsel::{…}` import
    (42–58). Step 3 rewrites that `use` block with it included.
  - `Mod10HashStrategy` is left out because it sends every row to partition 0, where the
    copy equals the input.

- [ ] **Step 2: Run the pin. It fails.**

  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l   # prints 0
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds parallel_stores_its_partitioned_copy
  ```
  Expected: FAILED.
  - The panic is `…: buffer` on the first input whose rows span two or more partitions
    (`interleaved` or `random`).
  - The post-T3 build keeps the input batch as its buffer.

- [ ] **Step 3: `parallel:N` partitions the rows themselves.**

  All edits are in `kermit-ds/src/ds/hash_trie/parallel.rs`.

  **The non-test `use` (42–58)** becomes the following. It replaces whatever T3 added for
  the identity `origin` (for example a `bulk::all_rows` or `morsel::row_id` import),
  which is unused from here on:
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
      crate::morsel::{
          dispatch, partition_rows, scatter_rows, RowPartition, Threads, MORSEL_TUPLES,
          PARTITIONS_PER_THREAD,
      },
      kermit_iters::{HashStrategy, RowId, Tuples},
      std::{cmp::Reverse, collections::BinaryHeap},
  };
  ```
  `scatter_rows` and `RowPartition` stay until T5: `presized:N` still uses them.

  **After `partition_bits` (136–143)**, add the hook's helper:
  ```rust
  /// The size of each partition of `partitioned`, empty ones included: the
  /// test hooks' [`ParallelBuild::partition_sizes`].
  #[cfg(any(test, feature = "test-hooks"))]
  fn partition_sizes(partitioned: &crate::morsel::Partitioned) -> Vec<usize> {
      partitioned
          .offsets
          .windows(2)
          .map(|pair| pair[1] - pair[0])
          .collect()
  }
  ```
  The full path, not an import, is deliberate. The helper is the only non-test user of
  the type, so an import would be unused in a build without `test-hooks`, and
  `-Dwarnings` would reject it.

  **Replace `fill_root` and `fill_root_in_morsels`** (145–208, as T3 left them) with:
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
          // The bulk build of nothing is the empty root over the empty batch:
          // copy nothing and start no worker.
          return tuples;
      }
      // 1. Partition, on `threads` workers, by the top bits of each row's first
      //    attribute's hash. The rows are copied into partition order, each
      //    partition's in input order, with each copy's input position in
      //    `origin`. The copy is the trie's buffer from here on, so the input
      //    is freed before the build steps.
      let bits = partition_bits(threads);
      let shift = 64 - bits;
      let partitioned = partition_rows(threads, &tuples, morsel_tuples, 1 << bits, |row| {
          (H::hash(row[0]) >> shift) as usize
      });
      drop(tuples);
      #[cfg(any(test, feature = "test-hooks"))]
      PARALLEL_BUILDS.with(|builds| {
          builds.borrow_mut().push(ParallelBuild {
              threads: threads.get(),
              partition_sizes: partition_sizes(&partitioned),
              deferred: None,
          });
      });
      // An empty partition would build and drop an empty scratch root; the
      // radix build skips them too.
      let ranges: Vec<_> = (0..partitioned.partitions())
          .map(|p| partitioned.ids(p))
          .filter(|ids| !ids.is_empty())
          .collect();
      // 2. Build, on `threads` workers: each partition's id range into a
      //    scratch root, whose entries then leave in the order their keys
      //    arrived, each tagged with its key's first input position (`origin`),
      //    not its id.
      let (rows, origin) = (&partitioned.tuples, &partitioned.origin);
      let entries = dispatch(threads, ranges, |ids| {
          let (scratch, first_seen) =
              radix::build_scratch_root::<H, P, E>(rows, ids, origin, arity, config);
          Entries::take_from(scratch, &first_seen)
      });
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
  ```
  - `Partitioned` has no `Drop` impl, so `partitioned.tuples` moves out once the borrows in
    `rows` and `origin` end.
  - `Range<RowId>::is_empty` is std's.

  **Replace `merge_in_first_appearance_order`'s doc** (210–216) with:
  ```rust
  /// Inserts every partition's entries into the real root in the order their
  /// keys first appeared in the input, the order the bulk build inserts
  /// them, so the root's buckets, length and capacity are the bulk
  /// build's. Each entry's tag is an input position (`origin`, through
  /// `radix::build_scratch_root`), not an id into the copy: ids run
  /// partition-major, so merging on them would insert each partition's keys
  /// in turn. Each list is already in that order, so a k-way merge suffices:
  /// a heap pop and push per entry over at most P lists, where the radix
  /// build sorts every entry. It runs on the calling thread: the build's
  /// sequential step.
  ```

  **Replace the module doc's `parallel:N` part** (1–22, up to the blank `//!` before
  `` //! `presized:N` ``) with:
  ```rust
  //! The `parallel:N` and `presized:N` builds of a [`HashTrie`](super::HashTrie):
  //! `parallel:N` is the radix build's three steps (`radix.rs`), with the
  //! partition and build steps run on N threads by the morsel-driven helpers
  //! of `crate::morsel` (Leis et al., SIGMOD 2014; SIGMOD 2020 §3.3.2; issue
  //! #94).
  //!
  //! 1. **Partition.** [`partition_rows`] copies every row into one of P
  //!    partitions by the top log₂P bits of the row's first attribute's hash,
  //!    and records each copied row's input position (`origin`); P is four per
  //!    thread, rounded up to a power of two. The copy is the paper's
  //!    materialisation step (§3.3.2, #101): it becomes the trie's buffer, the
  //!    input batch is freed, and each partition's rows are a contiguous range
  //!    of it, so every later read of a partition's rows is local.
  //! 2. **Build.** [`dispatch`] groups each non-empty partition's id range into
  //!    a scratch root and builds its children, as `bulk` does
  //!    (`radix::build_scratch_root`), then takes the scratch root's entries
  //!    out, each tagged with the input position at which its key first
  //!    appeared.
  //! 3. **Merge.** The calling thread inserts every entry into the real root in
  //!    first-appearance order, by a k-way merge of the partitions' lists.
  //!
  //! The result holds the bulk trie's contents, and its root is the bulk
  //! build's, bucket for bucket and capacity for capacity, for the reasons
  //! the radix build's is.
  //! - **Children.** A partition keeps its rows in input order (`partition_rows`
  //!   is stable), so each child is built from the tuples `bulk` builds it
  //!   from, in the same order, under partition-local ids.
  //! - **Root.** The root receives its new keys in the bulk order, because the
  //!   merge orders them by `origin`, not by id.
  //! - **Collisions.** Equal 64-bit hashes share a root entry and also a
  //!   partition, because the partition is a function of the hash.
  //!
  //! Only the ids differ, so the trie is equivalent to `bulk`'s rather than
  //! array-identical (Amendment 2).
  ```

- [ ] **Step 4: `from_tuples_partitioned` becomes `pub(super)`, and the `parallel:N` arm keeps the copy.**

  In `kermit-ds/src/ds/hash_trie/implementation.rs`:
  - At 620, `fn from_tuples_partitioned(` becomes `pub(super) fn from_tuples_partitioned(`.
  - Append this line to its doc comment (615–619, as T3 left it):
    ```rust
        /// Tests in `parallel.rs` call it with small morsels and regions.
    ```
  - Replace the `HashTrieBuildMode::Parallel(threads)` arm (512–518; T3 made its closure
    pass `&tuples` and return `(tuples, root)`) with:
  ```rust
              | HashTrieBuildMode::Parallel(threads) => {
                  Self::from_tuples_partitioned(header, config, tuples, |tuples, log2, arity| {
                      let mut root = Self::make_root_sized(arity, log2);
                      let tuples =
                          parallel::fill_root::<H, P, E>(tuples, &mut root, arity, threads, config);
                      (tuples, root)
                  })
              },
  ```

- [ ] **Step 5: The parallel tests compare contents.**

  The old tests no longer compile against the new `fill_root_in_morsels`, so this step
  comes before any run.

  All edits are in `mod tests` of `kermit-ds/src/ds/hash_trie/parallel.rs`.

  **The test-module `use` (383–402)** becomes:
  ```rust
      use {
          super::*,
          crate::{
              ds::hash_trie::{
                  build_mode::{HashTrieBuildMode, RadixBits},
                  bulk::all_rows,
                  config::{ChildCapacity, HashTrieConfig, RootCapacity},
                  expansion::{EagerExpansion, LazyExpansion},
                  identity::{
                      assert_equivalent_root, assert_equivalent_trie, assert_same_contents,
                      assert_same_node, assert_same_trie, configs, inputs, LOAD_PERCENTS,
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
  `assert_same_node` stays until T5, since `check_presized` still uses it.

  **Replace `check_identity`** (414–469) with this helper and the new `check_identity`:
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
                  let tuples = fill_root_in_morsels::<H, P, E>(
                      tuples,
                      &mut root,
                      arity,
                      threads(t),
                      7,
                      config,
                  );
                  (tuples, root)
              },
          )
      }

      /// `parallel:N` builds the trie `bulk` builds, content for content, for
      /// every arity, config (load factor, root capacity and child capacity),
      /// thread count and input. Every table, the root included, holds the
      /// same hashes in the same buckets at the same capacity. Every chain,
      /// singleton and pending list holds the same tuples in the same order,
      /// each id read through its own trie's buffer (`assert_same_contents`).
      /// The root being `bulk`'s bucket for bucket is the merge's
      /// first-appearance order at work (`origin`). Its `root-capacity=tuples`
      /// configs are the evidence that `parallel:N` under
      /// `root-capacity=tuples` is the exact merge build. Each case runs twice:
      /// through the public constructor (morsels of 16 384 tuples), and with
      /// morsels of 7 tuples, which make the partition step cut the input many
      /// times. Miri runs the second only.
      fn check_identity<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
          for arity in 1..=3 {
              for config in configs() {
                  for (input, tuples) in inputs(arity) {
                      let bulk = HashTrie::<H, P, E>::from_tuples_with_config(
                          arity.into(),
                          config,
                          tuples.clone(),
                      );
                      for &t in THREADS {
                          let label = format!(
                              "{}/{}/{} arity {arity}, root {}, load {}%, children {}, \
                               parallel:{t}, {input}",
                              H::NAME,
                              P::NAME,
                              E::NAME,
                              config.root_capacity.axis_value(),
                              config.load_factor.numerator(),
                              config.child_capacity.axis_value(),
                          );
                          if !cfg!(miri) {
                              let built = HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
                                  arity.into(),
                                  config,
                                  HashTrieBuildMode::Parallel(threads(t)),
                                  tuples.clone(),
                              );
                              assert_same_contents(&bulk, &built, &label);
                          }
                          let small = parallel_in_small_morsels::<H, P, E>(arity, config, &tuples, t);
                          assert_same_contents(&bulk, &small, &format!("{label}, morsels of 7"));
                      }
                  }
              }
          }
      }
  ```

  **In `parallel_builds_the_bulk_trie_on_large_and_skewed_inputs`** (554):
  - `assert_same_trie(&bulk, &built, &label);` becomes
    `assert_same_contents(&bulk, &built, &label);`.

  **Replace `build_modes_reach_their_builds`** (576–626, as T3 left it) with:
  ```rust
      /// Every mode builds the bulk trie's contents, so only the records show
      /// which build ran (`bulk` and `incremental` record nothing here;
      /// `each_build_mode_runs_its_own_build` tells them apart). They also
      /// show that `parallel:N` spreads its work:
      /// - N workers in every step: three `run_workers` calls,
      ///   `partition_rows`' histogram and scatter, then `dispatch`;
      /// - the tuples spread over the partitions.
      ///
      /// The radix build partitions too, on one thread (two runs of one
      /// worker), and records nothing. Eight first keys of eight tuples each
      /// fill the eight partitions of `parallel:2` evenly. `parallel:3` aims at
      /// 12 partitions, rounded up to 16, so key k lands in partition 2k: every
      /// other partition is empty, and the record lists those too. A second
      /// thread count is what shows that N itself reaches the build.
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
                  vec![1, 1],
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

  **In `parallel_build_merges_under_every_root_capacity`** (999–1034):
  - Its doc (999–1001) becomes:
    ```rust
        /// `parallel:N` is the exact merge build under every root capacity: a
        /// presized root changes the root's size, not the process. The result
        /// holds bulk's contents under the same config, and its root is bulk's
        /// bucket for bucket. Three worker runs: `partition_rows`' histogram
        /// and scatter, then `dispatch`.
    ```
  - `vec![3, 3],` (1029) becomes `vec![3, 3, 3],`.
  - `assert_same_trie(&bulk, &built, &format!("{root_capacity:?}"));` (1032) becomes
    `assert_same_contents(&bulk, &built, &format!("{root_capacity:?}"));`.

  **Unchanged** (each still holds):
  - `parallel_build_of_nothing_starts_no_worker` keeps `assert_same_trie`. The empty
    build returns its input batch, so the two tries are array-identical, and it records
    no worker run.
  - `parallel_build_rejects_a_wrong_arity`.
  - `partition_bits_give_four_partitions_per_thread_rounded_up`.

- [ ] **Step 6: Fix the two comments that call `parallel:2` identical.**

  `kermit-ds/tests/hash_trie_tests.rs` 183–185 becomes:
  ```rust
  // `parallel:2` builds eight partitions on two threads, copying the rows into
  // partition order (#101), and must build the bulk trie's contents over that
  // copy (issue #94), so the iterator contract holds unchanged, eager or lazy,
  // pruned or not.
  ```
  In `kermit/tests/join_tests.rs`, the second and third lines of the block at 472–474,
  `// These build on two threads, in eight partitions. Every mode must build` and
  `// the identical trie (issue #94). The same three Layouts as the radix rows`,
  become:
  ```rust
  // These build on two threads, in eight partitions, copying the rows into
  // partition order (#101); the trie holds the bulk trie's contents (issue
  // #94). The same three Layouts as the radix rows
  ```

- [ ] **Step 7: Run the pin, the parallel tests, then everything that pins the mode.**

  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds parallel_stores_its_partitioned_copy
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_trie::parallel
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit hash_trie_families_build_with_their_parallel_mode
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests hashparallel2
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test tuple_allocation
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_hash_trie_build_mode
  ```
  Expected: every `test result: ok`, 0 failed. The first run includes:
  - `parallel_builds_the_bulk_trie_under_{siphash,fxhash,colliding_hashes}`;
  - `parallel_builds_the_bulk_trie_on_large_and_skewed_inputs`;
  - `parallel_stores_its_partitioned_copy`;
  - `build_modes_reach_their_builds`;
  - `parallel_build_merges_under_every_root_capacity`;
  - `parallel_build_of_nothing_starts_no_worker`;
  - the unchanged presized tests.

  Then the workspace, through the long-run procedure:
  ```bash
  setsid nohup nix develop --command env CARGO_BUILD_JOBS=2 cargo test --workspace > $SCRATCH/t4-test.log 2>&1 &
  echo $! > $SCRATCH/t4-test.pid; disown
  while kill -0 "$(cat $SCRATCH/t4-test.pid)" 2>/dev/null; do sleep 30; done
  grep -E '^test result|FAILED|panicked' $SCRATCH/t4-test.log | sort | uniq -c
  ```
  Expected: every `test result:` line reads `ok` with `0 failed`.

- [ ] **Step 8: Lint, format, document.**

  ```bash
  nix develop --command env RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc --workspace --no-deps
  ```
  Expected: all clean.
  - `[`partition_rows`]` and `[`dispatch`]` in the module doc resolve through the
    `crate::morsel` import.
  - `[`ParallelBuild::partition_sizes`]` resolves to the struct's field.

- [ ] **Step 9: Commit.**

  ```bash
  git add kermit-ds/src/ds/hash_trie/parallel.rs kermit-ds/src/ds/hash_trie/implementation.rs \
    kermit-ds/tests/hash_trie_tests.rs kermit/tests/join_tests.rs
  git commit -m "$(cat <<'EOF'
  feat(hash-trie): parallel:N copies its rows into partition order (#101)

  parallel:N partitions the rows themselves with morsel::partition_rows
  on N threads, where scatter_rows partitioned only their ids. The
  partition-ordered copy becomes the trie's buffer and the input batch is
  freed: the paper's materialisation step (VLDB 2020 §3.3.2).

  dispatch builds each non-empty partition's id range into a scratch
  root, and the k-way merge keys on origin, each key's first input
  position, so the root stays bulk's bucket for bucket. Chains,
  singletons and pending lists hold the bulk trie's tuples in the same
  order under partition-local ids, so the build is equivalent rather
  than array-identical (Amendment 2).

  The tests compare contents (assert_same_contents) and pin the buffer
  to partition_rows' output at partition_bits(N) partitions. Small-morsel
  builds go through from_tuples_partitioned, now pub(super).

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  EOF
  )"
  ```

- [ ] **Step 10: Mutation check: the merge keyed on new ids.**

  Follow the ground rules: commit first, apply the exact edit, confirm it applied, see the
  test fail, reverse the exact edit, never with `git checkout`.
  - In `fill_root_in_morsels`, replace the line
    `    let (rows, origin) = (&partitioned.tuples, &partitioned.origin);`
    with these two lines:
    ```rust
        let identity: Vec<RowId> = (0..crate::morsel::row_id(partitioned.tuples.len())).collect();
        let (rows, origin) = (&partitioned.tuples, &identity);
    ```
    Every `first_seen` tag is then the row's new id. Ids run partition-major, so the merge
    inserts each partition's keys in turn.
  - Check that it applied: `git diff --stat` shows one file changed, and
    `rg -c 'let identity' kermit-ds/src/ds/hash_trie/parallel.rs` prints `1`.
  - Run:
    ```bash
    nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds parallel_builds_the_bulk_trie_under_siphash
    ```
    Expected: FAILED.
    - `assert_same_contents` panics at the root, where a bucket's hash differs from
      bulk's, on the `random` input at `parallel:1` already.
    - Keys that collide at the root mostly sit in different partitions, so the
      partition-major order places them differently.
  - Reverse the exact edit, re-run the test (`ok`), and check that `git status --short`
    prints nothing.

---

### Task 5: `presized:N` copies its rows into partition order

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/parallel.rs`. Lines are at a85c263 plus T3 and T4's
  shifts; each edit names its item.
  - module doc, the `presized:N` paragraph (24–40);
  - the non-test `use`: drop `scatter_rows` and `RowPartition`, add `std::ops::Range`;
  - a new `presized_parts` after `REGION_BUCKETS` (240–246);
  - `fill_presized_root` (248–267) and `fill_presized_root_in` (269–339);
  - `fill_runs` (356–379);
  - test module `use`, which drops `assert_same_node`;
  - new: `presized_in_small_regions`, `TailOrderHash`,
    `presized_tail_goes_in_input_order_for_every_n` and
    `presized_stores_its_partitioned_copy`;
  - `check_presized` (739–811);
  - `keys_that_cannot_fit_their_region_go_to_the_tail` (947–975);
  - `presized_build_reaches_its_own_path` (977–997);
  - the doc of `presized_parallel_builds_are_the_same_for_every_n_on_dense_roots`
    (1049–1055).
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs`, the
  `HashTrieBuildMode::Presized(threads)` arm (519–535).

**Unchanged, and why each still holds:**
- `push_in_run` (341–354) reads whichever buffer it is given, now the copy.
- `check_one_run` / `one_run_of_grouping_builds_the_bulk_root` (655–712) calls
  `push_in_run` and `build_nested` on the input batch itself. Its ids are input
  positions, as are bulk's, so `assert_equivalent_root` still compares like with like.
- `presized_build_requires_a_presized_root`.
- `presized_parallel_builds_are_equivalent_*`, whose bodies call `check_presized`.
- `kermit/tests/join_tests.rs`, the `HashPresized2` cells, compare answers.
- `kermit/tests/cli_hash_trie_build_mode.rs` checks the presized `--verify` runs and the
  prerequisite errors, and neither depends on where rows sit.
- `kermit` (bin)
  `execution::tests::hash_trie_families_build_with_their_parallel_mode` sees `threads ==
  [3]` and `deferred.is_some()` for `presized:3`.
- `kermit-ds/src/configured.rs` `built_with_stacks_on_configured` checks the record and
  the sorted walk.
- `kermit-ds/tests/{hash_trie_tests,parquet_tests}.rs`, the `*Presized2` suites, check the
  iterator contract and a sorted round trip.
- `kermit/tests/tuple_allocation.rs`, `hash_trie_presized_build_allocates_per_list_not_per_row`:
  - it asserts `deferred == Some(0)` and partition counts `(1, 4)`, which `offsets`
    still gives;
  - its `many − once` bound only gains slack, because `partition_rows` replaces the
    per-partition id buckets with constant-count allocations;
  - T6 re-derives its comment.

- [ ] **Step 1: Write the pins, against the post-T4 code.**

  **Extract `presized_parts`.** It is a pure refactor, so the copy pin can compute the
  partition count the build uses. After `REGION_BUCKETS` (240–246), add:
  ```rust
  /// The number of runs, one partition each, that `presized:threads` cuts a
  /// root of `2^log2_capacity` buckets into: [`PARTITIONS_PER_THREAD`] per
  /// thread rounded up to a power of two, and at most one per region of
  /// `region_buckets` (a region is the whole root when that is smaller).
  fn presized_parts(log2_capacity: u32, threads: Threads, region_buckets: usize) -> usize {
      let capacity = 1usize << log2_capacity;
      (PARTITIONS_PER_THREAD * threads.get())
          .next_power_of_two()
          .min(capacity / region_buckets.min(capacity))
  }
  ```
  In `fill_presized_root_in`, replace these lines (291–296):
  ```rust
      let capacity = 1usize << log2_capacity;
      let region_buckets = region_buckets.min(capacity);
      let parts = (PARTITIONS_PER_THREAD * threads.get())
          .next_power_of_two()
          .min(capacity / region_buckets);
      let shift = log2_capacity - parts.trailing_zeros();
  ```
  with:
  ```rust
      let parts = presized_parts(log2_capacity, threads, region_buckets);
      let shift = log2_capacity - parts.trailing_zeros();
  ```
  `fill_runs` now receives the unclamped `region_buckets`. That is the same, because
  `HashTable::with_runs` clamps it to the capacity itself (`hash_table.rs:556`).

  **In `mod tests`**, after `EndOfRegionHash`'s impls (933–945), add a helper, a hash and
  the two pins. The helper's body is the post-T4 adapter, which Step 3 replaces:
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
                  let root = fill_presized_root_in::<H, P, E>(
                      &tuples,
                      log2,
                      arity,
                      threads(t),
                      7,
                      region_buckets,
                      config,
                  );
                  (tuples, root)
              },
          )
      }

      /// Homes in a 32-bucket root: first keys 0 to 4 at buckets 7, 5, 6, 7
      /// and 5, and every other value at bucket 0. Distinct values keep
      /// distinct hashes.
      #[derive(Copy, Clone, Default, Debug)]
      struct TailOrderHash;

      impl LayoutOption for TailOrderHash {
          const NAME: &'static str = "tail-order";
      }

      impl HashStrategy for TailOrderHash {
          fn hash(key: usize) -> u64 {
              let home = [7, 5, 6, 7, 5].get(key).copied().unwrap_or(0);
              super::super::hash_table::hash_with_home(home, 5, key as u64)
          }
      }

      /// The deferred tail is pushed in input order, by `origin`, at every N.
      ///
      /// The fixture:
      /// - Sixteen rows `[i % 5, i]`, keys A to Y = 0 to 4, give a 32-bucket
      ///   root of 2-bucket regions.
      /// - A (key 0) and X (3) home at bucket 7, B (1) and Y (4) at 5, and
      ///   C (2) at 6.
      /// - So A, B and C take buckets 7, 5 and 6, and X's and Y's rows (three
      ///   each) overflow their regions.
      ///
      /// In input order, X comes first: it probes from 7 to bucket 8, then Y
      /// from 5 to bucket 9, as in `bulk`. The partitions differ by N:
      /// - `presized:1` cuts 4 partitions, and all five keys share the first,
      ///   so its copy is the input.
      /// - `presized:4` cuts 16, one region each. Y's partition (2) precedes
      ///   X's (3), so the copy gives Y's rows the smaller ids.
      ///
      /// A tail sorted by new id would put Y in bucket 8 under `presized:4`
      /// only.
      #[test]
      #[cfg_attr(miri, ignore = "compares thread counts; Miri runs one")]
      fn presized_tail_goes_in_input_order_for_every_n() {
          let config = tuples_config(70, ChildCapacity::Grow);
          let tuples = Tuples::from((0..16).map(|i| vec![i % 5, i]).collect::<Vec<Vec<usize>>>());
          assert_eq!(
              config.root_log2_capacity(tuples.len()),
              5,
              "a 32-bucket root"
          );
          let bulk: HashTrie<TailOrderHash> =
              HashTrie::from_tuples_with_config(2.into(), config, tuples.clone());
          let mut first: Option<HashTrie<TailOrderHash>> = None;
          for (t, partitions) in [(1, 4), (4, 16)] {
              let label = format!("presized:{t}");
              PARALLEL_BUILDS.with(|b| b.borrow_mut().clear());
              let built = presized_in_small_regions::<TailOrderHash, NoPruning, EagerExpansion>(
                  2, config, &tuples, t, 2,
              );
              let builds = PARALLEL_BUILDS.with(|b| b.take());
              assert_eq!(builds.len(), 1, "{label}");
              assert_eq!(builds[0].partition_sizes.len(), partitions, "{label}");
              assert_eq!(builds[0].deferred, Some(6), "{label}: X's and Y's rows");
              assert_eq!(
                  built.root().hash_at(8),
                  Some(TailOrderHash::hash(3)),
                  "{label}: X, first in input order, takes bucket 8"
              );
              assert_eq!(
                  built.root().hash_at(9),
                  Some(TailOrderHash::hash(4)),
                  "{label}: Y takes bucket 9"
              );
              assert_equivalent_trie(&bulk, &built, &label);
              match &first {
                  | None => first = Some(built),
                  | Some(one) => {
                      assert_same_contents(one, &built, &format!("{label} vs presized:1"))
                  },
              }
          }
      }

      /// `presized:N` stores the partitioned copy as its buffer: exactly what
      /// `partition_rows` makes of the input for its runs of regions, whose
      /// count `presized_parts` derives from N, the root's capacity and the
      /// region size, at the input's capacity. It is pinned on the public path
      /// (4 096-bucket regions) and with 8-bucket regions. With the latter the
      /// random input spreads over several runs, so there the copy must differ
      /// from the input, and a build that skipped the copy fails here.
      fn check_presized_copy<H: HashStrategy>() {
          let config = tuples_config(70, ChildCapacity::Grow);
          for arity in 1..=3 {
              for (input, tuples) in inputs(arity) {
                  let log2 = config.root_log2_capacity(tuples.len());
                  for &t in PRESIZED_THREADS {
                      let label = format!("{} arity {arity}, presized:{t}, {input}", H::NAME);
                      let copy = |morsel_tuples: usize, region_buckets: usize| {
                          let parts = presized_parts(log2, threads(t), region_buckets);
                          let shift = log2 - parts.trailing_zeros();
                          partition_rows(threads(t), &tuples, morsel_tuples, parts, |row| {
                              home_bucket(H::hash(row[0]), log2) >> shift
                          })
                          .tuples
                      };
                      let built: HashTrie<H> = HashTrie::from_tuples_with_config_and_build_mode(
                          arity.into(),
                          config,
                          HashTrieBuildMode::Presized(threads(t)),
                          tuples.clone(),
                      );
                      assert_eq!(
                          built.tuples(),
                          &copy(MORSEL_TUPLES, REGION_BUCKETS),
                          "{label}: buffer"
                      );
                      let small = presized_in_small_regions::<H, NoPruning, EagerExpansion>(
                          arity, config, &tuples, t, 8,
                      );
                      assert_eq!(small.tuples(), &copy(7, 8), "{label}, small regions: buffer");
                      for trie in [&built, &small] {
                          assert_eq!(
                              trie.tuples().value_capacity(),
                              tuples.value_capacity(),
                              "{label}: buffer capacity"
                          );
                      }
                      if input == "random" {
                          assert_ne!(
                              small.tuples(),
                              &tuples,
                              "{label}, small regions: the copy is in run order"
                          );
                      }
                  }
              }
          }
      }

      #[test]
      fn presized_stores_its_partitioned_copy() {
          check_presized_copy::<SipHashStrategy>();
          check_presized_copy::<FxHashStrategy>();
      }
  ```
  Notes:
  - `partition_rows`, `home_bucket`, `REGION_BUCKETS`, `MORSEL_TUPLES`, `Tuples` and
    `HashStrategy` reach the tests through `super::*`.
  - `HashTrie::root()` is `pub(crate)`; `HashTrieNode::hash_at` is `pub(crate)`
    (`node.rs`).
  - `TailOrderHash` hashes the second column too. Its values 5 to 15 get distinct
    home-0 hashes, and that is all a child table needs.

- [ ] **Step 2: Run the pins on the post-T4 code.**

  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l   # prints 0
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds presized_tail_goes_in_input_order_for_every_n
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds presized_stores_its_partitioned_copy
  ```
  Expected:
  - **The tail test passes.** Ids are still input positions, so the tail runs in input
    order. This checks the fixture's hand-computed buckets 7, 5, 6, 8 and 9 and the six
    deferred rows before the refactor.
  - **The copy pin FAILS** with `…: buffer` at the first input whose rows span two or
    more runs: `interleaved` with small regions, or `random` at the latest.
    - The random input's 8 192-bucket root has two 4 096-bucket regions even on the public
      path.
    - The build still keeps the input batch.

- [ ] **Step 3: `presized:N` partitions the rows themselves.**

  All edits are in `kermit-ds/src/ds/hash_trie/parallel.rs`.

  **The non-test `use`** becomes:
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
  `scatter_rows` and `RowPartition` stay in `morsel.rs`, where TreeTrie's parallel build
  uses both (`tree_trie/implementation.rs:225–236`).

  **Replace `fill_presized_root` and `fill_presized_root_in`** with:
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

  /// [`fill_presized_root`] with the morsel and region sizes as parameters,
  /// so tests can cut a small input into many morsels and regions.
  ///
  /// 1. **Partition**: [`partition_rows`] copies the rows by the top bits of
  ///    each row's home bucket, so partition k is a contiguous run of regions.
  ///    Its rows are a contiguous range of the copy, in input order, with each
  ///    row's input position in `origin`. The copy becomes the buffer; the
  ///    input is freed.
  /// 2. **Group** (Algorithm 2, lines 4–7): each worker takes a partition's
  ///    id range with its run of a presized table of row-id lists, and pushes
  ///    every row's id onto the list in its bucket. Rows whose key's probe
  ///    runs off its region are deferred.
  /// 3. **Tail**: the calling thread pushes the deferred rows in input order
  ///    (ascending `origin`), with ordinary probing. The paper does not say
  ///    how a probe crossing a partition's end is handled; this is kermit's
  ///    answer.
  /// 4. **Children** (lines 8–12): each worker builds the children of its
  ///    run's buckets, by the bulk build's `child`, reading only its
  ///    partition's rows. The paper does not say how the recursion is spread
  ///    over threads; one run per worker is kermit's choice.
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
      // 3. The tail, in input order: by each row's input position, `origin`.
      //    Its new id would order the tail partition-major, and the partition
      //    count depends on N, so tail keys would take buckets that vary with
      //    N. The table is presized for every row, so it does not grow here.
      let origin = &partitioned.origin;
      deferred.sort_unstable_by_key(|&row| origin[row as usize]);
      for row in deferred {
          lists
              .entry_or_insert_with(H::hash(rows.row(row)[0]), config.load_factor, Vec::new)
              .push(row);
      }
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
  Notes:
  - `origin` values are distinct, so the unstable sort is deterministic.
  - A key overflows its region whole: buckets only fill, so once one of its rows is
    deferred every later row is too. Each tail key's list therefore holds its rows in
    input order, as `bulk`'s does.

  **Replace `fill_runs`** with:
  ```rust
  /// Lends `table`'s runs to the workers: run k is filled through `step` with
  /// the ids of `ranges[k]`, partition k of the copy, in order, and the ids
  /// `step` hands back are returned. Within a partition, ids follow input
  /// order; across partitions they do not, so the caller restores input
  /// order through `origin`.
  fn fill_runs<V: Send>(
      table: &mut HashTable<V>, threads: Threads, ranges: Vec<Range<RowId>>, region_buckets: usize,
      step: impl Fn(&mut BucketRun<'_, V>, RowId) -> Result<(), RowId> + Sync,
  ) -> Vec<RowId> {
      let parts = ranges.len();
      table.with_runs(parts, region_buckets, |runs| {
          let work: Vec<_> = ranges.into_iter().zip(runs).collect();
          dispatch(threads, work, |(ids, mut run)| {
              let mut deferred = Vec::new();
              for row in ids {
                  if let Err(row) = step(&mut run, row) {
                      deferred.push(row);
                  }
              }
              deferred
          })
          .into_iter()
          .flatten()
          .collect()
      })
  }
  ```

  **Replace the module doc's `presized:N` paragraph** (24–40) with:
  ```rust
  //! `presized:N`
  //! (`docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`;
  //! spelled `parallel:N` under `root-capacity=tuples` until 2026-10-07;
  //! [`fill_presized_root`]) follows the paper where the paper describes it:
  //! - the input is partitioned by its first attribute's hash as it is copied
  //!   (§3.3.2);
  //! - Algorithm 2 groups it into a root sized before any tuple arrives (line
  //!   3; `root-capacity=tuples`, #88) and builds each bucket's child.
  //!
  //! How this runs on threads is kermit's:
  //! 1. Each partition is a contiguous run of the root's fixed-size regions,
  //!    its rows a contiguous range of the copy. One worker groups it into its
  //!    run of a presized table of lists.
  //! 2. A row whose probe would cross its region's end is deferred to the
  //!    calling thread, which pushes the deferred rows in input order (by
  //!    `origin`).
  //! 3. Each worker builds the children of one run's buckets at a time.
  //!
  //! Root keys may then sit in other buckets than the bulk build's, so this
  //! trie is equivalent rather than identical (Amendment 2): the root has the
  //! same capacity, occupied buckets and total displacement, and every subtrie
  //! holds the same tuples in the same order. Regions, not threads, decide
  //! which tuples are deferred, and the tail goes in input order, so the
  //! trie's contents are the same for every N, bucket for bucket. Only its
  //! buffer, each N's partitioned copy, differs.
  ```

  **`implementation.rs`, the `HashTrieBuildMode::Presized(threads)` arm** (519–535). Keep
  the prerequisite `assert_eq!`. Its `from_tuples_partitioned` call, which T3 made pass
  `&tuples` and return `(tuples, root)`, becomes:
  ```rust
                  Self::from_tuples_partitioned(header, config, tuples, |tuples, log2, arity| {
                      parallel::fill_presized_root::<H, P, E>(tuples, log2, arity, threads, config)
                  })
  ```

- [ ] **Step 4: The presized tests compare contents.**

  All edits are in `mod tests` of `kermit-ds/src/ds/hash_trie/parallel.rs`.

  **`presized_in_small_regions`'s closure** (Step 1) becomes the direct call:
  ```rust
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
  ```

  **The test-module `use`'s `identity` import** drops `assert_same_node`, which nothing
  uses any more:
  ```rust
                  identity::{
                      assert_equivalent_root, assert_equivalent_trie, assert_same_contents,
                      assert_same_trie, configs, inputs, LOAD_PERCENTS,
                  },
  ```

  **Replace `check_presized`** (739–811) with:
  ```rust
      /// The presized build through the public constructor, and through
      /// `presized_in_small_regions` with morsels of 7 and 8-bucket regions, so
      /// small inputs span many regions and overflow, under each of
      /// `child_capacities`. Both must be equivalent to bulk under `Tuples`
      /// (Amendment 2). Both must also hold, bucket for bucket, the same
      /// contents as their own `presized:1` for every N; each N's buffer is its
      /// own partitioned copy, so ids are not compared.
      fn check_presized<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
          inputs: &dyn Fn(usize) -> Vec<(&'static str, Tuples)>,
          arities: std::ops::RangeInclusive<usize>, child_capacities: &[ChildCapacity],
      ) {
          for arity in arities {
              for &percent in PRESIZED_LOAD_PERCENTS {
                  for &child_capacity in child_capacities {
                      let config = tuples_config(percent, child_capacity);
                      for (input, tuples) in inputs(arity) {
                          let bulk = HashTrie::<H, P, E>::from_tuples_with_config(
                              arity.into(),
                              config,
                              tuples.clone(),
                          );
                          // `presized:1`'s two builds, which every N must match.
                          let mut first = None;
                          for &t in PRESIZED_THREADS {
                              let label = format!(
                                  "{}/{}/{} arity {arity}, load {percent}%, children {}, \
                                   presized:{t}, {input}",
                                  H::NAME,
                                  P::NAME,
                                  E::NAME,
                                  child_capacity.axis_value(),
                              );
                              let built = HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
                                  arity.into(),
                                  config,
                                  HashTrieBuildMode::Presized(threads(t)),
                                  tuples.clone(),
                              );
                              assert_equivalent_trie(&bulk, &built, &label);
                              let small =
                                  presized_in_small_regions::<H, P, E>(arity, config, &tuples, t, 8);
                              assert_equivalent_trie(&bulk, &small, &format!("{label}, small regions"));
                              match &first {
                                  | None => first = Some((built, small)),
                                  | Some((built_1, small_1)) => {
                                      assert_same_contents(
                                          built_1,
                                          &built,
                                          &format!("{label} vs presized:1"),
                                      );
                                      assert_same_contents(
                                          small_1,
                                          &small,
                                          &format!("{label} vs presized:1, small regions"),
                                      );
                                  },
                              }
                          }
                      }
                  }
              }
          }
      }
  ```
  - The small-regions path used to compare only the root (`assert_equivalent_root`). It
    now compares the whole trie.
  - That adds the buffer's multiset and capacity, the heap size and the tuple count.
    - **Heap size.** The tail's lists grow by pushes from empty, as `bulk`'s chains do.
      The children come from `child`, as `bulk`'s do. So the heap sizes are equal.
    - **Precedent.** The public path's `assert_equivalent_trie` already asserts exactly
      that.

  **Replace `keys_that_cannot_fit_their_region_go_to_the_tail`** (947–975) with:
  ```rust
      #[test]
      fn keys_that_cannot_fit_their_region_go_to_the_tail() {
          let config = tuples_config(70, ChildCapacity::Grow);
          let tuples = Tuples::from((0..16).map(|i| vec![i % 4, i]).collect::<Vec<Vec<usize>>>());
          assert_eq!(
              config.root_log2_capacity(tuples.len()),
              5,
              "a 32-bucket root"
          );
          let bulk: HashTrie<EndOfRegionHash> =
              HashTrie::from_tuples_with_config(2.into(), config, tuples.clone());
          PARALLEL_BUILDS.with(|b| b.borrow_mut().clear());
          let built =
              presized_in_small_regions::<EndOfRegionHash, NoPruning, EagerExpansion>(
                  2, config, &tuples, 2, 8,
              );
          // Four distinct keys, each with four tuples: the first key's
          // tuples stay in the region, the other three keys' twelve go to
          // the tail.
          let builds = PARALLEL_BUILDS.with(|b| b.take());
          assert_eq!(builds.len(), 1);
          assert_eq!(builds[0].deferred, Some(12));
          assert_equivalent_trie(&bulk, &built, "end of region");
      }
  ```
  `from_tuples_partitioned` sizes the root by `root_log2_capacity(16)`, which the
  assertion pins at 5: the `5` the old call passed by hand.

  **In `presized_build_reaches_its_own_path`** (977–997):
  - its doc (977–979) becomes:
    ```rust
        /// `presized:N` reaches its own path, at the thread count asked: four
        /// worker runs (`partition_rows`' histogram and scatter, the regions'
        /// lists, then the regions' children), and a record with a deferred
        /// count.
    ```
  - `assert_eq!(crate::morsel::take_worker_runs(), vec![3, 3, 3]);` becomes
    `assert_eq!(crate::morsel::take_worker_runs(), vec![3, 3, 3, 3]);`.

  **The doc of `presized_parallel_builds_are_the_same_for_every_n_on_dense_roots`**
  (1049–1055) becomes:
  ```rust
      /// Roots dense enough that many keys overflow their 8-bucket region,
      /// with 32 regions, more than any thread count here cuts partitions, so
      /// N changes how regions are grouped, and with it the partitioned copy
      /// and its ids. The trie's contents must still be the same for every N,
      /// bucket for bucket: regions alone decide what is deferred, and the
      /// tail goes in input order, by `origin`. The matrix's small inputs make
      /// at most four partitions at every N, and its large ones fill their
      /// roots to a few percent, so neither has a tail whose order or contents
      /// could depend on N.
  ```
  Its body is unchanged; it compares across N through `check_presized`.

- [ ] **Step 5: Run the presized tests, then everything that pins the mode.**

  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_trie::parallel
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit hash_trie_families_build_with_their_parallel_mode
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests hashpresized2
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test tuple_allocation
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_hash_trie_build_mode
  ```
  Expected: every `test result: ok`, 0 failed. The first run includes:
  - `presized_tail_goes_in_input_order_for_every_n` and
    `presized_stores_its_partitioned_copy`, now green;
  - `presized_parallel_builds_are_equivalent_under_siphash`;
  - `presized_parallel_builds_are_equivalent_under_fxhash_and_colliding_hashes`;
  - `presized_parallel_builds_are_equivalent_on_large_and_skewed_inputs`;
  - `presized_parallel_builds_are_the_same_for_every_n_on_dense_roots`;
  - `keys_that_cannot_fit_their_region_go_to_the_tail`;
  - `presized_build_reaches_its_own_path`;
  - `one_run_of_grouping_builds_the_bulk_root`.

  Also check that nothing in `hash_trie/` still scatters ids:
  ```bash
  rg -n 'scatter_rows|RowPartition' kermit-ds/src/ds/hash_trie
  ```
  Expected: no output.

  Then the workspace, through the long-run procedure (log `$SCRATCH/t5-test.log`, as in
  Task 4 Step 7). Expected: every `test result:` line reads `ok` with `0 failed`.

- [ ] **Step 6: Lint, format, document.**

  ```bash
  nix develop --command env RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc --workspace --no-deps
  ```
  Expected: all clean. Clippy builds kermit's tests, which turn on kermit-ds'
  `test-hooks` feature, so it checks that `partition_sizes` is used under that feature
  and that no import is left unused.

- [ ] **Step 7: Commit.**

  ```bash
  git add kermit-ds/src/ds/hash_trie/parallel.rs kermit-ds/src/ds/hash_trie/implementation.rs
  git commit -m "$(cat <<'EOF'
  feat(hash-trie): presized:N copies its rows into partition order (#101)

  presized:N partitions the rows themselves by home-bucket run with
  morsel::partition_rows. Each run of regions then reads a contiguous
  range of the copy, which becomes the trie's buffer, and the input batch
  is freed.

  fill_runs groups each partition's id range into its run of the
  presized table, and children are built run by run from
  partition-local ids. The deferred tail is sorted by origin, not by new
  id: new ids run partition-major and the partition count depends on N,
  so sorting by id would move tail keys between buckets as N changed.

  The tests compare contents, across N too, and pin the buffer to
  partition_rows' output. A new test pins the tail's input order at
  N = 1 and N = 4.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  EOF
  )"
  ```

- [ ] **Step 8: Mutation check: the tail sorted by new id.**

  Follow the ground rules: commit first, apply the exact edit, confirm it applied, see the
  test fail, reverse the exact edit, never with `git checkout`.
  - In `fill_presized_root_in`, replace
    `    deferred.sort_unstable_by_key(|&row| origin[row as usize]);` with
    `    deferred.sort_unstable();`.
  - Confirm that it applied: `rg -c 'deferred.sort_unstable\(\);'
    kermit-ds/src/ds/hash_trie/parallel.rs` prints `1`. The `origin` binding is then
    unused, and the compiler warns about it. That is expected.
  - Run:
    ```bash
    nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds presized_tail_goes_in_input_order_for_every_n
    ```
    Expected: FAILED, with `presized:4: X, first in input order, takes bucket 8`.
    - Under the mutant, `presized:1` passes, because its copy is the input.
    - At `presized:4`, Y's rows have the smaller new ids, so Y takes bucket 8.
  - Optionally run `presized_parallel_builds_are_the_same_for_every_n_on_dense_roots`
    too. It is expected to fail as well, on a `vs presized:1` label.
  - Reverse the exact edit. Re-run the tail test, which reads `ok`. `git status --short`
    prints nothing.

---

### Task 6: Cleanup: leftovers, and the allocation bounds re-derived

T3–T5 leave three kinds of residue:
- items only a removed path used;
- comments that name a removed function or an arrival-order buffer;
- `kermit/tests/tuple_allocation.rs`, whose HashTrie bounds still count per-partition id
  lists that no build grows any more.

The compiler already rejects dead code and unused imports under the gate's `-D warnings`.
This task looks where it cannot: `#[allow(dead_code)]` markers, comments and docs, and the
test bounds. Keep `scatter_rows` and `RowPartition`: TreeTrie's `parallel:N` uses both,
`len()` and `ids()` included (tree_trie/implementation.rs:225–236).

**B1's allocations, recounted (the `tuple_allocation.rs` HashTrie cells).** Every cell is
differential: `many <= once + bound`, comparing 100 rows (1 repetition) against 10 000 rows
(100 repetitions) of the same 100 distinct tuples. An allocation counts only if its number
differs between the two sizes. `radix:2` passes its whole batch as one morsel (T3), and both
sizes fit one of `parallel:1`'s and `presized:1`'s 16 384-row morsels, so every cell runs
`partition_rows` over exactly one morsel at both sizes.

| Cell | Partition step before B1 | Grows with the rows? | Partition step after B1 | Grows with the rows? |
| --- | --- | --- | --- | --- |
| `radix:2` | `radix::partition`: a `Vec<u16>` of partition numbers and a `sizes` array, then four id lists `with_capacity` from the histogram | no | `partition_rows`, one thread. Per batch: the copy (`value_capacity()`, once), `origin` (once), `offsets` (P + 1, once). Per morsel: one histogram, one list of partition numbers (`with_capacity` of the morsel's rows, never grown; RECONCILE 10(b)) and one range list. Per worker: one result list. Two `run_workers` calls. | no: one morsel, so every count is the same at both sizes |
| `parallel:1` | `scatter_rows`: four id lists per morsel, **each grown one push per id**, plus their segment lists | **yes, 4 lists** | `partition_rows`, as for `radix:2` (T4) | no |
| `presized:1` | `scatter_rows` by home-bucket run: 1 partition at 100 rows, 4 at 10 000. **Each partition is a pushed id list plus a segment list.** | **yes, 3 new partitions and the shared one** | `partition_rows` by home-bucket run (T5). 1 or 4 partitions are ranges of the copy, with no per-partition allocation. | no |

Both before and after, the following cost the same at both sizes:
- the scratch roots' tables, which are sized by keys;
- `first_seen`, the `Entries` lists and the merge heap, all sized by keys;
- the workers;
- the test-hook record (`partition_sizes` is collected from the offsets, at its exact size).

For `presized:1` the same holds for `with_runs`' `inserted` array and run list,
`map_in_runs`' run list, and each `dispatch` result list (sized once; a worker's own list
holds up to four entries in its first allocation). One partition or four therefore cost the
same.

What still grows, in every cell, is one list per first key and one chain per distinct tuple.
That is 10 + 100 = 110 lists, each by at most ⌈log₂ 100⌉ = 7 growth steps.

**The bounds.** Both stay honest upper bounds, so nothing forces a change. But their
derivations name lists that no longer grow: `parallel:1`'s four partition lists, and
`presized:1`'s new partitions plus the partition both builds share. This task removes those
terms. That **tightens** both bounds and loosens neither, so every remaining term names a
list B1 grows:
- `hash_trie_builds_allocate_per_list_not_per_row`: (1 + 10 + 100 + 4) × 7 = 805 becomes
  (1 + 10 + 100) × 7 = 777. The root's list (`all_rows`, a `Range` never built) is kept,
  as before, as 7 allocations of documented slack.
- `hash_trie_presized_build_allocates_per_list_not_per_row`: (10 + 100 + 1) × 7 + 3 × (13 + 1)
  = 819 becomes (10 + 100) × 7 = 770.

**Re-checked under one hash per row** (RECONCILE 10(b)). The partition numbers add one
allocation per morsel. Every cell has one morsel at both sizes, so it is one allocation at
100 rows and one at 10 000 rows, and the differential bounds do not move: 777 and 770
stand. The list is sized up front (`Vec::with_capacity(ids.len())`) and never pushed past
it. Every other `partition_rows` list is also sized once: `offsets` (`with_capacity(P + 1)`),
each morsel's ranges (`with_capacity(P)`), the copy and `origin`. The per-worker result
lists hold one morsel's entry in their first allocation.

By the derivation, the largest measured difference in any cell is at most 110 × 7 = 770,
since only the 110 lists grow. If either test fails, do not raise the bound. Find the
allocation that grows with the rows: under B1 it is a regression against the spec's
"no per-partition list".

**State assumed:** T3, T4 and T5 have landed.

**Files:**
- Modify: `kermit/tests/tuple_allocation.rs` (doc of `hash_trie_builds_allocate_per_list_not_per_row` 364–376 and its `lists` 379; doc of `hash_trie_presized_build_allocates_per_list_not_per_row` 432–442 and its bound 462–465)
- Modify: `kermit-ds/src/relation.rs` (comment 944)
- Not here: T3's `implementation.rs` Storage and Construction sentences, which name only
  `radix:K`, are extended to all three modes by T7 (docs), which owns them.
- Modify, only where Step 1 finds residue: `kermit-ds/src/morsel.rs`, `kermit-ds/src/ds/hash_trie/parallel.rs`, `kermit-ds/src/ds/hash_trie/identity.rs`, `kermit-ds/tests/hash_trie_tests.rs`, `kermit/tests/join_tests.rs`

- [ ] **Step 1: Sweep for leftovers.** Host check first:
  `find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l` prints `0`.

  ```bash
  rg -n "fn partition\b|radix::partition|partition_of: Vec<u16>" kermit-ds/src kermit/src kermit/tests
  rg -n "allow\(dead_code\)" kermit-ds/src kermit-iters/src
  rg -n "scatter_rows|RowPartition|row_id\(tuples.len\(\)\)" kermit-ds/src/ds/hash_trie
  rg -n "TupleList" kermit-ds/src
  rg -n "all_rows" kermit-ds/src
  rg -n "arrival order|identical trie|identical one|input position|kept as is" \
    kermit-ds/src/ds/hash_trie kermit-ds/src/morsel.rs kermit-ds/src/relation.rs \
    kermit-ds/tests/hash_trie_tests.rs kermit/tests/join_tests.rs kermit/tests/cli_hash_trie_build_mode.rs
  ```
  Expected:
  1. **`fn partition` / `radix::partition`:** no hit outside `tuple_allocation.rs:372`'s
     comment, which Step 2 rewrites.
  2. **`allow(dead_code)`:** exactly the three pre-existing markers:
     - `kermit-ds/src/ds/hash_trie/implementation.rs`, two lines in a test fixture (HEAD lines
       1476, 1481);
     - `kermit-ds/src/ds/hash_trie/mod.rs:15` (`mod hash_table`).

     Any marker on a `morsel.rs` item is T1's: remove it if the item is now used.
  3. **`scatter_rows` / `RowPartition` / `row_id(tuples.len())`:** no hit in `hash_trie/`.
     The last pattern is T3's identity `origin`, which T4 replaces. A hit in a comment is
     stale: rewrite it to name `partition_rows`, as in item 6.
  4. **`TupleList`:** every hit is a live use. It is still the chain, list and pending-list
     type until B2: `bulk.rs`, `radix.rs` (scratch lists), `parallel.rs` (`Entries::Chains`,
     the presized table of lists, `push_in_run`), `identity.rs` (`assert_same_chain`) and
     `implementation.rs` (`tuple_list_heap_bytes`). Nothing to remove.
  5. **`all_rows`:** live in `bulk.rs` (definition), `implementation.rs` (the `bulk` and
     `incremental` builds) and any test that builds an Algorithm 2 root by hand. No hit
     remains in `radix.rs`, since T3 removed its import.
  6. **The last grep:** each remaining hit must be true of the code after T5. The one known
     site no earlier task rewrites is `relation.rs:944` (Step 2); Step 2 also lists the sites
     T1, T3, T4 and T5 already made true. For a hit not listed, reword it the same way:
     arrival order belongs to `bulk` and `incremental` only, and partitioning builds name rows
     by their position in the copy, with `origin` mapping back to the input. Hits for "input
     position" in T4's and T5's code (`origin`'s own docs) are true as they stand. Leave the
     `implementation.rs` Storage and Construction sentences and `build_mode.rs` to T7.

- [ ] **Step 2: Rewrite the stale comments found in Step 1.** Wrap at 80 columns. Leave
  T3's two `implementation.rs` sentences that name only `radix:K` (Storage, Construction)
  to T7.

  `kermit-ds/src/relation.rs:944`. Before:
  ```rust
      /// A hash trie's tuples, sorted (a hash trie keeps them in arrival order).
  ```
  After:
  ```rust
      /// A hash trie's tuples, sorted: a hash trie keeps them in its buffer's
      /// order, arrival order or a partitioning build's partition order.
  ```

  **Already true: verify, do not edit** (RECONCILE 4). The Step 1 grep's other known sites
  were rewritten by earlier tasks:
  - the radix blocks of `kermit-ds/tests/hash_trie_tests.rs` and `kermit/tests/join_tests.rs`
    (T3 Step 7), and their parallel blocks (T4 Step 6);
  - in `parallel.rs`: the module doc and `fill_root_in_morsels`' comments (T4 Step 3), and
    `fill_presized_root_in`'s doc and tail comment and `fill_runs`' doc (T5 Step 3). None of
    them names `scatter_rows` any more;
  - `morsel.rs`' module doc, which T1 Step 7 extended to name `partition_rows`.

  If the Step 1 grep shows one of them still stale, that task's edit did not land: apply the
  task's own text, not a new wording.

- [ ] **Step 3: Re-derive `tuple_allocation.rs`'s HashTrie bounds.**

  The doc of `hash_trie_builds_allocate_per_list_not_per_row` (lines 364–376). Before:
  ```rust
  /// Under the default `root-capacity=grow` every table's size depends on its
  /// keys alone (`tuples` would size the root from the row count), so the
  /// tables, the chains and the lazy pending lists are the same at both
  /// repetitions. Only the row-id lists hold more ids: one per first key and
  /// one chain per distinct tuple, each by at most ⌈log₂ 100⌉ = 7 growth steps.
  /// The bound also counts the root's list and four partition lists. Only
  /// `parallel:1`'s four partitions grow, one push per row id: `scatter_rows`
  /// keeps each in one list, as both inputs fit one 16 384-row morsel. The
  /// root's list is `all_rows`, a `Range` never built, and `radix::partition`
  /// sizes `radix:2`'s four partitions exactly from a histogram, so under
  /// every other mode those five lists are part of the bound's slack, 5 × 7 =
  /// 35 allocations. `parallel:1`'s workers, merge and test-hook record cost
  /// the same at both repetitions.
  ```
  After:
  ```rust
  /// Under the default `root-capacity=grow` every table's size depends on its
  /// keys alone (`tuples` would size the root from the row count), so the
  /// tables, the chains and the lazy pending lists are the same at both
  /// repetitions. Only the row-id lists hold more ids: one per first key and
  /// one chain per distinct tuple, each by at most ⌈log₂ 100⌉ = 7 growth steps.
  /// The bound also counts the root's list, which is `all_rows`, a `Range`
  /// never built: 7 allocations of slack under every mode.
  ///
  /// The partitioning builds, `radix:2` and `parallel:1`, grow no list per
  /// partition (#101). `partition_rows` counts each partition's rows in a
  /// histogram pass, keeping each row's partition number in a list sized
  /// from its morsel, then copies the rows and their input positions
  /// (`origin`) into buffers sized once from the batch, and a partition is a
  /// range of the copy. `radix:2` passes its whole batch as one morsel, and
  /// both inputs fit one of `parallel:1`'s 16 384-row morsels, so the
  /// histogram, the partition numbers, the offsets and the morsel's ranges
  /// are one allocation each at both repetitions. So are the workers, the
  /// scratch roots' tables (sized by their keys), the merge, and
  /// `parallel:1`'s test-hook record.
  ```
  The `lists` line (line 379). Before:
  ```rust
      let lists = (1 + KEYS + KEYS * KEYS + 4) as u64;
  ```
  After:
  ```rust
      let lists = (1 + KEYS + KEYS * KEYS) as u64;
  ```

  The doc of `hash_trie_presized_build_allocates_per_list_not_per_row` (lines 432–442).
  Before:
  ```rust
  /// `presized:1` under `root-capacity=tuples`, its prerequisite. The root is
  /// sized once from the row count, one allocation at either repetition, and
  /// its lists and chains grow as under the other builds. Its partitions are
  /// whole runs of the root's 4 096-bucket regions, at most
  /// [`ONE_THREAD_PARTITIONS`]: the 100 rows' 256-bucket root is one region and
  /// one partition, the 10 000 rows' 16 384-bucket root four. So the larger
  /// build has three partitions more, each a row-id bucket grown from empty (at
  /// most `doublings(rows)` steps) and a segment list, and the one partition
  /// both builds share is one more growing list. The fixture's ten keys never
  /// probe past a region's end, so no row is deferred to the tail, whose list
  /// the bound does not count.
  ```
  After:
  ```rust
  /// `presized:1` under `root-capacity=tuples`, its prerequisite. The root is
  /// sized once from the row count, one allocation at either repetition, and
  /// its lists and chains grow as under the other builds: one list per first
  /// key and one chain per distinct tuple, each by at most ⌈log₂ 100⌉ = 7
  /// growth steps. Its partitions are whole runs of the root's 4 096-bucket
  /// regions, at most [`ONE_THREAD_PARTITIONS`]: the 100 rows' 256-bucket
  /// root is one region and one partition, the 10 000 rows' 16 384-bucket
  /// root four.
  ///
  /// The three extra partitions cost no allocation (#101). `partition_rows`
  /// sizes its histogram, partition numbers, offsets, copy and `origin`
  /// once, and a partition is a range of the copy. `with_runs` and `map_in_runs` size their run lists
  /// once, and each worker's result list holds four entries in its first
  /// allocation, so one partition or four cost the same. The fixture's ten
  /// keys never probe past a region's end, so no row is deferred to the
  /// tail, whose list the bound does not count.
  ```
  The bound (lines 462–465). Before:
  ```rust
      let rows = REPEATS * KEYS * KEYS;
      let lists = (KEYS + KEYS * KEYS + 1) as u64;
      let new_partitions = (many_partitions - once_partitions) as u64;
      let bound = lists * ceil_log2(REPEATS) + new_partitions * (doublings(rows) + 1);
  ```
  After:
  ```rust
      let rows = REPEATS * KEYS * KEYS;
      let lists = (KEYS + KEYS * KEYS) as u64;
      let bound = lists * ceil_log2(REPEATS);
  ```
  The `assert_eq!((once_partitions, many_partitions), (1, 4));` line stays: it pins that the
  larger build really has three more partitions, which now cost nothing. `doublings` and
  `ONE_THREAD_PARTITIONS` stay in use by the TreeTrie cells and the presized doc link.

- [ ] **Step 4: Run.**

  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test tuple_allocation
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds
  ```
  Expected: `tuple_allocation`: 9 passed, among them
  `hash_trie_builds_allocate_per_list_not_per_row` (bound 777) and
  `hash_trie_presized_build_allocates_per_list_not_per_row` (bound 770). Every `kermit-ds`
  result is `ok`. If a HashTrie cell fails, its message prints both counts. Do not raise the
  bound: find the allocation that grows with the rows (see the derivation above).

- [ ] **Step 5: Lint, format, document.**

  ```bash
  nix develop --command env RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc --workspace --no-deps
  ```
  Expected: no warnings, and no fmt diff after the formatting run.

- [ ] **Step 6: Commit.** Stage each file the steps changed, by name. For example:

  ```bash
  git add kermit/tests/tuple_allocation.rs kermit-ds/src/relation.rs
  git status --short   # add any Step 2 file listed as modified, by name
  git commit -m "$(cat <<'EOF'
  test: re-derive the HashTrie allocation bounds after partitioned materialisation (#101)

  partition_rows sizes the copy, origin and offsets once from the batch,
  and a partition is a range of the copy, so radix:2, parallel:1 and
  presized:1 grow no list per partition. tuple_allocation.rs's HashTrie
  bounds drop the terms for the per-partition id lists scatter_rows used
  to grow (805 -> 777, 819 -> 770): every remaining term is a list B1
  still grows, one per first key and one chain per distinct tuple.

  Comments that still described the radix family's buffer as arrival
  order, or named radix::partition and scatter_rows for HashTrie, now
  describe the partitioned copy.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  EOF
  )"
  ```

---

### Task 7: Documentation

The spec's Documentation list, plus every comment and doc that T3–T6 left naming only
`radix:K`, or still calling the radix family identical to `bulk` (RECONCILE 5, 8 and 9).
- **The parity wording** the HashTrie direction requires (#105):
  - the partitioned copy is the paper's materialisation step (§3.3.2);
  - one round of partitioning is kermit's (spec Decision 4);
  - `bulk` and `incremental` keep arrival order;
  - the chain pointer per tuple is B2's.
- **No outward-facing step here.** The #105 board and the issue comments are in "After this
  plan", as text for the user to approve.

Each edit below is an exact before/after. Where a line is long, "before" is the exact
substring to replace. Check each is unique with `grep -cF '<before>' <file>` (expect `1`)
before editing; where a "before" is not unique, the step says what anchors it.

Line numbers are at a85c263. No earlier task edits these lines, except:
- T3, the `implementation.rs` Storage and Construction paragraphs (Step 1 anchors them by
  their first and last lines);
- T4 and T5, which edit none of these files' docs.

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (Storage and Construction paragraphs as T3 left them; the `from_tuples_with_config_and_build_mode` doc, 464–468)
- Modify: `kermit-ds/src/ds/hash_trie/build_mode.rs` (10–15, 29–31, 33–37, 39–47, 51–52, 57)
- Modify: `kermit/src/execution.rs` (1846–1848, 1863–1865; doc comments only)
- Modify: `kermit/tests/cli_hash_trie_build_mode.rs` (module doc, 5–7)
- Modify: `docs/data-structures/hash-trie.md` (12, 43, 45–52, 68–75, 97, 111, 123, 422–425, 450–536, 556–602, 614–619, 637, 683, 784–787)
- Modify: `docs/data-structures/parallel-build.md` (6–13, 31–33, 150–159, 163–191, 193–211, 229, 233–246, 248–261, 270–285, 304–319, 326–328, 335–354, 544)
- Modify: `CLAUDE.md` (13, 148)
- Modify: `ARCHITECTURE.md` (218, after 231, 234)

- [ ] **Step 1: The Rust doc comments.** Each is wrapped at 80 columns.

  **`implementation.rs`, the Storage paragraph.** Replace every line from the one beginning
  `/// The trie owns its tuples as one row-major buffer` through the one ending
  `/// second until #101 layer 3).`. That is the paragraph under `/// # Storage`, as T3 and
  `cargo fmt` left it, naming `radix:K` only. The new text:
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

  **`implementation.rs`, the Construction sentence** that T3 wrote for `radix:K` only.
  - Replace the lines from the one beginning `/// the same contents and capacities.`
    through the one ending `/// other ids.`.
  - Before (as T3 wrote it; `cargo fmt` may have re-wrapped it, so match the sentence, not
    the line breaks):
  ```rust
  /// the same contents and capacities. `presized:N` may place root keys in
  /// other buckets, and `radix:K` stores its rows in partition order, under
  /// other ids.
  ```
  After:
  ```rust
  /// the same contents and capacities. `presized:N` may place root keys in
  /// other buckets, and `radix:K`, `parallel:N` and `presized:N` store their
  /// rows in partition order, under other ids (#101).
  ```

  **`implementation.rs`, `from_tuples_with_config_and_build_mode`'s doc** (464–468). Before:
  ```rust
      /// Creates a trie holding `config`, populated with `tuples` and built by
      /// `mode` — the one constructor that takes both the Config and the
      /// BuildMode. Every mode builds an equivalent trie (issues #91, #94;
      /// `Presized` may place root keys in other buckets, Amendment 2), so
      /// `mode` changes only how long this takes.
  ```
  After:
  ```rust
      /// Creates a trie holding `config`, populated with `tuples` and built by
      /// `mode` — the one constructor that takes both the Config and the
      /// BuildMode. Every mode builds an equivalent trie (issues #91, #94,
      /// #101; Amendment 2): `Radix`, `Parallel` and `Presized` store the
      /// tuples in partition order, under other row ids, and `Presized` may
      /// place root keys in other buckets. So `mode` changes how long this
      /// takes and where tuples sit, never the trie's contents.
  ```

  **`build_mode.rs`, the enum's doc** (10–15). Before:
  ```rust
  /// How a [`HashTrie`](super::HashTrie) is built from a known set of tuples.
  /// Every mode builds an equivalent trie, the same contents with the same
  /// capacities (the BuildMode rule). `Bulk`, `Incremental`, `Radix` and
  /// `Parallel` also put each key in the same bucket; `Presized` may place root
  /// keys in other buckets (Amendment 2). The mode changes how long the build
  /// takes, never the trie's contents (issues #91, #94, #107).
  ```
  After:
  ```rust
  /// How a [`HashTrie`](super::HashTrie) is built from a known set of tuples.
  /// Every mode builds an equivalent trie, the same contents with the same
  /// capacities (the BuildMode rule). `Bulk`, `Incremental`, `Radix` and
  /// `Parallel` also put each key in the same bucket; `Presized` may place root
  /// keys in other buckets (Amendment 2). `Radix`, `Parallel` and `Presized`
  /// copy the tuples into partition order and keep the copy as the trie's
  /// buffer (#101), so their row ids differ from `Bulk`'s. The mode changes
  /// how long the build takes and where tuples sit, never the trie's contents
  /// (issues #91, #94, #101, #107).
  ```

  **`build_mode.rs`, the `Radix` variant** (29–31). Before:
  ```rust
      /// Radix-partition the tuples on the top bits of their first
      /// attribute's hash, build each partition separately, then merge
      /// (SIGMOD 2020 §3.3.2).
  ```
  After:
  ```rust
      /// Radix-partition the tuples on the top bits of their first
      /// attribute's hash, copying them into partition order (the trie's
      /// buffer, #101), build each partition separately, then merge
      /// (SIGMOD 2020 §3.3.2).
  ```

  **`build_mode.rs`, the `Parallel` variant** (33–37). Before:
  ```rust
      /// The radix build's partition and build steps on this many threads,
      /// the calling thread included: the morsel-driven parallel build
      /// (`docs/data-structures/parallel-build.md`, issue #94). The exact
      /// merge build under every root capacity: a presized root changes the
      /// root's size, never the process.
  ```
  After:
  ```rust
      /// The radix build's partition and build steps on this many threads,
      /// the calling thread included: the morsel-driven parallel build
      /// (`docs/data-structures/parallel-build.md`, issue #94), whose workers
      /// also make the copy into partition order (#101). The exact merge build
      /// under every root capacity: a presized root changes the root's size,
      /// never the process.
  ```

  **`build_mode.rs`, the `Presized` variant** (39–47). Before:
  ```rust
      /// The partitioned build of a presized root on this many threads: the
      /// input partitioned by its first attribute's hash, as the paper does
      /// (VLDB 2020 §3.3.2); each worker groups its run of the root's regions,
      /// a tail of deferred tuples goes to the calling thread, then each worker
      /// builds its run's children (kermit's scheme, `parallel.rs`). The root
      /// must be sized before any tuple arrives, so this mode **requires
      /// `root-capacity=tuples`**
      /// (`docs/specs/2026-10-07-dependent-optimisations-design.md`);
      /// the constructor panics without it, and the CLI rejects it first.
  ```
  After:
  ```rust
      /// The partitioned build of a presized root on this many threads: the
      /// input copied into partition order by its first attribute's hash, as
      /// the paper does (VLDB 2020 §3.3.2, #101); each worker groups its run of
      /// the root's regions, a tail of deferred tuples goes to the calling
      /// thread in input order, then each worker builds its run's children
      /// (kermit's scheme, `parallel.rs`). The root must be sized before any
      /// tuple arrives, so this mode **requires `root-capacity=tuples`**
      /// (`docs/specs/2026-10-07-dependent-optimisations-design.md`);
      /// the constructor panics without it, and the CLI rejects it first.
  ```

  **`build_mode.rs`, `RadixBits`** (51–52 and 57). `radix::partition` numbered partitions in a
  `u16`, and `partition_rows` keeps them in a `u32`. Before:
  ```rust
  /// The radix bits of a `radix` build: `2^bits` partitions, `bits` in
  /// `1..=16`, so a tuple's partition number fits a `u16`.
  ```
  After:
  ```rust
  /// The radix bits of a `radix` build: `2^bits` partitions, `bits` in
  /// `1..=16`.
  ```
  Before:
  ```rust
      /// The most bits: 65,536 partitions, numbered in a `u16`.
  ```
  After:
  ```rust
      /// The most bits: 65,536 partitions.
  ```

  **`kermit/src/execution.rs`, `CountingHash`** (1846–1848). With one hash per row in
  `partition_rows` (RECONCILE 10(b)), the radix build still hashes each first attribute
  exactly once more than `bulk`. Before:
  ```rust
      /// SipHash that counts its calls. The radix build hashes each tuple's
      /// first attribute once more than the bulk build, and the two build
      /// identical tries, so the count is the only way to see which one ran.
  ```
  After:
  ```rust
      /// SipHash that counts its calls. The radix build hashes each tuple's
      /// first attribute once more than the bulk build (it partitions on that
      /// hash first), and the two build tries of the same contents, so this
      /// test tells them apart by the count.
  ```

  **`kermit/src/execution.rs`, `hash_families_build_with_their_mode`'s doc** (1863–1865).
  Before:
  ```rust
      /// Both modes build identical tries, so only a spy can see whether the
      /// family's mode reached the build, on every route a relation is built,
      /// eager or lazy.
  ```
  After:
  ```rust
      /// Both modes build tries of the same contents, so only a spy can see
      /// whether the family's mode reached the build, on every route a
      /// relation is built, eager or lazy.
  ```
  The sorted families' test at 1666 ("Both modes build identical tries") is ColumnTrie's
  and stays.

  **`kermit/tests/cli_hash_trie_build_mode.rs`, module doc** (5–7). Before:
  ```rust
  //! `--ds-build hash-trie=…`. Every mode builds an equivalent trie (the
  //! identical one, except `presized:N`, whose root keys may sit in other
  //! buckets), so only the axis (and build time) shows which ran.
  ```
  After:
  ```rust
  //! `--ds-build hash-trie=…`. Every mode builds an equivalent trie, the same
  //! contents and capacities (`presized:N` may place root keys in other
  //! buckets, and `radix:K`, `parallel:N` and `presized:N` store their rows
  //! in partition order, #101), so only the axis (and build time) shows which
  //! ran.
  ```

- [ ] **Step 2: `docs/data-structures/hash-trie.md`.**

  Line 12, before:
  ```text
      tuples:      Tuples,            // every stored tuple, arrival order; len() = multiset size
  ```
  After:
  ```text
      tuples:      Tuples,            // every stored tuple: arrival or partition order (below); len() = multiset size
  ```

  Line 43, the whole paragraph beginning `**The tuple buffer** (#111).`, becomes:
  ```markdown
  **The tuple buffer** (#111, #101). The trie owns its relation's tuples as one row-major `Tuples` batch: the batch a build is given, then every row `insert` appends. `bulk` and `incremental` keep that batch without a copy, in arrival order. `radix:K`, `parallel:N` and `presized:N` copy it into partition order on the first attribute's hash (`morsel::partition_rows`, the paper's materialisation step, §3.3.2) and keep the copy, so each partition's rows are one contiguous range of the buffer, in input order; the input batch is freed once copied. Below the tables every tuple is named by its `RowId` (`u32`, an index into the buffer): a leaf chain is a `Vec<RowId>` in input order, a pruned `Singleton` one `RowId`, a lazy pending list a `Vec<RowId>`. `HashTrieIterator::leaf_tuples` lends a chain as a `LeafRows` view over the buffer. Two builds that keep their input own equal buffers, so the `bulk`/`incremental` identity tests compare ids, after checking the buffers are equal; the partitioning modes are compared by contents, each id read through its own trie's buffer (`identity::assert_same_contents`). Reading back: `for_each_tuple` walks the trie depth-first, reading each row through its id (`bench ds` times it, #79); `TupleScan::scan_tuples` (statistics, which need only the multiset) scans the buffer in its row order; `collect_tuples` copies the buffer.
  ```

  Lines 45–52, from `| Element (VLDB 2020 §3.3.1–3.3.2, Fig. 3) | kermit since #111 |` through the
  paragraph ending `and with it the paper's cost.`, become:
  ```markdown
  | Element (VLDB 2020 §3.3.1–3.3.2, Fig. 3) | kermit since #101 B1 (2026-10-08) |
  | --- | --- |
  | Tuples in one contiguous buffer, fixed-length layout | ✓ the relation's buffer, row-major (#111) |
  | Leaves, singletons and lazy children refer to tuple memory | ✓ by row id (4 B, kermit's; the paper's pointers are 8 B) |
  | Tuples partitioned by the first key's hash as they are materialised | ✓ in `radix:K`, `parallel:N` and `presized:N` (`morsel::partition_rows`); ✗ arrival order in `bulk` (the default) and `incremental` |
  | Partitioning morsel-driven, on N threads | ✓ the copy itself, in `parallel:N` and `presized:N` |
  | Two-pass radix partitioning (Balkesen et al.) | ✗ one round of partitions (kermit's: #101 found no fan-out cost at this scale, `radix:16` within 1.5 % of `radix:12`) |
  | A leaf is a list threaded through an 8-byte chain pointer per tuple | ✗ a `Vec<RowId>` per chain (kermit's), until #101 B2 |

  The paper's buffer is a copy its build makes. `bulk` and `incremental` take the relation's buffer without copying it, so they pay nothing for it but keep arrival order. The partitioning modes make the paper's copy and pay for it:
  - a histogram pass and a scatter of every row;
  - while the copy is made, the input and the copy both held (the input is freed before the build);
  - 4 bytes per row of `origin`, each copied row's input position, held until the build returns, so the root merge and `presized:N`'s tail keep `bulk`'s order;
  - 4 bytes per row of partition numbers, held between the histogram and the scatter, so each row is hashed once.

  Design: [`docs/specs/2026-10-08-hash-trie-partitioned-materialisation-design.md`](../specs/2026-10-08-hash-trie-partitioned-materialisation-design.md).
  ```

  The Construction table (68–75): insert a row directly below the separator under
  `| Algorithm 2 | kermit |` (the `|---|---|` on line 69):
  ```markdown
  | before Algorithm 2, the input "materialized contiguously in an in-memory buffer" and "partitioned based on the hash values of the first join key attribute" (§3.3.2) | `bulk` and `incremental` keep the relation's batch, in arrival order, without a copy (kermit's); `radix:K`, `parallel:N` and `presized:N` copy it into partition order (`morsel::partition_rows`, #101) and keep the copy ([Build modes](#build-modes)) |
  ```
  Line 75's tail, before:
  ```text
  which needs the partitioned copy of the buffer (#101 layer 3) |
  ```
  After:
  ```text
  which needs the partitioned copy of the buffer: the partitioning modes make it since #101 B1, and the chain pointer is #101 B2 |
  ```

  Line 97, before (substring):
  ```text
  which walked the trie too; both now read its buffer in arrival order)
  ```
  After:
  ```text
  which walked the trie too; both now read its buffer, in its row order)
  ```

  Line 111, the whole row, before:
  ```text
  | `from_tuples` under `radix:K` | O(n · a + D log D) | O(n · a) | the bulk build, plus two partition passes, a second first-attribute hash per tuple and a sort of the D distinct root keys; builds the identical trie |
  ```
  After:
  ```text
  | `from_tuples` under `radix:K` | O(n · a + D log D) | O(n · a) | the bulk build over a partitioned copy: a histogram pass and a scatter copying every row (`partition_rows`, one first-attribute hash per tuple, kept for the scatter), a second first-attribute hash per tuple in the scratch roots, and a sort of the D distinct root keys; transiently the copy beside the input while it is made, and 4 B per tuple each of `origin` and partition number; builds an equivalent trie, the bulk trie's contents and root with the rows in partition order |
  ```

  Line 123, before (substring):
  ```text
  `collect_tuples()` copies the buffer, in arrival order, and
  ```
  After:
  ```text
  `collect_tuples()` copies the buffer, in its row order, and
  ```

  Config flags, the child-capacity record (line 425), before:
  ```text
      - Under `presized:16` it is unchanged (0.99×).
  ```
  After:
  ```text
      - Under `presized:16` it is unchanged (0.99×; measured before
        `presized:N` copied its rows into partition order, #101 B1).
  ```

  Build modes, the intro (450–457), before:
  ```markdown
  Every mode here but `presized:N` builds the identical trie under the same
  config, under every config it accepts — the same buckets, the same
  capacities, the same `heap_size_bytes` — so the mode changes the
  `insertion` and `end_to_end` timings and nothing else (issues #91, #94,
  #107). The BuildMode rule asks less since Amendment 2
  (2026-10-06): the same contents and capacities. `presized:N` uses that
  freedom; it may place root keys in other buckets, so `iteration` is measured
  for it.
  ```
  After:
  ```markdown
  Every mode here builds an equivalent trie under the same config, under
  every config it accepts: the same contents, the same capacities, the same
  `heap_size_bytes`. That is the BuildMode rule since Amendment 2
  (2026-10-06; issues #91, #94, #101, #107). The modes differ only in what
  the rule leaves free:
  - `bulk` and `incremental` build the identical trie, bucket for bucket and
    id for id.
  - `radix:K`, `parallel:N` and `presized:N` copy their rows into partition
    order and keep the copy as the trie's buffer (#101 B1, 2026-10-08). Every
    chain, singleton and pending list holds the same tuples, in the same
    order, under other row ids.
  - `radix:K`'s and `parallel:N`'s tables are `bulk`'s bucket for bucket, the
    root included; `presized:N` may also place root keys in other buckets.

  So `iteration` is measured for the three partitioning modes, never assumed.
  ```

  Build modes, the table's three rows (463–465). Line 463, before (substring):
  ```text
  | radix-partition on the top K bits of the first attribute's hash (SIGMOD 2020 §3.3.2, the partitioning only), group each partition into a scratch root and build its children by Algorithm 2, merge the scratch roots (kermit's) |
  ```
  After:
  ```text
  | copy the rows into 2^K partitions on the top K bits of the first attribute's hash and keep the copy as the buffer (SIGMOD 2020 §3.3.2: the tuples partitioned as they are materialised, in one round, #101), group each partition into a scratch root and build its children by Algorithm 2, merge the scratch roots (kermit's) |
  ```
  Line 464, before (substring):
  ```text
  | the radix build's partition and build steps on N threads (P = 4·N partitions, rounded up to a power of two), then a k-way merge into the root on the calling thread (morsel-driven partitioning per §3.3.2; the scratch-root build and the merge are kermit's) |
  ```
  After:
  ```text
  | the radix build's partition step (the copy) and build step on N threads (P = 4·N partitions, rounded up to a power of two), then a k-way merge into the root on the calling thread (morsel-driven partitioning and materialisation per §3.3.2; the scratch-root build and the merge are kermit's) |
  ```
  Line 465, before (substring):
  ```text
  | the input partitioned by the first attribute's hash (§3.3.2) into regions of the presized root, each worker grouping its regions (Algorithm 2, lines 4–7), a tail of deferred tuples pushed on the calling thread, then
  ```
  After:
  ```text
  | the input copied into partition order by the first attribute's hash (§3.3.2, #101), one partition per run of regions of the presized root, each worker grouping its regions (Algorithm 2, lines 4–7), a tail of deferred tuples pushed on the calling thread in input order, then
  ```

  The radix build (467–501), from `**The radix build** ([`radix.rs`]` through the line
  `  tables.` that ends the Cost list (just before `**The #66 input shape.**`), becomes:
  ```markdown
  **The radix build** ([`radix.rs`](../../kermit-ds/src/ds/hash_trie/radix.rs)):

  1. **Partition.** `morsel::partition_rows`, on the calling thread with the
     whole batch as one morsel, copies every row into one of 2^K partitions
     by the top K bits of its first attribute's hash: a histogram pass, which
     keeps each row's partition number, then a stable scatter into one new
     buffer, recording each copied row's input position (`origin`). The copy
     becomes the trie's buffer and the input is freed, so partition p is the
     contiguous id range `offsets[p]..offsets[p + 1]`, in input order (#101).
     One morsel, because `partition_rows` keeps a count and a range per
     (partition, morsel) pair: at `radix:16` over 10⁷ rows, 16 384-row
     morsels would make about 40 million pairs.
  2. **Scratch roots.** Each non-empty partition's ids are grouped into a
     scratch root of the real root's kind (Algorithm 2, lines 4–7), recording
     the input position (`origin[id]`) of the row that introduced each key;
     then each key's child is built from its list, as the bulk build builds
     it, reading only that partition's rows.
  3. **Merge.** Every scratch entry moves into the real root, in the order of
     those first-appearance positions.

  **Why it is equivalent.**

  - A table's final layout depends only on the order in which its *new* keys
    arrive, because `entry_or_insert_with` returns an existing entry before
    its resize check.
  - The merge inserts the root's distinct keys in first-appearance order, as
    the bulk build does. It orders them by input position, not by id, since
    ids in the copy run partition by partition, so the root is `bulk`'s,
    bucket for bucket.
  - Each root key's child is built from the same tuples, in input order,
    because the copy is stable. That covers every table, `Singleton`, chain
    and capacity and, under `LazyExpansion`, every pending list. Only the ids
    differ: a chain names its tuples by their positions in the copy.

  **Cost.** Every per-tuple probe and every child build stays inside one
  partition, about 1/2^K of the trie and one contiguous range of the buffer,
  and only the D merge inserts (one per distinct first-attribute hash) touch
  the real root at random. In return the build pays for:

  - the copy: a histogram pass and a scatter of every row, which zero-fills
    the copy first (safe Rust cannot hand out uninitialised ranges);
  - a second hash of each tuple's first attribute (`partition_rows` hashes
    each row once and keeps the number for its scatter);
  - a sort of the D merge entries;
  - transient memory: the copy beside the input while it is made, 4 bytes
    per tuple of partition number (between the two passes), 4 bytes per
    tuple of `origin` (until the build returns), and the scratch tables.
    Before #101 B1 it was 2 bytes of partition number and 4 of row id in
    the partitioned lists, and 32 bytes of `(index, tuple)` pair before
    #111.
  ```

  The parallel build (510–516), before:
  ```markdown
  **The parallel build** (`parallel.rs`) runs the radix build's partition
  step through `morsel::scatter_rows` and its build step through
  `morsel::dispatch`, so each runs on N threads. The calling thread then
  merges the partitions' entries into the root by a k-way merge on their
  first-appearance positions. The trie is the radix build's, and so the
  bulk build's. Steps, identity argument, complexity and a worked example:
  [`parallel-build.md`](./parallel-build.md#hashtrie).
  ```
  After:
  ```markdown
  **The parallel build** (`parallel.rs`) runs the radix build's partition
  step through `morsel::partition_rows` and its build step through
  `morsel::dispatch`, so each runs on N threads: N workers make the copy,
  each scattering its morsels' rows into disjoint ranges of the one new
  buffer. The calling thread then merges the partitions' entries into the
  root by a k-way merge on their first-appearance positions (`origin`). The
  trie holds the radix build's contents and root, and so the bulk build's.
  Steps, equivalence argument, complexity and a worked example:
  [`parallel-build.md`](./parallel-build.md#hashtrie).
  ```

  The presized build (518–531), before:
  ```markdown
  **The presized build** (`presized:N`). It requires `root-capacity=tuples`,
  since its regions are cut from a root sized before any tuple arrives; the
  CLI rejects the pair otherwise and the constructor panics. It partitions the
  input by the first attribute's hash (§3.3.2) into contiguous regions of the
  presized root, and each worker pushes the id of every row of its regions
  onto its bucket's list. Keys whose probe would cross their region's end are finished
  by the calling thread, in a tail. The paper does not say how it handles
  that case, so this is kermit's answer. After the tail, the children are
  built by Algorithm 2 a run of buckets at a time, the runs handed to
  whichever worker is free (how the recursion is spread over threads is
  kermit's choice; the paper is silent). The trie is
  equivalent to bulk's (Amendment 2): every subtrie and chain is
  array-identical; the root has the same capacity, the same occupied buckets
  and the same total displacement; and it is the same for every N. The
  ```
  After:
  ```markdown
  **The presized build** (`presized:N`). It requires `root-capacity=tuples`,
  since its regions are cut from a root sized before any tuple arrives; the
  CLI rejects the pair otherwise and the constructor panics. It copies the
  input into partition order by the first attribute's hash (§3.3.2, #101),
  one partition per contiguous run of regions of the presized root, and each
  worker pushes the id of every row of its partition, one contiguous range
  of the copy, onto its bucket's list. Keys whose probe would cross their
  region's end are finished by the calling thread, in a tail, in input order
  (by `origin`: ids in the copy run partition by partition, and the
  partition count depends on N). The paper does not say how it handles
  that case, so this is kermit's answer. After the tail, the children are
  built by Algorithm 2 a run of buckets at a time, the runs handed to
  whichever worker is free (how the recursion is spread over threads is
  kermit's choice; the paper is silent). The trie is equivalent to bulk's
  (Amendment 2): every subtrie and chain holds the same tuples in the same
  order; the root has the same capacity, the same occupied buckets and the
  same total displacement; and its contents are the same for every N,
  bucket for bucket, though each N's buffer is its own partitioned copy. The
  ```
  (Lines 532–536, from `closest-to-paper configuration is` on, are unchanged.)

  Build modes, **Tests** (551–602). Lines 556–558, before:
  ```markdown
    - `radix_builds_the_bulk_trie_*` in `radix.rs`: array-level identity,
      capacities included, across arity, pruning, expansion, hasher, K, load
      factor and input. Miri runs a smaller matrix.
  ```
  After:
  ```markdown
    - `radix_builds_the_bulk_trie_*` in `radix.rs`: content-level identity
      (`identity::assert_same_contents`: every table bucket for bucket, the
      root included; every chain, singleton and pending list by the tuples
      its ids name; the buffers as a multiset at the same capacity) and equal
      heap size, across arity, pruning, expansion, hasher, K, load factor and
      input. Miri runs a smaller matrix.
      `radix_keeps_the_partitioned_copy_as_its_buffer` pins the buffer to
      `morsel::partition_rows`' output, the one difference no content test
      sees (#101).
  ```
  Lines 565–568, before:
  ```markdown
    - `parallel_builds_the_bulk_trie_*` and `build_modes_reach_their_builds`
      in `parallel.rs`: identity for N ∈ {1, 2, 3, 8}, morsels of 7 and of
      16 384, arity 1–4 and a dominant key; and the record that shows which
      build ran, with which N.
  ```
  After:
  ```markdown
    - `parallel_builds_the_bulk_trie_*` and `build_modes_reach_their_builds`
      in `parallel.rs`: content-level identity for N ∈ {1, 2, 3, 8}, morsels
      of 7 and of 16 384, arity 1–4 and a dominant key; and the record that
      shows which build ran, with which N, and its worker runs.
      `parallel_stores_its_partitioned_copy` pins the buffer to
      `partition_rows`' output at `partition_bits(N)` partitions.
  ```
  Lines 574–578, before:
  ```markdown
    - `presized_parallel_builds_are_equivalent_*` and
      `presized_parallel_builds_are_the_same_for_every_n_on_dense_roots` in
      `parallel.rs`: equivalence with bulk under `root-capacity=tuples` and
      identity with `presized:1`, for every Layout, load factors 50–95 % and
      N ∈ {1, 2, 3, 8}, at 8-bucket regions and at the real size;
  ```
  After:
  ```markdown
    - `presized_parallel_builds_are_equivalent_*` and
      `presized_parallel_builds_are_the_same_for_every_n_on_dense_roots` in
      `parallel.rs`: equivalence with bulk under `root-capacity=tuples`
      (children compared by contents) and content identity with
      `presized:1`, for every Layout, load factors 50–95 % and
      N ∈ {1, 2, 3, 8}, at 8-bucket regions and at the real size;
      `presized_stores_its_partitioned_copy` (the buffer is
      `partition_rows`' copy by home-bucket run);
      `presized_tail_goes_in_input_order_for_every_n` (the tail is pushed by
      `origin`, so a tail key's bucket is the same at N = 1 and N = 4);
  ```
  Lines 583–586, before (substring):
  ```markdown
      mode reaches its own fill, and `parallel:N` is identical to bulk under
      both root capacities; `presized_build_requires_a_presized_root`
  ```
  After:
  ```markdown
      mode reaches its own fill, and `parallel:N` holds bulk's contents and
      root under both root capacities; `presized_build_requires_a_presized_root`
  ```
  After the bullet that ends `` `kermit/tests/cli_hash_trie_build_mode.rs`. `` (line 602),
  insert:
  ```markdown
    - `partition_rows_*` in `kermit-ds/src/morsel.rs`: offsets, membership,
      stability, `origin`, the same copy for every thread count and morsel
      size, the input's capacity, one `partition_of` call per row and two
      worker runs. The `identity.rs` self-tests show that the content
      comparison accepts a reversed buffer and rejects a row that differs
      while hashing alike.
  ```

  **Dated notes** on the records that measured the radix family before B1.

  "Measured effect, earlier modes" (614–619), before (substring):
  ```text
    these numbers measures a post-#107 mode.)* On inputs that arrive grouped
  ```
  After:
  ```text
    these numbers measures a post-#107 mode. 2026-10-08, #101 B1: they also
    partitioned row ids and left the rows in arrival order, where today's
    `radix:K`, `parallel:N` and `presized:N` copy the rows into partition
    order first.)* On inputs that arrive grouped
  ```

  `#### Radix build A/B` (635–637): the `> **2026-10-07 (#107):**` note under it is word for
  word the one under `#### Radix build A/B, shuffled input`. So anchor on the heading, and
  insert after the note directly below `#### Radix build A/B` (line 637), separated by a blank
  line:
  ```markdown
  > **2026-10-08 (#101 B1):** These arms also partitioned row ids and left the rows in arrival order. Today's `radix:K` copies the rows into partition order (`morsel::partition_rows`) and keeps the copy as the trie's buffer, so each partition's build reads one contiguous range, and its `insertion` includes the copy. These numbers do not measure a post-B1 `radix:K` either.
  ```

  `#### Radix build A/B, shuffled input` (681–683): insert after the note directly below that
  heading (line 683), separated by a blank line:
  ```markdown
  > **2026-10-08 (#101 B1):** These arms also partitioned row ids and left the rows in arrival order. #101's locality run (2026-10-08, `kermit-bench-runs/hash-trie-locality-2026-10-08/`) located the order penalty this record found in reads of tuple memory. Today's `radix:K` copies the rows into partition order first, so the shuffled column is the case #101 B1 targets; these numbers do not measure it.
  ```

  Algorithm 2 A/B, part B (784–787). After the paragraph ending
  `` and `presized:16`, each at load factors 0.7 and 0.8. ``, insert a blank line and:
  ```markdown
  > **2026-10-08 (#101 B1):** The `presized:16` cells built over the input in arrival order. Today's `presized:N` copies its rows into partition order first, so those cells do not measure a post-B1 build; the `bulk` cells do.
  ```

**Chunk boundary (C07a ends here; C07b starts).** C07a edits doc comments and `hash-trie.md`
only, so the tree still compiles; it commits nothing. C07b continues in the same working tree
and makes Task 7's one commit (Step 8), staging C07a's files too.

- [ ] **Step 3: `docs/data-structures/parallel-build.md`.**

  The intro (6–13), before:
  ```markdown
  whichever thread is free takes the next unit of work. `parallel:N`
  produces exactly the trie the structure's single-threaded build produces
  (TreeTrie `serial`, HashTrie `bulk`) under the same config, so only the
  build-timing metrics (`insertion`, `end_to_end`) can move.
  HashTrie's presized build (`hash-trie=presized:N`, which requires
  `--ds-config root-capacity=tuples`) builds an equivalent trie whose root
  keys may sit in other buckets, so `iteration` is measured for it too
  (Amendment 2).
  ```
  After:
  ```markdown
  whichever thread is free takes the next unit of work.
  - **TreeTrie's `parallel:N`** produces exactly the trie its `serial` build
    produces, so only the build-timing metrics (`insertion`, `end_to_end`)
    can move.
  - **HashTrie's `parallel:N`** builds the `bulk` trie's contents and root,
    bucket for bucket, under the same config, over a buffer of its own. It
    copies the rows into partition order (#101), so its row ids differ and
    `iteration` is measured for it (Amendment 2).
  - **HashTrie's presized build** (`hash-trie=presized:N`, which requires
    `--ds-config root-capacity=tuples`) copies its rows the same way. It
    builds an equivalent trie whose root keys may also sit in other buckets,
    so `iteration` is measured for it too.
  ```

  The shared steps (31–33), before:
  ```markdown
  The shared steps live in `kermit-ds/src/morsel.rs` (`scatter_rows`, `dispatch`).
  `scatter_rows` sends the 4-byte row ids of a `Tuples` batch to partitions
  (`RowPartition`, its ids in input order); the batch itself never moves (#111).
  ```
  After:
  ```markdown
  The shared steps live in `kermit-ds/src/morsel.rs`:
  - `scatter_rows` sends the 4-byte row ids of a `Tuples` batch to partitions
    (`RowPartition`, its ids in input order), and the batch itself never
    moves (#111). TreeTrie's build uses it.
  - `partition_rows` copies the rows themselves into partition order, the
    paper's materialisation step (§3.3.2, #101). It returns a `Partitioned`:
    the copy, the partition offsets, and each copied row's input position
    (`origin`). HashTrie's builds use it.
  - `dispatch` runs one task per partition.

  ```
  (The lines after it, `The TreeTrie build is …` and `The HashTrie builds are …`, are
  unchanged.)

  The HashTrie list (150–159), before:
  ```markdown
  - `parallel:N` is the **exact build**: the radix build of #91
    ([`hash-trie.md`](./hash-trie.md#build-modes), `radix.rs`) with its first
    two steps on N threads, under every root capacity. It builds the bulk
    trie, bucket for bucket.
  ```
  After:
  ```markdown
  - `parallel:N` is the **exact build**: the radix build of #91
    ([`hash-trie.md`](./hash-trie.md#build-modes), `radix.rs`) with its first
    two steps on N threads, under every root capacity. It builds the bulk
    trie's contents and root, bucket for bucket, over the rows copied into
    partition order (#101).
  ```
  And before (158–159):
  ```markdown
    `root-capacity=tuples` (#88), since it fills that root, and it builds an
    equivalent trie.
  ```
  After:
  ```markdown
    `root-capacity=tuples` (#88), since it fills that root, and it builds an
    equivalent trie over the rows copied into partition order (#101).
  ```

  The exact build's pseudocode (163–179): the whole ```` ```text ```` block becomes:
  ```text
  parallel_build(tuples, N):
      check arities                                     // the serial check, on the caller
      if no tuples: return the empty root               // no worker started
      P = 4·N rounded up to a power of two; b = log₂ P
      copy = partition_rows(tuples, morsels of 16 384)  // step 1, N workers, two passes
          // row t goes to partition H(t[0]) >> (64 − b): a histogram pass keeps
          // each row's partition, then a scatter copies the rows into partition
          // order, with origin[id] = the copied row's input position
      free tuples                                       // the copy is the trie's buffer
      lists = dispatch(non-empty partitions' id ranges): // step 2, N workers
          group into a scratch root in id order (input order within a partition),
              noting (origin[id], hash) whenever it gains a key
          build each key's child from its list (Algorithm 2)
          take its entries out in that order
          -> [(first input position, hash, subtrie or chain)]
      root = empty                                       // step 3, the caller
      k-way merge the lists by first input position:
          insert each (hash, value) into root
  ```

  The three bullets after it (181–191), before:
  ```markdown
  - **Partition.** As for TreeTrie, but by the top b bits of the first
    attribute's hash (the radix build's rule; FxHash mixes its low bits
    poorly), so there are no splitters to sample. A row's id is its input
    position, so the partitions carry nothing else.
  - **Build.** A worker builds a whole partition into a scratch root of the
    real root's kind, by Algorithm 2, as the bulk build does, then moves the
    scratch root's entries out in the order their keys arrived.
  - **Merge.** The calling thread inserts every entry into the real root in
    the order its key first appeared in the input. Each list is already in
    that order, so a k-way merge over at most P lists suffices. (The radix
    build, single-threaded, sorts its entries instead.)
  ```
  After:
  ```markdown
  - **Partition.** By the top b bits of the first attribute's hash (the radix
    build's rule; FxHash mixes its low bits poorly), so there are no
    splitters to sample. Unlike TreeTrie's, this step moves the rows:
    `partition_rows` counts each morsel's rows per partition, then each
    worker copies its morsels' rows into disjoint ranges of one new buffer.
    Partition p is the contiguous id range `offsets[p]..offsets[p + 1]` of
    the copy, in input order, and `origin` maps each id back to its input
    position. The input batch is freed once copied.
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

  The invariant (195–211): from the paragraph beginning "Every `parallel:N` build is
  identical to the bulk build" through the bullet ending "because the partition is a function
  of the hash.", becomes:
  ```markdown
  Every `parallel:N` build holds the bulk build's contents for the same
  tuples, under every Layout, load factor, root capacity and child capacity:
  every table bucket for bucket and capacity for capacity, the root
  included, and every chain, singleton and pending list holding the same
  tuples in the same order. Only the row ids differ, since the trie's buffer
  is the partitioned copy, so the build is equivalent to `bulk`'s
  (Amendment 2), not array-identical. `parallel_builds_the_bulk_trie_*` in
  `parallel.rs` pins it with `identity::assert_same_contents`:
  - for N ∈ {1, 2, 3, 8};
  - with morsels of 7 tuples and of 16 384 (three morsels, two of them full,
    in `…_on_large_and_skewed_inputs`).

  `parallel_stores_its_partitioned_copy` pins the buffer itself.

  - A table's final layout depends only on the order in which its *new* keys
    arrive, because `HashTable::entry_or_insert_with` returns an existing
    entry before its resize check.
  - `partition_rows` keeps each partition in input order, so each root key's
    subtrie is built from the same tuples, in the same order, under
    partition-local ids. That covers every `Singleton`, every chain, every
    capacity and, under lazy expansion, every pending list.
  - The merge inserts the root's keys in first-appearance order (by
    `origin`), the bulk order. Equal 64-bit hashes share a root entry and
    also a partition, because the partition is a function of the hash.
  ```

  The exact build's complexity, and the presized build's below it. Both tables have this
  Partition row (lines 229 and 326), and both become the same: use a replace-all. Before:
  ```text
  | Partition | O(n) hashes and 4-byte id pushes | N workers |
  ```
  After:
  ```text
  | Partition | O(n) hashes, then O(n · a) value copies into one new buffer (a histogram pass, then a scatter) | N workers |
  ```
  Then lines 233–246, before:
  ```markdown
  The sequential share is the checks, the merge of per-morsel buckets
  ((n / 16 384) · P of them) and the merge into the root. It grows with D/n:
  when the first attribute is a key (D = n) every root insert happens on one
  thread, and a unary relation is the extreme, since its whole trie is the
  root. Thread starts are as for TreeTrie, 2·(N − 1) per build. Partition
  sizes follow the hash, so balance comes from P > N rather than from
  splitters, and a dominant first key still fills one partition.

  `parallel:1` against `bulk` is what partitioning costs on one thread, as
  `radix:2` measures it: #91 found the radix build slower than the per-tuple
  build (then called `serial`, now `incremental`) on inputs grouped by their
  first attribute, and faster only on shuffled ones. There is no sort saving
  to offset it, unlike TreeTrie's, so expect `parallel:1` to be slower than
  `bulk` on grouped inputs. #91's measurement predates `bulk`.
  ```
  After:
  ```markdown
  The sequential share is the checks, the prefix sums over the
  (n / 16 384) · P (partition, morsel) pairs, and the merge into the root.
  It grows with D/n: when the first attribute is a key (D = n) every root
  insert happens on one thread, and a unary relation is the extreme, since
  its whole trie is the root. Thread starts are 3·(N − 1) per build: the
  two passes of `partition_rows`, then `dispatch`. Partition sizes follow
  the hash, so balance comes from P > N rather than from splitters, and a
  dominant first key still fills one partition.

  Memory: while the copy is made, the input and the copy are both held, so
  the build's peak holds the tuples twice. `origin` adds 4 bytes per tuple
  until the build returns, and the partition numbers 4 bytes per tuple
  between the two passes.

  `parallel:1` against `bulk` is what partitioning costs on one thread, as
  `radix:2` measures it, the copy now included: #91 found the radix build
  slower than the per-tuple build (then called `serial`, now `incremental`)
  on inputs grouped by their first attribute, and faster only on shuffled
  ones. There is no sort saving to offset it, unlike TreeTrie's, so expect
  `parallel:1` to be slower than `bulk` on grouped inputs. #91's measurement
  predates both `bulk` and the copy (#101), which is meant to pay on
  shuffled inputs.
  ```

  The exact build's worked example (248–261): from `### Worked micro-example (exact build)`
  through `are the bulk root's.`, becomes:
  ```markdown
  ### Worked micro-example (exact build)

  `parallel:1` over `[3,1] [1,2] [2,9] [1,1]` (positions 0–3) has P = 4
  partitions, chosen by the top two bits of each first attribute's hash. Say
  those bits are `10` for 3 and 1, and `01` for 2. The copy holds partition 1
  first, then partition 2, each in input order; partitions 0 and 3 are empty:

  | Id in the copy | Tuple | `origin` | Partition |
  |---|---|---|---|
  | 0 | `[2,9]` | 2 | 1 (`01`) |
  | 1 | `[3,1]` | 0 | 2 (`10`) |
  | 2 | `[1,2]` | 1 | 2 |
  | 3 | `[1,1]` | 3 | 2 |

  So `offsets` is `[0, 0, 1, 4, 4]`, and partition 2 is ids 1–3.

  | Partition | Ids | Scratch root's keys, by arrival | Entries out (first input position, hash, value) |
  |---|---|---|---|
  | 1 (`01`) | 0 | 2 | (2, h(2), {9}) |
  | 2 (`10`) | 1, 2, 3 | 3, 1 | (0, h(3), {1}), (1, h(1), {2, 1}) |

  The merge takes input positions 0, 1 and 2 in turn, inserting 3, then 1,
  then 2: the order in which the bulk build first meets them, so the root's
  buckets are the bulk root's. Key 1's child names ids 2 and 3 where bulk's
  names rows 1 and 3: the same tuples, `[1,2]` and `[1,1]`, in the same
  order. Had the merge keyed on ids, it would have inserted 2 (id 0) first.
  ```

  The presized pseudocode (270–285): the whole ```` ```text ```` block becomes:
  ```text
  presized:N (requires root-capacity=tuples):
      check arities; no tuples → the empty root, no worker started
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
                  push_in_run(run k, id)           lines 6–7, onto its bucket's list
                      off the region's end → defer
      tail: sort the deferred ids by origin, push each by ordinary probing
      children: dispatch over the P runs: map a run's lists to children (`child`, lines 8–12)
  ```

  "Why it is correct", two bullets (304–319). Before:
  ```markdown
  - **Subtries are bulk's.** A key's tuples reach its bucket's list in input
    order, all inside its region or all in the tail, so its child is built
    from the list `bulk` builds it from: once a key is deferred it cannot be
    found in its region, so its later tuples are deferred too.
  ```
  After:
  ```markdown
  - **Subtries hold bulk's contents.** A key's tuples reach its bucket's
    list in input order, all inside its region or all in the tail, so its
    child is built from the tuples `bulk` builds it from, under the copy's
    ids: the partition keeps input order, and once a key is deferred it
    cannot be found in its region, so its later tuples are deferred too.
  ```
  Before:
  ```markdown
  - **The same for every N.** Regions have a fixed size and all of a
    region's tuples fall in one partition, in input order, so what is
    deferred does not depend on N, and the tail is sorted by position.
    `presized_parallel_builds_are_the_same_for_every_n_on_dense_roots`
    checks this where N changes how regions are grouped into partitions.
  ```
  After:
  ```markdown
  - **The same contents for every N.** Regions have a fixed size and all of
    a region's tuples fall in one partition, in input order, so what is
    deferred does not depend on N.
    - The tail is sorted by input position (`origin`), not by id.
    - Ids in the copy run partition by partition, and the partition count
      depends on N, so sorting by id would move tail keys between buckets as
      N changed.
    - Only the buffer, each N's own partitioned copy, differs.

    `presized_parallel_builds_are_the_same_for_every_n_on_dense_roots`
    checks this where N changes how regions are grouped into partitions.
    `presized_tail_goes_in_input_order_for_every_n` checks a tail whose
    order N would change.
  ```

  The presized complexity table's tail row (328), before:
  ```text
  | Tail | the deferred tuples, sorted and pushed | the calling thread |
  ```
  After:
  ```text
  | Tail | the deferred tuples, sorted by `origin` and pushed | the calling thread |
  ```

  The presized worked example (335–354): from the paragraph beginning "**Worked example.**
  `presized:2` over" through the line ending "later region's key compete for the same
  bucket.", becomes:
  ```markdown
  **Worked example.** `presized:2` over `[1,a] [2,b] [3,c] [1,d]` (rows
  0–3) under `root-capacity=tuples`: n = 4 at 70 % gives an 8-bucket root.
  Real regions are 4096 buckets; for the example, take 4-bucket regions, so
  there are 2 regions, 2 runs and 2 partitions. Say the keys' home buckets
  are 3 for key 1, 3 for key 2, and 5 for key 3:
  - partition 0 (homes 0–3) holds rows 0, 1 and 3;
  - partition 1 (homes 4–7) holds row 2.

  So the copy is `[1,a] [2,b] [1,d] [3,c]`, with `origin` `[0, 1, 3, 2]`.

  | Run | Tuple, id (`origin`) | Step | Bucket |
  |---|---|---|---|
  | 0 (buckets 0–3) | `[1,a]`, 0 (0) | key 1 is new: its list starts with id 0 | 3 |
  | 0 | `[2,b]`, 1 (1) | key 2 probes 3 (taken), and 4 is past its region's end | deferred |
  | 0 | `[1,d]`, 2 (3) | key 1 is found at 3: id 2 is pushed onto its list | 3 |
  | 1 (buckets 4–7) | `[3,c]`, 3 (2) | key 3 is new | 5 |

  The tail then pushes id 1 (`[2,b]`, input position 1) by ordinary probing
  from its home, bucket 3. Bucket 3 is taken and bucket 4 is free, so key 2
  lands at 4. The children are then built a run at a time: run 0's task
  builds key 1's from ids `[0, 2]` (`[1,a] [1,d]`), and run 1's builds key
  2's and key 3's.

  The bulk build would have put key 1 at 3, key 2 at 4 and key 3 at 5: the
  same occupied buckets, and here even the same slots. They differ only when
  a deferred key and a later region's key compete for the same bucket.
  ```

  The HashTrie scaling record (542–544). After the `> **2026-10-07 (#107):**` note directly
  below `### Scaling result: HashTrie, presized and grown (2026-10-06)` (line 544), insert a
  blank line and:
  ```markdown
  > **2026-10-08 (#101 B1):** These arms also partitioned row ids and left the rows in arrival order. Today's `parallel:N` and `presized:N` copy the rows into partition order first (`morsel::partition_rows`), which adds the copy to `insertion` and moves the rows `iteration` reads, so these numbers do not measure a post-B1 build either.
  ```

- [ ] **Step 4: `CLAUDE.md`.**

  Line 13 (Priorities item 1), before (substring):
  ```text
  (array-level where the structure fixes placement, as every landed mode but `presized:N` does; Amendment 2, 2026-10-06)
  ```
  After:
  ```text
  (array-level where a mode also fixes placement, as TreeTrie's, ColumnTrie's and HashTrie's `bulk` and `incremental` do; content-level for HashTrie's `radix:K`, `parallel:N` and `presized:N`, which store their rows in partition order, #101; Amendment 2, 2026-10-06)
  ```

  Line 148 (the `BuildModeRelation` bullet), four substrings. Before:
  ```text
  `radix:<bits>` = radix-partition on the first attribute's hash, group each partition into a scratch root and build its children by Algorithm 2, merge them in first-appearance order, #91;
  ```
  After:
  ```text
  `radix:<bits>` = copy the rows into partition order on the first attribute's hash (`morsel::partition_rows`, one thread, one morsel), group each partition into a scratch root and build its children by Algorithm 2, merge them in first-appearance order (by `origin`, each row's input position), #91, #101;
  ```
  Before:
  ```text
  `parallel:N` = the same steps with partition and build on N threads and a k-way merge, #94;
  ```
  After:
  ```text
  `parallel:N` = the same steps with the copy and the build on N threads and a k-way merge, #94;
  ```
  Before:
  ```text
  `presized:N` = a hash-partitioned fill of a presized root by regions, a deferred tail, then children built run by run
  ```
  After:
  ```text
  `presized:N` = the rows copied into partition order by home-bucket run, a fill of a presized root by regions, a deferred tail pushed in input order (by `origin`), then children built run by run
  ```
  Before:
  ```text
  Every landed mode but `presized:N` is array-identical.
  ```
  After:
  ```text
  HashTrie's `bulk` and `incremental` are array-identical. The radix family (`radix:K`, `parallel:N`, `presized:N`) keeps the partition-ordered copy as the trie's buffer (the paper's materialisation step, §3.3.2), so it is equivalent rather than array-identical: the same contents under other row ids, `radix:K`'s and `parallel:N`'s root bucket for bucket `bulk`'s; `presized:N` was already equivalent. Its tests compare with `identity::assert_same_contents`. TreeTrie's and ColumnTrie's modes are array-identical.
  ```

- [ ] **Step 5: `ARCHITECTURE.md`.**

  Line 218, before:
  ```text
      tuples: Tuples,         // every stored tuple, arrival order; len() is the multiset count
  ```
  After:
  ```text
      tuples: Tuples,         // every stored tuple, arrival or partition order (below); len() is the multiset count
  ```

  After line 231 (the `- **Leaves reach whole tuples.**` bullet), insert:
  ```markdown
  - **Construction.** `from_tuples` builds by the paper's Algorithm 2 (`bulk`, the default): each table's tuples are grouped into its buckets, then each bucket's child is built from its list. `bulk` and `incremental` keep the batch they are given as the buffer, in arrival order. The partitioning BuildModes (`radix:K`, `parallel:N`, `presized:N`) first copy the batch into partition order on the first attribute's hash, the paper's materialisation step (§3.3.2, #101). They do it through `morsel::partition_rows` and keep the copy, so each partition's rows are one contiguous range, and their tries hold the same contents under other row ids. See [`docs/data-structures/hash-trie.md`](docs/data-structures/hash-trie.md#build-modes).
  ```

  Line 234, before (substring):
  ```text
  is reported by its bench family instead, because the built trie is the same under every mode.
  ```
  After:
  ```text
  is reported by its bench family instead, because the built trie holds the same contents under every mode.
  ```

- [ ] **Step 6: Sweep.**
  ```bash
  rg -n "identical trie|identical one|is array-identical|bucket for bucket" \
    docs/data-structures/hash-trie.md docs/data-structures/parallel-build.md CLAUDE.md ARCHITECTURE.md \
    kermit-ds/src/ds/hash_trie kermit/src/execution.rs kermit/tests
  rg -n "arrival order" docs/data-structures/hash-trie.md ARCHITECTURE.md CLAUDE.md kermit-ds/src/ds/hash_trie
  rg -n "scatter_rows" docs/data-structures/hash-trie.md docs/data-structures/parallel-build.md
  rg -n "u16" kermit-ds/src/ds/hash_trie/build_mode.rs
  ```
  Expected:
  - **The first grep.** Every hit is true after B1. Each one:
    - compares `bulk` with `incremental` (hash-trie.md's "Against the per-tuple build", `implementation.rs` 236, 460 and 827, `join_tests.rs` 408);
    - is TreeTrie's or ColumnTrie's;
    - sits in a dated record;
    - or claims "bucket for bucket" only of tables or of a root, never of ids.
  - **The second grep.** Every hit names `bulk` or `incremental`, a dated record, or a test about a `bulk` trie.
  - **The third grep.** Hits appear only in parallel-build.md's shared-steps list and its TreeTrie sections.
  - **The fourth grep** prints nothing.

  Fix any other hit in the wording of this task.

  Read the rendered tables of `hash-trie.md` (the parity table, the Construction table, the
  Complexity row) and of `parallel-build.md` (the two worked examples) once: a stray `|`
  breaks a table silently.

- [ ] **Step 7: Lint and document** (four of the edited files are Rust).
  ```bash
  find /proc/[0-9]*/exe -maxdepth 0 -lname '*kermit-bench-runs*' 2>/dev/null | wc -l
  nix develop --command env RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all
  nix develop --command cargo fmt --all -- --check
  nix develop --command env RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc --workspace --no-deps
  ```
  Expected: `0`, then all clean. `execution.rs` is in the binary, whose docs only the Final
  gate's `cargo rustdoc --bin kermit` lints. `git diff --stat` lists exactly the eight files
  of this task.

- [ ] **Step 8: Commit.**
  ```bash
  git add kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/build_mode.rs \
    kermit/src/execution.rs kermit/tests/cli_hash_trie_build_mode.rs \
    docs/data-structures/hash-trie.md docs/data-structures/parallel-build.md CLAUDE.md ARCHITECTURE.md
  git commit -m "$(cat <<'EOF'
  docs: the radix family copies its rows into partition order (#101)

  hash-trie.md, parallel-build.md, CLAUDE.md and ARCHITECTURE.md describe
  #101 B1: radix:K, parallel:N and presized:N copy their rows into
  first-key-hash partition order through morsel::partition_rows and keep
  the copy as the trie's buffer (the paper's materialisation step,
  §3.3.2), so they are equivalent to bulk, not array-identical: the same
  contents under other row ids, radix:K's and parallel:N's root bucket for
  bucket bulk's. The parity rows close the partitioned-materialisation
  gap and leave the chain pointer to B2; one partitioning round is
  kermit's. Records measured before B1 carry dated notes, and the worked
  examples show the copy and origin. The HashTrie build-mode docs in
  implementation.rs, build_mode.rs, execution.rs and the CLI test name all
  three modes.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_015P4eTR34w48GpD7DvPPaee
  EOF
  )"
  ```

- [ ] **Step 9: The #105 board and the issues are not edited here.** They are outward-facing,
  so they wait for the user's word after landing. The exact texts are in "After this plan",
  Step 0.

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

  Expected: `0 failed`, and the second grep prints nothing. The passed count is a85c263's
  plus the 27 tests T1–T5 add:
  - T1: 13 (one in `tuples`, twelve in `morsel`);
  - T2: 10;
  - T3: 1;
  - T4: 1;
  - T5: 2.

  No task removes a test.

- [ ] **Step 3: Lints, format and docs.**
  ```bash
  nix develop --command env RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings
  nix develop --command cargo fmt --all -- --check
  nix develop --command env RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc --workspace --no-deps
  nix develop --command env CARGO_BUILD_JOBS=2 cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings
  ```
  Expected:
  - **clippy, fmt and `cargo doc`:** clean.
    - If `fmt --check` disagrees with CI later, suspect the flake's pinned nightly first
      (CLAUDE.md, "The flake's nightly must track CI's").
    - The fix is `nix flake update rust-overlay` and a re-run. Commit `flake.lock` separately,
      and only with the user's agreement.
  - **`cargo rustdoc --bin kermit`:** exactly the 2 errors known before this plan. The
    binary's private docs are never linted by CI, since the lib and the bin share the name
    `kermit`.
    - If in doubt, run the same command in a `git archive a85c263` copy under `$SCRATCH` and
      compare.
    - Any further error is this plan's: T7 edits two doc comments in `execution.rs`. Fix it,
      and commit the fix on its own (`docs(kermit): … (#101)`, with the two trailer lines).

- [ ] **Step 4: kermit-lab against a release binary.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo build --release -p kermit
  uv --directory python/kermit-lab sync --group test
  KERMIT_BIN=$WT/target/release/kermit uv --directory python/kermit-lab run pytest -q
  ```
  Expected: all pass, the real-binary contract test included. No report field changed
  (spec, "No schema bump"), so nothing in kermit-lab should move.

- [ ] **Step 5: Scope (Priority 6).**
  ```bash
  git diff a85c263 --stat -- kermit-ds/src/ds/tree_trie kermit-ds/src/ds/column_trie \
    kermit-algos kermit-parser kermit-bench kermit-rdf kermit-iters/src/hash_trie.rs \
    kermit-iters/src/leaf_rows.rs python
  git diff a85c263 --stat -- kermit/src
  git diff a85c263 -U0 -- kermit/src/execution.rs \
    | grep -E '^[+-]' | grep -vE '^(\+\+\+|---)' | grep -vE '^[+-][[:space:]]*///'
  git diff a85c263 -U0 -- kermit-ds/src/morsel.rs | grep -E '^-' | grep -v '^---'
  git diff a85c263 --stat
  ```
  Expected:
  - **TreeTrie, ColumnTrie, `kermit-algos` and the rest:** the first command prints nothing.
  - **The binary.** The second lists only `kermit/src/execution.rs`. The third prints nothing:
    every changed line there is a `///` doc comment (T7). So the binary's plumbing is
    untouched.
  - **`morsel.rs`.** The fourth prints only the lines T1 replaced: the module doc's list
    item, the "Neither result depends on scheduling" sentence and the `std::{…}` import. So
    `scatter_rows`, `RowPartition`, `dispatch` and `run_workers` are unchanged.
  - **Every file.** The last lists exactly the file map's files plus this plan:
    - `kermit-iters/src/tuples.rs`;
    - `kermit-ds/src/morsel.rs`;
    - in `kermit-ds/src/ds/hash_trie/`: `identity.rs`, `radix.rs`, `implementation.rs`,
      `parallel.rs` and `build_mode.rs`;
    - `kermit-ds/src/relation.rs`;
    - `kermit-ds/tests/hash_trie_tests.rs`;
    - in `kermit/tests/`: `join_tests.rs`, `tuple_allocation.rs` and
      `cli_hash_trie_build_mode.rs`;
    - `kermit/src/execution.rs`;
    - `docs/data-structures/hash-trie.md` and `docs/data-structures/parallel-build.md`;
    - `CLAUDE.md` and `ARCHITECTURE.md`.

- [ ] **Step 6: Report to the user.** Include:
  - the commit SHAs (T1–T7);
  - the gate's counts;
  - the mutation checks, each with its result: T1's three, T2's three, T3's M1 and M2, T4's
    and T5's;
  - the rustdoc error count;
  - anything done differently from this plan, with reasons.

**Landing is the user's call.** On their word only:
1. `git fetch origin`;
2. `git merge origin/master` (never rebase);
3. if anything came in, re-run Steps 1–4;
4. `git push origin HEAD:master`.

Never push without it.

---

## After this plan: the board, the issues, and the replicated measurement

Everything here is outward-facing or a timing run, so each step waits for the user's word.

- [ ] **Step 0: The #105 board and #101 (only on the user's word, after landing).** The
  spec's Acceptance 5 and Documentation list ask for the board update. The board's current
  row texts are not in the repo, so first show the user the rows as they stand and the
  replacements below, and edit only once they approve.
  ```bash
  gh issue view 105 --repo aidan-bailey/kermit --json body -q .body > $SCRATCH/105-before.md
  cp $SCRATCH/105-before.md $SCRATCH/105-after.md
  ```
  - Find the two rows in `$SCRATCH/105-after.md` and replace each with the text below,
    keeping the row's own markup (its table cells or checkbox).
  - Then `diff -u $SCRATCH/105-before.md $SCRATCH/105-after.md`, and show the diff to the
    user.

  The two rows:
  - **The Representation row on contiguous partitioned tuple storage** (#101, layer 3).
    New text:
    > Contiguous tuple storage, partitioned on the first join key's hash as it is materialised (§3.3.2): ◐ **materialisation done** (#101 B1, `<B1 landing SHA>`). `radix:K`, `parallel:N` and `presized:N` copy the tuples into partition order (`morsel::partition_rows`) and keep the copy as the trie's buffer; `bulk` (the default) and `incremental` keep arrival order. One partitioning round, not the paper's two passes (kermit's; #101 found no fan-out cost at this scale). Still open: the 8-byte chain pointer per tuple in place of a `Vec<RowId>` per chain (#101 B2).
  - **The Build row on the partitioned parallel build**, whose text includes "tuples stay in
    file order". New text:
    > Partitioned parallel build: the input partitioned by the first attribute's hash, morsel-driven (§3.3.2) ✓, the tuples themselves copied into contiguous partitions on N threads (#101 B1, `<B1 landing SHA>`) ✓; partitions as contiguous root regions filled in parallel, and the deferred tail, are kermit's.

  `<B1 landing SHA>` is `git -C $WT rev-parse --short HEAD` at the push; the user
  confirms it before the edit. On their word:
  ```bash
  gh issue edit 105 --repo aidan-bailey/kermit --body-file $SCRATCH/105-after.md
  gh issue comment 105 --repo aidan-bailey/kermit --body "#101 B1 (partitioned materialisation) landed at <B1 landing SHA>: radix:K, parallel:N and presized:N copy their rows into first-key-hash partition order and keep the copy as the trie's buffer (VLDB 2020 §3.3.2). Rows updated: contiguous partitioned storage (materialisation done; chain pointers are B2) and the parallel build (tuples no longer stay in file order). Spec: docs/specs/2026-10-08-hash-trie-partitioned-materialisation-design.md. The replicated measurement follows."
  gh issue comment 101 --repo aidan-bailey/kermit --body "Layer 3, first half (B1) landed at <B1 landing SHA>: the radix family copies its rows into partition order (morsel::partition_rows) and keeps the copy, so every read of a partition's rows is local. bulk and incremental keep arrival order, so one binary measures the effect; the replicated A/B (Parts A–C of the spec's Measurement) is next. B2 (chain links) is designed after it."
  ```
  The user may edit any of these texts. Use their version, and post nothing they have not
  seen.

**The measurement.** The spec's § Measurement, in the #107 style:
- after landing, and only once the user says to run it;
- in its own run directory, `RUN=/tb/Source/Academia/kermit-bench-runs/hash-trie-partitioned-<date>/`, with a README written before the run that states the predictions below.

**Binaries.**
- **B1** is the landing SHA. **Part C** also uses 89771a9 (before #111) and d746b97 (#111).
- **The build, per SHA:**
  - `git archive <sha>` into `$RUN/src/<sha>`;
  - `Cargo.lock` is gitignored, so `git archive` leaves it out. Copy the worktree's into
    each tree, as the locality run did, and record its sha256;
  - build in its own `CARGO_TARGET_DIR` (`$RUN/target-<sha>`) through the worktree flake,
    so all use one toolchain (default features: jemalloc);
  - copy the binary to `$RUN/bin/kermit-<sha>` and record its sha256 in `$RUN/env.txt`.
- **The sha256s must all differ.** A shared target dir once copied one binary out twice:
  `git archive` dates files at the commit time, so cargo saw the second tree as up to date.

**Precautions.** Copy `run.sh`, `chain.sh` and `host_sampler.sh` from the
`hash-trie-algorithm-2-2026-10-07` and `hash-trie-locality-2026-10-08` runs.
- **Before every step:** `lib.sh`'s quiet gate
  (`/tb/Source/Academia/kermit-bench-runs/build-2026-10-05/lib.sh`, at most 1.5 busy cores
  over 10 s), then a memory guard sized from a probed peak. The partitioning modes hold the
  tuples twice while they copy, so probe them, not `bulk`.
- **The host sampler**, run-local, logs to `logs/host-samples.tsv` until the chain prints
  `CHAIN_EXIT=`. Rows that overlap another session's work are left out and re-run.
- **Replicates:** 5, outermost, with the arm order rotated per replicate (and the binary
  order alternating in Part C).
- **A distinct `--name` per step** (#98: Criterion overwrites a group shared across runs),
  and a `--report-json` per step.
- **Criterion output:** `KERMIT_WORKSPACE=$RUN`, so it lands in `$RUN/target/criterion`.
- **Inputs, read-only:**
  - `friendof` and `price` from `~/.cache/kermit/benchmarks/watdiv-stress-100-test-1/`.
    Read them, never write there: no `--force`, `bench clean` or `bench gen` under
    `~/.cache/kermit`.
  - The file-shuffled `friendof` of `../radix-shuffle-2026-10-05/data/`.
  - `binary-1e7.csv` from `../tree-trie-scaling-2026-10-05/data/`, in file order.
  - A stable sort of it on the first column, written once to `$RUN/data/binary-1e7-sorted.csv`:
    `(head -1 F; tail -n +2 F | sort -t, -k1,1n -s)`.
  - `oxford-uniform-s3`'s YAML, copied into `$RUN/benchmarks`.
- **Report schemas.** 89771a9 writes schema-3 reports and the others schema 4, so Part C's
  analysis passes `allow_mixed_schema=True` deliberately and compares like with like only.

**Parts.**
- **A, the effect within one binary (B1).** `bench ds -m insertion iteration space`, with
  `--sample-size 10 --measurement-time 3 --warm-up-time 1`.
  - **Modes:** `bulk`, `radix:12`, `radix:16` and `parallel:8` under the default config.
  - **The presized cell:** `presized:8` with `--ds-config root-capacity=tuples`, beside a
    `bulk` arm under the same config, so its `space` is compared like with like.
  - **Inputs:** `friendof` grouped and file-shuffled, `price`, and `binary-1e7` sorted and
    in file order.
  - **`space`** must be identical across the modes of one config (Acceptance 3). Any
    difference is a bug, not a result.
- **B, the #101 locality harness re-run with `radix:12`.**
  - **The harness.** Use the `hash-trie-locality-2026-10-08` harness, pointed at a B1 source
    tree (a copy of `harness/` whose `kermit-ds` path names `src/<B1>`). Run its E1 block,
    `friendof` × cells GG/SS/GS/SG, for `bulk` and `radix:12`.
  - **The spec's prediction:** `radix:12`'s GS and SS cells fall to about GG plus the copy,
    because the copy undoes any allocation order. This answers #101's open question with the
    paper's mechanism in place.
  - **Note for the run.** The harness passes `Vec<Vec<usize>>` into the timed constructor.
    On a post-#111 tree, the conversion into one `Tuples` batch happens inside the timed
    build, and it is the only step that reads the allocation order. GS therefore keeps that
    conversion's random reads under every mode.
    - Time the conversion alone per cell, untimed for the build, so the copy's effect is
      attributed separately.
    - Say so in the README before the run.
- **C, combined with #111's own A/B**, which has not yet run.
  - **Binaries:** 89771a9 (before #111), d746b97 (#111) and B1.
  - **Workloads:** #111's plan's Parts A and C, run on all three in one chain, so one run
    records both changes' effects.
    - A: `bench ds -m insertion space iteration` on the 12 relations of the 2026-10-06 runs
      and the shuffled `friendof`, HashTrie `bulk`, `radix:12`, `parallel:16` and
      `presized:16` (with `root-capacity=tuples`).
    - C: `bench run oxford-uniform-s3 -q triangle -m insertion iteration space`, HashTrie
      pruning {off, on} × expansion {eager, lazy}, with TreeTrie/LFTJ as the control.
  - **Comparisons:** d746b97 against 89771a9 is #111's effect, and B1 against d746b97 is
    B1's. `bulk`'s B1-over-d746b97 ratio, like TreeTrie's, is the codegen bound: B1 does not
    change `bulk`.

**Expected** (the spec's, written into the README before the run):
- on shuffled input, `radix:12` falls from its 2.13× order penalty to near its grouped time
  plus the copy, and beats `bulk`, which pays 2.32×;
- on grouped input, the copy makes the radix family slightly slower than `bulk`;
- `space` is unchanged by B1 in every mode;
- `bulk` and `incremental` are unchanged by B1 within the codegen bound.

**Afterwards.** Record the results as dated records in `docs/data-structures/hash-trie.md` and
`parallel-build.md`, and post them to #101 and #105, the texts shown to the user first. Making
a radix-family mode the default is decided from these numbers (spec, Out of scope), not
before. B2 (chain links) is brainstormed after this measurement.

---

## Merge notes

This plan was assembled from three drafted parts (T1–T2, T3 and T6, T4–T5), a
reconciliation list and the coordinator's decisions. Where they disagreed, RECONCILE won,
then the spec, then the code, checked at a85c263. These choices were made:

1. **`radix:K` passes one morsel** (RECONCILE 10(a)).
   - T3's draft passed `MORSEL_TUPLES`. Its `fill_root` now passes `tuples.len().max(1)`,
     with a comment saying why (about 40 M (partition, morsel) pairs at `radix:16` over 10⁷
     rows).
   - The pin test computes its expectation the same way, and `MORSEL_TUPLES` left
     `radix.rs`' imports.
   - The T3 preamble, Step 2's expected warnings, the commit message and T7's `hash-trie.md`
     radix section say so.
2. **`partition_rows` hashes each row once** (RECONCILE 10(b)). Rewritten in T1:
   - the histogram keeps a per-morsel `Vec<u32>`, `with_capacity` of the morsel's rows;
   - the scatter zips the morsel's ids with it instead of calling `partition_of`;
   - an assertion bounds `partitions` by `u32::MAX`, so the `as u32` cannot truncate;
   - the doc, the borrow-check notes and the intro say so.

   A new test (`partition_rows_calls_partition_of_once_per_row`) and a third T1 mutant (the
   scatter hashing again) pin it. T6's recount gained the list (one allocation per morsel,
   one morsel per cell at both sizes), and the tightened bounds 777 and 770 stand.
3. **The zero-fill** (RECONCILE 10(c)) is kept and named in `partition_rows`' doc as a known
   cost.
4. **A worker-run test in T1.** T3's draft cited "T1's own worker-run test", which T1 lacked.
   `partition_rows_starts_workers_twice` now pins two `run_workers` calls, an empty batch
   included (RECONCILE 2). T1 Step 8 also greps for the four call sites.
   - The pins `[1, 1]` (T3), `[N, N, N]` (T4) and `[3, 3, 3, 3]` (T5) follow from it.
   - T1 now adds 12 morsel tests: 21 pass in Step 8.
5. **T3 Step 0's premise check** grepped 40 lines after `fn assert_same_contents` for
   `hash_at` / `assert_same_table`.
   - T2's `assert_same_contents` delegates to `assert_same_node_contents`, which calls
     `assert_same_table`, so that window would have printed nothing and stopped the executor.
   - The step now follows the chain in three greps.
6. **Doc edits moved from T3 to T7** (RECONCILE 8):
   - `build_mode.rs`' `Radix` and `RadixBits` docs, which T7 rewrites with the enum's,
     `Parallel`'s and `Presized`'s;
   - `execution.rs` 1846–1848 and 1863–1865;
   - the CLI test's module doc.

   T3's `identity.rs` module-doc edit is dropped: T2 rewrote that doc, and its text stays
   true. T3's file list and `git add` lost both files, and T3 Step 5f now names T1's three
   markers exactly.
7. **T6 Step 2 lost its duplicates** (RECONCILE 4). T1, T4 and T5 rewrite their sites:
   - the parallel comments in `hash_trie_tests.rs` and `join_tests.rs`;
   - the `parallel.rs` comments;
   - `morsel.rs`' module doc.

   T6 keeps `relation.rs:944` and lists the others to verify. Its sweep's item 6 says the
   same.
8. **`CountingHash`'s doc** keeps "once more" (T3's handed-over text said "more often"):
   with 10(b), `radix:K` still hashes each first attribute exactly once more than `bulk`.
9. **T7 covers more than the spec's list**, because each of these is false after B1
   otherwise:
   - `CLAUDE.md` Priorities item 1's "every landed mode but `presized:N`" parenthetical;
   - `implementation.rs`' `from_tuples_with_config_and_build_mode` doc;
   - `build_mode.rs`' enum, `Parallel` and `Presized` docs;
   - `ARCHITECTURE.md` line 234's "the built trie is the same under every mode";
   - `hash-trie.md` lines 12, 97 and 123 and the child-capacity record's `presized:16`
     bullet.

   ARCHITECTURE.md had no HashTrie construction summary, so T7 adds a Construction bullet.
10. **The #105 board** went to "After this plan", Step 0. The current row texts are not in
    the repo, so the step shows the user the rows before and after, and runs `gh issue edit`
    / `gh issue comment` only on their word, as the #107 and presized plans did at landing.
11. **Commands normalised.** T4's and T5's drafts set `RUSTFLAGS` / `RUSTDOCFLAGS` outside
    `nix develop`; every command now sets them inside its `env`, after the flake's shell
    hook. The drafts otherwise stay verbatim, comments and line references included.
12. **Measurement, Part B.** Under #111 the locality harness's `Vec<Vec<usize>>` converts
    inside the timed constructor, so a cell's allocation order reaches only that
    conversion. The step asks for the conversion to be timed alone, so the copy's effect is
    attributed.
13. **Chunks.** Tasks 3 and 7 run over ~900 lines, so each is two chunks.
    - C03a ends with a compiling tree and the pin failing by design.
    - C07a ends with doc-only edits.
    - Each second chunk makes its task's one commit.
