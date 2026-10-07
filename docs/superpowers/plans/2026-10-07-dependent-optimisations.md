# Dependent Optimisations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the optimisation standard a prerequisite mechanism between `--ds-*` axes, and retrofit the presized parallel build to it as its own mode, `--ds-build hash-trie=presized:N`, which requires `--ds-config root-capacity=tuples`.

**Architecture:** `HashTrieBuildMode` gains a `Presized(Threads)` variant; the constructor's `Parallel` arm stops branching on the config and a new `Presized` arm asserts the prerequisite. A `Prerequisite` enum beside `DsFlag` in `kermit/src/options.rs` is the one table of prerequisites, checked at the end of `DsChoices::resolve` so every command rejects a violation with one message. No algorithm, report schema or kermit-lab structure changes; `threads_of` learns the new spelling.

**Tech Stack:** Rust nightly workspace (`cargo`, nightly `rustfmt` via `nix develop`), `clap`, `anyhow`, `paste` test macros; Python `uv` + `pytest` for kermit-lab.

**Spec:** `docs/specs/2026-10-07-dependent-optimisations-design.md`.

**Conventions for every task:**
- Run cargo foreground with `CARGO_BUILD_JOBS=2` (a background memory monitor on this host kills larger builds).
- Format only with `nix develop --command cargo fmt --all` (stable rustfmt rewrites 30+ files).
- Commit messages end with the attribution lines in the session's system reminder.
- The branch is `aidanb/dependent-optimisations`, a loom worktree: commit on it, never switch branches.

---

## File map

| File | Responsibility in this plan |
|---|---|
| `kermit-ds/src/ds/hash_trie/build_mode.rs` | the `Presized(Threads)` variant, its parser and axis value |
| `kermit-ds/src/ds/hash_trie/implementation.rs` | the constructor arms: `Parallel` always merges, `Presized` asserts then fills regions |
| `kermit-ds/src/ds/hash_trie/parallel.rs` | module doc and tests select the presized path by mode |
| `kermit-ds/src/ds/hash_trie/identity.rs`, `src/test_hooks.rs`, `src/configured.rs` | comments and the stacked-marker test |
| `kermit-ds/tests/hash_trie_tests.rs`, `tests/parquet_tests.rs` | `HashPresized2` provider for the presized aliases |
| `kermit/src/options.rs` | `Prerequisite`, `Violation`, the check in `DsChoices::resolve`, `--ds-build` docs |
| `kermit/src/execution.rs` | the family test selects the presized path by mode |
| `kermit/tests/join_tests.rs`, `tests/cli_hash_trie_build_mode.rs` | `HashPresized2`, the CLI rejection test |
| `python/kermit-lab/kermit_lab/frame.py`, `tests/test_frame.py` | `threads_of` reads `presized:N` |
| `docs/specs/optimization-standard.md`, `bench-report-schema.md`, `docs/data-structures/hash-trie.md`, `parallel-build.md`, `docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`, `CLAUDE.md` | Amendment 3 and the renamed mode |

---

### Task 1: `HashTrieBuildMode::Presized(Threads)`

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/build_mode.rs`

- [ ] **Step 1: Write the failing tests**

In the `tests` module of `build_mode.rs`, add a helper beside `parallel(n)` and extend the three existing tests:

```rust
    fn presized(n: usize) -> HashTrieBuildMode {
        HashTrieBuildMode::Presized(Threads::new(n).unwrap())
    }
```

In `axis_values_round_trip_through_from_str`, after `modes.push(parallel(Threads::MAX));`:

```rust
        modes.extend((1..=8).map(presized));
        modes.push(presized(Threads::MAX));
```

In `axis_values_and_default_are_pinned`, before the `default()` assertion:

```rust
        assert_eq!(presized(8).axis_value(), "presized:8");
```

In `malformed_modes_are_rejected_with_the_accepted_forms`, add rows after the `parallel:99…` row:

```rust
            ("presized", "presized needs a thread count"),
            ("presized:", "whole number"),
            ("presized:x", "whole number"),
            ("presized:0", "between 1 and 1024, got 0"),
            ("presized:1025", "between 1 and 1024, got 1025"),
```

and change the accepted-forms assertion in that test to:

```rust
            assert!(
                msg.contains(
                    "expected serial, radix:<bits>, parallel:<threads> or presized:<threads>"
                ),
                "{input:?}: {msg}"
            );
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds build_mode::tests`
Expected: compile error, `no variant named Presized`.

- [ ] **Step 3: Add the variant, parser arm and axis value**

In the enum, after `Parallel(Threads)`:

```rust
    /// The paper's partitioned build (SIGMOD 2020 §3.3.2) on this many
    /// threads: the root is cut into regions, each worker inserts every
    /// tuple of its regions straight into the root, and a tail of deferred
    /// tuples is inserted by the calling thread. The root must be sized
    /// before any tuple arrives, so this mode **requires
    /// `root-capacity=tuples`** (`docs/specs/2026-10-07-dependent-optimisations-design.md`);
    /// the constructor panics without it, and the CLI rejects it first.
    Presized(Threads),
```

In `from_str`, change the `error` closure's expected clause to
`"{why}; expected serial, radix:<bits>, parallel:<threads> or presized:<threads>; bits in {}..={}, threads in 1..={}"`.

Replace the `Some(("parallel", threads))` arm with one arm for both threaded modes:

```rust
            | Some((mode @ ("parallel" | "presized"), threads)) => {
                // TreeTrie's rule (`tree_trie/build_mode.rs`), so every
                // threaded mode accepts and rejects the same thread counts.
                if threads.is_empty() || !threads.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(error(format!(
                        "{mode} threads must be a whole number, got {threads:?}"
                    )));
                }
                // All digits, so a failed parse is a count too large for
                // `usize`: out of range, like any count above the limit.
                let threads = threads
                    .parse::<usize>()
                    .ok()
                    .and_then(Threads::new)
                    .ok_or_else(|| {
                        error(format!(
                            "{mode} threads must be between 1 and {}, got {threads}",
                            Threads::MAX
                        ))
                    })?;
                Ok(if mode == "parallel" {
                    Self::Parallel(threads)
                } else {
                    Self::Presized(threads)
                })
            },
