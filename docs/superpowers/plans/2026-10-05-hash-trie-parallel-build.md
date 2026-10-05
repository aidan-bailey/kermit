# HashTrie Parallel Build (Plan 2 of #94) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `HashTrie` a `parallel:N` build mode, `--ds-build
hash-trie=parallel:N`, that builds exactly the serial trie on N threads.
Every HashTrie report records it as `ds_build_mode`, and kermit-lab's
`speedup` preset reads it unchanged.

**Architecture:** `parallel:N` is the radix build of #91 (`radix.rs`: partition
→ scratch roots → merge in first-appearance order) with its first two steps
run on N threads by plan 1's `morsel.rs` (`scatter`, `dispatch`). The merge
into the real root stays on the calling thread, as a k-way merge of the
partitions' already-ordered entry lists. There are four work packages; see
"Work packages".

- **P1** (Tasks 1–5), all in `kermit-ds`:
  - every Layout's nodes become `Send`;
  - the radix building blocks and its identity checks are shared;
  - `HashTrieBuildMode::Parallel(Threads)` and `parallel.rs`, with
    array-level identity tests, the test-hooks record, and parsing;
  - the structure-level suites.
- **P2** (Tasks 6–7), the `kermit` binary: the mode reaches the build on
  every route, the CLI accepts and reports it, and the join suites run it.
- **P3** (Tasks 8–9): kermit-lab tests and the docs.
- **Controller** (Tasks 0, 10, 11): merges, gates, a smoke run, the
  checkpoints, and landing (only on the user's instruction).

**Tech Stack:** Rust nightly workspace (std scoped threads, clap, Criterion,
`serde_json`), Python kermit-lab (pandas, pytest via uv), Nix dev shell.

**Spec:** [`docs/specs/2026-10-05-parallel-build-design.md`](../../specs/2026-10-05-parallel-build-design.md) ·
**Plan 1:** [`2026-10-05-tree-trie-parallel-build.md`](./2026-10-05-tree-trie-parallel-build.md) ·
**Issue:** #94

---

## Preconditions

- **Plan 1 has landed** (00bcc88, with the test-hooks follow-up d3c7945), and
  so have #91 (radix build, 596f218) and #92 (lazy expansion, 149614f). This
  branch starts at 48a403c or later.
- **The user decided the CLI shape on 2026-10-05:** a new
  `hash-trie=parallel:N` mode beside `serial` and `radix:K`, spelled like
  `tree-trie=parallel:N`. Internally it is the radix pipeline on threads,
  with the partition count set from N. It is neither `radix:K:N` nor
  `parallel:N:K`, and it takes no override.
- **Prototyped while planning (2026-10-05, then reverted).** Task 1's bounds
  checked across the whole workspace. Task 3's non-test code, with Task 2's
  visibility changes, passed clippy. Its identity, reach and empty-input
  tests all passed, under every hasher × pruning × expansion combination.
  Only the arity test failed, because the prototype skipped Task 2's
  `from_tuples_partitioned` check. If the code here does not compile as
  written, suspect a transcription slip before the design.
- **No measurement claims.** Task 10's smoke run checks that the parallel
  build is faster at all. Its numbers go nowhere but the checkpoint report.
  The HashTrie half of the scaling measurement is a separate step after this
  plan; see "After this plan".

## Where this plan departs from the spec

The spec predates #91 and #92. Its "Plan 2" bullets are partly done and
partly stale:

| Spec says | This plan does | Why |
|---|---|---|
| Add `HashTrieBuildMode` and an inherent `HashTrie::from_tuples_with(header, config, mode, tuples)` | Adds a `Parallel(Threads)` variant to the existing `HashTrieBuildMode`; the constructor is #91's `from_tuples_with_config_and_build_mode` | #91 landed both |
| Add `HashTable::into_slots` | Uses #91's `into_buckets` through `radix::take_in_arrival_order` | #91 landed the equivalent |
| Give `BuiltWith` its `HashTrieIterable` forwarding; add the `HashHtj` build field | Nothing to do | #91 landed both |
| "#91 reuses `scatter`" | `radix:K` keeps its own serial histogram partition; `parallel:N` uses `scatter` | #91 landed first, and changing radix's executed path would invalidate its recorded A/B |
| `OnceCell` is `Send`, so moving children works | The policies' associated types gain `Send` bounds (Task 1) | Generic code cannot prove `HashTrieNode<P, E>: Send` otherwise (checked 2026-10-05: the bounds compile workspace-wide) |
| Partition count "the next power of two at or above 4·N" | The same; empty partitions are skipped, as the radix build skips them | — |

The k-way merge (`BinaryHeap`) is as specified. The radix build sorts its
merge entries instead; that is left alone (Priority 6, and its A/B is on
record).

## Ground rules for every task

- **Paths.**
  - The worktree is
    `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/hash-trie-parallel_18dbb2f717cebe75`.
  - `SCRATCH` is the executing session's scratchpad directory.
  - **Never `cd`** in a Bash call. Use absolute paths, `env -C <dir>`,
    `git -C $WT` or `uv --directory`.
- **Cargo.** Run it **in the foreground** through the flake with
  `CARGO_BUILD_JOBS=2`, e.g. `CARGO_BUILD_JOBS=2 nix develop $WT --command
  cargo test -p kermit-ds`. A background memory monitor kills
  `run_in_background` cargo jobs. Anything longer than 10 minutes runs fully
  detached (`setsid nohup … & disown`), and you poll its log.
- **Formatting.** Format **only** with `nix develop $WT --command cargo fmt
  --all`, before every commit; stable rustfmt rewrites dozens of files. If
  CI's fmt later disagrees, report it: the fix (`nix flake update
  rust-overlay`) changes `flake.lock` and is the user's call.
- **Doc comments.** Backtick every identifier (clippy's `doc_markdown` runs
  under `-Dwarnings`).
- **Python.** `uv --directory $WT/python/kermit-lab run pytest -q`. The first
  run needs `uv --directory $WT/python/kermit-lab sync --group test`.
- **Commits.**
  - Plain conventional commits referencing `(#94)`. Never amend, never push.
  - Stage files by name; new files are marked intent-to-add.
  - Every commit message ends with these two lines. An executing session
    substitutes its own `Claude-Session` line.
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky
    ```
- **Mutation checks.**
  1. Commit first.
  2. Apply the mutant with an exact edit.
  3. Confirm the named test fails **and** that the mutant applied:
     `git -C $WT diff` shows it.
  4. Revert by reversing the exact edit, never with `git checkout`.
  5. Re-run the test and see it pass. `git -C $WT status` must then be clean.
- **Scope (Priority 6).**
  - Do **not** change TreeTrie, ColumnTrie, any algorithm or any optimiser.
    `morsel.rs` changes by one comment (Task 3), nothing else.
  - The serial bodies of `HashTrie::from_tuples_with_config`, `insert`,
    `insert_at` and `expand_level` stay byte-identical.
  - The radix build's executed path stays the same: Task 2 changes
    visibility and one parameter type, nothing that runs differently.
    Task 10 checks both with a diff.
- **Shared bench cache.** Never write under `~/.cache/kermit` (no `--force`,
  `bench gen` or `bench clean`). Nothing in this plan needs a generated
  benchmark.
- **Checkpoints with the user:**
  1. after P1: identity holds and Miri is clean;
  2. after Task 10: the gate is green and the smoke numbers are in;
  3. before any merge or push.

---

## Work packages

One implementer agent executes each package end to end: its tasks in
order, one commit per task, mutation checks included. The package is then
reviewed before the next starts. The controller keeps Tasks 0, 10 and 11,
and **merges `origin/master` at every package boundary** (plan 1 met 16
conflicts by merging only at the end).

| Package | Tasks | Scope | Depends on | Commits | Done when |
|---|---|---|---|---|---|
| *Controller* | 0 | Merge `origin/master`, record `BASE` and the baseline counts in a file | — | merge only | `$SCRATCH/baseline.txt` written; workspace tests and kermit-lab pytest green |
| **P1 — `kermit-ds`** | 1–5 | `Send` bounds; shared radix steps; `Parallel(Threads)` and `parallel.rs`; parsing; structure suites | Task 0 | 5 | `cargo test -p kermit-ds` green and warning-free; the new unit tests Miri-clean; mutation checks recorded |
| **P2 — `kermit` binary** | 6, 7 | the mode reaches the build on every route; CLI and report; join suites | P1 + merge | 2 | `cargo test -p kermit` green, including the new execution, options, CLI and 192 join tests; mutation checks recorded |
| **P3 — analysis and docs** | 8, 9 | kermit-lab tests; every doc in Task 9 | P2 + merge | 2 | kermit-lab pytest green with `KERMIT_BIN`; `cargo doc` and clippy clean |
| *Controller* | 10, 11 | final gate, smoke run, checkpoint 2; landing only on the user's instruction | P3 | — | — |

Each implementer receives this plan, its package's task numbers, the ground
rules and a report budget of about 40 lines. The report covers commit SHAs,
test counts, mutation-check outcomes, and any deviation from the plan with
its reason. An implementer must not start the next package, push or merge.

---

## Task 0 [controller]: Catch up and record the baseline

**Files:** none modified, apart from a possible merge commit.

- [ ] **Step 1: Merge `origin/master` into the branch (never rebase)**

```bash
git -C $WT fetch origin
git -C $WT merge origin/master
```

Expected: "Already up to date" or a fast-forward; the branch holds only this
plan beyond 48a403c.

- [ ] **Step 2: Record the baseline in a file**

```bash
{
  echo "BASE $(git -C $WT rev-parse HEAD)"
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace 2>&1 \
    | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {print "cargo "p" passed, "f" failed, "i" ignored"}'
  uv --directory $WT/python/kermit-lab sync --group test >/dev/null
  echo "pytest $(uv --directory $WT/python/kermit-lab run pytest -q 2>&1 | tail -1)"
  echo "bin-rustdoc errors: $(CARGO_BUILD_JOBS=2 nix develop $WT --command cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings 2>&1 | grep -c '^error')"
} | tee $SCRATCH/baseline.txt
git -C $WT rev-parse HEAD > $SCRATCH/base.sha
```

Expected: 0 failed; pytest all passed (the contract tests skip without
`KERMIT_BIN`). The binary's rustdoc has known pre-existing errors; the
count is the baseline Task 6 must not raise. Hand the file's contents to
each implementer.

---

# P1 — `kermit-ds`

> **As executed (2026-10-05).** P1 landed as 97a9263, 7e03950, a3a0b2b,
> 54f071b and 9e99c5e, then its review follow-ups as 6b35ff1. The follow-ups
> changed P1's code from the text below in four ways:
> - `build_modes_reach_their_builds` gained a `parallel:3` row. Every record
>   check used N = 2, so a build that ignored N passed all 1136 kermit-ds
>   tests; the new row expects `(3, [8, 0].repeat(8))` and worker runs
>   `[3, 3]`;
> - `parallel_builds_the_serial_trie_on_large_and_skewed_inputs` covers
>   arity 4, a first key holding half the tuples, and three morsels, two
>   of them full (not under Miri);
> - `Entries::Subtries` became `Entries::Children`, with `take_from`,
>   `into_children` and `into_chains`, and `fill_root_in_morsels` carries
>   step comments 1–3;
> - the record's docs say it lists empty partitions, and `identity.rs` keeps
>   its file-local helpers private.
>
> Miri on `hash_trie::parallel`: 5 passed, 3 ignored, about 20 s. The P2 and
> P3 text below is written against the code as it now stands.

## Task 1 [P1]: Every Layout's nodes are `Send`

The build step moves each finished scratch root from a worker to the
calling thread, so `HashTrieNode<P, E>` must be `Send` for generic `P` and
`E`. Both policies' payloads are `Send` today (`Never`, `Vec<usize>`,
`Box<LazyChild<N>>` for `N: Send`), but generic code cannot prove it without
bounds on the associated types.

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/pruning.rs` (the `Payload` associated type)
- Modify: `kermit-ds/src/ds/hash_trie/expansion.rs` (the `Pending` associated type and its two impls)
- Test: `kermit-ds/src/ds/hash_trie/implementation.rs` (the `tests` module, after `eager_tries_stay_sync`)

- [ ] **Step 1: Write the failing test**

Add after `eager_tries_stay_sync` in `implementation.rs`'s `mod tests`:

```rust
    /// Every Layout's nodes are `Send`, so the `parallel:N` build can hand a
    /// worker's finished scratch root to the calling thread (#94). Lazy
    /// tries stay `!Sync` (their cells) but are `Send`. Generic, so it pins
    /// the policies' bounds rather than today's two policies of each kind.
    #[test]
    fn nodes_are_send_under_every_layout() {
        use super::super::{expansion::LazyExpansion, pruning::SingletonPruning};
        fn send<T: Send>() {}
        fn node_is_send<P: PruningPolicy, E: ExpansionPolicy>() { send::<HashTrieNode<P, E>>(); }
        node_is_send::<NoPruning, EagerExpansion>();
        node_is_send::<SingletonPruning, LazyExpansion>();
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds nodes_are_send_under_every_layout`
Expected: a compile error, E0277, "`<P as PruningPolicy>::Payload` cannot be
sent between threads safely" (or the same for `Pending`).

- [ ] **Step 3: Add the bounds**

In `pruning.rs`, inside `pub trait PruningPolicy`:

```rust
    /// What a `HashTrieNode::Singleton` holds under this policy. `Send`, so
    /// a parallel build can move finished subtries between threads.
    type Payload: SingletonPayload + Send;
```

In `expansion.rs`, inside `pub trait ExpansionPolicy`:

```rust
    /// What a `HashTrieNode::Unexpanded` holds under this policy, for a
    /// node type `N`. `Send` whenever `N` is, so a parallel build can move
    /// finished subtries between threads: `LazyChild`'s cells are `Send`,
    /// only `!Sync`.
    type Pending<N: Send>: PendingChild<N> + Send;
```

and in the two impls:

```rust
impl ExpansionPolicy for EagerExpansion {
    type Pending<N: Send> = Never;

    const LAZY: bool = false;
}
```

```rust
impl ExpansionPolicy for LazyExpansion {
    type Pending<N: Send> = Box<LazyChild<N>>;

    const LAZY: bool = true;
}
```

`HashTrieNode`'s `Unexpanded(E::Pending<HashTrieNode<P, E>>)` now needs
`HashTrieNode<P, E>: Send`, which the compiler proves coinductively (an auto
trait). If a use site elsewhere fails to compile, report it rather than
adding `where` clauses: a `where HashTrieNode<P, E>: Send` on a public item
names a crate-private type (`private_bounds`, an error under CI's
`-Dwarnings`).

- [ ] **Step 4: Run the test and the whole workspace's build**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds nodes_are_send_under_every_layout
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo check --workspace --all-targets
```

Expected: PASS, and the check finishes without errors or warnings.

- [ ] **Step 5: Commit**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit-ds/src/ds/hash_trie/pruning.rs kermit-ds/src/ds/hash_trie/expansion.rs kermit-ds/src/ds/hash_trie/implementation.rs
git -C $WT commit -m "refactor(hash-trie): every Layout's nodes are Send (#94)

A parallel build moves finished scratch roots between threads.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
```

---

## Task 2 [P1]: Share the radix build's steps and identity checks

A pure refactor: nothing that runs changes. The parallel build reuses three
of `radix.rs`'s functions, the two builds share one entry point's checks,
and both test modules share the array-level identity assertions.

**Files:**
- Create: `kermit-ds/src/ds/hash_trie/identity.rs` (test-only)
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs`
- Modify: `kermit-ds/src/ds/hash_trie/radix.rs`
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (`from_tuples_with_config_and_build_mode`)

- [ ] **Step 1: Move the identity assertions into `identity.rs`**

Create `kermit-ds/src/ds/hash_trie/identity.rs`. The bodies are moved
verbatim from `radix.rs`'s `mod tests`; only `pub(super)` and the module doc
are new:

```rust
//! Array-level identity checks for the HashTrie build modes (#91, #94).
//! Every mode must build the trie the serial build does, bucket for bucket
//! and capacity for capacity (the BuildMode rule of
//! `docs/specs/optimization-standard.md`), so the radix and parallel builds'
//! tests share these assertions and inputs.

use {
    super::{
        expansion::{ExpansionPolicy, PendingChild},
        hash_table::HashTable,
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::{PruningPolicy, SingletonPayload},
    },
    crate::{cardinality::Cardinality, heap_size::HeapSize, test_support::Lcg},
    kermit_iters::HashStrategy,
};

/// Asserts that two tables hold the same buckets: the same capacity
/// (array length and allocation), the same length, the same hash in
/// every bucket, and values that `same_value` accepts.
pub(super) fn assert_same_table<V>(
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

// `&Vec`, not a slice: the comparison reads `capacity()`.
#[allow(clippy::ptr_arg)]
pub(super) fn assert_same_chain(a: &Vec<Vec<usize>>, b: &Vec<Vec<usize>>, path: &str) {
    assert_eq!(a, b, "{path}: chain");
    assert_eq!(a.capacity(), b.capacity(), "{path}: chain capacity");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert_eq!(x.capacity(), y.capacity(), "{path}: tuple {i} capacity");
    }
}

pub(super) fn assert_same_node<P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrieNode<P, E>, b: &HashTrieNode<P, E>, path: &str,
) {
    match (a, b) {
        | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
            assert_same_table(x, y, path, &|x, y, path| assert_same_node(x, y, path))
        },
        | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
            assert_same_table(x, y, path, &|x, y, path| assert_same_chain(x, y, path))
        },
        | (HashTrieNode::Singleton(x), HashTrieNode::Singleton(y)) => {
            assert_eq!(x.tuple(), y.tuple(), "{path}: singleton");
            assert_eq!(
                x.tuple().capacity(),
                y.tuple().capacity(),
                "{path}: singleton capacity"
            );
        },
        | (HashTrieNode::Unexpanded(x), HashTrieNode::Unexpanded(y)) => {
            // Building expands nothing, so both children are still the
            // pending lists `insert_at` appended to.
            assert!(
                x.built().is_none() && y.built().is_none(),
                "{path}: expanded during the build"
            );
            assert_same_chain(&x.pending(), &y.pending(), &format!("{path}: pending"));
        },
        | _ => panic!("{path}: node variants differ"),
    }
}

