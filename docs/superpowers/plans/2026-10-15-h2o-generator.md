# H2O Generator (#95, push 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add H2O, the H2O.ai db-benchmark join tasks (J1), as a `kind: h2o` generator benchmark. It ships with `bench gen h2o`, the committed `benchmarks/h2o.yml` and `benchmarks/h2o-sorted.yml` at N = 1e7, and the 1e7 acceptance measurements in `docs/benchmarks/H2O.md`.

**Architecture:**
- **Story 1, random numbers and keys (kermit-generate).** `h2o/rng.rs`: SplitMix64, Lemire's bounded draws, Fisher–Yates, and one stream per (column, step). `h2o/keys.rs`: `KeyPlan` and `KeyPool`, the four steps of key generation. Both are crate-private except `KeyPlan`.
- **Story 2, the spec's types (kermit-bench).** `H2oOrder`, `H2O_MIN_SCALE` and `DEFAULT_H2O_SEED`, with no `GeneratorSpec` change yet.
- **Story 3, the generator.** `H2oGenerator` implements the landed `kermit_generate::Generator`. It writes six per-query projections (`x_id3`, `big`, `x_id1`, `small`, `x_id2`, `medium`) and returns q1, q2 and q5, each with an exact `expected`. Golden digests pin the data of one small plan in both orders.
- **Story 4, the spec variant and the dispatch arm.** `GeneratorSpec::H2o { scale, seed, order }`, its validation, and `materialize`'s `run_h2o` arm, in one commit, because both matches are exhaustive. Also the cache test (miss, hit, drift, force).
- **Story 5, `bench gen h2o`**, through the `GenTarget` that #96's phase 2 landed.
- **Story 6, the committed YAMLs and the end-to-end test**: every cell × optimiser × column-order policy reproduces each `expected`.
- **Story 7, docs** (no measured values).
- **Then:** the final gate, and the manager's acceptance runs at N = 1e7, which fill H2O.md's measurements.
- **Untouched:** every data structure and algorithm, the generator layer (`kermit-generate/src/{generator,sink,meta,run,output_dir}.rs`), kermit-rdf, the bench report schema, and SNAP's and JOB's code.

**Tech Stack:** Rust nightly (workspace `rust-toolchain.toml`) in the nix dev shell; the landed `kermit-generate` layer; `serde` / `serde_yaml` / `serde_json`; `anyhow`; `clap` 4; `sha2` 0.11 (dev, for the golden test); `tempfile`.

**Spec:** `docs/specs/2026-10-15-h2o-benchmark-design.md`

**Plan review:** two code-quality-reviewers, 2026-10-15, read-only against spec and contract 84ad4e5 and the post-SNAP tree (`aidanb/snap` f496c4c), with no cargo. Every pinned value was re-derived from the contract's Python model. There were no critical findings; 4 important and 11 minor findings were fixed before commit (Merge notes, "Plan review").

The spec is at 84ad4e5: Part 2, § Testing → H2O, § Documentation, and Amendment 2 apply. **Names contract:** `docs/superpowers/plans/2026-10-15-h2o-generator-contract.md` (84ad4e5). Every name, message and pinned value comes from it, and its appendix records how each pinned value was computed. A needed deviation goes back to the design session; it is never improvised.

---

## Ground rules for the executor

- **Paths.** `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/h2o_18dea44a38dbca15`. Every command runs from `$WT`. `SCRATCH` is the session scratchpad. `BASE` is the merge commit made below; the scope checks diff against it.
- **Branch:** the loom branch `aidanb/h2o`. Never switch branches, never rebase, never amend, never push. Pushing is "After this plan", and it needs the user's go-ahead.
- **Before Story 1, in order:**
  1. If this plan is not committed yet, commit it by name: `git add docs/superpowers/plans/2026-10-15-h2o-generator.md`, message `docs: plan #95 push 2, the H2O generator`, with the trailer lines below.
  2. **Precondition: #96's phase 2 is on master.** Run `git fetch origin`. Both of these must hold:
     ```bash
     git show origin/master:kermit/src/main.rs | grep -c '^struct GenTarget'
     git show origin/master:kermit/src/materialize.rs | grep -c 'fn generator_failed'
     ```
     Each must print `1`. If either prints `0`, SNAP's phase 2 has not landed: stop and report
     to the design session. The plan is based on the tree after SNAP (contract § Base), and
     re-basing it is the design session's job.
  3. `git merge origin/master` (merge, never rebase). A conflict stops execution; report it.
     The merge commit is `BASE`: record `BASE=$(git rev-parse HEAD)`.
  4. The supervisor's premise check runs against the merged tree:
     `python3 /nix/store/02b0qd7bq81fgcn1axw3bfs15kmpvfvd-source/scripts/plan_premises.py docs/superpowers/plans/2026-10-15-h2o-generator.md`.
     The plan was drafted against `aidanb/snap` at 74668ed. A quote that SNAP's later fixes
     moved is re-anchored on the merged tree, its text unchanged. A quote whose text changed
     goes back to the design session.
  5. In the first cargo slot, before any story's edit, record the binary-docs baseline that
     the final gate compares against:
     ```bash
     nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings 2>&1 | grep -E '^error' | sort > "$SCRATCH/rustdoc-base.txt"
     ```
- **Host: cargo only in a slot that the benchmark co-ordinator session
  (`aidanb/benchmark-co-ordinator`) allots, on the user's call.** Before any compile or
  test, check that the #101 chain's scripts are stopped (`ps -o stat=` shows `T`) or gone,
  and that this prints nothing:
  ```bash
  pgrep -af "[k]ermit-bench-runs/.*/bin/kermit"
  ```
  If it prints anything, stop and report: a compile under a timing run contaminates it.
- **Builds:** every cargo command runs in the dev shell on the floated nightly that CI uses,
  with two jobs, in the foreground:
  `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo …`.
  Set `RUSTFLAGS` / `RUSTDOCFLAGS` inside the `env`. Never commit `flake.lock` or
  `Cargo.lock`. The full workspace test (final gate) runs through `setsid nohup … & disown`,
  with a `kill -0` watcher.
- **Test runner:** `cargo test`. The dev shell has no `cargo-nextest`.
- **Formatting:** `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all` only. Stable rustfmt rewrites ~30 files.
- **Doc comments:** backtick every code identifier (clippy `doc_markdown` runs under
  `-Dwarnings`). Every library crate has `#![deny(missing_docs)]`. `wrap_comments` wraps at
  80 columns, so accept `cargo fmt`'s re-wrapping. Binary docs (`main.rs`, `materialize.rs`)
  are linted only by the final gate's `cargo rustdoc --bin kermit` step.
- **No `unsafe`.**
- **`cargo fmt --all` touches only the task's files.** If it changes any other file, leave that file out of the pathspec commit and report it.
- **Staging and committing by path only.** Never `git add -A` / `-u` / `.` / a directory,
  never `git commit -a` / `--amend`, and commit with a pathspec:
  `git commit -m "…" -- <files>`. The developer agents' guard hook enforces this and blocks
  `git reset`, `stash`, `checkout`, `restore`, `clean`, `rebase`, `switch` and `push`. The
  hook runs `scripts/hooks/developer_git_guard.py`, an **untracked copy** listed in
  `.git/info/exclude` (with `.superpowers/`). Never commit, edit or delete it.
- **Edit now, verify later** applies only if the user approves it for this plan. Otherwise
  every task's run steps run in order, in a cargo slot.
- **Commit trailers**, on every commit:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD
  ```

## File map

This map is derived from the contract. Each task's own **Files** list is authoritative.

| File | Story | Change |
|---|---|---|
| `kermit-generate/src/lib.rs` | 1 | `pub mod h2o;` |
| `kermit-generate/src/h2o/mod.rs` | 1, 3 | new: module doc and `KeyPlan` re-export (1); `H2oExtras`, `H2oGenerator` (3) |
| `kermit-generate/src/h2o/rng.rs` | 1 | new: `SplitMix64`, `KeyColumn`, `Step`, `stream`, tests |
| `kermit-generate/src/h2o/keys.rs` | 1 | new: `KeyPlan`, `KeyPool`, tests |
| `kermit-bench/src/definition.rs` | 2, 4 | `H2oOrder`, `H2O_MIN_SCALE`, `DEFAULT_H2O_SEED` (2); `GeneratorSpec::H2o`, `default_h2o_seed`, the validate arm (4); tests |
| `kermit-bench/src/lib.rs` | 2 | exports |
| `kermit-generate/Cargo.toml` | 3 | dev-dependency `sha2` |
| `kermit-generate/tests/h2o.rs` | 3 | new: the generator's integration tests |
| `kermit/src/materialize.rs` | 4 | `run_h2o`, its `dispatch` arm, the cache test |
| `kermit/src/main.rs` | 5 | `H2oOrderArg`, `GenSubcommand::H2o`, `h2o_bench_name`, `run_gen_h2o`, tests |
| `kermit/tests/cli_gen_scale_range.rs` | 5 | the h2o floor test |
| `benchmarks/h2o.yml`, `benchmarks/h2o-sorted.yml` | 6 | new |
| `kermit/tests/h2o_generate.rs` | 6 | new: end to end |
| `docs/benchmarks/H2O.md` | 7, acceptance | new (7); the 1e7 measurements (acceptance) |
| `docs/benchmarks/README.md`, `benchmarks/README.md`, `USAGE.md`, `README.md`, `CLAUDE.md`, `ARCHITECTURE.md` | 7 | `h2o` beside `snap` |

## Execution order

Stories run in order: 1 → 2 → 3 → 4 → 5 → 6, then the final gate, then the acceptance runs.
Story 3 uses Stories 1 and 2, Story 4 uses Stories 2 and 3, and Story 5 and Story 6 use
Story 4. Every code story builds with cargo, so none of them run concurrently. Story 7 runs
no cargo and may run alongside any of them.

## Story 1: Random numbers and keys

- **Developer:** intermediate-dev
- **Files:** `kermit-generate/src/lib.rs`, `kermit-generate/src/h2o/mod.rs`, `kermit-generate/src/h2o/rng.rs`, `kermit-generate/src/h2o/keys.rs`
- **Interfaces:** exposes `kermit_generate::h2o::KeyPlan` (`new`, `for_scale`, `rows`, `small`, `medium`; `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Serialize`, `Deserialize`) and, crate-private for Story 3, `h2o::rng::{SplitMix64, KeyColumn, Step, stream}` and `h2o::keys::KeyPool`, `KeyPlan::cardinality`; uses `kermit_ds::RowId`, `anyhow` and `serde`, all already in `kermit-generate`'s manifest
- **Verify:** `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate h2o`

H2O's data must be a pure function of `(plan, seed, order)` (#74), so this story writes the random numbers and the key columns the generator of Story 3 builds its six relations from, and nothing that touches a file. Task 1.1 adds the module `kermit_generate::h2o` and `rng.rs`: `SplitMix64` exactly as Vigna's `splitmix64.c` has it, Lemire's unbiased bounded draw, Fisher-Yates, and `stream`, which gives every (key column, step) pair its own generator so that no relation depends on the order the others are made in. Task 1.2 adds `keys.rs`: `KeyPlan`, the three cardinalities with their validation, and `KeyPool`, the four steps of key generation (the shuffled pool and its integer 90% split, x's key column, the right-hand table, and the shared-key count). Every pinned value in the tests comes from the design session's independent Python model, not from the Rust. `h2o/mod.rs` gets two `#[allow(dead_code)]` lines on the new submodules, because nothing outside the tests uses them until Story 3 removes the lines. No store query, `cfg(not(feature))` arm or async test is involved, so no `effort: xhigh` dispatch is needed; the shapes a naive suite would miss (the rejection loop of the bounded draw, the stream numbering, an off-by-one in the pool split) each have a mutation check below.

### Task 1.1: `h2o` module and `rng.rs`

**Files:**
- Modify: `kermit-generate/src/lib.rs:59-60`
- Create: `kermit-generate/src/h2o/mod.rs`
- Create: `kermit-generate/src/h2o/rng.rs`
- Test: `kermit-generate/src/h2o/rng.rs`