```

and add beside `None if s == "parallel"`:

```rust
            | None if s == "presized" => Err(error("presized needs a thread count".to_owned())),
```

In `axis_value`:

```rust
            | Self::Presized(threads) => format!("presized:{}", threads.get()),
```

Update the enum's doc comment: replace "and every mode here also puts each key in the same bucket, so the mode changes how long the build takes, never the trie it builds (issues #91, #94)" with "`Serial`, `Radix` and `Parallel` also put each key in the same bucket; `Presized` may place root keys in other buckets (Amendment 2). The mode changes how long the build takes, never the trie's contents (issues #91, #94)."

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds build_mode::tests`
Expected: all pass. Then `grep -rn "radix:<bits> or parallel:<threads>" --include='*.rs' --include='*.md' .` must list only `kermit/tests/cli_hash_trie_build_mode.rs` (fixed in Task 6); fix any other hit now.

- [ ] **Step 5: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/build_mode.rs
git commit -m "feat(ds): HashTrieBuildMode::Presized(Threads), the presized build's own mode"
```

---

### Task 2: The constructor selects the presized fill by mode

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (`from_tuples_with_config_and_build_mode`, ~line 539)
- Modify: `kermit-ds/src/ds/hash_trie/parallel.rs` (module doc, `ParallelBuild.deferred` doc, tests)

- [ ] **Step 1: Write the failing tests in `parallel.rs`**

Replace `root_capacity_selects_the_parallel_path` with two tests:

```rust
    /// `presized:N` reaches its own path, at the thread count asked: two
    /// worker runs (scatter, then the regions), and a record with a
    /// deferred count.
    #[test]
    fn presized_build_reaches_its_own_path() {
        let tuples: Vec<Vec<usize>> = (0..64).map(|i| vec![i % 16, i]).collect();
        PARALLEL_BUILDS.with(|b| b.borrow_mut().clear());
        crate::morsel::take_worker_runs();
        let _: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            tuples_config(70),
            HashTrieBuildMode::Presized(threads(3)),
            tuples,
        );
        let builds = PARALLEL_BUILDS.with(|b| b.take());
        assert_eq!(builds.len(), 1);
        assert_eq!(builds[0].threads, 3);
        assert!(builds[0].deferred.is_some(), "the presized record");
        assert_eq!(builds[0].partition_sizes.iter().sum::<usize>(), 64);
        assert_eq!(crate::morsel::take_worker_runs(), vec![3, 3]);
    }

    /// `parallel:N` is the exact merge build under every root capacity: a
    /// presized root changes the root's size, not the process, and the
    /// result is identical to serial's under the same config.
    #[test]
    fn parallel_build_merges_under_every_root_capacity() {
        let tuples: Vec<Vec<usize>> = (0..64).map(|i| vec![i % 16, i]).collect();
        for root_capacity in [RootCapacity::Grow, RootCapacity::Tuples] {
            let config = HashTrieConfig {
                root_capacity,
                ..HashTrieConfig::default()
            };
            let serial: HashTrie =
                HashTrie::from_tuples_with_config(2.into(), config, tuples.clone());
            PARALLEL_BUILDS.with(|b| b.borrow_mut().clear());
            crate::morsel::take_worker_runs();
            let built: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
                2.into(),
                config,
                HashTrieBuildMode::Parallel(threads(3)),
                tuples.clone(),
            );
            let builds = PARALLEL_BUILDS.with(|b| b.take());
            assert_eq!(builds.len(), 1, "{root_capacity:?}");
            assert_eq!(builds[0].threads, 3, "{root_capacity:?}");
            assert_eq!(builds[0].deferred, None, "{root_capacity:?}: the merge record");
            assert_eq!(crate::morsel::take_worker_runs(), vec![3, 3], "{root_capacity:?}");
            assert_same_trie(&serial, &built, &format!("{root_capacity:?}"));
        }
    }

    /// The prerequisite at the library boundary: the CLI rejects this pair
    /// first, so a call that reaches here is a broken invariant.
    #[test]
    #[should_panic(expected = "hash-trie=presized:2 requires root-capacity=tuples")]
    fn presized_build_requires_a_presized_root() {
        let _: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            HashTrieConfig::default(),
            HashTrieBuildMode::Presized(threads(2)),
            vec![vec![1, 2]],
        );
    }
```

In `check_presized` (the equivalence loop, ~line 757), change `HashTrieBuildMode::Parallel(threads(t))` to `HashTrieBuildMode::Presized(threads(t))`, and the label text `parallel:{t}` to `presized:{t}`; the two `"… vs parallel:1"` labels become `"… vs presized:1"`. `check_presized`'s doc comment: "identical to their own `presized:1`".

- [ ] **Step 2: Run the tests to verify they fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_trie::parallel`
Expected: `presized_build_reaches_its_own_path` fails (the `Presized` arm is missing, so the `match` in the constructor does not compile) — fix compilation only by adding the arm in Step 3.

- [ ] **Step 3: Rewrite the constructor arms**

In `from_tuples_with_config_and_build_mode`:

```rust
            | HashTrieBuildMode::Parallel(threads) => {
                Self::from_tuples_partitioned(header, config, tuples, |root, arity, tuples| {
                    parallel::fill_root::<H, P, E>(root, arity, tuples, threads, config.load_factor)
                })
            },
            | HashTrieBuildMode::Presized(threads) => {
                // The prerequisite (docs/specs/2026-10-07-dependent-optimisations-design.md):
                // the regions are cut from a root sized before any tuple
                // arrives. The CLI rejects the pair first; here it is a
                // broken invariant, like a wrong arity.
                assert_eq!(
                    config.root_capacity,
                    RootCapacity::Tuples,
                    "hash-trie=presized:{} requires root-capacity=tuples; got root-capacity={}",
                    threads.get(),
                    config.root_capacity.axis_value(),
                );
                Self::from_tuples_partitioned(header, config, tuples, |root, arity, tuples| {
                    parallel::fill_presized_root::<H, P, E>(
                        root,
                        arity,
                        tuples,
                        threads,
                        config.load_factor,
                    )
                })
            },
```

Update the `from_tuples_partitioned` doc: "What the partitioned builds share (`radix:K`, `parallel:N`, `presized:N`)". Update the two root-step doc comments (~lines 337 and 353): "the presized parallel build's root step" → "the `presized:N` build's root step".

In `parallel.rs`, rewrite the module doc's last paragraph:

```rust
//! `presized:N` (`docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`;
//! spelled `parallel:N` under `root-capacity=tuples` until 2026-10-07) is
//! the paper's build of a presized root ([`fill_presized_root`]), and
//! requires `root-capacity=tuples` (#88). The partitions are contiguous runs
//! of the root's fixed-size regions, by each tuple's home bucket, and each
//! worker inserts every tuple of its partition once, straight into its run
//! of the root. A tuple whose key's probe would cross its region's end is
//! deferred, and the calling thread inserts the deferred tuples afterwards,
//! in input order. Root keys may then sit in other buckets than the serial
//! build's, so this trie is equivalent rather than identical (Amendment 2):
//! the root has the same capacity, occupied buckets and total displacement,
//! and every subtrie is identical. Regions, not threads, decide which tuples
//! are deferred, so the trie is the same for every N.
```

and the first line: "The `parallel:N` and `presized:N` builds of a [`HashTrie`]…". `ParallelBuild.deferred`'s doc: "`None` for `parallel:N` (the exact build). For `presized:N`, the number of tuples deferred to the calling thread."

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_trie`
Expected: all pass, including `presized_parallel_builds_are_equivalent_*` (now through `Presized`) and the `should_panic`.

- [ ] **Step 5: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/parallel.rs
git commit -m "feat(ds): select the presized fill by mode, not by root capacity"
```

---

### Task 3: kermit-ds consumers of the presized build

**Files:**
- Modify: `kermit-ds/src/configured.rs` (~line 240)
- Modify: `kermit-ds/tests/hash_trie_tests.rs` (~lines 167–181)
- Modify: `kermit-ds/tests/parquet_tests.rs` (~lines 162–172)
- Modify: `kermit-ds/src/test_hooks.rs`, `kermit-ds/src/ds/hash_trie/identity.rs` (comments)

- [ ] **Step 1: Run the DS suites to see them panic**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --test hash_trie_tests presized`
Expected: still passes (they stack `Parallel(2)` on a presized config, which is now the exact build). The change below is to keep testing the presized fill.

- [ ] **Step 2: Point the presized aliases at a `presized:2` provider**

`hash_trie_tests.rs`: after the `HashParallel2` provider add

```rust
// ── BuildMode × Config: the presized build ──────────────────────────────
// `presized:2` requires `root-capacity=tuples` (`PresizedRoot` above), so
// it is only ever stacked on a presized alias. It fills the root by region
// (the paper's build, #94); the trie is equivalent to serial's (Amendment
// 2), so the iterator contract holds unchanged.
define_build_mode_provider!(
    HashPresized2,
    HashTrieBuildMode,
    HashTrieBuildMode::Presized(Threads::new(2).expect("2 is not zero"))
);
```

and change the three `*PresizedParallel2` aliases to `BuiltWith<…, HashPresized2>`, renaming them `HashTrieSipPresized2`, `HashTrieSipLazyPresized2`, `HashTrieFxPrunedPresized2` (and their `hash_trie_test_suite!` lines). Delete the old comment block above them.

`parquet_tests.rs`: same provider, alias `HashTrieSipPresized2 = BuiltWith<HashTrieSipPresized, HashPresized2>`, helper `sorted_tuples_presized2`, comment "…and the presized build, `presized:2`, which requires root-capacity=tuples (#94, #88)."

`configured.rs`: change `TwoThreads` to `HashTrieBuildMode::Presized(…)` and the test doc to "(the presized build needs the config's root capacity, so a wrong stack would panic here)".

`test_hooks.rs`: `take_hash_trie_parallel_builds` doc: "Takes the record of every [`HashTrieBuildMode::Parallel`] and [`HashTrieBuildMode::Presized`] build … and, for a `presized:N` build, how many tuples it deferred". Add the second intra-doc link line `[`HashTrieBuildMode::Presized`]: crate::HashTrieBuildMode::Presized`.

`identity.rs` module doc: "the `presized:N` build must be equivalent (Amendment 2)".

- [ ] **Step 3: Run the crate's tests and docs**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds && CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-ds --no-deps`
Expected: all pass; docs build without warnings.

- [ ] **Step 4: Commit**

```bash
git add kermit-ds
git commit -m "test(ds): stack presized:2 on the presized aliases"
```

---

### Task 4: The `Prerequisite` table and the check in `DsChoices::resolve`

**Files:**
- Modify: `kermit/src/options.rs` (`DsFlag` block ~line 53; `DsChoices::resolve` ~line 895; `BuildChoices` doc ~line 720; tests)

