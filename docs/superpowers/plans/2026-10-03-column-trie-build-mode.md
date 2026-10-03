# ColumnTrie Build Modes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `ColumnTrie::from_tuples` build in one pass (O(n·a) after the sort instead of O(n·a·b)), and keep the old routine as the `incremental` mode of a `--ds-build` BuildMode knob, reported as the `ds_build_mode` axis.

**Architecture:** Four work packages (see "Work packages"). Phase 1 (Tasks 1–4) replaces the insert loop with a push-only bulk builder and stamps `ds_build_mode: "bulk"` on ColumnTrie reports through a new `RelationFamily::build_mode_axes` hook; it can land on its own. Phase 2 (Tasks 5–11) bundles the `--ds-*` choices into `DsChoices`, adds `ColumnTrieBuildMode` / `BuildModeRelation` / `BuiltWith` in `kermit-ds`, threads the mode through the `Execution` cell and `SortedTrieFamily`, adds the build-mode join-test macro, the kermit-lab back-fill, and docs.

**Tech Stack:** Rust nightly workspace (clap, Criterion, serde_json), Python kermit-lab (pandas, pytest via uv), Nix dev shell.

**Spec:** [`docs/specs/2026-10-03-column-trie-build-mode-design.md`](../../specs/2026-10-03-column-trie-build-mode-design.md)

---

## Ground rules for every task

- Worktree: `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/84_18daf797cf2ee900`. **Never `cd`** in a Bash call (it moves the session); use absolute paths, `env -C <dir>`, or `uv --directory`.
- Scratch: `SCRATCH=/tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-84-18daf797cf2ee900/a9db6b79-bd81-4d76-a55b-2fe951c7a08c/scratchpad`.
- Run cargo **in the foreground** through the flake with `CARGO_BUILD_JOBS=2`, e.g. `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds`. A background memory monitor kills `run_in_background` cargo jobs. Anything longer than 10 minutes runs fully detached (`setsid nohup … & disown`).
- Format **only** with `nix develop $WT --command cargo fmt --all` (stable rustfmt rewrites dozens of files).
- Doc comments are linted by clippy `doc_markdown`: backtick every identifier.
- Commits: plain conventional commits, never amend, never push. Every commit message ends with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01EQYT5rUt5xkPAh85YeHs8t
  ```
- Mutation checks: commit first, apply the mutant with an exact edit, confirm the named test fails **and** that the mutant applied (`git diff` shows it), then revert by reversing the exact edit (never `git checkout`), and re-run the test to see it pass. `git status` must be clean afterwards.
- Supervisor session: `issues-18daf754b5505502-3d` (SendMessage `to: "uds:/run/user/1000/cc-socks/2053498.sock"`). Checkpoints: this plan before any code; after Task 4 and after Task 11; before any merge or push.
- Do **not** touch `TreeTrie`, `HashTrie`, or any algorithm (Priority 6).

---

## Work packages

The twelve tasks are grouped into four packages, each executed by one implementer agent end to end (its tasks in order, one commit per task, mutation checks included), then reviewed before the next package starts. The gates, evidence runs, supervisor checkpoints and landing (Tasks 4, 11, 12) stay with the controller, between packages.

| Package | Tasks | Scope | Depends on | Commits | Done when |
|---|---|---|---|---|---|
| **P1 — Phase 1: the bulk build** | 1, 2, 3 | `column_trie/implementation.rs`; the `build_mode_axes` hook in `execution.rs`, `bench/ds.rs`, `bench/run.rs`; `cli_column_trie_build_mode.rs`; `column-trie.md`, `bench-report-schema.md` | — | 3 | `cargo test -p kermit-ds` and `-p kermit` green, clippy clean on both, both mutation checks recorded |
| *Controller* | 4 | Phase 1 gate, miri, `friendof` before/after, supervisor checkpoint 2, ask the user about landing phase 1 (Task 12 for phase 1 alone if approved) | P1 | — | — |
| **P2 — Prepare (no behaviour change)** | 5, 6 | Merge `origin/master` first (Task 5 Step 0); `DsChoices` in the binary; `ColumnTrieBuildMode`, `BuildModeRelation`, `BuiltWith` and the `ColumnTrieIncremental` DS-level aliases in `kermit-ds` | P1 | 2 | Every existing test green with the old flag behaviour; new `ds_choices_*`, `build_mode`, `built_with` and equivalence tests green |
| **P3 — Expose the knob** | 7, 8 | `--ds-build`, the cell and family carrying the mode, `build_relation` made required; `define_multiway_join_test_suite_for_build_mode!` and its two invocations | P2 | 2 | `cargo test -p kermit` green incl. 8 `cli_column_trie_build_mode` tests and 28 `columntrieincremental` join tests; both mutation checks recorded |
| **P4 — Analysis and docs** | 9, 10 | kermit-lab back-fill, ablation guard, contract test; every doc in Task 10 | P3 (the contract test needs `--ds-build`) | 2 | kermit-lab pytest green with `KERMIT_BIN`; `cargo doc` clean |
| *Controller* | 11, 12 | Phase 2 gate, miri, evidence, supervisor checkpoint; landing only on the user's instruction | P4 | — | — |

Each implementer gets: this plan, its package's task numbers, the ground rules above, and a report budget of about 40 lines — commit SHAs, test counts, mutation-check outcomes, and any deviation from the plan with its reason. It must not start the next package, push, or merge (except Task 5 Step 0's `origin/master` merge).

---

# Phase 1 — the one-pass bulk build (P1, then controller)

## Task 1 [P1]: One-pass bulk build with an array-level equivalence test

**Files:**
- Modify: `kermit-ds/src/ds/column_trie/implementation.rs`

The new build must produce *identical* arrays and capacities to today's sort-then-insert build, so today's build is the oracle. The equivalence tests therefore pass both before and after the change (they are characterisation tests); the mixed-arity test is the one that fails first. The mutation check in Step 9 proves the equivalence tests bite.

- [ ] **Step 1: Share the LCG between the randomised tests**

In `implementation.rs`, inside `#[cfg(test)] mod tests`, add at the top of the module (after the `use` block):

```rust
    /// Linear-congruential generator, so the randomised tests need no `rand`
    /// dev-dependency. The constants are Knuth's MMIX ones.
    struct Lcg(u64);

    impl Lcg {
        fn next_usize(&mut self) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) as usize
        }
    }
```

Then rewrite the body of `random_inserts_round_trip_to_sorted_deduped_input` to use it (same seed, same sequence):

```rust
    #[test]
    fn random_inserts_round_trip_to_sorted_deduped_input() {
        let mut rng = Lcg(0x00C0_FFEE_DEAD_BEEF_u64);
        let arity = 3;
        let n = 500;
        let mut tuples: Vec<Vec<usize>> = (0..n)
            .map(|_| (0..arity).map(|_| rng.next_usize() % 50).collect())
            .collect();
        let mut trie = ColumnTrie::new(arity.into());
        for t in &tuples {
            trie.insert(t.clone());
        }
        // Expected: sorted, deduped.
        tuples.sort();
        tuples.dedup();
        let mut collected: Vec<Vec<usize>> = trie.trie_iter().into_iter().collect();
        collected.sort();
        assert_eq!(collected, tuples);
    }
```

Keep the existing doc comment above the test, minus its first line about the LCG (now on `Lcg`).

- [ ] **Step 2: Add the equivalence helpers and tests**

Extend the `use` block of `mod tests` to:

```rust
    use {
        super::ColumnTrie,
        crate::{
            relation::{Projectable, Relation as _},
            HeapSize,
        },
        kermit_iters::TrieIterable,
    };
```

Append to `mod tests`:

```rust
    /// The build before issue #84, kept as the oracle: one `insert` per
    /// tuple, in the order given.
    fn insert_one_by_one(arity: usize, tuples: &[Vec<usize>]) -> ColumnTrie {
        let mut trie = ColumnTrie::new(arity.into());
        for tuple in tuples {
            trie.insert(tuple.clone());
        }
        trie
    }

    /// Asserts two tries are identical down to each `Vec`'s capacity, which
    /// `heap_size_bytes` sums.
    fn assert_identical(actual: &ColumnTrie, expected: &ColumnTrie, case: &str) {
        assert_eq!(
            actual.layers.len(),
            expected.layers.len(),
            "{case}: layer count"
        );
        for (depth, (a, e)) in actual.layers.iter().zip(&expected.layers).enumerate() {
            assert_eq!(a.data, e.data, "{case}: layer {depth} data");
            assert_eq!(a.interval, e.interval, "{case}: layer {depth} interval");
            assert_eq!(
                a.data.capacity(),
                e.data.capacity(),
                "{case}: layer {depth} data capacity"
            );
            assert_eq!(
                a.interval.capacity(),
                e.interval.capacity(),
                "{case}: layer {depth} interval capacity"
            );
        }
        assert_eq!(
            actual.tuple_count, expected.tuple_count,
            "{case}: tuple_count"
        );
        assert_eq!(
            actual.heap_size_bytes(),
            expected.heap_size_bytes(),
            "{case}: heap_size_bytes"
        );
    }

    /// `from_tuples` builds exactly what inserting the same tuples one at a
    /// time builds: the same arrays, the same capacities, the same count and
    /// heap size (issue #84). Small key ranges make duplicates and long
    /// shared prefixes common.
    #[test]
    fn bulk_build_matches_one_by_one_inserts() {
        let seeds: &[u64] = if cfg!(miri) {
            &[1]
        } else {
            &[1, 2, 3, 0x00C0_FFEE_DEAD_BEEF]
        };
        let sizes: &[usize] = if cfg!(miri) {
            &[0, 1, 2, 17]
        } else {
            &[0, 1, 2, 17, 200]
        };
        for &seed in seeds {
            let mut rng = Lcg(seed);
            for arity in 1..=4 {
                for key_range in [2, 5, 50] {
                    for &n in sizes {
                        let tuples: Vec<Vec<usize>> = (0..n)
                            .map(|_| (0..arity).map(|_| rng.next_usize() % key_range).collect())
                            .collect();
                        let case = format!("seed {seed}, arity {arity}, keys < {key_range}, n {n}");
                        let bulk = ColumnTrie::from_tuples(arity.into(), tuples.clone());

                        let mut sorted = tuples.clone();
                        sorted.sort();
                        assert_identical(&bulk, &insert_one_by_one(arity, &sorted), &case);
                        // The layout depends only on the tuple set, so
                        // inserting in arrival order agrees too.
                        assert_identical(
                            &bulk,
                            &insert_one_by_one(arity, &tuples),
                            &format!("{case}, unsorted inserts"),
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn bulk_build_of_duplicates_stores_one_tuple() {
        let tuples = vec![vec![3, 1, 4]; 5];
        let bulk = ColumnTrie::from_tuples(3.into(), tuples.clone());
        assert_eq!(bulk.tuple_count, 1);
        assert_identical(&bulk, &insert_one_by_one(3, &tuples), "all duplicates");
    }

    #[test]
    fn bulk_build_of_no_tuples_is_the_empty_trie() {
        let bulk = ColumnTrie::from_tuples(2.into(), vec![]);
        assert!(bulk
            .layers
            .iter()
            .all(|layer| layer.data.is_empty() && layer.interval.is_empty()));
        assert_identical(&bulk, &ColumnTrie::new(2.into()), "empty");
    }

    /// Arity 0 keeps the result it had before issue #84: no layers and a
    /// count of 0, because `insert` stores nothing for an empty tuple.
    #[test]
    fn bulk_build_of_nullary_tuples_stores_nothing() {
        let tuples = vec![vec![]; 3];
        let bulk = ColumnTrie::from_tuples(0.into(), tuples.clone());
        assert!(bulk.layers.is_empty());
        assert_eq!(bulk.tuple_count, 0);
        assert_identical(&bulk, &insert_one_by_one(0, &tuples), "arity 0");
    }

    /// Pins the worked example in `docs/data-structures/column-trie.md`.
    #[test]
    fn bulk_build_matches_the_documented_example() {
        let trie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        assert_eq!(trie.layers[0].data, vec![1, 2]);
        assert_eq!(trie.layers[0].interval, vec![0]);
        assert_eq!(trie.layers[1].data, vec![2, 3, 4]);
        assert_eq!(trie.layers[1].interval, vec![0, 2]);
    }

    #[test]
    #[should_panic(expected = "does not match header arity")]
    fn bulk_build_rejects_a_tuple_of_the_wrong_arity() {
        ColumnTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3]]);
    }
```

- [ ] **Step 3: Run the new tests against today's build**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib column_trie::implementation::tests`
Expected: every `bulk_build_*` test **passes** except `bulk_build_rejects_a_tuple_of_the_wrong_arity`, which FAILS: today the panic comes from `insert` with the message "tuple arity must match relation arity". The passing ones confirm the oracle and the comparison work on today's build.

- [ ] **Step 4: Add the two push-only layer mutators**

In `impl ColumnTrieLayer`, after `add_interval`, add:

```rust
    /// Appends `key` to the end of this layer's data. Used by the bulk
    /// build, where every key arrives in its final position.
    fn push_key(&mut self, key: usize) { self.data.push(key); }

    /// Opens the child interval of a parent just appended to the layer
    /// above: its children start at the current end of `data`.
    fn open_interval(&mut self) { self.interval.push(self.data.len()); }
```

Replace the last three lines of the `ColumnTrieLayer` doc comment:

```rust
/// All read access is through the methods below; mutation goes through
/// [`insert_key_and_shift_intervals`](Self::insert_key_and_shift_intervals)
/// and [`add_interval`](Self::add_interval).
```

with:

```rust
/// All read access is through the methods below. Incremental `insert`
/// mutates through
/// [`insert_key_and_shift_intervals`](Self::insert_key_and_shift_intervals)
/// and [`add_interval`](Self::add_interval); the bulk build only appends,
/// through [`push_key`](Self::push_key) and
/// [`open_interval`](Self::open_interval).
```

- [ ] **Step 5: Add the bulk builder**

Add a free function just above `enum LayerStep`:

```rust
/// The number of leading positions at which `a` and `b` agree.
fn common_prefix_len(a: &[usize], b: &[usize]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}
```

Inside `impl ColumnTrie` (the block holding `internal_insert` and `step_layer`), add:

```rust
    /// Builds a trie from tuples already sorted lexicographically, in one
    /// pass: append keys layer by layer, and open a new child interval
    /// wherever the prefix changes.
    ///
    /// Each tuple shares some leading keys with its predecessor; those keys
    /// are already stored. So the tuple appends one key to every layer from
    /// the first differing depth down, and every key it appends above the
    /// last layer is a new parent, whose child interval the layer below
    /// opens first. A tuple equal to its predecessor appends nothing.
    ///
    /// Produces exactly the arrays, and the capacities, of inserting the
    /// same tuples one at a time: on sorted input `insert` only ever
    /// appends, so the two builds perform the same pushes in the same
    /// order. Never pre-size these `Vec`s — `heap_size_bytes` sums their
    /// capacities.
    ///
    /// O(n · a) for n tuples of arity a.
    fn from_sorted(header: RelationHeader, sorted: Vec<Vec<usize>>) -> Self {
        let mut trie = Self::new(header);
        if sorted.is_empty() {
            return trie;
        }
        let arity = trie.header.arity();
        // A non-empty trie's root layer holds exactly one interval.
        if let Some(root) = trie.layers.first_mut() {
            root.open_interval();
        }
        let mut previous: Option<Vec<usize>> = None;
        for tuple in sorted {
            // The depth at which this tuple leaves its predecessor's path:
            // the number of leading keys the two share.
            let diverge = previous
                .as_deref()
                .map_or(0, |prev| common_prefix_len(prev, &tuple));
            if diverge == arity {
                // Equal to its predecessor, so already stored.
                continue;
            }
            for (depth, &key) in tuple.iter().enumerate().skip(diverge) {
                let layer = &mut trie.layers[depth];
                if depth > diverge {
                    // The key just appended one layer up is a new parent,
                    // so its children start here.
                    layer.open_interval();
                }
                layer.push_key(key);
            }
            trie.tuple_count += 1;
            previous = Some(tuple);
        }
        trie
    }
```

- [ ] **Step 6: Route `from_tuples` through it**

Replace the whole `fn from_tuples` in `impl Relation for ColumnTrie` with:

```rust
    fn from_tuples(header: RelationHeader, mut tuples: Vec<Vec<usize>>) -> Self {
        if tuples.is_empty() {
            return Self::new(header);
        }
        let arity = header.arity();
        // Checked before the sort: its comparator indexes `b` by `a`'s
        // length, so a shorter tuple would panic there with an index error
        // instead of this message.
        for tuple in &tuples {
            assert_eq!(
                tuple.len(),
                arity,
                "from_tuples: tuple arity {} does not match header arity {arity}",
                tuple.len()
            );
        }
        // Reproduces the derived `Vec<usize>` lexicographic order (kept
        // hand-rolled here rather than `sort_unstable()`).
        tuples.sort_unstable_by(|a, b| {
            for i in 0..a.len() {
                match a[i].cmp(&b[i]) {
                    | std::cmp::Ordering::Less => return std::cmp::Ordering::Less,
                    | std::cmp::Ordering::Greater => return std::cmp::Ordering::Greater,
                    | std::cmp::Ordering::Equal => continue,
                }
            }
            std::cmp::Ordering::Equal
        });
        Self::from_sorted(header, tuples)
    }
```

Update the two doc comments that describe the build:

In the `ColumnTrie` struct doc, replace

```rust
/// overhead and is more cache-friendly for large relations, at the cost of
/// more expensive inserts (keys in later layers must shift when earlier
/// layers grow).
```

with

```rust
/// overhead and is more cache-friendly for large relations, at the cost of
/// more expensive incremental inserts: `insert` must shift the offsets of
/// later intervals when an earlier layer grows. `from_tuples` builds every
/// layer in one pass and pays no such cost.
```

and on the `tuple_count` field replace `/// Number of distinct tuples stored; maintained by `insert`.` with `/// Number of distinct tuples stored; maintained by `insert` and by the bulk build.`

- [ ] **Step 7: Run the tests**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds`
Expected: all pass, including all six `bulk_build_*` tests and the existing pinning, cardinality and heap-size tests.

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests columntrie`
Expected: the 28 ColumnTrie join tests pass.