/// The array-level identity the standard requires of a BuildMode:
/// every table's buckets and capacity, every chain and tuple capacity,
/// every singleton and pending list, the heap size and the tuple count.
pub(super) fn assert_same_trie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_same_node(a.root(), b.root(), label);
    assert_eq!(
        a.heap_size_bytes(),
        b.heap_size_bytes(),
        "{label}: heap size"
    );
    assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
}

/// Rows of up to three columns, cut to `arity`.
pub(super) fn rows(arity: usize, rows: &[[usize; 3]]) -> Vec<Vec<usize>> {
    rows.iter().map(|row| row[..arity].to_vec()).collect()
}

/// Enough distinct first values to double the root several times (three
/// times under Miri), with repeats so subtries and chains hold several
/// tuples. The builds are safe Rust, so Miri runs a small matrix: the full
/// one took almost five minutes there.
pub(super) const RANDOM_TUPLES: usize = if cfg!(miri) {
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

pub(super) fn inputs(arity: usize) -> Vec<(&'static str, Vec<Vec<usize>>)> {
    let mut lcg = Lcg(0x91);
    let random = (0..RANDOM_TUPLES)
        .map(|_| {
            let row = [
                lcg.next_usize() % (RANDOM_TUPLES / 4),
                lcg.next_usize() % 50,
                lcg.next_usize() % 7,
            ];
            row[..arity].to_vec()
        })
        .collect();
    vec![
        ("empty", vec![]),
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
```

In `radix.rs`'s `mod tests`, delete the moved items (`assert_same_table`,
`assert_same_chain`, `assert_same_node`, `assert_same_trie`, `rows`,
`RANDOM_TUPLES`, `LOAD_PERCENTS`, `inputs`). Keep `BITS`, `check_identity`
and every `#[test]`. Replace the module's `use` block with:

```rust
    use {
        super::*,
        crate::{
            ds::hash_trie::{
                build_mode::HashTrieBuildMode,
                config::HashTrieConfig,
                expansion::{EagerExpansion, LazyExpansion},
                identity::{assert_same_trie, inputs, rows, LOAD_PERCENTS},
                pruning::{NoPruning, SingletonPruning},
            },
            relation::{BuildModeRelation, ConfigurableRelation, Relation},
            test_support::Mod10HashStrategy,
        },
        kermit_iters::{FxHashStrategy, SipHashStrategy},
    };
```

In `hash_trie/mod.rs`, after `mod hash_trie_iter;`:

```rust
#[cfg(test)]
mod identity;
```

- [ ] **Step 2: Expose the three radix steps to the parallel build**

In `radix.rs` (non-test code), change only these signatures:

```rust
/// A root entry ready to merge: the input position at which its key first
/// appeared, its hash, and its finished value.
pub(super) type Arrival<V> = (usize, u64, V);
```

```rust
pub(super) fn build_scratch_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    partition: impl IntoIterator<Item = Indexed>, arity: usize, load_factor: LoadFactor,
) -> (HashTrieNode<P, E>, Vec<(usize, u64)>) {
```

```rust
pub(super) fn take_in_arrival_order<V>(
```

The bodies stay as they are; `fill_root` still passes each partition's
`Vec`, so the radix build compiles to the same loop.

- [ ] **Step 3: One entry point for the partitioned builds**

In `implementation.rs`, in the `impl` block holding
`from_tuples_with_config_and_build_mode`, add below that method:

```rust
    /// What the partitioned builds share (`radix:K`, `parallel:N`): the
    /// serial build's arity check, with its message, then `fill` on the
    /// empty root, then the multiset count the serial build keeps.
    fn from_tuples_partitioned(
        header: RelationHeader, config: HashTrieConfig, tuples: Vec<Vec<usize>>,
        fill: impl FnOnce(&mut HashTrieNode<P, E>, usize, Vec<Vec<usize>>),
    ) -> Self {
        let arity = header.arity();
        for tuple in &tuples {
            assert_eq!(
                tuple.len(),
                arity,
                "from_tuples: tuple arity {} does not match header arity {}",
                tuple.len(),
                arity,
            );
        }
        let tuple_count = tuples.len();
        let mut trie = Self::with_config(header, config);
        fill(&mut trie.root, arity, tuples);
        trie.tuple_count = tuple_count;
        trie
    }
```

and replace the `Radix` arm with:

```rust
            | HashTrieBuildMode::Radix(bits) => {
                Self::from_tuples_partitioned(header, config, tuples, |root, arity, tuples| {
                    radix::fill_root::<H, P, E>(root, arity, tuples, bits, config.load_factor)
                })
            },
```

- [ ] **Step 4: The radix tests still pass, unchanged in number**

Run the first command once **before Step 1** too, and note its count.

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib radix 2>&1 | grep -E '^test result|FAILED|panicked'
RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit-ds --all-targets
```

Expected: the same number of tests pass as before Step 1, 0 failed; clippy
is clean, so no import is left unused.

- [ ] **Step 5: Commit**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit-ds/src/ds/hash_trie/identity.rs kermit-ds/src/ds/hash_trie/mod.rs kermit-ds/src/ds/hash_trie/radix.rs kermit-ds/src/ds/hash_trie/implementation.rs
git -C $WT commit -m "refactor(hash-trie): share the radix build's steps and identity checks (#94)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
```

---

## Task 3 [P1]: `HashTrieBuildMode::Parallel` and the parallel build

**Files:**
- Create: `kermit-ds/src/ds/hash_trie/parallel.rs`
- Modify: `kermit-ds/src/ds/hash_trie/build_mode.rs` (variant, `axis_value`, docs, pinned-labels test)
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (the `Parallel` arm, the `use` block, the docs)
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs`, `kermit-ds/src/ds/mod.rs`, `kermit-ds/src/test_hooks.rs` (the record's re-exports)
- Modify: `kermit-ds/src/morsel.rs:253` (one comment)

- [ ] **Step 1: The variant and its label**

In `build_mode.rs`, change the `use` block and the enum:

```rust
use {
    crate::morsel::Threads,
    std::{fmt, str::FromStr},
};

/// How a [`HashTrie`](super::HashTrie) is built from a known set of tuples.
/// Every mode builds the identical trie — the same buckets and the same
/// capacities — so the mode changes how long the build takes, never the
/// trie it builds (issues #91, #94).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HashTrieBuildMode {
    /// One insert per tuple, in input order: Algorithm 2 of the paper, and
    /// the only build before issue #91.
    #[default]
    Serial,
    /// Radix-partition the tuples on the top bits of their first
    /// attribute's hash, build each partition separately, then merge
    /// (SIGMOD 2020 §3.3.2).
    Radix(RadixBits),
    /// The radix build's partition and build steps on this many threads,
    /// the calling thread included: the morsel-driven parallel build
    /// (`docs/data-structures/parallel-build.md`, issue #94).
    Parallel(Threads),
}
```

In `axis_value`, add the arm:

```rust
            | Self::Parallel(threads) => format!("parallel:{}", threads.get()),
```

and in `axis_values_and_default_are_pinned`, add:

```rust
        assert_eq!(
            HashTrieBuildMode::Parallel(Threads::new(8).unwrap()).axis_value(),
            "parallel:8"
        );
```

`FromStr` keeps rejecting `parallel:…` until Task 4.

- [ ] **Step 2: Write the failing tests in a new `parallel.rs`**

Create `kermit-ds/src/ds/hash_trie/parallel.rs` holding only the module doc,
the imports below, a stub, and the tests (Step 4 fills in the code):

```rust
//! The `parallel:N` build of a [`HashTrie`](super::HashTrie): the radix
//! build's three steps (`radix.rs`), with the partition and build steps run
//! on N threads by the morsel-driven helpers of `crate::morsel` (Leis et
//! al., SIGMOD 2014; SIGMOD 2020 §3.3.2; issue #94).
//!
//! 1. **Partition.** [`scatter`] moves every tuple, with its input position,
//!    into one of P partitions by the top log₂P bits of its first
//!    attribute's hash; P is four per thread, rounded up to a power of two.
//! 2. **Build.** [`dispatch`] builds each non-empty partition into a scratch
//!    root by the serial build's own `insert_at`, then takes the scratch
//!    root's entries out, each tagged with the input position at which its
//!    key first appeared.
//! 3. **Merge.** The calling thread inserts every entry into the real root
//!    in first-appearance order, by a k-way merge of the partitions' lists.
//!
//! The result is the serial trie, bucket for bucket and capacity for
//! capacity, for the reasons the radix build's is. Each partition lists its
//! tuples in input order (`scatter` keeps it), so each subtrie receives the
//! serial build's `insert_at` calls; and the root receives its new keys in
//! the serial order. Equal 64-bit hashes share a root entry and also a
//! partition, because the partition is a function of the hash.

use {
    super::{
        config::LoadFactor,
        expansion::ExpansionPolicy,
        hash_table::HashTable,
        node::HashTrieNode,
        pruning::PruningPolicy,
        radix::{self, Arrival},
    },
    crate::morsel::{
        dispatch, scatter, Partition, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD,
    },
    kermit_iters::HashStrategy,
    std::{cmp::Reverse, collections::BinaryHeap},
};

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            ds::hash_trie::{
                build_mode::{HashTrieBuildMode, RadixBits},
                config::HashTrieConfig,
                expansion::{EagerExpansion, LazyExpansion},
                identity::{assert_same_node, assert_same_trie, inputs, LOAD_PERCENTS},
                implementation::HashTrie,
                pruning::{NoPruning, SingletonPruning},
            },
            relation::{BuildModeRelation, ConfigurableRelation, Relation},
            test_support::Mod10HashStrategy,
        },
        kermit_iters::{FxHashStrategy, LayoutOption, SipHashStrategy},
    };

    fn threads(n: usize) -> Threads { Threads::new(n).expect("tests use a nonzero count") }

    /// Thread counts under test. Spawning threads is slow under Miri, so it
    /// runs two.
    const THREADS: &[usize] = if cfg!(miri) {
        &[2]
    } else {
        &[1, 2, 3, 8]
    };

    /// `parallel:N` builds the trie `serial` builds, for every arity, load
    /// factor, thread count and input. Each case runs twice: through the
    /// public constructor (morsels of 16 384 tuples, the whole trie
    /// compared), and with morsels of 7 tuples, which make the partition
    /// step cut the input many times (the root compared, which holds the
    /// whole trie). Miri runs the second only.
    fn check_identity<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for &percent in LOAD_PERCENTS {
                let config = HashTrieConfig {
                    load_factor: LoadFactor::percent(percent).unwrap(),
                };
                for (input, tuples) in inputs(arity) {
                    let serial = HashTrie::<H, P, E>::from_tuples_with_config(
                        arity.into(),
                        config,
                        tuples.clone(),
                    );
                    for &t in THREADS {
                        let label = format!(
                            "{}/{}/{} arity {arity}, load {percent}%, parallel:{t}, {input}",
                            H::NAME,
                            P::NAME,
                            E::NAME
                        );
                        if !cfg!(miri) {
                            let built = HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
                                arity.into(),
                                config,
                                HashTrieBuildMode::Parallel(threads(t)),
                                tuples.clone(),
                            );
                            assert_same_trie(&serial, &built, &label);
                        }
                        let mut root = HashTrie::<H, P, E>::make_root(arity);
                        fill_root_in_morsels::<H, P, E>(
                            &mut root,
                            arity,
                            tuples.clone(),
                            threads(t),
                            7,
                            config.load_factor,
                        );
                        assert_same_node(serial.root(), &root, &format!("{label}, morsels of 7"));
                    }
                }
            }
        }
    }

    #[test]
    fn parallel_builds_the_serial_trie_under_siphash() {
        check_identity::<SipHashStrategy, NoPruning, EagerExpansion>();
        check_identity::<SipHashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<SipHashStrategy, NoPruning, LazyExpansion>();
        check_identity::<SipHashStrategy, SingletonPruning, LazyExpansion>();
    }

    #[test]
    #[cfg_attr(miri, ignore = "threads are slow under Miri; the SipHash matrix runs the same code")]
    fn parallel_builds_the_serial_trie_under_fxhash() {
        check_identity::<FxHashStrategy, NoPruning, EagerExpansion>();
        check_identity::<FxHashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<FxHashStrategy, NoPruning, LazyExpansion>();
        check_identity::<FxHashStrategy, SingletonPruning, LazyExpansion>();
    }

    /// Every hash is below 10, so every tuple lands in partition 0, and
    /// distinct keys share full hashes, root entries and leaf chains.
    #[test]
    #[cfg_attr(miri, ignore = "threads are slow under Miri; the SipHash matrix runs the same code")]
    fn parallel_builds_the_serial_trie_under_colliding_hashes() {
        check_identity::<Mod10HashStrategy, NoPruning, EagerExpansion>();
        check_identity::<Mod10HashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<Mod10HashStrategy, NoPruning, LazyExpansion>();
        check_identity::<Mod10HashStrategy, SingletonPruning, LazyExpansion>();
    }

    #[test]
    fn partition_bits_give_four_partitions_per_thread_rounded_up() {
        assert_eq!(partition_bits(threads(1)), 2);
        assert_eq!(partition_bits(threads(2)), 3);
        assert_eq!(partition_bits(threads(3)), 4);
        assert_eq!(partition_bits(threads(8)), 5);
        assert_eq!(partition_bits(threads(Threads::MAX)), 12);
    }

    /// Hashes a key below 8 to the key in its top three bits, so under
    /// `parallel:2` (eight partitions) key k lands in partition k.
    #[derive(Copy, Clone, Default, Debug)]
    struct TopBitsHash;

    impl LayoutOption for TopBitsHash {
        const NAME: &'static str = "top-bits";
    }

    impl HashStrategy for TopBitsHash {
        fn hash(key: usize) -> u64 { (key as u64) << 61 }
    }

    /// Every mode builds the same trie, so only the records show which build
    /// ran, and that `parallel:N` spreads its work: N workers in both steps
    /// (two `run_workers` calls), and the tuples spread over the partitions.
    /// Eight first keys of eight tuples each fill the eight partitions of
    /// `parallel:2` evenly. The radix build partitions too, but serially.
    #[test]
    fn build_modes_reach_their_builds() {
        let tuples: Vec<Vec<usize>> = (0..64).map(|i| vec![i % 8, i]).collect();
        let serial: HashTrie<TopBitsHash> = HashTrie::from_tuples(2.into(), tuples.clone());
        for (mode, builds, worker_runs) in [
            (HashTrieBuildMode::Serial, vec![], vec![]),
            (
                HashTrieBuildMode::Radix(RadixBits::new(3).unwrap()),
                vec![],
                vec![],
            ),
            (
                HashTrieBuildMode::Parallel(threads(2)),
                vec![(2, vec![8; 8])],
                vec![2, 2],
            ),
        ] {
            PARALLEL_BUILDS.with(|builds| builds.borrow_mut().clear());
            crate::morsel::take_worker_runs();
            let built: HashTrie<TopBitsHash> =
                HashTrie::from_tuples_with_build_mode(2.into(), mode, tuples.clone());
            assert_eq!(PARALLEL_BUILDS.with(|b| b.take()), builds, "{mode:?}");
            assert_eq!(crate::morsel::take_worker_runs(), worker_runs, "{mode:?}");
            assert_same_trie(&serial, &built, &format!("{mode:?}"));
        }
    }

    /// The parallel build of nothing is the empty trie, built without
    /// partitioning or starting a worker.
    #[test]
    fn parallel_build_of_nothing_starts_no_worker() {
        PARALLEL_BUILDS.with(|builds| builds.borrow_mut().clear());
        crate::morsel::take_worker_runs();
        let built: HashTrie = HashTrie::from_tuples_with_build_mode(
            2.into(),
            HashTrieBuildMode::Parallel(threads(4)),
            vec![],
        );
        assert_same_trie(&HashTrie::from_tuples(2.into(), vec![]), &built, "empty");
        assert!(PARALLEL_BUILDS.with(|b| b.take()).is_empty());
        assert!(crate::morsel::take_worker_runs().is_empty());
    }

    #[test]
    #[should_panic(expected = "does not match header arity")]
    fn parallel_build_rejects_a_wrong_arity() {
        let _: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            HashTrieConfig::default(),
            HashTrieBuildMode::Parallel(threads(2)),
            vec![vec![1, 2], vec![3]],
        );
    }
}
```

In `hash_trie/mod.rs`, after `mod node;`, add `mod parallel;`.

- [ ] **Step 3: Run the tests to see them fail**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds parallel`
Expected: compile errors: `fill_root_in_morsels`, `partition_bits` and
`PARALLEL_BUILDS` not found, and the non-exhaustive match on
`HashTrieBuildMode` in `from_tuples_with_config_and_build_mode`.