- [ ] **Step 1: Write the failing tests**

In `options.rs`'s `tests` module, after `ds_choices_resolve_carries_the_build_mode`:

```rust
    /// Every row of the prerequisite table can fire and can be satisfied:
    /// a row whose `violated` arm never matches would be dead.
    #[test]
    fn every_prerequisite_is_reachable() {
        let presized = HashTrieBuildMode::Presized(Threads::new(2).unwrap());
        for &row in Prerequisite::ALL {
            let (violating, satisfied) = match row {
                | Prerequisite::PresizedBuildNeedsPresizedRoot => {
                    let mut violating = DsChoices::default();
                    violating.build.hash_trie = presized;
                    let mut satisfied = violating;
                    satisfied.config.root_capacity = RootCapacity::Tuples;
                    (violating, satisfied)
                },
            };
            let v = row.violated(&violating).unwrap_or_else(|| panic!("{row:?} never fires"));
            assert_eq!(v.dependent, "--ds-build hash-trie=presized:2", "{row:?}");
            assert_eq!(v.requires, "--ds-config root-capacity=tuples", "{row:?}");
            assert_eq!(v.actual, "root-capacity=grow (the default)", "{row:?}");
            assert!(row.violated(&satisfied).is_none(), "{row:?} fires when satisfied");
            assert!(row.violated(&DsChoices::default()).is_none(), "{row:?} fires by default");
        }
    }

    /// The check runs last in `resolve`, on the resolved values, and its
    /// message names the flag to add.
    #[test]
    fn ds_choices_resolve_rejects_a_violated_prerequisite() {
        let err = DsChoices::resolve(
            IndexStructureSelector::HashTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build(&["hash-trie=presized:2"]),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "--ds-build hash-trie=presized:2 requires --ds-config root-capacity=tuples; got \
             root-capacity=grow (the default)"
        );
        let explicit = DsChoices::resolve(
            IndexStructureSelector::All,
            &LayoutChoices::default(),
            &ConfigChoices {
                ds_config: vec!["root-capacity=grow".into()],
            },
            &build(&["hash-trie=presized:2"]),
        )
        .unwrap_err();
        assert!(explicit.to_string().ends_with("got root-capacity=grow (the default)"));
        let ok = DsChoices::resolve(
            IndexStructureSelector::All,
            &LayoutChoices::default(),
            &ConfigChoices {
                ds_config: vec!["root-capacity=tuples".into()],
            },
            &build(&["hash-trie=presized:2"]),
        )
        .unwrap();
        assert_eq!(
            ok.build.hash_trie,
            HashTrieBuildMode::Presized(Threads::new(2).unwrap())
        );
        assert_eq!(ok.config.root_capacity, RootCapacity::Tuples);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit prerequisite`
Expected: compile error, `Prerequisite` not found.

- [ ] **Step 3: Add the table and the check**

After the `impl fmt::Display for DsFlag` block:

```rust
/// A `--ds-*` value that is valid only under another: the one table of
/// prerequisites between optimisation axes
/// (`docs/specs/optimization-standard.md` § Dependencies between
/// optimisations). An axis value means one thing under every other axis,
/// so the only dependency the standard admits is a prerequisite, and the
/// only answer to a missing one is this usage error: no flag implies
/// another. A row applies on the structure that has both axes, which the
/// per-structure checks have already established.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Prerequisite {
    /// `--ds-build hash-trie=presized:N` cuts its regions from a root sized
    /// before any tuple arrives, so it needs `--ds-config
    /// root-capacity=tuples`.
    PresizedBuildNeedsPresizedRoot,
}

impl Prerequisite {
    /// Every row, for the check and the guard test.
    pub(crate) const ALL: &'static [Prerequisite] = &[Self::PresizedBuildNeedsPresizedRoot];

    /// `Some(violation)` when `choices` selects the dependent value without
    /// its prerequisite.
    pub(crate) fn violated(self, choices: &DsChoices) -> Option<Violation> {
        match self {
            | Self::PresizedBuildNeedsPresizedRoot => match choices.build.hash_trie {
                | HashTrieBuildMode::Presized(threads)
                    if choices.config.root_capacity != RootCapacity::Tuples =>
                {
                    Some(Violation {
                        dependent: format!("--ds-build hash-trie=presized:{}", threads.get()),
                        requires: "--ds-config root-capacity=tuples",
                        actual: format!(
                            "root-capacity={}{}",
                            choices.config.root_capacity.axis_value(),
                            if choices.config.root_capacity == RootCapacity::default() {
                                " (the default)"
                            } else {
                                ""
                            }
                        ),
                    })
                },
                | _ => None,
            },
        }
    }
}

/// What a violated [`Prerequisite`] reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Violation {
    /// The flag and value the user gave: `--ds-build hash-trie=presized:8`.
    pub dependent: String,
    /// The flag and value it needs: `--ds-config root-capacity=tuples`.
    pub requires: &'static str,
    /// What the required axis resolved to: `root-capacity=grow (the default)`.
    pub actual: String,
}
```

In `DsChoices::resolve`, replace the final `Ok(Self { … })` with:

```rust
        let choices = Self {
            hasher: layout.hash_trie_hasher_resolved(),
            pruning: layout.hash_trie_pruning_resolved(),
            expansion: layout.hash_trie_expansion_resolved(),
            seek: layout.sorted_trie_seek_resolved(),
            config: config.hash_trie_config_resolved()?,
            build: build.resolved()?,
        };
        if let Some(v) = Prerequisite::ALL.iter().find_map(|p| p.violated(&choices)) {
            anyhow::bail!("{} requires {}; got {}", v.dependent, v.requires, v.actual);
        }
        Ok(choices)