- [ ] **Step 1: Write the failing test.** In `kermit-generate/src/lib.rs` (as SNAP's phase 2 left it, lines 59-60) the module list begins

  ```rust
  mod generator;
  mod meta;
  ```

  Insert the new module between them:

  ```rust
  mod generator;
  pub mod h2o;
  mod meta;
  ```

  Create `kermit-generate/src/h2o/mod.rs`. Its module doc is final; the two lines below it are Task 1.1's half of the submodule block, and Task 1.2 completes it:

  ```rust
  //! H2O: the join task ("J1") of H2O.ai's db-benchmark as a generated
  //! benchmark (#95).
  //!
  //! db-benchmark is an open benchmark of dataframe and database engines. Its
  //! join task builds a fact table `x` of N rows with three integer key columns
  //! (`id1`, `id2`, `id3`) and three right-hand tables, `small` (N / 10⁶ keys),
  //! `medium` (N / 10³ keys) and `big` (N keys), each sharing 90% of its keys
  //! with the matching column of `x`. The data comes from the benchmark's
  //! `_data/join-datagen.R`, which is distributed under the Mozilla Public
  //! License 2.0. This module is a reimplementation of that generator's
  //! structure, not a copy of its code: it keeps the key pools, their 90%
  //! overlap and the way `x`'s column covers them, but draws from an in-repo
  //! random number generator (so a dependency bump cannot change the data) and
  //! splits the pools with integer arithmetic.
  //!
  //! Kermit joins integer relations, so the benchmark is six relations, each a
  //! projection that one query needs (a query over a shared `x(id1, id2, id3)`
  //! would descend its unused columns, because a `_` is still a trie level):
  //!
  //! | relation | columns | rows |
  //! |---|---|---|
  //! | `x_id3` | `id3` | N |
  //! | `big` | `id3` | N |
  //! | `x_id1` | `id1`, `id3` | N |
  //! | `small` | `id1` | N / 10⁶ |
  //! | `x_id2` | `id2`, `id3` | N |
  //! | `medium` | `id2` | N / 10³ |
  //!
  //! and three of upstream's five join queries, each with its exact answer
  //! count:
  //!
  //! | query | Datalog |
  //! |---|---|
  //! | `q1` | `Q(K, I) :- small(K), x_id1(K, I).` |
  //! | `q2` | `Q(K, I) :- medium(K), x_id2(K, I).` |
  //! | `q5` | `Q(K) :- big(K), x_id3(K).` |
  //!
  //! `x` carries its unique `id3` into `x_id1` and `x_id2`, so every result row
  //! is distinct and each count is the same for every structure.

  // Story 3 uses every item of these modules; until then they are dead outside
  // the tests.
  #[allow(dead_code)]
  mod rng;
  ```

  Create `kermit-generate/src/h2o/rng.rs` with the tests alone for now. The SplitMix64 vectors are what Vigna's `splitmix64.c` prints when compiled and run; the other pinned values come from the design session's Python model of this spec (the contract's appendix), which shares no code with the Rust:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      /// All twelve (column, step) pairs, in stream-number order.
      const ALL_STREAMS: [(KeyColumn, Step); 12] = [
          (KeyColumn::Id1, Step::Pool),
          (KeyColumn::Id1, Step::Pad),
          (KeyColumn::Id1, Step::LeftShuffle),
          (KeyColumn::Id1, Step::RightShuffle),
          (KeyColumn::Id2, Step::Pool),
          (KeyColumn::Id2, Step::Pad),
          (KeyColumn::Id2, Step::LeftShuffle),
          (KeyColumn::Id2, Step::RightShuffle),
          (KeyColumn::Id3, Step::Pool),
          (KeyColumn::Id3, Step::Pad),
          (KeyColumn::Id3, Step::LeftShuffle),
          (KeyColumn::Id3, Step::RightShuffle),
      ];

      #[test]
      fn splitmix64_matches_the_reference_implementation() {
          let mut zero = SplitMix64::new(0);
          let got: Vec<u64> = (0..5).map(|_| zero.next_u64()).collect();
          assert_eq!(got, [
              0xe220_a839_7b1d_cdaf,
              0x6e78_9e6a_a1b9_65f4,
              0x06c4_5d18_8009_454f,
              0xf88b_b8a8_724c_81ec,
              0x1b39_896a_51a8_749b,
          ]);
          let mut seeded = SplitMix64::new(1_234_567);
          let got: Vec<u64> = (0..5).map(|_| seeded.next_u64()).collect();
          assert_eq!(got, [
              0x599e_d017_fb08_fc85,
              0x2c73_f084_5854_0fa5,
              0x883e_bce5_a3f2_7c77,
              0x3fbe_f740_e917_7b3f,
              0xe3b8_3467_08cb_5ecd,
          ]);
      }

      #[test]
      fn streams_are_seeded_by_seed_column_and_step() {
          assert_eq!(
              stream(108, KeyColumn::Id3, Step::Pool).next_u64(),
              0x44a7_9af1_687f_f9dc
          );
          assert_eq!(
              stream(108, KeyColumn::Id1, Step::Pad).next_u64(),
              0xa1b0_6f19_77ea_1cb8
          );
          assert_eq!(
              stream(108, KeyColumn::Id2, Step::RightShuffle).next_u64(),
              0xde70_fad6_0a6e_c795
          );
          let firsts: std::collections::HashSet<u64> = ALL_STREAMS
              .iter()
              .map(|&(column, step)| stream(108, column, step).next_u64())
              .collect();
          assert_eq!(firsts.len(), 12, "two (column, step) pairs share a stream");
          assert_ne!(
              stream(109, KeyColumn::Id3, Step::Pool).next_u64(),
              stream(108, KeyColumn::Id3, Step::Pool).next_u64(),
              "the seed must reach the stream"
          );
      }

      #[test]
      fn bounded_matches_the_reference_model() {
          let mut rng = SplitMix64::new(0);
          let got: Vec<u64> = (0..8).map(|_| rng.bounded(10)).collect();
          assert_eq!(got, [8, 4, 0, 9, 1, 3, 1, 7]);
          // n = 2^63 + 1 rejects more than half of the raw draws, so this case
          // runs Lemire's rejection loop. Without the loop the sequence starts
          // 8147104208329303768.
          let n = (1_u64 << 63) + 1;
          let mut rng = SplitMix64::new(0);
          let got: Vec<u64> = (0..8).map(|_| rng.bounded(n)).collect();
          assert_eq!(got, [
              243_808_509_735_772_839,
              8_954_805_688_390_271_222,
              980_875_101_213_047_373,
              1_603_648_013_000_153_456,
              7_116_260_932_800_173_470,
              2_266_080_580_496_311_649,
              8_780_933_256_989_530_195,
              3_656_771_639_923_220_100,
          ]);
      }

      #[test]
      fn bounded_stays_in_range() {
          let bounds = [
              1,
              2,
              3,
              7,
              8,
              9,
              (1_u64 << 32) - 1,
              1 << 32,
              (1 << 32) + 1,
              1 << 63,
              u64::MAX,
          ];
          for n in bounds {
              let mut rng = SplitMix64::new(7);
              for draw in 0..10_000 {
                  let value = rng.bounded(n);
                  assert!(value < n, "draw {draw} of bounded({n}) gave {value}");
              }
          }
          let mut rng = SplitMix64::new(7);
          assert!((0..100).all(|_| rng.bounded(1) == 0), "bounded(1) must be 0");
      }

      #[test]
      fn bounded_is_uniform_on_a_small_range() {
          let mut rng = SplitMix64::new(42);
          let mut counts = [0_u32; 10];
          for _ in 0..100_000 {
              counts[rng.bounded(10) as usize] += 1;
          }
          // The counts of the reference model; pinning them pins the stream.
          assert_eq!(counts, [
              10066, 9876, 10129, 9933, 10060, 10042, 10104, 10155, 9839, 9796
          ]);
          let chi_square: f64 = counts
              .iter()
              .map(|&count| {
                  let off = f64::from(count) - 10_000.0;
                  off * off / 10_000.0
              })
              .sum();
          // 27.88 is the p = 0.001 point of chi-square with 9 degrees of freedom;
          // the reference model gives 14.8604.
          assert!(chi_square < 27.88, "chi-square {chi_square}");
      }

      #[test]
      #[should_panic(expected = "n must be positive")]
      fn bounded_of_zero_panics() { SplitMix64::new(0).bounded(0); }

      #[test]
      fn shuffle_yields_a_permutation() {
          let original: Vec<u32> = (0..1000).collect();
          let mut items = original.clone();
          SplitMix64::new(3).shuffle(&mut items);
          assert_ne!(items, original, "a 1000-element shuffle left the order alone");
          items.sort_unstable();
          assert_eq!(items, original);

          let mut empty: [u32; 0] = [];
          SplitMix64::new(3).shuffle(&mut empty);
          let mut one = [5_u32];
          SplitMix64::new(3).shuffle(&mut one);
          assert_eq!(one, [5]);
      }

      #[test]
      fn shuffle_matches_the_reference_model() {
          let mut ten: Vec<u32> = (0..10).collect();
          SplitMix64::new(3).shuffle(&mut ten);
          assert_eq!(ten, [5, 7, 2, 8, 3, 9, 0, 4, 6, 1]);
          let mut twenty: Vec<u32> = (0..20).collect();
          SplitMix64::new(3).shuffle(&mut twenty);
          assert_eq!(twenty, [
              12, 8, 0, 18, 7, 4, 19, 15, 10, 6, 14, 5, 17, 16, 9, 3, 1, 11, 13, 2
          ]);
      }
  }
  ```

- [ ] **Step 2: Run it to see it fail.**
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate h2o::rng
  ```
  Expected: compile errors in the test module, ``error[E0433]: failed to resolve: use of undeclared type `SplitMix64` ``, the same for `KeyColumn` and `Step`, ``error[E0412]: cannot find type `KeyColumn` in this scope`` and the same for `Step` (from `ALL_STREAMS`'s type), and ``error[E0425]: cannot find function `stream` in this scope``.

- [ ] **Step 3: Implement.** In `kermit-generate/src/h2o/rng.rs`, add above the `#[cfg(test)]` block:

  ```rust
  //! The random numbers behind H2O's data.
  //!
  //! Three published algorithms, written out here:
  //!
  //! - `SplitMix64` (Steele, Lea & Flood, "Fast splittable pseudorandom number
  //!   generators", OOPSLA 2014), exactly as Vigna's reference `splitmix64.c`
  //!   has it. It passes `BigCrush`, and its period of 2⁶⁴ is far beyond the
  //!   roughly 10¹⁰ draws the largest N needs.
  //! - Lemire's nearly divisionless bounded draws (ACM TOMACS 2019): an
  //!   unbiased value in `0..n` from one 128-bit multiply, with a rejection
  //!   step that almost never runs.
  //! - The Fisher-Yates shuffle, in Durstenfeld's form (1964).
  //!
  //! They are in this repository because a generator's output must be a pure
  //! function of its spec (#74), and `rand`'s `StdRng` and its sampling
  //! algorithms are not value-stable across versions: a dependency bump would
  //! change every relation, and the golden hashes of the tests with it.
  //!
  //! Key generation gives every (column, step) pair its own stream (see
  //! [`stream`]), so no relation's data depends on the order in which the
  //! others are made.

  /// The increment of `SplitMix64`, the golden-ratio constant of
  /// `splitmix64.c`.
  const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

  /// The output function of `SplitMix64`. Each step is a bijection on `u64`, so
  /// the whole function is one too.
  fn mix(z: u64) -> u64 {
      let z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
      let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
      z ^ (z >> 31)
  }

  /// `SplitMix64`, exactly as Vigna's `splitmix64.c`.
  #[derive(Debug, Clone)]
  pub(crate) struct SplitMix64 {
      /// `splitmix64.c`'s `x`.
      state: u64,
  }

  impl SplitMix64 {
      /// A generator whose state is `state`, which is `splitmix64.c`'s `x`.
      pub(crate) fn new(state: u64) -> Self { Self { state } }

      /// `splitmix64.c`'s `next()`: add `GAMMA` to the state (wrapping) and
      /// return its image under `mix`.
      pub(crate) fn next_u64(&mut self) -> u64 {
          self.state = self.state.wrapping_add(GAMMA);
          mix(self.state)
      }

      /// An unbiased value in `0..n`, by Lemire's method. The high 64 bits of
      /// `next * n` (a 128-bit product) are the candidate; it is biased only
      /// when the low 64 bits fall below `2⁶⁴ mod n`, so those draws are
      /// redrawn. The `low < n` test lets the division in the threshold be
      /// skipped almost always.
      ///
      /// # Panics
      ///
      /// Panics if `n` is 0.
      pub(crate) fn bounded(&mut self, n: u64) -> u64 {
          assert!(n > 0, "SplitMix64::bounded: n must be positive");
          let mut product = u128::from(self.next_u64()) * u128::from(n);
          let mut low = product as u64;
          if low < n {
              // 2⁶⁴ mod n, as `(2⁶⁴ - n) mod n`.
              let threshold = n.wrapping_neg() % n;
              while low < threshold {
                  product = u128::from(self.next_u64()) * u128::from(n);
                  low = product as u64;
              }
          }
          (product >> 64) as u64
      }

      /// Fisher-Yates (Durstenfeld): for `i` from `len - 1` down to 1, swap
      /// item `i` with item `bounded(i + 1)`.
      pub(crate) fn shuffle<T>(&mut self, items: &mut [T]) {
          for i in (1..items.len()).rev() {
              let j = self.bounded(i as u64 + 1) as usize;
              items.swap(i, j);
          }
      }
  }

  /// H2O's key columns, numbered as the streams number them. Renumbering one
  /// changes every relation the generator writes.
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub(crate) enum KeyColumn {
      /// `id1`, the key of `small`.
      Id1 = 0,
      /// `id2`, the key of `medium`.
      Id2 = 1,
      /// `id3`, the key of `big`.
      Id3 = 2,
  }

  /// The steps of key generation that draw random numbers, numbered as the
  /// streams number them.
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub(crate) enum Step {
      /// Shuffling the pool of keys.
      Pool = 0,
      /// Drawing the keys that pad `x`'s column out to N rows.
      Pad = 1,
      /// Shuffling `x`'s column.
      LeftShuffle = 2,
      /// Shuffling the right-hand table.
      RightShuffle = 3,
  }

  /// The generator of one (column, step) pair under `seed`. Its state is
  /// `mix(seed << 32 | (column * 4 + step))`. That input is injective in
  /// (seed, column, step) and `mix` is a bijection, so every stream starts at
  /// its own point on the 2⁶⁴ cycle, and no relation depends on the order in
  /// which the others are generated.
  pub(crate) fn stream(seed: u32, column: KeyColumn, step: Step) -> SplitMix64 {
      let id = column as u64 * 4 + step as u64;
      SplitMix64::new(mix((u64::from(seed) << 32) | id))
  }
  ```

- [ ] **Step 4: Run it to see it pass.**
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate h2o::rng
  ```
  Expected: `running 8 tests`, all `ok`, `test result: ok. 8 passed; 0 failed`. Then format and lint:
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-generate --all-targets
  git status --short
  ```
  Expected: clippy prints no warning, and `git status` lists only `kermit-generate/src/lib.rs` modified and `kermit-generate/src/h2o/` new (if `cargo fmt` touched any other file, leave it out of the pathspec commit and report it; the guard blocks `git checkout` and `git restore`).

- [ ] **Step 5: Commit**

```bash
git add -N kermit-generate/src/h2o/mod.rs kermit-generate/src/h2o/rng.rs
git commit -m "feat(generate): add the h2o module and its SplitMix64 random numbers (#95)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-generate/src/lib.rs kermit-generate/src/h2o/mod.rs kermit-generate/src/h2o/rng.rs
```

### Task 1.2: `keys.rs`: `KeyPlan` and `KeyPool`

**Files:**
- Modify: `kermit-generate/src/h2o/mod.rs` (the submodule block Task 1.1 wrote)
- Create: `kermit-generate/src/h2o/keys.rs`
- Test: `kermit-generate/src/h2o/keys.rs`

- [ ] **Step 1: Write the failing test.** In `kermit-generate/src/h2o/mod.rs`, Task 1.1 wrote

  ```rust
  // Story 3 uses every item of these modules; until then they are dead outside
  // the tests.
  #[allow(dead_code)]
  mod rng;
  ```

  Replace it with the block the contract fixes for Story 1:

  ```rust
  // Story 3 uses every item of these modules; until then they are dead outside
  // the tests.
  #[allow(dead_code)]
  mod keys;
  #[allow(dead_code)]
  mod rng;

  pub use keys::KeyPlan;
  ```

  Create `kermit-generate/src/h2o/keys.rs` with the tests alone for now. They use the production items through `super::*`, and the pinned vectors of the last test are the design session's Python model's for `n = 20`, seed 108, column `id3`:

  ```rust
  #[cfg(test)]
  mod tests {
      use {super::*, std::collections::HashSet};

      fn sorted(mut keys: Vec<usize>) -> Vec<usize> {
          keys.sort_unstable();
          keys
      }

      #[test]
      fn pool_splits_have_the_right_sizes() {
          for n in [7, 20, 200, 2000] {
              let pool = KeyPool::new(n, 108, KeyColumn::Id3);
              let tenth = n / 10;
              assert_eq!(pool.shared().len(), n - tenth, "shared, n = {n}");
              assert_eq!(pool.left().len(), tenth, "left, n = {n}");
              assert_eq!(pool.right().len(), tenth, "right, n = {n}");
              let together = [pool.shared(), pool.left(), pool.right()].concat();
              assert_eq!(
                  sorted(together.clone()),
                  (1..=n + tenth).collect::<Vec<_>>(),
                  "n = {n}: the splits are not 1..=n + n/10, each once"
              );
              assert_ne!(
                  together,
                  sorted(together.clone()),
                  "n = {n}: the pool was not shuffled"
              );
          }
          // n / 10 = 0: nothing is left-only or right-only.
          let pool = KeyPool::new(7, 108, KeyColumn::Id3);
          assert!(pool.left().is_empty() && pool.right().is_empty());
      }

      #[test]
      fn the_right_table_is_shared_and_right_without_duplicates() {
          for n in [7, 20, 200] {
              let pool = KeyPool::new(n, 108, KeyColumn::Id1);
              let table = pool.right_table();
              assert_eq!(table.len(), n, "n = {n}");
              let distinct: HashSet<usize> = table.iter().copied().collect();
              assert_eq!(distinct.len(), n, "n = {n}: the table repeats a key");
              let expected: HashSet<usize> =
                  pool.shared().iter().chain(pool.right()).copied().collect();
              assert_eq!(distinct, expected, "n = {n}");
          }
          let pool = KeyPool::new(200, 108, KeyColumn::Id1);
          assert_ne!(
              pool.right_table(),
              [pool.shared(), pool.right()].concat(),
              "the right table was not shuffled"
          );
      }

      #[test]
      fn x_column_covers_shared_and_left_and_never_right() {
          for (n, rows) in [(20, 2000), (200, 2000), (7, 50)] {
              let pool = KeyPool::new(n, 108, KeyColumn::Id2);
              let x = pool.x_column(rows);
              assert_eq!(x.len(), rows, "n = {n}");
              let present: HashSet<usize> = x.iter().copied().collect();
              for key in pool.shared().iter().chain(pool.left()) {
                  assert!(present.contains(key), "n = {n}: key {key} is missing");
              }
              for key in pool.right() {
                  assert!(!present.contains(key), "n = {n}: right-only key {key} in x");
              }
          }
      }

      #[test]
      #[should_panic(expected = "below n")]
      fn x_column_needs_at_least_n_rows() {
          KeyPool::new(20, 108, KeyColumn::Id2).x_column(19);
      }

      #[test]
      fn the_x_column_at_n_equal_rows_is_a_permutation() {
          let pool = KeyPool::new(2000, 108, KeyColumn::Id3);
          let x = pool.x_column(2000);
          let covered = [pool.shared(), pool.left()].concat();
          assert_ne!(x, covered, "the column was not shuffled");
          assert_eq!(sorted(x), sorted(covered));
      }

      #[test]
      fn matches_count_the_rows_with_a_shared_key() {
          for n in [20, 200] {
              let pool = KeyPool::new(n, 108, KeyColumn::Id1);
              let x = pool.x_column(2000);
              let shared: HashSet<usize> = pool.shared().iter().copied().collect();
              let expected = x.iter().filter(|&&key| shared.contains(&key)).count();
              assert_eq!(pool.matches(&x), expected as u64, "n = {n}");
              assert!(expected > 0, "n = {n}: no row has a shared key");
              assert!(expected < x.len(), "n = {n}: every row has a shared key");
          }
          // n / 10 = 0: every key of x is shared.
          let pool = KeyPool::new(7, 108, KeyColumn::Id1);
          assert_eq!(pool.matches(&pool.x_column(50)), 50);
          assert_eq!(pool.matches(&[]), 0);
      }

      #[test]
      fn key_plan_rejects_bad_cardinalities() {
          let past_row_ids = RowId::MAX as usize + 1;
          let cases = [
              (100, 0, 10, "small = 0 is outside 1..=100"),
              (100, 10, 0, "medium = 0 is outside 1..=100"),
              (100, 101, 10, "small = 101 is outside 1..=100"),
              (100, 10, 101, "medium = 101 is outside 1..=100"),
              (past_row_ids, 1, 1, "rows = 4294967296 exceeds RowId::MAX = 4294967295"),
          ];
          for (rows, small, medium, message) in cases {
              let Err(err) = KeyPlan::new(rows, small, medium) else {
                  panic!("({rows}, {small}, {medium}) was accepted");
              };
              assert!(
                  err.to_string().contains(message),
                  "({rows}, {small}, {medium}): {err}"
              );
              assert!(err.to_string().starts_with("H2O plan: "), "{err}");
          }
          // The bounds themselves are fine.
          assert!(KeyPlan::new(100, 100, 100).is_ok());
          assert!(KeyPlan::new(100, 1, 1).is_ok());
          assert!(KeyPlan::new(RowId::MAX as usize, 1, 1).is_ok());
      }

      #[test]
      fn key_plan_for_scale_divides_by_a_million_and_a_thousand() {
          let plan = KeyPlan::for_scale(10_000_000).unwrap();
          assert_eq!(
              (plan.rows(), plan.small(), plan.medium()),
              (10_000_000, 10, 10_000)
          );
          let plan = KeyPlan::for_scale(1_000_000).unwrap();
          assert_eq!((plan.rows(), plan.small(), plan.medium()), (1_000_000, 1, 1_000));
          let Err(err) = KeyPlan::for_scale(999_999) else {
              panic!("a scale below 1,000,000 was accepted");
          };
          assert!(err.to_string().contains("small = 0"), "{err}");
      }

      #[test]
      fn key_plan_serializes_its_three_cardinalities() {
          let plan = KeyPlan::new(2000, 20, 200).unwrap();
          let json = serde_json::to_string(&plan).unwrap();
          assert_eq!(json, r#"{"rows":2000,"small":20,"medium":200}"#);
          assert_eq!(serde_json::from_str::<KeyPlan>(&json).unwrap(), plan);
      }

      #[test]
      fn the_cardinality_of_a_column_is_small_medium_or_rows() {
          let plan = KeyPlan::new(2000, 20, 200).unwrap();
          assert_eq!(plan.cardinality(KeyColumn::Id1), 20);
          assert_eq!(plan.cardinality(KeyColumn::Id2), 200);
          assert_eq!(plan.cardinality(KeyColumn::Id3), 2000);
      }

      #[test]
      fn the_pool_and_its_columns_match_the_reference_model() {
          let pool = KeyPool::new(20, 108, KeyColumn::Id3);
          assert_eq!(pool.shared(), [
              19, 11, 8, 5, 10, 13, 16, 15, 9, 12, 3, 22, 18, 14, 1, 7, 20, 4
          ]);
          assert_eq!(pool.left(), [21, 17]);
          assert_eq!(pool.right(), [2, 6]);
          let x = pool.x_column(30);
          assert_eq!(x, [
              22, 19, 10, 20, 10, 12, 10, 3, 8, 4, 11, 14, 9, 16, 1, 22, 19, 14, 17, 18, 19, 3, 5,
              11, 7, 21, 9, 13, 19, 15
          ]);
          assert_eq!(pool.right_table(), [
              15, 18, 12, 5, 7, 11, 20, 14, 8, 16, 2, 6, 4, 13, 19, 9, 22, 10, 1, 3
          ]);
          // Two rows (the left-only keys 21 and 17, once each) are not shared.
          assert_eq!(pool.matches(&x), 28);
      }
  }
  ```

- [ ] **Step 2: Run it to see it fail.**
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate h2o::keys
  ```
  Expected: compile errors, ``error[E0432]: unresolved import `keys::KeyPlan` `` in `h2o/mod.rs`, and in the tests ``error[E0433]: failed to resolve: use of undeclared type `KeyPool` ``, the same for `KeyColumn`, `KeyPlan` and `RowId`.

- [ ] **Step 3: Implement.** In `kermit-generate/src/h2o/keys.rs`, add above the `#[cfg(test)]` block:

  ```rust
  //! H2O's key columns: the cardinalities of a plan ([`KeyPlan`]) and the four
  //! steps that turn one cardinality into a key column of `x` and a right-hand
  //! table ([`KeyPool`]).
  //!
  //! For a column of cardinality n, with n/10 taken as integer division:
  //!
  //! 1. The pool is the keys `1..=n + n/10`, shuffled. Its first `n - n/10`
  //!    keys are shared by `x` and the right-hand table, the next `n/10` are
  //!    left-only (`x`'s only) and the last `n/10` are right-only. The integer
  //!    split replaces upstream's `n * 0.9`, so every n is well defined.
  //! 2. `x`'s column is the `n` shared and left keys, padded to the row count
  //!    with uniform draws from them (upstream's `sample_all`), then shuffled.
  //!    Every shared and left key appears at least once.
  //! 3. The right-hand table is the shared keys and the right-only keys,
  //!    shuffled: exactly n distinct keys.
  //! 4. The match count is the number of `x`'s rows whose key is shared.

  use {
      super::rng::{stream, KeyColumn, Step},
      anyhow::ensure,
      kermit_ds::RowId,
  };

  /// One H2O plan's cardinalities: `rows` = N, the rows of `x` and the keys of
  /// `big`; `small` and `medium`, the keys of the other two right-hand tables.
  #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
  pub struct KeyPlan {
      /// N, the rows of `x` and the keys of `big`.
      rows: usize,
      /// The keys of `small`.
      small: usize,
      /// The keys of `medium`.
      medium: usize,
  }

  impl KeyPlan {
      /// A plan of `rows` fact-table rows, `small` keys in `small` and `medium`
      /// keys in `medium`.
      ///
      /// # Errors
      ///
      /// Unless `1 <= small <= rows`, `1 <= medium <= rows` and
      /// `rows <= RowId::MAX`.
      pub fn new(rows: usize, small: usize, medium: usize) -> anyhow::Result<Self> {
          ensure!(
              rows <= RowId::MAX as usize,
              "H2O plan: rows = {rows} exceeds RowId::MAX = {}",
              RowId::MAX
          );
          ensure!(
              (1..=rows).contains(&small),
              "H2O plan: small = {small} is outside 1..={rows}"
          );
          ensure!(
              (1..=rows).contains(&medium),
              "H2O plan: medium = {medium} is outside 1..={rows}"
          );
          Ok(Self { rows, small, medium })
      }

      /// The plan of scale `scale`: `(N, N / 1_000_000, N / 1_000)`, through
      /// [`KeyPlan::new`], so a scale below 1,000,000 is an error (`small`
      /// would be 0).
      ///
      /// # Errors
      ///
      /// As [`KeyPlan::new`].
      pub fn for_scale(scale: u32) -> anyhow::Result<Self> {
          let rows = scale as usize;
          Self::new(rows, rows / 1_000_000, rows / 1_000)
      }

      /// N, the rows of `x` and the keys of `big`.
      pub fn rows(&self) -> usize { self.rows }

      /// The keys of `small`.
      pub fn small(&self) -> usize { self.small }

      /// The keys of `medium`.
      pub fn medium(&self) -> usize { self.medium }

      /// The cardinality of `column`'s keys: `small`, `medium` or `rows`.
      pub(crate) fn cardinality(&self, column: KeyColumn) -> usize {
          match column {
              | KeyColumn::Id1 => self.small,
              | KeyColumn::Id2 => self.medium,
              | KeyColumn::Id3 => self.rows,
          }
      }
  }

  /// Step 1 of key generation for one column of cardinality `n`: the keys
  /// `1..=n + n/10`, shuffled (stream `Pool`). The first `n - n/10` are shared,
  /// the next `n/10` are left-only (`x`'s only), and the last `n/10` are
  /// right-only.
  pub(crate) struct KeyPool {
      /// The shuffled keys: shared, then left-only, then right-only.
      keys: Vec<usize>,
      /// The column's cardinality.
      n: usize,
      /// The seed every stream of this column is drawn from.
      seed: u32,
      /// The column, which numbers its streams.
      column: KeyColumn,
  }

  impl KeyPool {
      /// The pool of a column of cardinality `n` under `seed`.
      pub(crate) fn new(n: usize, seed: u32, column: KeyColumn) -> Self {
          let mut keys: Vec<usize> = (1..=n + n / 10).collect();
          stream(seed, column, Step::Pool).shuffle(&mut keys);
          Self { keys, n, seed, column }
      }

      /// The keys that `x` and the right-hand table share: `keys[..n - n/10]`.
      pub(crate) fn shared(&self) -> &[usize] { &self.keys[..self.n - self.n / 10] }

      /// The keys only `x` has: `keys[n - n/10..n]`. Only the tests need
      /// them on their own; `x_column` takes them with the shared keys.
      #[cfg(test)]
      pub(crate) fn left(&self) -> &[usize] { &self.keys[self.n - self.n / 10..self.n] }

      /// The keys only the right-hand table has: `keys[n..]`.
      pub(crate) fn right(&self) -> &[usize] { &self.keys[self.n..] }

      /// Step 2, `x`'s column (upstream's `sample_all`): `keys[..n]`, then
      /// `rows - n` draws `keys[bounded(n)]` (stream `Pad`), then shuffled
      /// (stream `LeftShuffle`). Every shared and left key appears.
      ///
      /// # Panics
      ///
      /// Panics if `rows < n`.
      pub(crate) fn x_column(&self, rows: usize) -> Vec<usize> {
          assert!(rows >= self.n, "KeyPool::x_column: rows = {rows} is below n = {}", self.n);
          let mut x = Vec::with_capacity(rows);
          x.extend_from_slice(&self.keys[..self.n]);
          let mut pad = stream(self.seed, self.column, Step::Pad);
          for _ in self.n..rows {
              x.push(self.keys[pad.bounded(self.n as u64) as usize]);
          }
          stream(self.seed, self.column, Step::LeftShuffle).shuffle(&mut x);
          x
      }

      /// Step 3, the right-hand table: the shared keys and the right-only keys,
      /// shuffled (stream `RightShuffle`): exactly `n` distinct keys.
      pub(crate) fn right_table(&self) -> Vec<usize> {
          let mut table = Vec::with_capacity(self.n);
          table.extend_from_slice(self.shared());
          table.extend_from_slice(self.right());
          stream(self.seed, self.column, Step::RightShuffle).shuffle(&mut table);
          table
      }

      /// Step 4: how many of `x`'s keys are shared, by a bitmap over
      /// `0..=n + n/10`. `x` must hold keys of this pool.
      pub(crate) fn matches(&self, x: &[usize]) -> u64 {
          let mut is_shared = vec![false; self.keys.len() + 1];
          for &key in self.shared() {
              is_shared[key] = true;
          }
          x.iter().filter(|&&key| is_shared[key]).count() as u64
      }
  }
  ```

- [ ] **Step 4: Run it to see it pass.**
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate h2o
  ```
  Expected: `running 19 tests` (8 in `h2o::rng`, 11 in `h2o::keys`), all `ok`, `test result: ok. 19 passed; 0 failed`. Then format, lint and document:
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-generate --all-targets
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-generate --no-deps
  git status --short
  ```
  Expected: no clippy warning, no rustdoc warning, and `git status` lists only `kermit-generate/src/h2o/mod.rs` modified and `keys.rs` new (plus Task 1.1's files, already committed).

- [ ] **Step 5: Commit**

```bash
git add -N kermit-generate/src/h2o/keys.rs
git commit -m "feat(generate): add H2O's KeyPlan and key columns (#95)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-generate/src/h2o/mod.rs kermit-generate/src/h2o/keys.rs
```

### Mutation checks

For the manager. Apply each edit alone, run `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate h2o`, expect the named test to fail, then undo the edit by reversing it exactly (commit first or reverse the exact edit; never `git checkout` over uncommitted work). Check the edit applied (`git diff` shows it) before trusting a pass or a fail.

1. **No rejection loop.** In `rng.rs`, `bounded`: change `if low < n {` to `if false {`. `bounded_matches_the_reference_model` must fail (its second case starts at `8147104208329303768` instead of `243808509735772839`).
2. **Wrong stream id.** In `rng.rs`, `stream`: change `let id = column as u64 * 4 + step as u64;` to `let id = column as u64 * 3 + step as u64;`. `streams_are_seeded_by_seed_column_and_step` must fail (the `Id3` `Pool` pin), and so must `the_pool_and_its_columns_match_the_reference_model`.
3. **Seed not in the stream.** In `rng.rs`, `stream`: change `(u64::from(seed) << 32) | id` to `(u64::from(seed) << 31) | id`. `streams_are_seeded_by_seed_column_and_step` must fail.
4. **Pool split off by one.** In `keys.rs`, `shared`: change `&self.keys[..self.n - self.n / 10]` to `&self.keys[..self.n - self.n / 10 + 1]`. `pool_splits_have_the_right_sizes` must fail (shared has one key too many).
5. **Shuffle drawing from the wrong range.** In `rng.rs`, `shuffle`: change `self.bounded(i as u64 + 1)` to `self.bounded(i as u64)`. The result is still a permutation, so `shuffle_yields_a_permutation` passes; `shuffle_matches_the_reference_model` must fail.
6. **Padding from the right-only keys.** In `keys.rs`, `x_column`: change `pad.bounded(self.n as u64)` to `pad.bounded(self.keys.len() as u64)`. `x_column_covers_shared_and_left_and_never_right` must fail (a right-only key reaches `x`).

## Story 2: kermit-bench types

- **Developer:** junior-dev
- **Files:** `kermit-bench/src/definition.rs`, `kermit-bench/src/lib.rs`
- **Interfaces:** exposes `kermit_bench::{H2oOrder, H2O_MIN_SCALE, DEFAULT_H2O_SEED}` (`H2oOrder::{Shuffled, Sorted}`, `ALL`, `name`, `Default` = `Shuffled`, serde kebab case); uses nothing from earlier stories, and `GeneratorSpec` is unchanged (Story 4 adds its `H2o` variant)
- **Verify:** `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench h2o`

Story 4 will add `GeneratorSpec::H2o { scale, seed, order }`; this story lands only the three items it and the generator need, so that Story 3's generator and Story 4's spec can both name them. `H2O_MIN_SCALE` is upstream's floor for N (1e7, below which the `small` table has no left-only or right-only key) and `DEFAULT_H2O_SEED` is upstream's `set.seed(108)`. `H2oOrder` is the row-order axis of every H2O relation (#101): `Shuffled` (the default) is the order the generator draws rows in, `Sorted` is each relation in `Tuples::sort` order, and its kebab-case spelling is the YAML and command-line one, as `SnapGraph` and `SnapQuery` do for SNAP. The three tests mirror SNAP's: names match their serde spelling, the default, and an unknown spelling fails with an error that names the valid ones. It is a single small file pair with complete code and no wiring beyond the crate-root export, so a junior-dev suffices.

### Task 2.1: `H2oOrder` and the two constants

**Files:**
- Modify: `kermit-bench/src/definition.rs:194-199` (insert after `impl SnapQuery`)
- Modify: `kermit-bench/src/definition.rs:1493-1498` (tests, after `an_unknown_snap_name_fails_to_parse`)
- Modify: `kermit-bench/src/lib.rs:29-35`
- Test: `kermit-bench/src/definition.rs`

- [ ] **Step 1: Write the failing test.** In `kermit-bench/src/definition.rs`'s `mod tests`, `an_unknown_snap_name_fails_to_parse` ends (as SNAP's phase 2 left it, lines 1491-1498):

  ```rust
          // One unknown name spoils a whole list; the same list without it
          // parses.
          assert!(serde_yaml::from_str::<Vec<SnapQuery>>("[triangle, nope]").is_err());
          assert_eq!(
              serde_yaml::from_str::<Vec<SnapQuery>>("[triangle, clique4]").unwrap(),
              [SnapQuery::Triangle, SnapQuery::Clique4]
          );
      }
  ```

  Add after it (a blank line between):

  ```rust
      #[test]
      fn h2o_order_names_match_their_serde_spelling() {
          for order in H2oOrder::ALL {
              assert_eq!(
                  serde_yaml::to_string(&order).unwrap().trim_end(),
                  order.name()
              );
              let parsed: H2oOrder = serde_yaml::from_str(order.name()).unwrap();
              assert_eq!(parsed, order);
          }
          // The documented spellings, in the documented order.
          assert_eq!(H2oOrder::ALL.map(H2oOrder::name), ["shuffled", "sorted"]);
      }

      #[test]
      fn h2o_order_defaults_to_shuffled() {
          assert_eq!(H2oOrder::default(), H2oOrder::Shuffled);
          assert_eq!(H2oOrder::ALL[0], H2oOrder::default());
          // The other two defaults of an H2O spec: upstream's floor and seed.
          assert_eq!(H2O_MIN_SCALE, 10_000_000);
          assert_eq!(DEFAULT_H2O_SEED, 108);
      }

      #[test]
      fn an_unknown_h2o_order_fails_to_parse() {
          for bad in ["random", "Sorted", ""] {
              let parsed = serde_yaml::from_str::<H2oOrder>(&format!("'{bad}'"));
              assert!(parsed.is_err(), "order {bad:?} parsed as {parsed:?}");
          }
          // The error names the valid spellings.
          let err = serde_yaml::from_str::<H2oOrder>("'random'")
              .unwrap_err()
              .to_string();
          assert!(err.contains("shuffled"), "{err}");
          assert!(err.contains("sorted"), "{err}");
      }
  ```

- [ ] **Step 2: Run it to see it fail.**
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench h2o
  ```
  Expected: compile errors in the test module, ``error[E0433]: failed to resolve: use of undeclared type `H2oOrder` ``, ``error[E0412]: cannot find type `H2oOrder` in this scope`` (from `let parsed: H2oOrder` and `from_str::<H2oOrder>`), and ``error[E0425]: cannot find value `H2O_MIN_SCALE` in this scope`` (and the same for `DEFAULT_H2O_SEED`).

- [ ] **Step 3: Implement.** In `kermit-bench/src/definition.rs`, `impl SnapQuery` ends and the WatDiv spec begins (SNAP's tip, lines 194-199):

  ```rust
              | Self::Clique4 => "clique4",
          }
      }
  }

  /// WatDiv stress-template parameters. Field defaults match the `bench gen
  /// watdiv` CLI defaults.
  ```

  Insert between the closing `}` of `impl SnapQuery` and the `/// WatDiv stress-template parameters.` line (blank lines kept on both sides):

  ```rust
  /// The smallest H2O scale: upstream's floor. Below N = 1e7, `small` (N / 1e6
  /// keys) has no left-only or right-only key.
  pub const H2O_MIN_SCALE: u32 = 10_000_000;

  /// The default H2O seed: upstream's `set.seed(108)`.
  pub const DEFAULT_H2O_SEED: u32 = 108;

  /// The row order of every relation an H2O generator writes (#101's
  /// input-order axis).
  ///
  /// The YAML and command-line spelling is kebab case (`order: sorted`);
  /// [`H2oOrder::name`] returns it.
  #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
  #[serde(rename_all = "kebab-case")]
  pub enum H2oOrder {
      /// The order the generator draws rows in, as upstream writes them.
      #[default]
      Shuffled,
      /// Each relation's rows sorted lexicographically (`Tuples::sort`), the
      /// order the sorted tries build in. The draws are the same as
      /// `Shuffled`'s.
      Sorted,
  }

  impl H2oOrder {
      /// Every order, in this order.
      pub const ALL: [H2oOrder; 2] = [Self::Shuffled, Self::Sorted];

      /// The YAML and CLI spelling: `shuffled`, `sorted`.
      pub fn name(self) -> &'static str {
          match self {
              | Self::Shuffled => "shuffled",
              | Self::Sorted => "sorted",
          }
      }
  }

  ```

  Then in `kermit-bench/src/lib.rs` (lines 29-35 on SNAP's tip) the export reads

  ```rust
  pub use {
      definition::{
          BenchmarkDefinition, GeneratorSpec, QueryDefinition, RelationSource, SnapGraph, SnapQuery,
          WatdivStressSpec, DEFAULT_LUBM_ONTOLOGY,
      },
      error::BenchError,
  };
  ```

  Replace it with the following (types in alphabetical order, then the constants, as rustfmt sorts them; if `cargo fmt` lays the lines out differently, take its layout):

  ```rust
  pub use {
      definition::{
          BenchmarkDefinition, GeneratorSpec, H2oOrder, QueryDefinition, RelationSource, SnapGraph,
          SnapQuery, WatdivStressSpec, DEFAULT_H2O_SEED, DEFAULT_LUBM_ONTOLOGY, H2O_MIN_SCALE,
      },
      error::BenchError,
  };
  ```

- [ ] **Step 4: Run it to see it pass.**
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench h2o
  ```
  Expected: `running 3 tests`, all `ok`, `test result: ok. 3 passed; 0 failed`. Then the whole crate, format, lint and docs:
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-bench --all-targets
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-bench --no-deps
  git status --short
  ```
  Expected: the crate's tests pass, no clippy or rustdoc warning, and `git status` lists only `kermit-bench/src/definition.rs` and `kermit-bench/src/lib.rs` modified (if `cargo fmt` touched any other file, leave it out of the pathspec commit and report it; the guard blocks `git checkout` and `git restore`).

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(kermit-bench): add H2oOrder, H2O_MIN_SCALE and DEFAULT_H2O_SEED (#95)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-bench/src/definition.rs kermit-bench/src/lib.rs
```

### Mutation checks

For the manager. Apply each edit alone, run `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench h2o`, expect the named test to fail, then reverse the edit exactly; check the edit applied (`git diff`) first.

1. **Wrong spelling.** In `H2oOrder::name`, change `| Self::Sorted => "sorted",` to `| Self::Sorted => "sort",`. `h2o_order_names_match_their_serde_spelling` must fail.
2. **Wrong default.** In `H2oOrder`, move `#[default]` from `Shuffled` to `Sorted`. `h2o_order_defaults_to_shuffled` must fail.
3. **Wrong serde spelling.** Delete the `#[serde(rename_all = "kebab-case")]` line above `pub enum H2oOrder`. `h2o_order_names_match_their_serde_spelling` and `an_unknown_h2o_order_fails_to_parse` must fail (`Sorted` would parse, and `sorted` would not).
4. **Wrong floor.** Change `H2O_MIN_SCALE` to `1_000_000`. `h2o_order_defaults_to_shuffled` must fail.

## Story 3: The generator

- **Developer:** intermediate-dev
- **Files:** `kermit-generate/Cargo.toml`, `kermit-generate/src/h2o/mod.rs`, `kermit-generate/tests/h2o.rs`
- **Interfaces:** exposes `kermit_generate::h2o::{H2oGenerator, H2oExtras}` (with `KeyPlan` from Story 1) to Stories 4 (`run_h2o`), 5 (`run_gen_h2o`) and 6 (`h2o_generate.rs`); uses `KeyPool`, `KeyPlan::cardinality` and `KeyColumn` (Story 1), `kermit_bench::H2oOrder` (Story 2), and the layer's `Generator`, `Generated`, `RelationSink` and `run`
- **Verify:** `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --test h2o`

This story turns Story 1's random numbers and key columns into the H2O benchmark. `H2oGenerator` implements the layer's `Generator`: for each key column in turn (`id3`, `id1`, `id2`) it builds the key pool, x's column, the right-hand table and the match count, and writes the two relations that column yields through the sink (`x_id3`, `big`, `x_id1`, `small`, `x_id2`, `medium`, in that order). Counts are taken from the unsorted columns, and under `H2oOrder::Sorted` each relation is sorted (`Tuples::sort`) just before it is written, so a sorted relation is the shuffled one sorted and `expected` is the same in both orders. The three queries q1, q2 and q5 carry those counts as `expected`. Task 3.1 creates the generator and the integration test file, with the structural tests (the six relations and three queries, their sizes, the counts against a brute-force join). Task 3.2 adds the `sha2` dev-dependency and the tests that pin the data (golden digests from the design session's independent Python model, determinism, sorted equals sorted-shuffled, a plan with no left-only keys). The tests use only the public API. The story ends with six mutants that the tests must kill.

### Task 3.1: `H2oGenerator`, `H2oExtras` and the structural tests

**Files:**
- Modify: `kermit-generate/src/h2o/mod.rs` (replace Story 1's block of two gated module declarations and `pub use keys::KeyPlan;`, and append the generator below it)
- Create: `kermit-generate/tests/h2o.rs`
- Test: `kermit-generate/tests/h2o.rs`

- [ ] **Step 1: Write the failing test.** Create `kermit-generate/tests/h2o.rs`. `generate` runs the generator into `<tempdir>/h2o-test` (the last component of `out_dir` must equal the bench name) and reads everything back: `benchmark.yml` as a `BenchmarkDefinition`, each relation with `read_parquet`. `brute_force` shares no code with the generator's bitmap: it counts each key's occurrences in the right-hand table and sums them over x's rows. The reference plan is `KeyPlan::new(2000, 20, 200)` with seed 108. `Output::out_dir` is also used to read `meta.json` back, which pins that the flattened extras round-trip.

  ```rust
  //! The H2O generator through the layer's public API: `run` over an
  //! `H2oGenerator`, then the Parquet files, `benchmark.yml` and `meta.json` it
  //! leaves behind, read back the way `bench run` reads them.

  use {
      kermit_bench::{BenchmarkDefinition, H2oOrder},
      kermit_ds::{read_parquet, RelationHeader, Tuples},
      kermit_generate::{
          h2o::{H2oExtras, H2oGenerator, KeyPlan},
          run, GeneratedMeta,
      },
      kermit_rdf::generator::{read_meta_json, Target},
      std::{collections::HashMap, fs, path::PathBuf},
  };

  /// Upstream's `set.seed(108)`, the default seed.
  const SEED: u32 = 108;

  /// The benchmark's name, which is also the last component of its directory.
  const BENCH_NAME: &str = "h2o-test";

  /// The six relations in write order, with their columns.
  const RELATIONS: [(&str, &[&str]); 6] = [
      ("x_id3", &["id3"]),
      ("big", &["id3"]),
      ("x_id1", &["id1", "id3"]),
      ("small", &["id1"]),
      ("x_id2", &["id2", "id3"]),
      ("medium", &["id2"]),
  ];

  /// The queries in order: name, description and rule.
  const QUERIES: [(&str, &str, &str); 3] = [
      (
          "q1",
          "H2O J1 q1 (small inner on int): few keys, each with many x rows",
          "Q(K, I) :- small(K), x_id1(K, I).",
      ),
      (
          "q2",
          "H2O J1 q2 (medium inner on int): thousands of keys, each with about a thousand x rows",
          "Q(K, I) :- medium(K), x_id2(K, I).",
      ),
      (
          "q5",
          "H2O J1 q5 (big inner on int): two dense key lists that overlap by 90%",
          "Q(K) :- big(K), x_id3(K).",
      ),
  ];

  /// Each query with the right-hand table and the fact relation it joins.
  const JOINS: [(&str, &str, &str); 3] = [
      ("q1", "small", "x_id1"),
      ("q2", "medium", "x_id2"),
      ("q5", "big", "x_id3"),
  ];

  /// 2,000 rows of `x`, 20 `small` keys and 200 `medium` keys.
  fn reference_plan() -> KeyPlan { KeyPlan::new(2000, 20, 200).unwrap() }

  /// One generated benchmark, read back from disk.
  struct Output {
      /// Keeps the directory alive.
      _dir: tempfile::TempDir,
      out_dir: PathBuf,
      meta: GeneratedMeta<H2oExtras>,
      definition: BenchmarkDefinition,
      /// Each relation as `read_parquet` returns it, by name.
      relations: HashMap<String, (RelationHeader, Tuples)>,
  }

  impl Output {
      fn tuples(&self, name: &str) -> &Tuples { &self.relations[name].1 }

      /// The `expected` count `benchmark.yml` records for `query`.
      fn expected(&self, query: &str) -> u64 {
          let definition = self
              .definition
              .queries
              .iter()
              .find(|q| q.name == query)
              .unwrap_or_else(|| panic!("benchmark.yml has no query {query}"));
          definition
              .expected
              .unwrap_or_else(|| panic!("{query} has no expected count"))
      }
  }

  /// Generates `plan` under `seed` and `order` into `<tempdir>/h2o-test`, with
  /// tag `test` and no spec hash.
  fn generate(plan: KeyPlan, seed: u32, order: H2oOrder) -> Output {
      let dir = tempfile::tempdir().unwrap();
      let out_dir = dir.path().join(BENCH_NAME);
      let target = Target {
          out_dir: &out_dir,
          bench_name: BENCH_NAME,
          tag: "test",
          spec_hash: None,
      };
      let meta = run(&H2oGenerator::new(target, plan, seed, order)).unwrap();
      let yaml = fs::read_to_string(out_dir.join("benchmark.yml")).unwrap();
      let definition: BenchmarkDefinition = serde_yaml::from_str(&yaml).unwrap();
      let relations = definition
          .relations
          .iter()
          .map(|relation| {
              let path = out_dir.join(format!("{}.parquet", relation.name));
              (relation.name.clone(), read_parquet(path).unwrap())
          })
          .collect();
      Output {
          _dir: dir,
          out_dir,
          meta,
          definition,
          relations,
      }
  }

  /// The join's cardinality by definition: how often each key occurs in
  /// `table`'s first column, summed over the keys in `x`'s first column.
  /// It shares no code with the generator's bitmap.
  fn brute_force(out: &Output, table: &str, x: &str) -> u64 {
      let mut occurrences: HashMap<usize, u64> = HashMap::new();
      for row in out.tuples(table).rows() {
          *occurrences.entry(row[0]).or_default() += 1;
      }
      out.tuples(x)
          .rows()
          .map(|row| occurrences.get(&row[0]).copied().unwrap_or(0))
          .sum()
  }

  /// Whether `x`'s second column is `x3`'s only column, row for row.
  fn carries_id3(x: &Tuples, x3: &Tuples) -> bool {
      x.rows().zip(x3.rows()).all(|(pair, single)| pair[1] == single[0])
  }

  #[test]
  fn writes_six_relations_and_three_queries() {
      let plan = reference_plan();
      for order in H2oOrder::ALL {
          let out = generate(plan, SEED, order);

          // The relations, in write order, with their columns.
          let written: Vec<&str> = RELATIONS.iter().map(|&(name, _)| name).collect();
          let listed: Vec<&str> = out
              .definition
              .relations
              .iter()
              .map(|relation| relation.name.as_str())
              .collect();
          assert_eq!(listed, written, "{order:?}");
          let recorded: Vec<&str> = out.meta.relations.iter().map(|r| r.name.as_str()).collect();
          assert_eq!(recorded, written, "{order:?}");
          for (name, attrs) in RELATIONS {
              let (header, _) = &out.relations[name];
              assert_eq!(header.attrs(), attrs, "{name} {order:?}");
              let record = out.meta.relations.iter().find(|r| r.name == name).unwrap();
              assert_eq!(record.columns, attrs, "{name} {order:?}");
          }

          // The queries, in order, each with a count.
          let queries: Vec<(&str, &str, &str)> = out
              .definition
              .queries
              .iter()
              .map(|q| (q.name.as_str(), q.description.as_str(), q.query.as_str()))
              .collect();
          assert_eq!(queries, QUERIES, "{order:?}");
          assert!(out.definition.queries.iter().all(|q| q.expected.is_some()));
          assert_eq!(
              out.definition.description,
              format!(
                  "H2O db-benchmark join (J1): N = 2000, small = 20, medium = 200, seed 108, \
                   {} rows",
                  order.name()
              )
          );

          // meta.json: the kind, the counts and the extras, which also read
          // back from the file through the flattened layout.
          assert_eq!(out.meta.kind, "h2o");
          assert_eq!((out.meta.relation_count, out.meta.query_count), (6, 3));
          let extras = H2oExtras {
              seed: SEED,
              order,
              plan,
          };
          assert_eq!(out.meta.extras, extras, "{order:?}");
          let read: GeneratedMeta<H2oExtras> = read_meta_json(&out.out_dir).unwrap();
          assert_eq!(read.extras, extras, "{order:?}");
      }
  }

  #[test]
  fn relation_sizes_follow_the_plan() {
      let plan = reference_plan();
      for order in H2oOrder::ALL {
          let out = generate(plan, SEED, order);
          let sizes = [2000, 2000, 2000, 20, 2000, 200];
          for ((name, attrs), rows) in RELATIONS.into_iter().zip(sizes) {
              let tuples = out.tuples(name);
              assert_eq!(
                  (tuples.len(), tuples.arity()),
                  (rows, attrs.len()),
                  "{name} {order:?}"
              );
          }
          let recorded: Vec<u64> = out.meta.relations.iter().map(|r| r.rows).collect();
          assert_eq!(recorded, [2000, 2000, 2000, 20, 2000, 200], "{order:?}");
      }

      // Under `Shuffled`, x_id1 and x_id2 carry x_id3's id3 column row for row.
      let shuffled = generate(plan, SEED, H2oOrder::Shuffled);
      let x3 = shuffled.tuples("x_id3");
      for name in ["x_id1", "x_id2"] {
          assert!(carries_id3(shuffled.tuples(name), x3), "{name}");
      }
      // Under `Sorted` they no longer do, which shows the check above can fail.
      let sorted = generate(plan, SEED, H2oOrder::Sorted);
      let x3 = sorted.tuples("x_id3");
      for name in ["x_id1", "x_id2"] {
          assert!(!carries_id3(sorted.tuples(name), x3), "{name}");
      }
  }

  #[test]
  fn expected_counts_equal_a_brute_force_join() {
      for order in H2oOrder::ALL {
          let out = generate(reference_plan(), SEED, order);
          for (query, table, x) in JOINS {
              let counted = brute_force(&out, table, x);
              assert_eq!(counted, out.expected(query), "{query} {order:?}");
              // Some of x's rows join and some (the left-only keys) don't.
              assert!(counted > 0, "{query} {order:?}: nothing joins");
              assert!(counted < 2000, "{query} {order:?}: every row joins");
          }
      }
  }
  ```

- [ ] **Step 2: Run it to see it fail.**
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --test h2o
  ```
  Expected: the test target fails to compile: ``error[E0432]: unresolved imports `kermit_generate::h2o::H2oExtras`, `kermit_generate::h2o::H2oGenerator` `` (both are missing from `h2o`).

- [ ] **Step 3: Implement.** In `kermit-generate/src/h2o/mod.rs`, Story 1 left, below the module doc,

  ```rust
  // Story 3 uses every item of these modules; until then they are dead outside
  // the tests.
  #[allow(dead_code)]
  mod keys;
  #[allow(dead_code)]
  mod rng;

  pub use keys::KeyPlan;
  ```

  Replace exactly that block (leave the module doc above it alone) with the following, which drops the comment and both `#[allow(dead_code)]` lines, adds the imports, and appends the generator:

  ```rust
  mod keys;
  mod rng;

  pub use keys::KeyPlan;
  use {
      crate::{
          generator::{Generated, Generator},
          h2o::{keys::KeyPool, rng::KeyColumn},
          sink::RelationSink,
      },
      kermit_bench::{H2oOrder, QueryDefinition},
      kermit_ds::{RelationHeader, Tuples},
      kermit_rdf::generator::Target,
      serde::{Deserialize, Serialize},
  };

  /// q1, q2 and q5: name, description and Datalog rule. `generate` takes
  /// their counts in this order.
  const QUERIES: [(&str, &str, &str); 3] = [
      (
          "q1",
          "H2O J1 q1 (small inner on int): few keys, each with many x rows",
          "Q(K, I) :- small(K), x_id1(K, I).",
      ),
      (
          "q2",
          "H2O J1 q2 (medium inner on int): thousands of keys, each with about a thousand x rows",
          "Q(K, I) :- medium(K), x_id2(K, I).",
      ),
      (
          "q5",
          "H2O J1 q5 (big inner on int): two dense key lists that overlap by 90%",
          "Q(K) :- big(K), x_id3(K).",
      ),
  ];

  /// H2O's own `meta.json` fields, flattened beside the common ones.
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct H2oExtras {
      /// The seed every stream was drawn from.
      pub seed: u32,
      /// The row order of every relation.
      pub order: H2oOrder,
      /// The cardinalities.
      pub plan: KeyPlan,
  }

  /// Generates an H2O join benchmark: six relations and the queries q1, q2 and
  /// q5, each with the exact count it returns.
  #[derive(Debug)]
  pub struct H2oGenerator<'a> {
      target: Target<'a>,
      plan: KeyPlan,
      seed: u32,
      order: H2oOrder,
  }

  impl<'a> H2oGenerator<'a> {
      /// A generator of `plan`'s relations into `target`, drawn from `seed` and
      /// written in `order`.
      pub fn new(target: Target<'a>, plan: KeyPlan, seed: u32, order: H2oOrder) -> Self {
          Self {
              target,
              plan,
              seed,
              order,
          }
      }
  }

  impl Generator for H2oGenerator<'_> {
      type Extras = H2oExtras;

      const KIND: &'static str = "h2o";

      fn target(&self) -> Target<'_> { self.target }

      /// Writes `x_id3`, `big`, `x_id1`, `small`, `x_id2` and `medium`, in that
      /// order, and returns q1, q2 and q5 with their counts.
      ///
      /// One key column at a time, so that a column's pool and, once it is
      /// interleaved, its `x` column are dropped before the next is built:
      /// the peak is about 4N words (`x3`, `x1` and the 2N interleaved
      /// buffer). The counts are taken from the unsorted columns, and
      /// `x3` is cloned for `x_id3` because sorting that copy must not
      /// reorder the `id3` values that `x_id1` and `x_id2` are paired with.
      fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<H2oExtras>> {
          let (rows, small, medium) = (self.plan.rows(), self.plan.small(), self.plan.medium());
          let (seed, order) = (self.seed, self.order);
          let pool = |column: KeyColumn| KeyPool::new(self.plan.cardinality(column), seed, column);

          // `id3`: n = rows, so x's column is a permutation. It is kept: it is
          // the second column of `x_id1` and `x_id2`.
          let pool3 = pool(KeyColumn::Id3);
          let x3 = pool3.x_column(rows);
          let q5 = pool3.matches(&x3);
          let big = pool3.right_table();
          drop(pool3);
          write(sink, order, "x_id3", &["id3"], Tuples::from_flat(1, rows, x3.clone()))?;
          write(sink, order, "big", &["id3"], Tuples::from_flat(1, rows, big))?;

          // `id1`.
          let pool1 = pool(KeyColumn::Id1);
          let x1 = pool1.x_column(rows);
          let q1 = pool1.matches(&x1);
          let small_table = pool1.right_table();
          drop(pool1);
          let x_id1 = Tuples::from_flat(2, rows, interleave(&x1, &x3));
          drop(x1);
          write(sink, order, "x_id1", &["id1", "id3"], x_id1)?;
          write(sink, order, "small", &["id1"], Tuples::from_flat(1, small, small_table))?;

          // `id2`; `x3` is not needed after this.
          let pool2 = pool(KeyColumn::Id2);
          let x2 = pool2.x_column(rows);
          let q2 = pool2.matches(&x2);
          let medium_table = pool2.right_table();
          drop(pool2);
          let x_id2 = Tuples::from_flat(2, rows, interleave(&x2, &x3));
          drop(x2);
          drop(x3);
          write(sink, order, "x_id2", &["id2", "id3"], x_id2)?;
          write(sink, order, "medium", &["id2"], Tuples::from_flat(1, medium, medium_table))?;

          let queries = QUERIES
              .iter()
              .zip([q1, q2, q5])
              .map(|(&(name, description, rule), expected)| QueryDefinition {
                  name: name.to_string(),
                  description: description.to_string(),
                  query: rule.to_string(),
                  expected: Some(expected),
              })
              .collect();
          Ok(Generated {
              description: format!(
                  "H2O db-benchmark join (J1): N = {rows}, small = {small}, medium = {medium}, \
                   seed {seed}, {order} rows",
                  order = order.name()
              ),
              queries,
              extras: H2oExtras {
                  seed,
                  order,
                  plan: self.plan,
              },
          })
      }
  }

  /// Sorts `tuples` under `H2oOrder::Sorted`, then writes it as the relation
  /// `name` with columns `attrs`.
  fn write(
      sink: &mut RelationSink, order: H2oOrder, name: &str, attrs: &[&str], mut tuples: Tuples,
  ) -> anyhow::Result<()> {
      if order == H2oOrder::Sorted {
          tuples.sort();
      }
      let attrs = attrs.iter().map(|attr| (*attr).to_string()).collect();
      sink.write(&RelationHeader::new(name, attrs), &tuples)
  }

  /// The pairs `[left[i], right[i]]`, row after row, in one buffer sized
  /// exactly for them.
  ///
  /// # Panics
  ///
  /// Panics if the slices differ in length.
  fn interleave(left: &[usize], right: &[usize]) -> Vec<usize> {
      assert_eq!(left.len(), right.len(), "interleave: the columns differ in length");
      let mut pairs = Vec::with_capacity(2 * left.len());
      for (&l, &r) in left.iter().zip(right) {
          pairs.push(l);
          pairs.push(r);
      }
      pairs
  }
  ```

  The module doc above the block stays as Story 1 wrote it. Removing the two `#[allow(dead_code)]` lines is only sound if every item in `keys.rs` and `rng.rs` is now used outside the tests; if the compiler reports one that is not, do not restore the allow: stop and report which item, because that is a Story 1 contract question.

- [ ] **Step 4: Run it to see it pass.**
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --test h2o
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit-generate --all-targets -- -D warnings
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all --check
  ```
  Expected: `test result: ok. 3 passed; 0 failed` for the `h2o` target (`writes_six_relations_and_three_queries`, `relation_sizes_follow_the_plan`, `expected_counts_equal_a_brute_force_join`); clippy, `cargo doc` and the format check are clean. If `cargo fmt --check` reports a diff, run `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all` and re-check; commit only this task's files, and report any other file it changed.

- [ ] **Step 5: Commit.**
  ```bash
  git add -N kermit-generate/tests/h2o.rs
  git commit -m "feat(generate): add the H2O generator (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-generate/src/h2o/mod.rs kermit-generate/tests/h2o.rs
  ```

### Task 3.2: Golden digests, determinism, sorted order and a plan without left-only keys

**Files:**
- Modify: `kermit-generate/Cargo.toml` (`[dev-dependencies]`)
- Modify: `kermit-generate/tests/h2o.rs` (the `use` block, and four tests appended)
- Test: `kermit-generate/tests/h2o.rs`

These tests characterise a generator that Task 3.1 finished, so their red step is the missing dev-dependency. The four tests can fail: Step 4 of this task is followed by the story's mutation checks, which each of them kills. The pinned digests and counts come from the design session's Python model of the spec, which shares no code with the Rust; a mismatch is a bug in the generator or in Story 1, never a reason to change a pin.

- [ ] **Step 1: Write the failing test.** In `kermit-generate/tests/h2o.rs`, add the line `sha2::{Digest, Sha256},` to the `use` block between the `kermit_rdf::generator::{read_meta_json, Target},` line and the `std::{…}` entry. Then append the following at the end of the file, after `expected_counts_equal_a_brute_force_join`:

  ```rust
  /// The reference plan's relations in `Shuffled` order, each as the SHA-256
  /// of its values (see [`digest`]), in write order.
  const GOLDEN_SHUFFLED: [(&str, &str); 6] = [
      ("x_id3", "a6995949eb98f11444e9bd59e5353a46928a2648ae45bb9e8d254afb0399589c"),
      ("big", "daeb0faaa79972199c71600bf24ac7a8a9dba4c1d0e4a25204b2eec574010102"),
      ("x_id1", "3a9a6901e57520c8c51da77f54e373758cf47e20d5b29441b2814055a4b1fcab"),
      ("small", "7a7ca985781668a8c510f5905ad5bd56e7fc6a471bac204accf59a6023bad7fa"),
      ("x_id2", "01ae30e97a521fced72f5faa1e26e2eaa2304b1bb5c6888d593e98a241a4eb97"),
      ("medium", "ca2644e7d6a743f68a010af7c2ec46fced86a942bb724e2dba5a46af90f93255"),
  ];

  /// The same relations in `Sorted` order.
  const GOLDEN_SORTED: [(&str, &str); 6] = [
      ("x_id3", "46d0bb7d56f4102f31d36ea3896df676d431bd73fc332b430c7d8d7be430db48"),
      ("big", "078cf7896fb356d148669cea09eb3dfaf06961c984e3fe397e886315f387ca85"),
      ("x_id1", "69239a8aac021928a0b533e5fede0b3aff66215bd8d0e1f2f47749156e1c2814"),
      ("small", "c38798bf9a2d0d3f28a36a50aa81ef6e3b5fefc477f7dbc3029f0705b096a5aa"),
      ("x_id2", "136e352cc2d950268350010d22d50a4e23d0255e5bf2c564535bada8da8fbc6d"),
      ("medium", "14135e2b2fc781278aed6f1d400f19d5f12f339f3cf0bf4f5b3f040547f11f03"),
  ];

  /// The reference plan's counts, in both orders.
  const GOLDEN_COUNTS: [(&str, u64); 3] = [("q1", 1812), ("q2", 1814), ("q5", 1800)];

  /// The SHA-256 of a relation's data, not of its file (the Parquet footer
  /// records the `parquet` crate's version): every value of `as_flat()`, in
  /// order, as `(v as u64).to_le_bytes()`.
  fn digest(tuples: &Tuples) -> String {
      let mut hasher = Sha256::new();
      for &value in tuples.as_flat() {
          hasher.update((value as u64).to_le_bytes());
      }
      hex(&hasher.finalize())
  }

  /// Lowercase hex, as `bench fetch` compares digests.
  fn hex(bytes: &[u8]) -> String {
      use std::fmt::Write as _;
      let mut out = String::with_capacity(bytes.len() * 2);
      for byte in bytes {
          write!(out, "{byte:02x}").expect("writing to a String never fails");
      }
      out
  }

  /// Pins the reference plan's data, in both orders, to digests computed by a
  /// Python model of the spec that shares no code with this crate, and the
  /// three counts to the same model's.
  ///
  /// If these digests must change, the generator's output changed: bump
  /// `META_SCHEMA_VERSION` and add an `h2o` floor to `OUTDATED_BEFORE`, so that
  /// caches generated before the change are reported as outdated rather than
  /// passing as hits.
  #[test]
  fn the_reference_plan_matches_its_golden_digests() {
      let cases = [
          (H2oOrder::Shuffled, GOLDEN_SHUFFLED),
          (H2oOrder::Sorted, GOLDEN_SORTED),
      ];
      for (order, golden) in cases {
          let out = generate(reference_plan(), SEED, order);
          for (name, expected) in golden {
              assert_eq!(digest(out.tuples(name)), expected, "{name} {order:?}");
          }
          for (query, count) in GOLDEN_COUNTS {
              assert_eq!(out.expected(query), count, "{query} {order:?}");
          }
      }
  }

  #[test]
  fn the_same_spec_reproduces_and_another_seed_differs() {
      for order in H2oOrder::ALL {
          let first = generate(reference_plan(), SEED, order);
          let second = generate(reference_plan(), SEED, order);
          for (name, _) in RELATIONS {
              assert_eq!(first.tuples(name), second.tuples(name), "{name} {order:?}");
          }
          // The files are byte-identical too: the records hold their hashes.
          assert_eq!(first.meta.relations, second.meta.relations, "{order:?}");

          let other = generate(reference_plan(), SEED + 1, order);
          assert_eq!(other.meta.extras.seed, 109);
          assert_ne!(other.tuples("x_id3"), first.tuples("x_id3"), "{order:?}");
          assert_ne!(other.tuples("x_id1"), first.tuples("x_id1"), "{order:?}");
      }
  }

  #[test]
  fn sorted_relations_are_the_shuffled_relations_sorted() {
      let plan = reference_plan();
      let shuffled = generate(plan, SEED, H2oOrder::Shuffled);
      let sorted = generate(plan, SEED, H2oOrder::Sorted);
      for (name, _) in RELATIONS {
          let mut expected = shuffled.tuples(name).clone();
          expected.sort();
          assert_eq!(sorted.tuples(name), &expected, "{name}");
          assert!(sorted.tuples(name).rows().is_sorted(), "{name} is not sorted");
      }
      // Sorting moved the rows of the big relations, so the check above
      // compares two different orders.
      for name in ["x_id3", "x_id1", "x_id2", "big"] {
          assert_ne!(sorted.tuples(name), shuffled.tuples(name), "{name}");
      }
      // The draws and the counts are the same in both orders.
      for (query, _, _) in JOINS {
          assert_eq!(sorted.expected(query), shuffled.expected(query), "{query}");
      }
  }

  /// With `small` = 5 and `medium` = 9, n / 10 is 0: those pools have no
  /// left-only and no right-only key, so every row of `x` joins `small` and
  /// `medium`. `big` = 50 still has 5 of each, and q5 = 50 - 5.
  #[test]
  fn a_plan_without_left_only_keys_generates() {
      let out = generate(KeyPlan::new(50, 5, 9).unwrap(), SEED, H2oOrder::Shuffled);
      for (query, count) in [("q1", 50), ("q2", 50), ("q5", 45)] {
          assert_eq!(out.expected(query), count, "{query}");
      }
      for (query, table, x) in JOINS {
          assert_eq!(brute_force(&out, table, x), out.expected(query), "{query}");
      }
      for (name, keys) in [("small", 5), ("medium", 9)] {
          let mut written: Vec<usize> = out.tuples(name).rows().map(|row| row[0]).collect();
          written.sort_unstable();
          assert_eq!(written, (1..=keys).collect::<Vec<_>>(), "{name}");
      }
  }
  ```

- [ ] **Step 2: Run it to see it fail.**
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --test h2o
  ```
  Expected: the test target fails to compile: ``error[E0432]: unresolved import `sha2` ``, because `kermit-generate` does not depend on it yet.

- [ ] **Step 3: Implement.** In `kermit-generate/Cargo.toml`, replace

  ```toml
  [dev-dependencies]
  tempfile = "3"
  ```

  with

  ```toml
  [dev-dependencies]
  sha2 = "0.11"
  tempfile = "3"
  ```

  `0.11` is the version `kermit`, `kermit-bench` and `kermit-rdf` use. No source changes.

- [ ] **Step 4: Run it to see it pass.**
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit-generate --all-targets -- -D warnings
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all --check
  ```
  Expected: every `test result:` line is `ok`, and the `h2o` target reports `ok. 7 passed; 0 failed` (the three from Task 3.1, then `the_reference_plan_matches_its_golden_digests`, `the_same_spec_reproduces_and_another_seed_differs`, `sorted_relations_are_the_shuffled_relations_sorted` and `a_plan_without_left_only_keys_generates`); clippy and the format check are clean. If `cargo fmt --check` reports a diff, run `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all` and re-check; commit only this task's files, and report any other file it changed.

- [ ] **Step 5: Commit.**
  ```bash
  git commit -m "test(generate): pin the H2O reference plan's digests and its determinism (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-generate/Cargo.toml kermit-generate/tests/h2o.rs
  ```

### Mutation checks

For the manager, after Task 3.2 is committed. Copy `kermit-generate/src/h2o/mod.rs` to a scratch directory outside the repository first, apply one mutant at a time, run `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --test h2o`, and restore with `cp` from the scratch copy and `cmp` (silent) before the next. Never restore with `git checkout`. Each mutant must make the named tests fail (assert that the edit applied, e.g. with `git diff --stat`, before reading the result); then re-run the full target, which must pass again.

1. **`order` is ignored.** In `write`, replace `if order == H2oOrder::Sorted {` with `if false {`. Must fail: `sorted_relations_are_the_shuffled_relations_sorted` (a sorted relation differs from the shuffled one sorted) and `the_reference_plan_matches_its_golden_digests` (the `GOLDEN_SORTED` digests), and `relation_sizes_follow_the_plan` (under `Sorted`, `x_id1` still carries `x_id3`'s `id3` column row for row).
2. **The pairs are `[x3, x1]`.** Replace `interleave(&x1, &x3)` with `interleave(&x3, &x1)`. Must fail: `expected_counts_equal_a_brute_force_join` (`x_id1`'s first column is no longer `id1`), `relation_sizes_follow_the_plan` (its second column is no longer `x_id3`'s), and `the_reference_plan_matches_its_golden_digests`.
3. **`big` is written from the left keys.** Replace `let big = pool3.right_table();` with `let big = x3.clone();`, a table that holds x's keys, left-only ones included, instead of the right-only ones. Must fail: `expected_counts_equal_a_brute_force_join` (q5 counted over `big` is 2000, `expected` says 1800) and `the_reference_plan_matches_its_golden_digests`.
4. **The counts are paired with the wrong queries.** Replace `.zip([q1, q2, q5])` with `.zip([q2, q1, q5])`. Must fail: `expected_counts_equal_a_brute_force_join` and `the_reference_plan_matches_its_golden_digests` (1814 where q1 is pinned at 1812). `a_plan_without_left_only_keys_generates` still passes, since q1 = q2 = 50 there.
5. **The seed is ignored.** Replace `KeyPool::new(self.plan.cardinality(column), seed, column)` with `KeyPool::new(self.plan.cardinality(column), 108, column)`. Must fail: `the_same_spec_reproduces_and_another_seed_differs` (seed 109 yields the same `x_id3`). The golden test passes, since it runs at seed 108, which is why the determinism test needs a second seed.
6. **The extras forget the order.** In the `H2oExtras { … }` literal at the end of `generate`, replace the line `order,` (between `seed,` and `plan: self.plan,`) with `order: H2oOrder::Shuffled,`. Must fail: `writes_six_relations_and_three_queries` under `Sorted`.

## Story 4: The spec variant and the dispatch arm

- **Developer:** intermediate-dev
- **Files:** `kermit-bench/src/definition.rs`, `kermit/src/materialize.rs`
- **Interfaces:** exposes `kermit_bench::GeneratorSpec::H2o { scale, seed, order }` (kind `h2o`, with the defaults `seed` 108 and `order` shuffled, and `validate_generator`'s floor check on `scale`) and, in the binary, `materialize`'s `dispatch` arm for it (`run_h2o`); uses `H2oOrder`, `H2O_MIN_SCALE` and `DEFAULT_H2O_SEED` from Story 2 (`kermit-bench/src/definition.rs`, re-exported at the crate root), `kermit_generate::h2o::{H2oGenerator, KeyPlan}` from Story 3, and `kermit_generate::run` and `kermit_rdf::generator::Target` from the landed layer
- **Verify:** `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench -p kermit --lib --bins h2o`

This story makes `kind: h2o` a declarative generator spec and routes it to Story 3's generator. `GeneratorSpec::H2o` takes `scale` (required), `seed` (default `DEFAULT_H2O_SEED`, upstream's 108) and `order` (default `Shuffled`). All three fields are always serialized, so all three feed `spec_hash`, and an omitted `seed` hashes the same as `seed: 108`. `validate_generator` rejects a `scale` below `H2O_MIN_SCALE`, upstream's floor, in words that name it. `materialize`'s `dispatch` gains the matching arm, `run_h2o`, which builds `KeyPlan::for_scale(scale)` and an `H2oGenerator` with the spec hash and runs it. Task 4.1 is one commit across both files, because `GeneratorSpec`'s matches in `validate_generator` and `dispatch` are exhaustive and the workspace would not build between the two halves; it carries the four spec tests. Task 4.2 adds the cache test, `h2o_cache_misses_hits_drifts_and_regenerates`, which drives `materialize` end to end at scale 1,000,000 (below the floor, which `materialize` does not check, so `small` is one key). The supervisor should dispatch this story with `effort: xhigh`: `run_h2o` takes `scale` and `seed`, both `u32`, so a transposed call compiles and passes any suite that does not read the plan back, and a dropped `order` compiles too. Task 4.2's meta assertions (`plan`, `seed`, `order`) are what catch both.

Line numbers below are those of the SNAP phase-2 tip (`aidanb/snap`, 74668ed), which this plan builds on. Story 2 inserts its block after `impl SnapQuery` (line 197 of `definition.rs`), which moves every `definition.rs` line after it, so find each quote by its text.

### Task 4.1: `GeneratorSpec::H2o`, its validation, and the dispatch arm

**Files:**
- Modify: `kermit-bench/src/definition.rs:49-50`, `:114-116`, `:237`, `:659-666`, and `mod tests`, after the SNAP tests that end at `:1702-1703`
- Modify: `kermit/src/materialize.rs:26-31`, `:310-317`, `:463-467`
- Test: `kermit-bench/src/definition.rs`

- [ ] **Step 1: Write the failing tests.** In `kermit-bench/src/definition.rs`, the last SNAP test in `mod tests` ends (lines 1702-1703):

  ```rust
          assert!(!hashes.contains(&lubm.spec_hash()));
      }
  ```

  Insert after it, in `kermit-bench/src/definition.rs`:

  ```rust

      /// A `GeneratorSpec::H2o` with exactly the fields given.
      fn h2o(scale: u32, seed: u32, order: H2oOrder) -> GeneratorSpec {
          GeneratorSpec::H2o {
              scale,
              seed,
              order,
          }
      }

      /// The reason `validate_generator` rejects `spec` with; `spec` must be
      /// invalid.
      fn h2o_reason(spec: &GeneratorSpec) -> String {
          match validate_generator("h", spec).unwrap_err() {
              | BenchError::Invalid {
                  name,
                  reason,
              } => {
                  assert_eq!(name, "h");
                  reason
              },
              | other => panic!("expected Invalid, got {other:?}"),
          }
      }

      /// The `generator:` block of a definition whose only other fields are a
      /// name and a description, parsed.
      fn h2o_generator(block: &str) -> Result<GeneratorSpec, serde_yaml::Error> {
          let yaml = format!("name: h2o-x\ndescription: \"h2o\"\ngenerator:\n{block}");
          let def: BenchmarkDefinition = serde_yaml::from_str(&yaml)?;
          def.validate().expect("the definition is valid");
          Ok(def.generator.expect("the block was parsed"))
      }

      #[test]
      fn h2o_spec_parses_with_its_defaults() {
          let spec = h2o_generator("  kind: h2o\n  scale: 10000000\n").unwrap();
          assert_eq!(spec, h2o(10_000_000, 108, H2oOrder::Shuffled));
          // All three fields are written, so all three feed `spec_hash`.
          assert_eq!(
              serde_yaml::to_string(&spec).unwrap(),
              "kind: h2o\nscale: 10000000\nseed: 108\norder: shuffled\n"
          );
      }

      #[test]
      fn h2o_order_sorted_parses() {
          let spec =
              h2o_generator("  kind: h2o\n  scale: 10000000\n  seed: 7\n  order: sorted\n").unwrap();
          assert_eq!(spec, h2o(10_000_000, 7, H2oOrder::Sorted));
          assert_eq!(
              serde_yaml::to_string(&spec).unwrap(),
              "kind: h2o\nscale: 10000000\nseed: 7\norder: sorted\n"
          );
          // An order outside the enum is a parse error, not a silent default.
          assert!(h2o_generator("  kind: h2o\n  scale: 10000000\n  order: random\n").is_err());
          // So is a missing scale: it has no default.
          assert!(h2o_generator("  kind: h2o\n  seed: 7\n").is_err());
      }

      #[test]
      fn h2o_spec_hash_covers_every_field() {
          let base = h2o(10_000_000, 108, H2oOrder::Shuffled);
          let specs = [
              base.clone(),
              h2o(10_000_001, 108, H2oOrder::Shuffled),
              h2o(10_000_000, 109, H2oOrder::Shuffled),
              h2o(10_000_000, 108, H2oOrder::Sorted),
          ];
          let hashes: HashSet<String> = specs.iter().map(GeneratorSpec::spec_hash).collect();
          assert_eq!(hashes.len(), specs.len(), "two specs share a spec_hash");
          assert_eq!(base.spec_hash(), base.clone().spec_hash());

          // Omitted fields hash as their defaults: the cache of a YAML that
          // spells the defaults out is a hit for one that leaves them off.
          let omitted = h2o_generator("  kind: h2o\n  scale: 10000000\n").unwrap();
          let spelled =
              h2o_generator("  kind: h2o\n  scale: 10000000\n  seed: 108\n  order: shuffled\n")
                  .unwrap();
          assert_eq!(omitted.spec_hash(), spelled.spec_hash());
          assert_eq!(omitted.spec_hash(), base.spec_hash());
      }

      #[test]
      fn h2o_scale_below_the_floor_is_rejected() {
          // The floor itself validates, with any seed and either order.
          for order in H2oOrder::ALL {
              validate_generator("h", &h2o(10_000_000, 108, order)).unwrap();
              validate_generator("h", &h2o(u32::MAX, 0, order)).unwrap();
          }
          for scale in [0, 1, 1_000_000, 9_999_999] {
              let reason = h2o_reason(&h2o(scale, 108, H2oOrder::Shuffled));
              assert!(
                  reason.starts_with(&format!(
                      "h2o generator scale {scale} is below the floor 10000000: "
                  )),
                  "{reason}"
              );
              assert!(reason.contains("`small`"), "{reason}");
          }
          // The loader reaches the same check through the definition.
          let def = BenchmarkDefinition {
              name: "h2o-small".to_string(),
              description: "x".to_string(),
              relations: vec![],
              queries: vec![],
              generator: Some(h2o(9_999_999, 108, H2oOrder::Sorted)),
          };
          match def.validate().unwrap_err() {
              | BenchError::Invalid {
                  name,
                  reason,
              } => {
                  assert_eq!(name, "h2o-small");
                  assert!(reason.contains("10000000"), "{reason}");
              },
              | other => panic!("expected Invalid, got {other:?}"),
          }
      }
  ```

- [ ] **Step 2: Run it to see it fail.**

  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench h2o
  ```

  Expected: the test target does not compile: ``error[E0599]`` errors naming the variant `H2o` of `GeneratorSpec` (the wording differs between a struct literal, a pattern and a path), from `h2o()` and the tests, then ``error: could not compile `kermit-bench` (lib test)``.

- [ ] **Step 3: Implement.** Five edits in `kermit-bench/src/definition.rs`, then three in `kermit/src/materialize.rs`.

  `kermit-bench/src/definition.rs` current (lines 49-50):

  ```rust
  /// A declarative generator spec. Tagged on the `kind` field
  /// (`kind: watdiv`, `kind: watdiv-basic`, `kind: lubm` or `kind: snap`).
  ```

  Replace with:

  ```rust
  /// A declarative generator spec. Tagged on the `kind` field
  /// (`kind: watdiv`, `kind: watdiv-basic`, `kind: lubm`, `kind: snap` or
  /// `kind: h2o`).
  ```

  `kermit-bench/src/definition.rs` current (lines 114-116), the end of the `Snap` variant and of the enum:

  ```rust
          queries: Option<Vec<SnapQuery>>,
      },
  }
  ```

  Replace with:

  ```rust
          queries: Option<Vec<SnapQuery>>,
      },
      /// Drives the H2O generator (`kermit_generate::h2o::H2oGenerator`): six
      /// relations from db-benchmark's join data generator (J1) at `scale` rows,
      /// and the queries q1, q2 and q5 with their exact counts. All three
      /// fields are always serialized, so all three feed `spec_hash`.
      H2o {
          /// N, the rows of `x`. At least [`H2O_MIN_SCALE`].
          scale: u32,
          /// Default [`DEFAULT_H2O_SEED`] (upstream's `set.seed(108)`).
          #[serde(default = "default_h2o_seed")]
          seed: u32,
          /// Default [`H2oOrder::Shuffled`].
          #[serde(default)]
          order: H2oOrder,
      },
  }
  ```

  `kermit-bench/src/definition.rs` current (line 237), the last of the spec's default functions:

  ```rust
  fn default_lubm_ontology() -> String { DEFAULT_LUBM_ONTOLOGY.to_string() }
  ```

  Replace with:

  ```rust
  fn default_lubm_ontology() -> String { DEFAULT_LUBM_ONTOLOGY.to_string() }
  /// `GeneratorSpec::H2o`'s default `seed`.
  fn default_h2o_seed() -> u32 { DEFAULT_H2O_SEED }
  ```

  `kermit-bench/src/definition.rs` current (lines 659-666), the end of the `Snap` arm of `validate_generator`:

  ```rust
                          )));
                      }
                  }
              }
          },
      }
      Ok(())
  }
  ```

  Replace with:

  ```rust
                          )));
                      }
                  }
              }
          },
          | GeneratorSpec::H2o {
              scale, ..
          } => {
              if *scale < H2O_MIN_SCALE {
                  return Err(BenchError::Invalid {
                      name: bench_name.to_string(),
                      reason: format!(
                          "h2o generator scale {scale} is below the floor {H2O_MIN_SCALE}: \
                           upstream's join data generator needs N >= 1e7, below which `small` \
                           (N / 1e6 keys) has no left-only or right-only key"
                      ),
                  });
              }
          },
      }
      Ok(())
  }
  ```

  `kermit/src/materialize.rs` current (lines 26-31), the imports:

  ```rust
  use {
      kermit_bench::{
          BenchError, BenchmarkDefinition, GeneratorSpec, SnapGraph, SnapQuery, WatdivStressSpec,
      },
      kermit_generate::snap::{SnapGenerator, SnapSource},
      kermit_rdf::generator::{MetaHeader, Target},
  ```

  Replace with:

  ```rust
  use {
      kermit_bench::{
          BenchError, BenchmarkDefinition, GeneratorSpec, H2oOrder, SnapGraph, SnapQuery,
          WatdivStressSpec,
      },
      kermit_generate::{
          h2o::{H2oGenerator, KeyPlan},
          snap::{SnapGenerator, SnapSource},
      },
      kermit_rdf::generator::{MetaHeader, Target},
  ```

  `kermit/src/materialize.rs` current (lines 310-317), the end of `dispatch`:

  ```rust
              queries.as_deref(),
              bench_name,
              out_dir,
              spec_hash,
          ),
      }
  }
  ```

  Replace with:

  ```rust
              queries.as_deref(),
              bench_name,
              out_dir,
              spec_hash,
          ),
          | GeneratorSpec::H2o {
              scale,
              seed,
              order,
          } => run_h2o(*scale, *seed, *order, bench_name, out_dir, spec_hash),
      }
  }
  ```

  `kermit/src/materialize.rs` current (lines 463-467), the end of `run_snap` and the start of the tests:

  ```rust
      kermit_generate::run(&SnapGenerator::new(target, graph, source, queries))?;
      Ok(())
  }

  #[cfg(test)]
  ```

  Replace with:

  ```rust
      kermit_generate::run(&SnapGenerator::new(target, graph, source, queries))?;
      Ok(())
  }

  /// Runs the H2O generator for a `kind: h2o` spec into `out_dir`, passing
  /// `spec_hash` through so it lands in the produced `meta.json`.
  fn run_h2o(
      scale: u32, seed: u32, order: H2oOrder, bench_name: &str, out_dir: &Path, spec_hash: &str,
  ) -> anyhow::Result<()> {
      let plan = KeyPlan::for_scale(scale)?;
      let target = Target {
          out_dir,
          bench_name,
          tag: bench_name,
          spec_hash: Some(spec_hash),
      };
      kermit_generate::run(&H2oGenerator::new(target, plan, seed, order))?;
      Ok(())
  }

  #[cfg(test)]
  ```

- [ ] **Step 4: Run it to see it pass.**

  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-bench h2o
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit materialize
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit-bench -p kermit --all-targets -- -D warnings
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-bench --no-deps
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all --check
  ```

  Expected: the first prints `test result: ok. 7 passed; 0 failed` (Story 2's `h2o_order_names_match_their_serde_spelling`, `h2o_order_defaults_to_shuffled` and `an_unknown_h2o_order_fails_to_parse`, and this task's `h2o_spec_parses_with_its_defaults`, `h2o_order_sorted_parses`, `h2o_spec_hash_covers_every_field` and `h2o_scale_below_the_floor_is_rejected`). The second builds the binary with the new arm and passes every existing `materialize` test. Clippy, `cargo doc` and the format check are silent. If `cargo fmt --check` reports a diff, run `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all` and re-check; commit only this task's files, and report any other file it changed.

- [ ] **Step 5: Commit.**

  ```bash
  git commit -m "feat(kermit-bench): kind: h2o generator spec, floor validation and materialize dispatch (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-bench/src/definition.rs kermit/src/materialize.rs
  ```

### Task 4.2: The cache test

**Files:**
- Modify: `kermit/src/materialize.rs:840-842` (the end of `mod tests`)
- Test: `kermit/src/materialize.rs`

- [ ] **Step 1: Write the failing test.** In `kermit/src/materialize.rs`, the tests module ends (lines 840-842) with the end of `run_snap_without_a_source_is_an_error`:

  ```rust
          assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
      }
  }
  ```

  Insert between the test's closing `}` and the module's, in `kermit/src/materialize.rs`:

  ```rust

      /// `kind: h2o` through `materialize` (#95, D11): a miss generates, a hit
      /// reuses the cache, a changed seed is `SpecDrift`, and `--force`
      /// wipes the cache and regenerates. The scale is 1,000,000, below the
      /// floor, which `materialize` does not check (validation does, and this
      /// definition never goes through it); it makes `small` one key. The
      /// counts are the design session's independent model of the spec at
      /// (1,000,000, 1, 1,000).
      #[test]
      fn h2o_cache_misses_hits_drifts_and_regenerates() {
          fn spec(seed: u32, order: H2oOrder) -> GeneratorSpec {
              GeneratorSpec::H2o {
                  scale: 1_000_000,
                  seed,
                  order,
              }
          }
          fn definition(spec: GeneratorSpec) -> BenchmarkDefinition {
              BenchmarkDefinition {
                  name: "h2o-cache".to_string(),
                  description: "x".to_string(),
                  relations: vec![],
                  queries: vec![],
                  generator: Some(spec),
              }
          }

          let root = tempfile::tempdir().unwrap();
          let subdir = root.path().join("h2o-cache");
          let meta_path = subdir.join("meta.json");
          let read_meta = || -> serde_json::Value {
              serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap()
          };
          let base = spec(108, H2oOrder::Shuffled);

          // A miss generates six relations and three queries, each with a count.
          let first = materialize(definition(base.clone()), root.path(), false).unwrap();
          let names: Vec<&str> = first.relations.iter().map(|r| r.name.as_str()).collect();
          assert_eq!(names, ["x_id3", "big", "x_id1", "small", "x_id2", "medium"]);
          let counts: Vec<(&str, Option<u64>)> = first
              .queries
              .iter()
              .map(|q| (q.name.as_str(), q.expected))
              .collect();
          assert_eq!(counts, [
              ("q1", Some(1_000_000)),
              ("q2", Some(899_809)),
              ("q5", Some(900_000))
          ]);
          let meta = read_meta();
          assert_eq!(meta["kind"], "h2o");
          assert_eq!(meta["spec_hash"], base.spec_hash());
          assert_eq!(meta["seed"], 108);
          assert_eq!(meta["order"], "shuffled");
          assert_eq!(
              meta["plan"],
              serde_json::json!({ "rows": 1_000_000, "small": 1, "medium": 1_000 })
          );

          // A marker in the cache, and `meta.json` as it stands.
          let marker = subdir.join("planted.marker");
          fs::write(&marker, "planted").unwrap();
          let meta_bytes = fs::read(&meta_path).unwrap();
          let meta_modified = fs::metadata(&meta_path).unwrap().modified().unwrap();

          // The same spec is a hit: the cache is loaded and nothing is written.
          let hit = materialize(definition(base.clone()), root.path(), false).unwrap();
          assert_eq!(hit.relations.len(), 6);
          assert_eq!(hit.queries.len(), 3);
          assert_eq!(
              fs::metadata(&meta_path).unwrap().modified().unwrap(),
              meta_modified,
              "a hit regenerated the cache: meta.json was rewritten"
          );
          assert_eq!(fs::read(&meta_path).unwrap(), meta_bytes);
          assert!(marker.exists(), "a hit must not touch the cache");

          // Another seed is drift, and drift without --force deletes nothing.
          let drifted = spec(109, H2oOrder::Shuffled);
          match materialize(definition(drifted.clone()), root.path(), false).unwrap_err() {
              | BenchError::SpecDrift {
                  name,
                  expected_hash,
                  actual_hash,
                  ..
              } => {
                  assert_eq!(name, "h2o-cache");
                  assert_eq!(expected_hash, drifted.spec_hash());
                  assert_eq!(actual_hash, base.spec_hash());
              },
              | other => panic!("expected SpecDrift, got {other:?}"),
          }
          assert!(marker.exists(), "drift without --force must not delete the cache");
          assert_eq!(fs::read(&meta_path).unwrap(), meta_bytes);

          // --force wipes the cache and regenerates under the new spec. This
          // one also changes the order, so that `meta.json` shows both fields
          // of the spec reaching the generator.
          let forced_spec = spec(109, H2oOrder::Sorted);
          let forced = materialize(definition(forced_spec.clone()), root.path(), true).unwrap();
          assert!(!marker.exists(), "--force must wipe the old cache");
          assert_eq!(forced.relations.len(), 6);
          assert_eq!(forced.queries.len(), 3);
          let meta = read_meta();
          assert_eq!(meta["spec_hash"], forced_spec.spec_hash());
          assert_eq!(meta["seed"], 109);
          assert_eq!(meta["order"], "sorted");
      }
  ```

- [ ] **Step 2: Run it to see it fail.** Task 4.1 and Stories 1-3 are in, so the test passes as written; check that it can fail. Copy the file aside, then break the cache hit so that it falls through to a regeneration:

  ```bash
  : "${SCRATCH:?set SCRATCH to this session's scratchpad (Ground rules)}"
  cp kermit/src/materialize.rs "$SCRATCH/materialize.rs.orig" &&
  python3 -c 'p="kermit/src/materialize.rs"; s=open(p).read(); old="        | CacheState::Hit => return load_cached_yaml(&yml_path),\n"; assert s.count(old)==1; open(p,"w").write(s.replace(old,"        | CacheState::Hit => {},\n"))' &&
  { nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit h2o_cache; cp "$SCRATCH/materialize.rs.orig" kermit/src/materialize.rs; }
  cmp "$SCRATCH/materialize.rs.orig" kermit/src/materialize.rs
  ```

  Expected: the script's `assert` holds (the mutant applied; `git diff` cannot show it here, since the task's own test is uncommitted in the same file). The second `materialize` call now regenerates, so the test fails with ``a hit regenerated the cache: meta.json was rewritten``, and the restore's `cmp` prints nothing. Restore by copying the saved file back, never by reverting through git: the file's other changes are uncommitted.

- [ ] **Step 3: Implement.** Nothing to implement: the test needs no code beyond Task 4.1's arm, and Step 2 restored the file.

- [ ] **Step 4: Run it to see it pass.**

  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit h2o_cache
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit --all-targets -- -D warnings
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all --check
  ```

  Expected: `test materialize::tests::h2o_cache_misses_hits_drifts_and_regenerates ... ok` and `test result: ok. 1 passed; 0 failed`; a debug build generates 1,000,000 rows twice (the miss and the forced run), at `-O0` (the workspace has no dependency opt-level override), which may take tens of seconds: record the measured time in the story's report. Clippy and the format check are silent. If `cargo fmt --check` reports a diff, run `cargo fmt --all` through the same `nix develop` prefix and re-check.

