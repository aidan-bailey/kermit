# TreeTrie Parallel Build (Plan 1 of #94) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `TreeTrie` a `parallel:N` build mode that builds exactly the
serial trie on N threads. `--ds-build parallel:N` selects it, every TreeTrie
report records it as `ds_build_mode`, and a kermit-lab `speedup` preset turns
a set of reports into speedup, efficiency and Karp–Flatt numbers.

**Architecture:** There are four work packages; see "Work packages".

- **P1** (Tasks 1–3), all in `kermit-ds`:
  - `kermit-ds/src/morsel.rs` holds two structure-agnostic steps on
    `std::thread::scope`: `scatter`, the partition step, and `dispatch`, the
    build step.
  - `TreeTrieBuildMode` is the mode type.
  - `TreeTrie::build_parallel` partitions by first-key range, sorts and
    inserts per partition, then pushes the nodes onto the root in key order.
    Array-level tests prove it identical to the serial build.
- **P2** (Tasks 4–6), the `kermit` binary:
  - the TreeTrie cell carries and reports its build mode;
  - `--ds-build` parses `bulk | incremental | serial | parallel:N`;
  - the join suites run the new mode.
- **P3** (Tasks 7–8): kermit-lab (the `serial` back-fill, a `threads` column,
  `speedup_table` and the `speedup` preset with its CLI subcommand) and the
  docs.
- **Controller** (Tasks 0, 9, 10): the merge, the gates, a smoke run, the
  checkpoints, and landing (only on the user's instruction).

HashTrie is plan 2 of #94. Nothing in this plan touches it.

**Tech Stack:** Rust nightly workspace (std threads, clap, Criterion,
`serde_json`), Python kermit-lab (pandas, scipy, matplotlib, pytest via uv),
Nix dev shell.

**Spec:** [`docs/specs/2026-10-05-parallel-build-design.md`](../../specs/2026-10-05-parallel-build-design.md) · **Issue:** #94

---

## Preconditions

- **The spec is approved** (2026-10-05). This plan is its delivery step
  "Plan 1". Plan 2 (HashTrie) and the scaling measurement follow separately.
- **Plan 2 will change some behaviour pinned here.**
  - `serial` and `parallel:N` belong to TreeTrie alone, so
    `-i hash-trie --ds-build parallel:2` is rejected.
  - HashTrie reports carry no `ds_build_mode`.
  - Tests here pin both, with a comment naming plan 2, which updates them.
- **No measurement claims.** Task 9's smoke run checks that the parallel
  build is faster at all. Its numbers go nowhere but the checkpoint report.

## Ground rules for every task

- **Paths.**
  - The worktree is
    `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/upgrades_18db947ab2b6aa1c`.
  - `SCRATCH` is the executing session's scratchpad directory.
  - **Never `cd`** in a Bash call, because it moves the session. Use absolute
    paths, `env -C <dir>`, `git -C $WT` or `uv --directory`.
- **Cargo.** Run it **in the foreground** through the flake with
  `CARGO_BUILD_JOBS=2`, e.g. `CARGO_BUILD_JOBS=2 nix develop $WT --command
  cargo test -p kermit-ds`. A background memory monitor kills
  `run_in_background` cargo jobs. Anything longer than 10 minutes runs fully
  detached (`setsid nohup … & disown`).
- **Formatting.** Format **only** with `nix develop $WT --command cargo fmt
  --all`, before every commit; stable rustfmt rewrites dozens of files. If
  CI's fmt later disagrees, report it: the fix (`nix flake update
  rust-overlay`) changes `flake.lock` and is the user's call.
- **Doc comments.** Backtick every identifier.
- **Python.** Run kermit-lab with `uv --directory $WT/python/kermit-lab run
  pytest -q`. The first run needs `uv --directory $WT/python/kermit-lab sync
  --group test`.
- **Commits.**
  - Plain conventional commits referencing `(#94)`. Never amend, never push.
  - Stage files by name; new files are marked intent-to-add.
  - Every commit message ends with these two lines. An executing session
    substitutes its own `Claude-Session` line.
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01EC25BmBrMyFPmp424HLMDd
    ```
- **Mutation checks.**
  1. Commit first.
  2. Apply the mutant with an exact edit.
  3. Confirm the named test fails **and** that the mutant applied:
     `git -C $WT diff` shows it.
  4. Revert by reversing the exact edit, never with `git checkout`.
  5. Re-run the test and see it pass. `git -C $WT status` must then be clean.
- **Scope (Priority 6).**
  - Do **not** change HashTrie, ColumnTrie's build, any algorithm or any
    optimiser.
  - The serial bodies of `TreeTrie::from_tuples`, `TreeTrie::insert` and
    `insert_into_children` stay byte-identical. Task 9 checks this with a diff.
- **Shared bench cache.** Never write under `~/.cache/kermit` (no `--force`,
  `bench gen` or `bench clean`). Nothing in this plan needs a generated
  benchmark.
- **Checkpoints with the user:**
  1. after P1: identity holds and Miri is clean;
  2. after Task 9: the gate is green and the smoke numbers are in;
  3. before any merge or push.

---

## Work packages

One implementer agent executes each package end to end: its tasks in
order, one commit per task, mutation checks included. The package is then
reviewed before the next starts. The controller keeps Tasks 0, 9 and 10.

| Package | Tasks | Scope | Depends on | Commits | Done when |
|---|---|---|---|---|---|
| *Controller* | 0 | Merge `origin/master`, record `BASE`, green baseline | — | merge only | `BASE` recorded; workspace tests and kermit-lab pytest green |
| **P1 — `kermit-ds`** | 1, 2, 3 | `morsel.rs`; `TreeTrieBuildMode`; the parallel build, its identity tests and the structure-level suites | Task 0 | 3 | `cargo test -p kermit-ds` green and warning-free; new tests Miri-clean; mutation checks recorded |
| **P2 — `kermit` binary** | 4, 5, 6 | the TreeTrie cell's build mode; `--ds-build` values; join suites | P1 | 3 | `cargo test -p kermit` green, including `cli_tree_trie_build_mode` and the 48 new join tests; mutation checks recorded |
| **P3 — analysis and docs** | 7, 8 | kermit-lab; every doc in Task 8 | P2 | 2 | kermit-lab pytest green with `KERMIT_BIN`; `cargo doc` clean |
| *Controller* | 9, 10 | final gate, smoke run, checkpoint 2; landing only on the user's instruction | P3 | — | — |

Each implementer receives this plan, its package's task numbers, the ground
rules and a report budget of about 40 lines. The report covers commit SHAs,
test counts, mutation-check outcomes, and any deviation from the plan with
its reason. An implementer must not start the next package, push or merge.

---

## Task 0 [controller]: Catch up and record the baseline

**Files:** none modified, apart from the merge commit.

- [ ] **Step 1: Merge `origin/master` into the branch (never rebase)**

```bash
git -C $WT fetch origin
git -C $WT merge origin/master
```

Expected: a merge or a fast-forward. The branch holds only the spec
(10d13ac, 5afed87) and this plan, so conflicts are not expected.

- [ ] **Step 2: Record the baseline commit**

```bash
git -C $WT rev-parse HEAD | tee $SCRATCH/base.sha
```

- [ ] **Step 3: Green baseline**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}'
uv --directory $WT/python/kermit-lab sync --group test
uv --directory $WT/python/kermit-lab run pytest -q 2>&1 | tail -1
```

Expected: 0 failed, and pytest all passed (the contract tests skip without
`KERMIT_BIN`). Put both counts in the P1 brief, so later deltas can be
explained.

---

# P1 — `kermit-ds`

> **As executed (2026-10-05).** P1 landed as 93a9e55, 352069e and 6252323,
> and then its quality review's fixes as 2913e8c. Those fixes changed P1's
> code from the text below in seven ways:
> - the splitter sample keeps its duplicates and takes 128 keys per
>   partition, so the splitters share out tuples rather than distinct keys;
> - the parallel arm sorts with the serial build's hand-rolled comparator;
> - the test records are now `(threads, partitions)` per build and each
>   `run_workers` thread count;
> - a rendezvous test covers a helper thread's panic;
> - partitions are collected pre-sized;
> - the morsel-size comment is reworded;
> - `Threads` is bounded by `Threads::MAX = 1024`.
>
> The spec records the design changes. P2 and P3 below are written against
> the code as it now stands.

## Task 1 [P1]: The morsel-driven partition and build steps

**Files:**
- Create: `kermit-ds/src/morsel.rs`
- Modify: `kermit-ds/src/lib.rs` (the module, and the `Threads` re-export)

- [ ] **Step 1: Write the failing tests**

Create `kermit-ds/src/morsel.rs` containing only the tests. The
implementation goes above them in Step 3.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn threads(n: usize) -> Threads { Threads::new(n).expect("tests use a nonzero count") }

    #[test]
    fn threads_rejects_zero() {
        assert_eq!(Threads::new(0), None);
        assert_eq!(threads(3).get(), 3);
    }

    /// Every tuple lands exactly once, in the partition `partition_of`
    /// names, and each partition lists its tuples in input order with their
    /// input positions — whatever the thread count, and however the
    /// morsels divide the input.
    #[test]
    fn scatter_keeps_every_tuple_once_in_input_order() {
        let n = if cfg!(miri) { 23 } else { 1000 };
        let input: Vec<Vec<usize>> = (0..n).map(|i| vec![i % 7, i]).collect();
        let thread_counts: &[usize] = if cfg!(miri) { &[1, 3] } else { &[1, 2, 3, 8] };
        let morsel_sizes = if cfg!(miri) {
            vec![1, 5, n + 1]
        } else {
            vec![1, 4, 5, n, n + 1]
        };
        for &t in thread_counts {
            for &morsel in &morsel_sizes {
                let case = format!("threads {t}, morsel {morsel}");
                let partitions = scatter(threads(t), input.clone(), morsel, 3, |tuple| tuple[0] % 3);
                assert_eq!(partitions.len(), 3, "{case}");
                let mut seen = 0;
                for (p, partition) in partitions.into_iter().enumerate() {
                    let tuples: Vec<Positioned> = partition.into_tuples().collect();
                    assert!(
                        tuples.windows(2).all(|pair| pair[0].0 < pair[1].0),
                        "{case}: partition {p} is out of input order"
                    );
                    for (position, tuple) in &tuples {
                        assert_eq!(tuple, &input[*position], "{case}: position {position}");
                        assert_eq!(tuple[0] % 3, p, "{case}: tuple {tuple:?} in partition {p}");
                    }
                    seen += tuples.len();
                }
                assert_eq!(seen, n, "{case}: every tuple exactly once");
            }
        }
    }

    /// Tuples are moved, not copied: each keeps its heap buffer, and so its
    /// capacity, which a trie that stores tuples (HashTrie, plan 2) counts in
    /// `heap_size_bytes`.
    #[test]
    fn scatter_moves_tuples_without_reallocating() {
        let input: Vec<Vec<usize>> = (0..10)
            .map(|i| {
                let mut tuple = Vec::with_capacity(9);
                tuple.extend([i, i]);
                tuple
            })
            .collect();
        let buffers: Vec<*const usize> = input.iter().map(|tuple| tuple.as_ptr()).collect();
        for partition in scatter(threads(2), input, 3, 2, |tuple| tuple[0] % 2) {
            for (position, tuple) in partition.into_tuples() {
                assert_eq!(tuple.capacity(), 9, "position {position}");
                assert_eq!(tuple.as_ptr(), buffers[position], "position {position}");
            }
        }
    }

    #[test]
    fn dispatch_returns_results_in_item_order() {
        let n = if cfg!(miri) { 9 } else { 200 };
        for t in [1, 2, 3, 8] {
            let items: Vec<usize> = (0..n).collect();
            let expected: Vec<usize> = (0..n).map(|i| i * i).collect();
            assert_eq!(dispatch(threads(t), items, |i| i * i), expected, "threads {t}");
        }
    }

    /// The workers really run at once: the first task can finish only after
    /// another worker has started the second. A single worker would wait
    /// out the timeout and return `false`.
    #[test]
    #[cfg_attr(miri, ignore = "waits on a wall-clock timeout")]
    fn dispatch_runs_tasks_on_more_than_one_thread() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let sender = Mutex::new(sender);
        let receiver = Mutex::new(receiver);
        let met = dispatch(threads(2), vec![0, 1], |i: usize| {
            if i == 0 {
                receiver
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .is_ok()
            } else {
                sender.lock().unwrap().send(()).unwrap();
                true
            }
        });
        assert_eq!(met, vec![true, true]);
    }

    #[test]
    fn empty_input_needs_no_work() {
        assert!(dispatch(threads(4), Vec::<usize>::new(), |i| i).is_empty());
        let partitions = scatter(threads(4), Vec::new(), 8, 3, |_| 0);
        assert_eq!(partitions.len(), 3);
        assert!(partitions
            .into_iter()
            .all(|partition| partition.into_tuples().next().is_none()));
    }

    #[test]
    #[should_panic(expected = "task failed")]
    fn a_workers_panic_reaches_the_caller() {
        let _results: Vec<usize> = dispatch(threads(3), (0..30).collect::<Vec<usize>>(), |i| {
            if i == 17 {
                panic!("task failed");
            }
            i
        });
    }
}
```

Register the module in `kermit-ds/src/lib.rs` by adding `mod morsel;` after
`mod heap_size;`:

```rust
mod heap_size;
mod morsel;
mod relation;
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds morsel
```

Expected: compilation fails with `cannot find type 'Threads'` and `cannot
find function 'scatter'` (and `dispatch`).

- [ ] **Step 3: Implement**

Insert this above the `#[cfg(test)] mod tests` in `kermit-ds/src/morsel.rs`:

```rust
//! Morsel-driven work distribution for the parallel builds (Leis et al.,
//! *Morsel-Driven Parallelism*, SIGMOD 2014).
//!
//! The input is cut into small fixed-size *morsels*. The *dispatcher*, a
//! mutex-guarded queue, hands the next unit of work to whichever worker is
//! free, so a slow worker never stalls the rest. A build uses it twice:
//!
//! 1. [`scatter`] moves tuples into partitions, one morsel at a time;
//! 2. [`dispatch`] then runs one task per partition.
//!
//! Neither result depends on scheduling. A partition lists its tuples in
//! input order, and task results come back in task order, so a parallel
//! build can reproduce its serial counterpart exactly
//! (`docs/data-structures/parallel-build.md`).
//!
//! The workers are [`std::thread::scope`] threads, the calling thread among
//! them. Every worker is joined before a call returns, and a worker's panic
//! is re-raised in the caller.

use std::{num::NonZeroUsize, panic, sync::Mutex, thread};

/// How many threads a parallel build uses, the calling thread included.
/// Zero threads cannot be represented.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Threads(NonZeroUsize);

impl Threads {
    /// `n` threads, or `None` when `n` is zero.
    pub fn new(n: usize) -> Option<Self> { NonZeroUsize::new(n).map(Self) }

    /// The number of threads.
    pub fn get(self) -> usize { self.0.get() }
}

/// Tuples per morsel in the parallel builds. A worker takes one morsel per
/// lock of the shared queue, so a morsel must hold enough work to make that
/// lock negligible. Leis et al. use morsels of about 100 000 tuples; these
/// are smaller because partitioning one tuple is cheap.
pub(crate) const MORSEL_TUPLES: usize = 16_384;

/// Partitions per thread in the parallel builds. More partitions than
/// threads let [`dispatch`] balance partitions of uneven size.
pub(crate) const PARTITIONS_PER_THREAD: usize = 4;

/// A tuple and its position in the input.
pub(crate) type Positioned = (usize, Vec<usize>);

/// One morsel's tuples, split by partition.
type Buckets = Vec<Vec<Positioned>>;

/// The tuples [`scatter`] sent to one partition: one segment per morsel that
/// contributed any, in morsel order. Reading the segments in order reads the
/// partition in input order.
#[derive(Debug, Default)]
pub(crate) struct Partition {
    segments: Vec<Vec<Positioned>>,
}

impl Partition {
    /// The partition's tuples in input order, each with its input position.
    pub(crate) fn into_tuples(self) -> impl Iterator<Item = Positioned> {
        self.segments.into_iter().flatten()
    }
}

/// The queue's next item. The lock is held only while taking it — a guard
/// in a `while let` condition would live through the loop body and
/// serialise the workers.
fn take_next<I: Iterator>(queue: &Mutex<I>) -> Option<I::Item> {
    queue.lock().unwrap().next()
}

/// Runs `work` on `threads` workers, the calling thread and `threads − 1`
/// scoped threads, and returns every worker's result once all have
/// finished. A worker's panic is re-raised here with its original payload.
fn run_workers<R: Send>(threads: Threads, work: impl Fn() -> R + Sync) -> Vec<R> {
    thread::scope(|scope| {
        let helpers: Vec<_> = (1..threads.get()).map(|_| scope.spawn(&work)).collect();
        let mut results = vec![work()];
        for helper in helpers {
            match helper.join() {
                | Ok(result) => results.push(result),
                | Err(payload) => panic::resume_unwind(payload),
            }
        }
        results
    })
}

/// Moves every tuple of `tuples` into partition `partition_of(tuple)`, which
/// must be below `partitions`.
///
/// `threads` workers take morsels of `morsel_tuples` tuples from a shared
/// queue, so a worker that finishes early takes more. Every partition lists
/// its tuples in input order, whatever order the morsels ran in. Tuples are
/// moved, not copied: each keeps its heap buffer and its capacity.
///
/// # Panics
///
/// Panics if `morsel_tuples` is zero, or if `partition_of` returns
/// `partitions` or more, or panics itself.
pub(crate) fn scatter(
    threads: Threads, mut tuples: Vec<Vec<usize>>, morsel_tuples: usize, partitions: usize,
    partition_of: impl Fn(&[usize]) -> usize + Sync,
) -> Vec<Partition> {
    let queue = Mutex::new(tuples.chunks_mut(morsel_tuples).enumerate());
    let mut morsels: Vec<(usize, Buckets)> = run_workers(threads, || {
        let mut scattered = Vec::new();
        while let Some((index, morsel)) = take_next(&queue) {
            let first = index * morsel_tuples;
            let mut buckets: Buckets = (0..partitions).map(|_| Vec::new()).collect();
            for (offset, slot) in morsel.iter_mut().enumerate() {
                let tuple = std::mem::take(slot);
                let partition = partition_of(&tuple);
                buckets[partition].push((first + offset, tuple));
            }
            scattered.push((index, buckets));
        }
        scattered
    })
    .into_iter()
    .flatten()
    .collect();
    morsels.sort_unstable_by_key(|&(index, _)| index);

    let mut out: Vec<Partition> = (0..partitions).map(|_| Partition::default()).collect();
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
/// they are free.
///
/// # Panics
///
/// Re-raises a panicking `task`'s panic once every worker has stopped.
pub(crate) fn dispatch<T: Send, R: Send>(
    threads: Threads, items: Vec<T>, task: impl Fn(T) -> R + Sync,
) -> Vec<R> {
    let len = items.len();
    let queue = Mutex::new(items.into_iter().enumerate());
    let done = run_workers(threads, || {
        let mut done = Vec::new();
        while let Some((index, item)) = take_next(&queue) {
            done.push((index, task(item)));
        }
        done
    });
    let mut results: Vec<Option<R>> = (0..len).map(|_| None).collect();
    for (index, result) in done.into_iter().flatten() {
        results[index] = Some(result);
    }
    results
        .into_iter()
        .map(|result| result.expect("the queue hands out every item exactly once"))
        .collect()
}
```

Then re-export `Threads` from `kermit-ds/src/lib.rs`. In its `pub use { … }`
block, add `morsel::Threads,` right after `heap_size::HeapSize,`:

```rust
    heap_size::HeapSize,
    morsel::Threads,
    relation::{
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds morsel
```

Expected: 7 passed. The library build warns `dead_code` for `scatter`,
`dispatch`, `Partition`, `MORSEL_TUPLES` and `PARTITIONS_PER_THREAD`,
because nothing but the tests uses them until Task 3. That is expected; do
not silence it.

- [ ] **Step 5: Format, commit, then run two mutation checks**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit-ds/src/morsel.rs kermit-ds/src/lib.rs
git -C $WT commit -F - <<'EOF'
feat(ds): morsel-driven partition and build steps (#94)

`scatter` moves tuples into partitions a morsel at a time and `dispatch`
runs one task per partition, both on std::thread::scope workers taking work
from a mutex-guarded queue. Neither result depends on scheduling: a
partition keeps input order and results come back in task order, which the
parallel builds need in order to reproduce their serial builds exactly.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EC25BmBrMyFPmp424HLMDd
EOF
```

Mutation checks, following the protocol in the ground rules:
1. In `scatter`, change `let first = index * morsel_tuples;` to
   `let first = 0;`. `scatter_keeps_every_tuple_once_in_input_order` must
   fail.
2. In `run_workers`, change `(1..threads.get())` to `(1..1)`.
   `dispatch_runs_tasks_on_more_than_one_thread` must fail after its 10 s
   timeout, with `left: [false, true]`.

---

## Task 2 [P1]: `TreeTrieBuildMode`

**Files:**
- Create: `kermit-ds/src/ds/tree_trie/build_mode.rs`
- Modify: `kermit-ds/src/ds/tree_trie/mod.rs`, `kermit-ds/src/ds/mod.rs`, `kermit-ds/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `kermit-ds/src/ds/tree_trie/build_mode.rs` containing only the test:

```rust
#[cfg(test)]
mod tests {
    use {super::*, kermit_iters::BuildMode};

    /// The labels name every TreeTrie report's `ds_build_mode`, and
    /// kermit-lab reads a TreeTrie report without the axis as `"serial"`.
    #[test]
    fn axis_values_and_default_are_pinned() {
        let parallel = |n| TreeTrieBuildMode::Parallel(Threads::new(n).unwrap());
        assert_eq!(TreeTrieBuildMode::Serial.axis_value(), "serial");
        assert_eq!(parallel(1).axis_value(), "parallel:1");
        assert_eq!(parallel(16).axis_value(), "parallel:16");
        assert_eq!(TreeTrieBuildMode::default(), TreeTrieBuildMode::Serial);
    }
}
```

Register it in `kermit-ds/src/ds/tree_trie/mod.rs`, which becomes:

```rust
//! This module provides a [trie](https://en.wikipedia.org/wiki/Trie)-based implementation of a relation.

mod build_mode;
mod implementation;
mod tree_trie_iter;

#[cfg(test)]
mod tests;

pub use {build_mode::TreeTrieBuildMode, implementation::TreeTrie};
```

In `kermit-ds/src/ds/mod.rs`, change `tree_trie::TreeTrie,` to
`tree_trie::{TreeTrie, TreeTrieBuildMode},`. In `kermit-ds/src/lib.rs`, add
`TreeTrieBuildMode` to the `ds::{ … }` list after `TreeTrie`:

```rust
    ds::{
        ColumnTrie, ColumnTrieBuildMode, HashTrie, HashTrieConfig, IndexStructure,
        InvalidLoadFactor, LoadFactor, NoPruning, PruningPolicy, SingletonPruning, TreeTrie,
        TreeTrieBuildMode,
    },
```

- [ ] **Step 2: Run the test to see it fail**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds build_mode
```

Expected: compilation fails, because `TreeTrieBuildMode` is not defined.

- [ ] **Step 3: Implement**

Insert above the tests in `kermit-ds/src/ds/tree_trie/build_mode.rs`:

```rust
//! [`TreeTrieBuildMode`]: how a [`TreeTrie`](super::TreeTrie) is built from
//! a known set of tuples — the BuildMode category of the optimization
//! standard (`docs/specs/optimization-standard.md`).

use crate::morsel::Threads;

/// How a [`TreeTrie`](super::TreeTrie) is built from a known set of tuples.
/// Every mode builds the identical trie, down to each `Vec`'s capacity, so
/// the mode changes how long the build takes, never the trie it builds
/// (issue #94).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TreeTrieBuildMode {
    /// Sort the tuples, then insert them one at a time on the calling
    /// thread: the build `Relation::from_tuples` has always run.
    #[default]
    Serial,
    /// The morsel-driven build on this many threads, the calling thread
    /// included (`docs/data-structures/parallel-build.md`).
    Parallel(Threads),
}

impl kermit_iters::BuildMode for TreeTrieBuildMode {
    fn axis_value(&self) -> String {
        match self {
            | Self::Serial => "serial".to_string(),
            | Self::Parallel(threads) => format!("parallel:{}", threads.get()),
        }
    }
}
```

- [ ] **Step 4: Run the test to see it pass**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds build_mode
```

Expected: `axis_values_and_default_are_pinned` passes, together with
ColumnTrie's two `build_mode` tests.