- [ ] **Step 8: Format, lint, commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit-ds --all-targets -- -D warnings
git -C $WT add kermit-ds/src/ds/column_trie/implementation.rs
git -C $WT commit -F - <<'EOF'
perf(kermit-ds): ColumnTrie builds from sorted tuples in one pass (#84)

from_tuples sorted its input and then inserted tuple by tuple, scanning
each interval from its start: O(n·a·b), about 125 s on the WatDiv
stress-100 sample against ~2 s for the other structures. It now appends
keys layer by layer and opens a child interval wherever the prefix
changes: O(n·a) after the sort.

The build only pushes, so every Vec ends at the capacity the old build
reached and heap_size_bytes is unchanged. An array-level test compares
the two builds (contents, capacities, count, heap size) over a random
grid with duplicates, empty input and arity 0.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EQYT5rUt5xkPAh85YeHs8t
EOF
```

- [ ] **Step 9: Mutation check (two mutants)**

Mutant A — drop the duplicate skip. In `from_sorted`, replace

```rust
            if diverge == arity {
                // Equal to its predecessor, so already stored.
                continue;
            }
```

with nothing. Run `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib bulk_build`. Expected: FAIL (`bulk_build_of_duplicates_stores_one_tuple` and the grid, on `tuple_count`). Restore the exact four lines; re-run; expect PASS.

Mutant B — misplace an interval push. Replace `if depth > diverge {` with `if depth >= diverge {`. Run the same command. Expected: FAIL (`layer … interval`). Restore; re-run; expect PASS. `git -C $WT status` must be clean.

---

## Task 2 [P1]: ColumnTrie reports carry `ds_build_mode: "bulk"`

**Files:**
- Modify: `kermit/src/execution.rs`
- Modify: `kermit/src/bench/ds.rs`
- Modify: `kermit/src/bench/run.rs`
- Create: `kermit/tests/cli_column_trie_build_mode.rs`

kermit-lab will later read "ColumnTrie row without `ds_build_mode`" as `incremental`, so no bulk-built row may ship without the axis. The axis comes from the bench family, not the relation, because the built trie is the same under every build.

- [ ] **Step 1: Write the failing unit test**

Append to `mod tests` in `kermit/src/execution.rs`:

```rust
    /// Every ColumnTrie report says how its relations were built; the
    /// other structures have a single build and carry no such axis (issue
    /// #84).
    #[test]
    fn only_column_trie_families_report_a_build_mode() {
        let bulk = BTreeMap::from([(
            "ds_build_mode".to_string(),
            serde_json::Value::from("bulk"),
        )]);
        assert_eq!(SortedTrieFamily::<ColumnTrie>::new().build_mode_axes(), bulk);
        assert_eq!(
            TrieLftj::<ColumnTrie>::new(Optimiser::Lexicographic).build_mode_axes(),
            bulk
        );
        assert!(SortedTrieFamily::<TreeTrie>::new()
            .build_mode_axes()
            .is_empty());
        assert!(TrieLftj::<TreeTrie>::new(Optimiser::Lexicographic)
            .build_mode_axes()
            .is_empty());
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

- [ ] **Step 2: Write the failing CLI tests**

Create `kermit/tests/cli_column_trie_build_mode.rs`:

```rust
//! CLI smoke test for ColumnTrie's `ds_build_mode` axis (issue #84). Every
//! ColumnTrie report says which build made its relations, so kermit-lab can
//! read a ColumnTrie report *without* the axis as the pre-#84 incremental
//! build. The other structures have a single build and carry no such axis.

mod common;

use common::cli::{axes_of, bench_ds, bench_join, bench_run, reports_of};

#[test]
fn cli_bench_ds_column_trie_records_bulk_build_mode() {
    let (output, report) = bench_ds("column-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "bulk");
}

#[test]
fn cli_bench_ds_other_structures_have_no_build_mode() {
    for ds in ["tree-trie", "hash-trie"] {
        let (output, report) = bench_ds(ds, &[]);
        assert!(
            output.status.success(),
            "{ds}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let axes = axes_of(&report);
        assert!(axes.get("ds_build_mode").is_none(), "{ds}: {axes}");
    }
}

#[test]
fn cli_bench_run_sweep_reports_build_mode_only_on_column_trie() {
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
        if axes["data_structure"] == "ColumnTrie" {
            assert_eq!(axes["ds_build_mode"], "bulk", "{axes}");
        } else {
            assert!(axes.get("ds_build_mode").is_none(), "{axes}");
        }
    }
}

#[test]
fn cli_bench_join_column_trie_records_bulk_build_mode() {
    let (output, report) = bench_join("column-trie", "leapfrog-triejoin", &["-m", "space"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "bulk");
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit only_column_trie_families_report_a_build_mode`
Expected: compile error, `no method named build_mode_axes`.

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_column_trie_build_mode`
Expected: the three ColumnTrie tests FAIL (`ds_build_mode` is `null`); `cli_bench_ds_other_structures_have_no_build_mode` passes. (Integration tests build the binary without its `#[cfg(test)]` module, so the unit test's compile error does not block them.)

- [ ] **Step 4: Add the hook**

In `kermit/src/execution.rs`:

Add to `trait SortedTrieRelation`, after `const KIND`:

```rust
    /// The `ds_build_mode` axis of every relation the bench builds of this
    /// type. The build process leaves no trace in the built structure, so
    /// the type that ran it reports it. Empty for a structure with a single
    /// build process.
    fn build_mode_axes() -> BTreeMap<String, serde_json::Value> { BTreeMap::new() }
```

Replace `impl SortedTrieRelation for ColumnTrie { … }` with:

```rust
impl SortedTrieRelation for ColumnTrie {
    const KIND: SortedTrie = SortedTrie::ColumnTrie;

    /// `from_tuples` is the one-pass bulk build (issue #84). Reported so a
    /// row built this way is never read as a pre-#84 row, which kermit-lab
    /// treats as `incremental`.
    fn build_mode_axes() -> BTreeMap<String, serde_json::Value> {
        BTreeMap::from([(
            "ds_build_mode".to_string(),
            serde_json::Value::from("bulk"),
        )])
    }
}
```

Add to `trait RelationFamily`, after `optimization_axes`:

```rust
    /// The `ds_build_mode` axis of the relations this family builds, merged
    /// into the report's axes. A build mode describes the build, and every
    /// mode builds the same structure, so the family that ran the build
    /// reports it rather than the relation. Empty for structures with a
    /// single build process.
    fn build_mode_axes(&self) -> BTreeMap<String, serde_json::Value> { BTreeMap::new() }
```

In `impl<R: SortedTrieRelation + 'static> RelationFamily for SortedTrieFamily<R>` add:

```rust
    fn build_mode_axes(&self) -> BTreeMap<String, serde_json::Value> { R::build_mode_axes() }
```

In `impl<R: SortedTrieRelation + 'static> RelationFamily for TrieLftj<R>` add:

```rust
    fn build_mode_axes(&self) -> BTreeMap<String, serde_json::Value> {
        self.structure.build_mode_axes()
    }
```

The hash families keep the empty default.

- [ ] **Step 5: Merge it into both reports**

In `kermit/src/bench/ds.rs`, replace

```rust
    // Standard optimization axes: merge in dimensions emitted by the DS.
    // The `ds_layout_*` / `ds_config_*` / `ds_build_mode` naming convention
    // (see `kermit_iters::HasOptimizationAxes`) guarantees no collision
    // with the base axes assembled above. Empty for the sorted tries.
    axes.extend(F::optimization_axes(&relation));
```

with

```rust
    // Standard optimization axes: the relation's Layout / Config dimensions
    // (empty for the sorted tries), then the build mode, which only the
    // family that ran the build can report. The `ds_layout_*` /
    // `ds_config_*` / `ds_build_mode` naming convention (see
    // `kermit_iters::HasOptimizationAxes`) guarantees no collision with the
    // base axes assembled above.
    axes.extend(F::optimization_axes(&relation));
    axes.extend(family.build_mode_axes());
```

In `kermit/src/bench/run.rs`, replace

```rust
    let optimization_axes = relations
        .first()
        .map(|r| F::optimization_axes(r))
        .unwrap_or_default();
```

with

```rust
    let mut optimization_axes = relations
        .first()
        .map(|r| F::optimization_axes(r))
        .unwrap_or_default();
    // The build mode comes from the family, not a relation: it describes the
    // build, which leaves no trace in the structure, and it must be present
    // even for a workload with no relations.
    optimization_axes.extend(family.build_mode_axes());
```

- [ ] **Step 6: Run the tests**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit execution`
Expected: PASS, including `only_column_trie_families_report_a_build_mode`.

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_column_trie_build_mode --test cli_hash_trie_layout_pruning --test cli_bench_join_axes`
Expected: PASS.

- [ ] **Step 7: Format, lint, commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit --all-targets -- -D warnings
git -C $WT add kermit/src/execution.rs kermit/src/bench/ds.rs kermit/src/bench/run.rs kermit/tests/cli_column_trie_build_mode.rs
git -C $WT commit -F - <<'EOF'
feat(bench): ColumnTrie reports carry ds_build_mode (#84)

A new RelationFamily::build_mode_axes hook stamps ds_build_mode: "bulk"
on every ColumnTrie report (bench ds, run and join). The axis comes from
the family that ran the build, because the built trie is the same under
every build, and it is merged even when a workload has no relations.

kermit-lab will read a ColumnTrie row without the axis as the pre-#84
incremental build, so the axis has to ship with the bulk build.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EQYT5rUt5xkPAh85YeHs8t
EOF
```

- [ ] **Step 8: Mutation check**

Mutant — drop the `bench run` merge: delete the line `optimization_axes.extend(family.build_mode_axes());` in `run.rs`. Run `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_column_trie_build_mode`. Expected: FAIL in `cli_bench_run_sweep_reports_build_mode_only_on_column_trie` and `cli_bench_join_column_trie_records_bulk_build_mode`. Restore the line; re-run; expect PASS. `git status` clean.

---

## Task 3 [P1]: Document the one-pass build

**Files:**
- Modify: `docs/data-structures/column-trie.md`
- Modify: `docs/specs/bench-report-schema.md`

- [ ] **Step 1: `column-trie.md` — representation and construction**

Replace the paragraph

```markdown
Compared to [`TreeTrie`](./tree-trie.md), this layout avoids per-node allocation and is cache-friendly for large relations, at the cost of expensive inserts — placing a key in an early layer can force later-layer offsets to shift.
```

with

```markdown
Compared to [`TreeTrie`](./tree-trie.md), this layout avoids per-node allocation and is cache-friendly for large relations, at the cost of expensive incremental inserts — `insert` placing a key in an early layer can force later-layer offsets to shift. Building from a known set of tuples avoids that cost; see Construction.

### Construction

`from_tuples` sorts the tuples lexicographically, then builds every layer in one pass (`ColumnTrie::from_sorted`). Each tuple shares some leading keys with the tuple before it; call that count its *divergence depth* `d`. Those keys are already stored, so the tuple appends one key to every layer from `d` down. Every key it appends above the last layer is a new parent, so the layer below first opens a child interval for it (`interval.push(data.len())`). A tuple whose divergence depth equals the arity duplicates its predecessor and appends nothing. The first tuple also opens the root layer's single interval.

For `{(1, 2), (1, 3), (2, 4)}` the divergence depths are 0, 1 and 0:

| Tuple | `d` | Layer 0 | Layer 1 |
|---|---|---|---|
| `(1, 2)` | 0 (first) | open interval 0; push `1` | open interval 0; push `2` |
| `(1, 3)` | 1 | — | push `3` |
| `(2, 4)` | 0 | push `2` | open interval 2; push `4` |

That yields the arrays of the worked micro-example below. Incremental `insert` reaches the same arrays by another route: it scans the target interval for the key's position, inserts there, and shifts the start offset of every later interval.
```

- [ ] **Step 2: `column-trie.md` — invariant and complexity**

After the `**Interval bounds.**` bullet in Invariants, add:

```markdown
- **Canonical layout.** The arrays depend only on the tuple *set*: each layer's `data` is every parent's sorted children, concatenated in parent order. So `from_tuples` and any sequence of `insert` calls over the same tuples produce identical arrays — down to each `Vec`'s capacity, because both grow one element at a time — and so the same `heap_size_bytes`. Pinned by `bulk_build_matches_one_by_one_inserts`.
```

Replace the complexity row

```markdown
| `from_tuples(n)` | O(n · a · b + n · a · log n) | O(n · a) | sort lexicographically, then insert |
```

with

```markdown
| `from_tuples(n)` | O(n · a · log n) | O(n · a) | sort lexicographically (O(n · a · log n)), then build every layer in one pass (O(n · a)); see Construction. Before issue #84 it inserted tuple by tuple, O(n · a · b) |
```

- [ ] **Step 3: `bench-report-schema.md` — the axis and the change-log row**

In "Standard axis prefixes", replace

```markdown
- `ds_build_mode` — construction-time build mode for the data structure
  (single key; value is a `<mode>[:<params>]` string, e.g., `parallel:8`).
```

with

```markdown
- `ds_build_mode` — construction-time build mode for the data structure
  (single key; value is a `<mode>[:<params>]` string, e.g., `bulk`). Emitted
  only on ColumnTrie reports, by the bench family that ran the build rather
  than by the relation, since every build mode builds the same structure.
```

Append to the Change log table:

```markdown
| 3 (no bump) | 2026-10-03 | ColumnTrie's `from_tuples` builds in one pass (#84), so its `insertion` and `end_to_end` values drop; every ColumnTrie report now carries `ds_build_mode: "bulk"`, which tells it apart from earlier ColumnTrie rows. Additive, so `schema_version` stays `3`. |
```

- [ ] **Step 4: Commit**

```bash
git -C $WT add docs/data-structures/column-trie.md docs/specs/bench-report-schema.md
git -C $WT commit -F - <<'EOF'
docs(column-trie): the one-pass build (#84)

Construction section with the divergence-depth walk-through, the
canonical-layout invariant, the new from_tuples complexity, and the
ds_build_mode axis in the report schema.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EQYT5rUt5xkPAh85YeHs8t
EOF
```

---

## Task 4 [controller]: Phase 1 gate, evidence, checkpoint

- [ ] **Step 1: Full gate**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace 2>&1 | tail -40
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --workspace --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings nix develop $WT --command cargo doc --workspace --no-deps
nix develop $WT --command cargo fmt --all -- --check
```

Expected: all green. `e2e_watdiv` can fail intermittently under the parallel run; re-run it alone (`cargo test -p kermit-rdf --test e2e_watdiv`) before suspecting this change.

- [ ] **Step 2: Miri on `kermit-ds` (detached, private sysroot)**

```bash
setsid nohup nix develop $WT --command bash -c "export MIRI_SYSROOT=$SCRATCH/miri-sysroot MIRIFLAGS=-Zmiri-disable-isolation CARGO_BUILD_JOBS=2 && cd $WT && cargo miri setup && cargo miri test -p kermit-ds; echo EXIT=\$?" > $SCRATCH/miri-phase1.log 2>&1 < /dev/null & disown
```

Wait for it with a background `until grep -q '^EXIT=' $SCRATCH/miri-phase1.log; do sleep 10; done`. Expected: `EXIT=0`, `test result: ok` for every `kermit-ds` test binary.

- [ ] **Step 3: Evidence — before and after on WatDiv `friendof`**

```bash
REL=$HOME/.cache/kermit/benchmarks/watdiv-stress-100-test-1-prelim/friendof.parquet
mkdir -p $SCRATCH/kermit-base $SCRATCH/ab-run
git -C $WT archive 62f722e | tar -x -C $SCRATCH/kermit-base
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build --release -p kermit --manifest-path $SCRATCH/kermit-base/Cargo.toml
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build --release -p kermit
```

The baseline needs about 10 × 34 s, so run it detached:

```bash
setsid nohup env -C $SCRATCH/ab-run $SCRATCH/kermit-base/target/release/kermit bench --sample-size 10 --measurement-time 1 --warm-up-time 1 --name col84-base --report-json $SCRATCH/ab-run/base.json ds --relation $REL -i column-trie -m insertion > $SCRATCH/ab-base.log 2>&1 < /dev/null & disown
```

When `$SCRATCH/ab-base.log` shows the summary, run the new binary in the foreground (never concurrently with a build):

```bash
env -C $SCRATCH/ab-run $WT/target/release/kermit bench --sample-size 10 --measurement-time 1 --warm-up-time 1 --name col84-bulk --report-json $SCRATCH/ab-run/bulk.json ds --relation $REL -i column-trie -m insertion
for g in col84-base col84-bulk; do printf '%s ' $g; jq '.mean.point_estimate / 1e9' $SCRATCH/ab-run/target/criterion/$g/ColumnTrie_insertion/new/estimates.json; done
```

Expected: `col84-base` ≈ 34 s; `col84-bulk` well under 1 s. Record both numbers.

- [ ] **Step 4: Checkpoint 2 — report to the supervisor and the user**

SendMessage to the supervisor: the three phase-1 commit SHAs, the gate results (test counts, clippy/doc/fmt/miri), both mutation-check outcomes, and the two `friendof` timings. Tell the user the same in a short summary.

Landing order (supervisor): phase 1 unblocks the authoritative sweep's `insertion` re-run, so it should reach master first — as soon as checkpoint 2 passes **and the user approves the push**. Ask the user; if approved, follow Task 12 for phase 1 alone (supervisor checkpoint 3 first). Then continue with phase 2.

---

# Phase 2 — the `--ds-build` knob (P2, P3, P4, then controller)

## Task 5 [P2]: Resolved `--ds-*` choices travel as one `DsChoices`

**Files:**
- Modify: `kermit/src/options.rs`
- Modify: `kermit/src/execution.rs`
- Modify: `kermit/src/bench/run.rs`
- Modify: `kermit/src/bench/ds.rs`
- Modify: `kermit/src/main.rs`

Behaviour-preserving refactor: the existing tests are the safety net; one new test pins `resolve`. The existing `validate_layout_choices*` / `validate_config_choices*` tests must keep passing unchanged (their signatures do not change) — that is the evidence hash-trie handling did not move.

- [ ] **Step 0: Catch up with master before changing signatures**

#78's P1 edits `execution.rs` (a `read_relation_header` beside `read_relation`), `main.rs`'s `load_query_runner` and `bench run`'s setup — exactly where this task and Task 7 change signatures. Run `git -C $WT fetch origin`; if `origin/master` has moved since the branch point, `git -C $WT merge origin/master` now (never rebase), resolve, and re-run `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit` before Step 1, so the refactor is written against the current code.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `kermit/src/options.rs`:

```rust
    #[test]
    fn ds_choices_resolve_applies_flags_and_defaults() {
        let layout = LayoutChoices {
            hash_trie_hasher: Some(HasherChoice::Fxhash),
            ..LayoutChoices::default()
        };
        let config = ConfigChoices {
            ds_config: vec!["load-factor=0.5".into()],
        };
        let choices = DsChoices::resolve(IndexStructureSelector::HashTrie, &layout, &config).unwrap();
        assert_eq!(choices.hasher, HasherChoice::Fxhash);
        assert_eq!(choices.pruning, PruningChoice::Off);
        assert_eq!(choices.config.load_factor, LoadFactor::percent(50).unwrap());
        assert_eq!(
            DsChoices::resolve(
                IndexStructureSelector::TreeTrie,
                &LayoutChoices::default(),
                &ConfigChoices::default()
            )
            .unwrap(),
            DsChoices::default()
        );
    }

    /// `resolve` validates before it resolves, so no command can act on a
    /// flag the selected structure would ignore.
    #[test]
    fn ds_choices_resolve_rejects_flags_the_structure_lacks() {
        let layout = LayoutChoices {
            hash_trie_pruning: Some(PruningChoice::On),
            ..LayoutChoices::default()
        };
        let msg = DsChoices::resolve(
            IndexStructureSelector::TreeTrie,
            &layout,
            &ConfigChoices::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(msg.contains("--ds-layout-pruning"), "{msg}");
        let config = ConfigChoices {
            ds_config: vec!["load-factor=0.5".into()],
        };
        let msg = DsChoices::resolve(
            IndexStructureSelector::ColumnTrie,
            &LayoutChoices::default(),
            &config,
        )
        .unwrap_err()
        .to_string();
        assert!(msg.contains("--ds-config"), "{msg}");
    }
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit ds_choices`
Expected: compile error, `cannot find type DsChoices`.

- [ ] **Step 2: Add `DsChoices`**

In `kermit/src/options.rs`, after `validate_config_choices`, add:

```rust
/// The resolved value of every `--ds-*` option for one command — what the
/// execution cells are built from. Commands obtain it from
/// [`DsChoices::resolve`], which rejects a flag the selected structure
/// would ignore before applying any default.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DsChoices {
    /// `--ds-layout-hasher`; reaches the hash-trie cell only.
    pub hasher: HasherChoice,
    /// `--ds-layout-pruning`; reaches the hash-trie cell only.
    pub pruning: PruningChoice,
    /// `--ds-config`; reaches the hash-trie cell only.
    pub config: HashTrieConfig,
}

impl DsChoices {
    /// Validates every `--ds-*` flag against `indexstructure`, then resolves
    /// them, applying each option's default where no flag was given.
    ///
    /// # Errors
    ///
    /// Returns an error if a flag was given for a structure that lacks its
    /// axis, or if `--ds-config` is malformed.
    pub(crate) fn resolve(
        indexstructure: IndexStructureSelector, layout: &LayoutChoices, config: &ConfigChoices,
    ) -> anyhow::Result<Self> {
        validate_layout_choices(indexstructure, layout)?;
        validate_config_choices(indexstructure, config)?;
        Ok(Self {
            hasher: layout.hash_trie_hasher_resolved(),
            pruning: layout.hash_trie_pruning_resolved(),
            config: config.hash_trie_config_resolved()?,
        })
    }
}
```

In the `with_hash_trie_layout!` doc comment, replace

```rust
/// a `LayoutChoices` field plus one entry in each of `Execution::HashHtj`,
/// `Execution::for_pair`, `Sweep::expand`, `HashHtj`, and the two command
/// entry points (`run_ds_bench_command`, `run_bench_run_command`). Before a
```

with

```rust
/// a `LayoutChoices` field plus one field each in `DsChoices` and
/// `Execution::HashHtj`, and the matching arms in `HashHtj`. Before a
```

- [ ] **Step 3: Take `DsChoices` in `execution.rs`**

Change the import to `crate::options::{hasher_of, pruning_of, DsChoices, HasherChoice, PruningChoice}`.

Replace `Execution::for_pair`'s signature and first line:

```rust
    /// The only way to obtain an `Execution` from a concrete pair. Returns
    /// `None` for the three incompatible pairs, which the sweep skips.
    /// `choices` reach the cells that have each axis: today the hash-trie
    /// cell's hasher, pruning and config.
    pub fn for_pair(
        ds: IndexStructure, algo: JoinAlgorithm, choices: DsChoices,
    ) -> Option<Execution> {
        let DsChoices {
            hasher,
            pruning,
            config,
        } = choices;
        match (ds, algo) {
```

(the match body is unchanged). Likewise `for_structure`:

```rust
    pub fn for_structure(ds: IndexStructure, choices: DsChoices) -> Execution {
        let DsChoices {
            hasher,
            pruning,
            config,
        } = choices;
        match ds {
```

And `Sweep::expand`:

```rust
    /// Expands the cross product of `structures × algorithms` into valid
    /// cells, partitioning off the incompatible pairs. `choices` reach the
    /// cells that have each axis (see [`Execution::for_pair`]).
    pub fn expand(
        structures: &[IndexStructure], algorithms: &[JoinAlgorithm], choices: DsChoices,
    ) -> Sweep {
        let mut cells = Vec::new();
        let mut skipped = Vec::new();
        for &ds in structures {
            for &algo in algorithms {
                match Execution::for_pair(ds, algo, choices) {
```

Update the tests in `mod tests`:
- the three `Sweep::expand(…, HasherChoice::Sip, PruningChoice::Off, HashTrieConfig::default())` calls become `Sweep::expand(…, DsChoices::default())`;
- in `execution_axes_round_trip_through_for_pair`, declare `let fx = DsChoices { hasher: HasherChoice::Fxhash, ..DsChoices::default() };` and call `Execution::for_pair(ds, algo, fx)` and `Execution::for_pair(IndexStructure::HashTrie, JoinAlgorithm::HashTriejoin, fx)`;
- in `for_structure_agrees_with_for_pair`, inside the two loops build `let choices = DsChoices { hasher, pruning, config };` and call `Execution::for_structure(ds, choices)` and `Execution::for_pair(ds, cell.algorithm(), choices)`.

- [ ] **Step 4: Take `DsChoices` in the bench dispatchers**

`kermit/src/bench/run.rs` — replace `resolve_sweep`'s signature and the `Sweep::expand` call:

```rust
pub(crate) fn resolve_sweep(
    indexstructure: IndexStructureSelector, algorithm: JoinAlgorithmSelector, choices: DsChoices,
) -> anyhow::Result<Vec<Execution>> {
    let sweep = Sweep::expand(&indexstructure.expand(), &algorithm.expand(), choices);
```

Change the imports: `options::{with_hash_trie_layout, DsChoices}` and `kermit_ds::Relation` (drop `HasherChoice`, `PruningChoice`, `HashTrieConfig`; the compiler confirms they are unused).

`kermit/src/bench/ds.rs` — `dispatch_ds_bench` drops to 7 parameters, so remove its `#[allow(clippy::too_many_arguments)]`:

```rust
pub(crate) fn dispatch_ds_bench(
    ds: IndexStructure, choices: DsChoices, relation: &Path, metrics: &[Metric],
    queries_per_build: u32, group_name: &str, bench_args: &BenchArgs,
) -> anyhow::Result<BenchReport> {
    match Execution::for_structure(ds, choices) {
```

Change the imports: `options::{with_hash_trie_layout, DsChoices}` and `kermit_ds::{IndexStructure, Relation}`.

- [ ] **Step 5: Resolve once per command in `main.rs`**

Change the options import to `options::{with_hash_trie_layout, ConfigChoices, DsChoices, LayoutChoices}`.

In `load_query_runner`, replace the `Execution::for_pair(…)` call's five arguments:

```rust
    let cell = Execution::for_pair(args.indexstructure, args.algorithm, DsChoices {
        hasher: args.layout.hash_trie_hasher_resolved(),
        pruning: args.layout.hash_trie_pruning_resolved(),
        config,
    })
```

(keep its inline layout check and the `.ok_or_else(…)` unchanged).

In `run_bench_join`, replace the validation, resolution and cell construction with:

```rust
    let selector = IndexStructureSelector::of(query_args.indexstructure);
    let choices = DsChoices::resolve(selector, &query_args.layout, &config)?;
    let cell = Execution::for_pair(query_args.indexstructure, query_args.algorithm, choices)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "incompatible selection: {:?} cannot run under {:?}",
                query_args.indexstructure,
                query_args.algorithm
            )
        })?;
```

and in its `--output` branch call `load_query_runner(&query_args, choices.config)?`.

In `run_ds_bench_command`, replace the two `validate_*` lines and `let hash_trie_config = …` with `let choices = DsChoices::resolve(indexstructure, &layout, &config)?;`, and the `dispatch_ds_bench` call with:

```rust
        let report = dispatch_ds_bench(
            ds,
            choices,
            &relation,
            &metrics,
            queries_per_build,
            group_name,
            bench_args,
        )?;
```

In `run_bench_run_command`, replace the same three lines with `let choices = DsChoices::resolve(indexstructure, &layout, &config)?;` and the `resolve_sweep` call with `let cells = resolve_sweep(indexstructure, algorithm, choices)?;`.

- [ ] **Step 6: Run the tests**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit`
Expected: all pass, including the two new `ds_choices_*` tests and every `cli_*` test (the flag behaviour is unchanged).

- [ ] **Step 7: Format, lint, commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit --all-targets -- -D warnings
git -C $WT add kermit/src
git -C $WT commit -F - <<'EOF'
refactor(kermit): resolved --ds-* choices travel as one DsChoices

DsChoices::resolve validates every --ds-* flag against the selected
structure and then resolves it, so no command can act on a flag the
structure would ignore. Execution::for_pair / for_structure,
Sweep::expand, resolve_sweep and dispatch_ds_bench take the bundle
instead of one parameter per axis, ready for a fourth (--ds-build, #84).
No behaviour change.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EQYT5rUt5xkPAh85YeHs8t
EOF
```

---

## Task 6 [P2]: `ColumnTrieBuildMode`, `BuildModeRelation` and `BuiltWith`

**Files:**
- Create: `kermit-ds/src/ds/column_trie/build_mode.rs`
- Create: `kermit-ds/src/built_with.rs`
- Modify: `kermit-ds/src/ds/column_trie/mod.rs`, `kermit-ds/src/ds/column_trie/implementation.rs`
- Modify: `kermit-ds/src/ds/mod.rs`, `kermit-ds/src/relation.rs`, `kermit-ds/src/lib.rs`
- Modify: `kermit-ds/tests/trie_tests.rs`, `kermit-ds/tests/parquet_tests.rs`

- [ ] **Step 1: The mode, test first**

Create `kermit-ds/src/ds/column_trie/build_mode.rs`:

```rust
//! [`ColumnTrieBuildMode`]: how a [`ColumnTrie`](super::ColumnTrie) is built
//! from a known set of tuples — the BuildMode category of the optimization
//! standard (`docs/specs/optimization-standard.md`).

/// How a [`ColumnTrie`](super::ColumnTrie) is built from a known set of
/// tuples. Both modes sort the tuples first and build identical layers —
/// the same arrays and the same capacities — so the mode changes how long
/// the build takes, never the trie it builds (issue #84).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ColumnTrieBuildMode {
    /// One `insert` per tuple, in sorted order: the build before issue #84,
    /// kept so its measurements can be reproduced. Each insert scans its
    /// interval from the start, so the build is O(n · a · b).
    Incremental,
    /// One pass over the sorted tuples, appending keys layer by layer and
    /// opening a child interval wherever the prefix changes. O(n · a).
    #[default]
    Bulk,
}

impl kermit_iters::BuildMode for ColumnTrieBuildMode {
    fn axis_value(&self) -> String {
        match self {
            | Self::Incremental => "incremental",
            | Self::Bulk => "bulk",
        }
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use {super::*, clap::ValueEnum, kermit_iters::BuildMode};

    /// Pins `axis_value` to clap's derived value name, so a report's
    /// `ds_build_mode` is always what the user typed after `--ds-build`.
    #[test]
    fn axis_values_match_clap_value_names() {
        for mode in ColumnTrieBuildMode::value_variants() {
            assert_eq!(
                mode.axis_value(),
                mode.to_possible_value().unwrap().get_name()
            );
        }
    }

    /// The labels name every ColumnTrie report's `ds_build_mode`, and
    /// kermit-lab reads a missing axis as `"incremental"`.
    #[test]
    fn axis_values_and_default_are_pinned() {
        assert_eq!(ColumnTrieBuildMode::Incremental.axis_value(), "incremental");
        assert_eq!(ColumnTrieBuildMode::Bulk.axis_value(), "bulk");
        assert_eq!(ColumnTrieBuildMode::default(), ColumnTrieBuildMode::Bulk);
    }
}
```

Register it. `kermit-ds/src/ds/column_trie/mod.rs` becomes:

```rust
//! Column-oriented (flattened) trie implementation.

mod build_mode;
mod column_trie_iter;
mod implementation;

pub use {build_mode::ColumnTrieBuildMode, implementation::ColumnTrie};
```

In `kermit-ds/src/ds/mod.rs` change `column_trie::ColumnTrie,` in the `pub use` to `column_trie::{ColumnTrie, ColumnTrieBuildMode},`. In `kermit-ds/src/lib.rs` add `ColumnTrieBuildMode` to the `ds::{…}` re-export list.

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib build_mode`
Expected: PASS (2 tests).

- [ ] **Step 2: The construction seam**

In `kermit-ds/src/relation.rs`, after `trait ConfigurableRelation`, add:

```rust
/// A [`Relation`] with more than one way to build from a known set of
/// tuples — the BuildMode category of the optimization standard
/// (`docs/specs/optimization-standard.md`).
///
/// Every mode must build the *same* relation: same contents, same layout,
/// same [`HeapSize`](crate::HeapSize). Only the construction process
/// differs. [`Relation::from_tuples`] must equal
/// `from_tuples_with_build_mode(header, Self::BuildMode::default(), tuples)`.
/// Wrapper types that exist to inject a mode (see `BuiltWith` in
/// `built_with.rs`) deliberately do not implement this.
///
/// Only structures with a BuildMode axis implement this.
pub trait BuildModeRelation: Relation {
    /// The construction processes this structure offers.
    type BuildMode: kermit_iters::BuildMode + Copy + Default;

    /// Creates a relation populated with `tuples`, built by `mode`. Same
    /// contract as [`Relation::from_tuples`].
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: Self::BuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self;
}
```

Add `BuildModeRelation` to the `relation::{…}` re-export in `lib.rs`.

- [ ] **Step 3: Reshape the equivalence test around the two modes (failing)**

In `implementation.rs`'s `mod tests`, extend the `use` block with `super::ColumnTrieBuildMode` and `crate::relation::BuildModeRelation`, then replace the two assertions inside the innermost loop of `bulk_build_matches_one_by_one_inserts` with:

```rust
                        let incremental = ColumnTrie::from_tuples_with_build_mode(
                            arity.into(),
                            ColumnTrieBuildMode::Incremental,
                            tuples.clone(),
                        );
                        assert_identical(&bulk, &incremental, &case);
                        // The layout depends only on the tuple set, so
                        // inserting in arrival order agrees too.
                        assert_identical(
                            &bulk,
                            &insert_one_by_one(arity, &tuples),
                            &format!("{case}, unsorted inserts"),
                        );
```

and the line building `bulk` with:

```rust
                        let bulk = ColumnTrie::from_tuples_with_build_mode(
                            arity.into(),
                            ColumnTrieBuildMode::Bulk,
                            tuples.clone(),
                        );
```

Delete the now-unused `let mut sorted …; sorted.sort();` lines. Rename the test to `bulk_and_incremental_builds_are_identical` and update its doc comment to: "Every build mode builds exactly the same trie: the same arrays, the same capacities, the same count and heap size (issue #84)."

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib column_trie`
Expected: compile errors — the `super::ColumnTrieBuildMode` import is unresolved and `ColumnTrie` has no `from_tuples_with_build_mode`.

- [ ] **Step 4: Implement the modes on `ColumnTrie`**

In `implementation.rs`, add `super::build_mode::ColumnTrieBuildMode` and `crate::relation::BuildModeRelation` to the top `use` block.

Inside `impl ColumnTrie` (next to `from_sorted`) add:

```rust
    /// Builds a trie from sorted tuples by inserting them one at a time —
    /// the build before issue #84, kept as
    /// [`ColumnTrieBuildMode::Incremental`] so its measurements can be
    /// reproduced. Each `insert` scans its interval from the start, so this
    /// is O(n · a · b).
    fn from_sorted_by_insertion(header: RelationHeader, sorted: Vec<Vec<usize>>) -> Self {
        let mut trie = Self::new(header);
        for tuple in sorted {
            trie.insert(tuple);
        }
        trie
    }
```

Add the trait impl after `impl Relation for ColumnTrie`, moving the prologue there:

```rust
impl BuildModeRelation for ColumnTrie {
    type BuildMode = ColumnTrieBuildMode;

    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: ColumnTrieBuildMode, mut tuples: Vec<Vec<usize>>,
    ) -> Self {
        if tuples.is_empty() {
            return Self::new(header);
        }
        let arity = header.arity();
        // Checked before the sort: its comparator indexes `b` by `a`'s
        // length, so a shorter tuple would panic there with an index error
        // instead of this message.
        for tuple in &tuples {
            assert_eq!(
                tuple.len(),
                arity,
                "from_tuples: tuple arity {} does not match header arity {arity}",
                tuple.len()
            );
        }
        // Reproduces the derived `Vec<usize>` lexicographic order (kept
        // hand-rolled here rather than `sort_unstable()`).
        tuples.sort_unstable_by(|a, b| {
            for i in 0..a.len() {
                match a[i].cmp(&b[i]) {
                    | std::cmp::Ordering::Less => return std::cmp::Ordering::Less,
                    | std::cmp::Ordering::Greater => return std::cmp::Ordering::Greater,
                    | std::cmp::Ordering::Equal => continue,
                }
            }
            std::cmp::Ordering::Equal
        });
        match mode {
            | ColumnTrieBuildMode::Incremental => Self::from_sorted_by_insertion(header, tuples),
            | ColumnTrieBuildMode::Bulk => Self::from_sorted(header, tuples),
        }
    }
}
```

Replace `fn from_tuples` in `impl Relation for ColumnTrie` with:

```rust
    /// Builds with the default [`ColumnTrieBuildMode`], `Bulk`.
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        Self::from_tuples_with_build_mode(header, ColumnTrieBuildMode::default(), tuples)
    }
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib column_trie`
Expected: PASS, including `bulk_and_incremental_builds_are_identical` and the arity, duplicate, empty and documented-example tests.

- [ ] **Step 5: `BuiltWith`, test first**

Create `kermit-ds/src/built_with.rs`:

```rust
//! [`BuiltWith`]: lifts a runtime [`BuildModeRelation::BuildMode`] value to
//! the type level, as [`Configured`](crate::Configured) does for a Config.
//!
//! Test suites in this workspace are macro-generated and name a relation by
//! a single type identifier. A build mode has no type-level identity, so
//! `BuiltWith<R, P>` pairs a relation `R` with a zero-sized marker
//! `P: BuildModeProvider<R::BuildMode>` that supplies the mode. The wrapper
//! delegates every trait to `R`; only `from_tuples` differs, routing
//! through `P::build_mode()`.
//!
//! Like `Configured`, this is test scaffolding shipped in the library so
//! that `kermit-ds` and `kermit` integration tests share one definition.

use {
    crate::{
        cardinality::Cardinality,
        heap_size::HeapSize,
        relation::{BuildModeRelation, Projectable, Relation, RelationHeader},
    },
    kermit_iters::{JoinIterable, TrieIterable, TrieIterator},
    std::{marker::PhantomData, ops::Deref},
};

/// A zero-sized marker that names one build mode.
pub trait BuildModeProvider<M> {
    /// The build mode this marker stands for.
    fn build_mode() -> M;
}

/// Declares a [`BuildModeProvider`] marker type.
///
/// ```
/// use kermit_ds::{
///     define_build_mode_provider, BuiltWith, ColumnTrie, ColumnTrieBuildMode, Relation,
/// };
///
/// define_build_mode_provider!(
///     Incremental,
///     ColumnTrieBuildMode,
///     ColumnTrieBuildMode::Incremental
/// );
///
/// type ColumnTrieIncremental = BuiltWith<ColumnTrie, Incremental>;
///
/// let r = ColumnTrieIncremental::from_tuples(2.into(), vec![vec![1, 2]]);
/// assert_eq!(r.header().arity(), 2);
/// ```
#[macro_export]
macro_rules! define_build_mode_provider {
    ($name:ident, $mode:ty, $value:expr $(,)?) => {
        /// `BuildModeProvider` marker declared by
        /// `define_build_mode_provider!`; supplies one fixed build mode.
        #[derive(Copy, Clone, Debug, Default)]
        pub struct $name;

        impl $crate::BuildModeProvider<$mode> for $name {
            fn build_mode() -> $mode { $value }
        }
    };
}

/// `R` built by the mode `P::build_mode()`.
///
/// Derefs to `R`. Unlike [`Configured`](crate::Configured), nothing in the
/// built relation records the mode — every mode builds the same relation —
/// so there is no value for the marker to drift from, and
/// [`Projectable::project`] may rebuild through the default mode without
/// changing anything observable.
pub struct BuiltWith<R, P> {
    inner: R,
    _provider: PhantomData<P>,
}

impl<R, P> BuiltWith<R, P> {
    fn wrap(inner: R) -> Self {
        Self {
            inner,
            _provider: PhantomData,
        }
    }

    /// Unwraps the relation.
    pub fn into_inner(self) -> R { self.inner }
}

impl<R, P> Deref for BuiltWith<R, P> {
    type Target = R;

    fn deref(&self) -> &R { &self.inner }
}

impl<R: JoinIterable, P> JoinIterable for BuiltWith<R, P> {}

impl<R: Projectable, P> Projectable for BuiltWith<R, P> {
    fn project(&self, columns: Vec<usize>) -> Self { Self::wrap(self.inner.project(columns)) }
}

impl<R, P> Relation for BuiltWith<R, P>
where
    R: BuildModeRelation,
    P: BuildModeProvider<R::BuildMode>,
{
    fn header(&self) -> &RelationHeader { self.inner.header() }

    /// An empty relation has nothing to build, so no mode applies.
    fn new(header: RelationHeader) -> Self { Self::wrap(R::new(header)) }

    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        Self::wrap(R::from_tuples_with_build_mode(
            header,
            P::build_mode(),
            tuples,
        ))
    }

    fn insert(&mut self, tuple: Vec<usize>) { self.inner.insert(tuple) }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) { self.inner.insert_all(tuples) }
}

impl<R: HeapSize, P> HeapSize for BuiltWith<R, P> {
    fn heap_size_bytes(&self) -> usize { self.inner.heap_size_bytes() }
}

impl<R: Cardinality, P> Cardinality for BuiltWith<R, P> {
    fn tuple_count(&self) -> usize { self.inner.tuple_count() }
}

impl<R: TrieIterable, P> TrieIterable for BuiltWith<R, P> {
    fn trie_iter(&self) -> impl TrieIterator + IntoIterator<Item = Vec<usize>> {
        self.inner.trie_iter()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::ds::{ColumnTrie, ColumnTrieBuildMode},
    };

    crate::define_build_mode_provider!(
        Incremental,
        ColumnTrieBuildMode,
        ColumnTrieBuildMode::Incremental
    );

    type IncrementalTrie = BuiltWith<ColumnTrie, Incremental>;

    fn tuples_of(relation: &impl TrieIterable) -> Vec<Vec<usize>> {
        relation.trie_iter().into_iter().collect()
    }

    #[test]
    fn from_tuples_builds_the_same_relation() {
        let tuples = vec![vec![3, 4], vec![1, 2], vec![1, 2]];
        let r = IncrementalTrie::from_tuples(2.into(), tuples.clone());
        let plain = ColumnTrie::from_tuples(2.into(), tuples);
        assert_eq!(r.header().arity(), 2);
        assert_eq!(tuples_of(&r), vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(Cardinality::tuple_count(&r), 2);
        assert_eq!(r.heap_size_bytes(), plain.heap_size_bytes());
    }

    #[test]
    fn projection_and_inserts_reach_the_inner_relation() {
        let r = IncrementalTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(tuples_of(&r.project(vec![1])), vec![vec![2], vec![4]]);
        let mut grown = IncrementalTrie::new(2.into());
        grown.insert(vec![3, 4]);
        grown.insert_all(vec![vec![1, 2]]);
        assert_eq!(tuples_of(&grown), vec![vec![1, 2], vec![3, 4]]);
    }
}
```

In `kermit-ds/src/lib.rs` add `mod built_with;` (beside `mod configured;`) and `built_with::{BuildModeProvider, BuiltWith},` to the `pub use` list.

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib built_with`
Expected: PASS (2 tests). Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --doc BuiltWith`. Expected: the `define_build_mode_provider` doctest passes.

- [ ] **Step 6: Run the sorted-trie suites under the incremental mode**

`kermit-ds/tests/trie_tests.rs` becomes:

```rust
use kermit_ds::{define_build_mode_provider, BuiltWith, ColumnTrie, ColumnTrieBuildMode, TreeTrie};
mod common;

relation_trie_test_suite!(TreeTrie);

relation_trie_test_suite!(ColumnTrie);

// The incremental BuildMode must satisfy the same contract: it builds the
// same trie as the default bulk build (issue #84).
define_build_mode_provider!(
    Incremental,
    ColumnTrieBuildMode,
    ColumnTrieBuildMode::Incremental
);

type ColumnTrieIncremental = BuiltWith<ColumnTrie, Incremental>;

relation_trie_test_suite!(ColumnTrieIncremental);
```

In `kermit-ds/tests/parquet_tests.rs`, add `define_build_mode_provider, BuiltWith, ColumnTrieBuildMode` to the `kermit_ds::{…}` import, and after `parquet_test_suite!(ColumnTrie);` add:

```rust
// …and under ColumnTrie's incremental BuildMode, which must load the same
// trie (issue #84).
define_build_mode_provider!(
    Incremental,
    ColumnTrieBuildMode,
    ColumnTrieBuildMode::Incremental
);

type ColumnTrieIncremental = BuiltWith<ColumnTrie, Incremental>;

parquet_test_suite!(ColumnTrieIncremental);
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --test trie_tests --test parquet_tests`
Expected: PASS; the `columntrieincremental` and `parquet_columntrieincremental` modules appear in the output.

- [ ] **Step 7: Format, lint, commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit-ds --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings nix develop $WT --command cargo doc -p kermit-ds --no-deps
git -C $WT add kermit-ds
git -C $WT commit -F - <<'EOF'
feat(kermit-ds): ColumnTrieBuildMode, BuildModeRelation and BuiltWith (#84)

ColumnTrie is the first BuildMode consumer. ColumnTrieBuildMode::Bulk
(the default) is the one-pass build; Incremental is the pre-#84 sort-
then-insert build, kept so its measurements can be reproduced. Both
build identical tries, which the array-level test now checks between
the two modes.

BuildModeRelation is the construction seam beside ConfigurableRelation;
BuiltWith<R, P> lifts a mode to a type for the macro test suites, as
Configured does for a Config. The sorted-trie and parquet suites run
under a ColumnTrieIncremental alias.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EQYT5rUt5xkPAh85YeHs8t
EOF
```

---

## Task 7 [P3]: `--ds-build` selects ColumnTrie's build mode

**Files:**
- Modify: `kermit/src/options.rs`, `kermit/src/execution.rs`, `kermit/src/bench/run.rs`, `kermit/src/bench/ds.rs`, `kermit/src/main.rs`
- Modify: `kermit/tests/cli_column_trie_build_mode.rs`

- [ ] **Step 1: Write the failing CLI tests**

Append to `kermit/tests/cli_column_trie_build_mode.rs`:

```rust
#[test]
fn cli_bench_ds_with_incremental_build_records_axis() {
    let (output, report) = bench_ds("column-trie", &["--ds-build", "incremental"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "incremental");
}

#[test]
fn cli_bench_ds_rejects_ds_build_off_column_trie() {
    for ds in ["tree-trie", "hash-trie"] {
        let (output, _) = bench_ds(ds, &["--ds-build", "bulk"]);
        assert!(!output.status.success(), "{ds} accepted --ds-build");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--ds-build"), "{ds}: {stderr}");
        assert!(stderr.contains("column-trie"), "{ds}: {stderr}");
    }
}

#[test]
fn cli_bench_run_sweep_carries_build_mode_only_to_column_trie_cells() {
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
        if axes["data_structure"] == "ColumnTrie" {
            assert_eq!(axes["ds_build_mode"], "incremental", "{axes}");
        } else {
            assert!(axes.get("ds_build_mode").is_none(), "{axes}");
        }
    }
}

#[test]
fn cli_bench_join_with_incremental_build_records_axis() {
    let (output, report) = bench_join("column-trie", "leapfrog-triejoin", &[
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
    assert_eq!(axes_of(&report)["ds_build_mode"], "incremental");
}
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_column_trie_build_mode`
Expected: the four new tests FAIL (clap: unexpected argument `--ds-build`); the phase-1 tests still pass.

- [ ] **Step 2: The CLI group and its validator**

In `kermit/src/options.rs`, change the `kermit_ds` import to `kermit_ds::{ColumnTrieBuildMode, HashTrieConfig, LoadFactor, PruningPolicy}`, and after `validate_config_choices` add:

```rust
/// BuildMode-axis CLI choice, flattened beside [`LayoutChoices`] and
/// [`ConfigChoices`] into `bench ds`, `bench run` and `bench join`. Every
/// build mode builds the same structure, so the flag changes build time
/// only. `kermit join` takes no `--ds-build`, for the same reason it takes
/// no `--ds-config`: it cannot change a query's answers.
#[derive(Args, Clone, Debug, Default)]
pub(crate) struct BuildChoices {
    /// How `ColumnTrie` is built from its tuples (default: `bulk`; `incremental`
    /// is the build before issue #84). Only valid with `--indexstructure
    /// column-trie` (or `all`).
    #[arg(long = "ds-build", value_name = "MODE", value_enum)]
    column_trie_build: Option<ColumnTrieBuildMode>,
}

impl BuildChoices {
    /// The mode to build ColumnTrie relations with, applying the default
    /// when none was supplied.
    pub(crate) fn column_trie_build_resolved(&self) -> ColumnTrieBuildMode {
        self.column_trie_build.unwrap_or_default()
    }

    /// Whether the user explicitly passed `--ds-build`.
    pub(crate) fn column_trie_build_explicit(&self) -> bool { self.column_trie_build.is_some() }
}

/// Rejects `--ds-build` on index structures that have no BuildMode axis, so
/// a report can never carry a `ds_build_mode` the build ignored. Same
/// discipline as [`validate_config_choices`].
pub(crate) fn validate_build_choices(
    indexstructure: IndexStructureSelector, build: &BuildChoices,
) -> anyhow::Result<()> {
    if build.column_trie_build_explicit()
        && !matches!(
            indexstructure,
            IndexStructureSelector::ColumnTrie | IndexStructureSelector::All
        )
    {
        anyhow::bail!(
            "--ds-build is only valid with --indexstructure column-trie (or all); got \
             --indexstructure {indexstructure:?}"
        );
    }
    Ok(())
}
```

Extend `DsChoices` with a fourth field and `resolve` with a fourth argument:

```rust
    /// `--ds-build`; reaches the column-trie cell only.
    pub build: ColumnTrieBuildMode,
```

```rust
    pub(crate) fn resolve(
        indexstructure: IndexStructureSelector, layout: &LayoutChoices, config: &ConfigChoices,
        build: &BuildChoices,
    ) -> anyhow::Result<Self> {
        validate_layout_choices(indexstructure, layout)?;
        validate_config_choices(indexstructure, config)?;
        validate_build_choices(indexstructure, build)?;
        Ok(Self {
            hasher: layout.hash_trie_hasher_resolved(),
            pruning: layout.hash_trie_pruning_resolved(),
            config: config.hash_trie_config_resolved()?,
            build: build.column_trie_build_resolved(),
        })
    }
```

Update the three `DsChoices::resolve(…)` calls in the Task 5 tests to pass `&BuildChoices::default()` as the fourth argument, and add:

```rust
    #[test]
    fn validate_build_choices_accepts_column_trie_or_all_only() {
        let build = BuildChoices {
            column_trie_build: Some(ColumnTrieBuildMode::Incremental),
        };
        assert!(validate_build_choices(IndexStructureSelector::ColumnTrie, &build).is_ok());
        assert!(validate_build_choices(IndexStructureSelector::All, &build).is_ok());
        for sel in [
            IndexStructureSelector::TreeTrie,
            IndexStructureSelector::HashTrie,
        ] {
            let msg = validate_build_choices(sel, &build).unwrap_err().to_string();
            assert!(msg.contains("--ds-build"), "{msg}");
            assert!(msg.contains("column-trie"), "{msg}");
            assert!(validate_build_choices(sel, &BuildChoices::default()).is_ok());
        }
    }

    #[test]
    fn ds_choices_resolve_carries_the_build_mode() {
        let build = BuildChoices {
            column_trie_build: Some(ColumnTrieBuildMode::Incremental),
        };
        let choices = DsChoices::resolve(
            IndexStructureSelector::ColumnTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build,
        )
        .unwrap();
        assert_eq!(choices.build, ColumnTrieBuildMode::Incremental);
        assert_eq!(DsChoices::default().build, ColumnTrieBuildMode::Bulk);
        assert!(DsChoices::resolve(
            IndexStructureSelector::TreeTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build,
        )
        .is_err());
    }
```

- [ ] **Step 3: The cell and the family carry the mode**

In `kermit/src/execution.rs`:

Add `ColumnTrieBuildMode` to the `kermit_ds::{…}` import and `BuildMode` to the `kermit_iters::{…}` import.

Replace `enum SortedTrie` and its `index_structure`:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SortedTrie {
    /// Pointer-based trie (`-i tree-trie`).
    TreeTrie,
    /// Column-oriented trie (`-i column-trie`), built by the `--ds-build`
    /// mode `build`.
    ColumnTrie {
        /// The `--ds-build` mode every relation is built with.
        build: ColumnTrieBuildMode,
    },
}

impl SortedTrie {
    /// The `IndexStructure` this sorted trie corresponds to.
    pub fn index_structure(self) -> IndexStructure {
        match self {
            | Self::TreeTrie => IndexStructure::TreeTrie,
            | Self::ColumnTrie {
                ..
            } => IndexStructure::ColumnTrie,
        }
    }
}
```

Replace `trait SortedTrieRelation` and both impls (this supersedes Task 2's no-argument `build_mode_axes`):

```rust
/// Ties a sorted-family relation type to its [`SortedTrie`] label and its
/// build modes, so the runner's report axes are derived from the type it
/// monomorphised over and the mode it built with, not from a separately
/// threaded value that could disagree with them.
pub trait SortedTrieRelation:
    Relation + RelationFileExt + TrieIterable + Cardinality + HeapSize
{
    /// How this structure can be built from its tuples: `()` for a
    /// structure with a single build process.
    type BuildMode: Copy + Default;

    /// The CLI-visible identity of this relation type built by `build`.
    fn kind(build: Self::BuildMode) -> SortedTrie;

    /// Builds one relation from `tuples` by `build`.
    fn build_with(header: RelationHeader, build: Self::BuildMode, tuples: Vec<Vec<usize>>) -> Self;

    /// The `ds_build_mode` axis of relations built by `build`. The build
    /// leaves no trace in the built structure, so this — not the relation —
    /// reports it. Empty for a structure with a single build process.
    fn build_mode_axes(build: Self::BuildMode) -> BTreeMap<String, serde_json::Value>;
}

impl SortedTrieRelation for TreeTrie {
    type BuildMode = ();

    fn kind(_: ()) -> SortedTrie { SortedTrie::TreeTrie }

    fn build_with(header: RelationHeader, _: (), tuples: Vec<Vec<usize>>) -> Self {
        TreeTrie::from_tuples(header, tuples)
    }

    fn build_mode_axes(_: ()) -> BTreeMap<String, serde_json::Value> { BTreeMap::new() }
}

impl SortedTrieRelation for ColumnTrie {
    type BuildMode = ColumnTrieBuildMode;

    fn kind(build: ColumnTrieBuildMode) -> SortedTrie {
        SortedTrie::ColumnTrie {
            build,
        }
    }

    fn build_with(
        header: RelationHeader, build: ColumnTrieBuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self {
        ColumnTrie::from_tuples_with_build_mode(header, build, tuples)
    }

    /// Every ColumnTrie report says which build made it, so kermit-lab can
    /// read a ColumnTrie row *without* the axis as the pre-#84 incremental
    /// build.
    fn build_mode_axes(build: ColumnTrieBuildMode) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::from([(
            "ds_build_mode".to_string(),
            serde_json::Value::from(build.axis_value()),
        )])
    }
}
```

Add `BuildModeRelation` to the `kermit_ds::{…}` import (for `from_tuples_with_build_mode`).

In `for_pair` and `for_structure`, destructure `build` too (`let DsChoices { hasher, pruning, config, build } = choices;`) and make the ColumnTrie arms `Some(Execution::TrieLftj(SortedTrie::ColumnTrie { build }))` and `Execution::TrieLftj(SortedTrie::ColumnTrie { build })`. Update the `for_pair` doc's last sentence to: "`choices` reach the cells that have each axis: the hash-trie cell's hasher, pruning and config, and the column-trie cell's build mode."

Make `RelationFamily::build_relation` **required** — replace its default body with a declaration, and extend its doc:

```rust
    /// Builds one relation from a header and its tuples, honouring the
    /// family's configuration and build mode.
    ///
    /// Every relation the family builds *from tuples* routes through this
    /// one site — the `insertion` metric, [`load`](Self::load),
    /// [`load_with_tuples`](Self::load_with_tuples), and both families'
    /// [`build_from_tuples`] — so such a measurement can never build a
    /// relation the report's `ds_config_*` / `ds_build_mode` axes fail to
    /// describe. Required, with no default, so no family can silently fall
    /// back to `Relation::from_tuples` and build with a configuration or
    /// mode its report does not name.
    ///
    /// [`build_from_tuples`]: ExecutionFamily::build_from_tuples
    fn build_relation(&self, header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self::Rel;
```

Replace `struct SortedTrieFamily` and its two impls:

```rust
/// The sorted-family structure `R` on its own: what `bench ds -i tree-trie`
/// / `-i column-trie` measures. Carries the `--ds-build` mode every
/// relation is built with (`()` for a structure with a single build), but
/// no optimiser or engine, so it cannot join — the type says what
/// `bench ds` does.
pub struct SortedTrieFamily<R: SortedTrieRelation> {
    build: R::BuildMode,
}

impl<R: SortedTrieRelation> SortedTrieFamily<R> {
    /// The family building every relation by `build`.
    pub fn new(build: R::BuildMode) -> Self {
        Self {
            build,
        }
    }
}

impl<R: SortedTrieRelation> Default for SortedTrieFamily<R> {
    fn default() -> Self { Self::new(R::BuildMode::default()) }
}
```

In `impl RelationFamily for SortedTrieFamily<R>`: replace `execution` with `fn execution(&self) -> Execution { Execution::TrieLftj(R::kind(self.build)) }`, replace `build_mode_axes` with `fn build_mode_axes(&self) -> BTreeMap<String, serde_json::Value> { R::build_mode_axes(self.build) }`, and add:

```rust
    fn build_relation(&self, header: RelationHeader, tuples: Vec<Vec<usize>>) -> R {
        R::build_with(header, self.build, tuples)
    }
```

Give `TrieLftj` the same bound and a mode:

```rust
pub struct TrieLftj<R: SortedTrieRelation> {
    structure: SortedTrieFamily<R>,
    optimiser: Box<dyn QueryOptimiser>,
}

impl<R: SortedTrieRelation> TrieLftj<R> {
    /// Creates the family building every relation by `build`, planned by
    /// `optimiser`.
    pub fn new(build: R::BuildMode, optimiser: Optimiser) -> Self {
        Self {
            structure: SortedTrieFamily::new(build),
            optimiser: optimiser.instantiate(),
        }
    }
}
```

and add to `impl RelationFamily for TrieLftj<R>`:

```rust
    fn build_relation(&self, header: RelationHeader, tuples: Vec<Vec<usize>>) -> R {
        self.structure.build_relation(header, tuples)
    }
```

Remove the now-unused `PhantomData` import only if the compiler reports it unused (`HashTrieFamily` still uses it).

- [ ] **Step 4: Update the `execution.rs` tests**

- `SortedTrieFamily::<TreeTrie>::new()` → `SortedTrieFamily::<TreeTrie>::default()` and `SortedTrieFamily::<ColumnTrie>::new()` → `SortedTrieFamily::<ColumnTrie>::default()` (in `structure_markers_agree_with_join_families`, `load_with_tuples_returns_file_order`, `scan_agrees_with_tuple_count_in_every_family`).
- `TrieLftj::<TreeTrie>::new(Optimiser::Lexicographic)` → `TrieLftj::<TreeTrie>::new((), Optimiser::Lexicographic)`; `TrieLftj::<ColumnTrie>::new(Optimiser::Lexicographic)` → `TrieLftj::<ColumnTrie>::new(ColumnTrieBuildMode::default(), Optimiser::Lexicographic)`.
- In `families_report_their_own_execution`, replace the ColumnTrie block with:

```rust
        let column =
            TrieLftj::<ColumnTrie>::new(ColumnTrieBuildMode::Incremental, Optimiser::Lexicographic);
        assert_eq!(
            column.execution(),
            Execution::TrieLftj(SortedTrie::ColumnTrie {
                build: ColumnTrieBuildMode::Incremental
            })
        );
```

- In `for_structure_agrees_with_for_pair`, wrap the body in `for build in [ColumnTrieBuildMode::Incremental, ColumnTrieBuildMode::Bulk]` and build `DsChoices { hasher, pruning, config, build }`.
- Replace Task 2's `only_column_trie_families_report_a_build_mode` with:

```rust
    fn build_mode_axis(mode: &str) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::from([(
            "ds_build_mode".to_string(),
            serde_json::Value::from(mode),
        )])
    }

    /// Every ColumnTrie family reports the mode it builds with; the other
    /// structures have a single build and carry no such axis (issue #84).
    #[test]
    fn only_column_trie_families_report_their_build_mode() {
        assert_eq!(
            SortedTrieFamily::<ColumnTrie>::default().build_mode_axes(),
            build_mode_axis("bulk")
        );
        assert_eq!(
            SortedTrieFamily::<ColumnTrie>::new(ColumnTrieBuildMode::Incremental)
                .build_mode_axes(),
            build_mode_axis("incremental")
        );
        assert_eq!(
            TrieLftj::<ColumnTrie>::new(ColumnTrieBuildMode::Incremental, Optimiser::Lexicographic)
                .build_mode_axes(),
            build_mode_axis("incremental")
        );
        assert!(SortedTrieFamily::<TreeTrie>::default()
            .build_mode_axes()
            .is_empty());
        assert!(TrieLftj::<TreeTrie>::new((), Optimiser::Lexicographic)
            .build_mode_axes()
            .is_empty());
        assert!(HashTrieFamily::<SipHashStrategy, NoPruning>::default()
            .build_mode_axes()
            .is_empty());
    }

    /// The `--ds-build` mode reaches the column-trie cell of a sweep and no
    /// other.
    #[test]
    fn sweep_attaches_the_build_mode_to_the_column_trie_cell_only() {
        let choices = DsChoices {
            build: ColumnTrieBuildMode::Incremental,
            ..DsChoices::default()
        };
        let sweep = Sweep::expand(&all_structures(), &all_algorithms(), choices);
        assert!(sweep.cells.contains(&Execution::TrieLftj(SortedTrie::ColumnTrie {
            build: ColumnTrieBuildMode::Incremental
        })));
        assert!(sweep
            .cells
            .contains(&Execution::TrieLftj(SortedTrie::TreeTrie)));
    }
```

- [ ] **Step 5: Thread the mode through the dispatchers and `main.rs`**

`kermit/src/bench/run.rs`, `dispatch_run_bench`:

```rust
        | Execution::TrieLftj(SortedTrie::TreeTrie) => run_benchmark(
            &TrieLftj::<kermit_ds::TreeTrie>::new((), optimiser),
            workload,
            settings,
        ),
        | Execution::TrieLftj(SortedTrie::ColumnTrie {
            build,
        }) => run_benchmark(
            &TrieLftj::<kermit_ds::ColumnTrie>::new(build, optimiser),
            workload,
            settings,
        ),
```

`kermit/src/bench/ds.rs`, `dispatch_ds_bench`:

```rust
        | Execution::TrieLftj(SortedTrie::TreeTrie) => run_ds_bench(
            &SortedTrieFamily::<kermit_ds::TreeTrie>::default(),
            relation,
            metrics,
            queries_per_build,
            group_name,
            bench_args,
        ),
        | Execution::TrieLftj(SortedTrie::ColumnTrie {
            build,
        }) => run_ds_bench(
            &SortedTrieFamily::<kermit_ds::ColumnTrie>::new(build),
            relation,
            metrics,
            queries_per_build,
            group_name,
            bench_args,
        ),
```

and extend its doc comment's last sentence: "…and the `--ds-config` values and the `--ds-build` mode ride along on the family, so the relation this measures is the one the report's `ds_*` axes describe."

`kermit/src/main.rs`:
- imports: `kermit_ds::{ColumnTrieBuildMode, HashTrieConfig, IndexStructure}` and `options::{with_hash_trie_layout, BuildChoices, ConfigChoices, DsChoices, LayoutChoices}`;
- add `#[command(flatten)] build: BuildChoices,` after `config: ConfigChoices,` in `BenchSubcommand::Join`, `Ds` and `Run`, and `build` to each destructuring in `main()` and each handler call;
- handler signatures gain `build: BuildChoices` as their last parameter (`run_bench_join`, `run_ds_bench_command`, `run_bench_run_command`), and each `DsChoices::resolve(…, &config)` becomes `DsChoices::resolve(…, &config, &build)`;
- `load_query_runner(args: &QueryArgs, config: HashTrieConfig, build: ColumnTrieBuildMode)`: pass `build` into its `DsChoices { …, config, build }`, and make its two sorted arms:

```rust
        | Execution::TrieLftj(SortedTrie::TreeTrie) => build_join_runner(
            TrieLftj::<kermit_ds::TreeTrie>::new((), optimiser),
            &args.relations,
        ),
        | Execution::TrieLftj(SortedTrie::ColumnTrie {
            build,
        }) => build_join_runner(
            TrieLftj::<kermit_ds::ColumnTrie>::new(build, optimiser),
            &args.relations,
        ),
```

- replace its doc paragraph "`kermit join` deliberately carries no `--ds-config` …" with: "`kermit join` deliberately carries no `--ds-config` or `--ds-build` and passes the defaults: neither the load factor nor the build mode can change a query's answers. `bench join --output` passes its resolved values so the CSV comes from the same build the measurements use.";
- `run_join` calls `load_query_runner(&query_args, HashTrieConfig::default(), ColumnTrieBuildMode::default())`; `run_bench_join`'s `--output` branch calls `load_query_runner(&query_args, choices.config, choices.build)`.

- [ ] **Step 6: Run the tests**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit`
Expected: all pass, including the eight `cli_column_trie_build_mode` tests and every existing `cli_*` test.

- [ ] **Step 7: Format, lint, commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit --all-targets -- -D warnings
git -C $WT add kermit
git -C $WT commit -F - <<'EOF'
feat(kermit): --ds-build selects ColumnTrie's build mode (#84)

bench ds / run / join take --ds-build bulk|incremental, rejected on a
structure without a BuildMode axis (accepted under -i all, where it
reaches only the column-trie cell). The mode rides on the cell,
SortedTrie::ColumnTrie { build }, and on SortedTrieFamily, which builds
through it and reports it as ds_build_mode. RelationFamily::build_relation
is now required, so no family can fall back to a build its report does
not name.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EQYT5rUt5xkPAh85YeHs8t
EOF
```

- [ ] **Step 8: Mutation checks**

Mutant A — the cell ignores the mode: in `Execution::for_pair`, change the ColumnTrie arm to `SortedTrie::ColumnTrie { build: ColumnTrieBuildMode::Bulk }`. Run `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_column_trie_build_mode`. Expected: FAIL (`cli_bench_run_sweep_carries_build_mode_only_to_column_trie_cells`, `cli_bench_join_with_incremental_build_records_axis`). Restore; re-run; PASS.

Mutant B — the validator lets tree-trie through: change `IndexStructureSelector::ColumnTrie | IndexStructureSelector::All` in `validate_build_choices` to `IndexStructureSelector::ColumnTrie | IndexStructureSelector::All | IndexStructureSelector::TreeTrie`. Run the same command. Expected: FAIL (`cli_bench_ds_rejects_ds_build_off_column_trie`). Restore; re-run; PASS. `git status` clean.

Note: whether `TrieLftj::build_relation` delegates to the structure cannot be observed through output, because both modes build identical tries. The required-method change prevents the silent fallback at compile time; Task 11's `bench run` timing check confirms the mode reaches the build end to end.

---

## Task 8 [P3]: `define_multiway_join_test_suite_for_build_mode!`

**Files:**
- Modify: `kermit/tests/common/macros.rs`
- Modify: `kermit/tests/join_tests.rs`

- [ ] **Step 1: Write the failing invocations**

In `kermit/tests/join_tests.rs`, add `define_build_mode_provider, ColumnTrieBuildMode` to the `kermit_ds::{…}` import and append:

```rust
// ── BuildMode axis: ColumnTrie's build ──────────────────────────────────
// The plain ColumnTrie invocations above build with the default (`bulk`);
// these run the alternate. Every mode must build the same trie (issue #84).
define_build_mode_provider!(
    Incremental,
    ColumnTrieBuildMode,
    ColumnTrieBuildMode::Incremental
);

define_multiway_join_test_suite_for_build_mode!(
    ColumnTrie,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    Incremental
);
define_multiway_join_test_suite_for_build_mode!(
    ColumnTrie,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    Incremental
);
```

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests`
Expected: compile error, `cannot find macro define_multiway_join_test_suite_for_build_mode`.

- [ ] **Step 2: Add the macro**

In `kermit/tests/common/macros.rs`, after `define_multiway_join_test_suite_with_config!`, add:

```rust
/// The BuildMode-axis counterpart of [`define_multiway_join_test_suite!`]
/// prescribed by `docs/specs/optimization-standard.md`.
///
/// Declares `type <Relation><Provider> = BuiltWith<Relation, Provider>;`
/// inside a uniquely named module and runs the standard join patterns
/// against it by delegating to [`define_multiway_join_test_suite!`], so a
/// pattern added there runs here too. `Provider` is a marker declared with
/// `kermit_ds::define_build_mode_provider!`.
///
/// ```ignore
/// define_build_mode_provider!(Incremental, ColumnTrieBuildMode, ColumnTrieBuildMode::Incremental);
/// define_multiway_join_test_suite_for_build_mode!(ColumnTrie, LeapfrogTriejoin, LexicographicOptimiser, Incremental);
/// // → tests named e.g. `triangle_columntrieincremental_leapfrogtriejoin_lexicographicoptimiser`
/// ```
#[macro_export]
macro_rules! define_multiway_join_test_suite_for_build_mode {
    (
        $(
            $relation_type:ident,
            $join_algorithm:ident,
            $optimiser:ident,
            $provider:ident
        ),+
    ) => {
        $(
            paste::paste! {
                // One module per invocation, as in the Config variant, so
                // sibling invocations' aliases cannot collide.
                mod [<build_mode_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower _ $provider:lower>] {
                    use super::*;

                    type [<$relation_type $provider>] =
                        kermit_ds::BuiltWith<$relation_type, $provider>;

                    $crate::define_multiway_join_test_suite!(
                        [<$relation_type $provider>], $join_algorithm, $optimiser
                    );
                }
            }
        )+
    };
}
```

- [ ] **Step 3: Run the tests**

Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests columntrieincremental`
Expected: PASS, 28 tests (14 patterns × 2 optimisers).

- [ ] **Step 4: Format, lint, commit**

```bash
nix develop $WT --command cargo fmt --all
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit --all-targets -- -D warnings
git -C $WT add kermit/tests/common/macros.rs kermit/tests/join_tests.rs
git -C $WT commit -F - <<'EOF'
test(kermit): define_multiway_join_test_suite_for_build_mode! (#84)

The BuildMode counterpart of the Config suite macro, prescribed by the
optimization standard and landing with its first consumer. It wraps the
relation in BuiltWith and delegates to define_multiway_join_test_suite!,
so patterns added to the base suite reach it. ColumnTrie's incremental
mode runs every pattern under both optimisers.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EQYT5rUt5xkPAh85YeHs8t
EOF
```

---

## Task 9 [P4]: kermit-lab back-fills `ds_build_mode` on pre-#84 ColumnTrie rows

**Files:**
- Modify: `python/kermit-lab/kermit_lab/defaults.py`
- Modify: `python/kermit-lab/tests/test_defaults.py`
- Modify: `python/kermit-lab/tests/test_contract.py`

- [ ] **Step 1: Write the failing tests**

In `tests/test_defaults.py`, change the import to `from kermit_lab.defaults import AXIS_DEFAULTS, SCOPED_AXIS_DEFAULTS, apply_axis_defaults` and append:

```python
def test_build_mode_backfills_column_trie_rows_only() -> None:
    df = pd.DataFrame({
        "data_structure": ["ColumnTrie", "ColumnTrie", "TreeTrie", "HashTrie"],
        "ds_build_mode": [pd.NA, "bulk", pd.NA, pd.NA],
    })
    out = apply_axis_defaults(df)
    assert out["ds_build_mode"].iloc[0] == "incremental"
    assert out["ds_build_mode"].iloc[1] == "bulk"
    assert out["ds_build_mode"].iloc[2:].isna().all()
    assert SCOPED_AXIS_DEFAULTS[("ds_build_mode", "ColumnTrie")] == "incremental"
    assert "ds_build_mode" not in AXIS_DEFAULTS


def test_scoped_default_needs_the_data_structure_column() -> None:
    df = pd.DataFrame({"ds_build_mode": [pd.NA]})
    out = apply_axis_defaults(df)
    assert out["ds_build_mode"].isna().all()  # no crash, untouched
```

In `tests/test_contract.py`, append:

```python
def test_bench_ds_column_trie_reports_its_build_mode(tmp_path: Path) -> None:
    report = tmp_path / "ds.json"
    _run(
        tmp_path, report,
        "ds", "--relation", str(FIXTURES / "edge.csv"), "-i", "column-trie", "-m", "space",
        "--ds-build", "incremental",
    )
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion")
    assert len(df) == 1
    assert df.iloc[0]["ds_build_mode"] == "incremental"
```

On a cold environment first run `uv --directory $WT/python/kermit-lab sync --group test`.

Run: `systemd-run --user --scope -p MemoryMax=3G -p MemorySwapMax=0 uv --directory $WT/python/kermit-lab run pytest tests/test_defaults.py`
Expected: FAIL, `ImportError: cannot import name 'SCOPED_AXIS_DEFAULTS'`.

- [ ] **Step 2: Implement the scoped registry**

In `kermit_lab/defaults.py`, after `AXIS_DEFAULTS`, add:

```python
# (axis column, data_structure) -> value to substitute for NaN on rows of
# that structure only. An axis that exists on one structure needs this: a
# structure-blind fill would stamp the value on rows that never had the axis.
SCOPED_AXIS_DEFAULTS: dict[tuple[str, str], object] = {
    # ColumnTrie's build before issue #84 inserted tuple by tuple. Every
    # ColumnTrie report since carries the axis ("bulk" by default).
    ("ds_build_mode", "ColumnTrie"): "incremental",
}
```

and replace the body of `apply_axis_defaults` (keep its docstring, adding one sentence: "Scoped defaults fill only the rows of their data structure.") with:

```python
    out = df.copy()
    for col, default in AXIS_DEFAULTS.items():
        if col in out.columns:
            out[col] = out[col].fillna(default)
    if "data_structure" in out.columns:
        for (col, data_structure), default in SCOPED_AXIS_DEFAULTS.items():
            if col in out.columns:
                rows = out["data_structure"] == data_structure
                out.loc[rows, col] = out.loc[rows, col].fillna(default)
    return out
```

- [ ] **Step 3: Run the tests**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit
systemd-run --user --scope -p MemoryMax=3G -p MemorySwapMax=0 env KERMIT_BIN=$WT/target/debug/kermit uv --directory $WT/python/kermit-lab run pytest
```

Expected: all pass, including the two new default tests and the three contract tests (none skipped, since `KERMIT_BIN` is set).

- [ ] **Step 4: Write the failing ablation-guard tests**

Required by the supervisor's checkpoint-1 review. `render_all` defaults to `phase="iteration"` and draws `ablation-<axis>` for every axis with two values. Once old ColumnTrie rows are back-filled `incremental` beside new `bulk` ones, it would draw an iteration-time "build-mode ablation" — but both modes build the same trie, so any difference there is noise or binary drift. A BuildMode axis can only explain phases that time the build (`insertion`, `end_to_end`).

Append to `tests/conftest.py`:

```python
@pytest.fixture
def fixture_build_mode_tree(tmp_path: Path) -> dict:
    """ColumnTrie reports from before and after issue #84, for the
    build-mode ablation guard.

    The old report carries no ``ds_build_mode`` (back-filled to
    ``incremental`` on load); the new one carries ``"bulk"``. Each times
    insertion and iteration (plus space), so the build-mode axis has two
    values and applies to one time phase but not the other.
    """
    criterion_root = tmp_path / "target" / "criterion"
    reports_dir = tmp_path / "reports"
    criterion_root.mkdir(parents=True)
    reports_dir.mkdir()

    paths: list[Path] = []
    for tag, build_mode, insertion_point in (("old", None, 9000.0), ("new", "bulk", 300.0)):
        groups: list[tuple[str, str, str]] = []
        # Same trie under both builds, so the same traversal time.
        for phase, point in (("insertion", insertion_point), ("iteration", 100.0)):
            function = f"ColumnTrie/{tag}/{phase}"
            samples = [(i + 1, point * (i + 1)) for i in range(10)]
            _write_function_dir(
                criterion_root, _FunctionSpec("run", function, "time", point, samples)
            )
            groups.append(("run", function, "time"))
        space_function = f"ColumnTrie/{tag}/space"
        space_samples = [(i + 1, 6400.0 * (i + 1)) for i in range(10)]
        _write_function_dir(
            criterion_root,
            _FunctionSpec("run", space_function, "space", 6400.0, space_samples),
        )
        groups.append(("run", space_function, "space"))
        axes = {
            "benchmark": "triangle",
            "query": "triangle",
            "data_structure": "ColumnTrie",
            "algorithm": "LeapfrogTriejoin",
            "tuples": 100,
        }
        if build_mode is not None:
            axes["ds_build_mode"] = build_mode
        paths.append(
            _write_report(
                reports_dir, f"run-ColumnTrie-{tag}", kind="run", axes=axes,
                metadata=[], groups=groups,
            )
        )
    return {
        "criterion_root": criterion_root,
        "reports_dir": reports_dir,
        "paths": sorted(paths),
    }
```

Append to `tests/test_render_all.py` (add `import pytest`, `import kermit_lab as kl`, `from kermit_lab import presets` and `from kermit_lab.plots_errors import InsufficientAxesError` to its imports):

```python
def test_build_mode_ablation_is_drawn_only_for_build_phases(
    fixture_build_mode_tree, tmp_path: Path
) -> None:
    """Old ColumnTrie rows back-fill to ``incremental`` beside new ``bulk``
    ones, but both builds produce the same trie: an iteration-time
    "build-mode ablation" would chart drift, not the build mode."""
    reports = load_reports(fixture_build_mode_tree["paths"])
    for phase, drawn in (("iteration", False), ("insertion", True)):
        out = tmp_path / phase
        out.mkdir()
        render_all(reports, out, fixture_build_mode_tree["criterion_root"], "pdf", phase=phase)
        names = {p.name for p in out.iterdir()}
        assert ("ablation-ds_build_mode.pdf" in names) is drawn, (phase, names)


def test_ablation_preset_refuses_build_mode_outside_build_phases(
    fixture_build_mode_tree,
) -> None:
    df = kl.load(
        fixture_build_mode_tree["paths"],
        criterion_root=fixture_build_mode_tree["criterion_root"],
    )
    assert set(df["ds_build_mode"].dropna()) == {"incremental", "bulk"}
    with pytest.raises(InsufficientAxesError, match="built"):
        presets.ablation(df, axis="ds_build_mode", phase="iteration")
```

Run: `systemd-run --user --scope -p MemoryMax=3G -p MemorySwapMax=0 uv --directory $WT/python/kermit-lab run pytest tests/test_render_all.py`
Expected: both new tests FAIL (the iteration-phase render draws `ablation-ds_build_mode.pdf`; the preset raises nothing). If a non-ablation shape raises something other than `InsufficientAxesError` on this fixture, align the fixture with `fixture_opt_tree`'s shape rather than weakening `render_all`.

- [ ] **Step 5: Guard the ablation preset**

In `kermit_lab/presets.py`, import `from .plots_errors import InsufficientAxesError` (if not already imported), and above `def ablation` add:

```python
# A BuildMode changes how a structure is built, never the structure, so its
# axis can only explain the phases that time a build. On any other phase two
# build modes measure the same structure, and a difference between them is
# noise or binary drift, not a build-mode effect.
_BUILD_ONLY_AXES = frozenset({"ds_build_mode"})
_BUILD_PHASES = frozenset({"insertion", "end_to_end"})
```

and make `ablation` begin:

```python
def ablation(
    df: pd.DataFrame, *, axis: str, phase: str = "iteration", out: Optional[Path] = None
) -> Figure:
    """Ablation: time vs an optimization axis, coloured by DS, faceted by query when >1.

    Raises :class:`InsufficientAxesError` for a build-mode axis on a phase
    that does not time the build.
    """
    if axis in _BUILD_ONLY_AXES and phase not in _BUILD_PHASES:
        raise InsufficientAxesError(
            f"{axis} changes only how a structure is built, so it cannot affect "
            f"phase {phase!r}; plot it on 'insertion' or 'end_to_end'"
        )
```

(the rest of the body is unchanged). `render_all`'s `_try` already demotes `InsufficientAxesError` to an info log (`skipped ablation-ds_build_mode: …`), so it skips rather than raises; the CLI `ablation` subcommand reports it the same way.

Run the full suite again (Step 3's two commands). Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git -C $WT add python/kermit-lab
git -C $WT commit -F - <<'EOF'
feat(kermit-lab): back-fill ds_build_mode for pre-#84 ColumnTrie rows (#84)

A ColumnTrie report without ds_build_mode predates the axis, so it was
built incrementally; every ColumnTrie report since carries the axis. The
new SCOPED_AXIS_DEFAULTS fills it on ColumnTrie rows only, since the
other structures have no build-mode axis. The contract test checks the
real binary emits the key.

The ablation preset refuses ds_build_mode on any phase but insertion and
end_to_end: both modes build the same structure, so on iteration (the
render-all default) old and new rows differ only by drift. render-all
logs the skip.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EQYT5rUt5xkPAh85YeHs8t
EOF
```

---

## Task 10 [P4]: Document the first BuildMode consumer

**Files:**
- Modify: `docs/data-structures/column-trie.md`, `docs/specs/optimization-standard.md`, `docs/specs/bench-report-schema.md`, `CLAUDE.md`, `ARCHITECTURE.md`, `BENCHMARKING.md`, `USAGE.md`

Read each passage before replacing it; the old text below is quoted from 62f722e.

- [ ] **Step 1: `column-trie.md` — Optimizations section**

Insert before `## When to prefer this structure`:

```markdown
## Optimizations

Under [`optimization-standard.md`](../specs/optimization-standard.md), `ColumnTrie` has one axis, a **BuildMode**: how the trie is built from a known set of tuples. Every mode builds the identical trie — the same arrays and the same capacities — so the mode changes the `insertion` and `end_to_end` timings and nothing else.

| Mode | `--ds-build` | Build | Time |
|---|---|---|---|
| `Bulk` (default) | `bulk` | sort, then one pass (see Construction) | O(n · a · log n) |
| `Incremental` | `incremental` | sort, then one `insert` per tuple — the build before issue #84 | O(n · a · b) |

- **Axis:** `ds_build_mode`, on every ColumnTrie report. The bench family that ran the build emits it (`RelationFamily::build_mode_axes`), because the trie cannot tell how it was built. kermit-lab reads a ColumnTrie row without the axis as `incremental`, the only build before the axis existed.
- **API:** `ColumnTrieBuildMode`, through `BuildModeRelation::from_tuples_with_build_mode`; `Relation::from_tuples` uses the default, `Bulk`.
- **Tests:** `bulk_and_incremental_builds_are_identical` (array-level, capacities included), the `ColumnTrieIncremental` alias in `kermit-ds/tests/{trie,parquet}_tests.rs`, and `define_multiway_join_test_suite_for_build_mode!` in `kermit/tests/join_tests.rs`.
- **When `incremental` is useful:** reproducing pre-#84 `insertion` numbers, and measuring how much of ColumnTrie's build cost belonged to the routine rather than the layout.
```

Also update the pinning-test name in the Canonical-layout invariant bullet from `bulk_build_matches_one_by_one_inserts` to `bulk_and_incremental_builds_are_identical`.

- [ ] **Step 2: `optimization-standard.md`**

1. BuildMode section: replace the paragraph starting `> **Concrete example (hypothetical).** Parallel build.` with:

```markdown
> **Concrete example (implemented).** ColumnTrie's build. `ColumnTrie::from_tuples_with_build_mode(header, ColumnTrieBuildMode::Incremental, tuples)` sorts the tuples and inserts them one at a time (the build before issue #84, O(n · a · b)); `ColumnTrieBuildMode::Bulk`, the default, builds every layer in one pass (O(n · a)). Both build identical arrays with identical capacities, which an array-level test pins.
```

   In its table, replace the `Examples (potential)` cell with `ColumnTrie bulk / incremental ✓, parallel build, radix partitioning` and the `Test obligation` cell with `Each non-default mode via `define_multiway_join_test_suite_for_build_mode!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)), plus an array-level test that every mode builds the identical structure, capacities included`.

2. CLI section: replace

````markdown
Hypothetically (when BuildMode gets a consumer):

```bash
kermit bench run triangle -i hash-trie -a hash-triejoin \
    --ds-layout-hasher fxhash \
    --ds-layout-pruning on \
    --ds-config load-factor=0.5 \
    --ds-build parallel:8
```
````

   with

````markdown
The BuildMode category has one consumer, ColumnTrie:

```bash
# The pre-#84 build, to reproduce its insertion numbers
kermit bench run triangle -i column-trie -a leapfrog-triejoin --ds-build incremental
```
````

3. Axis table: replace `` `ds_build_mode: "parallel:8"` `` with `` `ds_build_mode: "bulk"` ``.

4. `BuildMode` trait section: replace the hypothetical `HashTrieBuildMode` usage block (from `Usage (hypothetical):` to the end of its code fence) with:

````markdown
Usage (`ColumnTrieBuildMode`, in
[`kermit-ds/src/ds/column_trie/build_mode.rs`](../../kermit-ds/src/ds/column_trie/build_mode.rs)):
```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ColumnTrieBuildMode {
    Incremental,
    #[default]
    Bulk,
}

impl BuildMode for ColumnTrieBuildMode {
    fn axis_value(&self) -> String {
        match self {
            Self::Incremental => "incremental",
            Self::Bulk => "bulk",
        }
        .to_string()
    }
}
```

Unlike a Layout or Config axis, the `ds_build_mode` axis is **not** emitted by the relation's `HasOptimizationAxes`: every mode builds the same relation, so the relation cannot know. The bench family that ran the build reports it, through `RelationFamily::build_mode_axes` (`kermit/src/execution.rs`).
````

5. After the `ConfigurableRelation and Configured<R, P>` section (before its trailing `---`), add:

```markdown
### `BuildModeRelation` and `BuiltWith<R, P>` (kermit-ds)

`kermit_ds::BuildModeRelation` ([`kermit-ds/src/relation.rs`](../../kermit-ds/src/relation.rs))
adds `from_tuples_with_build_mode(header, mode, tuples)`. Every mode must build
the same relation, and `Relation::from_tuples` must use the default mode.
Only `ColumnTrie` implements it today.

`kermit_ds::BuiltWith<R, P>` ([`kermit-ds/src/built_with.rs`](../../kermit-ds/src/built_with.rs))
wraps an `R: BuildModeRelation` with a zero-sized
`P: BuildModeProvider<R::BuildMode>`, declared with
`kermit_ds::define_build_mode_provider!`, so macro suites can name a mode as a
type. Unlike `Configured`, it needs no rule that `project` preserve the mode:
projection rebuilds through the default mode, and every mode builds the same
relation.
```

6. "Where to look" table: replace the row `| Bench-report axes merge | [`kermit/src/main.rs`](../../kermit/src/main.rs) (search `optimization_axes`) |` with `| Bench-report axes merge | [`kermit/src/bench/run.rs`](../../kermit/src/bench/run.rs) and [`kermit/src/bench/ds.rs`](../../kermit/src/bench/ds.rs) (search `optimization_axes` / `build_mode_axes`), from the families in [`kermit/src/execution.rs`](../../kermit/src/execution.rs) |`, and append rows:

```markdown
| First BuildMode consumer (ColumnTrie build) | [`kermit-ds/src/ds/column_trie/build_mode.rs`](../../kermit-ds/src/ds/column_trie/build_mode.rs) |
| BuildMode seam and test wrapper | [`kermit-ds/src/relation.rs`](../../kermit-ds/src/relation.rs) (`BuildModeRelation`), [`kermit-ds/src/built_with.rs`](../../kermit-ds/src/built_with.rs) |
| BuildMode join test macro | [`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs) (search `for_build_mode`) |
```

7. "What's implemented today": replace `Three optimizations are implemented — two Layout dimensions and one Config\nvalue:` with `Four optimizations are implemented — two Layout dimensions, one Config\nvalue and one BuildMode:`, add the table row `| ColumnTrie build (bulk / incremental) | BuildMode | `ds_build_mode` | (kermit-specific, issue #84) |`, and replace the paragraph

```markdown
The BuildMode category still has no consumer, so
`define_multiway_join_test_suite_for_build_mode!` lands with the first
BuildMode consumer.
```

   with

```markdown
`define_multiway_join_test_suite_for_build_mode!` landed with the first
BuildMode consumer, ColumnTrie's build.
```

- [ ] **Step 3: `bench-report-schema.md`**

In the `ds_build_mode` bullet (Task 3), append: `Values: `bulk` (default) and `incremental` (`--ds-build incremental`).`

In the back-fill bullet, replace `` and
  `ds_config_load_factor == 0.7` (the historical constant). `` with `` `ds_config_load_factor == 0.7` (the historical constant), and, on
  ColumnTrie rows only, `ds_build_mode == "incremental"` (ColumnTrie's build
  before issue #84; every ColumnTrie report since carries the axis). The
  other structures have no build-mode axis, so their rows stay NaN. ``

Extend Task 3's change-log row with: ` `--ds-build incremental` restores the old build, and kermit-lab back-fills `incremental` on earlier ColumnTrie rows, so the two builds stay comparable.`

- [ ] **Step 4: `CLAUDE.md`**

1. Priorities item 1: replace `BuildMode is tested via the prescribed (not-yet-implemented) `define_multiway_join_test_suite_for_build_mode!`; until the first BuildMode consumer lands, write per-mode tests by hand.` with `BuildMode dimensions are tested via `define_multiway_join_test_suite_for_build_mode!` (one invocation per non-default mode; see the `Incremental` ColumnTrie precedent in `kermit/tests/join_tests.rs`), plus an array-level test that every mode builds the identical structure, capacities included.`
2. Build Commands: after the `--ds-config load-factor=0.5` line add `cargo run -- bench run triangle -i column-trie -a leapfrog-triejoin --ds-build incremental  # BuildMode axis (the pre-#84 build)`.
3. Key Trait Hierarchy, after the `ConfigurableRelation` bullet:
   `- **BuildModeRelation**: `Relation` + a runtime BuildMode value (`from_tuples_with_build_mode`, in `kermit-ds/src/relation.rs`); implemented only by `ColumnTrie` (`ColumnTrieBuildMode`: `bulk` default, `incremental` = the pre-#84 sort-then-insert build). Every mode must build the identical structure. `BuiltWith<R, P>` (`kermit-ds/src/built_with.rs`) lifts a mode to a type so macro test suites can name it. The `ds_build_mode` axis comes from the bench family (`RelationFamily::build_mode_axes`), never the relation.`
4. Testing Patterns, after the `_with_config!` bullet:
   `- `define_multiway_join_test_suite_for_build_mode!(Relation, Algo, Optimiser, Provider)` — the same patterns on `BuiltWith<Relation, Provider>`, delegating to `define_multiway_join_test_suite!` so new patterns reach it; pair it with `kermit_ds::define_build_mode_provider!` (see `Incremental` in `kermit/tests/join_tests.rs`)`
5. "Adding an optimization" step 2: append ` A BuildMode on a data structure implements `BuildModeRelation`; `BuiltWith<R, P>` lifts it to a type.` Step 3: append ` A BuildMode axis is emitted by the bench family instead (`RelationFamily::build_mode_axes`), since the relation is identical under every mode.` Step 4: replace `` `validate_config_choices` rejects `--ds-config` on structures without a Config axis`` with `` `validate_config_choices` / `validate_build_choices` reject `--ds-config` / `--ds-build` on structures without the axis, and every `--ds-*` flag resolves through `DsChoices::resolve` ``.
6. Gotcha "bench run sweeps are cells, not pairs": replace `(`TrieLftj(TreeTrie|ColumnTrie)`, `HashHtj { hasher, pruning, config }`)` with `(`TrieLftj(SortedTrie)` — `SortedTrie::ColumnTrie { build }` carries `--ds-build` — and `HashHtj { hasher, pruning, config }`)`, and append to that bullet: ` `RelationFamily::build_relation` has no default, so every family states how it builds.`

- [ ] **Step 5: `ARCHITECTURE.md`, `BENCHMARKING.md`, `USAGE.md`**

ARCHITECTURE.md:
- Bench-cell table row: `` `Execution::TrieLftj(SortedTrie)` / `TrieLftj<R>` `` → `` `Execution::TrieLftj(SortedTrie)` (`ColumnTrie { build }` carries `--ds-build`) / `TrieLftj<R>` ``.
- Replace the sentence `` `H` is the crate's only optimization axis: `SipHashStrategy` (default) and `FxHashStrategy` are zero-sized types implementing both `HashStrategy` and `LayoutOption`, and `HashTrie<H>` is the sole `HasOptimizationAxes` implementor in the workspace, reporting `ds_layout_hasher`. `` with `` `HashTrie<H, P>` has two Layout axes — the hasher `H` (`SipHashStrategy` default, `FxHashStrategy`) and the pruning policy `P` — and one Config axis, the load factor; it is the only structure implementing `HasOptimizationAxes`, reporting `ds_layout_hasher`, `ds_layout_pruning` and `ds_config_load_factor`. `ColumnTrie`'s BuildMode axis, `ds_build_mode`, is reported by its bench family instead, because the built trie is the same under every mode. ``
- Selector dispatch: `` `TrieLftj(TreeTrie | ColumnTrie)` `` → `` `TrieLftj(SortedTrie)` (whose `ColumnTrie { build }` variant carries the `--ds-build` mode) ``, and append to the `bench ds` paragraph: `` `SortedTrieFamily<R>` likewise carries ColumnTrie's `--ds-build` mode (`R::BuildMode`, `()` for TreeTrie), builds through it, and reports it as `ds_build_mode` via `RelationFamily::build_mode_axes`. ``
- New Data Structure step 5: `` a `SortedTrie` variant plus a `SortedTrieRelation` impl `` → `` a `SortedTrie` variant plus a `SortedTrieRelation` impl (with its `BuildMode`, `()` for a single build process) ``.

BENCHMARKING.md:
- `honouring\n  any `--ds-config` values.` → `honouring\n  any `--ds-config` values and ColumnTrie's `--ds-build` mode.`
- In "Ablation: measuring an optimization", after the `kl.ablation` Python block, add:

````markdown
`--ds-build` works the same way for ColumnTrie's build (`bulk` by default,
`incremental` for the build before issue #84). For thesis figures, compare the
two modes **within one binary** — `--ds-build incremental` against the default.
kermit-lab back-fills `incremental` on pre-#84 ColumnTrie reports, but those
rows are for continuity only: reports carry no binary identity, so a
difference between an old row and a new one mixes the build mode with every
other change between the two binaries. A build mode only changes the build,
so `kl.ablation` (and `render-all`) draw the `ds_build_mode` axis for the
`insertion` and `end_to_end` phases only.

Give every run its own `--name`: Criterion group names do not encode
optimisation axes, so two runs that differ only in a `--ds-*` flag write into
the same `target/criterion/` directory, and the later overwrites the earlier's
samples.

```sh
kermit bench --name col-bulk --report-json bench-runs/col-bulk.json \
  ds -r data.parquet -i column-trie -m insertion
kermit bench --name col-incr --report-json bench-runs/col-incr.json \
  ds -r data.parquet -i column-trie -m insertion --ds-build incremental
```
````

USAGE.md:
- `bench join` paragraph: ``accepts\n`--ds-config` alongside the `--ds-layout-*` flags`` → ``accepts\n`--ds-config` and `--ds-build` alongside the `--ds-layout-*` flags``.
- End of the `bench ds` section (after the `--queries-per-build 4` example) add:

````markdown
`-i column-trie` takes `--ds-build bulk|incremental` (default `bulk`), choosing
how the trie is built from its tuples. Both build the identical trie, so the
flag changes the `insertion` and `end-to-end` timings only; the report records
it as `ds_build_mode`. `bench run` and `bench join` accept it too.

```sh
kermit bench ds -r data.csv -i column-trie -m insertion --ds-build incremental
```
````

- [ ] **Step 6: Verify and commit**

```bash
CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings nix develop $WT --command cargo doc --workspace --no-deps
git -C $WT add docs CLAUDE.md ARCHITECTURE.md BENCHMARKING.md USAGE.md
git -C $WT commit -F - <<'EOF'
docs: ColumnTrie is the first BuildMode consumer (#84)

The standard's BuildMode examples become ColumnTrie's real ones, with
the BuildModeRelation / BuiltWith seam, the family-emitted axis and the
landed test macro. column-trie.md gains an Optimizations section; the
schema documents the ColumnTrie-scoped back-fill; CLAUDE.md, ARCHITECTURE,
BENCHMARKING and USAGE describe --ds-build. Also corrects two stale
claims touched on the way (the axes-merge location and HashTrie being
the only axis).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01EQYT5rUt5xkPAh85YeHs8t
EOF
```

---

## Task 11 [controller]: Phase 2 gate, evidence, checkpoint

- [ ] **Step 1: Full gate** — repeat Task 4 Step 1, then the kermit-lab suite with the real binary:

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit
systemd-run --user --scope -p MemoryMax=3G -p MemorySwapMax=0 env KERMIT_BIN=$WT/target/debug/kermit uv --directory $WT/python/kermit-lab run pytest
```

- [ ] **Step 2: Miri** — repeat Task 4 Step 2 with log `$SCRATCH/miri-phase2.log`.

- [ ] **Step 3: Evidence — the incremental mode is the old build**

```bash
CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build --release -p kermit
setsid nohup env -C $SCRATCH/ab-run $WT/target/release/kermit bench --sample-size 10 --measurement-time 1 --warm-up-time 1 --name col84-incr --report-json $SCRATCH/ab-run/incr.json ds --relation $REL -i column-trie -m insertion --ds-build incremental > $SCRATCH/ab-incr.log 2>&1 < /dev/null & disown
```

When done: `for g in col84-base col84-bulk col84-incr; do printf '%s ' $g; jq '.mean.point_estimate / 1e9' $SCRATCH/ab-run/target/criterion/$g/ColumnTrie_insertion/new/estimates.json; done`. Expected: `col84-incr` within run-to-run noise of `col84-base` (≈ 34 s); `col84-bulk` far below.

Then confirm `bench run` delivers the mode through `TrieLftj` (the path `bench ds` does not exercise):

```bash
for m in bulk incremental; do env -C $SCRATCH/ab-run $WT/target/release/kermit bench --sample-size 10 --measurement-time 1 --warm-up-time 1 --name col84-run-$m --report-json $SCRATCH/ab-run/run-$m.json run oxford-uniform-s3 -i column-trie -a leapfrog-triejoin -m insertion --ds-build $m; done
find $SCRATCH/ab-run/target/criterion -path '*col84-run-*insertion/new/estimates.json' -exec sh -c 'printf "%s " "$1"; jq ".mean.point_estimate / 1e6" "$1"' _ {} \;
```

Expected: the `incremental` insertion time is clearly above `bulk`'s (if the gap is within noise on this workload, say so and rely on the `bench ds` evidence plus the required-method guarantee).

- [ ] **Step 4: Checkpoint — supervisor and user**

SendMessage to the supervisor: phase-2 commit SHAs, gate and miri results, mutation-check outcomes, the three `friendof` timings and the `bench run` pair. Summarise the same for the user.

---

## Task 12 [controller]: Hand-off (only on the user's instruction)

- [ ] **Step 1:** Invoke `superpowers:finishing-a-development-branch`. Before any merge or push, send supervisor checkpoint 3 and get the user's explicit approval.
- [ ] **Step 2:** If landing: `git -C $WT fetch origin`, then `git -C $WT merge origin/master` (never rebase). Expect conflicts in `kermit/tests/common/macros.rs` and `CLAUDE.md` if the #78 session (`aidanb/78`) has landed; keep both sides' additions. Re-run the full gate (Task 11 Steps 1–2) on the merged tree, then `git -C $WT push origin HEAD:master` only with the user's go-ahead.