- [ ] **Step 5: Commit.**

  ```bash
  git commit -m "test(kermit): kind: h2o through materialize, miss, hit, drift and force (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit/src/materialize.rs
  ```

### Mutation checks

For the manager, after Task 4.2. Each mutant is one edit; copy the file aside first, confirm with `git diff` that the edit applied (a mutant that did not apply proves nothing), run the named test, then restore with `cp` and `cmp` (never `git checkout`: the story's changes may be uncommitted).

1. In `kermit-bench/src/definition.rs`, `if *scale < H2O_MIN_SCALE {` becomes `if *scale <= H2O_MIN_SCALE {`. `cargo test -p kermit-bench h2o_scale_below_the_floor_is_rejected` must fail (the floor itself no longer validates), and so must `h2o_spec_parses_with_its_defaults`.
2. In the same file, delete `#[serde(default)]` above `order: H2oOrder,` in the `H2o` variant. `cargo test -p kermit-bench h2o_spec_parses_with_its_defaults` must fail with ``missing field `order` ``, and `h2o_spec_hash_covers_every_field` with it.
3. In `kermit/src/materialize.rs`, `| CacheState::Hit => return load_cached_yaml(&yml_path),` becomes `| CacheState::Hit => {},` (Task 4.2's Step 2). `cargo test -p kermit --bin kermit h2o_cache` must fail with ``a hit regenerated the cache``.
4. In `kermit/src/materialize.rs`, `run_h2o`'s last call `H2oGenerator::new(target, plan, seed, order)` becomes `H2oGenerator::new(target, plan, seed, H2oOrder::Shuffled)`. `cargo test -p kermit --bin kermit h2o_cache` must fail at ``assert_eq!(meta["order"], "sorted")`` after the forced run. (The transposed variant, `run_h2o(*seed, *scale, …)` in the `dispatch` arm, must fail the first `materialize` call: `KeyPlan::for_scale(108)` is an error.)

## Story 5: `bench gen h2o`

- **Developer:** intermediate-dev
- **Files:** `kermit/src/main.rs`, `kermit/tests/cli_gen_scale_range.rs`
- **Interfaces:** exposes the CLI `bench gen h2o --scale N [--seed S] [--order shuffled|sorted] --tag T [--output-dir PATH]`, which writes `h2o-<N>-<T>`; uses `GenTarget` (already in `kermit/src/main.rs` from SNAP's phase 2: `tag`, `output_dir` and `prepare`), `kermit_bench::{GeneratorSpec, H2oOrder, H2O_MIN_SCALE, DEFAULT_H2O_SEED}` (Stories 2 and 4), `kermit_generate::h2o::{H2oGenerator, KeyPlan}` (Story 3) and `kermit_generate::run`
- **Verify:** `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit h2o && nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_gen_scale_range`

This story adds the imperative `bench gen h2o` beside `bench gen snap`. kermit-bench has no clap dependency, so the binary declares `H2oOrderArg`, a `clap::ValueEnum` that `From`-maps onto `kermit_bench::H2oOrder`, and a unit test pins that its names equal the spec's. `GenSubcommand::H2o` takes `--scale` (range-checked in clap against `H2O_MIN_SCALE`, so a scale below upstream's floor is a usage error, exit 2, before anything runs), `--seed` (default `DEFAULT_H2O_SEED`), `--order` (default `shuffled`) and the shared `GenTarget` flags (`--tag`, `--output-dir`), which SNAP's phase 2 already landed and which this story uses without adding a second copy. `run_gen_h2o` names the benchmark `h2o_bench_name(scale, tag)`, validates the equivalent declarative spec (the floor and the name) before anything is written, prepares the output directory with `GenTarget::prepare("h2o", …)`, and runs `H2oGenerator` with no `spec_hash`, like every imperative `bench gen`. The seed and the order go to `meta.json`, not the name. Task 5.1 is the surface and its three unit tests; Task 5.2 is the CLI test that runs the real binary at a scale below the floor.

Line numbers below are those of the SNAP phase-2 tip (`aidanb/snap`, 74668ed), which this plan builds on; find each quote by its text.

### Task 5.1: `H2oOrderArg`, `GenSubcommand::H2o` and `run_gen_h2o`

**Files:**
- Modify: `kermit/src/main.rs:492-493`, `:613-616`, `:1399-1404`, `:1545-1548`, `:2080-2082`
- Test: `kermit/src/main.rs`

- [ ] **Step 1: Write the failing tests.** In `kermit/src/main.rs`, the tests module ends (lines 2080-2082) with the last assertion of `snap_flags_conflict_and_require_as_documented`:

  ```rust
          assert_eq!(names, ["triangle", "clique4"]);
      }
  }
  ```

  Insert between that test's closing `}` and the module's, in `kermit/src/main.rs`:

  ```rust

      /// The names clap accepts for `--order` are the names the YAML spec
      /// uses: kermit-bench has no clap dependency, so the binary spells them
      /// twice and this pins that the two agree, variant for variant, in
      /// order.
      #[test]
      fn h2o_clap_names_match_the_spec_names() {
          use clap::ValueEnum;

          let mut orders = Vec::new();
          for arg in H2oOrderArg::value_variants() {
              let clap_name = arg.to_possible_value().unwrap().get_name().to_string();
              assert_eq!(clap_name, kermit_bench::H2oOrder::from(*arg).name());
              orders.push(clap_name);
          }
          let spec_orders: Vec<&str> = kermit_bench::H2oOrder::ALL
              .iter()
              .map(|o| o.name())
              .collect();
          assert_eq!(orders, spec_orders);
      }

      /// `--scale` is required and floored at `H2O_MIN_SCALE`, `--seed` and
      /// `--order` default to upstream's 108 and `shuffled`, and a value
      /// outside any of them is refused at parse time, before the handler
      /// runs.
      #[test]
      fn h2o_flags_default_and_enforce_the_floor() {
          use kermit_bench::H2oOrder;

          let parse = |extra: &[&str]| {
              let mut argv = vec!["kermit", "bench", "gen", "h2o", "--tag", "t"];
              argv.extend_from_slice(extra);
              Cli::try_parse_from(argv)
          };
          let fields = |cli: Cli| {
              let Commands::Bench {
                  subcommand:
                      BenchSubcommand::Gen {
                          subcommand:
                              GenSubcommand::H2o {
                                  scale, seed, order, ..
                              },
                      },
                  ..
              } = cli.command
              else {
                  panic!("not a `bench gen h2o` command");
              };
              (scale, seed, H2oOrder::from(order))
          };

          let cli = parse(&["--scale", "10000000"]).expect("the floor itself parses");
          assert_eq!(fields(cli), (10_000_000, 108, H2oOrder::Shuffled));
          let cli = parse(&["--scale", "12000000", "--order", "sorted", "--seed", "7"])
              .expect("a seed and an order parse");
          assert_eq!(fields(cli), (12_000_000, 7, H2oOrder::Sorted));
          let cli = parse(&["--scale", "4294967295"]).expect("the largest u32 parses");
          assert_eq!(fields(cli).0, u32::MAX);

          // One below the floor is refused, and the error names the flag and
          // the floor.
          let Err(below) = parse(&["--scale", "9999999"]) else {
              panic!("--scale 9999999 was accepted");
          };
          let message = below.to_string();
          assert!(
              message.contains("--scale") && message.contains("10000000"),
              "{message}"
          );

          let rejected: &[&[&str]] = &[
              // zero, and no scale at all
              &["--scale", "0"],
              &[],
              // an order outside the enum, or spelled in another case
              &["--scale", "10000000", "--order", "random"],
              &["--scale", "10000000", "--order", "Sorted"],
          ];
          for args in rejected {
              assert!(parse(args).is_err(), "{args:?} should be rejected");
          }
      }

      /// `h2o-<scale>-<tag>`, as `bench gen lubm` names `lubm-<scale>-<tag>`.
      #[test]
      fn h2o_bench_name_is_scale_and_tag() {
          assert_eq!(h2o_bench_name(10_000_000, "dev"), "h2o-10000000-dev");
          assert_eq!(h2o_bench_name(12_000_000, "a-b"), "h2o-12000000-a-b");
      }
  ```

- [ ] **Step 2: Run it to see it fail.**

  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit h2o
  ```

  Expected: the binary's test target does not compile: ``error[E0433]: failed to resolve: use of undeclared type `H2oOrderArg` ``, ``error[E0599]`` errors naming the variant `H2o` of `GenSubcommand`, and ``error[E0425]: cannot find function `h2o_bench_name` in this scope``, then ``error: could not compile `kermit` (bin "kermit" test)``.

- [ ] **Step 3: Implement.** Four edits in `kermit/src/main.rs`, then the handler.

  `kermit/src/main.rs` current (lines 492-493), the start of the `bench gen` subcommands, which follows `SnapQueryArg`'s `From` impl:

  ```rust
  #[derive(Subcommand)]
  enum GenSubcommand {
  ```

  Insert above it, in `kermit/src/main.rs`:

  ```rust
  /// The CLI spelling of `kermit_bench::H2oOrder`; see [`SnapGraphArg`].
  /// `h2o_clap_names_match_the_spec_names` pins that the names agree.
  #[derive(Clone, Copy, clap::ValueEnum)]
  enum H2oOrderArg {
      /// Each relation's rows in the order they are drawn (upstream's)
      Shuffled,
      /// Each relation's rows sorted, the order the sorted tries build in
      Sorted,
  }

  impl From<H2oOrderArg> for kermit_bench::H2oOrder {
      fn from(arg: H2oOrderArg) -> Self {
          match arg {
              | H2oOrderArg::Shuffled => Self::Shuffled,
              | H2oOrderArg::Sorted => Self::Sorted,
          }
      }
  }

  ```

  `kermit/src/main.rs` current (lines 613-616), the end of the `Snap` subcommand and of the enum:

  ```rust
          #[command(flatten)]
          target: GenTarget,
      },
  }
  ```

  Replace with:

  ```rust
          #[command(flatten)]
          target: GenTarget,
      },

      /// Generate an H2O db-benchmark join benchmark (#95)
      H2o {
          /// N, the rows of the fact table `x` (at least 10000000, upstream's
          /// floor)
          #[arg(
              long,
              value_name = "N",
              value_parser = clap::value_parser!(u32).range(i64::from(kermit_bench::H2O_MIN_SCALE)..)
          )]
          scale: u32,

          /// Random seed (default: upstream's 108)
          #[arg(long, value_name = "SEED", default_value_t = kermit_bench::DEFAULT_H2O_SEED)]
          seed: u32,

          /// Row order of every relation
          #[arg(long, value_enum, default_value_t = H2oOrderArg::Shuffled)]
          order: H2oOrderArg,

          #[command(flatten)]
          target: GenTarget,
      },
  }
  ```

  `kermit/src/main.rs` current (lines 1399-1404), the end of `run_gen_snap` and the start of `main`:

  ```rust
      kermit_generate::run(&generator)?;
      eprintln!("snap benchmark written to {}", out.display());
      Ok(())
  }

  fn main() -> anyhow::Result<()> {
  ```

  Replace with:

  ```rust
      kermit_generate::run(&generator)?;
      eprintln!("snap benchmark written to {}", out.display());
      Ok(())
  }

  /// The name of a `bench gen h2o` benchmark: `h2o-<scale>-<tag>`, as `bench
  /// gen lubm` names `lubm-<scale>-<tag>`. The seed and the order go to
  /// `meta.json`, not the name.
  fn h2o_bench_name(scale: u32, tag: &str) -> String { format!("h2o-{scale}-{tag}") }

  /// Handler for `bench gen h2o`: generate the H2O db-benchmark join relations
  /// and queries through [`kermit_generate::h2o::H2oGenerator`].
  ///
  /// The benchmark is named by [`h2o_bench_name`]. The equivalent declarative
  /// spec is validated first (the floor and the name), before anything is
  /// written. The run has no `spec_hash`, like every imperative `bench gen`.
  fn run_gen_h2o(scale: u32, seed: u32, order: H2oOrderArg, target: GenTarget) -> anyhow::Result<()> {
      use {
          kermit_bench::{GeneratorSpec, H2oOrder},
          kermit_generate::h2o::{H2oGenerator, KeyPlan},
      };

      let order = H2oOrder::from(order);
      let bench_name = h2o_bench_name(scale, &target.tag);

      BenchmarkDefinition {
          name: bench_name.clone(),
          description: String::new(),
          relations: vec![],
          queries: vec![],
          generator: Some(GeneratorSpec::H2o {
              scale,
              seed,
              order,
          }),
      }
      .validate()?;

      let plan = KeyPlan::for_scale(scale)?;
      let out = target.prepare("h2o", &bench_name)?;
      let generator = H2oGenerator::new(
          kermit_rdf::generator::Target {
              out_dir: &out,
              bench_name: &bench_name,
              tag: &target.tag,
              spec_hash: None,
          },
          plan,
          seed,
          order,
      );
      kermit_generate::run(&generator)?;
      eprintln!("h2o benchmark written to {}", out.display());
      Ok(())
  }

  fn main() -> anyhow::Result<()> {
  ```

  `kermit/src/main.rs` current (lines 1545-1548), the `Snap` arm of `main`'s `bench gen` match and the closing of the matches:

  ```rust
                  } => run_gen_snap(graph, url, sha256, queries, target)?,
              },
          },
      }
  ```

  Replace with:

  ```rust
                  } => run_gen_snap(graph, url, sha256, queries, target)?,

                  | GenSubcommand::H2o {
                      scale,
                      seed,
                      order,
                      target,
                  } => run_gen_h2o(scale, seed, order, target)?,
              },
          },
      }
  ```

- [ ] **Step 4: Run it to see it pass.**

  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit h2o
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit --all-targets -- -D warnings
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all --check
  ```

  Expected: `test result: ok. 4 passed; 0 failed` for the filter `h2o` (`h2o_clap_names_match_the_spec_names`, `h2o_flags_default_and_enforce_the_floor`, `h2o_bench_name_is_scale_and_tag`, and Task 4.2's `materialize::tests::h2o_cache_misses_hits_drifts_and_regenerates`). Clippy and the format check are silent. If `cargo fmt --check` reports a diff (the `run_gen_h2o` signature is exactly 100 columns), run `cargo fmt --all` through the same `nix develop` prefix and re-check.

- [ ] **Step 5: Commit.**

  ```bash
  git commit -m "feat(kermit): bench gen h2o (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit/src/main.rs
  ```

### Task 5.2: The floor is a usage error

**Files:**
- Modify: `kermit/tests/cli_gen_scale_range.rs:1-6` and the end of the file
- Test: `kermit/tests/cli_gen_scale_range.rs`

- [ ] **Step 1: Write the failing test.** In `kermit/tests/cli_gen_scale_range.rs`, the module doc is currently (lines 1-6):

  ```rust
  //! Issue #78 row 7: `bench gen watdiv|lubm --scale 0` is a usage error,
  //! rejected by the CLI before anything runs. It used to reach the
  //! vendored generator — WatDiv aborted with `boost::bad_lexical_cast`, the
  //! LUBM jar rejected it — after creating an empty cache directory.
  //!
  //! Needs neither Java nor the WatDiv binary: clap rejects the flag first.
  ```

  Replace with:

  ```rust
  //! Issue #78 row 7: `bench gen watdiv|lubm --scale 0` is a usage error,
  //! rejected by the CLI before anything runs. It used to reach the
  //! vendored generator — WatDiv aborted with `boost::bad_lexical_cast`, the
  //! LUBM jar rejected it — after creating an empty cache directory.
  //!
  //! `bench gen h2o --scale` has a floor, `H2O_MIN_SCALE` (10000000, upstream's
  //! own), and a scale below it is the same usage error (#95).
  //!
  //! Needs neither Java nor the WatDiv binary: clap rejects the flag first.
  ```

  The file ends (lines 42-43) with

  ```rust
  #[test]
  fn lubm_scale_zero_is_a_usage_error() { assert_scale_zero_rejected("lubm"); }
  ```

  Append after it, in `kermit/tests/cli_gen_scale_range.rs`:

  ```rust

  /// `bench gen h2o --scale` is floored at upstream's 1e7: below it `small`
  /// (N / 1e6 keys) has no left-only or right-only key. Like `--scale 0`, it
  /// is refused by clap before the handler creates anything.
  #[test]
  fn h2o_scale_below_the_floor_is_a_usage_error() {
      let cache = TempDir::new().unwrap();
      let output = Command::new(kermit_bin())
          .env("XDG_CACHE_HOME", cache.path())
          .args(["bench", "gen", "h2o", "--scale", "1000", "--tag", "t"])
          .output()
          .unwrap();
      let stderr = String::from_utf8_lossy(&output.stderr);
      assert_eq!(
          output.status.code(),
          Some(2),
          "gen h2o --scale 1000 must be a clap usage error; stderr: {stderr}"
      );
      assert!(
          stderr.contains("'1000'") && stderr.contains("--scale") && stderr.contains("10000000"),
          "the error must name the flag, the value and the floor: {stderr}"
      );
      assert_eq!(
          std::fs::read_dir(cache.path()).unwrap().count(),
          0,
          "nothing may be written before the flag is validated"
      );
  }
  ```

- [ ] **Step 2: Run it to see it fail.** Task 5.1 is in, so the test passes as written; check that it can fail by removing the floor from the flag. Copy `kermit/src/main.rs` aside, delete the `value_parser` line of `GenSubcommand::H2o`'s `scale` and the comma before it, and run:

  ```bash
  : "${SCRATCH:?set SCRATCH to this session's scratchpad (Ground rules)}"
  cp kermit/src/main.rs "$SCRATCH/main.rs.orig" &&
  python3 -c 'p="kermit/src/main.rs"; s=open(p).read(); old=",\n            value_parser = clap::value_parser!(u32).range(i64::from(kermit_bench::H2O_MIN_SCALE)..)\n"; assert s.count(old)==1; open(p,"w").write(s.replace(old,"\n"))' &&
  git diff --stat -- kermit/src/main.rs &&
  { nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_gen_scale_range h2o; cp "$SCRATCH/main.rs.orig" kermit/src/main.rs; }
  cmp "$SCRATCH/main.rs.orig" kermit/src/main.rs
  ```

  Expected: the `assert` in the script holds (the mutant applied), and `git diff --stat` shows `kermit/src/main.rs` changed. Without the range `--scale 1000` reaches `run_gen_h2o`, whose validation refuses it, so the process exits 1: the test fails with ``gen h2o --scale 1000 must be a clap usage error`` and ``left: Some(1)`` / ``right: Some(2)``. The restore's `cmp` prints nothing.

- [ ] **Step 3: Implement.** Nothing to implement: Task 5.1's flag is the implementation, and Step 2 restored it.

- [ ] **Step 4: Run it to see it pass.**

  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_gen_scale_range
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit --all-targets -- -D warnings
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all --check
  ```

  Expected: `test result: ok. 3 passed; 0 failed` (`watdiv_scale_zero_is_a_usage_error`, `lubm_scale_zero_is_a_usage_error` and `h2o_scale_below_the_floor_is_a_usage_error`). Clippy and the format check are silent. The error clap prints for the new case reads ``error: invalid value '1000' for '--scale <N>': 1000 is not in 10000000..=4294967295``.

- [ ] **Step 5: Commit.**

  ```bash
  git commit -m "test(kermit): bench gen h2o below the scale floor is a usage error (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit/tests/cli_gen_scale_range.rs
  ```

### Mutation checks

For the manager, after Task 5.2. Each mutant is one edit; copy the file aside first, confirm with `git diff` that the edit applied, run the named test, then restore with `cp` and `cmp` (never `git checkout`).

1. In `kermit/src/main.rs`, remove `value_parser = clap::value_parser!(u32).range(i64::from(kermit_bench::H2O_MIN_SCALE)..)` from `GenSubcommand::H2o`'s `scale` (Task 5.2's Step 2). `cargo test -p kermit --bin kermit h2o_flags_default_and_enforce_the_floor` must fail (``--scale 9999999 was accepted``), and `cargo test -p kermit --test cli_gen_scale_range h2o` must fail on the exit code (1, not 2).
2. In `kermit/src/main.rs`, swap the arms of `impl From<H2oOrderArg> for kermit_bench::H2oOrder` (`H2oOrderArg::Shuffled => Self::Sorted` and the reverse). `cargo test -p kermit --bin kermit h2o_clap_names_match_the_spec_names` must fail, and so must `h2o_flags_default_and_enforce_the_floor` (the default order maps to `Sorted`).
3. In `kermit/src/main.rs`, `default_value_t = kermit_bench::DEFAULT_H2O_SEED` becomes `default_value_t = 0` (or `default_value_t = H2oOrderArg::Shuffled` becomes `H2oOrderArg::Sorted`). `cargo test -p kermit --bin kermit h2o_flags_default_and_enforce_the_floor` must fail on the `(10_000_000, 108, H2oOrder::Shuffled)` assertion.
4. In `kermit/src/main.rs`, `format!("h2o-{scale}-{tag}")` becomes `format!("h2o-{tag}-{scale}")`. `cargo test -p kermit --bin kermit h2o_bench_name_is_scale_and_tag` must fail.

## Story 6: Committed YAMLs and the end-to-end test

- **Developer:** intermediate-dev
- **Files:** `benchmarks/h2o.yml`, `benchmarks/h2o-sorted.yml`, `kermit/tests/h2o_generate.rs`
- **Interfaces:** none exposed (two YAMLs and an integration test); uses `kermit_generate::{run, h2o::{H2oGenerator, KeyPlan}}` (Stories 1 and 3), `kermit_bench::{H2oOrder, GeneratorSpec::H2o}` (Stories 2 and 4), `kermit_bench::discovery::load_benchmark`, and `common::utils::{join_under_planner, load_parquet_relations, JoinEntry}` (in the tree)
- **Verify:** `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test h2o_generate`

This story commits the two reference benchmarks and proves the generator end to end. `benchmarks/h2o.yml` and `benchmarks/h2o-sorted.yml` declare `kind: h2o` at `scale: 10000000`, the floor, in the two row orders; `bench run h2o` generates and caches them on first use. `kermit/tests/h2o_generate.rs` follows `snap_generate.rs` and lives in the `kermit` crate for the same reason: only this crate has both the generator and the join engine. The 1e7 floor stops any small `bench gen h2o`, so the test calls `kermit_generate::run` itself on the reference plan (`KeyPlan::new(2000, 20, 200)`, seed 108), in both orders, then runs every query on the three (structure, algorithm) cells (`TreeTrie` and `ColumnTrie` under `LeapfrogTriejoin`, `HashTrie` under `HashTriejoin`) under every optimiser and both column-order policies through the real `kermit::db` entry points. Every count must equal the `expected` the generator recorded, and a separate test pins those `expected` values to the design session's independent Python model (1812, 1814, 1800), so a generator whose own bitmap count drifts with its data cannot pass. A third test checks that the committed YAMLs parse, validate and declare the right spec. The `ExecutionFamily::load` path that `bench run` takes is covered by Story 8's manual `--verify` at 1e7, not here. The story touches no join path and no data structure, so the supervisor needs no `effort: xhigh` dispatch; the shapes a naive suite would miss (an order mix-up in a YAML, an `expected` that is off by one, a vacuous zero count) each have a mutation check at the end.

### Task 6.1: The end-to-end test and the committed YAMLs

**Files:**
- Create: `kermit/tests/h2o_generate.rs`
- Create: `benchmarks/h2o.yml`
- Create: `benchmarks/h2o-sorted.yml`
- Test: `kermit/tests/h2o_generate.rs`

- [ ] **Step 1: Write the failing test.** Create `kermit/tests/h2o_generate.rs` with exactly this content. The YAML test fails until Step 3; the other two exercise the generator that Stories 1 to 5 landed.

```rust
//! End-to-end H2O check (#95): the reference plan goes through
//! `kermit_generate::run` in both row orders, then every query runs on the
//! three (structure, algorithm) cells under every optimiser and column-order
//! policy through the real `kermit::db` entry points, and must return the
//! count the generator recorded as `expected`.
//!
//! It lives in the `kermit` crate for the reason `snap_generate.rs` does: only
//! this crate has both the generator and the join engine, and it needs nothing
//! of the binary. The 1e7 floor stops any small `bench gen h2o`, so the data
//! comes from `kermit_generate::run` directly, and the `ExecutionFamily::load`
//! path that `bench run` takes is covered by the manual `--verify` acceptance
//! at 1e7. The cells are `TreeTrie` and `ColumnTrie` under `LeapfrogTriejoin`,
//! and `HashTrie` under `HashTriejoin`; the shared `JoinEntry` driver in
//! `common/utils.rs` hosts both families.
//!
//! # The reference plan
//!
//! `KeyPlan::new(2000, 20, 200)` with seed 108: N = 2,000 rows of `x`, `small`
//! with 20 keys and `medium` with 200. `REFERENCE_COUNTS` pins the counts
//! against a Python model written from the spec alone, which shares no code
//! with the generator: q5 is N - N / 10 = 1800 by construction (`x_id3` is a
//! permutation, so the share of its keys that `big` holds does not depend on
//! the draws), and q1 and q2 (1812 and 1814) depend on every draw. The joins
//! and the generator's own bitmap count both read the generated data, so only
//! a pin like this fails when the data and its count drift together.

mod common;

use {
    clap::ValueEnum,
    common::utils::{join_under_planner, load_parquet_relations, JoinEntry},
    kermit_algos::{
        ColumnOrderPolicy, HashTriejoin, JoinQuery, LeapfrogTriejoin, Optimiser, Planner,
    },
    kermit_bench::{discovery::load_benchmark, BenchmarkDefinition, GeneratorSpec, H2oOrder},
    kermit_ds::{Cardinality, ColumnTrie, GallopingSeek, HashTrie, Relation, TreeTrie},
    kermit_generate::h2o::{H2oGenerator, KeyPlan},
    kermit_iters::SipHashStrategy,
    kermit_rdf::generator::Target,
    std::{
        fs,
        path::{Path, PathBuf},
    },
};

/// The benchmark name every generation here uses. `kermit_generate::run`
/// requires the output directory's last component to equal it.
const BENCH_NAME: &str = "h2o-test";

/// Upstream's seed (`set.seed(108)`), which the committed YAMLs default to.
const SEED: u32 = 108;

/// The reference plan's answer counts, in query order, from the independent
/// model (see the module doc).
const REFERENCE_COUNTS: [(&str, u64); 3] = [("q1", 1812), ("q2", 1814), ("q5", 1800)];

/// The relations of the emitted `benchmark.yml`, in write order.
const RELATIONS: [&str; 6] = ["x_id3", "big", "x_id1", "small", "x_id2", "medium"];

// The three cells' relation types. `TreeTrie` and `ColumnTrie` default to
// `GallopingSeek`; the alias names it so no inference fallback is needed.
type Tree = TreeTrie<GallopingSeek>;
type Column = ColumnTrie<GallopingSeek>;
type Hash = HashTrie<SipHashStrategy>;

/// A generation of the reference plan into `<root>/h2o-test`.
struct Generation {
    /// Keeps the temp dir alive.
    _root: tempfile::TempDir,
    /// The output directory (a cache subdirectory in all but location).
    dir: PathBuf,
}

/// Runs the generator on the reference plan in `order`.
fn generate_reference(order: H2oOrder) -> Generation {
    let root = tempfile::tempdir().expect("create temp dir");
    let dir = root.path().join(BENCH_NAME);
    let plan = KeyPlan::new(2000, 20, 200).expect("the reference plan is valid");
    {
        let generator = H2oGenerator::new(
            Target {
                out_dir: &dir,
                bench_name: BENCH_NAME,
                tag: "test",
                spec_hash: None,
            },
            plan,
            SEED,
            order,
        );
        kermit_generate::run(&generator).expect("the reference plan must generate");
    }
    Generation { _root: root, dir }
}

/// Parses the `benchmark.yml` a generation emitted into `dir`.
fn emitted_benchmark(dir: &Path) -> BenchmarkDefinition {
    let yaml =
        fs::read_to_string(dir.join("benchmark.yml")).expect("emitted benchmark.yml missing");
    serde_yaml::from_str(&yaml).expect("benchmark.yml malformed")
}

/// Each emitted query's name and `expected`, in order.
fn emitted_counts(bench: &BenchmarkDefinition) -> Vec<(String, u64)> {
    bench
        .queries
        .iter()
        .map(|q| {
            let expected = q
                .expected
                .unwrap_or_else(|| panic!("query {} has no expected count", q.name));
            (q.name.clone(), expected)
        })
        .collect()
}

/// Loads the generated relations as `R` into a fresh engine planned by
/// `planner`, runs every query through `JA`'s entry points (building the
/// reordered copies it reads under `--column-orders any`), and returns one
/// line per query whose result count differs from its `expected`. A join that
/// errors panics, naming the cell: under `stored` that is the
/// `CyclicAttributeOrder` check, which no H2O query may trip.
fn cell_mismatches<R, JA>(
    bench: &BenchmarkDefinition, dir: &Path, planner: &Planner, label: &str,
) -> Vec<String>
where
    R: Relation + Cardinality,
    JA: JoinEntry<R>,
{
    let (relations, inputs) =
        load_parquet_relations::<R>(dir, bench.relations.iter().map(|r| r.name.as_str()));
    let mut database = JA::database(relations, planner.required_statistics());

    let mut mismatches = Vec::new();
    for q in &bench.queries {
        let want = q.expected.expect("every H2O query carries expected");
        let parsed: JoinQuery = q.query.parse().expect("datalog parse failure");
        let (rows, _) = join_under_planner::<R, JA>(&mut database, &inputs, parsed, planner)
            .unwrap_or_else(|e| {
                panic!("[{label}] {} / {}: {e}", std::any::type_name::<R>(), q.name)
            });
        let got = rows.len() as u64;
        if got != want {
            mismatches.push(format!(
                "  [{label}] {} / {}: got {got}, expected {want}\n    query: {}",
                std::any::type_name::<R>(),
                q.name,
                q.query
            ));
        }
    }
    mismatches
}

/// The committed YAMLs declare what their names say, at the floor, and
/// validate (`load_benchmark` validates).
#[test]
fn committed_yamls_declare_h2o_at_the_floor() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kermit is inside the workspace");
    for (name, order) in [("h2o", H2oOrder::Shuffled), ("h2o-sorted", H2oOrder::Sorted)] {
        let def = load_benchmark(root, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            def.generator,
            Some(GeneratorSpec::H2o {
                scale: 10_000_000,
                seed: 108,
                order,
            }),
            "{name}"
        );
        assert!(
            def.description.contains(order.name()),
            "{name}: description {:?} should say its rows are {}",
            def.description,
            order.name()
        );
        assert!(def.relations.is_empty() && def.queries.is_empty(), "{name}");
    }
}