- [ ] **Step 4: Write the parallel build**

In `parallel.rs`, between the `use` block and `mod tests`:

```rust
#[cfg(any(test, feature = "test-hooks"))]
thread_local! {
    /// The `(threads, partition sizes)` of every parallel build on this
    /// thread. Every build mode builds the same trie, so only this record
    /// can tell a test which build ran, and where it put its tuples. Other
    /// crates' tests read it through `test_hooks` (the `test-hooks` feature).
    static PARALLEL_BUILDS: std::cell::RefCell<Vec<(usize, Vec<usize>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Takes this thread's record of parallel builds, oldest first, leaving it
/// empty: the `(threads, partition sizes)` of each.
#[cfg(feature = "test-hooks")]
pub(crate) fn take_parallel_builds() -> Vec<(usize, Vec<usize>)> {
    PARALLEL_BUILDS.with(|builds| builds.take())
}

/// One partition's root entries, each tagged with the input position at
/// which its key first appeared, in that order: subtries under an `Inner`
/// root (arity ≥ 2), chains under a `Leaf` root (arity 1).
enum Entries<P: PruningPolicy, E: ExpansionPolicy> {
    Subtries(Vec<Arrival<HashTrieNode<P, E>>>),
    Chains(Vec<Arrival<Vec<Vec<usize>>>>),
}

/// The radix bits for `threads`: [`PARTITIONS_PER_THREAD`] partitions per
/// thread, rounded up to a power of two. From 2 bits (one thread) to 12
/// ([`Threads::MAX`]).
fn partition_bits(threads: Threads) -> u32 {
    (PARTITIONS_PER_THREAD * threads.get())
        .next_power_of_two()
        .trailing_zeros()
}

/// Fills the empty `root` with `tuples` by the `parallel:threads` build.
/// Every tuple must have `arity` attributes; the caller checks.
pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    root: &mut HashTrieNode<P, E>, arity: usize, tuples: Vec<Vec<usize>>, threads: Threads,
    load_factor: LoadFactor,
) {
    fill_root_in_morsels::<H, P, E>(root, arity, tuples, threads, MORSEL_TUPLES, load_factor);
}

/// [`fill_root`] with the morsel size as a parameter, so tests can cut a
/// small input into many morsels.
fn fill_root_in_morsels<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    root: &mut HashTrieNode<P, E>, arity: usize, tuples: Vec<Vec<usize>>, threads: Threads,
    morsel_tuples: usize, load_factor: LoadFactor,
) {
    if tuples.is_empty() {
        // The serial build of nothing is the empty root: start no worker.
        return;
    }
    let bits = partition_bits(threads);
    let shift = 64 - bits;
    let partitions = scatter(threads, tuples, morsel_tuples, 1 << bits, |tuple| {
        (H::hash(tuple[0]) >> shift) as usize
    });
    #[cfg(any(test, feature = "test-hooks"))]
    PARALLEL_BUILDS.with(|builds| {
        let sizes = partitions.iter().map(Partition::len).collect();
        builds.borrow_mut().push((threads.get(), sizes));
    });
    // An empty partition would build and drop an empty scratch root; the
    // radix build skips them too.
    let partitions: Vec<Partition> = partitions
        .into_iter()
        .filter(|partition| partition.len() > 0)
        .collect();
    let entries = dispatch(threads, partitions, |partition| {
        let (scratch, first_seen) =
            radix::build_scratch_root::<H, P, E>(partition.into_tuples(), arity, load_factor);
        match scratch {
            | HashTrieNode::Inner(table) => {
                let mut subtries = Vec::with_capacity(first_seen.len());
                radix::take_in_arrival_order(table, &first_seen, &mut subtries);
                Entries::Subtries(subtries)
            },
            | HashTrieNode::Leaf(table) => {
                let mut chains = Vec::with_capacity(first_seen.len());
                radix::take_in_arrival_order(table, &first_seen, &mut chains);
                Entries::Chains(chains)
            },
            | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
                unreachable!("a root is never pruned or unexpanded")
            },
        }
    });
    match root {
        | HashTrieNode::Inner(table) => {
            let lists = entries.into_iter().map(|entries| match entries {
                | Entries::Subtries(subtries) => subtries,
                | Entries::Chains(_) => unreachable!("an Inner root's scratch roots are Inner"),
            });
            merge_in_first_appearance_order(table, lists.collect(), load_factor);
        },
        | HashTrieNode::Leaf(table) => {
            let lists = entries.into_iter().map(|entries| match entries {
                | Entries::Chains(chains) => chains,
                | Entries::Subtries(_) => unreachable!("a Leaf root's scratch roots are Leaf"),
            });
            merge_in_first_appearance_order(table, lists.collect(), load_factor);
        },
        | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
            unreachable!("a root is never pruned or unexpanded")
        },
    }
}

/// Inserts every partition's entries into the real root in the order their
/// keys first appeared in the input, the order the serial build inserts
/// them, so the root's buckets, length and capacity are the serial
/// build's. Each list is already in that order, so a k-way merge suffices:
/// one heap operation per entry over at most P lists, where the radix build
/// sorts every entry. It runs on the calling thread: the build's sequential
/// step.
fn merge_in_first_appearance_order<V>(
    root: &mut HashTable<V>, lists: Vec<Vec<Arrival<V>>>, load_factor: LoadFactor,
) {
    let mut lists: Vec<_> = lists
        .into_iter()
        .map(|list| list.into_iter().peekable())
        .collect();
    // Each list's next first-appearance position, smallest on top. Positions
    // are distinct, so no two heads tie.
    let mut heads: BinaryHeap<Reverse<(usize, usize)>> = lists
        .iter_mut()
        .enumerate()
        .filter_map(|(list, entries)| entries.peek().map(|&(first, ..)| Reverse((first, list))))
        .collect();
    while let Some(Reverse((_, list))) = heads.pop() {
        let (_, hash, value) = lists[list]
            .next()
            .expect("a head names a non-empty list");
        root.entry_or_insert_with(hash, load_factor, || value);
        if let Some(&(first, ..)) = lists[list].peek() {
            heads.push(Reverse((first, list)));
        }
    }
}
```

