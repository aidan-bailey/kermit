# HashTrie Presized Parallel Build (Paper Layer 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Under #88's `--ds-config root-capacity=tuples`, make
`--ds-build hash-trie=parallel:N` build HashTrie the paper's way. The input
is partitioned into contiguous regions of the presized root, and each worker
inserts every tuple of its regions once, straight into the root. The few
tuples whose probe runs past their region's end go to the calling thread
afterwards. Under the default config (`root-capacity=grow`), `parallel:N`
stays today's exact build.

**Architecture:** There are four work packages; see "Work packages".

- **P1** (Tasks 1–4), `kermit-ds`:
  - `HashTable` gains region runs (`with_runs`, `BucketRun`, `RunEntry`) and
    a free `home_bucket`;
  - `HashTrie` gains a root step mirrored from `insert_at`;
  - `parallel.rs` gains the presized path, selected in the `Parallel` arm by
    `config.root_capacity`;
  - equivalence and identity tests run under Amendment 2;
  - a `ConfiguredBuildModeRelation` trait lets `BuiltWith<Configured<…>, …>`
    compose for the suites.
- **P2** (Tasks 5–6), the `kermit` binary: route, report and CLI tests, and
  the join suites. The binary needs no production change: `HashHtj` already
  passes the config and the mode together.
- **P3** (Task 7): docs and the standard's one sentence.
- **Controller** (Tasks 0, 8, 9): the merge with #88, the gates, a smoke run,
  the checkpoints, and landing on the user's word.

**Tech Stack:** Rust nightly (std scoped threads), the kermit-ds test
macros, Nix dev shell.

**Spec:** [`docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`](../../specs/2026-10-06-hash-trie-presized-parallel-build-design.md) (approved 2026-10-06) · **Issues:** #94, #88, #105 · **Parity rule:** every change moves HashTrie toward the paper; see the spec's "Parity with the paper".

---

## Preconditions

- **#88 has landed on origin/master.** It provides the following, and Task 0
  checks each name:
  - `RootCapacity { Grow, Tuples }` and `HashTrieConfig { load_factor,
    root_capacity }`;
  - `HashTrieConfig::root_log2_capacity`;
  - `hash_table::log2_capacity_for`;
  - `HashTable::with_log2_capacity`;
  - `HashTrie::with_config_for(header, config, tuple_count)`;
  - `HashTrie::make_root_sized(arity, log2_capacity)`;
  - `--ds-config root-capacity=grow|tuples` and the axis
    `ds_config_root_capacity`.

  If a name differs, use #88's and record the difference in the P1 report.
- **This branch, `aidanb/hash-trie-parallel`, is up to date.** It holds plan
  2 (`parallel:N`), Amendment 2, and the two specs. It has either landed on
  origin/master or been merged with it in Task 0.
- **No measurement claims.** Task 8's smoke run is a go/no-go. The presized
  scaling curve is a separate step ("After this plan").

## Ground rules for every task

- **Paths.**
  - `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/hash-trie-parallel_18dbb2f717cebe75`.
  - `SCRATCH` is the executing session's scratchpad.
  - Never `cd`; use `git -C $WT`, `env -C`, `uv --directory`.
- **Cargo.** Run it in the foreground, as `CARGO_BUILD_JOBS=2 nix develop $WT
  --command cargo …`. A memory monitor kills cargo started with
  `run_in_background`. Run jobs over 10 minutes (kermit-ds Miri takes about
  25) with `setsid nohup … & disown`, and poll their log.
- **The host is shared.** Before any timing run, or any cargo run over a few
  minutes, run `pgrep -af 'kermit (bench|ds)|perf record'`. If another
  session is timing, wait, or run niced on one core
  (`nice -n 19 taskset -c 15`, `CARGO_BUILD_JOBS=1`).
- **Formatting.** Only `nix develop $WT --command cargo fmt --all`, before
  every commit.
- **Doc comments.** Backtick every identifier (clippy's `doc_markdown` runs
  under `-Dwarnings`).