/// The emitted `expected` values are the independent model's, in both
/// orders, and the relations are the six the queries need, in write order.
#[test]
fn the_emitted_counts_are_the_reference_models() {
    let want: Vec<(String, u64)> = REFERENCE_COUNTS
        .iter()
        .map(|&(name, count)| (name.to_string(), count))
        .collect();
    for order in H2oOrder::ALL {
        let generation = generate_reference(order);
        let bench = emitted_benchmark(&generation.dir);
        assert_eq!(emitted_counts(&bench), want, "{} rows", order.name());
        let relations: Vec<&str> = bench.relations.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(relations, RELATIONS, "{} rows", order.name());
    }
}

/// Every query returns its `expected` count on every cell, under every
/// optimiser and both column-order policies, in both row orders.
///
/// The plan an optimiser picks changes the join's descent order, never its
/// answer; iterating the CLI enums covers optimisers and policies added later
/// with no edit here. Under `stored` all three queries must validate for every
/// optimiser, which pins that each is written with its variables in the stored
/// column order. Under `any` the copies a plan needs are the planner's
/// business and are not asserted, only the answers.
#[test]
fn every_cell_optimiser_and_policy_returns_the_expected_counts() {
    let optimisers = Optimiser::value_variants();
    let policies = ColumnOrderPolicy::value_variants();
    let mut mismatches: Vec<String> = Vec::new();
    let mut joins = 0;
    for order in H2oOrder::ALL {
        let generation = generate_reference(order);
        let bench = emitted_benchmark(&generation.dir);
        assert_eq!(bench.queries.len(), REFERENCE_COUNTS.len());
        assert!(
            bench
                .queries
                .iter()
                .all(|q| q.expected.is_some_and(|count| count > 0)),
            "a zero expected count would make the comparison vacuous: {:?}",
            emitted_counts(&bench)
        );

        for &policy in policies {
            for &optimiser in optimisers {
                let planner = Planner::new(optimiser.instantiate(), policy);
                let label = format!(
                    "{} / {} / {}",
                    order.name(),
                    optimiser.axis_value(),
                    policy.axis_value()
                );
                let dir = &generation.dir;
                mismatches.extend(cell_mismatches::<Tree, LeapfrogTriejoin>(
                    &bench, dir, &planner, &label,
                ));
                mismatches.extend(cell_mismatches::<Column, LeapfrogTriejoin>(
                    &bench, dir, &planner, &label,
                ));
                mismatches.extend(cell_mismatches::<Hash, HashTriejoin>(
                    &bench, dir, &planner, &label,
                ));
                joins += 3 * bench.queries.len();
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "H2O cardinality mismatches ({} of {joins} joins; {} orders x {} optimisers x {} \
         column-order policies x 3 cells x {} queries):\n{}",
        mismatches.len(),
        H2oOrder::ALL.len(),
        optimisers.len(),
        policies.len(),
        REFERENCE_COUNTS.len(),
        mismatches.join("\n"),
    );
}
```

- [ ] **Step 2: Run it to see it fail.**

```bash
nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test h2o_generate
```

Expected: the file compiles (Stories 1 to 5 supply every name it uses), and the run ends `test result: FAILED. 2 passed; 1 failed`. The failure is `committed_yamls_declare_h2o_at_the_floor`, which panics with `h2o: benchmark not found: h2o` because `benchmarks/h2o.yml` does not exist yet. The other two tests pass.

- [ ] **Step 3: Implement.** Create the two YAMLs. `benchmarks/h2o.yml`:

```yaml
name: h2o
description: "H2O db-benchmark join (J1) at N = 1e7: q1, q2 and q5 over per-query projections of x, rows shuffled (#95)."
generator:
  kind: h2o
  scale: 10000000
```

`benchmarks/h2o-sorted.yml`:

```yaml
name: h2o-sorted
description: "H2O db-benchmark join (J1) at N = 1e7: q1, q2 and q5 over per-query projections of x, rows sorted, #101's input-order axis (#95)."
generator:
  kind: h2o
  scale: 10000000
  order: sorted
```

- [ ] **Step 4: Run it to see it pass.**

```bash
nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test h2o_generate
nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit --test h2o_generate -- -D warnings
nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all --check
```

Expected: `test result: ok. 3 passed; 0 failed`; clippy and the format check are clean. If `cargo fmt --check` reports a diff in the new test (the import block is the likely place), run `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all` and re-check. Then run the mutation checks below before committing.

- [ ] **Step 5: Commit.**

```bash
git add -N benchmarks/h2o.yml benchmarks/h2o-sorted.yml kermit/tests/h2o_generate.rs
git commit -m "test(kermit): H2O end-to-end test and the committed 1e7 YAMLs (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- benchmarks/h2o.yml benchmarks/h2o-sorted.yml kermit/tests/h2o_generate.rs
```

### Mutation checks

For the manager, after Step 4 and before Step 5. Each mutant is applied to a tracked or just-created file, so copy the file aside first (`cp <file> "$SCRATCH/orig"`), restore it with `cp "$SCRATCH/orig" <file>` after the run, and check the restore with `cmp` (silent). Assert that the edit applied (`git diff --stat` or `grep` shows it) before trusting a failure, and never use `git checkout`: the story's files are uncommitted. All runs use `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test h2o_generate`.

1. **A committed YAML in the wrong order.** In `benchmarks/h2o-sorted.yml`, replace the line `  order: sorted` with `  order: shuffled`. `committed_yamls_declare_h2o_at_the_floor` must fail, with an `assert_eq!` diff showing `order: Shuffled` against `order: Sorted` and the message `h2o-sorted`. The other two tests pass.
2. **A generator `expected` that is off by one.** In `kermit-generate/src/h2o/mod.rs`, in `H2oGenerator::generate`, change `.zip([q1, q2, q5])` to `.zip([q1 + 1, q2, q5])`, so only q1's `expected` is one too high. `the_emitted_counts_are_the_reference_models` must fail on the shuffled pass with `left: [("q1", 1813), ("q2", 1814), ("q5", 1800)]` against the model's 1812. `every_cell_optimiser_and_policy_returns_the_expected_counts` must fail too, with 36 mismatch lines for `q1` (2 orders x 3 optimisers x 2 policies x 3 cells), each reading `got 1812, expected 1813`.
3. **Drifting data, self-consistent counts.** In `kermit-generate/src/h2o/rng.rs`, replace `const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;` with `const GAMMA: u64 = 0x9e37_79b9_7f4a_7c16;`. Every stream changes, so the data changes and the generator's own counts stay consistent with it. `the_emitted_counts_are_the_reference_models` must fail (q1 and q2 move off 1812 and 1814; q5 stays 1800 by construction), while `every_cell_optimiser_and_policy_returns_the_expected_counts` must still pass: only the independent pin can see this mutant. Story 1's and Story 3's pinned-value tests fail as well, which this run does not execute.

Restore each file, `cmp` it against the copy, then re-run the three tests once more: `test result: ok. 3 passed; 0 failed`.

## Story 7: Docs (no measured values)

- **Developer:** junior-dev
- **Files:** `docs/benchmarks/H2O.md`, `docs/benchmarks/README.md`, `benchmarks/README.md`, `USAGE.md`, `README.md`, `CLAUDE.md`, `ARCHITECTURE.md`
- **Interfaces:** documents the H2O generator, spec and CLI from Stories 1 to 6; no code
- **Verify:** `for f in docs/benchmarks/H2O.md USAGE.md README.md CLAUDE.md; do grep -qF 'bench gen h2o' "$f" || echo "missing 'bench gen h2o' in $f"; done; for f in docs/benchmarks/README.md benchmarks/README.md CLAUDE.md; do grep -qF 'H2O.md' "$f" || echo "missing H2O.md in $f"; done; grep -qF 'snap | h2o' ARCHITECTURE.md || echo "missing h2o in ARCHITECTURE.md"; grep -qF '## Measured at N = 1e7' docs/benchmarks/H2O.md || echo "missing stub section"`

Exact new text and exact before/after Markdown edits; no code, no cargo. The new `docs/benchmarks/H2O.md` follows the schema in `docs/benchmarks/README.md` (the order of required sections is fixed, and `## Inference / preprocessing` and `## Practical scale ceiling` take their slots), in the register of `SNAP.md`, and it carries no measured value: q1 and q2 at N = 1e7 and every space figure are Story 8's, which replaces the one-sentence `## Measured at N = 1e7` stub. The facts it states come from the spec and the names contract: upstream's 1e7 counts, kermit's q5 at 1e7, the reference plan's counts, the floor and its reason, the load cost, the generation peak and the `RowId::MAX` ceiling. The other six files gain `h2o` at each place SNAP's phase 2 (c8e4505) added `snap`. Line numbers in the edits are those of the SNAP tip (74668ed, `aidanb/snap`), which this branch has after merging `origin/master`. They are the pre-edit numbers of each file, so find each edit by its quoted text, not its number, once an earlier edit in the same file has moved the lines; before each edit, check that the quoted text is unique with `grep -cF '<text>' <file>` (expect `1`).

### Task 7.1: docs/benchmarks/H2O.md

**Files:**
- Create: `docs/benchmarks/H2O.md`

- [ ] **Step 1: Write the failing check.**

```bash
test -f docs/benchmarks/H2O.md && echo present || echo absent
```

- [ ] **Step 2: Run it to see it fail.** It prints `absent`.

- [ ] **Step 3: Implement.** Create `docs/benchmarks/H2O.md` with exactly this content.

````markdown
# H2O (db-benchmark join tasks)

H2O.ai's db-benchmark is a public comparison of data-frame and database engines. Its
join task ("J1") joins one fact table `x` of N rows, with integer keys `id1`, `id2` and
`id3`, to three right-hand tables, `small` (N / 10⁶ keys), `medium` (N / 10³ keys) and
`big` (N keys). Kermit regenerates that data itself and runs three of upstream's join
questions, q1, q2 and q5, as Datalog joins. They are acyclic key/foreign-key joins, the
classic hash-join workload, so H2O is the thesis's relational baseline beside the cyclic
SNAP graphs. q1 is almost pure leaf enumeration (a few keys with about a million children
each at N = 1e7), and q5 intersects two dense key lists that overlap by 90%, which
separates the seek strategies (#80). N is a scaling knob, and the `order` field writes
the same data shuffled or sorted: a controlled input-order axis on synthetic data for
#101. Every query carries an exact `expected` count, so `--verify` checks it.

Unlike LUBM and WatDiv, H2O has no RDF and no external generator, and unlike SNAP it
downloads nothing: a seeded generator in the repo (`kermit-generate`,
`kermit-generate/src/h2o/`) writes the Parquet files itself. For a side-by-side
comparison with the other benchmarks in the suite, see
[`README.md`](README.md#cross-benchmark-comparison).

## How to run

### On-the-fly generation

```bash
kermit bench gen h2o --scale N [--seed S] [--order shuffled|sorted] \
    --tag STR [--output-dir PATH]
```

Writes `~/.cache/kermit/benchmarks/h2o-<N>-<tag>/` and prints
`h2o benchmark written to <dir>` on stderr. Run it with the unmodified `kermit bench run`:

```bash
kermit bench gen h2o --scale 10000000 --tag dev
kermit bench run h2o-10000000-dev -i all -a all --verify
```

The bench name is collision-checked against committed YAMLs at runtime. Seed and order
go to `meta.json`, not to the name, so two generations that differ only in them need
different tags.

| Flag | Default | Notes |
|------|---------|-------|
| `--scale` | required | N, the rows of `x`. At least 10000000 (upstream's floor, see [Inference / preprocessing](#inference--preprocessing)); a smaller value is a usage error that exits 2 and names `--scale` and 10000000 |
| `--seed` | 108 | Upstream's `set.seed(108)`. Any `u32` |
| `--order` | `shuffled` | `shuffled` (the order the generator draws rows, as upstream writes them) or `sorted` (each relation's rows sorted; see [Row order](#row-order-the-order-axis)) |
| `--tag` | required | Suffix on the bench name |
| `--output-dir` | `~/.cache/kermit/benchmarks` | Custom output dirs are NOT auto-discovered by `bench list/fetch/run` |

### Declarative YAML spec (commit-and-run)

`benchmarks/h2o.yml` and `benchmarks/h2o-sorted.yml` are committed. `bench run h2o`
generates and caches the data on first invocation:

```yaml
# benchmarks/h2o.yml
name: h2o
description: "H2O db-benchmark join (J1) at N = 1e7: q1, q2 and q5 over per-query projections of x, rows shuffled (#95)."
generator:
  kind: h2o
  scale: 10000000
  # seed: 108          # optional; upstream's set.seed(108) by default
  # order: shuffled    # optional; `shuffled` (default) or `sorted`
```

`h2o-sorted.yml` is the same with `name: h2o-sorted`, a description ending
`rows sorted, #101's input-order axis (#95).` and `order: sorted`.

Required: `scale`, at least 10000000. Optional: `seed` (default 108) and `order`
(`shuffled`, the default, or `sorted`). An unknown `order` is a parse error that lists
the valid names, and `BenchmarkDefinition::validate` rejects a `scale` below the floor.
All three fields feed the spec hash, and an omitted `seed` hashes the same as an
explicit `seed: 108`.

Editing the YAML and re-running errors with `SpecDrift` — pass `--force` to opt into
regenerating. See `benchmarks/README.md` for the full schema. Both YAMLs are N = 1e7
benchmarks, so `bench run --all` generates and runs them too.

## Pipeline

```
(scale, seed, order)             KeyPlan::for_scale: N, N / 10⁶, N / 10³
         │
         │ kermit-generate::h2o::rng   SplitMix64, one stream per (column, step)
         │ kermit-generate::h2o::keys  KeyPool: pool, x's column, right table, matches
         ▼
  for id3, then id1, then id2:
  x's column ──► count of rows with a shared key        right-hand table
         │                 │                                   │
         │ RelationSink::write (rows sorted first under order: sorted)
         ▼                 │                                   ▼
  x_id3 / x_id1 / x_id2    │                         big / small / medium
  .parquet                 ▼                         .parquet
         └──────► expected for q5 / q1 / q2 ◄───────────────┘
                           │
                           ▼
                benchmark.yml + meta.json
```

The three key columns are generated one at a time and dropped as soon as their
relations are written, which bounds the peak memory (see
[Practical scale ceiling](#practical-scale-ceiling)). The counts are taken from the
unsorted columns, by a bitmap that reads no `kermit-ds` or `kermit-algos` structure.

## Output layout

```
~/.cache/kermit/benchmarks/h2o-10000000-dev/
  meta.json        kind = "h2o"; written last, so its presence marks a finished cache
  benchmark.yml    six relations (file:// URL + SHA-256) and q1, q2, q5, each with `expected`
  x_id3.parquet    x_id3(id3): N rows
  big.parquet      big(id3): N rows
  x_id1.parquet    x_id1(id1, id3): N rows
  small.parquet    small(id1): N / 10⁶ rows
  x_id2.parquet    x_id2(id2, id3): N rows
  medium.parquet   medium(id2): N / 10³ rows
```

There is no `raw/` directory: nothing is downloaded. A declarative spec writes the same
layout into a directory named after the YAML (`h2o`). `meta.json` has the fields every
generated benchmark has (`kind`, `tag`, `generated_at_utc`, `spec_hash` for a
declarative run, and each relation's name, columns, rows and SHA-256) plus H2O's own:

| Field | Meaning |
|-------|---------|
| `seed` | The seed every random stream was drawn from |
| `order` | `shuffled` or `sorted`, the row order of every relation |
| `plan` | The cardinalities: `rows` (N), `small` and `medium` (the key counts of those two tables) |

## Inference / preprocessing

H2O has no input data: the generator draws it from the plan `(N, small, medium)`, where
`small` = N / 10⁶ and `medium` = N / 10³ (integer division), `big` has N keys and the
fact table has N rows.

### Key generation

For each key column (`id1`, `id2`, `id3`) with cardinality n (`small`, `medium` or N):

1. **Pool.** The keys 1..=n + n / 10, shuffled. The first n − n / 10 are *shared*, the
   next n / 10 are *left-only* (only `x` holds them) and the last n / 10 are *right-only*
   (only the right-hand table holds them). The integer split replaces R's `n * 0.9`, so
   every N is well-defined.
2. **`x`'s column** (upstream's `sample_all`). The first n keys of the pool (shared and
   left-only), then N − n draws from them, uniformly, then a shuffle. Every shared and
   left-only key appears at least once, and no right-only key does. For `id3`, n = N, so
   there is no padding and the column is a permutation.
3. **The right-hand table.** The shared keys and the right-only keys, shuffled: exactly n
   unique keys.
4. **The count.** The number of `x`'s rows whose key is shared, tested with a bitmap over
   0..=n + n / 10. That is the query's `expected`.

At N = 1e7, `small` has n = 10 keys (9 shared, 1 left-only, 1 right-only), `medium` has
10,000 (9,000 shared) and `big` has 10⁷ (9,000,000 shared). q5 is the `id3` count, and
`x`'s `id3` column is a permutation of the shared and left-only keys, so q5 = N − N / 10
for every seed: 9,000,000 at N = 1e7, as upstream's.

**The floor.** `scale` must be at least 10000000, upstream's floor. Below it, `small`
(N / 10⁶ keys) has fewer than 10 keys, so n / 10 = 0 and it has no left-only or
right-only key, which would make q1 a join in which every `x` row matches. The floor is a
property of the spec, not of the generator: `KeyPlan::new` accepts any plan with
1 ≤ small ≤ rows and 1 ≤ medium ≤ rows (and rows ≤ `RowId::MAX`), and the tests use
small plans (2,000 / 20 / 200, and one with no left-only key).

### Differences from upstream

The generator is a reimplementation of db-benchmark's `_data/join-datagen.R` that keeps
its structure, not a copy of it. It differs in these ways:

- **Integer split.** n − n / 10 shared keys instead of R's `n * 0.9`, so every N is
  well-defined.
- **Per-query projections.** Not one `x(id1, id2, id3)` but `x_id1(id1, id3)`,
  `x_id2(id2, id3)` and `x_id3(id3)`. A `_` is still a trie level, so a shared `x` would
  descend its unused columns on every query, under either column-order policy.
- **The carried `id3`.** Each `x` projection carries `id3`, which is unique per row.
  Every result row is therefore distinct and no relation holds a duplicate, so
  `expected` is the same for all three cells, HashTrie's multiset count included.
- **Dropped.** q3 (a left outer join), q4 (string keys, which under dictionary encoding
  are q2), the `v1`/`v2` payloads, the right-hand tables' extra columns and NAs.
- **The random-number generator.** Upstream draws from R's. Kermit's is in-repo
  (below), so its draws, and with them q1's and q2's counts, differ from upstream's. At
  N = 1e7 upstream's counts are q1 = 8,997,136, q2 = 9,001,606 and q5 = 9,000,000.
  Kermit's q5 is the same 9,000,000, by construction; its q1 and q2 land near 9.0
  million like upstream's but do not equal them. The values are recorded under
  [Measured at N = 1e7](#measured-at-n--1e7).

### Random numbers

`kermit-generate/src/h2o/rng.rs` holds the generator, in-repo because `rand`'s `StdRng`
and its sampling algorithms are not value-stable across versions, and a generator's
output must be a pure function of its spec (#74):

- **SplitMix64** (Steele, Lea and Flood, 2014; Vigna's `splitmix64.c`), tested against
  the reference implementation's output.
- **Bounded draws** by Lemire's nearly divisionless method (2019): unbiased values in
  `[0, n)` from a 128-bit multiply.
- **Shuffles** by Fisher–Yates (Durstenfeld, 1964): for `i` from `len − 1` down to 1,
  swap `i` with a draw in `[0, i]`.
- **Streams.** Every (column, step) pair has its own generator. The columns are `id1`,
  `id2`, `id3` (0, 1, 2) and the steps `Pool`, `Pad`, `LeftShuffle`, `RightShuffle`
  (0 to 3); a stream's state starts at SplitMix64's output function applied to
  `seed << 32 | (column × 4 + step)`. That input is injective and the function is a
  bijection, so each stream starts at its own point on the 2⁶⁴ cycle, and each
  relation's data depends only on `(plan, seed, order)`, not on the order the relations
  are generated or written in. The numbering is fixed: renumbering a column or a step
  changes every relation the generator writes.

### Row order (the `order` axis)

- **`shuffled`** (the default): rows are in the order the generator draws them, as
  upstream writes them.
- **`sorted`**: each relation's rows are sorted lexicographically (`Tuples::sort`, the
  sort both sorted tries build with) just before they are written.

The draws and the counts are the same in both modes, so a sorted relation equals its
shuffled counterpart sorted, and `expected` is identical. `x_id1` sorted is grouped by
`id1`, with ascending `id3` inside each group. HashTrie builds from the order its rows
arrive in, so the same data in the two orders separates a structure's cost from the
cost of input locality (#101). The order is a field of the spec, so the two orders are
two benchmarks with two caches (`h2o` and `h2o-sorted`).

## Workload reference

### The relations

| Relation | Columns | Rows | Holds |
|----------|---------|------|-------|
| `x_id3` | `id3` | N | `x`'s `id3` column: a permutation of the shared and left-only keys |
| `big` | `id3` | N | `big`'s keys: shared and right-only |
| `x_id1` | `id1`, `id3` | N | `x`'s `id1` column paired with its `id3` column |
| `small` | `id1` | N / 10⁶ | `small`'s keys |
| `x_id2` | `id2`, `id3` | N | `x`'s `id2` column paired with its `id3` column |
| `medium` | `id2` | N / 10³ | `medium`'s keys |

This is also the order the files are written and the order of `benchmark.yml`'s
relations. Keys are small positive integers, used as `usize` directly: there is no
dictionary.

### The queries

| Name | Datalog | Isolates |
|------|---------|----------|
| `q1` | `Q(K, I) :- small(K), x_id1(K, I).` | Leaf enumeration: 9 matching keys with about a million children each at 1e7. Upstream's "small inner on int" |
| `q2` | `Q(K, I) :- medium(K), x_id2(K, I).` | About 9,000 matching keys with about a thousand children each. Upstream's "medium inner on int" |
| `q5` | `Q(K) :- big(K), x_id3(K).` | A dense intersection of two key lists that overlap by 90%, which separates the seek strategies (#80). Upstream's "big inner on int" |

- **Every query is valid under `--column-orders stored`**: each atom's variables follow
  its stored column order, so no query needs a reordered copy and none is a
  `CyclicAttributeOrder`.
- **No query repeats a variable in an atom or uses a constant**, so none uses the
  selection or const rewrites; they run through the plain join path.
- **Counts are exact and the same on all three cells**, because no relation holds a
  duplicate row (see [Differences from upstream](#differences-from-upstream)).

### Verified counts

The reference plan is N = 2,000, `small` = 20, `medium` = 200 with seed 108 (the tests'
plan; it is below the spec's floor). Its counts, computed by a Python model written
from the spec alone, which shares no code with the Rust:

| | `q1` | `q2` | `q5` |
|---|---|---|---|
| reference plan, seed 108 | 1,812 | 1,814 | 1,800 |

They are the same in both row orders. `kermit/tests/h2o_generate.rs` pins them, and
`kermit-generate/tests/h2o.rs` pins SHA-256 digests of every relation's data.

### How `expected` is computed

Every query's `expected` is the count of step 4 of the key generation, taken from
`x`'s unsorted columns. It is a bitmap lookup over the key range, which shares no code
with the joins, so a bug in a trie, an intersection routine or an optimiser cannot make
the count wrong in the same way. The generator's tests check it against a brute-force
join (count each right-hand table's keys in a `HashMap`, then sum over `x`'s rows) in
both orders.

`bench run h2o -i all -a all --verify` then runs each query once and aborts on a
mismatch.

## Determinism

The output is a pure function of `(scale, seed, order)`: the same spec always gives the
same rows in the same order and the same counts. The Parquet bytes are identical when
regenerating with the same build; they can differ across builds, since the file's footer
records the Parquet writer's version. `meta.json` records the seed, the order, the
plan and each relation's SHA-256, and `benchmark.yml` carries the same digests, so
`bench fetch` verifies a generated cache and a cache can be identified after the fact.

`kermit-generate/tests/h2o.rs` pins the data itself: SHA-256 digests over each relation's
values (not its file bytes) for the reference plan, in both orders. If the digests must
change, the output of an existing spec has changed: bump `META_SCHEMA_VERSION` and add an
`h2o` floor to `OUTDATED_BEFORE`, so that old caches show as `stale` and `bench run`
refuses them, rather than passing as hits.

## Practical scale ceiling

N is the cost. The queries stream every answer to a counter and never collect, but
timing them still enumerates every row, and q1 and q2 each return close to N − N / 10
rows.

| Quantity | Cost | Notes |
|----------|------|-------|
| Loading a relation | 8 B per column per row | Since #111 a relation is one flat `usize` buffer. The six relations hold about 6N values, so the loaded batches alone are about 48N bytes: 480 MB at N = 1e7. This is before any structure is built over them |
| Generating | about 4N words at the peak | The kept `id3` column, the `id1` (or `id2`) column and the 2N-word interleaved buffer for the two-column relation: about 3.2 GB at N = 1e8 |
| Row count | at most `RowId::MAX` = 4,294,967,295 | A relation's rows are addressed by a 4-byte `RowId`. `scale` is a `u32`, so its largest value is the same, and `KeyPlan::new` rejects a plan above it |

- **Space per structure and the per-structure ceilings** on this host's memory are
  recorded under [Measured at N = 1e7](#measured-at-n--1e7).
- **Never write a result to disk.** `kermit join` writes every row as CSV; q1 and q2 at
  1e7 each return about nine million rows. `bench run` only counts.
- **`bench run --all` includes both H2O benchmarks**, and generating each takes the time
  and memory above on first use.

## Vendored generator

Nothing is vendored: the data is generated from a seed, and kermit's code is a
reimplementation of upstream's, not a copy of it.

| Field | Value |
|---|---|
| Upstream | db-benchmark's join data generator, `_data/join-datagen.R` (MPL-2.0); <https://github.com/h2oai/db-benchmark> |
| Version pin | none: no upstream code is copied or run; the spec records the structure kermit keeps |
| Build toolchain | none (no external tool runs) |
| Vendor path | none |
| Committed vs fetched | nothing committed or fetched; the data is generated on first use and cached |

To regenerate, run `bench run --force <name>`, or delete the cache directory.

## Tests

| Test | What it validates | Gate |
|------|-------------------|------|
| `kermit-bench/src/definition.rs` (`h2o_*` and `an_unknown_h2o_order_fails_to_parse`) | `H2oOrder`'s names match their serde spelling and default to `shuffled`; an unknown order fails to parse and names `shuffled`; a spec parses with `seed` 108 and `order` `shuffled` and serialises to exactly that; `order: sorted` parses; `spec_hash` changes with `scale`, `seed` and `order`, and an omitted `seed` hashes as 108; 9,999,999 is rejected and 10,000,000 accepted | always |
| `kermit-generate/src/h2o/rng.rs` | SplitMix64 reproduces `splitmix64.c`'s outputs for two seeds; three streams' first outputs are pinned and the twelve (column, step) streams differ; `bounded` matches a reference model, including a case that exercises Lemire's rejection loop, stays in range at n = 1, 2ᵏ and 2ᵏ ± 1, and passes a chi-square check on n = 10; `bounded(0)` panics; a shuffle is a permutation | always |
| `kermit-generate/src/h2o/keys.rs` | The pool splits have the right sizes, including n = 7 where n / 10 = 0; the right-hand table is the shared and right-only keys with no duplicate; `x`'s column holds every shared and left-only key and no right-only key, and is a permutation at n = N; `matches` equals a `HashSet` count; `KeyPlan::new` rejects zero and oversize cardinalities and rows above `RowId::MAX`; `for_scale` divides by 10⁶ and 10³ and rejects 999,999 | always |
| `kermit-generate/tests/h2o.rs` | Six relations and three queries, in order, with the right columns and sizes; each `expected` equals a brute-force join over the written relations, in both orders; the reference plan's data digests and counts are pinned; the same spec reproduces and another seed differs; a sorted relation is its shuffled counterpart sorted; a plan with no left-only key generates | always |
| `kermit/src/materialize.rs` (`h2o_cache_misses_hits_drifts_and_regenerates`) | A 1,000,000-row spec (below the floor, which `materialize` does not check) generates on a miss, is a hit the second time, raises `SpecDrift` after a seed change and regenerates under `force` | always |
| `kermit/src/main.rs` (`h2o_clap_names_match_the_spec_names`, `h2o_flags_default_and_enforce_the_floor`, `h2o_bench_name_is_scale_and_tag`) | The CLI's order names equal the spec's serde names; `--seed` and `--order` default as documented and `--scale` enforces the floor; the benchmark name is `h2o-<scale>-<tag>` | always |
| `kermit/tests/cli_gen_scale_range.rs` (`h2o_scale_below_the_floor_is_a_usage_error`) | `bench gen h2o --scale 1000` exits 2, names `'1000'`, `--scale` and 10000000, and writes no cache | always |
| `kermit/tests/h2o_generate.rs` | The committed YAMLs declare `kind: h2o` at 10000000 in the right order; the reference plan, in both orders, joins to its `expected` count for all three queries on the three cells × every optimiser × both `--column-orders` policies; the emitted counts are the model's 1,812, 1,814 and 1,800 | always |
| `bench run h2o -i all -a all --verify`, and the same for `h2o-sorted`, at N = 1e7 | Every cell verifies through the path `bench run` takes (`ExecutionFamily::load`), which the integration test does not | manual; release build inside `nix develop`; a heavy run, see [Measured at N = 1e7](#measured-at-n--1e7) |

## References

- G. L. Steele Jr., D. Lea, C. H. Flood. *Fast splittable pseudorandom number
  generators.* OOPSLA 2014.
- S. Vigna. `splitmix64.c`, the reference implementation: <https://prng.di.unimi.it/>
- D. Lemire. *Fast random integer generation in an interval.* ACM TOMACS 29(1), 2019.
- R. Durstenfeld. *Algorithm 235: Random permutation.* CACM 7(7), 1964.
- H2O.ai db-benchmark: <https://github.com/h2oai/db-benchmark>
- Design: `docs/specs/2026-10-15-h2o-benchmark-design.md`
- The generator: `kermit-generate/src/h2o/mod.rs`, `kermit-generate/src/h2o/rng.rs` and
  `kermit-generate/src/h2o/keys.rs`
- Sibling benchmark references: [`LUBM.md`](LUBM.md), [`WATDIV.md`](WATDIV.md),
  [`SNAP.md`](SNAP.md)

## Future work

- **q3, q4, the payload columns and NAs.** q3 (a left outer join) has no Datalog
  counterpart, q4 duplicates q2 under dictionary encoding, and the payloads and NAs do
  not reach a join on integer keys.
- **H2O in the thesis sweep.** The sweep's scripts name their benchmarks, and adding `h2o`
  and `h2o-sorted` to them is a separate decision (`bench run --all` already includes
  them).
- **A structure's memory ceiling at large N.** This page measures and documents the
  ceilings; fixing any structure's is out of scope.

## Measured at N = 1e7

Story 8 of #95's push 2 records the acceptance runs here.
````

- [ ] **Step 4: Run it to see it pass.**

```bash
test -f docs/benchmarks/H2O.md && echo present || echo absent
grep -cF '## Measured at N = 1e7' docs/benchmarks/H2O.md
grep -c '^## ' docs/benchmarks/H2O.md
```

Expected: `present`, `1`, and `12` (the title is `#`; the twelve `##` headings are, in order: How to run, Pipeline, Output layout, Inference / preprocessing, Workload reference, Determinism, Practical scale ceiling, Vendored generator, Tests, References, Future work, Measured at N = 1e7). Read the file once in full to confirm that every code fence is closed and that the in-page anchors (`#row-order-the-order-axis`, `#inference--preprocessing`, `#practical-scale-ceiling`, `#differences-from-upstream`, `#measured-at-n--1e7`) match their headings.

- [ ] **Step 5: Commit.**

```bash
git add -N docs/benchmarks/H2O.md
git commit -m "docs(benchmarks): document the H2O benchmark (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- docs/benchmarks/H2O.md
```

### Task 7.2: The benchmark indexes, USAGE.md and README.md

**Files:**
- Modify: `docs/benchmarks/README.md:17-20`
- Modify: `docs/benchmarks/README.md:28-40`
- Modify: `benchmarks/README.md:11`
- Modify: `benchmarks/README.md:59-60`
- Modify: `benchmarks/README.md:92`
- Modify: `benchmarks/README.md:185`
- Modify: `USAGE.md:471`
- Modify: `README.md:96`
- Modify: `README.md:99`

- [ ] **Step 1: Write the failing check.**

```bash
grep -cF 'H2O' docs/benchmarks/README.md
grep -cF 'h2o' benchmarks/README.md
grep -cF 'bench gen h2o' USAGE.md README.md
```

- [ ] **Step 2: Run it to see it fail.** Every count is `0`.

- [ ] **Step 3: Implement.** Make the edits below. Each file's edits are independent of the other files'.

**`docs/benchmarks/README.md`, the index.** Insert after line 20, the last line of the SNAP bullet, and before the blank line that precedes the `## Cross-benchmark comparison` heading:

```markdown
- [H2O](H2O.md) — the join task of H2O.ai's db-benchmark: one fact table and three
  right-hand tables joined on integer keys, with three queries (`q1`, `q2`, `q5`)
  whose counts are exact. Acyclic key/foreign-key joins, the relational baseline;
  the same data can be written shuffled or sorted.
```

**`docs/benchmarks/README.md`, the comparison table.** Current text (lines 28-40):

```text
| Dimension | LUBM | WatDiv | SNAP |
|---|---|---|---|
| Query count per dataset | 14 | ~12 400 per stress file (12 committed files) | 4 per graph (`triangle`, `path3`, `cycle4`, `clique4`) |
| Authoring | Hand-designed for specific OWL features | Mechanically generated from templates | Four hand-chosen graph patterns over a downloaded real graph |
| Predicate arity | All binary + unary type lookups | All binary | One relation, `edge`, binary |
| Inference required | OWL-Lite (subClassOf, subPropertyOf, transitivity, inverseOf, realisation) | None — data is pre-materialised | None — the edge list is symmetrised and cleaned, not inferred over |
| Triangle queries | **Q2, Q9 — explicit hand-designed** | Incidental (e.g. q0010 sharing a country variable) | **`triangle` and `clique4` explicit**; `cycle4` counts closed 4-step walks |
| Self-joins | None | Yes (e.g. `friendof(V_v2, V_v2)`) | **Every query** (all atoms are `edge`) |
| Result oracle | Paper Table 3, manually transcribed | None — vendored binary emits no `.desc` | **Generator-computed `expected` for every query**, by an oracle that shares no code with the joins |
| Reproducibility | Deterministic per `(seed, scale)` | Non-deterministic; tag-based snapshots | Deterministic: the `.gz` is pinned by SHA-256; Parquet bytes are stable per build |
| Sandbox | — | bwrap required (or `--no-bwrap`) | — |
| Vendored asset | Committed (~2.9 MB jar) | Committed (~360 KB binary) | None — downloaded from snap.stanford.edu (4–11 MB, SHA-256 checked) |
| Pipeline command | `kermit bench gen lubm` | `kermit bench gen watdiv` | `kermit bench gen snap` |
```

Replace with the same table plus an `H2O` column (every cell of the first twelve lines is unchanged; only the last cell is new):

```markdown
| Dimension | LUBM | WatDiv | SNAP | H2O |
|---|---|---|---|---|
| Query count per dataset | 14 | ~12 400 per stress file (12 committed files) | 4 per graph (`triangle`, `path3`, `cycle4`, `clique4`) | 3 per scale (`q1`, `q2`, `q5`) |
| Authoring | Hand-designed for specific OWL features | Mechanically generated from templates | Four hand-chosen graph patterns over a downloaded real graph | Three of db-benchmark's join questions, each over its own projection of one synthetic fact table |
| Predicate arity | All binary + unary type lookups | All binary | One relation, `edge`, binary | Unary and binary: `small`, `medium`, `big` and `x_id3` hold one key column, `x_id1` and `x_id2` two |
| Inference required | OWL-Lite (subClassOf, subPropertyOf, transitivity, inverseOf, realisation) | None — data is pre-materialised | None — the edge list is symmetrised and cleaned, not inferred over | None — the keys are integers already |
| Triangle queries | **Q2, Q9 — explicit hand-designed** | Incidental (e.g. q0010 sharing a country variable) | **`triangle` and `clique4` explicit**; `cycle4` counts closed 4-step walks | None — all three queries are acyclic key/foreign-key joins |
| Self-joins | None | Yes (e.g. `friendof(V_v2, V_v2)`) | **Every query** (all atoms are `edge`) | None |
| Result oracle | Paper Table 3, manually transcribed | None — vendored binary emits no `.desc` | **Generator-computed `expected` for every query**, by an oracle that shares no code with the joins | **Generator-computed `expected` for every query**, from a bitmap count that shares no code with the joins; the tests check it against a brute-force join |
| Reproducibility | Deterministic per `(seed, scale)` | Non-deterministic; tag-based snapshots | Deterministic: the `.gz` is pinned by SHA-256; Parquet bytes are stable per build | Deterministic per `(scale, seed, order)` from an in-repo random-number generator; the generator is pinned by golden digests of a small plan; Parquet bytes are stable per build |
| Sandbox | — | bwrap required (or `--no-bwrap`) | — | — |
| Vendored asset | Committed (~2.9 MB jar) | Committed (~360 KB binary) | None — downloaded from snap.stanford.edu (4–11 MB, SHA-256 checked) | None — generated by kermit, nothing downloaded |
| Pipeline command | `kermit bench gen lubm` | `kermit bench gen watdiv` | `kermit bench gen snap` | `kermit bench gen h2o` |
```

**`benchmarks/README.md`, the file list.** Insert after line 11 (the `snap-amazon.yml` bullet):

```markdown
- `h2o.yml` and `h2o-sorted.yml` are generator-driven H2O db-benchmark joins at N = 1e7 (`kind: h2o`: six relations, three queries with exact `expected` counts; the second writes every relation sorted); see [`docs/benchmarks/H2O.md`](../docs/benchmarks/H2O.md).
```

**`benchmarks/README.md`, the generator block table.** Current text (lines 59-60):

```text
| `generator.kind`            | `watdiv` \| `watdiv-basic` \| `lubm` \| `snap` | yes      | Selects the generator: a `kermit-rdf` pipeline, or for `snap` a `kermit-generate` module. |
| `generator.scale`           | u32 (≥ 1)          | for `watdiv`, `watdiv-basic` and `lubm` | Scale factor (`watdiv -d <N>` or `lubm-uba -u <N>`). `snap` has no scale. |
```

Replace with:

```markdown
| `generator.kind`            | `watdiv` \| `watdiv-basic` \| `lubm` \| `snap` \| `h2o` | yes      | Selects the generator: a `kermit-rdf` pipeline, or for `snap` and `h2o` a `kermit-generate` module. |
| `generator.scale`           | u32 (≥ 1)          | for `watdiv`, `watdiv-basic`, `lubm` and `h2o` | Scale factor (`watdiv -d <N>` or `lubm-uba -u <N>`); for `h2o`, N, the rows of the fact table, at least 10000000. `snap` has no scale. |
```

**`benchmarks/README.md`, the H2O fields.** Insert after line 92 (the SNAP `See …` line that ends the SNAP fields) and its blank line, before the `Structural invariants are checked at load time` line:

```markdown
H2O-specific fields (under `generator:` when `kind: h2o`):

| Field                       | Type        | Default                                                     | Description |
|-----------------------------|-------------|-------------------------------------------------------------|-------------|
| `generator.scale`           | u32 (≥ 10000000) | required                                               | N, the rows of the fact table `x`. 10000000 is upstream's floor; a smaller value is rejected by `BenchmarkDefinition::validate`. |
| `generator.seed`            | u32         | 108                                                         | Random seed (upstream's `set.seed(108)`). |
| `generator.order`           | `shuffled` \| `sorted` | `shuffled`                                       | Row order of every relation: as drawn, or sorted. |

All three feed the spec hash, and an omitted `seed` hashes the same as `seed: 108`. See [`docs/benchmarks/H2O.md`](../docs/benchmarks/H2O.md) for the relations, the queries and the floor.

```

**`benchmarks/README.md`, the example.** Current text (line 185):

```text
`snap-amazon` downloads the pinned `.gz` (4.5 MB, SHA-256 checked) on first use. A graph outside the named table takes `url` and `sha256` instead of `graph`; see `docs/benchmarks/SNAP.md`.
```

Insert after it (and its trailing blank line), before the `## Frozen snapshots` heading:

````markdown
```yaml
name: h2o-sorted
description: "H2O db-benchmark join (J1) at N = 1e7: q1, q2 and q5 over per-query projections of x, rows sorted, #101's input-order axis (#95)."
generator:
  kind: h2o
  scale: 10000000
  order: sorted
```

`h2o-sorted` generates the six H2O relations at N = 1e7 with every relation's rows sorted on first use; `h2o` is the same with `order` omitted (shuffled). `seed` defaults to 108. See `docs/benchmarks/H2O.md`.

````

**`USAGE.md`.** Insert after line 471 (the last line of the SNAP section) and its blank line, before the `## JSON reports` heading:

````markdown
### Generate an H2O join benchmark (`bench gen h2o`)

Generates the data of H2O.ai's db-benchmark join task (J1) at N rows and emits three
queries, `q1`, `q2` and `q5`, each with an exact `expected` count. Nothing is downloaded
and no external tool runs: a seeded generator in the repo writes six relations (`x_id3`,
`big`, `x_id1`, `small`, `x_id2`, `medium`). Output lands at
`~/.cache/kermit/benchmarks/h2o-<scale>-<tag>/`, so subsequent `bench run` invocations
pick it up.

```sh
kermit bench gen h2o --scale 10000000 --tag dev
# → ~/.cache/kermit/benchmarks/h2o-10000000-dev/
kermit bench run h2o-10000000-dev -i all -a all --verify
```

`--scale N` (the rows of the fact table) and `--tag` are required. `--scale` must be at
least 10000000, upstream's floor, and a smaller value is a usage error. Tunables
(optional): `--seed S` (default 108, upstream's `set.seed(108)`), `--order
shuffled|sorted` (default `shuffled`; `sorted` writes every relation's rows in sorted
order, for #101's input-order axis) and `--output-dir <PATH>` to write outside the
default cache (generated benchmarks placed there are NOT auto-discovered by
`bench list/fetch/run`). Seed and order are recorded in `meta.json`, not in the name.

Generation holds about four words per row at its peak, and each loaded relation costs 8
bytes per column per row; see [`docs/benchmarks/H2O.md`](docs/benchmarks/H2O.md). The
committed `h2o` and `h2o-sorted` (N = 1e7) need no `bench gen`:
`kermit bench run h2o -i all -a all --verify` generates on first use.

````

**`README.md`, the command.** Insert after line 96 (the `bench gen snap` line), inside the same code fence:

```text
kermit bench gen h2o    --scale 10000000 --tag dev   # → h2o-10000000-dev (N = 1e7 is the floor; nothing is downloaded)
```

**`README.md`, the prose.** Current text (line 99):

```text
All three write into `~/.cache/kermit/benchmarks/<name>/` and become discoverable to `bench run/list`. WatDiv uses the committed vendored binary at `kermit-rdf/vendor/watdiv/bin/Release/watdiv`; LUBM requires JDK 8 on PATH for the vendored jar; SNAP downloads a digest-pinned `.gz` from snap.stanford.edu and needs no external tool. See [`docs/benchmarks/WATDIV.md`](docs/benchmarks/WATDIV.md), [`docs/benchmarks/LUBM.md`](docs/benchmarks/LUBM.md), [`docs/benchmarks/SNAP.md`](docs/benchmarks/SNAP.md), and [`USAGE.md`](USAGE.md) for full flag listings.
```

Replace with:

```markdown
All four write into `~/.cache/kermit/benchmarks/<name>/` and become discoverable to `bench run/list`. WatDiv uses the committed vendored binary at `kermit-rdf/vendor/watdiv/bin/Release/watdiv`; LUBM requires JDK 8 on PATH for the vendored jar; SNAP downloads a digest-pinned `.gz` from snap.stanford.edu and needs no external tool; H2O draws its data from a seeded generator in the repo and needs neither a download nor a tool. See [`docs/benchmarks/WATDIV.md`](docs/benchmarks/WATDIV.md), [`docs/benchmarks/LUBM.md`](docs/benchmarks/LUBM.md), [`docs/benchmarks/SNAP.md`](docs/benchmarks/SNAP.md), [`docs/benchmarks/H2O.md`](docs/benchmarks/H2O.md), and [`USAGE.md`](USAGE.md) for full flag listings.
```

- [ ] **Step 4: Run it to see it pass.**

```bash
grep -cF 'H2O' docs/benchmarks/README.md       # expect 2
grep -cF 'h2o' benchmarks/README.md            # expect 7
grep -cF 'bench gen h2o' USAGE.md README.md    # expect USAGE.md:2 and README.md:1
grep -cF 'All four write' README.md            # expect 1
```

Read each edited region once to confirm that it renders: the new table has a header cell and a separator cell in every row, the fenced blocks are closed, and the comparison table still has the same number of rows.

- [ ] **Step 5: Commit.**

```bash
git commit -m "docs(benchmarks): add h2o to the benchmark index, schema, USAGE and README (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- docs/benchmarks/README.md benchmarks/README.md USAGE.md README.md
```

### Task 7.3: CLAUDE.md and ARCHITECTURE.md

**Files:**
- Modify: `CLAUDE.md:54`
- Modify: `CLAUDE.md:131-132`
- Modify: `CLAUDE.md:134`
- Modify: `CLAUDE.md:280`
- Modify: `CLAUDE.md:286`
- Modify: `CLAUDE.md:344`
- Modify: `CLAUDE.md:347`
- Modify: `ARCHITECTURE.md:392`
- Modify: `ARCHITECTURE.md:455`

- [ ] **Step 1: Write the failing check.**

```bash
grep -cF 'bench gen h2o' CLAUDE.md
grep -cF 'docs/benchmarks/H2O.md' CLAUDE.md
grep -cF 'snap | h2o' ARCHITECTURE.md
grep -cF 'kind: watdiv|lubm|snap|h2o' ARCHITECTURE.md
```

- [ ] **Step 2: Run it to see it fail.** Every count is `0`.

- [ ] **Step 3: Implement.**

**`CLAUDE.md`, Build Commands.** Insert after line 54 (the `bench gen snap` line), before the closing fence:

```text
cargo run -- bench gen h2o --scale 10000000 --tag dev              # Generate an H2O db-benchmark join benchmark on the fly (imperative; 10000000 is the floor, nothing is downloaded)
```

**`CLAUDE.md`, the `kermit-generate` row.** Current text (lines 131-132):

```text
                  `h2o` (#95), `snap` (#96: `snap.rs`, the parser and generator,
                  and `snap/counts.rs`, the count oracle), `job` (#97).
```

Replace with (three lines; the label column is 18 characters wide, as in the other lines of the row):

```text
                  `h2o` (#95: `h2o/mod.rs`, the generator; `h2o/rng.rs`, the random
                  numbers; `h2o/keys.rs`, the key columns), `snap` (#96: `snap.rs`, the
                  parser and generator, and `snap/counts.rs`, the count oracle), `job` (#97).
```

**`CLAUDE.md`, the `kermit` row.** Current text (line 134):

```text
                  gen [watdiv|lubm|snap]).
```

Replace with:

```text
                  gen [watdiv|lubm|snap|h2o]).
```

**`CLAUDE.md`, Adding a new benchmark.** Current text (a fragment of line 280):

```markdown
declare a `generator: { kind: watdiv|lubm|snap, ... }` block (`scale` is per-kind: `watdiv` and `lubm` require it, `snap` has none) — `bench run <name>` will materialise the data on demand via `kermit-rdf` (`kermit-generate` for `snap`)
```

Replace with:

```markdown
declare a `generator: { kind: watdiv|lubm|snap|h2o, ... }` block (`scale` is per-kind: `watdiv`, `lubm` and `h2o` require it, `h2o` at least 10000000, and `snap` has none) — `bench run <name>` will materialise the data on demand via `kermit-rdf` (`kermit-generate` for `snap` and `h2o`)
```

**`CLAUDE.md`, Benchmark Reference Docs.** Insert after line 286 (the `docs/benchmarks/SNAP.md` bullet):

```markdown
- `docs/benchmarks/H2O.md` — H2O db-benchmark joins (`kind: h2o`): the six relations and three queries (q1, q2, q5), the deviations from upstream's generator, the `order` axis, the 10000000 floor, memory costs.
```

**`CLAUDE.md`, the declarative generator gotcha.** Two fragments of line 344. Current text:

```markdown
may declare `generator: { kind: watdiv|lubm|snap, ... }` instead of `relations`/`queries` (`scale` is per-kind: `watdiv` and `lubm` require it, `snap` has none)
```

Replace with:

```markdown
may declare `generator: { kind: watdiv|lubm|snap|h2o, ... }` instead of `relations`/`queries` (`scale` is per-kind: `watdiv`, `lubm` and `h2o` require it, `snap` has none)
```

Current text:

```markdown
invokes the underlying `kermit-rdf` pipeline (`kermit-generate` for `snap`)
```

Replace with:

```markdown
invokes the underlying `kermit-rdf` pipeline (`kermit-generate` for `snap` and `h2o`)
```

**`CLAUDE.md`, a new gotcha.** Insert a new bullet after line 347 (the `Non-RDF generators live in kermit-generate` bullet):

```markdown
- **H2O's random numbers are in-repo, one stream per step** (#95): `kermit-generate/src/h2o/rng.rs` is SplitMix64 with Lemire bounded draws and Fisher–Yates, because `rand`'s `StdRng` and its sampling algorithms are not value-stable across versions (#74). Every (column, step) pair (`id1`/`id2`/`id3` × `Pool`/`Pad`/`LeftShuffle`/`RightShuffle`) has its own stream, seeded by SplitMix64's output function applied to `seed << 32 | (column × 4 + step)`, so a relation's data depends only on `(plan, seed, order)`, never on the order relations are generated in; renumbering a column or a step is an output change. `the_reference_plan_matches_its_golden_digests` (`kermit-generate/tests/h2o.rs`) pins SHA-256 digests of each relation's data (not its file bytes) for the 2,000 / 20 / 200 plan at seed 108 in both orders, and follows the bump-and-floor rule above: if the digests must change, bump `META_SCHEMA_VERSION` and add an `h2o` floor to `OUTDATED_BEFORE`, so old caches show as `stale` instead of passing as hits. The spec's floor is `scale >= 10000000` (`H2O_MIN_SCALE`): below it `small` has no left-only or right-only key. The three queries read per-query projections of `x` (`x_id1`, `x_id2`, `x_id3`), because a `_` is still a trie level, and each carries `id3`, which is unique per row, so no relation holds a duplicate and HashTrie's multiset count equals the sorted tries'.
```

**`ARCHITECTURE.md`, the generator-driven flavour.** Current text (line 392; #96's
final-review commit f496c4c gave it its `snap` wording):

```text
- **Generator-driven** — declares a `generator: { kind: watdiv|lubm|snap, ... }` block instead (`scale: N` is required by `watdiv` and `lubm`; `snap` has none). The relations and queries are produced on demand by a `kermit-rdf` pipeline (`watdiv`, `lubm`) or a `kermit-generate` generator (`snap`).
```

Replace with:

```markdown
- **Generator-driven** — declares a `generator: { kind: watdiv|lubm|snap|h2o, ... }` block instead (`scale: N` is required by `watdiv`, `lubm` and `h2o`; `snap` has none). The relations and queries are produced on demand by a `kermit-rdf` pipeline (`watdiv`, `lubm`) or a `kermit-generate` generator (`snap`, `h2o`).
```

**`ARCHITECTURE.md`, the CLI list.** Current text (line 455):

```text
  - `bench gen { watdiv | lubm | snap }` — imperative on-the-fly generation, bypassing YAML; writes into the same cache layout under a user-supplied `--tag`.
```

Replace with:

```markdown
  - `bench gen { watdiv | lubm | snap | h2o }` — imperative on-the-fly generation, bypassing YAML; writes into the same cache layout under a user-supplied `--tag`.
```

- [ ] **Step 4: Run it to see it pass.**

```bash
grep -cF 'bench gen h2o --scale 10000000 --tag dev' CLAUDE.md   # expect 1
grep -cF 'gen [watdiv|lubm|snap|h2o]' CLAUDE.md                 # expect 1
grep -cF 'kind: watdiv|lubm|snap|h2o' CLAUDE.md                 # expect 2
grep -cF 'docs/benchmarks/H2O.md' CLAUDE.md                     # expect 1
grep -cF "H2O's random numbers are in-repo" CLAUDE.md           # expect 1
grep -cF '(`kermit-generate` for `snap` and `h2o`)' CLAUDE.md   # expect 2
grep -cF 'bench gen { watdiv | lubm | snap | h2o }' ARCHITECTURE.md  # expect 1
grep -cF 'kind: watdiv|lubm|snap|h2o' ARCHITECTURE.md                # expect 1
grep -cF 'generator (`snap`, `h2o`)' ARCHITECTURE.md                  # expect 1
```

Read the edited regions of both files once: the Build Commands fence stays closed, and the Workspace Architecture block is still one fenced block with the `kermit-generate` row aligned.

- [ ] **Step 5: Commit.**

```bash
git commit -m "docs: add the h2o generator kind to CLAUDE.md and ARCHITECTURE.md (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- CLAUDE.md ARCHITECTURE.md
```

## Final gate

Run this after Stories 1–7, in the cargo slot the co-ordinator allots. Check the host first
(see Ground rules) before any of these commands.

- [ ] **Step 1: Scope.** The diff against `BASE` touches only the File map's files:
  ```bash
  git diff --name-only "$BASE"..HEAD | sort
  ```
  It must print exactly: `ARCHITECTURE.md`, `CLAUDE.md`, `README.md`, `USAGE.md`,
  `benchmarks/README.md`, `benchmarks/h2o-sorted.yml`, `benchmarks/h2o.yml`,
  `docs/benchmarks/H2O.md`, `docs/benchmarks/README.md`, `kermit-bench/src/definition.rs`,
  `kermit-bench/src/lib.rs`, `kermit-generate/Cargo.toml`, `kermit-generate/src/h2o/keys.rs`,
  `kermit-generate/src/h2o/mod.rs`, `kermit-generate/src/h2o/rng.rs`,
  `kermit-generate/src/lib.rs`, `kermit-generate/tests/h2o.rs`, `kermit/src/main.rs`,
  `kermit/src/materialize.rs`, `kermit/tests/cli_gen_scale_range.rs` and
  `kermit/tests/h2o_generate.rs`. Nothing may appear under `.superpowers/`, `scripts/`, the
  generator layer's own files, `kermit-rdf/`, or any data-structure or algorithm module.
- [ ] **Step 2: No temporary allows survive.** This prints nothing:
  ```bash
  grep -rn 'allow(dead_code)' kermit-generate/src/h2o/
  ```
- [ ] **Step 3: Format.** `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command cargo fmt --all --check` prints nothing.
- [ ] **Step 4: Clippy.** `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets` ends with no warning or error. Its non-test pass would catch dead code among the crate-private RNG and key items.
- [ ] **Step 5: Docs.** `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps` succeeds. Then the binary's docs:
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings 2>&1 | grep -E '^error' | sort > "$SCRATCH/rustdoc-head.txt"
  diff "$SCRATCH/rustdoc-base.txt" "$SCRATCH/rustdoc-head.txt"
  ```
  The `diff` prints nothing. `rustdoc-base.txt` is the baseline recorded at `BASE` before
  Story 1 (Ground rules); the tree has two known pre-existing errors there.
- [ ] **Step 6: The whole workspace's tests**, detached:
  ```bash
  setsid nohup nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 RUST_BACKTRACE=1 cargo test --workspace > "$SCRATCH/gate-test.log" 2>&1 & disown
  ```
  Watch the PID with `kill -0` until it exits. Then
  `grep -E '^test result:|FAILED|panicked' "$SCRATCH/gate-test.log"` must show only
  `test result: ok.` lines.
- [ ] **Step 7: Mutation checks.** Run every story's `### Mutation checks` list. For each
  one: commit the story's code first, apply the one edit with an exact `sed` or `Edit`,
  confirm with `git diff` that it applied, run the named test and see it fail, then reverse
  the exact edit and re-run the test to see it pass. Never restore with `git checkout`.
- [ ] **Step 8: Report** the results to the design session (≤ 15 lines), with the final
  `HEAD`.

## Acceptance at N = 1e7 (the manager; the contract's "Story 8")

The design session runs this itself after the final gate. It needs a heavy-run slot from the
co-ordinator and the user's go-ahead: about 1 GB of Parquet, a few GB of RAM, and minutes of
CPU on a shared host. It uses a scratch cache on `/tb`, never `~/.cache/kermit` and never
the tmpfs scratchpad.

- [ ] **Step 1: Run directory and binary.**
  ```bash
  RUN=/tb/Source/Academia/kermit-bench-runs/h2o-acceptance-$(date +%F)
  mkdir -p "$RUN"/{bin,xdg,reports,logs}
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo build --release -p kermit
  cp target/release/kermit "$RUN/bin/kermit"
  git rev-parse HEAD > "$RUN/COMMIT"
  ```
  The binary under `kermit-bench-runs/*/bin/` is what the other sessions' host checks look
  for, so they see the runs below.
- [ ] **Step 2: `--verify` and `space` on every cell, both orders.**
  ```bash
  set -o pipefail
  for b in h2o h2o-sorted; do
    XDG_CACHE_HOME="$RUN/xdg" "$RUN/bin/kermit" bench --name "acc-$b" \
      --report-json "$RUN/reports/$b-space.json" run "$b" -i all -a all --verify -m space \
      2>&1 | tee "$RUN/logs/$b.log" || echo "FAILED: $b"
  done
  ```
  `--name` and `--report-json` are `bench`'s own flags (`BenchArgs`), so they go before
  `run`. Both runs must finish without a `FAILED:` line, and every cell must report a verified query (`verified: true` in each
  report's `axes`). The first run generates the cache, and its time is in the log.
- [ ] **Step 3: Determinism and `bench gen h2o`.**
  ```bash
  for t in acc1 acc2; do
    XDG_CACHE_HOME="$RUN/xdg" /run/current-system/sw/bin/time -v "$RUN/bin/kermit" bench gen h2o \
      --scale 10000000 --tag "$t" 2> "$RUN/logs/gen-$t.log"
  done
  for d in h2o h2o-10000000-acc1 h2o-10000000-acc2; do
    jq -c '[.relations[] | {name, sha256}]' "$RUN/xdg/kermit/benchmarks/$d/meta.json"
  done | uniq | wc -l
  XDG_CACHE_HOME="$RUN/xdg" "$RUN/bin/kermit" bench gen h2o --scale 10000000 --order sorted \
    --tag acc3 2> "$RUN/logs/gen-acc3.log"
  for d in h2o-sorted h2o-10000000-acc3; do
    jq -c '[.relations[] | {name, sha256}]' "$RUN/xdg/kermit/benchmarks/$d/meta.json"
  done | uniq | wc -l
  jq -c '{seed, order}' "$RUN/xdg/kermit/benchmarks/h2o-10000000-acc3/meta.json"
  ```
  Both counts must print `1`. The first means all three shuffled caches have the same
  relations with the same digests. The second means `bench gen h2o --order sorted` writes
  exactly the declarative `h2o-sorted` cache, so the CLI passes `order` through. The last
  line prints `{"seed":108,"order":"sorted"}`. `gen-acc1.log` holds the generation's wall time (`Elapsed (wall clock) time`) and
  peak RSS (`Maximum resident set size`).
- [ ] **Step 4: Read the numbers.**
  - q1, q2 and q5 come from the `expected` values in
    `"$RUN/xdg/kermit/benchmarks/h2o/benchmark.yml"`; q5 must be 9,000,000.
  - The space per structure per relation comes from each report's `space/<relation>`
    groups, through kermit-lab's Criterion index (`kermit_lab.criterion.CriterionIndex`,
    keyed by the report's `criterion_groups`). A space value is the per-iteration mean,
    which equals the structure's `heap_size_bytes()` exactly.
- [ ] **Step 5: Write H2O.md's `## Measured at N = 1e7`.** Replace the stub sentence with:
  - the three counts, beside upstream's (8,997,136; 9,001,606; 9,000,000), and why q1 and
    q2 differ (the in-repo RNG, Amendment 2);
  - a table of space per structure (TreeTrie, ColumnTrie, HashTrie) per relation, with
    totals, for `shuffled` and for `sorted`;
  - the generation's wall time and peak RSS, with the commit (`$RUN/COMMIT`) and this
    host's description (16 cores, 30 GB);
  - each structure's ceiling: the N at which the six relations' total space reaches
    30 GB, extrapolated linearly from the 1e7 total and rounded to two significant figures
    (D7). State that it ignores the queries' working memory.

  Commit with `git commit -m "docs(benchmarks): H2O measurements at N = 1e7 (#95)" -- docs/benchmarks/H2O.md`, plus the trailers.
- [ ] **Step 6: Keep the run directory.** Add `$RUN/README.md`, three lines naming the
  commit, the commands, and that the data is a scratch cache that can be deleted.

## After this plan: landing push 2

This section is not part of execution. The design session does these steps, in order:

1. Tell the user that push 2 is gated and measured, and ask for the go-ahead to land it.
   Pushing to master is outward-facing, and approval for one push is not standing approval.
   If the heavy-run slot is far off, the user may land the gated code first. The acceptance
   section then lands later, as push 2b.
2. On a yes, land it the loom way:
   - `git fetch origin`;
   - `git merge origin/master` (merge, never rebase);
   - if the merge brought anything in, re-run the final gate's Steps 3–6 on the merged tree;
   - `git push origin HEAD:master`, with no `--force`.
3. Send the pushed SHA to the benchmark co-ordinator session, and to the job (#97) session
   if it is still unlanded (its `pub mod job;` sits beside `pub mod h2o;`).
4. Close #95 if the acceptance has landed; otherwise comment with what remains. Update the
   #95 project memory.

## Merge notes

Four drafters wrote the stories in parallel against the names contract (84ad4e5): Stories 1–2,
Story 3, Stories 4–5 and Stories 6–7. The design session wrote the head and tail. The drafts
quote `aidanb/snap` at 74668ed. The merged plan's premises were re-checked against SNAP's
later tip, f496c4c, which restores a doc line and edits `ARCHITECTURE.md` and a
`materialize.rs` comment: 3 of 261 were flagged, all false positives (the contract and the
guard copy exist only on `aidanb/h2o`, and one flag is a context quote).

Changes made while merging:
- **`KeyPlan::new`'s messages** use inline captures (`{rows}`, `{small}`, `{medium}`), not
  positional arguments: clippy's `uninlined_format_args` runs under `-Dwarnings`.
- **`KeyPool::left()` is `#[cfg(test)]`.** Story 3 removes Story 1's
  `#[allow(dead_code)]`, and only Story 1's tests call `left()` (`x_column` takes
  `keys[..n]` whole), so the non-test clippy pass would reject it. Story 3's drafter found
  this.

Additions beyond the contract, all of them kept:
- Story 1:
  - `shuffle_matches_the_reference_model` pins two shuffles from the Python model. Without
    it, `bounded(i)` for `bounded(i + 1)` still yields a permutation.
  - `the_pool_and_its_columns_match_the_reference_model` (n = 20, seed 108, `Id3`).
  - A `#[should_panic]` test for `x_column` below n.
  - The cardinality accessor's test, and the boundary values of `KeyPlan::new`.
- Story 3:
  - `meta.json` read back as `GeneratedMeta<H2oExtras>`.
  - Equal file digests across two runs.
  - Bounds on the brute-force counts.
- Story 4:
  - The cache test also checks that a hit leaves `meta.json`'s modification time
    unchanged, since its timestamp is second-granular.
  - The forced run uses `order: sorted` as well as seed 109, so a `run_h2o` that dropped
    `order` fails.
  - More spec parse cases.
- Story 6: the YAML descriptions name their order, and the emitted relation names are
  pinned in write order.

Left for later, out of this push's scope:
- `ARCHITECTURE.md` has other lines that name only `watdiv|lubm` (25, 57, 436 and 541 on
  SNAP's tip f496c4c). Story 7 edits the two lines SNAP's phase 2 edited for `snap`: 392
  (in f496c4c) and 455.
- `USAGE.md:348` ("the same one `bench gen watdiv|lubm` uses") describes the kermit-rdf
  pipeline and is left alone.

External facts in H2O.md that the spec and contract don't state, each checked by the design
session: the upstream repository URL (`https://github.com/h2oai/db-benchmark`, required by
the doc schema), Vigna's `https://prng.di.unimi.it/splitmix64.c` (the source of the pinned
vectors), Durstenfeld's "Algorithm 235: Random permutation" (CACM 7(7), 1964), Lemire's
"Fast Random Integer Generation in an Interval" (ACM TOMACS 29(1), 2019), and the 480 MB
load figure at 1e7 (about 6N values × 8 B, from D7).

Plan review (2026-10-15, two code-quality-reviewers, read-only, no cargo, against
`aidanb/snap` f496c4c):
- **Stories 1–3:** no critical findings. 1 important fix: Story 3 quotes Story 1's comment
  in its two-line form. 3 minor fixes: no `git checkout` restores (the guard blocks them),
  E0412 in two red steps, and a vacuous `header.name()` assertion replaced by the sink's
  recorded columns. All 16 of those stories' mutants were traced as killed.
- **Stories 4–7 and the tail:** no critical findings. 3 important fixes:
  - acceptance Step 2 put `bench`'s own `--name` / `--report-json` after `run`, and lost
    the exit status in `tee`;
  - `ARCHITECTURE.md:392` was missing;
  - the mutation steps had a hard-coded `SCRATCH`, and their restore could be skipped.

  8 minor fixes: the E0599 wording, an unambiguous Story 6 mutant (`q1 + 1`), `grep -rn`
  in the gate, the cache test's real cost, acceptance coverage of `--order sorted` and the
  `seed` / `order` plumbing, and three H2O.md / README wording corrections.