In `implementation.rs`, add `parallel,` to the `super::{…}` import (beside
`radix`), add the arm after `Radix`:

```rust
            | HashTrieBuildMode::Parallel(threads) => {
                Self::from_tuples_partitioned(header, config, tuples, |root, arity, tuples| {
                    parallel::fill_root::<H, P, E>(root, arity, tuples, threads, config.load_factor)
                })
            },
```

and extend two docs. In the struct's `# Construction` paragraph, replace "can
also be built by the `radix:K` BuildMode" with "can also be built by the
`radix:K` and `parallel:N` BuildModes". In
`from_tuples_with_config_and_build_mode`'s doc, replace "`Radix` partitions
first (see `radix.rs`)." with "`Radix` partitions first (see `radix.rs`), and
`Parallel` runs the radix build's partition and build steps on threads (see
`parallel.rs`)."

- [ ] **Step 5: Expose the record to other crates' tests**

`hash_trie/mod.rs`, after the `mod` lines:

```rust
#[cfg(feature = "test-hooks")]
pub(crate) use parallel::take_parallel_builds;
```

`kermit-ds/src/ds/mod.rs`, beside the TreeTrie line:

```rust
#[cfg(feature = "test-hooks")]
pub(crate) use hash_trie::take_parallel_builds as take_hash_trie_parallel_builds;
```

`kermit-ds/src/test_hooks.rs`, at the end:

```rust
/// Takes the record of every [`HashTrieBuildMode::Parallel`] build run on
/// the calling thread since the last call, oldest first: each build's thread
/// count and the size of each partition it built. Every other mode adds
/// nothing, and neither does a parallel build of no tuples, which returns
/// before partitioning.
///
/// [`HashTrieBuildMode::Parallel`]: crate::HashTrieBuildMode::Parallel
pub fn take_hash_trie_parallel_builds() -> Vec<(usize, Vec<usize>)> {
    crate::ds::take_hash_trie_parallel_builds()
}
```

In `morsel.rs`, the `scatter_moves_tuples_without_reallocating` doc says
"(HashTrie, plan 2)"; make it "(HashTrie)".

- [ ] **Step 6: Run the tests**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds parallel 2>&1 | grep -E '^test |^test result'
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}'
RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit-ds --all-targets --features test-hooks
```

Expected: the seven new tests pass; 0 failed overall; clippy clean with and
without the feature (run it once more without `--features test-hooks`).

- [ ] **Step 7: Miri on the new tests**

```bash
MIRIFLAGS="-Zmiri-disable-isolation" CARGO_BUILD_JOBS=2 nix develop $WT --command cargo miri test -p kermit-ds --lib hash_trie::parallel 2>&1 | tail -5
```

Expected: PASS, the FxHash and Mod10 tests ignored. Report the wall time. If
it exceeds 5 minutes, stop and report rather than trimming further.

- [ ] **Step 8: Commit**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit-ds/src/ds/hash_trie/parallel.rs kermit-ds/src/ds/hash_trie/build_mode.rs kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/mod.rs kermit-ds/src/ds/mod.rs kermit-ds/src/test_hooks.rs kermit-ds/src/morsel.rs
git -C $WT commit -m "feat(hash-trie): parallel:N build mode (#94)

The radix build's partition and build steps on N threads, merged into
the root in first-appearance order: the serial trie, capacities included.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
```