- [ ] **Step 5: Format, commit, then run the mutation check**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit-ds/src/ds/tree_trie/build_mode.rs kermit-ds/src/ds/tree_trie/mod.rs kermit-ds/src/ds/mod.rs kermit-ds/src/lib.rs
git -C $WT commit -F - <<'EOF'
feat(ds): TreeTrieBuildMode with serial and parallel:N (#94)

The BuildMode type for TreeTrie: `serial`, the build `from_tuples` has
always run, and `parallel:N`, the morsel-driven build on N threads. Its
axis values are pinned, since they name every TreeTrie report's
`ds_build_mode`.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EC25BmBrMyFPmp424HLMDd
EOF
```

Mutation check: change `"parallel:{}"` to `"parallel-{}"`.
`axis_values_and_default_are_pinned` must fail.

---

## Task 3 [P1]: TreeTrie's parallel build

**Files:**
- Modify: `kermit-ds/src/ds/tree_trie/implementation.rs` (the build, the
  `BuildModeRelation` impl, tests)
- Modify: `kermit-ds/tests/trie_tests.rs`, `kermit-ds/tests/parquet_tests.rs`
  (the `BuiltWith` alias suites)

The build lives in `implementation.rs`, beside `from_tuples`, because it
needs `TreeTrie`'s private fields and calls the private
`insert_into_children`. No visibility changes are needed.

- [ ] **Step 1: Write the failing tests**

Append this module to the end of
`kermit-ds/src/ds/tree_trie/implementation.rs`:

```rust
#[cfg(test)]
mod parallel_build_tests {
    use {
        super::*,
        crate::{heap_size::HeapSize, test_support::Lcg},
    };

    fn threads(n: usize) -> Threads { Threads::new(n).expect("tests use a nonzero count") }

    /// Asserts two child lists are identical, recursively, down to every
    /// `Vec`'s capacity, which `heap_size_bytes` sums.
    #[allow(clippy::ptr_arg)] // the capacity is part of what is compared
    fn assert_same_nodes(
        actual: &Vec<TrieNode>, expected: &Vec<TrieNode>, path: &mut Vec<usize>, case: &str,
    ) {
        assert_eq!(actual.len(), expected.len(), "{case}: child count under {path:?}");
        assert_eq!(
            actual.capacity(),
            expected.capacity(),
            "{case}: child capacity under {path:?}"
        );
        for (a, e) in actual.iter().zip(expected) {
            assert_eq!(a.key(), e.key(), "{case}: key under {path:?}");
            path.push(a.key());
            assert_same_nodes(a.children(), e.children(), path, case);
            path.pop();
        }
    }

    /// Asserts two tries are identical: the same nodes, capacities, count
    /// and heap size.
    fn assert_identical(actual: &TreeTrie, expected: &TreeTrie, case: &str) {
        assert_same_nodes(actual.children(), expected.children(), &mut Vec::new(), case);
        assert_eq!(actual.tuple_count, expected.tuple_count, "{case}: tuple_count");
        assert_eq!(
            actual.heap_size_bytes(),
            expected.heap_size_bytes(),
            "{case}: heap_size_bytes"
        );
    }

    /// `parallel:N` builds exactly the serial trie (issue #94). Small key
    /// ranges make duplicates and shared prefixes common, a key range of 1
    /// puts every tuple under one first key, arity 0 stores nothing, and
    /// morsels of 7 tuples make the partition step cut the input many times.
    #[test]
    fn parallel_builds_are_identical_to_serial() {
        let seeds: &[u64] = if cfg!(miri) {
            &[1]
        } else {
            &[1, 2, 3, 0x00C0_FFEE_DEAD_BEEF]
        };
        let sizes: &[usize] = if cfg!(miri) {
            &[0, 1, 17]
        } else {
            &[0, 1, 2, 17, 200, 1000]
        };
        let thread_counts: &[usize] = if cfg!(miri) { &[2] } else { &[1, 2, 3, 8] };
        let key_ranges: &[usize] = if cfg!(miri) { &[5] } else { &[1, 2, 5, 50] };
        for &seed in seeds {
            let mut rng = Lcg(seed);
            for arity in 0..=4 {
                for &key_range in key_ranges {
                    for &n in sizes {
                        let tuples: Vec<Vec<usize>> = (0..n)
                            .map(|_| (0..arity).map(|_| rng.next_usize() % key_range).collect())
                            .collect();
                        let serial: TreeTrie = TreeTrie::from_tuples(arity.into(), tuples.clone());
                        for &t in thread_counts {
                            for morsel in [7, MORSEL_TUPLES] {
                                let case = format!(
                                    "seed {seed}, arity {arity}, keys < {key_range}, n {n}, \
                                     threads {t}, morsel {morsel}"
                                );
                                let parallel: TreeTrie = TreeTrie::build_parallel(
                                    arity.into(),
                                    threads(t),
                                    morsel,
                                    tuples.clone(),
                                );
                                assert_identical(&parallel, &serial, &case);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Both modes build the same trie, so only `PARALLEL_BUILDS` can show
    /// which build each mode runs.
    #[test]
    fn build_modes_reach_their_builds() {
        let tuples = vec![vec![2, 1], vec![1, 2], vec![1, 2], vec![1, 3]];
        let serial: TreeTrie = TreeTrie::from_tuples(2.into(), tuples.clone());
        for (mode, parallel_builds) in [
            (TreeTrieBuildMode::Serial, 0),
            (TreeTrieBuildMode::Parallel(threads(2)), 1),
        ] {
            PARALLEL_BUILDS.with(|builds| builds.set(0));
            let built: TreeTrie =
                TreeTrie::from_tuples_with_build_mode(2.into(), mode, tuples.clone());
            assert_eq!(
                PARALLEL_BUILDS.with(|builds| builds.get()),
                parallel_builds,
                "{mode:?}"
            );
            assert_identical(&built, &serial, &format!("{mode:?}"));
        }
    }

    #[test]
    #[should_panic(expected = "does not match header arity")]
    fn parallel_build_rejects_a_first_tuple_of_the_wrong_arity() {
        let _: TreeTrie = TreeTrie::from_tuples_parallel(2.into(), threads(2), vec![vec![1, 2, 3]]);
    }

    #[test]
    #[should_panic(expected = "tuple.len() == arity")]
    fn parallel_build_rejects_mixed_arity() {
        let _: TreeTrie =
            TreeTrie::from_tuples_parallel(2.into(), threads(2), vec![vec![1, 2], vec![3]]);
    }

    /// The splitters are strictly increasing first keys, at most
    /// `partitions − 1` of them.
    #[test]
    fn splitters_are_increasing_first_keys() {
        let tuples: Vec<Vec<usize>> = (0..1000).map(|i| vec![(i * 7) % 100, i]).collect();
        let splitters = first_key_splitters(&tuples, 8);
        assert!(!splitters.is_empty() && splitters.len() <= 7, "{splitters:?}");
        assert!(splitters.windows(2).all(|pair| pair[0] < pair[1]), "{splitters:?}");
        assert!(splitters.iter().all(|&s| s < 100), "{splitters:?}");
        assert_eq!(first_key_splitters(&[vec![5, 1], vec![5, 2]], 4), vec![5]);
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds parallel_build
```

Expected: compilation fails, because `build_parallel`,
`from_tuples_parallel`, `first_key_splitters`, `PARALLEL_BUILDS` and the
`BuildModeRelation` impl are not defined yet.

- [ ] **Step 3: Implement the build**

First, the imports at the top of
`kermit-ds/src/ds/tree_trie/implementation.rs` become:

```rust
use {
    super::build_mode::TreeTrieBuildMode,
    crate::{
        morsel::{dispatch, scatter, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD},
        relation::{BuildModeRelation, Relation, RelationHeader},
        seek::{seek_axes, GallopingSeek, SeekStrategy},
    },
    kermit_iters::{HasOptimizationAxes, JoinIterable},
    serde_json::Value,
    std::{
        collections::BTreeMap,
        marker::PhantomData,
        ops::{Index, IndexMut},
    },
};
```

Second, insert this directly after the existing block
`impl<S: SeekStrategy> TreeTrie<S> { pub(crate) fn children(…) … }`:

```rust
/// First keys sampled per partition when the parallel build places its
/// splitters. More samples put the splitters nearer the true quantiles of
/// the first-key distribution.
const SPLITTER_SAMPLES_PER_PARTITION: usize = 32;

/// Up to `partitions − 1` splitters for the parallel build: evenly spaced
/// keys of a sorted, deduplicated stride sample of the first keys. A tuple
/// goes to partition `splitters.partition_point(|&s| s <= tuple[0])`, so
/// partitions are ordered by first key and all tuples sharing a first key
/// share a partition. The splitters move work between partitions, never
/// the trie.
fn first_key_splitters(tuples: &[Vec<usize>], partitions: usize) -> Vec<usize> {
    let stride = (tuples.len() / (SPLITTER_SAMPLES_PER_PARTITION * partitions)).max(1);
    let mut sample: Vec<usize> = tuples.iter().step_by(stride).map(|tuple| tuple[0]).collect();
    sample.sort_unstable();
    sample.dedup();
    let mut splitters: Vec<usize> = (1..partitions)
        .map(|p| sample[p * sample.len() / partitions])
        .collect();
    splitters.dedup();
    splitters
}

#[cfg(test)]
thread_local! {
    /// How many parallel builds ran on this thread. Every build mode builds
    /// the same trie, so only this count can tell a test which build ran.
    static PARALLEL_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl<S: SeekStrategy> TreeTrie<S> {
    /// The `parallel:N` build (`docs/data-structures/parallel-build.md`):
    /// the trie [`Relation::from_tuples`] builds, built on `threads` threads.
    fn from_tuples_parallel(
        header: RelationHeader, threads: Threads, tuples: Vec<Vec<usize>>,
    ) -> Self {
        Self::build_parallel(header, threads, MORSEL_TUPLES, tuples)
    }

    /// [`from_tuples_parallel`](Self::from_tuples_parallel) with the morsel
    /// size as a parameter, so tests can cut a small input into many
    /// morsels.
    ///
    /// 1. **Partition**: [`scatter`] the tuples into first-key ranges cut
    ///    at [`first_key_splitters`].
    /// 2. **Build**: [`dispatch`] each partition to a worker, which sorts it
    ///    and inserts its tuples one at a time, as the serial build does
    ///    with the whole input.
    /// 3. **Assemble**: push every partition's top-level nodes onto the
    ///    root, in key order, one at a time.
    ///
    /// Sorted order restricted to a key range is that range sorted, so each
    /// subtree receives the serial build's sequence of inserts, and the root
    /// grows by one push per first key, as the serial insert-at-the-end
    /// does. Every node and every `Vec` capacity therefore matches.
    fn build_parallel(
        header: RelationHeader, threads: Threads, morsel_tuples: usize, tuples: Vec<Vec<usize>>,
    ) -> Self {
        #[cfg(test)]
        PARALLEL_BUILDS.with(|builds| builds.set(builds.get() + 1));
        if tuples.is_empty() {
            return Self::new(header);
        }
        // The serial build's checks, with its messages.
        let arity = tuples[0].len();
        assert_eq!(
            arity,
            header.arity(),
            "from_tuples: tuple arity {arity} does not match header arity {}",
            header.arity()
        );
        assert!(tuples.iter().all(|tuple| tuple.len() == arity));
        if arity == 0 {
            // A nullary tuple inserts nothing, so the serial build of any
            // number of them is the empty trie.
            return Self::new(header);
        }

        let splitters = first_key_splitters(&tuples, PARTITIONS_PER_THREAD * threads.get());
        let partitions = scatter(threads, tuples, morsel_tuples, splitters.len() + 1, |tuple| {
            splitters.partition_point(|&splitter| splitter <= tuple[0])
        });
        let built = dispatch(threads, partitions, |partition| {
            let mut tuples: Vec<Vec<usize>> =
                partition.into_tuples().map(|(_, tuple)| tuple).collect();
            // The derived `Vec<usize>` order is exactly the serial build's
            // hand-rolled comparator, and equal tuples are indistinguishable.
            tuples.sort_unstable();
            let mut nodes = Vec::new();
            let mut count = 0;
            for tuple in tuples {
                if insert_into_children(&mut nodes, tuple) {
                    count += 1;
                }
            }
            (nodes, count)
        });

        let mut trie = Self::new(header);
        for (nodes, count) in built {
            // One push per node, as the serial build inserts one node at a
            // time: `extend` or `append` would reserve in bulk and leave the
            // root with a capacity the serial build never has.
            for node in nodes {
                trie.children.push(node);
            }
            trie.tuple_count += count;
        }
        trie
    }
}
```

Third, add the `BuildModeRelation` impl directly after the
`impl<S: SeekStrategy> Relation for TreeTrie<S> { … }` block:

```rust
impl<S: SeekStrategy> BuildModeRelation for TreeTrie<S> {
    type BuildMode = TreeTrieBuildMode;

    /// Builds by `mode`: [`Relation::from_tuples`] for `Serial`, the
    /// morsel-driven build for `Parallel`. Both build the identical trie.
    ///
    /// # Panics
    ///
    /// As [`Relation::from_tuples`].
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: TreeTrieBuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self {
        match mode {
            | TreeTrieBuildMode::Serial => Self::from_tuples(header, tuples),
            | TreeTrieBuildMode::Parallel(threads) => {
                Self::from_tuples_parallel(header, threads, tuples)
            },
        }
    }
}
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds parallel_build
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit-ds 2>&1 | grep -c warning
```

Expected: 5 passed. The warning count is `0`: Task 1's `dead_code` warnings
are gone now that the build uses `scatter` and `dispatch`.

- [ ] **Step 5: Add the structure-level suites**

In `kermit-ds/tests/trie_tests.rs`, extend the import with `Threads` and
`TreeTrieBuildMode`:

```rust
use kermit_ds::{
    define_build_mode_provider, BinarySeek, BuiltWith, ColumnTrie, ColumnTrieBuildMode,
    GallopingSeek, LinearSeek, Threads, TreeTrie, TreeTrieBuildMode,
};
```

Then append at the end of the file:

```rust
// The parallel BuildMode must satisfy the same contract: it builds the same
// trie as the serial build (issue #94).
define_build_mode_provider!(
    Parallel2,
    TreeTrieBuildMode,
    TreeTrieBuildMode::Parallel(Threads::new(2).expect("2 is not zero"))
);

type TreeTrieParallel2 = BuiltWith<TreeTrie, Parallel2>;

relation_trie_test_suite!(TreeTrieParallel2);
```

In `kermit-ds/tests/parquet_tests.rs`, extend the `kermit_ds::{ … }` import
with `Threads` and `TreeTrieBuildMode`. Then insert after
`parquet_test_suite!(ColumnTrieIncremental);`:

```rust
// …and under TreeTrie's parallel BuildMode, which must load the same trie
// (issue #94).
define_build_mode_provider!(
    Parallel2,
    TreeTrieBuildMode,
    TreeTrieBuildMode::Parallel(Threads::new(2).expect("2 is not zero"))
);

type TreeTrieParallel2 = BuiltWith<TreeTrie, Parallel2>;

parquet_test_suite!(TreeTrieParallel2);
```

Run them:

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --test trie_tests --test parquet_tests treetrieparallel2
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds
```

Expected: every `treetrieparallel2` test passes; then the whole crate is
green.

- [ ] **Step 6: Miri on the new tests**

```bash
MIRIFLAGS="-Zmiri-disable-isolation" CARGO_BUILD_JOBS=2 nix develop $WT --command cargo miri test -p kermit-ds -- morsel parallel_build
```

Expected: all pass except `dispatch_runs_tasks_on_more_than_one_thread`,
which is ignored. No data race, leak or UB is reported.

- [ ] **Step 7: Format, commit, then run three mutation checks**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit-ds/src/ds/tree_trie/implementation.rs kermit-ds/tests/trie_tests.rs kermit-ds/tests/parquet_tests.rs
git -C $WT commit -F - <<'EOF'
feat(ds): TreeTrie's parallel:N build, identical to the serial build (#94)

`TreeTrie` implements `BuildModeRelation`. `parallel:N` scatters the tuples
into first-key ranges, builds each range's subtrees on a worker by sorting
and inserting as the serial build does, and pushes the subtrees onto the
root in key order, one node at a time. Array-level tests pin that every N
builds the serial trie, down to each child list's capacity.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EC25BmBrMyFPmp424HLMDd
EOF
```

Mutation checks:
1. Replace the push loop `for node in nodes { trie.children.push(node); }`
   with `trie.children.extend(nodes);`.
   `parallel_builds_are_identical_to_serial` must fail on a child capacity.
2. Change `| TreeTrieBuildMode::Parallel(threads) => {
   Self::from_tuples_parallel(header, threads, tuples) },` to
   `| TreeTrieBuildMode::Parallel(_) => Self::from_tuples(header, tuples),`.
   `build_modes_reach_their_builds` must fail.
3. Change `for (nodes, count) in built {` to
   `for (nodes, count) in built.into_iter().rev() {`.
   `parallel_builds_are_identical_to_serial` must fail on a key.

**Checkpoint 1 (controller → user):** P1 is done. Report the commit SHAs,
the identity-test case count, the Miri outcome and the mutation checks.

---

# P2 — `kermit` binary

## Task 4 [P2]: The TreeTrie cell carries its build mode

**Files:**
- Modify: `kermit/src/execution.rs` (`SortedTrie::TreeTrie`, `SortedTrieRelation for TreeTrie<S>`, `for_pair`, `for_structure`, tests)
- Modify: `kermit/src/main.rs` (`load_query_runner`), `kermit/src/bench/run.rs` (`dispatch_run_bench`), `kermit/src/bench/ds.rs` (`dispatch_ds_bench`)
- Modify: `kermit/tests/cli_column_trie_build_mode.rs`

After this task every TreeTrie report carries `ds_build_mode: "serial"`.
The flag still cannot select `parallel:N`; Task 5 wires it.

- [ ] **Step 1: Write the failing tests**

In `kermit/src/execution.rs`'s `tests` module, replace the whole function
`only_column_trie_families_report_their_build_mode` (with its doc comment)
by:

```rust
    /// Every sorted family reports the mode it builds with; HashTrie has a
    /// single build and carries no such axis until #94's second plan
    /// (issues #84, #94).
    #[test]
    fn sorted_families_report_their_build_mode() {
        assert_eq!(
            SortedTrieFamily::<ColumnTrie>::default().build_mode_axes(),
            build_mode_axis("bulk")
        );
        assert_eq!(
            SortedTrieFamily::<ColumnTrie>::new(ColumnTrieBuildMode::Incremental).build_mode_axes(),
            build_mode_axis("incremental")
        );
        assert_eq!(
            TrieLftj::<ColumnTrie>::new(ColumnTrieBuildMode::Incremental, Optimiser::Lexicographic)
                .build_mode_axes(),
            build_mode_axis("incremental")
        );
        assert_eq!(
            SortedTrieFamily::<TreeTrie>::default().build_mode_axes(),
            build_mode_axis("serial")
        );
        let two = TreeTrieBuildMode::Parallel(kermit_ds::Threads::new(2).unwrap());
        assert_eq!(
            SortedTrieFamily::<TreeTrie>::new(two).build_mode_axes(),
            build_mode_axis("parallel:2")
        );
        assert_eq!(
            TrieLftj::<TreeTrie>::new(two, Optimiser::Lexicographic).build_mode_axes(),
            build_mode_axis("parallel:2")
        );
        assert!(HashTrieFamily::<SipHashStrategy, NoPruning>::default()
            .build_mode_axes()
            .is_empty());
        assert!(HashHtj::<SipHashStrategy, NoPruning>::new(
            HashTrieConfig::default(),
            Optimiser::Lexicographic
        )
        .build_mode_axes()
        .is_empty());
    }
```

In `kermit/tests/cli_column_trie_build_mode.rs`, first replace the module
doc comment (its first six lines) by:

```rust
//! CLI smoke test for the sorted tries' `ds_build_mode` axis (issues #84,
//! #94). Every `ColumnTrie` report says which build made its relations, so
//! kermit-lab can read a `ColumnTrie` report *without* the axis as the pre-#84
//! incremental build; every `TreeTrie` report does too (`serial` unless
//! `--ds-build parallel:N`). `HashTrie` has a single build and carries no such
//! axis until #94's second plan. It also covers selecting `ColumnTrie`'s mode
//! with `--ds-build` and rejecting its values on other structures.
```

Next, replace `cli_bench_ds_other_structures_have_no_build_mode` by:

```rust
#[test]
fn cli_bench_ds_tree_trie_records_serial_and_hash_trie_no_build_mode() {
    let (output, report) = bench_ds("tree-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "serial");

    let (output, report) = bench_ds("hash-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let axes = axes_of(&report);
    assert!(axes.get("ds_build_mode").is_none(), "{axes}");
}
```

Then replace `cli_bench_run_sweep_reports_build_mode_only_on_column_trie` by:

```rust
#[test]
fn cli_bench_run_sweep_reports_each_cells_build_mode() {
    let (output, report) = bench_run("triangle", &["-i", "all", "-a", "all", "-m", "space"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        match axes["data_structure"].as_str() {
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | Some("TreeTrie") => assert_eq!(axes["ds_build_mode"], "serial", "{axes}"),
            | Some("HashTrie") => assert!(axes.get("ds_build_mode").is_none(), "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}
```

Finally, replace `cli_bench_run_sweep_carries_build_mode_only_to_column_trie_cells`
by:

```rust
#[test]
fn cli_bench_run_sweep_carries_incremental_only_to_column_trie_cells() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-build",
        "incremental",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        match axes["data_structure"].as_str() {
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "incremental", "{axes}"),
            | Some("TreeTrie") => assert_eq!(axes["ds_build_mode"], "serial", "{axes}"),
            | Some("HashTrie") => assert!(axes.get("ds_build_mode").is_none(), "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit sorted_families_report_their_build_mode
```

Expected: compilation fails (`TreeTrieBuildMode` is not in scope, and
`SortedTrieFamily::<TreeTrie>::new` expects `()`).

- [ ] **Step 3: Implement**

In `kermit/src/execution.rs`:

1. Add `TreeTrieBuildMode` to the `kermit_ds::{ … }` import:

```rust
    kermit_ds::{
        BuildModeRelation, Cardinality, ColumnTrie, ColumnTrieBuildMode, ConfigurableRelation,
        HashTrie, HashTrieConfig, HeapSize, IndexStructure, PruningPolicy, Relation,
        RelationFileExt, RelationHeader, SeekStrategy, TreeTrie, TreeTrieBuildMode,
    },
```

2. Replace the `TreeTrie` variant of `pub enum SortedTrie`:

```rust
    /// Pointer-based trie (`-i tree-trie`), seeking with `seek` and built by
    /// the `--ds-build` mode `build`.
    TreeTrie {
        /// The `--ds-layout-seek` choice, in the two roles of
        /// `Execution::HashHtj`'s Layout fields: the request that selects
        /// `S`, and the label re-derived from `S`.
        seek: SeekChoice,
        /// The `--ds-build` mode every relation is built with.
        build: TreeTrieBuildMode,
    },
```

3. Replace `impl<S: SeekStrategy> SortedTrieRelation for TreeTrie<S> { … }`:

```rust
impl<S: SeekStrategy> SortedTrieRelation for TreeTrie<S> {
    type BuildMode = TreeTrieBuildMode;

    fn kind(build: TreeTrieBuildMode) -> SortedTrie {
        SortedTrie::TreeTrie {
            seek: seek_of::<S>(),
            build,
        }
    }

    fn build_with(
        header: RelationHeader, build: TreeTrieBuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self {
        Self::from_tuples_with_build_mode(header, build, tuples)
    }

    /// Every `TreeTrie` report says which build made it, so kermit-lab can
    /// read a `TreeTrie` row *without* the axis as the serial build, the
    /// only one before #94.
    fn build_mode_axes(build: TreeTrieBuildMode) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::from([(
            "ds_build_mode".to_string(),
            serde_json::Value::from(build.axis_value()),
        )])
    }
}
```

4. In `Execution::for_pair` and `Execution::for_structure`, the TreeTrie
   arms build `SortedTrie::TreeTrie { seek, build: TreeTrieBuildMode::default() }`.
   Task 5 replaces this default with the resolved `--ds-build` mode. In
   `for_pair`:

```rust
            | (IndexStructure::TreeTrie, JoinAlgorithm::LeapfrogTriejoin) => {
                Some(Execution::TrieLftj(SortedTrie::TreeTrie {
                    seek,
                    build: TreeTrieBuildMode::default(),
                }))
            },
```

   and in `for_structure`:

```rust
            | IndexStructure::TreeTrie => Execution::TrieLftj(SortedTrie::TreeTrie {
                seek,
                build: TreeTrieBuildMode::default(),
            }),
```

5. Update the comment above `pub enum Execution`:

```rust
// `Copy` relies on `HashTrieConfig: Copy` and the build modes being `Copy`
// (`ColumnTrieBuildMode`, `TreeTrieBuildMode`); a future Config or BuildMode
// carrying heap data would have to drop it here and clone the cells instead.
```

6. In the `tests` module, replace every `::new((), ` with
   `::new(TreeTrieBuildMode::default(), `. Use Edit with `replace_all`;
   there are six occurrences after Step 1's replacement: five
   `…::new((), Optimiser::Lexicographic)` and #81's
   `TrieLftj::<TreeTrie>::new((), optimiser)` in
   `engines_gather_the_statistics_their_optimiser_reads`. (Every `new((), `
   in `execution.rs` is in the tests module.) Then add `build: TreeTrieBuildMode::Serial,` after the
   `seek: …` line of each `SortedTrie::TreeTrie { seek: … }` literal in
   the tests:
   - `families_report_their_own_execution`: one (`SeekChoice::Galloping`);
   - `sweep_attaches_the_build_mode_to_the_column_trie_cell_only`: one;
   - `sorted_families_label_their_seek_strategy_from_the_type`: three
     (`Linear`, `Binary`, `Linear`).

   The compiler names any literal you miss (`missing field 'build'`).

In `kermit/src/main.rs` (`load_query_runner`), `kermit/src/bench/run.rs`
(`dispatch_run_bench`) and `kermit/src/bench/ds.rs` (`dispatch_ds_bench`),
the TreeTrie arm destructures `build` and passes it on:

```rust
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| build_join_runner(
            TrieLftj::<kermit_ds::TreeTrie<S>>::new(build, optimiser),
            cell,
            &args.relations,
        )),
```

```rust
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| run_benchmark(
            &TrieLftj::<kermit_ds::TreeTrie<S>>::new(build, optimiser),
            workload,
            settings,
        )),
```

```rust
        | Execution::TrieLftj(SortedTrie::TreeTrie {
            seek,
            build,
        }) => with_sorted_trie_layout!(seek, |S| run_ds_bench(
            &SortedTrieFamily::<kermit_ds::TreeTrie<S>>::new(build),
            relation,
            metrics,
            queries_per_build,
            group_name,
            bench_args,
        )),
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit execution
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_column_trie_build_mode --test cli_bench_run_ds_flag_reach
```

Expected: all pass. `every_flag_reaches_its_cell_under_all_all` still passes
unchanged, since it never checks that TreeTrie lacks a build mode.

- [ ] **Step 5: Format, commit, then run the mutation check**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit/src/execution.rs kermit/src/main.rs kermit/src/bench/run.rs kermit/src/bench/ds.rs kermit/tests/cli_column_trie_build_mode.rs
git -C $WT commit -F - <<'EOF'
feat(kermit): the TreeTrie cell carries its build mode (#94)

`SortedTrie::TreeTrie` gains a `build` field, TreeTrie's family builds
through `from_tuples_with_build_mode`, and every TreeTrie report records
`ds_build_mode` ("serial" until `--ds-build parallel:N` arrives in the next
commit).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EC25BmBrMyFPmp424HLMDd
EOF
```

Mutation check: make TreeTrie's `build_mode_axes` return `BTreeMap::new()`.
`sorted_families_report_their_build_mode` must fail.

---

## Task 5 [P2]: `--ds-build serial | parallel:N`

**Files:**
- Modify: `kermit/src/options.rs` (`BuildChoice`, `parse_build_choice`, `BuildModes`, `BuildChoices`, `DsFlag::Build`, `DsChoices.build`, tests)
- Modify: `kermit/src/execution.rs` (`for_pair`, `for_structure`, tests)
- Create: `kermit/tests/cli_tree_trie_build_mode.rs`

- [ ] **Step 1: Write the failing tests**

In `kermit/src/options.rs`'s `tests` module, make these replacements.

Replace `validate_build_choices_accepts_column_trie_or_all_only` by:

```rust
    /// Each `--ds-build` value reaches the structures that have it, and
    /// `all`, and no other. Until #94's second plan only TreeTrie has
    /// `serial` and `parallel:N`.
    #[test]
    fn validate_build_choices_accepts_each_value_on_its_structures_or_all() {
        let incremental = BuildChoices {
            build: Some(BuildChoice::Incremental),
        };
        let parallel = BuildChoices {
            build: Some(BuildChoice::Parallel(Threads::new(4).unwrap())),
        };
        for (build, home, others, named) in [
            (
                &incremental,
                IndexStructureSelector::ColumnTrie,
                [IndexStructureSelector::TreeTrie, IndexStructureSelector::HashTrie],
                "column-trie",
            ),
            (
                &parallel,
                IndexStructureSelector::TreeTrie,
                [IndexStructureSelector::ColumnTrie, IndexStructureSelector::HashTrie],
                "tree-trie",
            ),
        ] {
            assert!(validate_build_choices(home, build).is_ok(), "{home:?}");
            assert!(validate_build_choices(IndexStructureSelector::All, build).is_ok());
            for sel in others {
                let msg = validate_build_choices(sel, build).unwrap_err().to_string();
                assert!(msg.contains("--ds-build"), "{msg}");
                assert!(msg.contains(named), "{msg}");
                assert!(validate_build_choices(sel, &BuildChoices::default()).is_ok());
            }
        }
    }
```

Replace `ds_choices_resolve_carries_the_build_mode` by:

```rust
    #[test]
    fn ds_choices_resolve_carries_the_build_mode() {
        let incremental = BuildChoices {
            build: Some(BuildChoice::Incremental),
        };
        let choices = DsChoices::resolve(
            IndexStructureSelector::ColumnTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &incremental,
        )
        .unwrap();
        assert_eq!(choices.build, BuildModes {
            column: ColumnTrieBuildMode::Incremental,
            tree: TreeTrieBuildMode::Serial,
        });

        let four = Threads::new(4).unwrap();
        let parallel = BuildChoices {
            build: Some(BuildChoice::Parallel(four)),
        };
        let choices = DsChoices::resolve(
            IndexStructureSelector::All,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &parallel,
        )
        .unwrap();
        assert_eq!(choices.build, BuildModes {
            column: ColumnTrieBuildMode::Bulk,
            tree: TreeTrieBuildMode::Parallel(four),
        });

        assert_eq!(DsChoices::default().build, BuildModes {
            column: ColumnTrieBuildMode::Bulk,
            tree: TreeTrieBuildMode::Serial,
        });
        assert!(DsChoices::resolve(
            IndexStructureSelector::TreeTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &incremental,
        )
        .is_err());
        assert!(DsChoices::resolve(
            IndexStructureSelector::ColumnTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &parallel,
        )
        .is_err());
    }
```

In `ds_flag_given_lists_exactly_the_flags_passed`, the `build` binding
becomes:

```rust
        let build = BuildChoices {
            build: Some(BuildChoice::Incremental),
        };
```

and both expected vectors end with `DsFlag::Build(BuildChoice::Incremental)`
instead of `DsFlag::Build`.

Replace `unreached_flag_is_the_first_flag_no_structure_has` by:

```rust
    #[test]
    fn unreached_flag_is_the_first_flag_no_structure_has() {
        let incremental = DsFlag::Build(BuildChoice::Incremental);
        let parallel = DsFlag::Build(BuildChoice::Parallel(Threads::new(2).unwrap()));
        let sorted = [IndexStructure::TreeTrie, IndexStructure::ColumnTrie];
        assert_eq!(unreached_flag(&[DsFlag::LayoutSeek, incremental], &sorted), None);
        assert_eq!(
            unreached_flag(
                &[DsFlag::LayoutSeek, DsFlag::Config, DsFlag::LayoutHasher],
                &sorted
            ),
            Some(DsFlag::Config)
        );
        // A `--ds-build` value reaches only the structures that have it.
        assert_eq!(
            unreached_flag(&[incremental], &[IndexStructure::TreeTrie]),
            Some(incremental)
        );
        assert_eq!(unreached_flag(&[parallel], &[IndexStructure::TreeTrie]), None);
        assert_eq!(
            unreached_flag(&[parallel], &[IndexStructure::ColumnTrie]),
            Some(parallel)
        );
        assert_eq!(unreached_flag(&[], &[]), None);
    }
```

Add these tests at the end of the module:

```rust
    #[test]
    fn build_choices_parse_and_display_round_trip() {
        for text in ["bulk", "incremental", "serial", "parallel:1", "parallel:16"] {
            assert_eq!(parse_build_choice(text).unwrap().to_string(), text);
        }
        assert_eq!(
            parse_build_choice("parallel:8"),
            Ok(BuildChoice::Parallel(Threads::new(8).unwrap()))
        );
    }

    /// Every malformed value names the form it expected.
    #[test]
    fn build_choices_reject_malformed_values() {
        for bad in [
            "parallel",
            "parallel:",
            "parallel:0",
            "parallel:x",
            "parallel:-1",
            "parallel:1025",
            "Serial",
            "threads:4",
            "",
        ] {
            let msg = parse_build_choice(bad).unwrap_err();
            assert!(msg.contains("parallel:N"), "{bad:?}: {msg}");
        }
    }

    /// Which structures each value reaches. HashTrie joins `serial` and
    /// `parallel:N` in #94's second plan.
    #[test]
    fn build_choice_structures_are_pinned() {
        assert_eq!(BuildChoice::Bulk.structures(), &[IndexStructure::ColumnTrie]);
        assert_eq!(BuildChoice::Incremental.structures(), &[IndexStructure::ColumnTrie]);
        assert_eq!(BuildChoice::Serial.structures(), &[IndexStructure::TreeTrie]);
        assert_eq!(
            BuildChoice::Parallel(Threads::new(2).unwrap()).structures(),
            &[IndexStructure::TreeTrie]
        );
    }

    #[test]
    fn ds_build_flag_names_its_value() {
        let flag = DsFlag::Build(BuildChoice::Parallel(Threads::new(2).unwrap()));
        assert_eq!(flag.to_string(), "--ds-build parallel:2");
        assert_eq!(flag.structures_label(), "tree-trie");
    }
```

In `kermit/src/execution.rs`'s `tests` module, add
`crate::options::BuildModes` to the `use` block:

```rust
    use {
        super::*,
        crate::options::BuildModes,
        clap::ValueEnum,
        kermit_ds::{NoPruning, SingletonPruning},
        kermit_iters::SipHashStrategy,
        std::cell::Cell,
    };
```

Replace `for_structure_agrees_with_for_pair` by:

```rust
    /// `for_structure` must name the one cell `for_pair` accepts for that
    /// structure — otherwise `bench ds` and `bench run` could disagree on
    /// which family a structure belongs to.
    #[test]
    fn for_structure_agrees_with_for_pair() {
        let config = HashTrieConfig::default();
        let builds = [
            BuildModes::default(),
            BuildModes {
                column: ColumnTrieBuildMode::Incremental,
                ..BuildModes::default()
            },
            BuildModes {
                tree: TreeTrieBuildMode::Parallel(kermit_ds::Threads::new(4).unwrap()),
                ..BuildModes::default()
            },
        ];
        for build in builds {
            for ds in all_structures() {
                for hasher in [HasherChoice::Sip, HasherChoice::Fxhash] {
                    for pruning in [PruningChoice::Off, PruningChoice::On] {
                        for &seek in SeekChoice::value_variants() {
                            let choices = DsChoices {
                                hasher,
                                pruning,
                                seek,
                                config,
                                build,
                            };
                            let cell = Execution::for_structure(ds, choices);
                            assert_eq!(cell.index_structure(), ds);
                            assert_eq!(
                                Execution::for_pair(ds, cell.algorithm(), choices),
                                Some(cell)
                            );
                        }
                    }
                }
            }
        }
    }
```

Replace `sweep_attaches_the_build_mode_to_the_column_trie_cell_only` (with
its doc comment) by:

```rust
    /// Each structure's `--ds-build` mode reaches its own cell of a sweep and
    /// no other.
    #[test]
    fn sweep_attaches_each_build_mode_to_its_own_cell() {
        let four = TreeTrieBuildMode::Parallel(kermit_ds::Threads::new(4).unwrap());
        let choices = DsChoices {
            build: BuildModes {
                column: ColumnTrieBuildMode::Incremental,
                tree: four,
            },
            ..DsChoices::default()
        };
        let sweep = Sweep::expand(&all_structures(), &all_algorithms(), choices);
        assert!(sweep
            .cells
            .contains(&Execution::TrieLftj(SortedTrie::ColumnTrie {
                seek: SeekChoice::Galloping,
                build: ColumnTrieBuildMode::Incremental,
            })));
        assert!(sweep
            .cells
            .contains(&Execution::TrieLftj(SortedTrie::TreeTrie {
                seek: SeekChoice::Galloping,
                build: four,
            })));
    }
```

Create `kermit/tests/cli_tree_trie_build_mode.rs`:

```rust
//! CLI smoke test for `TreeTrie`'s `ds_build_mode` axis (issue #94):
//! `--ds-build serial | parallel:N` selects the build, every `TreeTrie` report
//! records it, and a value is rejected on a structure without it.

mod common;

use common::cli::{axes_of, bench_ds, bench_join, bench_run, reports_of};

#[test]
fn cli_bench_ds_tree_trie_records_a_parallel_build() {
    let (output, report) = bench_ds("tree-trie", &["--ds-build", "parallel:2"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "parallel:2");
}

#[test]
fn cli_bench_join_tree_trie_records_a_parallel_build() {
    let (output, report) = bench_join("tree-trie", "leapfrog-triejoin", &[
        "-m",
        "space",
        "--ds-build",
        "parallel:3",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "parallel:3");
}

/// `--verify` checks the answer counts of the parallel-built tries against
/// the benchmark's expected values.
#[test]
fn cli_bench_run_verifies_answers_from_parallel_built_tries() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "tree-trie",
        "-a",
        "leapfrog-triejoin",
        "-m",
        "iteration",
        "--verify",
        "--ds-build",
        "parallel:2",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let axes = axes_of(&report);
    assert_eq!(axes["ds_build_mode"], "parallel:2", "{axes}");
    assert_eq!(axes["verified"], true, "{axes}");
}

#[test]
fn cli_bench_run_sweep_carries_parallel_only_to_tree_trie_cells() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-build",
        "parallel:2",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 3, "three valid cells: {reports:?}");
    for r in &reports {
        let axes = &r["axes"];
        match axes["data_structure"].as_str() {
            | Some("TreeTrie") => assert_eq!(axes["ds_build_mode"], "parallel:2", "{axes}"),
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | Some("HashTrie") => assert!(axes.get("ds_build_mode").is_none(), "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}

/// Until #94's second plan only TreeTrie has `parallel:N`.
#[test]
fn cli_bench_ds_rejects_parallel_off_tree_trie() {
    for ds in ["column-trie", "hash-trie"] {
        let (output, _) = bench_ds(ds, &["--ds-build", "parallel:2"]);
        assert!(!output.status.success(), "{ds} accepted parallel:2");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--ds-build parallel:2"), "{ds}: {stderr}");
        assert!(stderr.contains("tree-trie"), "{ds}: {stderr}");
    }
}

#[test]
fn cli_rejects_malformed_parallel_values() {
    for bad in ["parallel", "parallel:0", "parallel:1025", "parallel:x"] {
        let (output, _) = bench_ds("tree-trie", &["--ds-build", bad]);
        assert!(!output.status.success(), "accepted {bad}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("parallel:N"), "{bad}: {stderr}");
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit options
```

Expected: compilation fails (`BuildChoice`, `parse_build_choice` and
`BuildModes` are not defined, and `BuildChoices` has no `build` field).

- [ ] **Step 3: Implement**

In `kermit/src/options.rs`:

1. The `kermit_ds` import gains `Threads` and `TreeTrieBuildMode`:

```rust
    kermit_ds::{
        ColumnTrieBuildMode, HashTrieConfig, IndexStructure, LoadFactor, PruningPolicy,
        SeekStrategy, Threads, TreeTrieBuildMode,
    },
```

2. `DsFlag::Build` carries its value. Replace the variant:

```rust
    /// `--ds-build`, with the value given: which structures have its axis
    /// depends on the value.
    Build(BuildChoice),
```

   In `DsFlag::structures`, replace the `| Self::Build => &[IndexStructure::ColumnTrie],`
   arm with:

```rust
            | Self::Build(choice) => choice.structures(),
```

   Replace `impl fmt::Display for DsFlag` with:

```rust
impl fmt::Display for DsFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | Self::LayoutHasher => f.write_str("--ds-layout-hasher"),
            | Self::LayoutPruning => f.write_str("--ds-layout-pruning"),
            | Self::LayoutSeek => f.write_str("--ds-layout-seek"),
            | Self::Config => f.write_str("--ds-config"),
            | Self::Build(choice) => write!(f, "--ds-build {choice}"),
        }
    }
}
```

3. Replace everything from the `/// BuildMode-axis CLI choice, …` doc
   comment through the end of `impl BuildChoices { … }` with:

```rust
/// One `--ds-build` value. `ColumnTrie`'s and `TreeTrie`'s modes share the
/// flag; which structures a value reaches is
/// [`structures`](Self::structures).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum BuildChoice {
    /// `bulk`: `ColumnTrie`'s one-pass build, its default.
    Bulk,
    /// `incremental`: `ColumnTrie`'s build before #84.
    Incremental,
    /// `serial`: `TreeTrie`'s single-threaded build, its default.
    Serial,
    /// `parallel:N`: `TreeTrie`'s morsel-driven build on `N` threads.
    Parallel(Threads),
}

impl BuildChoice {
    /// The structures that have this value's build mode: the `--ds-build`
    /// row of [`DsFlag::structures`].
    pub(crate) fn structures(self) -> &'static [IndexStructure] {
        match self {
            | Self::Bulk | Self::Incremental => &[IndexStructure::ColumnTrie],
            | Self::Serial | Self::Parallel(_) => &[IndexStructure::TreeTrie],
        }
    }
}

impl fmt::Display for BuildChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | Self::Bulk => f.write_str("bulk"),
            | Self::Incremental => f.write_str("incremental"),
            | Self::Serial => f.write_str("serial"),
            | Self::Parallel(threads) => write!(f, "parallel:{}", threads.get()),
        }
    }
}

/// Parses a `--ds-build` value: `bulk`, `incremental`, `serial`, or
/// `parallel:N` with `N` a whole number of threads from 1 to
/// [`Threads::MAX`].
pub(crate) fn parse_build_choice(value: &str) -> Result<BuildChoice, String> {
    match value {
        | "bulk" => Ok(BuildChoice::Bulk),
        | "incremental" => Ok(BuildChoice::Incremental),
        | "serial" => Ok(BuildChoice::Serial),
        | other => {
            let threads = other.strip_prefix("parallel:").ok_or_else(|| {
                format!("expected bulk, incremental, serial or parallel:N, got {other:?}")
            })?;
            threads
                .parse::<usize>()
                .ok()
                .and_then(Threads::new)
                .map(BuildChoice::Parallel)
                .ok_or_else(|| {
                    format!(
                        "parallel:N needs a whole number of threads N from 1 to {}, got {other:?}",
                        Threads::MAX
                    )
                })
        },
    }
}

/// Every structure's build mode, resolved from `--ds-build`: the value
/// reaches the structures that have it, and every other structure keeps its
/// default.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BuildModes {
    /// `ColumnTrie`'s build: `bulk` (default) or `incremental`.
    pub column: ColumnTrieBuildMode,
    /// `TreeTrie`'s build: `serial` (default) or `parallel:N`.
    pub tree: TreeTrieBuildMode,
}

/// BuildMode-axis CLI choice, flattened beside [`LayoutChoices`] and
/// [`ConfigChoices`] into `bench ds`, `bench run` and `bench join`. Every
/// build mode builds the same structure, so the flag changes build time
/// only. `kermit join` takes no `--ds-build`, for the same reason it takes
/// no `--ds-config`: it cannot change a query's answers.
#[derive(Args, Clone, Debug, Default)]
pub(crate) struct BuildChoices {
    /// How the selected structure is built from its tuples: `bulk` (default)
    /// or `incremental` for `column-trie`; `serial` (default) or `parallel:N`
    /// (N threads, 1 to 1024) for `tree-trie`. A value is only valid with a
    /// structure that has it (or `all`).
    #[arg(long = "ds-build", value_name = "MODE", value_parser = parse_build_choice)]
    build: Option<BuildChoice>,
}

impl BuildChoices {
    /// Every structure's build mode: `--ds-build` reaches the structures that
    /// have its value, and the rest keep their defaults.
    pub(crate) fn resolved(&self) -> BuildModes {
        let mut modes = BuildModes::default();
        match self.build {
            | None => {},
            | Some(BuildChoice::Bulk) => modes.column = ColumnTrieBuildMode::Bulk,
            | Some(BuildChoice::Incremental) => modes.column = ColumnTrieBuildMode::Incremental,
            | Some(BuildChoice::Serial) => modes.tree = TreeTrieBuildMode::Serial,
            | Some(BuildChoice::Parallel(threads)) => {
                modes.tree = TreeTrieBuildMode::Parallel(threads)
            },
        }
        modes
    }

    /// `--ds-build` if the user passed it. See [`DsFlag::given`].
    fn given(&self) -> Option<DsFlag> { self.build.map(DsFlag::Build) }
}
```

4. In `DsChoices`, the `build` field becomes:

```rust
    /// `--ds-build`; each sorted structure's mode reaches its own cell.
    pub build: BuildModes,
```

   and in `DsChoices::resolve`, `build: build.column_trie_build_resolved(),`
   becomes `build: build.resolved(),`.

In `kermit/src/execution.rs`, the sorted arms of `Execution::for_pair`
take their own structure's mode:

```rust
            | (IndexStructure::TreeTrie, JoinAlgorithm::LeapfrogTriejoin) => {
                Some(Execution::TrieLftj(SortedTrie::TreeTrie {
                    seek,
                    build: build.tree,
                }))
            },
            | (IndexStructure::ColumnTrie, JoinAlgorithm::LeapfrogTriejoin) => {
                Some(Execution::TrieLftj(SortedTrie::ColumnTrie {
                    seek,
                    build: build.column,
                }))
            },
```

and so do those of `Execution::for_structure`:

```rust
            | IndexStructure::TreeTrie => Execution::TrieLftj(SortedTrie::TreeTrie {
                seek,
                build: build.tree,
            }),
            | IndexStructure::ColumnTrie => Execution::TrieLftj(SortedTrie::ColumnTrie {
                seek,
                build: build.column,
            }),
```

In `for_pair`'s doc comment, change "and the column-trie cell's build mode"
to "and each sorted cell's own build mode".

- [ ] **Step 4: Run the tests to see them pass**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_tree_trie_build_mode --test cli_column_trie_build_mode --test cli_bench_run_ds_flag_reach
```

Expected: all pass. `cli_bench_ds_rejects_ds_build_off_column_trie` still
passes: its error now reads `--ds-build bulk is only valid with
--indexstructure column-trie (or all)`.

- [ ] **Step 5: Format, commit, then run two mutation checks**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit/src/options.rs kermit/src/execution.rs kermit/tests/cli_tree_trie_build_mode.rs
git -C $WT commit -F - <<'EOF'
feat(kermit): --ds-build serial | parallel:N selects TreeTrie's build (#94)

`--ds-build` parses one `BuildChoice` for every structure that has a build
mode: `bulk | incremental` for ColumnTrie, `serial | parallel:N` for
TreeTrie. `DsFlag::Build` carries the value, so the one structures table
rejects a value on a structure without it, and `DsChoices.build` resolves
to each structure's own mode.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EC25BmBrMyFPmp424HLMDd
EOF
```

Mutation checks:
1. Change `| Self::Serial | Self::Parallel(_) => &[IndexStructure::TreeTrie],`
   to `… => &[IndexStructure::ColumnTrie],`.
   `validate_build_choices_accepts_each_value_on_its_structures_or_all` must
   fail.
2. In `resolved`, change the `Parallel(threads)` arm to set
   `modes.tree = TreeTrieBuildMode::Serial`.
   `ds_choices_resolve_carries_the_build_mode` must fail.

---

## Task 6 [P2]: TreeTrie's parallel build through the join suites

**Files:**
- Modify: `kermit/tests/join_tests.rs`

- [ ] **Step 1: Add the suites**

Extend the `kermit_ds::{ … }` import with `Threads` and `TreeTrieBuildMode`:

```rust
    kermit_ds::{
        define_build_mode_provider, define_config_provider, BinarySeek, ColumnTrie,
        ColumnTrieBuildMode, GallopingSeek, HashTrie, HashTrieConfig, LinearSeek, LoadFactor,
        SingletonPruning, Threads, TreeTrie, TreeTrieBuildMode,
    },
```

Then insert directly after the ColumnTrie `Incremental` build-mode suites
(the three `define_multiway_join_test_suite_for_build_mode!(ColumnTrie, …,
Incremental);` invocations, one per optimiser since #81):

```rust
// ── BuildMode axis: TreeTrie's parallel build ───────────────────────────
// The plain TreeTrie invocations above build serially; these build on two
// threads. Every mode must build the same trie (issue #94).
define_build_mode_provider!(
    Parallel2,
    TreeTrieBuildMode,
    TreeTrieBuildMode::Parallel(Threads::new(2).expect("2 is not zero"))
);

define_multiway_join_test_suite_for_build_mode!(
    TreeTrie,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    Parallel2
);
define_multiway_join_test_suite_for_build_mode!(
    TreeTrie,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    Parallel2
);
define_multiway_join_test_suite_for_build_mode!(
    TreeTrie,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    Parallel2
);
```

- [ ] **Step 2: Run them**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests build_mode_treetrie
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit
```

Expected: 48 tests pass in the first run (the 16 standard patterns × three
optimisers); then the whole `kermit` crate is green.

- [ ] **Step 3: Format and commit**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit/tests/join_tests.rs
git -C $WT commit -F - <<'EOF'
test(kermit): TreeTrie's parallel build through the join suites (#94)

The 16 standard join patterns run on `BuiltWith<TreeTrie, Parallel2>` under
every optimiser, the BuildMode test obligation of the optimization standard.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EC25BmBrMyFPmp424HLMDd
EOF
```

---

# P3 — analysis and docs

## Task 7 [P3]: kermit-lab: `serial` back-fill, `threads` and the `speedup` preset

**Files:**
- Modify: `python/kermit-lab/kermit_lab/defaults.py`, `frame.py`, `analysis.py`, `presets.py`, `__init__.py`, `drivers/main.py`
- Modify tests: `python/kermit-lab/tests/conftest.py`, `test_defaults.py`, `test_frame.py`, `test_analysis.py`, `test_presets.py`, `test_cli.py`, `test_public_api.py`, `test_render_all.py`, `test_contract.py`

- [ ] **Step 1: Write the failing tests**

In `tests/conftest.py`, rewrite `fixture_build_mode_tree`: its third report
becomes a HashTrie one. From Task 4 on, TreeTrie reports carry a build
mode, and HashTrie is now the structure without one.

```python
@pytest.fixture
def fixture_build_mode_tree(tmp_path: Path) -> dict:
    """ColumnTrie reports from before and after issue #84, plus a HashTrie
    one, for the build-mode ablation guard.

    The old ColumnTrie report carries no ``ds_build_mode`` (back-filled to
    ``incremental`` on load); the new one carries ``"bulk"``. HashTrie has no
    build mode until #94's second plan, so its row stays NaN. Each times
    insertion and iteration (plus space), so the build-mode axis has two
    values and applies to one time phase but not the other.
    """
    criterion_root = tmp_path / "target" / "criterion"
    reports_dir = tmp_path / "reports"
    criterion_root.mkdir(parents=True)
    reports_dir.mkdir()

    paths: list[Path] = []
    for tag, data_structure, algorithm, build_mode, insertion_point in (
        ("old", "ColumnTrie", "LeapfrogTriejoin", None, 9000.0),
        ("new", "ColumnTrie", "LeapfrogTriejoin", "bulk", 300.0),
        ("hash", "HashTrie", "HashTriejoin", None, 500.0),
    ):
        groups: list[tuple[str, str, str]] = []
        # Same trie under both builds, so the same traversal time.
        for phase, point in (("insertion", insertion_point), ("iteration", 100.0)):
            function = f"{data_structure}/{tag}/{phase}"
            samples = [(i + 1, point * (i + 1)) for i in range(10)]
            _write_function_dir(
                criterion_root, _FunctionSpec("run", function, "time", point, samples)
            )
            groups.append(("run", function, "time"))
        space_function = f"{data_structure}/{tag}/space"
        space_samples = [(i + 1, 6400.0 * (i + 1)) for i in range(10)]
        _write_function_dir(
            criterion_root,
            _FunctionSpec("run", space_function, "space", 6400.0, space_samples),
        )
        groups.append(("run", space_function, "space"))
        axes = {
            "benchmark": "triangle",
            "query": "triangle",
            "data_structure": data_structure,
            "algorithm": algorithm,
            "tuples": 100,
        }
        if build_mode is not None:
            axes["ds_build_mode"] = build_mode
        paths.append(
            _write_report(
                reports_dir, f"run-{data_structure}-{tag}", kind="run", axes=axes,
                metadata=[], groups=groups,
            )
        )
    return {
        "criterion_root": criterion_root,
        "reports_dir": reports_dir,
        "paths": sorted(paths),
    }
```

Append a new fixture to `tests/conftest.py`:

```python
@pytest.fixture
def fixture_parallel_build_tree(tmp_path: Path) -> dict:
    """``bench ds`` reports of one relation: TreeTrie under ``serial``,
    ``parallel:2`` and ``parallel:4``, two replicates each (distinct
    ``--name``, so distinct Criterion groups), plus a ColumnTrie ``bulk``
    case with no serial baseline. Insertion time halves per thread
    doubling, so the expected speedups are 2 and 4 exactly.
    """
    criterion_root = tmp_path / "target" / "criterion"
    reports_dir = tmp_path / "reports"
    criterion_root.mkdir(parents=True)
    reports_dir.mkdir()

    paths: list[Path] = []
    for data_structure, mode, point in (
        ("TreeTrie", "serial", 8000.0),
        ("TreeTrie", "parallel:2", 4000.0),
        ("TreeTrie", "parallel:4", 2000.0),
        ("ColumnTrie", "bulk", 3000.0),
    ):
        for replicate, jitter in enumerate((1.0, 1.02)):
            group = f"{data_structure}-{mode.replace(':', '-')}-{replicate}"
            function = f"{data_structure}/insertion"
            mean = point * jitter
            samples = [(i + 1, mean * (i + 1)) for i in range(10)]
            _write_function_dir(
                criterion_root, _FunctionSpec(group, function, "time", mean, samples)
            )
            axes = {
                "data_structure": data_structure,
                "relation_path": "/data/edge.parquet",
                "tuples": 1000,
                "arity": 2,
                "ds_build_mode": mode,
            }
            paths.append(
                _write_report(
                    reports_dir, group, kind="ds", axes=axes, metadata=[],
                    groups=[(group, function, "time")],
                )
            )
    return {
        "criterion_root": criterion_root,
        "reports_dir": reports_dir,
        "paths": sorted(paths),
    }
```

In `tests/test_defaults.py`, replace `test_build_mode_backfills_column_trie_rows_only` by:

```python
def test_build_mode_backfills_column_and_tree_trie_rows_only() -> None:
    df = pd.DataFrame({
        "data_structure": ["ColumnTrie", "ColumnTrie", "TreeTrie", "TreeTrie", "HashTrie"],
        "ds_build_mode": [pd.NA, "bulk", pd.NA, "parallel:4", pd.NA],
    })
    out = apply_axis_defaults(df)
    assert out["ds_build_mode"].tolist()[:4] == ["incremental", "bulk", "serial", "parallel:4"]
    # HashTrie has no build mode until #94's second plan.
    assert pd.isna(out["ds_build_mode"].iloc[4])
    assert SCOPED_AXIS_DEFAULTS[("ds_build_mode", "ColumnTrie")] == "incremental"
    assert SCOPED_AXIS_DEFAULTS[("ds_build_mode", "TreeTrie")] == "serial"
```

In `test_build_mode_backfills_an_all_nan_float_column`, the last line
`assert pd.isna(out["ds_build_mode"].iloc[1])` becomes:

```python
    assert out["ds_build_mode"].iloc[1] == "serial"
```

Append to `tests/test_frame.py`:

```python
def test_threads_column_is_derived_from_the_build_mode(fixture_parallel_build_tree) -> None:
    df = load(fixture_parallel_build_tree["paths"], fixture_parallel_build_tree["criterion_root"])
    assert str(df["threads"].dtype) == "Int64"
    by_mode = df.groupby("ds_build_mode")["threads"]
    assert by_mode.apply(lambda t: t.isna().all())["serial"]
    assert by_mode.apply(lambda t: t.isna().all())["bulk"]
    assert set(by_mode.first().dropna()) == {2, 4}
    assert (df.loc[df["ds_build_mode"] == "parallel:4", "threads"] == 4).all()
```

Append to `tests/test_analysis.py` (it needs `import math`, `import pandas
as pd`, `import pytest` and `from kermit_lab.analysis import compare,
speedup_table`; add whichever it lacks):

```python
def _build_mode_rows(arms: dict[str, list[float]]) -> pd.DataFrame:
    """One summary row per run: TreeTrie insertion on one relation, under each
    build mode in ``arms`` (mode -> one mean per replicate)."""
    rows = []
    for mode, times in arms.items():
        threads = int(mode.removeprefix("parallel:")) if mode.startswith("parallel:") else pd.NA
        for run, t in enumerate(times):
            rows.append({
                "kind": "ds", "metric": "time", "phase": "insertion",
                "data_structure": "TreeTrie", "relation_path": "r.parquet",
                "ds_build_mode": mode, "threads": threads,
                "mean_ns": t, "mean_lo": t * 0.99, "mean_hi": t * 1.01,
                "source_path": f"{mode}-{run}.json", "criterion_group": f"{mode}-{run}",
                "criterion_function": "TreeTrie/insertion",
            })
    df = pd.DataFrame(rows)
    df["threads"] = df["threads"].astype("Int64")
    return df


def test_speedup_table_reports_speedup_efficiency_and_karp_flatt() -> None:
    df = _build_mode_rows({"serial": [100.0], "parallel:2": [60.0], "parallel:4": [40.0]})
    table = speedup_table(df).set_index("threads")
    assert table.loc[2, "speedup"] == pytest.approx(100 / 60)
    assert table.loc[4, "efficiency"] == pytest.approx(100 / 40 / 4)
    # e = (1/S - 1/N) / (1 - 1/N); S = 2.5, N = 4: (0.4 - 0.25) / 0.75 = 0.2.
    assert table.loc[4, "karp_flatt"] == pytest.approx(0.2)
    # One run per arm: the CI is the envelope of Criterion's own intervals.
    assert table.loc[2, "speedup_lo"] == pytest.approx(99 / 60.6)
    assert table.loc[2, "speedup_hi"] == pytest.approx(101 / 59.4)
    assert table["baseline_runs"].tolist() == [1, 1]


def test_speedup_table_bootstraps_replicates() -> None:
    df = _build_mode_rows({"serial": [100.0, 104.0, 98.0], "parallel:2": [50.0, 52.0, 51.0]})
    row = speedup_table(df).iloc[0]
    assert (row["baseline_runs"], row["runs"]) == (3, 3)
    assert row["speedup"] == pytest.approx((100 + 104 + 98) / (50 + 52 + 51))
    assert row["speedup_lo"] <= row["speedup"] <= row["speedup_hi"]


def test_speedup_table_has_no_karp_flatt_at_one_thread() -> None:
    df = _build_mode_rows({"serial": [100.0], "parallel:1": [110.0]})
    row = speedup_table(df).iloc[0]
    assert row["threads"] == 1
    assert row["speedup"] == pytest.approx(100 / 110)
    assert math.isnan(row["karp_flatt"])


def test_speedup_table_needs_a_serial_baseline() -> None:
    with pytest.raises(ValueError, match="no case"):
        speedup_table(_build_mode_rows({"parallel:2": [50.0]}))


def test_compare_pairs_build_modes_despite_the_derived_threads_column() -> None:
    df = _build_mode_rows({"serial": [100.0], "parallel:2": [50.0]})
    out = compare(df, baseline="serial", target="parallel:2", group_by="ds_build_mode")
    assert out["speedup"].tolist() == pytest.approx([2.0])
```

Append to `tests/test_presets.py`:

```python
def test_speedup(fixture_parallel_build_tree) -> None:
    df = load(fixture_parallel_build_tree["paths"], fixture_parallel_build_tree["criterion_root"])
    fig = presets.speedup(df)
    labels = {label for ax in fig.axes for label in ax.get_legend_handles_labels()[1]}
    plt.close(fig)
    assert isinstance(fig, Figure)
    assert {"TreeTrie", "ideal"} <= labels


def test_speedup_refuses_phases_a_build_mode_cannot_affect(fixture_parallel_build_tree) -> None:
    df = load(fixture_parallel_build_tree["paths"], fixture_parallel_build_tree["criterion_root"])
    with pytest.raises(InsufficientAxesError, match="built"):
        presets.speedup(df, phase="iteration")


def test_speedup_without_a_serial_baseline_is_insufficient_axes(fixture_build_mode_tree) -> None:
    df = load(fixture_build_mode_tree["paths"], fixture_build_mode_tree["criterion_root"])
    with pytest.raises(InsufficientAxesError, match="no case"):
        presets.speedup(df)
```

Append to `tests/test_cli.py`:

```python
def test_speedup_preset_subcommand(fixture_parallel_build_tree, tmp_path: Path) -> None:
    out = tmp_path / "speedup.pdf"
    rc = main([
        "speedup", *[str(p) for p in fixture_parallel_build_tree["paths"]],
        "--criterion-root", str(fixture_parallel_build_tree["criterion_root"]),
        "--out", str(out),
    ])
    assert rc == 0
    assert out.exists() and out.stat().st_size > 0


def test_speedup_subcommand_refuses_search_phases() -> None:
    with pytest.raises(SystemExit):
        _build_parser().parse_args(["speedup", "r.json", "--out", "s.pdf", "--phase", "iteration"])
```

In `tests/test_public_api.py`, add `"speedup", "speedup_table"` to the
tuple of names in `test_public_names`.

In `tests/test_render_all.py`, replace
`test_build_mode_ablation_leaves_out_structures_without_the_axis` by:

```python
def test_build_mode_ablation_leaves_out_structures_without_the_axis(
    fixture_build_mode_tree,
) -> None:
    """HashTrie rows keep NaN for ``ds_build_mode`` (it has no build mode
    until #94's second plan); charting them would add a bar for a structure
    that has none."""
    df = kl.load(
        fixture_build_mode_tree["paths"],
        criterion_root=fixture_build_mode_tree["criterion_root"],
    )
    assert df.loc[df["data_structure"] == "HashTrie", "ds_build_mode"].isna().all()
    fig = presets.ablation(df, axis="ds_build_mode", phase="insertion")
    labels = {t.get_text() for ax in fig.axes for t in ax.get_xticklabels()}
    plt.close(fig)
    assert labels == {"bulk", "incremental"}
```

Append to `tests/test_contract.py`:

```python
def test_bench_ds_tree_trie_reports_its_parallel_build(tmp_path: Path) -> None:
    report = tmp_path / "ds.json"
    _run(
        tmp_path, report,
        "ds", "--relation", str(FIXTURES / "edge.csv"), "-i", "tree-trie", "-m", "space",
        "--ds-build", "parallel:2",
    )
    # Without the back-fill, so a missing key cannot pass as "serial".
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion", apply_defaults=False)
    assert len(df) == 1
    assert df.iloc[0]["ds_build_mode"] == "parallel:2"
    assert df.iloc[0]["threads"] == 2
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
uv --directory $WT/python/kermit-lab run pytest -q
```

Expected failures:
- the tests that import `speedup_table`, and the speedup preset tests, do
  not collect or fail;
- the `threads` and `serial` back-fill tests fail;
- the render-all test fails, because the fixture's HashTrie row is new
  but nothing else changed yet. It passes once Step 3 is in.

- [ ] **Step 3: Implement**

In `kermit_lab/defaults.py`, add the TreeTrie entry to
`SCOPED_AXIS_DEFAULTS`, right after the ColumnTrie `ds_build_mode` entry:

```python
    # TreeTrie built serially until #94 added `--ds-build parallel:N`. Every
    # TreeTrie report since carries the axis ("serial" by default).
    ("ds_build_mode", "TreeTrie"): "serial",
```

In `kermit_lab/frame.py`, add this function before `_summary_from_reports`:

```python
def threads_of(build_mode: object) -> int | None:
    """The thread count of a ``parallel:N`` build mode, else ``None``.

    ``serial``, ColumnTrie's ``bulk`` / ``incremental`` and a missing mode
    are not thread counts, so they become ``<NA>`` in the ``threads`` column.
    """
    if isinstance(build_mode, str) and build_mode.startswith("parallel:"):
        return int(build_mode.removeprefix("parallel:"))
    return None
```

and at the end of `_summary_from_reports`, replace

```python
    if apply_defaults:
        df = apply_axis_defaults(df)
    return df
```

with

```python
    if apply_defaults:
        df = apply_axis_defaults(df)
    if "ds_build_mode" in df.columns:
        # Derived, not reported: `kl.speedup` and line plots want the thread
        # count as a number (#94).
        df.insert(
            df.columns.get_loc("ds_build_mode") + 1,
            "threads",
            df["ds_build_mode"].map(threads_of).astype("Int64"),
        )
    return df
```

In `kermit_lab/analysis.py`, add after `_VALUE_FAMILY`:

```python
# Columns `kl.load` derives from another column. `threads` varies exactly when
# `ds_build_mode` does, so it is never a key that pairs rows.
_DERIVED_COLS: frozenset[str] = frozenset({"threads"})
```

and change `compare`'s `join_keys` to skip them:

```python
    join_keys = [
        c for c in df.columns
        if c != group_by and c not in _PROVENANCE_COLS and c not in _VALUE_FAMILY
        and c not in _DERIVED_COLS
    ]
```

Then append to `kermit_lab/analysis.py`:

```python
# The columns that tell runs of one case apart, or that hold its numbers:
# everything else identifies the case.
_SPEEDUP_NON_KEYS: frozenset[str] = (
    _PROVENANCE_COLS | _VALUE_FAMILY | frozenset({"ds_build_mode", "threads"})
)


def speedup_table(
    df: pd.DataFrame,
    *,
    phase: str = "insertion",
    baseline: str = "serial",
    value: str = "mean_ns",
    n_resamples: int = 9999,
    rng: int | np.random.Generator | None = 0,
) -> pd.DataFrame:
    """Speedup of every ``parallel:N`` build over the ``baseline`` build.

    A *case* is everything a row says apart from its build mode and
    provenance: one structure, workload and relation, measured under several
    build modes. Replicates of one case and mode (one report each, told apart
    by ``criterion_group`` / ``source_path``) are pooled. Load one binary's
    reports only, or codegen drift between binaries enters the speedup.

    One row per case and thread count ``N``, with the case's columns and:

    - ``speedup``: mean baseline ``value`` over mean ``parallel:N`` ``value``;
      above 1 means the parallel build is faster;
    - ``speedup_lo`` / ``speedup_hi``: a percentile-bootstrap CI over the
      replicates when both sides have at least two, else the conservative
      envelope of Criterion's own CIs, as in :func:`compare`;
    - ``efficiency``: ``speedup / N``;
    - ``karp_flatt``: the experimentally determined serial fraction
      ``(1/speedup - 1/N) / (1 - 1/N)``, NaN at ``N = 1``. Flat across ``N``
      means a fixed sequential share limits the build; rising means a cost
      that grows with ``N`` does;
    - ``baseline_runs`` / ``runs``: the replicates pooled on each side.

    Raises ``ValueError`` when a needed column is missing or no case has both
    a baseline row and a ``parallel:N`` row on ``phase``.
    """
    value_lo, value_hi = _ci_columns_for(value)
    needed = ["metric", "phase", "ds_build_mode", "threads", value, value_lo, value_hi]
    missing = [c for c in needed if c not in df.columns]
    if missing:
        raise ValueError(f"speedup_table needs columns {missing}")
    on_phase = ((df["metric"] == "time") & (df["phase"] == phase)).fillna(False).astype(bool)
    rows = df[on_phase & df["ds_build_mode"].notna()]
    case_keys = [c for c in rows.columns if c not in _SPEEDUP_NON_KEYS]

    records: list[dict] = []
    for key, case in rows.groupby(case_keys, dropna=False, sort=True):
        is_base = case["ds_build_mode"] == baseline
        base = case.loc[is_base, value]
        if base.empty:
            continue
        identity = dict(zip(case_keys, key if isinstance(key, tuple) else (key,)))
        for threads, arm in case[case["threads"].notna()].groupby("threads", sort=True):
            n = int(threads)
            speedup = base.mean() / arm[value].mean()
            if len(base) >= 2 and len(arm) >= 2:
                lo, hi = bootstrap_ratio_ci(base, arm[value], n_resamples=n_resamples, rng=rng)
            else:
                lo = case.loc[is_base, value_lo].mean() / arm[value_hi].mean()
                hi = case.loc[is_base, value_hi].mean() / arm[value_lo].mean()
            records.append({
                **identity,
                "threads": n,
                "speedup": speedup,
                "speedup_lo": lo,
                "speedup_hi": hi,
                "efficiency": speedup / n,
                "karp_flatt": (1 / speedup - 1 / n) / (1 - 1 / n) if n > 1 else float("nan"),
                "baseline_runs": len(base),
                "runs": len(arm),
            })
    if not records:
        raise ValueError(
            f"no case has both a {baseline!r} row and a parallel:N row on {phase!r}"
        )
    return pd.DataFrame.from_records(records)
```

In `kermit_lab/presets.py`, add the imports:

```python
from .analysis import speedup_table
from .facet import finish, make_grid
from .styles import apply as apply_style
```

and append after `ablation`:

```python
# The columns of a speedup table that measure, rather than identify, a case.
_SPEEDUP_MEASURES: frozenset[str] = frozenset({
    "threads", "speedup", "speedup_lo", "speedup_hi", "efficiency", "karp_flatt",
    "baseline_runs", "runs",
})


def speedup(df: pd.DataFrame, *, phase: str = "insertion", out: Optional[Path] = None) -> Figure:
    """Build speedup over the serial build against thread count (#94).

    One line per case of :func:`~kermit_lab.analysis.speedup_table`, with its
    CI as a band, and the ideal ``speedup = N`` dashed. Load one binary's
    reports only (see :func:`~kermit_lab.analysis.speedup_table`).

    Raises :class:`InsufficientAxesError` for a phase no build mode can
    affect, or when no case has both a serial and a ``parallel:N`` row.
    """
    scope = AXIS_PHASES["ds_build_mode"]
    if phase not in scope.phases:
        allowed = " or ".join(repr(p) for p in sorted(scope.phases))
        raise InsufficientAxesError(
            f"ds_build_mode cannot affect phase {phase!r}: {scope.reason}; plot it on {allowed}"
        )
    try:
        table = speedup_table(df, phase=phase)
    except ValueError as e:
        raise InsufficientAxesError(str(e)) from e

    apply_style()
    fig, axes = make_grid(1)
    ax = axes[0]
    identity = [c for c in table.columns if c not in _SPEEDUP_MEASURES]
    varying = [c for c in identity if table[c].nunique(dropna=False) > 1] or ["data_structure"]
    for key, line in table.groupby(varying, dropna=False, sort=True):
        line = line.sort_values("threads")
        parts = key if isinstance(key, tuple) else (key,)
        label = " / ".join(str(p) for p in parts if not pd.isna(p))
        ax.plot(line["threads"], line["speedup"], marker="o", label=label)
        ax.fill_between(line["threads"], line["speedup_lo"], line["speedup_hi"], alpha=0.2)
    top = int(table["threads"].max())
    ax.plot([1, top], [1, top], linestyle="--", color="grey", label="ideal")
    ax.set_xscale("log", base=2)
    ax.set_xlabel("threads")
    ax.set_ylabel(f"speedup over serial ({phase})")
    finish(fig, axes, title="Parallel build speedup", out=out)
    return fig
```

In `kermit_lab/__init__.py`:
- import `speedup_table` beside the other analysis names:
  `from .analysis import bootstrap_ratio_ci, compare, mannwhitney_u, speedup_table, summary`;
- add `speedup` to the `from .presets import ( … )` list;
- add `"speedup"` and `"speedup_table"` to `__all__`, in alphabetical order.

In `kermit_lab/drivers/main.py`, register the subcommand after the
`ablation` parser in `_build_parser`:

```python
    p_speedup = sub.add_parser("speedup", help="parallel-build speedup over serial vs threads")
    _add_common(p_speedup)
    p_speedup.add_argument("--phase", choices=["insertion", "end_to_end"], default="insertion",
                           help="build phase to compare (default: insertion)")
```

and dispatch it in `_dispatch`, before the final `else`:

```python
    elif args.command == "speedup":
        fig = presets.speedup(df, phase=args.phase, out=args.out)
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
uv --directory $WT/python/kermit-lab run pytest -q
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit
KERMIT_BIN=$WT/target/debug/kermit uv --directory $WT/python/kermit-lab run pytest -q tests/test_contract.py
```

Expected: all pass. The contract run includes
`test_bench_ds_tree_trie_reports_its_parallel_build`.

- [ ] **Step 5: Commit, then run the mutation check**

```bash
git -C $WT add python/kermit-lab/kermit_lab/defaults.py python/kermit-lab/kermit_lab/frame.py python/kermit-lab/kermit_lab/analysis.py python/kermit-lab/kermit_lab/presets.py python/kermit-lab/kermit_lab/__init__.py python/kermit-lab/kermit_lab/drivers/main.py python/kermit-lab/tests/conftest.py python/kermit-lab/tests/test_defaults.py python/kermit-lab/tests/test_frame.py python/kermit-lab/tests/test_analysis.py python/kermit-lab/tests/test_presets.py python/kermit-lab/tests/test_cli.py python/kermit-lab/tests/test_public_api.py python/kermit-lab/tests/test_render_all.py python/kermit-lab/tests/test_contract.py
git -C $WT commit -F - <<'EOF'
feat(kermit-lab): threads column and speedup preset for parallel builds (#94)

TreeTrie rows without `ds_build_mode` back-fill as "serial". `kl.load`
derives a numeric `threads` column from `parallel:N`, which `compare` no
longer treats as a pairing key. `speedup_table` reports speedup and
efficiency per thread count, with bootstrap CIs over replicates and the
Karp-Flatt serial fraction; `kl.speedup` and `kermit-lab speedup` plot it.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EC25BmBrMyFPmp424HLMDd
EOF
```

Mutation check: in `speedup_table`, change the `karp_flatt` expression's
`(1 / speedup - 1 / n)` to `(1 / speedup + 1 / n)`.
`test_speedup_table_reports_speedup_efficiency_and_karp_flatt` must fail.

---

## Task 8 [P3]: Document TreeTrie's parallel build

**Files:**
- Create: `docs/data-structures/parallel-build.md`
- Modify: `docs/data-structures/tree-trie.md`, `docs/specs/optimization-standard.md`, `docs/specs/bench-report-schema.md`, `BENCHMARKING.md`, `ARCHITECTURE.md`, `CLAUDE.md`

- [ ] **Step 1: Write `docs/data-structures/parallel-build.md`**

````markdown
# Parallel builds (`--ds-build parallel:N`)

`TreeTrie` can be built on several threads (issue #94). The build is
morsel-driven in the sense of Leis et al. (*Morsel-Driven Parallelism*,
SIGMOD 2014): the input is cut into small morsels, and whichever thread is
free takes the next unit of work. It produces exactly the trie the serial
build produces, so only the build-timing metrics (`insertion`,
`end_to_end`) can move.

| Mode | `--ds-build` | `ds_build_mode` | Default |
|---|---|---|---|
| `TreeTrieBuildMode::Serial` | `serial` | `"serial"` | ✓ |
| `TreeTrieBuildMode::Parallel(n)` | `parallel:N` | `"parallel:N"` | |

`N` counts every thread the build uses, the calling one included. So
`parallel:1` runs the parallel code on one thread, and against `serial` it
measures what partitioning costs, net of one saving: sorting P partitions
takes about n·log₂P fewer comparisons than one sort of everything, so
`parallel:1` can beat `serial`. N ranges from 1 to 1024 (`Threads::MAX`).

The shared steps live in `kermit-ds/src/morsel.rs` (`scatter`, `dispatch`).
The TreeTrie build is `TreeTrie::build_parallel` in
`kermit-ds/src/ds/tree_trie/implementation.rs`.

## The three steps

```text
parallel_build(tuples, N):
    check arities                                   // the serial checks, on the caller
    splitters = quantiles of a sample of first keys  // duplicates kept; aiming at P = 4·N partitions
    partitions = scatter(tuples, morsels of 16 384)  // step 1, N workers
        // tuple t goes to partition_point(splitters, s <= t[0])
    built = dispatch(partitions):                    // step 2, N workers
        sort the partition; insert its tuples one at a time
        -> (its top-level nodes, its distinct tuples)
    root = []                                        // step 3, the caller
    for (nodes, count) in built, in key order:
        push each node onto root, one at a time
```

- **Partition.** Workers take morsels from a mutex-guarded queue and move
  each tuple, without copying it, into its partition's bucket for that
  morsel. A partition is its buckets in morsel order, so it lists its tuples
  in input order.
- **Build.** Workers take whole partitions from the same kind of queue.
  There are more partitions than threads, so a worker that drew a small
  partition takes another.
- **Assemble.** The calling thread pushes each partition's top-level nodes
  onto the root, in partition order, which is key order.

## Invariant: the parallel trie is the serial trie

Every `parallel:N` build is identical to the serial build of the same
tuples: the same nodes, the same `Vec` capacities, the same `tuple_count`
and `heap_size_bytes`. `parallel_builds_are_identical_to_serial` pins it for
N ∈ {1, 2, 3, 8}.

- Splitters are first keys, so all tuples sharing a first key share a
  partition, and the partitions are first-key ranges, in order.
- Sorted order restricted to a key range is that range sorted, so each
  subtree receives exactly the serial build's sequence of inserts.
- A child list grows one insert at a time in both builds, and `Vec::insert`
  grows a list exactly as `push` does, so capacities match. In fact a
  TreeTrie, capacities included, depends only on its distinct tuples. The
  per-partition sort is there for speed (every insert then appends), not
  for identity.
- The root grows by one push per first key, as the serial build's
  insert-at-the-end does. `extend` or `append` would reserve in bulk and
  break this, so the assemble step pushes.

The splitters, the partition count and the morsel size only decide how work
is spread. None of them can change the trie.

## Complexity

With `n` tuples of arity `a`, `k` distinct first keys and `N` threads:

| Step | Work | Runs on |
|---|---|---|
| Checks and sampling | O(n · a), plus sorting a sample of at most 128 keys per partition | the calling thread |
| Partition | O(n log P) | N workers |
| Build | O(n · a · log n): sorting and inserting, split across partitions | N workers |
| Assemble | O(k) moves | the calling thread |

The sequential share is the checks, the sampling and the assemble step.
With few tuples per first key (`k` close to `n`), the assemble step is a
larger share and the speedup falls. The sample keeps duplicate keys, so the
splitters share out tuples rather than distinct keys. Still, a key's tuples
cannot be split: one dominant first key fills one partition, which caps the
build step at one worker's speed.

## Worked micro-example

`parallel:2` over `[3,1] [1,2] [2,9] [1,1]` aims at 8 partitions. The sorted
sample is `1 1 2 3`; its quantiles for 8 partitions are `1 1 1 2 2 3 3`,
which merge to the splitters `[1, 2, 3]`, so there are four partitions:

| Partition | First keys | Tuples (input order) | After sorting and inserting |
|---|---|---|---|
| 0 | `< 1` | — | — |
| 1 | `1` | `[1,2] [1,1]` | `1 → {1, 2}` |
| 2 | `2` | `[2,9]` | `2 → {9}` |
| 3 | `≥ 3` | `[3,1]` | `3 → {1}` |

Pushing the nodes of partitions 1, 2 and 3 in order gives the root `1, 2, 3`:
the trie `from_tuples` builds from the same input.

## Measuring

See `BENCHMARKING.md`, "Scaling: measuring a parallel build", and kermit-lab's
`kl.speedup_table` / `kermit-lab speedup`. Compare build modes within one
binary.

## See also

- [`TreeTrie`](./tree-trie.md), whose Optimizations table lists the mode.
- The design: [`docs/specs/2026-10-05-parallel-build-design.md`](../specs/2026-10-05-parallel-build-design.md).
````

- [ ] **Step 2: Update `docs/data-structures/tree-trie.md`**

In "## Optimizations", add a row to the table and a sentence after the
seek strategy's sentence. The section becomes:

```markdown
## Optimizations

| Dimension | Category | Axis | Flag | Default | Test aliases |
|---|---|---|---|---|---|
| Seek strategy | Layout (`S: SeekStrategy`) | `ds_layout_seek` | `--ds-layout-seek linear\|binary\|galloping` | `galloping` | `TreeTrieLinear`, `TreeTrieBinary`, `TreeTrieGalloping` |
| Build | BuildMode (`TreeTrieBuildMode`) | `ds_build_mode` | `--ds-build serial\|parallel:N` | `serial` | `TreeTrieParallel2` (`BuiltWith<TreeTrie, Parallel2>`) |

The strategy changes only how `seek` searches; it changes no stored data, no build and no `heap_size_bytes`. Details: [seek strategies](seek-strategies.md).

The build mode changes only how long the build takes: `parallel:N` builds the identical trie, down to every `Vec`'s capacity, on N threads. Details: [parallel builds](parallel-build.md).
```

- [ ] **Step 3: Update `docs/specs/optimization-standard.md`**

Make three edits.

1. In the BuildMode aspect table, replace
   `| Examples (potential) | ColumnTrie bulk / incremental ✓, parallel build, radix partitioning |`
   with
   `| Examples (potential) | ColumnTrie bulk / incremental ✓, TreeTrie serial / parallel:N ✓, HashTrie parallel build, radix partitioning |`.
2. In the implemented catalogue, replace
   `Five optimizations are implemented — three Layout dimensions, one Config`
   / `value and one BuildMode:` with `Six optimizations are implemented —
   three Layout dimensions, one Config` / `value and two BuildModes:`. Then
   add this row under the ColumnTrie build row:
   `| TreeTrie build (serial / parallel:N) | BuildMode | \`ds_build_mode\` | §3.3.2 (morsel-driven; issue #94) |`
3. In "Available to add", replace `| Parallel build | BuildMode | Large | §3.3.2 |`
   with `| Parallel build (HashTrie; TreeTrie's landed with #94) | BuildMode | Large | §3.3.2 |`.

- [ ] **Step 4: Update `docs/specs/bench-report-schema.md`**

Make three edits.

1. The `ds_build_mode` bullet becomes:

```markdown
- `ds_build_mode` — construction-time build mode for the data structure
  (single key; value is a `<mode>[:<params>]` string, e.g., `bulk`). Emitted
  on ColumnTrie and TreeTrie reports, by the bench family that ran the build
  rather than by the relation, since every build mode builds the same
  structure. ColumnTrie values: `bulk` (default) and `incremental`
  (`--ds-build incremental`). TreeTrie values: `serial` (default) and
  `parallel:N` (`--ds-build parallel:N`, N threads).
```

2. In the back-fill paragraph, replace
   `report since carries the axis). \`data_structure\` has named HashTrie as`
   with:

```markdown
  report since carries the axis), and, on TreeTrie rows, `ds_build_mode ==
  "serial"` (TreeTrie's only build before issue #94). `data_structure` has named HashTrie as
```

3. Append a row at the end of the version table. Use the date the change
   lands (`date +%F`) if that is not 2026-10-05:

```markdown
| 3 (no bump) | 2026-10-05 | TreeTrie reports carry `ds_build_mode` (#94): `serial` (the default, the build every earlier TreeTrie report ran) or `parallel:N`. Every mode builds the identical trie, so `iteration` and `space` cannot move and `schema_version` stays `3`. kermit-lab back-fills `serial` on earlier TreeTrie rows and derives a numeric `threads` column. Compare build modes within one binary. |
```

- [ ] **Step 5: Add a scaling section to `BENCHMARKING.md`**

Insert this immediately before the paragraph that starts `The full
optimization model (Layout / Config / BuildMode, how to add one, and`:

````markdown
### Scaling: measuring a parallel build

`--ds-build parallel:N` builds a `TreeTrie` on N threads (#94). The trie is
identical to the serial build's, so only `insertion` and `end_to_end` can
move. Run one arm per thread count within one binary, each with its own
`--name`, then plot the speedup over `serial`:

```sh
for mode in serial parallel:1 parallel:2 parallel:4 parallel:8 parallel:16; do
  name="tt-${mode/:/-}"
  kermit bench --name "$name" --report-json "bench-runs/$name.json" \
    ds -r data.parquet -i tree-trie -m insertion --ds-build "$mode"
done
uv --directory python/kermit-lab run kermit-lab speedup "$PWD"/bench-runs/tt-*.json \
  --criterion-root "$PWD"/target/criterion --out "$PWD"/speedup.pdf
```

`kl.speedup_table(df)` gives the numbers behind the plot: speedup and
efficiency per thread count, with CIs, and the Karp–Flatt serial fraction.
A flat Karp–Flatt fraction means a fixed sequential share (the assemble
step) limits the build; a rising one means a cost that grows with N, such as
allocator contention. Replicates (one report per run, each with its own
`--name`) are pooled per arm and give bootstrap CIs once each arm has two.
`parallel:1` against `serial` is the cost of partitioning net of one
saving (P smaller sorts take about n·log₂P fewer comparisons than one big
one), so `parallel:1` can come out ahead.
`N = 16` on an 8-core host measures SMT, not more cores. The protocol behind
reported numbers is in
[`docs/specs/2026-10-05-parallel-build-design.md`](docs/specs/2026-10-05-parallel-build-design.md).

````

- [ ] **Step 6: Update `ARCHITECTURE.md`**

Make four edits.

1. Replace `SortedTrie::TreeTrie { seek } \| ColumnTrie` with
   `SortedTrie::TreeTrie { seek, build } \| ColumnTrie` (the bench-cell row
   of the families table).
2. Replace
   ``` `ColumnTrie`'s BuildMode axis, `ds_build_mode`, is reported by its bench family instead, because the built trie is the same under every mode.```
   with
   ```The sorted tries' BuildMode axis, `ds_build_mode` (`ColumnTrie`: `bulk` / `incremental`; `TreeTrie`: `serial` / `parallel:N`), is reported by their bench family instead, because the built trie is the same under every mode.```
3. Replace
   ``` `TrieLftj(SortedTrie::TreeTrie { seek } | ColumnTrie { seek, build })` (`seek` carries the `--ds-layout-seek` strategy, and ColumnTrie's `build` the `--ds-build` mode)```
   with
   ``` `TrieLftj(SortedTrie::TreeTrie { seek, build } | ColumnTrie { seek, build })` (`seek` carries the `--ds-layout-seek` strategy, and `build` the structure's `--ds-build` mode)```
4. Replace
   ``` `SortedTrieFamily<R>` likewise carries ColumnTrie's `--ds-build` mode (`R::BuildMode`, `()` for TreeTrie), builds through it```
   with
   ``` `SortedTrieFamily<R>` likewise carries the `--ds-build` mode (`R::BuildMode`: `ColumnTrieBuildMode` or `TreeTrieBuildMode`), builds through it```

- [ ] **Step 7: Update `CLAUDE.md`**

Make seven edits.

1. Build Commands: after the line ending `--ds-build incremental  # BuildMode axis (the pre-#84 build)`, add:
   ```
   cargo run -- bench run triangle -i tree-trie -a leapfrog-triejoin --ds-build parallel:8  # BuildMode axis: TreeTrie's morsel-driven parallel build (#94)
   ```
2. Priority 1: replace `see the \`Incremental\` ColumnTrie precedent in \`kermit/tests/join_tests.rs\`` with `see the \`Incremental\` ColumnTrie and \`Parallel2\` TreeTrie precedents in \`kermit/tests/join_tests.rs\``.
3. Workspace Architecture, `kermit-ds`: replace `SeekStrategy Layout (seek.rs).` with `SeekStrategy Layout (seek.rs). morsel.rs holds the\n                  morsel-driven partition and build steps of the parallel builds.`, keeping the column alignment of the surrounding lines.
4. Key Trait Hierarchy, `BuildModeRelation`: replace `implemented only by \`ColumnTrie\` (\`ColumnTrieBuildMode\`: \`bulk\` default, \`incremental\` = the pre-#84 sort-then-insert build).` with `implemented by \`ColumnTrie\` (\`ColumnTrieBuildMode\`: \`bulk\` default, \`incremental\` = the pre-#84 sort-then-insert build) and \`TreeTrie\` (\`TreeTrieBuildMode\`: \`serial\` default, \`parallel:N\` = the morsel-driven build on N threads, #94).`
5. Adding an optimization, step 4: replace `which structure has which flag's axis), which both that check and` with `which structure has which flag's axis; \`--ds-build\`'s row depends on the value, \`BuildChoice::structures\`), which both that check and`.
6. Gotchas, `bench run` sweeps: replace `TrieLftj(SortedTrie::TreeTrie { seek } | ColumnTrie { seek, build })` with `TrieLftj(SortedTrie::TreeTrie { seek, build } | ColumnTrie { seek, build })`.
7. Component Reference Docs: after the `seek-strategies.md` line, add:
   ```
   - `docs/data-structures/parallel-build.md` — the morsel-driven parallel build (`--ds-build parallel:N`; `TreeTrie`).
   ```

- [ ] **Step 8: Verify and commit**

```bash
RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo doc --workspace --no-deps 2>&1 | tail -2
grep -c "TreeTrie { seek }" $WT/CLAUDE.md $WT/ARCHITECTURE.md
```

Expected: `cargo doc` finishes without warnings, and both grep counts are
`0`.

```bash
git -C $WT add docs/data-structures/parallel-build.md docs/data-structures/tree-trie.md docs/specs/optimization-standard.md docs/specs/bench-report-schema.md BENCHMARKING.md ARCHITECTURE.md CLAUDE.md
git -C $WT commit -F - <<'EOF'
docs: TreeTrie's parallel build (#94)

A shared parallel-build page (the three steps, the identity invariant,
complexity and a worked example), TreeTrie's Optimizations row, the
catalogue and report-schema entries, a BENCHMARKING.md scaling section, and
the CLAUDE.md / ARCHITECTURE.md references to the TreeTrie cell's new
`build` field.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EC25BmBrMyFPmp424HLMDd
EOF
```

---

# Controller

## Task 9 [controller]: Final gate, smoke run, checkpoint 2

**Files:** none modified.

- [ ] **Step 1: The serial bodies are untouched**

```bash
git -C $WT diff "$(cat $SCRATCH/base.sha)" -- kermit-ds/src/ds/tree_trie/implementation.rs | grep -E '^-' | grep -v '^---'
```

Expected: only the replaced `use` block's lines. No line of `from_tuples`,
`insert` or `insert_into_children` may appear.

- [ ] **Step 2: The CI gate, locally**

```bash
nix develop $WT --command cargo fmt --all --check
RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}'
RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo doc --workspace --no-deps
MIRIFLAGS="-Zmiri-disable-isolation" CARGO_BUILD_JOBS=2 nix develop $WT --command cargo miri test -p kermit-ds
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit
KERMIT_BIN=$WT/target/debug/kermit uv --directory $WT/python/kermit-lab run pytest -q
```

Expected:
- every command succeeds;
- 0 failed;
- the passed count exceeds Task 0's by the tests this plan added.

Miri on `kermit-ds` can take longer than 10 minutes. If it does, run it
detached (`setsid nohup … & disown`) and poll its log.

- [ ] **Step 3: Smoke run (not reported anywhere)**

The data is a synthetic 2M-tuple relation in `$SCRATCH`, so nothing is
written to the shared cache.

```bash
awk 'BEGIN { srand(1); print "a,b,c"; for (i = 0; i < 2000000; i++) print int(rand() * 200000) "," int(rand() * 1000) "," int(rand() * 1000) }' > $SCRATCH/smoke.csv
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build --release -p kermit
for mode in serial parallel:1 parallel:8; do
  name="smoke-${mode/:/-}"
  env -C $SCRATCH $WT/target/release/kermit bench --sample-size 10 --name "$name" \
    --report-json "$SCRATCH/$name.json" \
    ds --relation $SCRATCH/smoke.csv -i tree-trie -m insertion --ds-build "$mode"
done
uv --directory $WT/python/kermit-lab run python -c "
import kermit_lab as kl
df = kl.load('$SCRATCH/smoke-*.json', criterion_root='$SCRATCH/target/criterion')
print(kl.speedup_table(df)[['threads', 'speedup', 'efficiency', 'karp_flatt']])
"
```

Expected: all three runs succeed. Then:
- `parallel:8`'s speedup is clearly above 1;
- `parallel:1`'s is near 1; it can exceed 1, since the per-partition sorts
  save comparisons.

If `parallel:8` is not faster, stop and report it at the checkpoint. The
scaling phase would then measure nothing worth having.

- [ ] **Step 4: Checkpoint 2 (controller → user)**

Report:
- the commit SHAs;
- the gate results;
- the test-count delta;
- the smoke table, labelled as a smoke run;
- any deviation from this plan.

## Task 10 [controller]: Hand-off (only on the user's instruction)

Nothing lands without the user's instruction. When it comes:
1. Fetch, and merge `origin/master` in (never rebase).
2. Re-run Task 9 Step 2.
3. Push `HEAD:master` as the user directs.
4. Comment on #94 with plan 1's commits.

#94 stays open for plan 2 (HashTrie) and the scaling measurement.