- **Commits.** Conventional commits referencing `(#94)`. Stage files by
  name; never amend or push. Every message ends with these two lines; an
  executing session substitutes its own `Claude-Session` line:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky
  ```
- **Mutation checks.**
  1. Commit first.
  2. Apply the mutant as an exact edit.
  3. Confirm the named test fails, and that `git -C $WT diff` shows the mutant
     applied.
  4. Reverse the exact edit, never with `git checkout`.
  5. Re-run the test to green; `git status` must be clean.
- **Scope (Priority 6).**
  - Untouched: `HashTrie::insert_at`, `insert`, `from_tuples_with_config`,
    `expand_level`, `HashTable::bucket_index`, `entry_or_insert_with` and
    `grow`; TreeTrie, ColumnTrie, the algorithms, the optimisers.
  - The radix build's executed path stays the same.
  - `parallel:N` under `root-capacity=grow` executes exactly as before.
  - Task 8 checks all of this with a diff.
- **Parity.**
  - The overflow tail is a kermit mechanism; the paper is silent on probes
    that cross a partition's end. No doc or comment may attribute it to the
    paper.
  - Children are still built by `insert_at` rather than by Algorithm 2's
    lists. That is #107's job, not this plan's.
- **Never write under `~/.cache/kermit`.**
- **Checkpoints with the user:** after P1 (equivalence holds, Miri is clean),
  after Task 8, and before any push.

---

## Work packages

One implementer per package. Each package is reviewed before the next
starts, and the controller merges `origin/master` at every boundary.

| Package | Tasks | Scope | Commits | Done when |
|---|---|---|---|---|
| *Controller* | 0 | Merge #88, resolve its overlap with this branch, record the baseline | merge + 1 | `$SCRATCH/baseline.txt`; workspace green |
| **P1 — `kermit-ds`** | 1–4 | region runs; root step; presized path; wrappers and suites | 4 | `cargo test -p kermit-ds` green; new tests Miri-clean; mutants killed |
| **P2 — `kermit`** | 5–6 | route, axis and CLI tests; join suites | 2 | `cargo test -p kermit` green |
| **P3 — docs** | 7 | parallel-build.md, hash-trie.md, standard, CLAUDE.md, spec status | 1 | `cargo doc` and clippy clean |
| *Controller* | 8, 9 | gate, smoke run, landing | — | — |

Each implementer gets this plan, its task numbers, the ground rules, and a
report budget of about 40 lines. The report covers commit SHAs, test counts,
mutation outcomes, and any deviation with its reason.

---

## Task 0 [controller]: Merge #88 and record the baseline

**Files:** a merge commit. Possibly
`kermit-ds/src/ds/hash_trie/{identity.rs,radix.rs,implementation.rs,parallel.rs}`
and test files, for the conflicts below.

- [ ] **Step 1: Merge.**
  ```bash
  git -C $WT fetch origin
  git -C $WT merge origin/master
  ```

  Expect conflicts. #88 and plan 2 both did the same refactor:
  - **`kermit-ds/src/ds/hash_trie/identity.rs` (add/add).** The two files
    differ only in their module doc. Keep this branch's body, and use this
    doc:
    ```rust
    //! Array-level identity checks for the HashTrie builds (#91, #94, #88).
    //! Every build mode must build the trie the serial build does, the
    //! root-capacity Config must leave every subtrie as it was, and the
    //! presized parallel build must be equivalent (Amendment 2), so the
    //! radix, parallel and root-capacity tests share these assertions and
    //! inputs.
    ```
  - **`radix.rs` tests.** Keep one `use` block naming the shared helpers.
    Keep #88's `check_identity` loop over `LOAD_PERCENTS × {Grow, Tuples}`.
  - **`implementation.rs`, `from_tuples_partitioned`.** It must create its
    root with #88's helper. Replace `let mut trie =
    Self::with_config(header, config);` with:
    ```rust
    let mut trie = Self::with_config_for(header, config, tuple_count);
    ```
    `tuple_count` is already computed just above. The `Radix` arm must go
    through `from_tuples_partitioned`, not #88's inline version.
  - **Every `HashTrieConfig { load_factor: … }` literal on this branch**
    gains `..HashTrieConfig::default()`. That covers
    `kermit-ds/src/ds/hash_trie/parallel.rs`'s `check_identity`, and
    anything else `cargo check --workspace --all-targets` reports.

- [ ] **Step 2: Check #88's names exist.**
  ```bash
  git -C $WT grep -n -E 'enum RootCapacity|fn root_log2_capacity|fn log2_capacity_for|fn with_log2_capacity|fn with_config_for|fn make_root_sized' -- kermit-ds/src
  ```
  Expected: six hits. Note each visibility. This plan needs `pub(super)` or
  wider for `with_config_for` and `make_root_sized`, and `pub(crate)` for
  `log2_capacity_for`. If one is narrower, widen it in this commit and say
  so in the report.

- [ ] **Step 3: Today's `parallel:N` stays exact under `Tuples`.**

  Under `Tuples`, `from_tuples_partitioned` now presizes. So the exact merge
  lands in a presized root, and must still equal serial bucket for bucket.
  In `parallel.rs`'s `check_identity`, add a `root_capacity` loop around the
  load-factor loop:
  ```rust
          for root_capacity in [RootCapacity::Grow, RootCapacity::Tuples] {
              for &percent in LOAD_PERCENTS {
                  let config = HashTrieConfig {
                      load_factor: LoadFactor::percent(percent).unwrap(),
                      root_capacity,
                  };
  ```
  Close the extra brace after the load-factor loop. Add `{root_capacity:?}`
  to the label. Add `config::RootCapacity` to the test module's `use`.
  `fill_root_in_morsels` receives the root that `make_root` (4 buckets)
  builds, so in the morsels-of-7 branch use the root
  `make_root_sized(arity, config.root_log2_capacity(tuples.len()))` instead.

- [ ] **Step 4: Green, commit, record the baseline.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo check --workspace --all-targets
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib hash_trie
  nix develop $WT --command cargo fmt --all
  git -C $WT commit -am "merge: origin/master with #88; parallel:N stays exact under root-capacity=tuples (#94)

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
  {
    echo "BASE $(git -C $WT rev-parse HEAD)"
    CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {print "cargo "p" passed, "f" failed, "i" ignored"}'
  } | tee $SCRATCH/baseline.txt
  git -C $WT rev-parse HEAD > $SCRATCH/base.sha
  ```
  If the merge left no conflicts and the merge commit already exists,
  commit only the Step 3 change, as `test(hash-trie): parallel:N stays
  exact under root-capacity=tuples (#94)`.

---

# P1 — `kermit-ds`

## Task 1 [P1]: Region runs on `HashTable`

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/hash_table.rs`, adding
  `home_bucket`, `Overflow`, `BucketRun`, `RunEntry`, `VacantBucket`,
  `with_runs` and the test helper `hash_with_home`.
- Test: the same file's `mod tests`.

- [ ] **Step 1: Write the failing tests.** Append these to `hash_table.rs`'s
  `mod tests`:

```rust
    /// A table of `2^log2` buckets holding nothing yet, for the run tests.
    fn empty(log2: u32) -> HashTable<u32> { HashTable::with_log2_capacity(log2) }

    /// Occupied bucket indices, in order.
    fn occupied<V>(t: &HashTable<V>) -> Vec<usize> {
        (0..t.buckets_len()).filter(|&i| t.hash_at(i).is_some()).collect()
    }

    /// Σ over occupied buckets of how far each key sits past its home.
    fn total_displacement<V>(t: &HashTable<V>) -> usize {
        let cap = t.buckets_len();
        let log2 = cap.trailing_zeros();
        (0..cap)
            .filter_map(|i| t.hash_at(i).map(|h| (i + cap - home_bucket(h, log2)) % cap))
            .sum()
    }

    #[test]
    fn home_bucket_matches_bucket_index() {
        let mut lcg = crate::test_support::Lcg(0x88);
        for log2 in 1..20 {
            let table = empty(log2);
            for _ in 0..200 {
                let hash = lcg.next_usize() as u64;
                assert_eq!(home_bucket(hash, log2), table.bucket_index(hash), "2^{log2}");
            }
        }
    }

    #[test]
    fn hash_with_home_lands_where_it_says() {
        for log2 in [2u32, 5, 12] {
            for home in [0usize, 1, (1 << log2) - 1] {
                for low in [0u64, 1, 7] {
                    assert_eq!(home_bucket(hash_with_home(home, log2, low), log2), home);
                }
            }
        }
    }

    /// Two runs of one 8-bucket region each in a 16-bucket table: a run
    /// finds a key it already holds, inserts at the first empty bucket, and
    /// counts its inserts into `len`.
    #[test]
    fn a_run_finds_a_key_or_inserts_at_the_first_empty_bucket() {
        let (a, b) = (hash_with_home(2, 4, 1), hash_with_home(2, 4, 2));
        let mut table = empty(4);
        table.with_runs(2, 8, |mut runs| {
            let run = &mut runs[0];
            let RunEntry::Vacant(slot) = run.entry(a).unwrap() else { panic!("a is new") };
            slot.insert(10);
            let RunEntry::Vacant(slot) = run.entry(b).unwrap() else { panic!("b is new") };
            slot.insert(20);
            let RunEntry::Occupied(value) = run.entry(a).unwrap() else { panic!("a is held") };
            assert_eq!(*value, 10);
        });
        assert_eq!(table.len(), 2);
        assert_eq!(table.index_of(a), Some(2));
        assert_eq!(table.index_of(b), Some(3));
        assert_eq!(table.get(b), Some(&20));
    }

    /// A probe that reaches its region's end is refused, at the end of a
    /// middle region and at the table's last bucket alike: a run never
    /// wraps and never spills into the next region.
    #[test]
    fn a_run_overflows_at_its_region_end_and_never_wraps() {
        let mut table = empty(4);
        table.with_runs(2, 8, |mut runs| {
            for (run, home) in [(0, 7), (1, 15)] {
                let first = hash_with_home(home, 4, 1);
                let RunEntry::Vacant(slot) = runs[run].entry(first).unwrap() else {
                    panic!("{home}: first key is new")
                };
                slot.insert(1);
                let second = hash_with_home(home, 4, 2);
                assert!(runs[run].entry(second).is_err(), "{home}: second key overflows");
            }
        });
        assert_eq!(table.len(), 2);
        assert_eq!(occupied(&table), vec![7, 15]);
    }

    /// Filling by runs, then inserting the overflow by ordinary probing,
    /// occupies exactly the buckets that inserting every key one at a time
    /// occupies, with the same total displacement. That is linear probing's
    /// order independence (Knuth, TAOCP §6.4), and what the presized
    /// parallel build relies on.
    #[test]
    fn runs_and_tail_fill_what_sequential_insertion_fills() {
        let lf = LoadFactor::percent(70).unwrap();
        let seeds = if cfg!(miri) { 2 } else { 40 };
        let mut overflowed = 0;
        for seed in 0..seeds {
            let mut lcg = crate::test_support::Lcg(seed);
            let keys: Vec<u64> = (0..85).map(|_| lcg.next_usize() as u64).collect();
            let mut sequential = empty(7);
            for &k in &keys {
                sequential.entry_or_insert_with(k, lf, || 0);
            }
            let mut by_runs = empty(7);
            let tail: Vec<u64> = by_runs.with_runs(4, 16, |mut runs| {
                let mut tail = Vec::new();
                for &k in &keys {
                    let run = &mut runs[home_bucket(k, 7) / 32];
                    match run.entry(k) {
                        | Ok(RunEntry::Vacant(slot)) => {
                            slot.insert(0);
                        },
                        | Ok(RunEntry::Occupied(_)) => {},
                        | Err(Overflow) => tail.push(k),
                    }
                }
                tail
            });
            overflowed += tail.len();
            for k in tail {
                by_runs.entry_or_insert_with(k, lf, || 0);
            }
            assert_eq!(by_runs.len(), sequential.len(), "seed {seed}");
            assert_eq!(occupied(&by_runs), occupied(&sequential), "seed {seed}");
            assert_eq!(
                total_displacement(&by_runs),
                total_displacement(&sequential),
                "seed {seed}"
            );
            for &k in &keys {
                assert!(by_runs.index_of(k).is_some(), "seed {seed}: {k:#x} lost");
            }
        }
        assert!(overflowed > 0, "no seed overflowed a region; the test proves nothing");
    }
```

- [ ] **Step 2: Run them; expect a compile failure.**

  Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib hash_table`

  Expected: unresolved `home_bucket`, `hash_with_home`, `with_runs`,
  `RunEntry` and `Overflow`.

- [ ] **Step 3: Implement.** In `hash_table.rs`, after the `Entry` struct:

```rust
/// The home bucket of `hash` in a table of `2^log2_capacity` buckets: the
/// index [`HashTable::bucket_index`] computes, as a free function so a
/// [`BucketRun`] can compute it without its table. The two are pinned
/// together by `home_bucket_matches_bucket_index`; `bucket_index` keeps its
/// own body, so the serial path's code is unchanged.
pub(super) fn home_bucket(hash: u64, log2_capacity: u32) -> usize {
    let mixed = hash.wrapping_mul(MULTIPLIERS[log2_capacity as usize]);
    (mixed >> (64 - log2_capacity)) as usize
}

/// A probe that reached the end of its region (see [`BucketRun`]).
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Overflow;

/// A contiguous run of a table's buckets, lent to one worker by
/// [`HashTable::with_runs`]. A run probes only within the
/// `region_buckets`-sized region that holds a key's home bucket, and never
/// grows: a probe that would cross the region's end is refused with
/// [`Overflow`], for the caller to insert afterwards with ordinary probing.
/// Regions, not runs, bound a probe, so which keys overflow does not depend
/// on how many runs the table is split into.
pub(super) struct BucketRun<'a, V> {
    /// The index, in the whole table, of `buckets[0]`.
    first: usize,
    /// Regions are this many buckets, aligned to multiples of it.
    region_buckets: usize,
    log2_capacity: u32,
    buckets: &'a mut [Option<Entry<V>>],
    /// Keys this run inserted; [`HashTable::with_runs`] adds them to `len`.
    inserted: &'a mut usize,
}

/// What [`BucketRun::entry`] found: the key's value, or the empty bucket
/// where the key would go.
pub(super) enum RunEntry<'r, V> {
    Occupied(&'r mut V),
    Vacant(VacantBucket<'r, V>),
}

/// An empty bucket inside a run, ready to take one key.
pub(super) struct VacantBucket<'r, V> {
    slot: &'r mut Option<Entry<V>>,
    hash: u64,
    inserted: &'r mut usize,
}

impl<'r, V> VacantBucket<'r, V> {
    /// Stores `value` under the probed hash and returns it.
    pub(super) fn insert(self, value: V) -> &'r mut V {
        *self.inserted += 1;
        &mut self
            .slot
            .insert(Entry {
                hash: self.hash,
                value,
            })
            .value
    }
}

impl<V> BucketRun<'_, V> {
    /// Probes for `hash` from its home bucket to the end of the home's
    /// region: the key's value if it is there, the first empty bucket if it
    /// is not, or [`Overflow`] if neither comes before the region's end.
    /// The run must hold the key's home bucket.
    pub(super) fn entry(&mut self, hash: u64) -> Result<RunEntry<'_, V>, Overflow> {
        let home = home_bucket(hash, self.log2_capacity);
        debug_assert!(
            (self.first..self.first + self.buckets.len()).contains(&home),
            "bucket {home} is outside this run"
        );
        let region_end = (home / self.region_buckets + 1) * self.region_buckets;
        let position = (home - self.first..region_end - self.first)
            .find(|&idx| self.buckets[idx].as_ref().is_none_or(|entry| entry.hash == hash))
            .ok_or(Overflow)?;
        Ok(match self.buckets[position] {
            | Some(_) => RunEntry::Occupied(
                &mut self.buckets[position]
                    .as_mut()
                    .expect("matched Some just above")
                    .value,
            ),
            | None => RunEntry::Vacant(VacantBucket {
                slot: &mut self.buckets[position],
                hash,
                inserted: &mut *self.inserted,
            }),
        })
    }
}
```

In `impl<V> HashTable<V>`, after `into_buckets`:

```rust
    /// Lends `fill` the bucket array as `parts` contiguous [`BucketRun`]s,
    /// each a whole number of `region_buckets`-sized regions (a region is
    /// the whole table when that is smaller), and adds the keys they insert
    /// to `len` when `fill` returns. The presized parallel build hands one
    /// run to each worker; scoping the runs here keeps `len` right.
    ///
    /// # Panics
    ///
    /// Unless `parts` and `region_buckets` are powers of two and every run
    /// holds whole regions.
    pub(super) fn with_runs<R>(
        &mut self, parts: usize, region_buckets: usize,
        fill: impl FnOnce(Vec<BucketRun<'_, V>>) -> R,
    ) -> R {
        let capacity = self.buckets.len();
        assert!(parts.is_power_of_two() && region_buckets.is_power_of_two());
        let region_buckets = region_buckets.min(capacity);
        assert!(
            parts * region_buckets <= capacity,
            "{parts} runs of whole {region_buckets}-bucket regions do not fit {capacity} buckets"
        );
        let run_len = capacity / parts;
        let log2_capacity = self.log2_capacity;
        let mut inserted = vec![0usize; parts];
        let runs = self
            .buckets
            .chunks_mut(run_len)
            .zip(inserted.iter_mut())
            .enumerate()
            .map(|(k, (buckets, inserted))| BucketRun {
                first: k * run_len,
                region_buckets,
                log2_capacity,
                buckets,
                inserted,
            })
            .collect();
        let result = fill(runs);
        self.len += inserted.iter().sum::<usize>();
        result
    }
```

Before `mod tests`:

```rust
/// A hash whose home bucket in a table of `2^log2_capacity` buckets is
/// `home`. Distinct `low` values give distinct hashes with the same home.
/// The bucket index multiplies by an odd `MULTIPLIERS[p]`, so it is
/// inverted by multiplying by its inverse modulo `2^64`.
#[cfg(test)]
pub(super) fn hash_with_home(home: usize, log2_capacity: u32, low: u64) -> u64 {
    assert!(low >> (64 - log2_capacity) == 0, "low bits must stay below the index");
    let m = MULTIPLIERS[log2_capacity as usize];
    // Newton's iteration for 1/m mod 2^64: m·m ≡ 1 (mod 8) for odd m, and
    // each step doubles the correct low bits (3 → 6 → 12 → 24 → 48 → 96).
    let mut inverse = m;
    for _ in 0..5 {
        inverse = inverse.wrapping_mul(2u64.wrapping_sub(m.wrapping_mul(inverse)));
    }
    debug_assert_eq!(m.wrapping_mul(inverse), 1);
    (((home as u64) << (64 - log2_capacity)) | low).wrapping_mul(inverse)
}
```

- [ ] **Step 4: Run them.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib hash_table
  RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit-ds --all-targets
  ```
  Expected: all `hash_table` tests pass and clippy is clean. If clippy flags
  `BucketRun` or `with_runs` as dead code, that's expected until Task 3:
  `hash_table` is already `#[allow(dead_code)]` in `mod.rs`.

- [ ] **Step 5: Commit.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit-ds/src/ds/hash_trie/hash_table.rs
  git -C $WT commit -m "feat(hash-trie): region runs on a presized table (#94)

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
  ```

- [ ] **Step 6: Mutation checks.**

| # | Exact edit | Must fail |
|---|---|---|
| R2 | In `with_runs`, `self.len += inserted.iter().sum::<usize>();` → delete the line | `a_run_finds_a_key_or_inserts_at_the_first_empty_bucket` (len) |
| R3 | In `home_bucket`, `>> (64 - log2_capacity)` → `>> (63 - log2_capacity)` | `home_bucket_matches_bucket_index` |

Bounding probes by the *run's* end instead of the region's (mutant R1) is
checked in Task 3. Occupancy can't catch it, since any insertion order
gives the same occupied buckets; what it breaks is N-independence.

---

## Task 2 [P1]: The root step, and the equivalence assertions

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs`. Add
  `insert_at_leaf_root_in_run` and `insert_at_inner_root_in_run` directly
  after `insert_at`.
- Modify: `kermit-ds/src/ds/hash_trie/identity.rs`. Add
  `assert_equivalent_root` and `assert_equivalent_trie`.
- Test: `kermit-ds/src/ds/hash_trie/parallel.rs` tests (the root step's
  one-run test).

- [ ] **Step 1: The equivalence assertions** (test-only), appended to
  `identity.rs`:

```rust
/// The equivalence Amendment 2 asks of a build that may place root keys in
/// other buckets. The root has the same capacity, allocation and length,
/// occupies the same buckets, and has the same total probe displacement.
/// Every key's child or chain is array-identical.
pub(super) fn assert_equivalent_root<P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrieNode<P, E>, b: &HashTrieNode<P, E>, label: &str,
) {
    match (a, b) {
        | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
            assert_equivalent_table(x, y, label, &|x, y, path| assert_same_node(x, y, path))
        },
        | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
            assert_equivalent_table(x, y, label, &|x, y, path| assert_same_chain(x, y, path))
        },
        | _ => panic!("{label}: root variants differ"),
    }
}

fn assert_equivalent_table<V>(
    a: &HashTable<V>, b: &HashTable<V>, path: &str, same_value: &dyn Fn(&V, &V, &str),
) {
    assert_eq!(a.buckets_len(), b.buckets_len(), "{path}: capacity");
    assert_eq!(a.shell_heap_bytes(), b.shell_heap_bytes(), "{path}: bucket allocation");
    assert_eq!(a.len(), b.len(), "{path}: len");
    let cap = a.buckets_len();
    let log2 = cap.trailing_zeros();
    let occupied =
        |t: &HashTable<V>| (0..cap).filter(|&i| t.hash_at(i).is_some()).collect::<Vec<_>>();
    assert_eq!(occupied(a), occupied(b), "{path}: occupied buckets");
    let displacement = |t: &HashTable<V>| {
        (0..cap)
            .filter_map(|i| t.hash_at(i).map(|h| (i + cap - home_bucket(h, log2)) % cap))
            .sum::<usize>()
    };
    assert_eq!(displacement(a), displacement(b), "{path}: total displacement");
    for idx in 0..cap {
        if let (Some(hash), Some(x)) = (a.hash_at(idx), a.value_at(idx)) {
            let j = b
                .index_of(hash)
                .unwrap_or_else(|| panic!("{path}: hash {hash:#x} missing"));
            same_value(x, b.value_at(j).expect("index_of found it"), &format!("{path}/{hash:#x}"));
        }
    }
}

/// [`assert_equivalent_root`] for whole tries, plus heap size and tuple
/// count.
pub(super) fn assert_equivalent_trie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_equivalent_root(a.root(), b.root(), label);
    assert_eq!(a.heap_size_bytes(), b.heap_size_bytes(), "{label}: heap size");
    assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
}
```

  Add `hash_table::home_bucket` to identity.rs's `use super::{…}`.
  `assert_same_chain` must be callable here. It already is, being in the
  same file.

- [ ] **Step 2: Write the failing test for the root step.** One run covers
  the whole presized root, and the tail follows in input order. Against
  serial under `Tuples`, the result must be equivalent below the root at
  array level, for every Layout and input. This is the drift guard for the
  mirrored root step. Append to `parallel.rs`'s `mod tests` (using
  `make_root_sized`, `RootCapacity`, `HashTable` and `assert_equivalent_root`):

```rust
    /// The mirrored root step, alone: one run over the whole presized root,
    /// the overflow inserted afterwards in input order. Below the root it
    /// must build serial's trie exactly; the root is equivalent
    /// (Amendment 2). A drift between `insert_at`'s root level and its
    /// mirror shows here before any threading is involved.
    fn check_root_step<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for &percent in LOAD_PERCENTS {
                let config = HashTrieConfig {
                    load_factor: LoadFactor::percent(percent).unwrap(),
                    root_capacity: RootCapacity::Tuples,
                };
                for (input, tuples) in inputs(arity) {
                    let label = format!(
                        "{}/{}/{} arity {arity}, load {percent}%, {input}",
                        H::NAME,
                        P::NAME,
                        E::NAME
                    );
                    let serial = HashTrie::<H, P, E>::from_tuples_with_config(
                        arity.into(),
                        config,
                        tuples.clone(),
                    );
                    let log2 = config.root_log2_capacity(tuples.len());
                    let mut root = HashTrie::<H, P, E>::make_root_sized(arity, log2);
                    let lf = config.load_factor;
                    let tail: Vec<Vec<usize>> = match &mut root {
                        | HashTrieNode::Inner(table) => table.with_runs(1, 1 << log2, |mut runs| {
                            tuples
                                .iter()
                                .cloned()
                                .filter_map(|t| {
                                    HashTrie::<H, P, E>::insert_at_inner_root_in_run(
                                        &mut runs[0], arity, t, lf,
                                    )
                                    .err()
                                })
                                .collect()
                        }),
                        | HashTrieNode::Leaf(table) => table.with_runs(1, 1 << log2, |mut runs| {
                            tuples
                                .iter()
                                .cloned()
                                .filter_map(|t| {
                                    HashTrie::<H, P, E>::insert_at_leaf_root_in_run(&mut runs[0], t)
                                        .err()
                                })
                                .collect()
                        }),
                        | _ => unreachable!("a root is never pruned or unexpanded"),
                    };
                    for t in tail {
                        HashTrie::<H, P, E>::insert_at(&mut root, 0, arity, t, lf);
                    }
                    assert_equivalent_root(serial.root(), &root, &label);
                }
            }
        }
    }

    #[test]
    fn the_root_step_builds_the_serial_trie_below_the_root() {
        check_root_step::<SipHashStrategy, NoPruning, EagerExpansion>();
        check_root_step::<SipHashStrategy, SingletonPruning, EagerExpansion>();
        check_root_step::<SipHashStrategy, NoPruning, LazyExpansion>();
        check_root_step::<SipHashStrategy, SingletonPruning, LazyExpansion>();
        check_root_step::<FxHashStrategy, SingletonPruning, LazyExpansion>();
        check_root_step::<Mod10HashStrategy, SingletonPruning, EagerExpansion>();
    }
```

  Add `node::HashTrieNode`, `config::RootCapacity` and
  `identity::assert_equivalent_root` to the test module's `use`. (`HashTable`
  is reached through `HashTrieNode`'s variants.)

- [ ] **Step 3: Run it.**

  Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib the_root_step`

  Expected: a compile failure, because `insert_at_inner_root_in_run` and
  `insert_at_leaf_root_in_run` don't exist yet.

- [ ] **Step 4: Implement the root step** in `implementation.rs`, directly
  after `insert_at`'s closing brace, in the same `impl` block. Add
  `hash_table::{BucketRun, RunEntry}` to the file's `super::{…}` import.

```rust
    /// [`insert_at`](Self::insert_at)'s `Leaf` arm at the root (arity 1),
    /// inside one [`BucketRun`] of a presized root: the presized parallel
    /// build's root step (`parallel.rs`). Returns the tuple if its key's
    /// probe ran off its region, for the caller to insert afterwards.
    pub(super) fn insert_at_leaf_root_in_run(
        run: &mut BucketRun<'_, Vec<Vec<usize>>>, tuple: Vec<usize>,
    ) -> Result<(), Vec<usize>> {
        let hash = H::hash(tuple[0]);
        match run.entry(hash) {
            | Ok(RunEntry::Occupied(chain)) => chain.push(tuple),
            | Ok(RunEntry::Vacant(slot)) => slot.insert(Vec::new()).push(tuple),
            | Err(_) => return Err(tuple),
        }
        Ok(())
    }

    /// [`insert_at`](Self::insert_at)'s `Inner` arm at the root (arity
    /// ≥ 2), inside one [`BucketRun`] of a presized root: the presized
    /// parallel build's root step. It mirrors `insert_at` decision for
    /// decision. A fresh key becomes a `Singleton` (pruning), an
    /// `Unexpanded` child (lazy), or a new table that `insert_at` descends
    /// into. An existing key unprunes, appends to its pending list, or
    /// descends. Every level below the root is `insert_at`, unchanged.
    /// `the_root_step_builds_the_serial_trie_below_the_root` guards the
    /// mirror. Returns the tuple if its key's probe ran off its region.
    pub(super) fn insert_at_inner_root_in_run(
        run: &mut BucketRun<'_, HashTrieNode<P, E>>, arity: usize, tuple: Vec<usize>,
        load_factor: LoadFactor,
    ) -> Result<(), Vec<usize>> {
        let hash = H::hash(tuple[0]);
        let child_is_leaf = Self::is_leaf_depth(1, arity);
        let child = match run.entry(hash) {
            | Err(_) => return Err(tuple),
            | Ok(RunEntry::Occupied(child)) => child,
            | Ok(RunEntry::Vacant(slot)) => {
                if P::ENABLED {
                    slot.insert(HashTrieNode::Singleton(P::Payload::from_tuple(tuple)));
                    return Ok(());
                }
                if E::LAZY {
                    slot.insert(HashTrieNode::Unexpanded(E::Pending::from_tuples(vec![tuple])));
                    return Ok(());
                }
                slot.insert(HashTrieNode::new_table(child_is_leaf))
            },
        };
        if E::LAZY {
            match child {
                | HashTrieNode::Unexpanded(pending) => match pending.built_mut() {
                    | Some(built) => Self::insert_at(built, 1, arity, tuple, load_factor),
                    | None => pending.push(tuple),
                },
                | HashTrieNode::Singleton(_) => {
                    let list =
                        HashTrieNode::Unexpanded(E::Pending::from_tuples(Vec::with_capacity(2)));
                    let HashTrieNode::Singleton(evicted) = std::mem::replace(child, list) else {
                        unreachable!("matched Singleton above")
                    };
                    let HashTrieNode::Unexpanded(pending) = child else {
                        unreachable!("replaced by an Unexpanded child just above")
                    };
                    pending.push(evicted.into_tuple());
                    pending.push(tuple);
                },
                | HashTrieNode::Inner(_) | HashTrieNode::Leaf(_) => {
                    unreachable!("a lazy Inner bucket holds a Singleton or an Unexpanded child")
                },
            }
            return Ok(());
        }
        if P::ENABLED && matches!(child, HashTrieNode::Singleton(_)) {
            let replacement = HashTrieNode::new_table(child_is_leaf);
            let HashTrieNode::Singleton(evicted) = std::mem::replace(child, replacement) else {
                unreachable!("matched Singleton above")
            };
            Self::insert_at(child, 1, arity, evicted.into_tuple(), load_factor);
        }
        Self::insert_at(child, 1, arity, tuple, load_factor);
        Ok(())
    }
```

- [ ] **Step 5: Run it.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib the_root_step
  RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit-ds --all-targets
  ```
  Expected: PASS, clippy clean.

- [ ] **Step 6: Commit.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/identity.rs kermit-ds/src/ds/hash_trie/parallel.rs
  git -C $WT commit -m "feat(hash-trie): insert_at's root level inside a region run (#94)

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
  ```

- [ ] **Step 7: Mutation checks.** Each must fail
  `the_root_step_builds_the_serial_trie_below_the_root`.

| # | Exact edit |
|---|---|
| S1 | In `insert_at_inner_root_in_run`'s lazy `Singleton` arm, swap `pending.push(evicted.into_tuple());` and `pending.push(tuple);` |
| S2 | `Vec::with_capacity(2)` → `Vec::new()` (a pending-list capacity differs) |
| S3 | In the eager unprune, delete `Self::insert_at(child, 1, arity, evicted.into_tuple(), load_factor);` |

---

## Task 3 [P1]: The presized parallel build

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/parallel.rs`. Add `REGION_BUCKETS`,
  `fill_presized_root`, `fill_presized_root_in` and `fill_runs`, and the
  `ParallelBuild` record.
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs`. The `Parallel` arm
  selects by `config.root_capacity`.
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs`, `kermit-ds/src/ds/mod.rs` and
  `kermit-ds/src/test_hooks.rs`: the record's type.

- [ ] **Step 1: Change the record to a struct.** In `parallel.rs`, replace the
  `PARALLEL_BUILDS` thread-local and `take_parallel_builds` with:

```rust
/// One parallel build, as the test hooks record it.
#[cfg(any(test, feature = "test-hooks"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParallelBuild {
    /// The `N` of `parallel:N`.
    pub threads: usize,
    /// The size of each partition, empty ones included.
    pub partition_sizes: Vec<usize>,
    /// `None` for the exact build (`root-capacity=grow`). For the presized
    /// build, the number of tuples deferred to the calling thread.
    pub deferred: Option<usize>,
}

#[cfg(any(test, feature = "test-hooks"))]
thread_local! {
    /// Every parallel build on this thread. Every build mode builds an
    /// equivalent trie, so only this record can tell a test which build
    /// ran and where it put its tuples. Other crates' tests read it through
    /// `test_hooks` (the `test-hooks` feature).
    static PARALLEL_BUILDS: std::cell::RefCell<Vec<ParallelBuild>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Takes this thread's record of parallel builds, oldest first, leaving it
/// empty.
#[cfg(feature = "test-hooks")]
pub(crate) fn take_parallel_builds() -> Vec<ParallelBuild> {
    PARALLEL_BUILDS.with(|builds| builds.take())
}
```

  In `fill_root_in_morsels`, the push becomes:

```rust
        builds.borrow_mut().push(ParallelBuild {
            threads: threads.get(),
            partition_sizes: sizes,
            deferred: None,
        });
```

  In the tests:
  - `build_modes_reach_their_builds` compares against
    `ParallelBuild { threads: 2, partition_sizes: vec![8; 8], deferred: None }`
    and `ParallelBuild { threads: 3, partition_sizes: [8, 0].repeat(8),
    deferred: None }`, instead of the tuples.
  - `PARALLEL_BUILDS.with(|b| b.take())` keeps its shape.

  Re-exports:
  - `hash_trie/mod.rs`: `#[cfg(feature = "test-hooks")] pub(crate) use
    parallel::{take_parallel_builds, ParallelBuild};`
  - `ds/mod.rs`: `#[cfg(feature = "test-hooks")] pub(crate) use
    hash_trie::{take_parallel_builds as take_hash_trie_parallel_builds,
    ParallelBuild as HashTrieParallelBuild};`
  - `test_hooks.rs`: `take_hash_trie_parallel_builds` returns
    `Vec<HashTrieParallelBuild>`, and add `pub use
    crate::ds::HashTrieParallelBuild;`. Its doc says the struct records the
    thread count, every partition's size, and the deferred count of a
    presized build.

- [ ] **Step 2: Write the failing tests.** Append to `parallel.rs`'s `mod
  tests`:

```rust
    /// Thread counts for the presized matrix; Miri runs two.
    const PRESIZED_THREADS: &[usize] = if cfg!(miri) { &[2] } else { &[1, 2, 3, 8] };

    /// Load factors under test, 80 % (the paper's sizing) included.
    const PRESIZED_LOAD_PERCENTS: &[u8] = if cfg!(miri) { &[70] } else { &[50, 70, 80, 95] };

    fn tuples_config(percent: u8) -> HashTrieConfig {
        HashTrieConfig {
            load_factor: LoadFactor::percent(percent).unwrap(),
            root_capacity: RootCapacity::Tuples,
        }
    }

    /// The presized build through the public constructor, and through the
    /// test entry with morsels of 7 and 8-bucket regions, so small inputs
    /// span many regions and overflow. Both must be equivalent to serial
    /// under `Tuples` (Amendment 2), and identical to their own `parallel:1`
    /// for every N.
    fn check_presized<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
        inputs: &dyn Fn(usize) -> Vec<(&'static str, Vec<Vec<usize>>)>,
        arities: std::ops::RangeInclusive<usize>,
    ) {
        for arity in arities {
            for &percent in PRESIZED_LOAD_PERCENTS {
                let config = tuples_config(percent);
                for (input, tuples) in inputs(arity) {
                    let serial = HashTrie::<H, P, E>::from_tuples_with_config(
                        arity.into(),
                        config,
                        tuples.clone(),
                    );
                    let mut first: Option<(HashTrie<H, P, E>, HashTrieNode<P, E>)> = None;
                    for &t in PRESIZED_THREADS {
                        let label = format!(
                            "{}/{}/{} arity {arity}, load {percent}%, parallel:{t}, {input}",
                            H::NAME,
                            P::NAME,
                            E::NAME
                        );
                        let built = HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
                            arity.into(),
                            config,
                            HashTrieBuildMode::Parallel(threads(t)),
                            tuples.clone(),
                        );
                        assert_equivalent_trie(&serial, &built, &label);
                        let log2 = config.root_log2_capacity(tuples.len());
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
                        assert_equivalent_root(serial.root(), &small, &format!("{label}, small regions"));
                        match &first {
                            | None => first = Some((built, small)),
                            | Some((built_1, small_1)) => {
                                assert_same_trie(built_1, &built, &format!("{label} vs parallel:1"));
                                assert_same_node(small_1, &small, &format!("{label} vs parallel:1, small regions"));
                            },
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn presized_parallel_builds_are_equivalent_under_siphash() {
        let shared = |arity: usize| inputs(arity);
        check_presized::<SipHashStrategy, NoPruning, EagerExpansion>(&shared, 1..=3);
        check_presized::<SipHashStrategy, SingletonPruning, EagerExpansion>(&shared, 1..=3);
        check_presized::<SipHashStrategy, NoPruning, LazyExpansion>(&shared, 1..=3);
        check_presized::<SipHashStrategy, SingletonPruning, LazyExpansion>(&shared, 1..=3);
    }

    #[test]
    #[cfg_attr(miri, ignore = "threads are slow under Miri; the SipHash matrix runs the same code")]
    fn presized_parallel_builds_are_equivalent_under_fxhash_and_colliding_hashes() {
        let shared = |arity: usize| inputs(arity);
        check_presized::<FxHashStrategy, NoPruning, EagerExpansion>(&shared, 1..=3);
        check_presized::<FxHashStrategy, SingletonPruning, LazyExpansion>(&shared, 1..=3);
        check_presized::<Mod10HashStrategy, NoPruning, EagerExpansion>(&shared, 1..=3);
        check_presized::<Mod10HashStrategy, SingletonPruning, LazyExpansion>(&shared, 1..=3);
    }

    /// Arity 4, a first key holding half the tuples, and three morsels at
    /// the real morsel size.
    #[test]
    #[cfg_attr(miri, ignore = "tens of thousands of inserts")]
    fn presized_parallel_builds_are_equivalent_on_large_and_skewed_inputs() {
        let large = |arity: usize| {
            let n = 2 * MORSEL_TUPLES + 7_000;
            let mut lcg = Lcg(0x94);
            let random: Vec<Vec<usize>> =
                (0..n).map(|_| (0..arity).map(|_| lcg.next_usize() % 5_000).collect()).collect();
            let skewed: Vec<Vec<usize>> = (0..n)
                .map(|i| {
                    let first = if i % 2 == 0 { 7 } else { lcg.next_usize() % 5_000 };
                    std::iter::once(first).chain((1..arity).map(|_| lcg.next_usize() % 50)).collect()
                })
                .collect();
            vec![("random", random), ("half one key", skewed)]
        };
        check_presized::<SipHashStrategy, NoPruning, EagerExpansion>(&large, 1..=4);
        check_presized::<FxHashStrategy, SingletonPruning, LazyExpansion>(&large, 1..=4);
    }

    /// Every key homes at bucket 7, the last of the first 8-bucket region of
    /// a 32-bucket root (16 tuples at 70 %): the first key takes bucket 7,
    /// and every later distinct key overflows to the tail.
    #[derive(Copy, Clone, Default, Debug)]
    struct EndOfRegionHash;

    impl LayoutOption for EndOfRegionHash {
        const NAME: &'static str = "end-of-region";
    }

    impl HashStrategy for EndOfRegionHash {
        fn hash(key: usize) -> u64 { super::super::hash_table::hash_with_home(7, 5, key as u64) }
    }

    #[test]
    fn keys_that_cannot_fit_their_region_go_to_the_tail() {
        let config = tuples_config(70);
        let tuples: Vec<Vec<usize>> = (0..16).map(|i| vec![i % 4, i]).collect();
        assert_eq!(config.root_log2_capacity(tuples.len()), 5, "a 32-bucket root");
        let serial: HashTrie<EndOfRegionHash> =
            HashTrie::from_tuples_with_config(2.into(), config, tuples.clone());
        let mut root = HashTrie::<EndOfRegionHash>::make_root_sized(2, 5);
        PARALLEL_BUILDS.with(|b| b.borrow_mut().clear());
        fill_presized_root_in::<EndOfRegionHash, NoPruning, EagerExpansion>(
            &mut root,
            2,
            tuples,
            threads(2),
            7,
            8,
            config.load_factor,
        );
        // Four distinct keys, each with four tuples: the first key's
        // tuples stay in the region, the other three keys' twelve go to
        // the tail.
        let builds = PARALLEL_BUILDS.with(|b| b.take());
        assert_eq!(builds.len(), 1);
        assert_eq!(builds[0].deferred, Some(12));
        assert_equivalent_root(serial.root(), &root, "end of region");
    }

    /// The presized build reaches its own path, at the thread count asked:
    /// two worker runs (scatter, then the regions), and a record with a
    /// deferred count.
    #[test]
    fn root_capacity_selects_the_parallel_path() {
        let tuples: Vec<Vec<usize>> = (0..64).map(|i| vec![i % 16, i]).collect();
        for (root_capacity, presized) in [(RootCapacity::Grow, false), (RootCapacity::Tuples, true)] {
            let config = HashTrieConfig { root_capacity, ..HashTrieConfig::default() };
            PARALLEL_BUILDS.with(|b| b.borrow_mut().clear());
            crate::morsel::take_worker_runs();
            let _: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
                2.into(),
                config,
                HashTrieBuildMode::Parallel(threads(3)),
                tuples.clone(),
            );
            let builds = PARALLEL_BUILDS.with(|b| b.take());
            assert_eq!(builds.len(), 1, "{root_capacity:?}");
            assert_eq!(builds[0].threads, 3, "{root_capacity:?}");
            assert_eq!(builds[0].deferred.is_some(), presized, "{root_capacity:?}");
            assert_eq!(builds[0].partition_sizes.iter().sum::<usize>(), 64, "{root_capacity:?}");
            assert_eq!(crate::morsel::take_worker_runs(), vec![3, 3], "{root_capacity:?}");
        }
    }
```

  Add `identity::assert_equivalent_trie`, `crate::test_support::Lcg` (if
  not already in scope) and `kermit_iters::LayoutOption` (already imported
  by `TopBitsHash`) to the test module's `use`.

- [ ] **Step 3: Run them.**

  Run: `CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib hash_trie::parallel`

  Expected: a compile failure, because `fill_presized_root_in` is
  undefined.

- [ ] **Step 4: Implement the presized path** in `parallel.rs`, after
  `merge_in_first_appearance_order`. Add `hash_table::{home_bucket,
  BucketRun}` and `implementation::HashTrie` to the `super::{…}` import, and
  `Positioned` to the `crate::morsel` import.

```rust
/// Buckets per region of a presized root. A probe during the parallel fill
/// never crosses a region's end, and a key whose probe would is deferred to
/// the calling thread. Regions, not partitions, decide that, so the layout
/// is the same for every thread count. 4096 buckets keep the deferred share
/// small at load factors up to 0.95, and keep a region's buckets within a
/// worker's cache.
const REGION_BUCKETS: usize = 4096;

/// Fills the presized, empty `root` with `tuples` by the `parallel:threads`
/// build of a presized root: the paper's partitioned build
/// (`docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`).
/// Every tuple must have `arity` attributes; the caller checks.
pub(super) fn fill_presized_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    root: &mut HashTrieNode<P, E>, arity: usize, tuples: Vec<Vec<usize>>, threads: Threads,
    load_factor: LoadFactor,
) {
    fill_presized_root_in::<H, P, E>(
        root,
        arity,
        tuples,
        threads,
        MORSEL_TUPLES,
        REGION_BUCKETS,
        load_factor,
    );
}

/// [`fill_presized_root`] with the morsel and region sizes as parameters, so
/// tests can cut a small input into many morsels and regions.
///
/// 1. **Partition**: `scatter` by the top bits of each tuple's home bucket,
///    so partition k is a contiguous run of regions, in input order.
/// 2. **Fill**: each worker takes a partition with its run of the root and
///    inserts every tuple once, through the root step; tuples whose key's
///    probe runs off its region are deferred.
/// 3. **Tail**: the calling thread inserts the deferred tuples in input
///    order, with ordinary probing. The paper does not say how a probe
///    crossing a partition's end is handled; this is kermit's answer.
fn fill_presized_root_in<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    root: &mut HashTrieNode<P, E>, arity: usize, tuples: Vec<Vec<usize>>, threads: Threads,
    morsel_tuples: usize, region_buckets: usize, load_factor: LoadFactor,
) {
    if tuples.is_empty() {
        return;
    }
    let capacity = match root {
        | HashTrieNode::Inner(table) => table.buckets_len(),
        | HashTrieNode::Leaf(table) => table.buckets_len(),
        | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
            unreachable!("a root is never pruned or unexpanded")
        },
    };
    let log2 = capacity.trailing_zeros();
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
    // 2. Fill each run of regions in parallel.
    let mut deferred = match root {
        | HashTrieNode::Inner(table) => {
            fill_runs(table, threads, partitions, region_buckets, |run, tuple| {
                HashTrie::<H, P, E>::insert_at_inner_root_in_run(run, arity, tuple, load_factor)
            })
        },
        | HashTrieNode::Leaf(table) => {
            fill_runs(table, threads, partitions, region_buckets, |run, tuple| {
                HashTrie::<H, P, E>::insert_at_leaf_root_in_run(run, tuple)
            })
        },
        | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
            unreachable!("a root is never pruned or unexpanded")
        },
    };
    #[cfg(any(test, feature = "test-hooks"))]
    PARALLEL_BUILDS.with(|builds| {
        builds.borrow_mut().push(ParallelBuild {
            threads: threads.get(),
            partition_sizes,
            deferred: Some(deferred.len()),
        });
    });
    // 3. The tail, in input order. The root is presized for every tuple, so
    //    it does not grow here.
    deferred.sort_unstable_by_key(|&(position, _)| position);
    for (_, tuple) in deferred {
        HashTrie::<H, P, E>::insert_at(root, 0, arity, tuple, load_factor);
    }
}

/// Lends `table`'s runs to the workers: partition k fills run k through
/// `step`, in input order, and the tuples `step` hands back are returned
/// with their input positions.
fn fill_runs<V: Send>(
    table: &mut HashTable<V>, threads: Threads, partitions: Vec<Partition>,
    region_buckets: usize,
    step: impl Fn(&mut BucketRun<'_, V>, Vec<usize>) -> Result<(), Vec<usize>> + Sync,
) -> Vec<Positioned> {
    let parts = partitions.len();
    table.with_runs(parts, region_buckets, |runs| {
        let work: Vec<_> = partitions.into_iter().zip(runs).collect();
        dispatch(threads, work, |(partition, mut run)| {
            let mut deferred = Vec::new();
            for (position, tuple) in partition.into_tuples() {
                if let Err(tuple) = step(&mut run, tuple) {
                    deferred.push((position, tuple));
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

  In `implementation.rs`, the `Parallel` arm becomes:

```rust
            | HashTrieBuildMode::Parallel(threads) => {
                Self::from_tuples_partitioned(header, config, tuples, |root, arity, tuples| {
                    match config.root_capacity {
                        // The root's size depends on the distinct keys, so
                        // it is filled after them, in first-appearance order.
                        | RootCapacity::Grow => parallel::fill_root::<H, P, E>(
                            root,
                            arity,
                            tuples,
                            threads,
                            config.load_factor,
                        ),
                        // The root is presized (#88): the paper's build.
                        | RootCapacity::Tuples => parallel::fill_presized_root::<H, P, E>(
                            root,
                            arity,
                            tuples,
                            threads,
                            config.load_factor,
                        ),
                    }
                })
            },
```

  Add `config::RootCapacity` to `implementation.rs`'s import if it isn't
  there already.

- [ ] **Step 5: Run them.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds --lib hash_trie
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}'
  RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit-ds --all-targets
  RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit-ds --all-targets --features test-hooks
  ```
  Expected: every test passes, with 0 failed overall, and clippy is clean
  both ways. `keys_that_cannot_fit_their_region_go_to_the_tail` reports
  `deferred == Some(12)`. If the count differs, report the observed value
  rather than editing the test to match: it means the region logic is not
  what the design says.

- [ ] **Step 6: Miri on the new tests** (detached if slow).
  ```bash
  MIRIFLAGS="-Zmiri-disable-isolation" CARGO_BUILD_JOBS=2 nix develop $WT --command cargo miri test -p kermit-ds --lib hash_trie
  ```
  Expected: PASS, with the FxHash, Mod10 and large tests ignored. Report the
  wall time. If it exceeds 10 minutes, stop and report.

- [ ] **Step 7: Commit.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit-ds/src/ds/hash_trie/parallel.rs kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/mod.rs kermit-ds/src/ds/mod.rs kermit-ds/src/test_hooks.rs
  git -C $WT commit -m "feat(hash-trie): parallel:N fills a presized root by region, the paper's build (#94)

  Under root-capacity=tuples, partitions are contiguous runs of root
  regions and each worker inserts every tuple of its regions once; probes
  that cross a region's end are finished by the calling thread. The
  default config keeps the exact build.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
  ```

- [ ] **Step 8: Mutation checks.**

| # | Exact edit | Must fail |
|---|---|---|
| M1 | `deferred.sort_unstable_by_key(\|&(position, _)\| position);` → delete the line | `presized_parallel_builds_are_equivalent_under_siphash` (the N-independence or the below-root identity) |
| M2 | `home_bucket(H::hash(tuple[0]), log2) >> shift` → `(H::hash(tuple[0]) >> (64 - parts.trailing_zeros())) as usize` (raw hash bits: a tuple lands in a run that doesn't hold its home) | a debug assertion in `BucketRun::entry`, or the equivalence tests |
| M3 | In the `Parallel` arm, `RootCapacity::Tuples => parallel::fill_presized_root` → `parallel::fill_root` | `root_capacity_selects_the_parallel_path` |
| M4 | `.min(capacity / region_buckets)` → delete (more runs than regions) | a `with_runs` panic in the small-input tests |
| R1 | In `BucketRun::entry`, `let region_end = (home / self.region_buckets + 1) * self.region_buckets;` → `let region_end = self.first + self.buckets.len();` (probes bounded by the run, whose size depends on N) | `presized_parallel_builds_are_equivalent_under_siphash`, on "vs parallel:1, small regions" |

---

## Task 4 [P1]: Stack `BuiltWith` on `Configured`, and the structure suites

**Files:**
- Modify: `kermit-ds/src/relation.rs` (the new trait).
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (its impl).
- Modify: `kermit-ds/src/configured.rs` (`BuildModeRelation` for
  `Configured`).
- Modify: `kermit-ds/src/lib.rs` (export).
- Modify: `kermit-ds/tests/hash_trie_tests.rs` and
  `kermit-ds/tests/parquet_tests.rs`.

- [ ] **Step 1: The trait** (`relation.rs`, after `BuildModeRelation`):

```rust
/// A relation with both a Config and a BuildMode, and the one constructor
/// that takes both, so the two test markers can stack:
/// `BuiltWith<Configured<R, C>, M>` builds by `M`'s mode under `C`'s config.
pub trait ConfiguredBuildModeRelation: ConfigurableRelation + BuildModeRelation {
    /// Builds `tuples` by `mode` under `config`. Same contract as
    /// [`BuildModeRelation::from_tuples_with_build_mode`].
    fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: Self::Config, mode: Self::BuildMode,
        tuples: Vec<Vec<usize>>,
    ) -> Self;
}
```

  In `implementation.rs` (the inherent method of the same name is the body;
  a path call resolves to it, not to the trait method):

```rust
impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> ConfiguredBuildModeRelation
    for HashTrie<H, P, E>
{
    fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: HashTrieConfig, mode: HashTrieBuildMode,
        tuples: Vec<Vec<usize>>,
    ) -> Self {
        HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(header, config, mode, tuples)
    }
}
```

  If calling it this way fails to resolve because the inherent and trait
  methods clash, rename the inherent method's call site to
  `Self::from_tuples_with_config_and_build_mode` inside an inherent helper,
  and report the change.

  In `configured.rs`:

```rust
/// `Configured` under [`BuiltWith`](crate::BuiltWith): the mode comes from
/// the outer marker, the config from `P`, so both reach the relation's one
/// constructor that takes both.
impl<R, P> BuildModeRelation for Configured<R, P>
where
    R: ConfiguredBuildModeRelation,
    P: ConfigProvider<R::Config>,
{
    type BuildMode = R::BuildMode;

    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: R::BuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self {
        Self::wrap(R::from_tuples_with_config_and_build_mode(header, P::config(), mode, tuples))
    }
}
```

  Export `ConfiguredBuildModeRelation` from `lib.rs`'s `relation::{…}`
  re-export.

- [ ] **Step 2: Unit test** in `configured.rs`'s tests:

```rust
    crate::define_config_provider!(Presized, HashTrieConfig, HashTrieConfig {
        root_capacity: crate::ds::RootCapacity::Tuples,
        ..HashTrieConfig::default()
    });
    crate::define_build_mode_provider!(
        TwoThreads,
        crate::ds::HashTrieBuildMode,
        crate::ds::HashTrieBuildMode::Parallel(crate::Threads::new(2).unwrap())
    );

    /// The two markers stack: the config reaches the relation, and so does
    /// the build mode (the presized path presizes the root).
    #[test]
    fn built_with_stacks_on_configured() {
        type Stacked = crate::BuiltWith<Configured<HashTrie<SipHashStrategy>, Presized>, TwoThreads>;
        let r = Stacked::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(r.config().root_capacity, crate::ds::RootCapacity::Tuples);
        let mut tuples = r.collect_tuples();
        tuples.sort();
        assert_eq!(tuples, vec![vec![1, 2], vec![3, 4]]);
    }
```

  If `RootCapacity` is not re-exported from `crate::ds`, use the path that
  #88 exports.

- [ ] **Step 3: Structure suites.** `hash_trie_tests.rs`, after the
  `HashParallel2` block (add `Configured`, `define_config_provider` and
  `RootCapacity` to its imports if they're missing):

```rust
// ── BuildMode × Config: the presized parallel build ─────────────────────
// Under `root-capacity=tuples` (#88), `parallel:2` fills the root by region
// (the paper's build). The trie is equivalent to serial's (Amendment 2), so
// the iterator contract holds unchanged.
define_config_provider!(PresizedRoot, HashTrieConfig, HashTrieConfig {
    root_capacity: RootCapacity::Tuples,
    ..HashTrieConfig::default()
});

type HashTrieSipPresized = Configured<HashTrieSip, PresizedRoot>;
type HashTrieSipLazyPresized = Configured<HashTrieSipLazy, PresizedRoot>;
type HashTrieFxPrunedPresized = Configured<HashTrieFxPruned, PresizedRoot>;
type HashTrieSipPresizedParallel2 = BuiltWith<HashTrieSipPresized, HashParallel2>;
type HashTrieSipLazyPresizedParallel2 = BuiltWith<HashTrieSipLazyPresized, HashParallel2>;
type HashTrieFxPrunedPresizedParallel2 = BuiltWith<HashTrieFxPrunedPresized, HashParallel2>;

hash_trie_test_suite!(HashTrieSipPresizedParallel2, SipHashStrategy);

hash_trie_test_suite!(HashTrieSipLazyPresizedParallel2, SipHashStrategy);

hash_trie_test_suite!(HashTrieFxPrunedPresizedParallel2, FxHashStrategy);
```

  If #88 already defines a provider for `RootCapacity::Tuples` in this file,
  use it in place of `PresizedRoot`.

  In `parquet_tests.rs`, after the `HashParallel2` block:

```rust
// …and the presized parallel build under root-capacity=tuples (#94, #88).
define_config_provider!(PresizedRoot, HashTrieConfig, HashTrieConfig {
    root_capacity: RootCapacity::Tuples,
    ..HashTrieConfig::default()
});

type HashTrieSipPresizedParallel2 = BuiltWith<Configured<HashTrieSip, PresizedRoot>, HashParallel2>;

fn sorted_tuples_presized(relation: &HashTrieSipPresizedParallel2) -> Vec<Vec<usize>> {
    // Two derefs: `BuiltWith` → `Configured` → `HashTrie`.
    sorted_tuples(relation)
}

parquet_test_suite!(HashTrieSipPresizedParallel2, sorted_tuples_presized);
```

- [ ] **Step 4: Run them.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit-ds 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}'
  RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy -p kermit-ds --all-targets
  ```
  Expected: 0 failed; the presized suites appear; clippy is clean.

- [ ] **Step 5: Commit.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit-ds/src/relation.rs kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/configured.rs kermit-ds/src/lib.rs kermit-ds/tests/hash_trie_tests.rs kermit-ds/tests/parquet_tests.rs
  git -C $WT commit -m "test(hash-trie): the presized parallel build through the structure suites (#94)

  ConfiguredBuildModeRelation lets BuiltWith stack on Configured.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
  ```

**Checkpoint 1 (controller → user):** report the SHAs, the test delta, the
Miri time, and the mutation outcomes for R1–R3, S1–S3 and M1–M4. Then merge
`origin/master`.

---

# P2 — `kermit` binary

## Task 5 [P2]: Route, report and CLI tests

**Files:**
- Modify: `kermit/src/execution.rs` (tests).
- Modify: `kermit/tests/cli_hash_trie_build_mode.rs`.

- [ ] **Step 1: The record's new shape, and a presized route.**

  In `hash_trie_families_build_with_their_parallel_mode`:
  - The `parallel_builds` closure maps `.map(|build| build.threads)`.
  - Wrap the mode loop in `for config in [HashTrieConfig::default(),
    HashTrieConfig { root_capacity: RootCapacity::Tuples,
    ..HashTrieConfig::default() }]`.
  - Assert, after each route, that the presized config's records carry
    `deferred: Some(_)` and the default's carry `None`. Read the record once
    per route:

  ```rust
            let parallel_builds = |build: &dyn Fn()| {
                kermit_ds::test_hooks::take_hash_trie_parallel_builds();
                build();
                kermit_ds::test_hooks::take_hash_trie_parallel_builds()
            };
            // …
                for (route, build) in routes {
                    let builds = parallel_builds(build);
                    let threads: Vec<usize> = builds.iter().map(|b| b.threads).collect();
                    assert_eq!(threads, expected, "{} {route} under {mode:?}, {config:?}", E::NAME);
                    assert!(
                        builds.iter().all(|b| b.deferred.is_some() == presized),
                        "{} {route} under {mode:?}, {config:?}: wrong path",
                        E::NAME
                    );
                }
  ```

  Here `presized` is `config.root_capacity == RootCapacity::Tuples`. Import
  `kermit_ds::RootCapacity` (or #88's path).

- [ ] **Step 2: CLI tests.** Append to `cli_hash_trie_build_mode.rs`:

```rust
/// Under `root-capacity=tuples`, `parallel:N` is the paper's presized
/// build. One report records both the config and the mode.
#[test]
fn cli_bench_ds_records_a_presized_parallel_build() {
    let (output, report) = bench_ds("hash-trie", &[
        "--ds-config",
        "root-capacity=tuples",
        "--ds-build",
        "hash-trie=parallel:2",
    ]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let axes = axes_of(&report);
    assert_eq!(axes["ds_build_mode"], "parallel:2", "{axes}");
    assert_eq!(axes["ds_config_root_capacity"], "tuples", "{axes}");
}

/// `--verify` checks the answers of presized parallel builds, eager and
/// lazy, at the paper's load factor.
#[test]
fn cli_bench_run_verifies_presized_parallel_builds() {
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
            "--ds-config",
            "root-capacity=tuples,load-factor=0.8",
            "--ds-build",
            "hash-trie=parallel:3",
        ]);
        assert!(
            output.status.success(),
            "{expansion}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let axes = axes_of(&report);
        assert_eq!(axes["verified"], true, "{expansion}: {axes}");
        assert_eq!(axes["ds_config_root_capacity"], "tuples", "{expansion}: {axes}");
    }
}
```

- [ ] **Step 3: Run them.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --bin kermit -- execution:: 2>&1 | grep -E '^test result|FAILED'
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test cli_hash_trie_build_mode 2>&1 | grep -E '^test result|FAILED'
  ```
  Expected: PASS. The binary needs no production change: `HashHtj` hands
  the config and the mode to `from_tuples_with_config_and_build_mode`. If a
  route fails, stop and report it.

- [ ] **Step 4: Commit.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit/src/execution.rs kermit/tests/cli_hash_trie_build_mode.rs
  git -C $WT commit -m "test: root-capacity=tuples reaches the presized parallel build on every route (#94)

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
  ```

- [ ] **Step 5: Mutation check.** In `kermit-ds`'s `Parallel` arm, map
  `RootCapacity::Tuples` to `parallel::fill_root`. The route test must fail
  with "wrong path".

## Task 6 [P2]: The join suites

**Files:**
- Modify: `kermit/tests/common/utils.rs` (a `JoinEntry` impl).
- Modify: `kermit/tests/join_tests.rs`.

- [ ] **Step 1: `JoinEntry` for the stacked wrapper.** In `utils.rs`, after
  the `BuiltWith<HashTrie<…>, B>` impl:

```rust
impl<
        H: HashStrategy,
        P: PruningPolicy,
        E: ExpansionPolicy,
        C: ConfigProvider<HashTrieConfig>,
        B: BuildModeProvider<HashTrieBuildMode>,
    > JoinEntry<BuiltWith<Configured<HashTrie<H, P, E>, C>, B>> for HashTriejoin
{
    fn database(
        relations: BTreeMap<String, BuiltWith<Configured<HashTrie<H, P, E>, C>, B>>,
        level: StatisticsLevel,
    ) -> Database<BuiltWith<Configured<HashTrie<H, P, E>, C>, B>> {
        Database::new::<HashFamily<H>>(relations, level)
    }

    fn join(
        database: &Database<BuiltWith<Configured<HashTrie<H, P, E>, C>, B>>, query: JoinQuery,
        planner: &Planner,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        hash_join::<BuiltWith<Configured<HashTrie<H, P, E>, C>, B>, H>(database, query, planner)
    }

    fn count(
        database: &Database<BuiltWith<Configured<HashTrie<H, P, E>, C>, B>>, query: JoinQuery,
        planner: &Planner,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        hash_join_for_each::<BuiltWith<Configured<HashTrie<H, P, E>, C>, B>, H>(
            database,
            query,
            planner,
            |_| rows += 1,
        )?;
        Ok(rows)
    }
}
```

- [ ] **Step 2: The invocations.** In `join_tests.rs`, after the
  `HashParallel2` rows (add `Configured`, `define_config_provider` and
  `RootCapacity` to its imports, or reuse #88's provider for
  `RootCapacity::Tuples` if it defines one):

```rust
// ── BuildMode × Config: the presized parallel build ─────────────────────
// Under root-capacity=tuples (#88), parallel:2 is the paper's partitioned
// build (#94). The same three Layouts, under every optimiser.
define_config_provider!(PresizedRoot, HashTrieConfig, HashTrieConfig {
    root_capacity: RootCapacity::Tuples,
    ..HashTrieConfig::default()
});
type HashTrieSipPresized = Configured<HashTrieSip, PresizedRoot>;
type HashTrieFxPrunedPresized = Configured<HashTrieFxPruned, PresizedRoot>;
type HashTrieSipPrunedLazyPresized = Configured<HashTrieSipPrunedLazy, PresizedRoot>;
define_multiway_join_test_suite_for_build_mode!(HashTrieSipPresized, HashTriejoin, LexicographicOptimiser, HashParallel2);
define_multiway_join_test_suite_for_build_mode!(HashTrieSipPresized, HashTriejoin, CardinalityOptimiser, HashParallel2);
define_multiway_join_test_suite_for_build_mode!(HashTrieSipPresized, HashTriejoin, CostBasedOptimiser, HashParallel2);
define_multiway_join_test_suite_for_build_mode!(HashTrieFxPrunedPresized, HashTriejoin, LexicographicOptimiser, HashParallel2);
define_multiway_join_test_suite_for_build_mode!(HashTrieFxPrunedPresized, HashTriejoin, CardinalityOptimiser, HashParallel2);
define_multiway_join_test_suite_for_build_mode!(HashTrieFxPrunedPresized, HashTriejoin, CostBasedOptimiser, HashParallel2);
define_multiway_join_test_suite_for_build_mode!(HashTrieSipPrunedLazyPresized, HashTriejoin, LexicographicOptimiser, HashParallel2);
define_multiway_join_test_suite_for_build_mode!(HashTrieSipPrunedLazyPresized, HashTriejoin, CardinalityOptimiser, HashParallel2);
define_multiway_join_test_suite_for_build_mode!(HashTrieSipPrunedLazyPresized, HashTriejoin, CostBasedOptimiser, HashParallel2);
```

  `cargo fmt` reflows these. In the column-orders block, add the alias
  `type HashTrieSipPresizedParallel2 = BuiltWith<HashTrieSipPresized,
  HashParallel2>;` and three rows: `HashTrieSipPresizedParallel2,
  HashTriejoin, {Lexicographic,Cardinality,CostBased}Optimiser, AnyOrders`.

  If the column-orders harness (`join_under_planner`) needs a trait the
  stacked type lacks, compile errors will name it. Add the forwarding impl
  to `BuiltWith`/`Configured` (only forwarding), and report it.

- [ ] **Step 3: Run them.**
  ```bash
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests presized 2>&1 | grep -E '^test result'
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test -p kermit --test join_tests 2>&1 | grep -E '^test result'
  ```
  Expected: 192 new tests (9 × 16 + 3 × 16), and 0 failed.

- [ ] **Step 4: Commit.**
  ```bash
  nix develop $WT --command cargo fmt --all
  git -C $WT add kermit/tests/common/utils.rs kermit/tests/join_tests.rs
  git -C $WT commit -m "test: the presized parallel build through the join suites (#94)

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
  ```

  Then the controller merges `origin/master`.

---

# P3 — docs

## Task 7 [P3]: Document the presized build

**Files:**
- Modify: `docs/data-structures/parallel-build.md` (§ HashTrie).
- Modify: `docs/data-structures/hash-trie.md` (§ Build modes).
- Modify: `docs/specs/optimization-standard.md`.
- Modify: `docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`
  (status).
- Modify: `CLAUDE.md`.

- [ ] **Step 1: `parallel-build.md` § HashTrie.**
  1. Its intro says HashTrie has two parallel paths, chosen by `--ds-config
     root-capacity`. Under `grow` (the default) it is the exact build
     already described. Under `tuples` it is the presized build below.
  2. Rename the existing subsections "The three steps (exact build)",
     "Invariant (exact build)", "Complexity (exact build)" and "Worked
     micro-example (exact build)".
  3. Add a `### The presized build (root-capacity=tuples)` subsection: the
     spec's data-flow pseudocode, then its "Why it is correct" bullets, then
     this worked example:

```markdown
**Worked example.** `parallel:2` over `[1,a] [2,b] [3,c] [1,d]` (positions
0–3) under `root-capacity=tuples`: n = 4 at 70 % gives an 8-bucket root.
Real regions are 4096 buckets; for the example, take 4-bucket regions, so
there are 2 regions and 2 runs. Say the keys' home buckets are 3 for key 1,
3 for key 2, and 5 for key 3.

| Run | Tuples (position) | Step | Bucket |
|---|---|---|---|
| 0 (buckets 0–3) | `[1,a]`@0 | key 1 is new: its child table, then `a` | 3 |
| 0 | `[2,b]`@1 | key 2 probes 3 (taken), and 4 is past its region's end | deferred |
| 1 (buckets 4–7) | `[3,c]`@2 | key 3 is new | 5 |
| 0 | `[1,d]`@3 | key 1 is found at 3: `d` goes into its child | 3 |

The tail then inserts `[2,b]` by ordinary probing from its home, bucket 3.
Bucket 3 is taken and bucket 4 is free, so key 2 lands at 4. Serial would
have put key 1 at 3, key 2 at 4 and key 3 at 5: the same occupied buckets,
and here even the same slots. They differ only when a deferred key and a
later region's key compete for the same bucket.
```
  4. In "## Measuring", add that the presized curve compares presized
     `parallel:N` with presized `serial` (both under `root-capacity=tuples`),
     at load factors 0.8 and 0.7.

- [ ] **Step 2: `hash-trie.md` § Build modes.** The `Parallel(N)` row's
  Method cell gains: "under `root-capacity=tuples` (#88), the paper's
  build: partitions are root regions, one insert per tuple, a tail on the
  calling thread". Add a paragraph:

```markdown
**The presized parallel build.** Under `--ds-config root-capacity=tuples`,
`parallel:N` partitions the input into contiguous regions of the presized
root, and each worker inserts every tuple of its regions once (the paper's
§3.3.2 build). Keys whose probe would cross their region's end are finished
by the calling thread. The paper does not say how it handles that case, so
this is kermit's answer. The trie is equivalent to serial's (Amendment 2):
every subtrie and chain is array-identical; the root has the same capacity,
the same occupied buckets and the same total displacement; and it is the
same for every N. The closest-to-paper configuration is
`--ds-config root-capacity=tuples,load-factor=0.8`. Details:
[`parallel-build.md`](./parallel-build.md#hashtrie).
```

  Add the tests to the **Tests** bullet:
  - `presized_parallel_builds_are_equivalent_*`;
  - `keys_that_cannot_fit_their_region_go_to_the_tail`;
  - `the_root_step_builds_the_serial_trie_below_the_root`;
  - the `…PresizedParallel2` suite aliases.

- [ ] **Step 3: The standard.** In `optimization-standard.md`, append to the
  BuildMode "Output equivalence" cell: "A mode may choose its process by a
  Config value, as long as within each config it builds the equivalent of
  serial under that config (HashTrie's `parallel:N` is the exact build under
  `root-capacity=grow` and the presized build under `tuples`)."

- [ ] **Step 4: The spec's status.** Change it to: "Design approved
  (2026-10-06); implemented (this plan)".

- [ ] **Step 5: `CLAUDE.md`.**
  - In the BuildModeRelation bullet, after the `parallel:N` text, add:
    "under `root-capacity=tuples` it is the paper's presized build
    (partitions are root regions, a deferred tail)".
  - In the test-hooks gotcha, the HashTrie record is now a
    `HashTrieParallelBuild` struct (threads, partition sizes, deferred
    count).
  - In the Key Trait Hierarchy, add: "**ConfiguredBuildModeRelation**: both
    constructors at once, so `BuiltWith<Configured<R, C>, M>` stacks
    (HashTrie only)".

- [ ] **Step 6: Check and commit.**
  ```bash
  RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo doc --workspace --no-deps
  RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets
  git -C $WT diff --check
  git -C $WT add docs/data-structures/parallel-build.md docs/data-structures/hash-trie.md docs/specs/optimization-standard.md docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md CLAUDE.md
  git -C $WT commit -m "docs: the presized parallel build (#94)

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01ESTGZP5vr5nSGUND1ZjVky"
  ```

---

# Controller

## Task 8 [controller]: Gate, smoke run, checkpoint 2

- [ ] **Step 1: Scope diff.**
  ```bash
  BASE=$(cat $SCRATCH/base.sha)
  git -C $WT diff "$BASE" -- kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/hash_table.rs | grep -E '^-' | grep -v '^---'
  git -C $WT diff --stat "$BASE" -- kermit-ds/src/ds/tree_trie kermit-ds/src/ds/column_trie kermit-ds/src/ds/hash_trie/radix.rs kermit-algos
  ```
  Expected:
  - The removed lines are only the old `Parallel` arm and import lines.
  - No line of `insert_at`, `insert`, `from_tuples_with_config`,
    `expand_level`, `bucket_index`, `entry_or_insert_with` or `grow`.
  - The stat is empty.

- [ ] **Step 2: The CI gate.**
  ```bash
  nix develop $WT --command cargo fmt --all --check
  RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo clippy --all-targets
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo test --workspace 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {print p" passed, "f" failed, "i" ignored"}'
  RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 nix develop $WT --command cargo doc --workspace --no-deps
  MIRIFLAGS="-Zmiri-disable-isolation" CARGO_BUILD_JOBS=2 nix develop $WT --command cargo miri test -p kermit-ds   # detached: about 25 min
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build -p kermit
  KERMIT_BIN=$WT/target/debug/kermit uv --directory $WT/python/kermit-lab run pytest -q
  ```
  Expected: all green. Account for the test delta against
  `$SCRATCH/baseline.txt` by task.

- [ ] **Step 3: Smoke run** (on a quiet host; check `pgrep` first). It is the
  go/no-go for the presized build's purpose: unary relations must now
  scale.
  ```bash
  awk 'BEGIN { srand(7); print "a"; for (i = 0; i < 2000000; i++) print int(rand() * 2000000) }' > $SCRATCH/unary.csv
  CARGO_BUILD_JOBS=2 nix develop $WT --command cargo build --release -p kermit
  for cfg in "root-capacity=tuples,load-factor=0.8" "root-capacity=grow"; do
    for mode in serial parallel:8; do
      name="smoke-${cfg%%,*}-${mode/:/-}"; name="${name//=/-}"
      env -C $SCRATCH $WT/target/release/kermit bench --sample-size 10 --name "$name" \
        --report-json "$SCRATCH/$name.json" \
        ds --relation $SCRATCH/unary.csv -i hash-trie -m insertion --ds-config "$cfg" --ds-build "hash-trie=$mode"
    done
  done
  uv --directory $WT/python/kermit-lab run python -c "
  import kermit_lab as kl
  df = kl.load('$SCRATCH/smoke-root-capacity-*.json', criterion_root='$SCRATCH/target/criterion', apply_defaults=False)
  print(kl.speedup_table(df)[['ds_config_root_capacity', 'threads', 'speedup', 'karp_flatt']])
  "
  ```
  Expected:
  - Under `grow`, unary `parallel:8` is about 1× serial (today's
    limitation).
  - Under `tuples`, it is clearly above 1×.

  If it is not, stop and report: the bottleneck moved somewhere the design
  didn't predict.

- [ ] **Step 4: Checkpoint 2.** Report the SHAs, the gate, the test delta,
  the smoke table (labelled smoke), and any deviation.

## Task 9 [controller]: Hand-off (only on the user's word)

1. Fetch, merge `origin/master` (never rebase), and re-run Task 8 Step 2.
2. Push `HEAD:master` as the user directs.
3. Comment on #94 and #105 with this plan's commits, and tick #105's
   parallel-build row as "layer 1 landed; children still built
   incrementally (#107), tuples in file order (#101)".

---

## After this plan: the presized scaling curve

A separate step, using the 2026-10-06 run's scripts
(`/tb/Source/Academia/kermit-bench-runs/hash-trie-scaling-2026-10-06/`)
and one new binary:

- **Arms:** `serial` and `parallel:{1,2,4,8,16}`, both under
  `root-capacity=tuples`, at `load-factor=0.8` (parity with the paper) and
  0.7 (kermit's default). Same 12 relations, 5 rotated replicates.
- **Metrics:** `insertion`, plus `iteration` (Amendment 2: a mode that moves
  placement must have it measured).
- **Comparisons:**
  - presized `parallel:N` against presized `serial`;
  - against the default-config curve of 2026-10-06;
  - presized `serial` against default `serial` (#88's own effect).
- **The 2026-10-06 run's own rerun.sh** (3 contended steps) runs first, on a
  quiet host.