- [ ] **Step 9: Mutation checks**

| # | Exact edit | Must fail |
|---|---|---|
| M1 | In `merge_in_first_appearance_order`, replace both `Reverse((first, list))` with `Reverse((usize::MAX - first, list))` (latest first) | `parallel_builds_the_serial_trie_under_siphash` |
| M2 | In `fill_root_in_morsels`, `scatter(threads, tuples,` → `scatter(Threads::new(1).unwrap(), tuples,` | `build_modes_reach_their_builds` (worker runs `[1, 2]`) |
| M3 | `(H::hash(tuple[0]) >> shift) as usize` → `(H::hash(tuple[0]) & ((1 << bits) - 1)) as usize` (low bits) | `build_modes_reach_their_builds` (sizes `[64, 0, …]`). The identity tests must still **pass**: record it, it shows the record is the only guard on spreading |
| M4 | In the `Parallel` arm, `parallel::fill_root::<H, P, E>(root, arity, tuples, threads, config.load_factor)` → `radix::fill_root::<H, P, E>(root, arity, tuples, super::build_mode::RadixBits::new(2).unwrap(), config.load_factor)` | `build_modes_reach_their_builds` (no record) |

---

## Task 4 [P1]: Parse `parallel:<threads>`

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/build_mode.rs` (`FromStr` and its tests)
- Modify: `kermit/src/options.rs` (one test expectation the new parse changes)

- [ ] **Step 1: Write the failing tests**

In `build_mode.rs`'s tests, add a helper beside `radix`:

```rust
    fn parallel(n: usize) -> HashTrieBuildMode { HashTrieBuildMode::Parallel(Threads::new(n).unwrap()) }
```

In `axis_values_round_trip_through_from_str`, after the radix line:

```rust
        modes.extend((1..=8).map(parallel));
        modes.push(parallel(Threads::MAX));
```

Replace `malformed_modes_are_rejected_with_the_accepted_forms`'s `cases`
array and final assertion with:

```rust
        let cases = [
            ("", "unknown hash-trie build mode \"\""),
            ("bulk", "unknown hash-trie build mode \"bulk\""),
            ("Serial", "unknown hash-trie build mode \"Serial\""),
            ("radix", "radix needs a bit count"),
            ("radix:", "whole number"),
            ("radix:x", "whole number"),
            ("radix:8:1", "whole number"),
            ("radix:0", "between 1 and 16, got 0"),
            ("radix:17", "between 1 and 16, got 17"),
            ("radix:300", "between 1 and 16, got 300"),
            ("parallel", "parallel needs a thread count"),
            ("parallel:", "whole number"),
            ("parallel:x", "whole number"),
            ("parallel:-1", "whole number"),
            ("parallel:2:1", "whole number"),
            ("parallel:0", "between 1 and 1024, got 0"),
            ("parallel:1025", "between 1 and 1024, got 1025"),
            (
                "parallel:99999999999999999999999",
                "between 1 and 1024, got 99999999999999999999999",
            ),
        ];
        for (input, why) in cases {
            let msg = input.parse::<HashTrieBuildMode>().unwrap_err().to_string();
            assert!(msg.contains(why), "{input:?}: {msg}");
            assert!(
                msg.contains("expected serial, radix:<bits> or parallel:<threads>"),
                "{input:?}: {msg}"
            );
        }
```

- [ ] **Step 2: Run them to see them fail**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds hash_trie::build_mode`
Expected: FAIL. `parallel:1` does not parse, and the messages say "expected
serial or radix:<bits>".

- [ ] **Step 3: Implement the parse**

Replace the `FromStr` impl's doc and body:

```rust
/// Parses the strings [`axis_value`](kermit_iters::BuildMode::axis_value)
/// returns: `serial`, `radix:<bits>` and `parallel:<threads>`.
impl FromStr for HashTrieBuildMode {
    type Err = ParseHashTrieBuildModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = |why: String| {
            ParseHashTrieBuildModeError(format!(
                "{why}; expected serial, radix:<bits> or parallel:<threads>; bits in {}..={}, \
                 threads in 1..={}",
                RadixBits::MIN,
                RadixBits::MAX,
                Threads::MAX
            ))
        };
        match s.split_once(':') {
            | None if s == "serial" => Ok(Self::Serial),
            | None if s == "radix" => Err(error("radix needs a bit count".to_owned())),
            | None if s == "parallel" => Err(error("parallel needs a thread count".to_owned())),
            | Some(("radix", bits)) => {
                let n: u32 = bits.parse().map_err(|_| {
                    error(format!("radix bits must be a whole number, got {bits:?}"))
                })?;
                u8::try_from(n)
                    .ok()
                    .and_then(|n| RadixBits::new(n).ok())
                    .map(Self::Radix)
                    .ok_or_else(|| {
                        error(format!(
                            "radix bits must be between {} and {}, got {n}",
                            RadixBits::MIN,
                            RadixBits::MAX
                        ))
                    })
            },
            | Some(("parallel", threads)) => {
                // TreeTrie's rule (`tree_trie/build_mode.rs`), so both tries
                // accept and reject the same thread counts.
                if threads.is_empty() || !threads.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(error(format!(
                        "parallel threads must be a whole number, got {threads:?}"
                    )));
                }
                // All digits, so a failed parse is a count too large for
                // `usize`: out of range, like any count above the limit.
                threads
                    .parse::<usize>()
                    .ok()
                    .and_then(Threads::new)
                    .map(Self::Parallel)
                    .ok_or_else(|| {
                        error(format!(
                            "parallel threads must be between 1 and {}, got {threads}",
                            Threads::MAX
                        ))
                    })
            },
            | _ => Err(error(format!("unknown hash-trie build mode {s:?}"))),
        }
    }
}
```

- [ ] **Step 4: Update the one binary test the new parse changes**

`bare_mode_hint` (`kermit/src/options.rs`) now finds `parallel:8` valid for
both tries. In `build_choices_reject_malformed_pairs`, change the case:

```rust
            (
                &["parallel:8"],
                "did you mean --ds-build tree-trie=parallel:8 or hash-trie=parallel:8?",
            ),
```

- [ ] **Step 5: Run the tests**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds hash_trie::build_mode
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit options::
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit-ds/src/ds/hash_trie/build_mode.rs kermit/src/options.rs
git -C $WT commit -m "feat(hash-trie): parse hash-trie=parallel:<threads> (#94)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
```

- [ ] **Step 7: Mutation check**

| # | Exact edit | Must fail |
|---|---|---|
| M5 | Delete the `if threads.is_empty() || …` block (the `return Err` included) | `malformed_modes_are_rejected_with_the_accepted_forms` (`parallel:x` says "between", not "whole number") |

---

## Task 5 [P1]: The structure-level suites under `parallel:2`

**Files:**
- Modify: `kermit-ds/tests/hash_trie_tests.rs` (after the radix block)
- Modify: `kermit-ds/tests/parquet_tests.rs` (after the radix block)

- [ ] **Step 1: Add the aliases and suites**

`hash_trie_tests.rs`: add `Threads` to the `kermit_ds::{…}` import, then
after `hash_trie_test_suite!(HashTrieSipLazyRadix2, SipHashStrategy);`:

```rust
// ── BuildMode: the parallel build ───────────────────────────────────────
// `parallel:2` builds eight partitions on two threads and must build the
// identical trie (issue #94), so the iterator contract holds unchanged,
// eager or lazy, pruned or not.
define_build_mode_provider!(
    HashParallel2,
    HashTrieBuildMode,
    HashTrieBuildMode::Parallel(Threads::new(2).expect("2 is not zero"))
);

type HashTrieSipParallel2 = BuiltWith<HashTrieSip, HashParallel2>;
type HashTrieSipLazyParallel2 = BuiltWith<HashTrieSipLazy, HashParallel2>;
type HashTrieFxPrunedParallel2 = BuiltWith<HashTrieFxPruned, HashParallel2>;

hash_trie_test_suite!(HashTrieSipParallel2, SipHashStrategy);

hash_trie_test_suite!(HashTrieSipLazyParallel2, SipHashStrategy);

hash_trie_test_suite!(HashTrieFxPrunedParallel2, FxHashStrategy);
```

`parquet_tests.rs`, after `parquet_test_suite!(HashTrieSipRadix2, sorted_tuples_radix);`
(`Threads` is already imported there; the provider is named
`HashParallel2` because TreeTrie's `Parallel2` lives in the same file):

```rust
// …and under the parallel BuildMode, which must load the same trie (#94).
define_build_mode_provider!(
    HashParallel2,
    HashTrieBuildMode,
    HashTrieBuildMode::Parallel(Threads::new(2).expect("2 is not zero"))
);

type HashTrieSipParallel2 = BuiltWith<HashTrieSip, HashParallel2>;

fn sorted_tuples_parallel(relation: &HashTrieSipParallel2) -> Vec<Vec<usize>> {
    // `BuiltWith` derefs to the inner `HashTrie`, as `Configured` does.
    sorted_tuples(relation)
}

parquet_test_suite!(HashTrieSipParallel2, sorted_tuples_parallel);
```

- [ ] **Step 2: Run them**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --test hash_trie_tests parallel2 2>&1 | grep -E '^test result'
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --test parquet_tests parallel2 2>&1 | grep -E '^test result'
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}'
```

Expected: the new suites pass (the names contain `parallel2` after paste's
lowercasing; if the filter matches nothing, run the files whole). 0 failed.