```

and extend its `# Errors` doc: "…, if `--ds-config` or `--ds-build` is malformed, or if a value's prerequisite is missing ([`Prerequisite`])."

`BuildChoices`'s field doc: change the hash-trie clause to
`` `hash-trie=serial|radix:<bits>|parallel:<threads>|presized:<threads>` (default `serial`; bits in 1..=16, threads in 1..=1024; `presized` requires `--ds-config root-capacity=tuples`) ``.

`Threads` is in scope for the tests already (`kermit_ds::Threads` is imported in the test module's `use`); `RootCapacity` and `HashTrieBuildMode` are imported at the top of the file.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit options::`
Expected: all pass, including `build_choices_resolve_to_each_structures_labels` (it reads `axis_value`, which Task 1 covers).

- [ ] **Step 5: Commit**

```bash
git add kermit/src/options.rs
git commit -m "feat(cli): the Prerequisite table; reject presized:N without root-capacity=tuples"
```

---

### Task 5: The family test selects the presized path by mode

**Files:**
- Modify: `kermit/src/execution.rs` (`hash_trie_families_build_with_their_parallel_mode`, ~line 1868)

- [ ] **Step 1: Rewrite the loop**

Replace the `for config in [HashTrieConfig::default(), presized] { let presized = …; for (mode, expected) in […] {` header and the deferred assertion so the mode, not the config, decides the path. The table becomes `(config, mode, expected threads, deferred?)`:

```rust
            let three_presized = HashTrieBuildMode::Presized(kermit_ds::Threads::new(3).unwrap());
            // `presized:N` requires `root-capacity=tuples`, so it is only
            // paired with the presized config; `parallel:N` runs the exact
            // build under both.
            for (config, mode, expected, deferred) in [
                (HashTrieConfig::default(), HashTrieBuildMode::Serial, vec![], false),
                (HashTrieConfig::default(), radix, vec![], false),
                (HashTrieConfig::default(), three, vec![3], false),
                (presized, HashTrieBuildMode::Serial, vec![], false),
                (presized, radix, vec![], false),
                (presized, three, vec![3], false),
                (presized, three_presized, vec![3], true),
            ] {
```

keep the body, and change the second assertion to
`builds.iter().all(|b| b.deferred.is_some() == deferred)` with the message `"{} {route} under {mode:?}, {config:?}: wrong path"`. Update the test's doc comment: "…the family's mode reached the build, on every route a relation is built, copies included, eager or lazy, and `presized:N` reached the presized fill (#94)."

- [ ] **Step 2: Run the test**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit hash_trie_families_build_with_their_parallel_mode`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add kermit/src/execution.rs
git commit -m "test(kermit): the presized fill is reached by presized:N on every route"
```

---

### Task 6: Join suite and CLI tests

**Files:**
- Modify: `kermit/tests/join_tests.rs` (the "BuildMode × Config" block, ~line 390)
- Modify: `kermit/tests/cli_hash_trie_build_mode.rs`

- [ ] **Step 1: Write the failing CLI test**

Append to `cli_hash_trie_build_mode.rs`:

```rust
/// `presized:N` requires `root-capacity=tuples`: the prerequisite table
/// (`Prerequisite` in `kermit/src/options.rs`) rejects the pair before
/// anything is built, on every command, naming the flag to add.
#[test]
fn cli_rejects_presized_without_a_presized_root() {
    const MESSAGE: &str = "--ds-build hash-trie=presized:2 requires --ds-config \
                           root-capacity=tuples; got root-capacity=grow (the default)";
    let (output, _) = bench_ds("hash-trie", &["--ds-build", "hash-trie=presized:2"]);
    assert!(!output.status.success(), "bench ds accepted presized:2 under grow");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(MESSAGE), "bench ds: {stderr}");

    let (output, _) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-config",
        "root-capacity=grow",
        "--ds-build",
        "hash-trie=presized:2",
    ]);
    assert!(!output.status.success(), "bench run accepted presized:2 under grow");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(MESSAGE), "bench run: {stderr}");

    let (output, _) = bench_join("hash-trie", "hash-triejoin", &[
        "-m",
        "space",
        "--ds-build",
        "hash-trie=presized:2",
    ]);
    assert!(!output.status.success(), "bench join accepted presized:2 under grow");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(MESSAGE), "bench join: {stderr}");
}
```

Then update the existing tests:
- the module doc: "`serial` by default, `radix:<bits>`, `parallel:<threads>` or `presized:<threads>` under `--ds-build hash-trie=…`. Every mode builds an equivalent trie (the identical one, except `presized:N`, whose root keys may sit in other buckets)".
- `cli_bench_ds_rejects_malformed_hash_trie_modes`: add `"presized"`, `"presized:0"` to the list and change the expected clause to `"expected serial, radix:<bits>, parallel:<threads> or presized:<threads>"`.
- `cli_bench_ds_records_a_presized_parallel_build`: pass `"hash-trie=presized:2"` and assert `axes["ds_build_mode"] == "presized:2"`; doc "`presized:N` is the paper's presized build; it requires `root-capacity=tuples`. One report records both."
- `cli_bench_run_verifies_presized_parallel_builds`: pass `"hash-trie=presized:3"` and add `assert_eq!(axes["ds_build_mode"], "presized:3", "{expansion}: {axes}");`.

- [ ] **Step 2: Run the CLI tests to verify the new one fails**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_hash_trie_build_mode`
Expected: PASS. Task 4 already rejects the pair, so this step confirms the three updated tests and the new one against the real binary. If the message differs, the test is wrong, not the code: the string is pinned by Task 4's unit test.

- [ ] **Step 3: Switch the join suite's presized block to `HashPresized2`**

In `join_tests.rs`, before the "BuildMode × Config" block's aliases add:

```rust
define_build_mode_provider!(
    HashPresized2,
    HashTrieBuildMode,
    HashTrieBuildMode::Presized(Threads::new(2).expect("2 is not zero"))
);
```

change the block comment to

```rust
// ── BuildMode × Config: the presized build ──────────────────────────────
// `presized:2` requires root-capacity=tuples (#88, `PresizedRoot` above),
// so it is stacked on the presized aliases only: the paper's partitioned
// build (#94), which builds an equivalent trie (Amendment 2). The same
// three Layouts, under every optimiser. This is the stacked cell the
// standard's prerequisite rule asks for.
```

and replace `HashParallel2` with `HashPresized2` in the nine invocations of that block only (the `HashParallel2` rows above it stay).

- [ ] **Step 4: Run the join tests**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests presized`
Expected: PASS (27 tests × 16 patterns through the presized fill).

- [ ] **Step 5: Commit**

```bash
git add kermit/tests/join_tests.rs kermit/tests/cli_hash_trie_build_mode.rs
git commit -m "test(kermit): presized:2 through the join suite; the CLI rejection pinned"
```

---

### Task 7: kermit-lab reads `presized:N`

**Files:**
- Modify: `python/kermit-lab/kermit_lab/frame.py` (`threads_of`, ~line 122)
- Modify: `python/kermit-lab/tests/test_frame.py` (~line 272)

- [ ] **Step 1: Write the failing test**

In `test_threads_of_reads_only_well_formed_parallel_modes` add, after the two `parallel` assertions:

```python
    assert threads_of("presized:4") == 4
    assert threads_of("presized:1024") == 1024
```

and add `"presized:", "presized:x"` to the not-a-thread-count tuple.

- [ ] **Step 2: Run it to verify it fails**

Run: `cd python/kermit-lab && uv run pytest tests/test_frame.py -k threads_of -q`
Expected: FAIL, `threads_of("presized:4") is None`.

- [ ] **Step 3: Implement**

```python
_THREADED_MODES = ("parallel:", "presized:")


def threads_of(build_mode: object) -> int | None:
    """The thread count of a ``parallel:N`` or ``presized:N`` build mode, else ``None``.

    ``serial``, ColumnTrie's ``bulk`` / ``incremental``, a missing mode and a
    malformed threaded value are not thread counts, so they become ``<NA>``
    in the ``threads`` column. ``presized:N`` is HashTrie's presized build
    (it requires ``root-capacity=tuples``); its speedup baseline is ``serial``
    under the same config, which the case keys already separate.
    """
    if isinstance(build_mode, str):
        for prefix in _THREADED_MODES:
            if build_mode.startswith(prefix):
                count = build_mode.removeprefix(prefix)
                # ASCII only: `"²".isdigit()` holds, but `int("²")` raises.
                if count.isascii() and count.isdigit():
                    return int(count)
    return None
```

Update the `threads` column docstring in the same file (~line 208): "the ``N`` of a ``parallel:N`` or ``presized:N`` build".

- [ ] **Step 4: Run the Python suite**

Run: `cd python/kermit-lab && uv run pytest -q`
Expected: all pass (the contract test needs `KERMIT_BIN`; it skips without it, which is fine here).

- [ ] **Step 5: Commit**

```bash
git add python/kermit-lab
git commit -m "feat(kermit-lab): threads_of reads presized:N"
```

---

### Task 8: Documentation

**Files:**
- Modify: `docs/specs/optimization-standard.md`
- Modify: `docs/specs/bench-report-schema.md`
- Modify: `docs/data-structures/hash-trie.md`
- Modify: `docs/data-structures/parallel-build.md`
- Modify: `docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: `optimization-standard.md`**

Insert a new section between "### BuildMode — *changes how the structure is built*" (ends at the `---` after its table) and "## What this looks like at the CLI":

~~~markdown
## Dependencies between optimisations

**One value, one behaviour** (Amendment 3, 2026-10-07,
[`2026-10-07-dependent-optimisations-design.md`](2026-10-07-dependent-optimisations-design.md)).
An axis value names exactly one shape, value or process, whatever every other
axis is set to. If an optimisation's behaviour would differ under another
axis, it is two optimisations; give each its own value. The presized parallel
build is the precedent: it had been `parallel:N` under `root-capacity=tuples`
and became `presized:N`.

**Prerequisites.** The only dependency the standard admits: value `v` of axis
B is valid only when axis A has a value in set S. Declare it as a variant of
`Prerequisite` in [`kermit/src/options.rs`](../../kermit/src/options.rs), the
one table of prerequisites, which `DsChoices::resolve` checks after resolving
every `--ds-*` flag. A violation is a usage error naming the flag to add; no
flag ever implies another:

```
$ kermit bench ds -i hash-trie --ds-build hash-trie=presized:8
Error: --ds-build hash-trie=presized:8 requires --ds-config root-capacity=tuples; got root-capacity=grow (the default)
```

The constructor asserts the same condition (`HashTrie::from_tuples_with_config_and_build_mode`
panics), so a library caller cannot reach the dependent process without its
prerequisite either. The report needs nothing new: both axes are already
columns, and a report can only carry a pair the binary accepted.

A prerequisite can name one value or several, cross categories (a BuildMode on
a Config, a Config on a Layout, …), express a conflict (a prerequisite on the
values that remain allowed) and chain (every row is checked against the one
resolved `DsChoices`). It cannot express conditional behaviour (split the
value), implication (state every axis), dependence on the algorithm or
optimiser (that is cell validity, owned by `Execution`), or dependence on the
data (a fallback inside the build, which the BuildMode rule already covers).

**Test obligation** for a dependent value: the full suite on a stacked cell
(`BuiltWith<Configured<R, Prereq>, Mode>` and the like) under every compatible
algorithm and optimiser; the CLI rejection pinned in the structure's `cli_*`
test; the constructor's panic pinned; and the guard test
`every_prerequisite_is_reachable`, which builds a violating and a satisfying
`DsChoices` per row. A Layout that depends on a Layout may also be made
unrepresentable by a trait bound, but the row and the CLI check are still
required, since `with_hash_trie_layout!` expands every combination.
~~~

Other edits in the same file:
- "How to classify" bullet list: add a fourth bullet after BuildMode: `- An optimisation that would behave differently under another axis is **two** optimisations, each a value of its own; see [Dependencies](#dependencies-between-optimisations).`
- BuildMode table "Examples" row: `ColumnTrie bulk / incremental ✓, HashTrie radix partitioning ✓, TreeTrie serial / parallel:N ✓, HashTrie serial / parallel:N / presized:N (requires root-capacity=tuples) ✓`.
- "What this looks like at the CLI" build examples: add
  ```bash
  # HashTrie's presized build (#94, the paper's §3.3.2): requires the presized root of #88
  kermit bench run triangle -i hash-trie -a hash-triejoin --ds-config root-capacity=tuples --ds-build hash-trie=presized:8
  ```
- "Where to look in the code" table: add `| The prerequisite table | [`kermit/src/options.rs`](../../kermit/src/options.rs) (`Prerequisite`, checked in `DsChoices::resolve`) |`.
- "What's implemented today": the HashTrie parallel row becomes `| HashTrie parallel build (serial / parallel:N) | BuildMode | `ds_build_mode` | §3.3.2 (morsel-driven, exact; issue #94) |` and add `| HashTrie presized build (presized:N; requires root-capacity=tuples) | BuildMode | `ds_build_mode` | §3.3.2 (the paper's partitioned fill; issue #94, Amendment 3) |`; the intro sentence becomes "Eleven optimizations are implemented — four Layout dimensions, two Config values and five BuildModes".
- Pitfalls FAQ: add after "What if my optimization spans categories?":
  ```markdown
  ### Q: My optimization only works when another one is on.

  Declare the prerequisite (see [Dependencies](#dependencies-between-optimisations)). If instead it *works differently* when the other one is on, it is two optimisations: give the dependent process its own value, as `presized:N` is to `parallel:N`.
  ```

- [ ] **Step 2: `bench-report-schema.md`**

In "Standard axis prefixes", the `ds_build_mode` bullet's HashTrie values: `serial` (default), `radix:<bits>` …, `parallel:N` … and `presized:N` (`--ds-build hash-trie=presized:N`, N threads in 1..=1024; valid only with `ds_config_root_capacity: "tuples"`, which the CLI enforces). Add a history row:

```markdown
| 3 (no bump) | 2026-10-07 | HashTrie's presized parallel build is its own `ds_build_mode` value, `presized:N` (`--ds-build hash-trie=presized:N`, requires `--ds-config root-capacity=tuples`); `parallel:N` is the exact merge build under every root capacity. A new value of an existing key, so `schema_version` stays `3`. Reports written before this change carry `parallel:N` with `ds_config_root_capacity: "tuples"` for the presized build (the 2026-10-06 scaling run); kermit-lab does not rewrite them. |
```

- [ ] **Step 3: `hash-trie.md`**

- § Build modes intro: "`presized:N` uses that freedom; it may place root keys in other buckets, so `iteration` is measured for it."
- Table: the `Parallel(N)` row drops its "under `root-capacity=tuples` …" clause; add `| `Presized(N)` | `hash-trie=presized:N`, N in 1..=1024; **requires `--ds-config root-capacity=tuples`** | the paper's build: partitions are regions of the presized root, one insert per tuple straight into the root, a tail of deferred tuples on the calling thread (§3.3.2; Amendment 3 of the standard) |`.
- "**The presized parallel build.**" paragraph: begins "**The presized build** (`presized:N`). It requires `root-capacity=tuples`, since its regions are cut from a root sized before any tuple arrives; the CLI rejects the pair otherwise and the constructor panics. It partitions the input into contiguous regions …"; its link becomes `parallel-build.md#the-presized-build-presizedn`.
- Axis bullet: `` (`serial` / `radix:K` / `parallel:N` / `presized:N`) ``.
- Tests bullet: aliases `HashTrieSipPresized2`, `HashTrieSipLazyPresized2`, `HashTrieFxPrunedPresized2` (and `HashTrieSipPresized2` in `parquet_tests.rs`); "`presized_parallel_builds_are_*` … equivalence with serial under `root-capacity=tuples` and identity with `presized:1`"; `hash_trie_families_build_with_their_parallel_mode` "(the mode reaches the build on every route, and `presized:N` reaches the presized fill)"; add "`presized_build_requires_a_presized_root` in `parallel.rs` and `cli_rejects_presized_without_a_presized_root` in `cli_hash_trie_build_mode.rs`: the prerequisite at both boundaries".
- Measured effect: "The presized build's curve (spelled `parallel:N` under `root-capacity=tuples` when it was run)".

- [ ] **Step 4: `parallel-build.md`**

- Title: `# Parallel builds (`--ds-build tree-trie=parallel:N`, `hash-trie=parallel:N`, `hash-trie=presized:N`)`.
- Intro: "HashTrie's presized build (`hash-trie=presized:N`, which requires `--ds-config root-capacity=tuples`) builds an equivalent trie …".
- Mode table: add `| `HashTrieBuildMode::Presized(n)` | `hash-trie=presized:N` (requires `root-capacity=tuples`) | `"presized:N"` | |`.
- "The HashTrie builds are `parallel::fill_root` (`parallel:N`) and `parallel::fill_presized_root` (`presized:N`), both in …".
- § HashTrie: "HashTrie has two parallel builds, two modes: `parallel:N`, the **exact build** … under every root capacity; and `presized:N`, the **presized build** (the paper's, §3.3.2), which requires `root-capacity=tuples` (#88) and builds an equivalent trie." Fix the in-page link to `#the-presized-build-presizedn`.
- Rename `### The presized build (root-capacity=tuples)` to `### The presized build (presized:N)`; its pseudocode header `presized:N (requires root-capacity=tuples):`.
- Scaling record § Setup, "Curves" bullet: append "At b882bd6 the presized arms were spelled `--ds-build hash-trie=parallel:N` under `root-capacity=tuples`; since 2026-10-07 that build is `--ds-build hash-trie=presized:N` (same prerequisite). The run's reports therefore carry `ds_build_mode: parallel:N` beside `ds_config_root_capacity: tuples`."
- Then `grep -rn "root-capacitytuples" docs/ CLAUDE.md` must return nothing; fix every remaining anchor.

- [ ] **Step 5: The presized design spec and `CLAUDE.md`**

Top of `2026-10-06-hash-trie-presized-parallel-build-design.md`, after the Status line:

```markdown
**Superseded in spelling (2026-10-07):** the build this design adds is now its own mode,
`--ds-build hash-trie=presized:N`, which requires `--ds-config root-capacity=tuples`;
`parallel:N` is the exact merge build under every config. The "new rule" under
§ Placement is withdrawn by Amendment 3 (`2026-10-07-dependent-optimisations-design.md`).
Everything below about the process, equivalence and tests still holds.
```

`CLAUDE.md`:
- line 42 area: add `cargo run -- bench run triangle -i hash-trie -a hash-triejoin --ds-config root-capacity=tuples --ds-build hash-trie=presized:8  # BuildMode axis: the paper's presized build (#94); requires the presized root (Amendment 3)`.
- `BuildModeRelation` bullet: replace "`parallel:N` = the same steps with partition and build on N threads and a k-way merge, #94; under `root-capacity=tuples` it is the paper's presized build (partitions are root regions, a deferred tail)" with "`parallel:N` = the same steps with partition and build on N threads and a k-way merge, #94; `presized:N` = the paper's presized build (partitions are root regions, a deferred tail), which **requires** `root-capacity=tuples`". Change "Every landed mode is array-identical." to "Every landed mode but `presized:N` is array-identical."
- "Adding an optimization" recipe, after step 4: add `5. If the optimization is valid only under another axis's value, add a `Prerequisite` variant in `kermit/src/options.rs` (the one table; `DsChoices::resolve` rejects a violation) and test it as a stacked cell. An optimization that would *behave differently* under another axis is two optimizations with their own values — see `docs/specs/optimization-standard.md` § Dependencies.` and renumber the two steps after it.
- `test-hooks` bullet: "the records each `parallel:N` / `presized:N` build pushes".
- Gotchas: add `- **Prerequisites between `--ds-*` flags**: `--ds-build hash-trie=presized:N` requires `--ds-config root-capacity=tuples`; `DsChoices::resolve` rejects the pair with a message naming the flag, and `HashTrie::from_tuples_with_config_and_build_mode` panics on it. No flag implies another. The table is `Prerequisite` in `kermit/src/options.rs` (Amendment 3, 2026-10-07).`

- [ ] **Step 6: Check the docs build and commit**

Run: `CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps && grep -rn "root-capacitytuples\|HashTrieSipPresizedParallel2" docs CLAUDE.md kermit kermit-ds python | grep -v target`
Expected: docs build; the grep prints nothing.

```bash
git add docs CLAUDE.md
git commit -m "docs: Amendment 3, prerequisites between optimisations; presized:N"
```

---

### Task 9: The full gate

- [ ] **Step 1: Format**

Run: `nix develop --command cargo fmt --all && git diff --stat`
Expected: at most whitespace in files this plan touched. Commit any formatting change as `style: rustfmt`.

- [ ] **Step 2: Clippy, tests, docs**

Run, each foreground:
```bash
CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
CARGO_BUILD_JOBS=2 cargo test --workspace
CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace
CARGO_BUILD_JOBS=2 cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings
```
Expected: clippy clean; every test passes; docs clean; the last command shows only its two known pre-existing errors (memory `project_bin_docs_unlinted`) and none in `options.rs`'s new items.

- [ ] **Step 3: Miri on the touched crate**

Run: `MIRIFLAGS="-Zmiri-disable-isolation" CARGO_BUILD_JOBS=2 cargo miri test -p kermit-ds hash_trie::parallel`
Expected: passes (the presized matrix is already Miri-sized).

- [ ] **Step 4: Python**

Run:
```bash
CARGO_BUILD_JOBS=2 cargo build --release -p kermit
cd python/kermit-lab && KERMIT_BIN="$(git rev-parse --show-toplevel)/target/release/kermit" uv run pytest -q
```
Expected: all pass, the contract test included.

- [ ] **Step 5: Smoke the CLI end to end**

```bash
cargo run -q -- bench ds -i hash-trie --ds-build hash-trie=presized:2; echo "exit $?"
cargo run -q -- bench ds -i hash-trie --ds-config root-capacity=tuples --ds-build hash-trie=presized:2 --report-json /tmp/presized.json && python3 -c "import json; r=json.load(open('/tmp/presized.json'))[0]['axes']; print(r['ds_build_mode'], r['ds_config_root_capacity'])"
```
Expected: the first prints the prerequisite error and `exit 1`; the second prints `presized:2 tuples`.

- [ ] **Step 6: Final commit state**

Run: `git status --short && git log --oneline origin/master..HEAD`
Expected: clean tree; the spec commit plus one commit per task above.