- [ ] **Step 3: Commit**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit-ds/tests/hash_trie_tests.rs kermit-ds/tests/parquet_tests.rs
git -C $WT commit -m "test(hash-trie): the structure suites under parallel:2 (#94)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
```

**Checkpoint 1 (controller → user):** P1 is done. Report the commit SHAs, the
test delta against `$SCRATCH/baseline.txt`, the Miri result and time of Task
3 Step 7, and the mutation outcomes M1–M5. Then merge `origin/master` before
P2 starts.

---

# P2 — `kermit` binary

The binary passes `HashTrieBuildMode` through unchanged (#91 made the flag
keyed and the families generic), so P2 changes only help text and tests.

## Task 6 [P2]: The mode reaches the build, the CLI and the report

**Files:**
- Modify: `kermit/src/execution.rs` (tests module)
- Modify: `kermit/src/options.rs` (`BuildChoices` help text; tests)
- Modify: `kermit/tests/cli_hash_trie_build_mode.rs`

- [ ] **Step 1: The mode-reaches-build test**

The counting-hasher spy (`hash_families_build_with_their_mode`) cannot tell
`parallel:N` from `radix:K`: both hash each first attribute once more than
`serial`. Add after it, in `execution.rs`'s tests:

```rust
    /// The real `HashTrie`, not the counting spy, which cannot tell
    /// `parallel:N` from `radix:K`: only kermit-ds's record of parallel
    /// builds (the `test-hooks` feature) shows that the family's mode reached
    /// the build, on every route a relation is built, copies included,
    /// eager or lazy (#94).
    #[test]
    fn hash_trie_families_build_with_their_parallel_mode() {
        fn check<E: ExpansionPolicy>() {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("r.csv");
            std::fs::write(&path, "a,b\n1,2\n2,1\n3,4\n").expect("write csv");
            let header = || RelationHeader::new_positional("r", 2);
            let tuples = || vec![vec![1, 2], vec![2, 1], vec![3, 4]];
            // The thread count of every parallel build `build` runs.
            let parallel_builds = |build: &dyn Fn()| {
                kermit_ds::test_hooks::take_hash_trie_parallel_builds();
                build();
                kermit_ds::test_hooks::take_hash_trie_parallel_builds()
                    .into_iter()
                    .map(|(threads, _)| threads)
                    .collect::<Vec<_>>()
            };
            let config = HashTrieConfig::default();
            let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
            let two = HashTrieBuildMode::Parallel(kermit_ds::Threads::new(2).unwrap());
            for (mode, expected) in [
                (HashTrieBuildMode::Serial, vec![]),
                (radix, vec![]),
                (two, vec![2]),
            ] {
                let structure = HashTrieFamily::<SipHashStrategy, NoPruning, E>::new(config, mode);
                let join = HashHtj::<SipHashStrategy, NoPruning, E>::new(
                    config,
                    mode,
                    Planner::stored(LexicographicOptimiser),
                );
                let routes: [(&str, &dyn Fn()); 6] = [
                    ("HashTrieFamily::build_relation", &|| {
                        structure.build_relation(header(), tuples());
                    }),
                    ("HashTrieFamily::load_with_tuples", &|| {
                        structure.load_with_tuples(&path).expect("load");
                    }),
                    ("HashHtj::build_relation", &|| {
                        join.build_relation(header(), tuples());
                    }),
                    ("HashHtj::load", &|| {
                        join.load(&path).expect("load");
                    }),
                    ("HashHtj::build_from_tuples", &|| {
                        join.build_from_tuples(vec![(header(), tuples())]);
                    }),
                    ("HashHtj::add_index", &|| {
                        let mut engine = join.build(Vec::new());
                        let spec = IndexSpec::new("r", vec![1, 0]);
                        join.add_index(&mut engine, spec, &header(), &tuples());
                    }),
                ];
                for (route, build) in routes {
                    assert_eq!(
                        parallel_builds(build),
                        expected,
                        "{} {route} under {mode:?}",
                        E::NAME
                    );
                }
            }
        }
        check::<EagerExpansion>();
        check::<LazyExpansion>();
    }
```

At the end of `families_report_their_build_mode`, add:

```rust
        let hash_two = HashTrieBuildMode::Parallel(kermit_ds::Threads::new(2).unwrap());
        assert_eq!(
            HashTrieFamily::<SipHashStrategy, NoPruning, EagerExpansion>::new(
                HashTrieConfig::default(),
                hash_two
            )
            .build_mode_axes(),
            build_mode_axis("parallel:2")
        );
        assert_eq!(
            HashHtj::<SipHashStrategy, NoPruning, LazyExpansion>::new(
                HashTrieConfig::default(),
                hash_two,
                Planner::stored(LexicographicOptimiser)
            )
            .build_mode_axes(),
            build_mode_axis("parallel:2")
        );
```

- [ ] **Step 2: The options tests**

In `options.rs`'s `build_choices_resolve_to_each_structures_labels`, change
the HashTrie row of `TABLE`:

```rust
            (IndexStructure::HashTrie, &[
                "serial",
                "radix:1",
                "radix:16",
                "parallel:1",
                "parallel:3",
            ]),
```

In `build_choices_reject_malformed_pairs`, add after the `hash-trie=radix:x`
case:

```rust
            (&["hash-trie=parallel"], "parallel needs a thread count"),
            (&["hash-trie=parallel:0"], "between 1 and 1024, got 0"),
            (&["hash-trie=parallel:1025"], "between 1 and 1024, got 1025"),
            (&["hash-trie=parallel:x"], "whole number"),
```

- [ ] **Step 3: The CLI tests**

In `kermit/tests/cli_hash_trie_build_mode.rs`, extend the module doc's
second sentence to "`serial` by default, `radix:<bits>` or
`parallel:<threads>` under `--ds-build hash-trie=…`", replace
`cli_bench_ds_rejects_malformed_hash_trie_modes`, and add three tests:

```rust
#[test]
fn cli_bench_ds_with_parallel_build_records_axis() {
    let (output, report) = bench_ds("hash-trie", &["--ds-build", "hash-trie=parallel:2"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "parallel:2");
}

/// `--verify` checks the answer counts of parallel-built tries against the
/// benchmark's expected values, eager and lazy.
#[test]
fn cli_bench_run_verifies_answers_from_parallel_built_tries() {
    for expansion in ["eager", "lazy"] {
        let (output, report) = bench_run("triangle", &[
            "-i",
            "hash-trie",
            "-a",
            "hash-triejoin",
            "-m",
            "iteration",
            "--verify",
            "--ds-layout-expansion",
            expansion,
            "--ds-build",
            "hash-trie=parallel:2",
        ]);
        assert!(
            output.status.success(),
            "{expansion}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let axes = axes_of(&report);
        assert_eq!(axes["ds_build_mode"], "parallel:2", "{expansion}: {axes}");
        assert_eq!(axes["verified"], true, "{expansion}: {axes}");
    }
}

/// The same mode name reaches each trie with that trie's own thread count.
#[test]
fn cli_bench_run_sweep_carries_each_structures_parallel_mode() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-build",
        "hash-trie=parallel:3,tree-trie=parallel:2",
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
            | Some("HashTrie") => assert_eq!(axes["ds_build_mode"], "parallel:3", "{axes}"),
            | Some("TreeTrie") => assert_eq!(axes["ds_build_mode"], "parallel:2", "{axes}"),
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | other => panic!("unexpected data_structure {other:?}: {axes}"),
        }
    }
}

#[test]
fn cli_bench_ds_rejects_malformed_hash_trie_modes() {
    for mode in [
        "radix",
        "radix:0",
        "radix:17",
        "bulk",
        "parallel",
        "parallel:0",
        "parallel:1025",
        "parallel:x",
    ] {
        let pair = format!("hash-trie={mode}");
        let (output, _) = bench_ds("hash-trie", &["--ds-build", &pair]);
        assert!(!output.status.success(), "{pair} was accepted");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--ds-build hash-trie"), "{pair}: {stderr}");
        assert!(
            stderr.contains("expected serial, radix:<bits> or parallel:<threads>"),
            "{pair}: {stderr}"
        );
    }
}
```

- [ ] **Step 4: Run them**

The binary already passes the mode through, so these tests should pass at
once: they are the regression guard (Step 7 proves they bite), not a
driver. Run:

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit -- execution:: options:: 2>&1 | grep -E '^test result|FAILED'
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_hash_trie_build_mode 2>&1 | grep -E '^test result|FAILED'
```

Expected: PASS. If one fails, the binary does not pass the mode through
somewhere: stop and report it, with the route.

- [ ] **Step 5: The help text**

In `options.rs`, `BuildChoices::ds_build`'s doc comment, replace
"`hash-trie=serial|radix:<bits>` (default `serial`; bits in 1..=16)." with:

```rust
    /// `hash-trie=serial|radix:<bits>|parallel:<threads>` (default `serial`;
    /// bits in 1..=16, threads in 1..=1024).
```

Check the binary's private docs (they are never rustdoc-linted in CI):

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings 2>&1 | grep -c '^error'
```

Expected: the same count as `bin-rustdoc errors` in `$SCRATCH/baseline.txt`.

- [ ] **Step 6: Commit**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit/src/execution.rs kermit/src/options.rs kermit/tests/cli_hash_trie_build_mode.rs
git -C $WT commit -m "test: hash-trie=parallel:N reaches the build on every route (#94)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
```

- [ ] **Step 7: Mutation checks**

| # | Exact edit | Must fail |
|---|---|---|
| M6 | In `HashTrieFamily::build_relation`, the `self.build,` argument → `HashTrieBuildMode::Serial,` | `hash_trie_families_build_with_their_parallel_mode` |
| M7 | In `HashHtj::new`, `HashTrieFamily::new(config, build)` → `HashTrieFamily::new(config, HashTrieBuildMode::default())` | `hash_trie_families_build_with_their_parallel_mode` (`HashHtj` routes) and `families_report_their_build_mode` |

For M6, also run `cargo test -p kermit` whole and record how many tests
fail besides the new one, as plan 1 did for TreeTrie; the number shows how
much the new test adds.

---

## Task 7 [P2]: HashTrie's parallel build through the join suites

**Files:**
- Modify: `kermit/tests/join_tests.rs`

- [ ] **Step 1: Add the provider and the invocations**

After the radix block (the last
`define_multiway_join_test_suite_for_build_mode!(HashTrieSipPrunedLazy, …, Radix2);`):

```rust
// ── BuildMode axis: HashTrie's parallel build ───────────────────────────
// These build on two threads, in eight partitions. Every mode must build
// the identical trie (issue #94). The same three Layouts as the radix rows
// above (Sip/off/eager, Fx/on/eager, Sip/on/lazy), under every optimiser.
// TreeTrie's provider is `Parallel2`, so this one is `HashParallel2`.
define_build_mode_provider!(
    HashParallel2,
    HashTrieBuildMode,
    HashTrieBuildMode::Parallel(Threads::new(2).expect("2 is not zero"))
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    LexicographicOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    CardinalityOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    CostBasedOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    LexicographicOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    CardinalityOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    CostBasedOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazy,
    HashTriejoin,
    LexicographicOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazy,
    HashTriejoin,
    CardinalityOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazy,
    HashTriejoin,
    CostBasedOptimiser,
    HashParallel2
);
```

In the column-orders section, add the alias after
`type HashTrieSipRadix2 = BuiltWith<HashTrieSip, Radix2>;`:

```rust
type HashTrieSipParallel2 = BuiltWith<HashTrieSip, HashParallel2>;
```

and at the end of the `define_multiway_join_test_suite_with_column_orders!`
argument list (after the last `HashTrieSipRadix2` row, before the closing
`);`):

```rust
    HashTrieSipParallel2,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipParallel2,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipParallel2,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
```

- [ ] **Step 2: Run them**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests hashparallel2 2>&1 | grep -E '^test result'
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests 2>&1 | grep -E '^test result'
```

Expected: the first shows the new tests passing (9 build-mode modules and 3
column-order rows, 16 patterns each: 192 if the filter catches every name;
otherwise compare the whole file's count with the baseline: +192). 0 failed.

- [ ] **Step 3: Commit**

```bash
nix develop $WT --command cargo fmt --all
git -C $WT add kermit/tests/join_tests.rs
git -C $WT commit -m "test: HashTrie's parallel build through the join suites (#94)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
```

Then the controller merges `origin/master` before P3 starts.

---

# P3 — analysis and docs

## Task 8 [P3]: kermit-lab reads HashTrie's parallel rows

kermit-lab is structure-blind here: `threads_of` reads any `parallel:N`, and
`speedup_table` pairs rows by case. These tests pin that for HashTrie,
including `radix:K` rows, which are neither baseline nor arm. They should
pass without a code change; if one fails, stop and report the kermit-lab
bug rather than fixing it inside this task.

**Files:**
- Modify: `python/kermit-lab/tests/test_analysis.py`
- Modify: `python/kermit-lab/tests/test_contract.py`

- [ ] **Step 1: The analysis test**

Add `import warnings` to `test_analysis.py`'s imports (after `import math`),
and after `test_speedup_table_warns_about_parallel_rows_without_a_baseline`:

```python
def test_speedup_table_reads_hash_trie_parallel_rows_beside_radix() -> None:
    """HashTrie has a third mode, ``radix:K``: neither baseline nor arm, so
    it is left out silently. It can also be the baseline, which measures
    the parallel build against the single-threaded partitioned build."""
    df = _build_mode_rows({"serial": [100.0], "radix:8": [120.0], "parallel:2": [50.0]}).assign(
        data_structure="HashTrie", criterion_function="HashTrie/insertion"
    )
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        table = speedup_table(df)
    # The radix row is not a `parallel:N` row left without a baseline.
    assert not [w for w in caught if "parallel:N row" in str(w.message)]
    assert table["threads"].tolist() == [2]
    assert table["speedup"].tolist() == pytest.approx([2.0])
    assert speedup_table(df, baseline="radix:8")["speedup"].tolist() == pytest.approx([2.4])
```

- [ ] **Step 2: The contract test**

After `test_bench_ds_tree_trie_reports_its_parallel_build` in
`test_contract.py`:

```python
def test_bench_ds_hash_trie_reports_its_parallel_build(tmp_path: Path) -> None:
    report = tmp_path / "ds.json"
    _run(
        tmp_path, report,
        "ds", "--relation", str(FIXTURES / "edge.csv"), "-i", "hash-trie", "-m", "space",
        "--ds-build", "hash-trie=parallel:2",
    )
    # Without the back-fill, so a missing key cannot pass as "serial".
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion", apply_defaults=False)
    assert len(df) == 1
    assert df.iloc[0]["ds_build_mode"] == "parallel:2"
    assert df.iloc[0]["threads"] == 2
```

- [ ] **Step 3: Run them**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit
KERMIT_BIN=$WT/target/debug/kermit uv --directory $WT/python/kermit-lab run pytest -q -k "hash_trie" 2>&1 | tail -3
KERMIT_BIN=$WT/target/debug/kermit uv --directory $WT/python/kermit-lab run pytest -q 2>&1 | tail -1
```

Expected: both new tests pass, and the full suite passes.

- [ ] **Step 4: Commit**

```bash
git -C $WT add python/kermit-lab/tests/test_analysis.py python/kermit-lab/tests/test_contract.py
git -C $WT commit -m "test(kermit-lab): HashTrie's parallel rows (#94)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
```

---

## Task 9 [P3]: Document HashTrie's parallel build

**Files:**
- Modify: `docs/data-structures/parallel-build.md`
- Modify: `docs/data-structures/hash-trie.md` (§ "Build modes")
- Modify: `docs/specs/2026-10-05-parallel-build-design.md`
- Modify: `docs/specs/optimization-standard.md`
- Modify: `docs/specs/bench-report-schema.md`
- Modify: `BENCHMARKING.md`, `USAGE.md`, `CLAUDE.md`

- [ ] **Step 1: `parallel-build.md`**

1. Title: `# Parallel builds (`--ds-build tree-trie=parallel:N`, `hash-trie=parallel:N`)`.
2. First sentence: "`TreeTrie` and `HashTrie` can each be built on several
   threads (issue #94)." Keep the rest of the paragraph.
3. Add two rows to the mode table:

   ```markdown
   | `HashTrieBuildMode::Serial` | `hash-trie=serial` | `"serial"` | ✓ |
   | `HashTrieBuildMode::Parallel(n)` | `hash-trie=parallel:N` | `"parallel:N"` | |
   ```

4. The paragraph on `parallel:1` describes TreeTrie: start it "For
   TreeTrie, `parallel:1` …".
5. Add "The HashTrie build is `parallel::fill_root` in
   `kermit-ds/src/ds/hash_trie/parallel.rs`." after the sentence naming
   `TreeTrie::build_parallel`.
6. Insert `## TreeTrie` before `## The three steps`, and demote that section,
   "Invariant: the parallel trie is the serial trie", "Complexity" and
   "Worked micro-example" to `###`.
7. Insert this before `## Measuring`:

````markdown
## HashTrie

The HashTrie build is the radix build of #91
([`hash-trie.md`](./hash-trie.md#build-modes), `radix.rs`) with its first
two steps on N threads.

### The three steps

```text
parallel_build(tuples, N):
    check arities                                     // the serial check, on the caller
    if no tuples: return the empty root               // no worker started
    P = 4·N rounded up to a power of two; b = log₂ P
    partitions = scatter(tuples, morsels of 16 384)   // step 1, N workers
        // tuple t goes to partition H(t[0]) >> (64 − b), with its position
    lists = dispatch(non-empty partitions):            // step 2, N workers
        build a scratch root by insert_at, in input order,
            noting (position, hash) whenever it gains a key
        take its entries out in that order
        -> [(first position, hash, subtrie or chain)]
    root = empty                                       // step 3, the caller
    k-way merge the lists by first position:
        insert each (hash, value) into root
```

- **Partition.** As for TreeTrie, but by the top b bits of the first
  attribute's hash (the radix build's rule; FxHash mixes its low bits
  poorly), so there are no splitters to sample, and each tuple keeps its
  input position.
- **Build.** A worker builds a whole partition into a scratch root of the
  real root's kind, by the serial build's own `insert_at`, then moves the
  scratch root's entries out in the order their keys arrived.
- **Merge.** The calling thread inserts every entry into the real root in
  the order its key first appeared in the input. Each list is already in
  that order, so a k-way merge over at most P lists suffices. (The radix
  build, single-threaded, sorts its entries instead.)

### Invariant: the parallel trie is the serial trie

Every `parallel:N` build is identical to the serial build of the same
tuples, bucket for bucket and capacity for capacity, under every Layout and
load factor. `parallel_builds_the_serial_trie_*` in `parallel.rs` pins it
for N ∈ {1, 2, 3, 8}, with morsels of 7 tuples and of 16 384 (three
morsels, two of them full, in `…_on_large_and_skewed_inputs`).

- A table's final layout depends only on the order in which its *new* keys
  arrive, because `HashTable::entry_or_insert_with` returns an existing
  entry before its resize check.
- `scatter` keeps each partition in input order, so each root key's subtrie
  is built by the serial build's `insert_at` calls, on the same tuples in
  the same order. That covers every `Singleton` and unprune, every chain,
  every capacity and, under lazy expansion, every pending list.
- The merge inserts the root's keys in first-appearance order, the serial
  order. Equal 64-bit hashes share a root entry and also a partition,
  because the partition is a function of the hash.

Finished subtries move from a worker to the caller, so every Layout's nodes
are `Send` (the policies' associated types are bounded so); lazy tries are
still `!Sync`, and no trie is ever shared between threads. The record of
parallel builds works as TreeTrie's does, except that it also lists the
empty partitions the build skips:
`hash_trie_families_build_with_their_parallel_mode` reads it through
`kermit_ds::test_hooks::take_hash_trie_parallel_builds`.

### Complexity

With `n` tuples of arity `a`, `D` distinct first-attribute hashes and `N`
threads (`P` = 4·N rounded up to a power of two):

| Step | Work | Runs on |
|---|---|---|
| Checks | O(n) | the calling thread |
| Partition | O(n) hashes and moves | N workers |
| Build | O(n · a) expected probes and inserts, split across partitions, plus O(D) to take the entries out | N workers |
| Merge | O(D log P) heap operations and D root inserts (expected O(1) each, plus the root's resizes) | the calling thread |

The sequential share is the checks, the merge of per-morsel buckets
((n / 16 384) · P of them) and the merge into the root. It grows with D/n:
when the first attribute is a key (D = n) every root insert happens on one
thread, and a unary relation is the extreme, since its whole trie is the
root. Thread starts are as for TreeTrie, 2·(N − 1) per build. Partition
sizes follow the hash, so balance comes from P > N rather than from
splitters, and a dominant first key still fills one partition.

`parallel:1` against `serial` is what partitioning costs on one thread, as
`radix:2` measures it: #91 found the radix build slower than `serial` on
inputs grouped by their first attribute, and faster only on shuffled ones.
There is no sort saving to offset it, unlike TreeTrie's, so expect
`parallel:1` to be slower than `serial` on grouped inputs.

### Worked micro-example

`parallel:1` over `[3,1] [1,2] [2,9] [1,1]` (positions 0–3) has P = 4
partitions, chosen by the top two bits of each first attribute's hash. Say
those bits are `10` for 3 and 1, and `01` for 2:

| Partition | Tuples @ position | Scratch root's keys, by arrival | Entries out |
|---|---|---|---|
| 1 (`01`) | `[2,9]`@2 | 2 | (2, h(2), {9}) |
| 2 (`10`) | `[3,1]`@0, `[1,2]`@1, `[1,1]`@3 | 3, 1 | (0, h(3), {1}), (1, h(1), {2, 1}) |

The merge takes positions 0, 1 and 2 in turn, inserting 3, then 1, then 2:
the order in which the serial build first meets them, so the root's buckets
are the serial root's.
````

8. In `## Measuring`, add: "For HashTrie, `kl.speedup_table(df,
   baseline="radix:K")` measures the parallel build against the
   single-threaded partitioned one."
9. In `## See also`, add "- [`HashTrie`](./hash-trie.md), whose Build modes
   section lists the mode."

- [ ] **Step 2: `hash-trie.md` § "Build modes"**

1. First paragraph: "(issue #91)" → "(issues #91, #94)".
2. Add the table row:

   ```markdown
   | `Parallel(N)` | `hash-trie=parallel:N`, N in 1..=1024 | the radix build's partition and build steps on N threads (P = 4·N partitions, rounded up to a power of two), then a k-way merge into the root on the calling thread (§3.3.2, morsel-driven) |
   ```

3. After the "**The #66 input shape.**" paragraph, add:

   ```markdown
   **The parallel build** (`parallel.rs`) runs the radix build's partition
   step through `morsel::scatter` and its build step through
   `morsel::dispatch`, so each runs on N threads. The calling thread then
   merges the partitions' entries into the root by a k-way merge on their
   first-appearance positions. The trie is the radix build's, and so the
   serial build's. Steps, identity argument, complexity and a worked example:
   [`parallel-build.md`](./parallel-build.md#hashtrie).
   ```

4. Bullets: **Axis** → "(`serial` / `radix:K` / `parallel:N`)". **API** →
   unchanged. **Tests** → add:
   - "`parallel_builds_the_serial_trie_*` and `build_modes_reach_their_builds`
     in `parallel.rs`: identity for N ∈ {1, 2, 3, 8} and morsels of 7, and
     the record that shows which build ran."
   - "The `HashTrieSipParallel2`, `HashTrieSipLazyParallel2` and
     `HashTrieFxPrunedParallel2` aliases in `hash_trie_tests.rs` (and
     `HashTrieSipParallel2` in `parquet_tests.rs`), and
     `define_multiway_join_test_suite_for_build_mode!` with `HashParallel2`
     in `join_tests.rs`."
   - "`hash_trie_families_build_with_their_parallel_mode` in
     `kermit/src/execution.rs`: the mode reaches the build on every route."
   **Measured effect** → add "`parallel:N`: not yet measured; #94's scaling
   run."

- [ ] **Step 3: The spec**

In `docs/specs/2026-10-05-parallel-build-design.md`:
1. **Status** → "Design approved (brainstormed 2026-10-05); TreeTrie landed
   (plan 1, 00bcc88); HashTrie implemented by plan 2".
2. After the HashTrie section's "**Only one shared internal changes.**"
   paragraph, add:

   ```markdown
   **As built (plan 2, after #91 and #92).** #91 landed first, so the
   HashTrie build reuses its radix steps: `HashTrieBuildMode` gained a
   `Parallel(Threads)` variant beside `Radix`, `from_tuples_with` is #91's
   `from_tuples_with_config_and_build_mode`, and `into_slots` is #91's
   `into_buckets` through `radix::take_in_arrival_order`. The radix build
   keeps its own serial partition, since changing its executed path would
   invalidate its recorded A/B; only `parallel:N` uses `scatter`. The
   policies' associated types gained `Send` bounds, because generic code
   cannot otherwise prove `HashTrieNode<P, E>: Send`. The CLI spelling is
   keyed (`hash-trie=parallel:N`, #91), and it is a new mode, not
   `radix:K:N` (user decision, 2026-10-05).
   ```

3. In "Coordination", end the #91 bullet with "(It did not: see "As built"
   above.)"

- [ ] **Step 4: The standard, the schema, the guides**

`docs/specs/optimization-standard.md`:
- The BuildMode "Examples" row: "HashTrie parallel build" → "HashTrie
  serial / parallel:N ✓".
- "Eight optimizations are implemented — four Layout dimensions, one Config
  value and three BuildModes:" → "Nine optimizations are implemented — four
  Layout dimensions, one Config value and four BuildModes:".
- Landed table, after the TreeTrie row:
  `| HashTrie parallel build (serial / parallel:N) | BuildMode | `ds_build_mode` | §3.3.2 (morsel-driven; issue #94) |`
- "Available to add": delete the "Parallel build (HashTrie; TreeTrie's
  landed with #94)" row.

`docs/specs/bench-report-schema.md`:
- In the `ds_build_mode` bullet, the HashTrie values become "`serial`
  (default), `radix:<bits>` (`--ds-build hash-trie=radix:<bits>`, bits in
  1..=16) and `parallel:N` (`--ds-build hash-trie=parallel:N`, N threads in
  1..=1024)".
- Changelog row at the end of the table: `| 3 (no bump) | 2026-10-05 |
  HashTrie reports can carry `ds_build_mode` `parallel:N` (#94,
  `--ds-build hash-trie=parallel:N`). Every mode builds the identical trie,
  so `schema_version` stays `3`; kermit-lab derives `threads` from it as for
  TreeTrie. |`

`BENCHMARKING.md`:
- The `--ds-build` paragraph: HashTrie's modes become "`hash-trie=serial` by
  default, `hash-trie=radix:<bits>` for the radix-partitioned build of issue
  #91 and `hash-trie=parallel:N` for the parallel build of issue #94".
- "Scaling": first sentence → "`--ds-build tree-trie=parallel:N` builds a
  `TreeTrie` on N threads, and `hash-trie=parallel:N` a `HashTrie` (#94)."
  After the `parallel:1`-against-`serial` paragraph, add:

  ```markdown
  For HashTrie, `parallel:1` against `serial` is the cost of partitioning
  with no sort saving, so it is normally above 1; `radix:2` is the same
  partitioning, run serially. `kl.speedup_table(df, baseline="radix:K")`
  measures the threads alone. Expect the Karp–Flatt fraction to be highest
  where most first-attribute values are distinct, and for unary relations,
  because the root merge runs on one thread.
  ```

`USAGE.md` (§ `--ds-build`): `hash-trie=serial|radix:<bits>` (default
`serial`; bits in 1..=16) → `hash-trie=serial|radix:<bits>|parallel:<threads>`
(default `serial`; bits in 1..=16, threads in 1..=1024). Add the example
`kermit bench ds -r data.csv -i hash-trie -m insertion --ds-build hash-trie=parallel:4`,
and make the closing pointer "for the `tree-trie=parallel:N` and
`hash-trie=parallel:N` speedup workflow".

`CLAUDE.md`:
- Build Commands, after the radix line:
  `cargo run -- bench run triangle -i hash-trie -a hash-triejoin --ds-build hash-trie=parallel:8  # BuildMode axis: HashTrie's parallel build (#94)`
- The **BuildModeRelation** bullet: after "merge them in first-appearance
  order, #91" add "; `parallel:N` = the same steps with partition and build
  on N threads and a k-way merge, #94".
- Component docs: the `parallel-build.md` line ends "(`--ds-build
  tree-trie=parallel:N`, `hash-trie=parallel:N`; `TreeTrie`, `HashTrie`)."
- The `test-hooks` gotcha: "(today `take_tree_trie_parallel_builds`, the
  record each TreeTrie `parallel:N` build pushes)" → "(today
  `take_tree_trie_parallel_builds` and `take_hash_trie_parallel_builds`, the
  records each `parallel:N` build pushes)".

- [ ] **Step 5: Check the docs**

```bash
RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo doc --workspace --no-deps
RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets
git -C $WT diff --check
```

Expected: all clean.

- [ ] **Step 6: Commit**

```bash
git -C $WT add docs/data-structures/parallel-build.md docs/data-structures/hash-trie.md docs/specs/2026-10-05-parallel-build-design.md docs/specs/optimization-standard.md docs/specs/bench-report-schema.md BENCHMARKING.md USAGE.md CLAUDE.md
git -C $WT commit -m "docs: HashTrie's parallel build (#94)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
```

---

# Controller

## Task 10 [controller]: Final gate, smoke run, checkpoint 2

**Files:** none modified.

- [ ] **Step 1: The serial bodies and the siblings are untouched**

```bash
BASE=$(cat $SCRATCH/base.sha)
git -C $WT diff "$BASE" -- kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/radix.rs | grep -E '^-' | grep -v '^---'
git -C $WT diff --stat "$BASE" -- kermit-ds/src/ds/tree_trie kermit-ds/src/ds/column_trie kermit-ds/src/morsel.rs kermit-algos
```

Expected:
- the removed lines are only the old `Radix` arm (now inside
  `from_tuples_partitioned`), two doc sentences, the moved test helpers, and
  the old signature lines of `Arrival`, `build_scratch_root` and
  `take_in_arrival_order`. No line of `from_tuples_with_config`, `insert`,
  `insert_at`, `expand_level`, `radix::fill_root`, `radix::partition` or
  `insert_in_first_appearance_order` appears;
- the stat names only `kermit-ds/src/morsel.rs`, with one changed line.

- [ ] **Step 2: The CI gate, locally**

```bash
nix develop $WT --command cargo fmt --all --check
RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {print p" passed, "f" failed, "i" ignored"}'
RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo doc --workspace --no-deps
MIRIFLAGS="-Zmiri-disable-isolation" CARGO_BUILD_JOBS=2 nix develop $WT --command cargo miri test -p kermit-ds
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit
KERMIT_BIN=$WT/target/debug/kermit uv --directory $WT/python/kermit-lab run pytest -q
```

Expected: every command succeeds with 0 failed. Compute the passed-count
delta against `$SCRATCH/baseline.txt` and account for it by task. Miri on
`kermit-ds` can take longer than 10 minutes; if it does, run it detached
(`setsid nohup … & disown`) and poll its log.

- [ ] **Step 3: Smoke run (not reported anywhere)**

A synthetic, shuffled 2M-tuple relation in `$SCRATCH`, so nothing touches the
shared cache. `radix:2` partitions as `parallel:1` does, but serially.

```bash
awk 'BEGIN { srand(1); print "a,b,c"; for (i = 0; i < 2000000; i++) print int(rand() * 200000) "," int(rand() * 1000) "," int(rand() * 1000) }' > $SCRATCH/smoke.csv
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build --release -p kermit
for mode in serial radix:2 parallel:1 parallel:8; do
  name="smoke-ht-${mode/:/-}"
  env -C $SCRATCH $WT/target/release/kermit bench --sample-size 10 --name "$name" \
    --report-json "$SCRATCH/$name.json" \
    ds --relation $SCRATCH/smoke.csv -i hash-trie -m insertion --ds-build "hash-trie=$mode"
done
uv --directory $WT/python/kermit-lab run python -c "
import kermit_lab as kl
df = kl.load('$SCRATCH/smoke-ht-*.json', criterion_root='$SCRATCH/target/criterion', apply_defaults=False)
print((df.groupby('ds_build_mode')['mean_ns'].mean() / 1e6).round(1))
print(kl.speedup_table(df)[['threads', 'speedup', 'efficiency', 'karp_flatt']])
print(kl.speedup_table(df, baseline='radix:2')[['threads', 'speedup']])
"
```

Expected: all four runs succeed, and then:
- `parallel:8`'s speedup over `serial` is clearly above 1;
- `parallel:1` is within about 15% of `radix:2`, which does the same work
  serially.

If `parallel:8` is not faster than `serial`, stop and report it at the
checkpoint: the scaling phase would then measure nothing worth having.

- [ ] **Step 4: Checkpoint 2 (controller → user)**

Report:
- the commit SHAs;
- the gate results and the test-count delta;
- the smoke table, labelled as a smoke run;
- any deviation from this plan.

## Task 11 [controller]: Hand-off (only on the user's instruction)

Nothing lands without the user's instruction. When it comes:
1. Fetch, and merge `origin/master` in (never rebase).
2. Re-run Task 10 Step 2.
3. Push `HEAD:master` as the user directs.
4. Comment on #94 with plan 2's commits.

#94 stays open for the scaling measurement.

---

## After this plan: the HashTrie half of the scaling measurement

Not part of this plan; a separate step once it lands. Its starting points
are these:
- **Template.** The TreeTrie half's run directory,
  `/tb/Source/Academia/kermit-bench-runs/tree-trie-scaling-2026-10-05/`, with
  its `run.sh`, `host_sampler.sh` and `analyse.py`. Use that run-local
  sampler, not the shared one, whose total-busy threshold flags every
  parallel arm. Its rule is `other_busy` = busy minus the benchmark's own
  cores, idle baseline 0.93, contended above 2.0.
- **Arms.** `serial`, `radix:K` (as the single-threaded partitioned
  reference), then `parallel:1`, `:2`, `:4`, `:8` and `:16`, at the default
  Layout. Add one pruning-on curve, and one lazy curve if wanted (a lazy
  build does little child work, so expect a smaller speedup).
- **Relations.** Include one whose first attribute is a key (D = n) and a
  unary one, where the sequential root merge should dominate Karp–Flatt.
  Include a grouped and a shuffled input as well (#91, #101).
- **Rules.** One binary per structure's half, so speedup ratios compare
  within a structure only. A distinct `--name` per (relation, arm,
  replicate) (#98). At least 5 replicates with the arm order rotated. A
  quiet host, with peers paused and their acknowledgements collected.
