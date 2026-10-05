# HashTrie Radix-Partitioned Build Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `radix:K`, HashTrie's first BuildMode (issue #91). It builds the trie the serial build makes, bucket for bucket and capacity for capacity. `--ds-build` becomes keyed (`hash-trie=radix:8,column-trie=incremental`).

**Architecture:**
1. **Partition.** Stably partition the tuples on the top K bits of `H::hash(tuple[0])`.
2. **Build scratch roots.** Fill one scratch root per partition with the unchanged `insert_at`.
3. **Merge.** Move every distinct root key, with its finished subtrie, into the real root in the order the key first appeared in the input. A table's layout depends only on the order its *new* keys arrive, so the result is identical to the serial build.

The mode rides on `Execution::HashHtj { …, build }` and is reported as `ds_build_mode` (`serial` by default).

**Tech Stack:** Rust nightly (workspace crates `kermit-ds`, `kermit`), clap 4 derive, Python `kermit-lab` (pandas, pytest via uv).

**Spec:** [`docs/specs/2026-10-05-hash-trie-radix-build-design.md`](../../specs/2026-10-05-hash-trie-radix-build-design.md). Read it first: the "Why the trie is identical" section is the correctness argument every task relies on.

---

## Ground rules for every task

- **Build host:** a background memory monitor kills large cargo jobs on this machine. Run cargo in the foreground with `CARGO_BUILD_JOBS=2` exported. Detach anything over ~10 minutes (`setsid nohup … &`). Never use `pkill -f`.
- **Formatting:** only `nix develop --command cargo fmt --all`. Stable rustfmt rewrites dozens of files.
- **Clippy:** `RUSTFLAGS=-Dwarnings`. Backtick identifiers in `///` docs, or `doc_markdown` fails.
- **Commits:** conventional style with `(#91)`. Stage files by name. End every message with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012T6pktZQ2vWZfAxHdqxvtD
  ```
  In the commit commands below, `-m "<attribution lines>"` stands for exactly those two lines, as the final paragraph.
- **History:** never amend, rebase or switch branches. This is a loom worktree on branch `aidanb/91`.
- **Mutant checks:** commit the real code first, apply the mutant, run the test, then restore with `git checkout -- <file>`. That is safe only because the file is committed.
- **No issue edits:** do not comment on or close #91.
- **Shared cache:** do not write under `~/.cache/kermit`.

## File map

| File | Responsibility | Task |
|---|---|---|
| `kermit-ds/src/ds/hash_trie/build_mode.rs` (new) | `HashTrieBuildMode`, `RadixBits`, the error types, `FromStr`, `BuildMode` | 1 |
| `kermit-ds/src/ds/hash_trie/hash_table.rs` | `into_buckets` (consuming accessor), #66 guard test | 2 |
| `kermit-ds/src/ds/hash_trie/radix.rs` (new) | the radix build, plus identity tests | 3 |
| `kermit-ds/src/ds/hash_trie/implementation.rs` | `from_tuples_with_config_and_build_mode`, `BuildModeRelation`, visibility | 3 |
| `kermit-ds/src/test_support.rs` | `Mod10HashStrategy` (colliding test hasher) | 3 |
| `kermit-ds/src/ds/hash_trie/mod.rs`, `kermit-ds/src/ds/mod.rs`, `kermit-ds/src/lib.rs` | module wiring, re-exports | 1, 3 |
| `kermit-ds/src/built_with.rs` | `HashTrieIterable` forward, hash spy test | 4 |
| `kermit-ds/tests/hash_trie_tests.rs`, `kermit-ds/tests/parquet_tests.rs` | `HashTrieSipRadix2` DS-layer aliases | 4 |
| `kermit/src/options.rs` | `BuildModes`, `DsChoices.build`, keyed `BuildChoices`, `DsFlag::Build(IndexStructure)` | 5, 6 |
| `kermit/src/execution.rs` | `HashHtj { build }`, families, axis, spy test | 5 |
| `kermit/src/bench/ds.rs`, `kermit/src/bench/run.rs`, `kermit/src/main.rs` | dispatch arms pass `build` | 5, 6 |
| `kermit/tests/cli_column_trie_build_mode.rs`, `kermit/tests/cli_bench_run_ds_flag_reach.rs` | HashTrie now reports `serial`; keyed spelling | 5, 6 |
| `kermit/tests/cli_hash_trie_build_mode.rs` (new) | CLI smoke for the hash-trie mode | 6 |
| `python/kermit-lab/tests/test_contract.py` | keyed spelling | 6 |
| `kermit/tests/common/utils.rs`, `kermit/tests/join_tests.rs` | `JoinEntry` for `BuiltWith<HashTrie>`, join suites | 7 |
| `python/kermit-lab/kermit_lab/defaults.py`, `python/kermit-lab/tests/test_defaults.py` | `serial` back-fill | 8 |
| docs (see Task 9) | — | 9 |

---

## Phase 1 — `kermit-ds`

### Task 1: The mode type

**Files:**
- Create: `kermit-ds/src/ds/hash_trie/build_mode.rs`
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs`, `kermit-ds/src/ds/mod.rs:10-17`, `kermit-ds/src/lib.rs:34-50`

- [ ] **Step 1: Write the type and its tests**

Create `kermit-ds/src/ds/hash_trie/build_mode.rs` with the full implementation *and* its tests in one go. The type is small, and its tests pin exact strings, so writing them against a stub buys nothing:

```rust
//! [`HashTrieBuildMode`]: how a [`HashTrie`](super::HashTrie) is built from a
//! known set of tuples — the BuildMode category of the optimization standard
//! (`docs/specs/optimization-standard.md`).

use std::{fmt, str::FromStr};

/// How a [`HashTrie`](super::HashTrie) is built from a known set of tuples.
/// Every mode builds the identical trie — the same buckets and the same
/// capacities — so the mode changes how long the build takes, never the
/// trie it builds (issue #91).
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
}

/// The radix bits of a `radix` build: `2^bits` partitions, `bits` in
/// `1..=16`, so a tuple's partition number fits a `u16`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RadixBits(u8);

impl RadixBits {
    /// The fewest bits: two partitions.
    pub const MIN: u8 = 1;
    /// The most bits: 65,536 partitions, numbered in a `u16`.
    pub const MAX: u8 = 16;

    /// `bits` radix bits, which must lie in `MIN..=MAX`.
    pub fn new(bits: u8) -> Result<Self, InvalidRadixBits> {
        if (Self::MIN..=Self::MAX).contains(&bits) {
            Ok(Self(bits))
        } else {
            Err(InvalidRadixBits(bits))
        }
    }

    /// The number of bits.
    pub fn get(self) -> u8 { self.0 }
}

/// A radix bit count outside `1..=16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidRadixBits(pub u8);

impl fmt::Display for InvalidRadixBits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "radix bits must be between {} and {}, got {}",
            RadixBits::MIN,
            RadixBits::MAX,
            self.0
        )
    }
}

impl std::error::Error for InvalidRadixBits {}

/// A string that names no [`HashTrieBuildMode`]. Its message names the
/// accepted forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseHashTrieBuildModeError(String);

impl fmt::Display for ParseHashTrieBuildModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}

impl std::error::Error for ParseHashTrieBuildModeError {}

/// Parses the strings [`axis_value`](kermit_iters::BuildMode::axis_value)
/// returns: `serial` and `radix:<bits>`.
impl FromStr for HashTrieBuildMode {
    type Err = ParseHashTrieBuildModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const EXPECTED: &str = "expected serial or radix:<bits>, bits in 1..=16";
        let error = |why: String| ParseHashTrieBuildModeError(format!("{why}; {EXPECTED}"));
        match s.split_once(':') {
            | None if s == "serial" => Ok(Self::Serial),
            | None if s == "radix" => Err(error("radix needs a bit count".to_owned())),
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
            | _ => Err(error(format!("unknown hash-trie build mode {s:?}"))),
        }
    }
}

impl kermit_iters::BuildMode for HashTrieBuildMode {
    fn axis_value(&self) -> String {
        match self {
            | Self::Serial => "serial".to_owned(),
            | Self::Radix(bits) => format!("radix:{}", bits.get()),
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_iters::BuildMode};

    fn radix(bits: u8) -> HashTrieBuildMode { HashTrieBuildMode::Radix(RadixBits::new(bits).unwrap()) }

    /// What a report's `ds_build_mode` says is what `--ds-build hash-trie=…`
    /// parses back, for every mode.
    #[test]
    fn axis_values_round_trip_through_from_str() {
        let mut modes = vec![HashTrieBuildMode::Serial];
        modes.extend((RadixBits::MIN..=RadixBits::MAX).map(radix));
        for mode in modes {
            assert_eq!(mode.axis_value().parse::<HashTrieBuildMode>(), Ok(mode));
        }
    }

    /// The labels name every HashTrie report's `ds_build_mode`, and
    /// kermit-lab reads a missing axis as `"serial"`.
    #[test]
    fn axis_values_and_default_are_pinned() {
        assert_eq!(HashTrieBuildMode::Serial.axis_value(), "serial");
        assert_eq!(radix(8).axis_value(), "radix:8");
        assert_eq!(HashTrieBuildMode::default(), HashTrieBuildMode::Serial);
    }

    #[test]
    fn radix_bits_are_bounded() {
        assert_eq!(RadixBits::new(0), Err(InvalidRadixBits(0)));
        assert_eq!(RadixBits::new(17), Err(InvalidRadixBits(17)));
        assert_eq!(RadixBits::new(1).map(RadixBits::get), Ok(1));
        assert_eq!(RadixBits::new(16).map(RadixBits::get), Ok(16));
    }

    /// Every rejection names what was wrong and the accepted forms.
    #[test]
    fn malformed_modes_are_rejected_with_the_accepted_forms() {
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
        ];
        for (input, why) in cases {
            let msg = input.parse::<HashTrieBuildMode>().unwrap_err().to_string();
            assert!(msg.contains(why), "{input:?}: {msg}");
            assert!(msg.contains("expected serial or radix:<bits>"), "{input:?}: {msg}");
        }
    }
}
```

- [ ] **Step 2: Wire the module and re-exports**

In `kermit-ds/src/ds/hash_trie/mod.rs`, add `mod build_mode;` directly above the `mod config;` line, and extend the `pub use` block:

```rust
mod build_mode;
mod config;
// …(existing lines unchanged)…

pub use {
    build_mode::{HashTrieBuildMode, InvalidRadixBits, ParseHashTrieBuildModeError, RadixBits},
    config::{HashTrieConfig, InvalidLoadFactor, LoadFactor},
    implementation::HashTrie,
    pruning::{NoPruning, PruningPolicy, SingletonPruning},
};
```

In `kermit-ds/src/ds/mod.rs`, extend the `hash_trie::{…}` re-export:

```rust
    hash_trie::{
        HashTrie, HashTrieBuildMode, HashTrieConfig, InvalidLoadFactor, InvalidRadixBits,
        LoadFactor, NoPruning, ParseHashTrieBuildModeError, PruningPolicy, RadixBits,
        SingletonPruning,
    },
```

In `kermit-ds/src/lib.rs`, extend the `ds::{…}` re-export the same way:

```rust
    ds::{
        ColumnTrie, ColumnTrieBuildMode, HashTrie, HashTrieBuildMode, HashTrieConfig,
        IndexStructure, InvalidLoadFactor, InvalidRadixBits, LoadFactor, NoPruning,
        ParseHashTrieBuildModeError, PruningPolicy, RadixBits, SingletonPruning, TreeTrie,
    },
```

- [ ] **Step 3: Run the tests**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds build_mode::tests`
Expected: 4 tests in `ds::hash_trie::build_mode::tests` pass. (A dead-code warning on `HashTrieBuildMode` is impossible: it is `pub` and re-exported.)

- [ ] **Step 4: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/build_mode.rs kermit-ds/src/ds/hash_trie/mod.rs kermit-ds/src/ds/mod.rs kermit-ds/src/lib.rs
git commit -m "feat(kermit-ds): HashTrieBuildMode, serial or radix:<bits> (#91)" -m "<attribution lines>"
```

---

### Task 2: `HashTable::into_buckets` and the #66 guard

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/hash_table.rs` (impl block after `shell_heap_bytes` near line 300, tests module end)

- [ ] **Step 1: Write the failing #66 guard test**

Append to the `tests` module of `hash_table.rs`, after `rebuilding_a_subset_in_a_larger_tables_iteration_order_costs_no_more_than_key_order`:

```rust
    /// The radix build (issue #91) fills each partition's scratch root with
    /// keys that share the top `bits` of their hash — a clustered input, as
    /// in #66. The per-capacity multiplier must keep absorbing one partition
    /// no dearer than absorbing an unrestricted key set of the same size.
    /// Without it, `bits` shared index bits would put every key of a small
    /// table in one bucket.
    #[test]
    #[cfg_attr(miri, ignore = "a cost test scanning ~10^6 hashes; nothing here is unsafe")]
    fn absorbing_one_radix_partition_costs_no_more_than_unrestricted_keys() {
        fn check<H: HashStrategy>() {
            for bits in [1u32, 4, 8] {
                let partition: Vec<u64> = (0..)
                    .map(H::hash)
                    .filter(|hash| hash >> (64 - bits) == 0)
                    .take(BUILD_KEYS)
                    .collect();
                let unrestricted: Vec<u64> = (0..BUILD_KEYS).map(H::hash).collect();
                let (clustered, baseline) = (build_probes(&partition), build_probes(&unrestricted));
                assert!(
                    clustered <= 2 * baseline,
                    "{} radix:{bits}: one partition took {clustered} probes, unrestricted keys \
                     {baseline}",
                    H::NAME
                );
            }
        }
        check::<SipHashStrategy>();
        check::<FxHashStrategy>();
    }

    #[test]
    fn into_buckets_returns_every_entry_at_its_bucket() {
        let mut t: HashTable<u32> = HashTable::new();
        for (i, hash) in [0x1000_0000_0000_0000_u64, 0x5000_0000_0000_0000, 0x9000_0000_0000_0000]
            .into_iter()
            .enumerate()
        {
            t.entry_or_insert_with(hash, LoadFactor::default(), || i as u32);
        }
        let positions: Vec<(usize, u64)> = t
            .iter()
            .map(|(hash, _)| (t.index_of(hash).unwrap(), hash))
            .collect();
        let capacity = t.buckets_len();
        let buckets = t.into_buckets();
        assert_eq!(buckets.len(), capacity);
        assert_eq!(buckets.iter().flatten().count(), 3);
        for (idx, hash) in positions {
            assert_eq!(buckets[idx].as_ref().map(|e| e.hash), Some(hash));
        }
    }
```

- [ ] **Step 2: Run them to verify the accessor test fails to compile**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_table::tests`
Expected: compile error `no method named into_buckets found for struct HashTable`.

- [ ] **Step 3: Add the accessor**

In the `impl<V> HashTable<V>` block, after `shell_heap_bytes`:

```rust
    /// Consumes the table, returning its bucket array: `buckets_len()`
    /// slots in bucket order, each `None` or the entry stored there. Used
    /// by the radix build to move each scratch entry out exactly once, by
    /// the position `index_of` reported for it.
    pub fn into_buckets(self) -> Vec<Option<Entry<V>>> { self.buckets }
```

- [ ] **Step 4: Run the tests**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds hash_table::tests`
Expected: all pass, including the two new tests. The guard runs in well under a second natively.

If the guard fails for FxHash or SipHash with the multiplier in place, **stop and report**. Do not loosen the factor. It would mean a radix partition really does cluster under that hasher: a #66-style finding the spec has to address before the radix build can be trusted.

- [ ] **Step 5: Mutant check that the guard bites**

Commit first (Step 6). Then in `bucket_index` temporarily replace `hash.wrapping_mul(MULTIPLIERS[p as usize])` with `hash`, and run:
`CARGO_BUILD_JOBS=2 cargo test -p kermit-ds absorbing_one_radix_partition`
Expected: FAIL. With the paper's raw high-bit index, one partition takes many times the baseline's probes. Confirm the failure message names `radix:`. Restore with `git checkout -- kermit-ds/src/ds/hash_trie/hash_table.rs` and re-run the test: PASS. If the mutant does *not* fail, stop and report. The guard would be vacuous.

- [ ] **Step 6: Commit (before Step 5's mutant)**

```bash
git add kermit-ds/src/ds/hash_trie/hash_table.rs
git commit -m "feat(kermit-ds): HashTable::into_buckets and a #66 guard for radix partitions (#91)" -m "<attribution lines>"
```

---

### Task 3: The radix build and the constructors

**Files:**
- Create: `kermit-ds/src/ds/hash_trie/radix.rs`
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs:88` (`make_root`), `:166` (`insert_at`), the `ConfigurableRelation` impl region (~`:251`), imports at the top
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs` (add `mod radix;`)
- Modify: `kermit-ds/src/test_support.rs` (add `Mod10HashStrategy`)

- [ ] **Step 1: Add the colliding test strategy**

Append to `kermit-ds/src/test_support.rs`:

```rust
/// Test-only strategy that forces real hash collisions: `hash(k) = k mod
/// 10`, so keys congruent modulo 10 share a bucket at every level and
/// tuples whose attributes all collide share a leaf chain. Every hash is
/// below 10, so all of them fall in radix partition 0. Twins live in
/// `kermit-ds/tests/hash_trie_tests.rs` and the `kermit-algos` tests; none
/// is shared, so that no crate ships a colliding strategy in its API.
#[derive(Copy, Clone, Default, Debug)]
pub(crate) struct Mod10HashStrategy;

impl LayoutOption for Mod10HashStrategy {
    const NAME: &'static str = "mod10";
}

impl kermit_iters::HashStrategy for Mod10HashStrategy {
    fn hash(key: usize) -> u64 { (key % 10) as u64 }
}
```

(`LayoutOption` is already imported in `test_support.rs`.)

- [ ] **Step 2: Write the radix module's tests first**

Create `kermit-ds/src/ds/hash_trie/radix.rs` containing *only* the module doc, the imports, the `fill_root` signature with `todo!()` body, and the full test module below. Add `mod radix;` to `hash_trie/mod.rs` (after `mod pruning;`).

```rust
//! The `radix:K` build of a [`HashTrie`](super::HashTrie): radix-partition
//! the tuples on their first attribute's hash, build each partition into a
//! scratch root, then merge the scratch roots into the real one (SIGMOD 2020
//! §3.3.2; issue #91).
//!
//! The result is the trie the serial build makes, bucket for bucket and
//! capacity for capacity. A table's final layout depends only on the order
//! in which its *new* keys arrive — `HashTable::entry_or_insert_with`
//! returns an existing entry before its resize check — so the merge inserts
//! the distinct root keys in the order they first appear in the input, as
//! the serial build does. Below the root nothing needs care: the partition
//! is stable, so each root key's subtrie is built by the same `insert_at`
//! calls, on the same tuples in the same order, as in the serial build.

use {
    super::{
        build_mode::RadixBits, config::LoadFactor, hash_table::HashTable,
        implementation::HashTrie, node::HashTrieNode, pruning::PruningPolicy,
    },
    kermit_iters::HashStrategy,
};

/// Fills the empty `root` with `tuples` by the `radix:bits` build. Every
/// tuple must have `arity` attributes; the caller checks.
pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy>(
    root: &mut HashTrieNode<P>, arity: usize, tuples: Vec<Vec<usize>>, bits: RadixBits,
    load_factor: LoadFactor,
) {
    todo!()
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            cardinality::Cardinality,
            ds::hash_trie::{
                build_mode::HashTrieBuildMode,
                config::HashTrieConfig,
                pruning::{NoPruning, SingletonPayload, SingletonPruning},
            },
            heap_size::HeapSize,
            relation::{BuildModeRelation, ConfigurableRelation, Relation},
            test_support::{Lcg, Mod10HashStrategy},
        },
        kermit_iters::{FxHashStrategy, SipHashStrategy},
    };

    /// Asserts that two tables hold the same buckets: the same capacity
    /// (array length and allocation), the same length, the same hash in
    /// every bucket, and values that `same_value` accepts.
    fn assert_same_table<V>(
        a: &HashTable<V>, b: &HashTable<V>, path: &str, same_value: &dyn Fn(&V, &V, &str),
    ) {
        assert_eq!(a.buckets_len(), b.buckets_len(), "{path}: capacity");
        assert_eq!(a.shell_heap_bytes(), b.shell_heap_bytes(), "{path}: bucket allocation");
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
    fn assert_same_chain(a: &Vec<Vec<usize>>, b: &Vec<Vec<usize>>, path: &str) {
        assert_eq!(a, b, "{path}: chain");
        assert_eq!(a.capacity(), b.capacity(), "{path}: chain capacity");
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            assert_eq!(x.capacity(), y.capacity(), "{path}: tuple {i} capacity");
        }
    }

    fn assert_same_node<P: PruningPolicy>(a: &HashTrieNode<P>, b: &HashTrieNode<P>, path: &str) {
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
            | _ => panic!("{path}: node variants differ"),
        }
    }

    /// The array-level identity the standard requires of a BuildMode:
    /// every table's buckets and capacity, every chain and tuple capacity,
    /// every singleton, the heap size and the tuple count.
    fn assert_same_trie<H: HashStrategy, P: PruningPolicy>(
        a: &HashTrie<H, P>, b: &HashTrie<H, P>, label: &str,
    ) {
        assert_same_node(a.root(), b.root(), label);
        assert_eq!(a.heap_size_bytes(), b.heap_size_bytes(), "{label}: heap size");
        assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
    }

    /// Rows of up to three columns, cut to `arity`.
    fn rows(arity: usize, rows: &[[usize; 3]]) -> Vec<Vec<usize>> {
        rows.iter().map(|row| row[..arity].to_vec()).collect()
    }

    /// Enough distinct first values to double the root several times, with
    /// repeats so subtries and chains hold several tuples.
    const RANDOM_TUPLES: usize = if cfg!(miri) { 200 } else { 3_000 };

    /// Bit counts under test. Sixteen bits make 65,536 partitions, too slow
    /// to set up a thousand times under Miri.
    const BITS: &[u8] = if cfg!(miri) { &[1, 4] } else { &[1, 4, 16] };

    fn inputs(arity: usize) -> Vec<(&'static str, Vec<Vec<usize>>)> {
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
            ("duplicates", rows(arity, &[[1, 2, 3], [1, 2, 3], [1, 2, 3]])),
            (
                "interleaved",
                rows(arity, &[[1, 2, 3], [2, 3, 4], [1, 5, 6], [3, 1, 1], [2, 3, 9], [1, 2, 7]]),
            ),
            ("random", random),
        ]
    }

    /// `radix:K` builds the trie `serial` builds, for every arity, load
    /// factor, bit count and input.
    fn check_identity<H: HashStrategy, P: PruningPolicy>() {
        for arity in 1..=3 {
            for percent in [70, 50] {
                let config = HashTrieConfig {
                    load_factor: LoadFactor::percent(percent).unwrap(),
                };
                for &bits in BITS {
                    let radix = HashTrieBuildMode::Radix(RadixBits::new(bits).unwrap());
                    for (input, tuples) in inputs(arity) {
                        let build = |mode| {
                            HashTrie::<H, P>::from_tuples_with_config_and_build_mode(
                                arity.into(),
                                config,
                                mode,
                                tuples.clone(),
                            )
                        };
                        let label = format!(
                            "{}/{} arity {arity}, load {percent}%, radix:{bits}, {input}",
                            H::NAME,
                            P::NAME
                        );
                        assert_same_trie(&build(HashTrieBuildMode::Serial), &build(radix), &label);
                    }
                }
            }
        }
    }

    #[test]
    fn radix_builds_the_serial_trie_under_siphash() {
        check_identity::<SipHashStrategy, NoPruning>();
        check_identity::<SipHashStrategy, SingletonPruning>();
    }

    #[test]
    fn radix_builds_the_serial_trie_under_fxhash() {
        check_identity::<FxHashStrategy, NoPruning>();
        check_identity::<FxHashStrategy, SingletonPruning>();
    }

    /// Every hash is below 10, so every tuple lands in partition 0, and
    /// distinct keys share full hashes, root entries and leaf chains.
    #[test]
    fn radix_builds_the_serial_trie_under_colliding_hashes() {
        check_identity::<Mod10HashStrategy, NoPruning>();
        check_identity::<Mod10HashStrategy, SingletonPruning>();
    }

    #[test]
    #[should_panic(expected = "tuple arity")]
    fn radix_build_rejects_a_wrong_arity() {
        let _: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            HashTrieConfig::default(),
            HashTrieBuildMode::Radix(RadixBits::new(4).unwrap()),
            vec![vec![1, 2], vec![3]],
        );
    }

    /// `BuildModeRelation` builds with the default config, and its default
    /// mode is `Relation::from_tuples`.
    #[test]
    fn build_mode_relation_uses_the_default_config() {
        let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
        let tuples = rows(2, &[[1, 2, 0], [3, 4, 0], [1, 5, 0]]);
        let built: HashTrie = HashTrie::from_tuples_with_build_mode(2.into(), radix, tuples.clone());
        assert_eq!(*built.config(), HashTrieConfig::default());
        let plain: HashTrie = HashTrie::from_tuples(2.into(), tuples.clone());
        let default: HashTrie =
            HashTrie::from_tuples_with_build_mode(2.into(), HashTrieBuildMode::default(), tuples);
        assert_same_trie(&plain, &default, "from_tuples vs the default mode");
        assert_same_trie(&plain, &built, "from_tuples vs radix:2");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds radix::tests`
Expected: compile errors. `from_tuples_with_config_and_build_mode` and `BuildModeRelation for HashTrie` do not exist yet.

- [ ] **Step 4: Add the constructors in `implementation.rs`**

4a. Change the two private helpers' visibility (bodies untouched):
- `fn make_root(arity: usize) -> HashTrieNode<P>` → `pub(super) fn make_root(arity: usize) -> HashTrieNode<P>`
- `fn insert_at(` → `pub(super) fn insert_at(`

4b. Extend the imports at the top of `implementation.rs`:

```rust
use {
    super::{
        build_mode::HashTrieBuildMode,
        config::{HashTrieConfig, LoadFactor},
        node::HashTrieNode,
        pruning::{NoPruning, PruningPolicy, SingletonPayload},
        radix,
    },
    crate::relation::{BuildModeRelation, ConfigurableRelation, Relation, RelationHeader},
    kermit_iters::{ConfigOption, HashStrategy, JoinIterable, LayoutOption, SipHashStrategy},
    std::marker::PhantomData,
};
```

4c. Add a new inherent `impl` block directly after the `impl ConfigurableRelation for HashTrie<H, P>` block:

```rust
impl<H: HashStrategy, P: PruningPolicy> HashTrie<H, P> {
    /// Creates a trie holding `config`, populated with `tuples` and built by
    /// `mode` — the one constructor that takes both the Config and the
    /// BuildMode. Every mode builds the identical trie (issue #91), so
    /// `mode` changes only how long this takes.
    ///
    /// `Serial` is [`ConfigurableRelation::from_tuples_with_config`],
    /// unchanged; `Radix` partitions first (see `radix.rs`).
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    pub fn from_tuples_with_config_and_build_mode(
        header: RelationHeader, config: HashTrieConfig, mode: HashTrieBuildMode,
        tuples: Vec<Vec<usize>>,
    ) -> Self {
        match mode {
            | HashTrieBuildMode::Serial => Self::from_tuples_with_config(header, config, tuples),
            | HashTrieBuildMode::Radix(bits) => {
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
                radix::fill_root::<H, P>(
                    &mut trie.root,
                    arity,
                    tuples,
                    bits,
                    config.load_factor,
                );
                trie.tuple_count = tuple_count;
                trie
            },
        }
    }
}

impl<H: HashStrategy, P: PruningPolicy> BuildModeRelation for HashTrie<H, P> {
    type BuildMode = HashTrieBuildMode;

    /// Builds with the default config; see
    /// [`from_tuples_with_config_and_build_mode`](HashTrie::from_tuples_with_config_and_build_mode).
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: HashTrieBuildMode, tuples: Vec<Vec<usize>>,
    ) -> Self {
        Self::from_tuples_with_config_and_build_mode(
            header,
            HashTrieConfig::default(),
            mode,
            tuples,
        )
    }
}
```

4d. Update the struct doc's `# Construction` paragraph (near line 46). Replace:

```
/// the config-carrying constructors; `new` / `from_tuples` are thin wrappers
/// over them that supply the default configuration.
```

with:

```
/// the config-carrying constructors; `new` / `from_tuples` are thin wrappers
/// over them that supply the default configuration. A known set of tuples
/// can also be built by the `radix:K` BuildMode
/// ([`from_tuples_with_config_and_build_mode`](Self::from_tuples_with_config_and_build_mode),
/// or [`BuildModeRelation`]), which builds the identical trie.
```

- [ ] **Step 5: Run the tests to verify they now fail at runtime**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds radix::tests`
Expected: it compiles, apart from an unused-variable warning in the `todo!()` body. The identity tests and `build_mode_relation_uses_the_default_config` FAIL with `not yet implemented`. `radix_build_rejects_a_wrong_arity` passes, because the arity check runs before `fill_root`.

- [ ] **Step 6: Implement `fill_root`**

Replace the `todo!()` stub in `radix.rs` with the full build, between the imports and the test module:

```rust
/// A tuple with its position in the input, which the merge orders by.
type Indexed = (usize, Vec<usize>);

/// A root entry ready to merge: the input position at which its key first
/// appeared, its hash, and its finished value.
type Arrival<V> = (usize, u64, V);

/// Fills the empty `root` with `tuples` by the `radix:bits` build. Every
/// tuple must have `arity` attributes; the caller checks.
pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy>(
    root: &mut HashTrieNode<P>, arity: usize, tuples: Vec<Vec<usize>>, bits: RadixBits,
    load_factor: LoadFactor,
) {
    // One of the two stays empty: the root is `Inner` for arity ≥ 2 (values
    // are subtries) and `Leaf` for arity 1 (values are chains).
    let mut subtries = Vec::new();
    let mut chains = Vec::new();
    for partition in partition::<H>(tuples, bits) {
        if partition.is_empty() {
            continue;
        }
        let (scratch, first_seen) = build_scratch_root::<H, P>(partition, arity, load_factor);
        match scratch {
            | HashTrieNode::Inner(table) => take_in_arrival_order(table, &first_seen, &mut subtries),
            | HashTrieNode::Leaf(table) => take_in_arrival_order(table, &first_seen, &mut chains),
            | HashTrieNode::Singleton(_) => unreachable!("a root is never pruned"),
        }
    }
    match root {
        | HashTrieNode::Inner(table) => {
            insert_in_first_appearance_order(table, subtries, load_factor)
        },
        | HashTrieNode::Leaf(table) => insert_in_first_appearance_order(table, chains, load_factor),
        | HashTrieNode::Singleton(_) => unreachable!("a root is never pruned"),
    }
}

/// Splits `tuples` into `2^bits` partitions on the top `bits` of
/// `H::hash(tuple[0])`. A histogram pass sizes each partition exactly; a
/// scatter pass then moves every tuple into its partition with its input
/// index. Stable — each partition keeps input order — which is what keeps
/// every subtrie identical to the serial build's.
fn partition<H: HashStrategy>(tuples: Vec<Vec<usize>>, bits: RadixBits) -> Vec<Vec<Indexed>> {
    let shift = 64 - u32::from(bits.get());
    // `bits <= 16`, so a partition number fits a `u16`.
    let partition_of: Vec<u16> = tuples
        .iter()
        .map(|tuple| (H::hash(tuple[0]) >> shift) as u16)
        .collect();
    let mut sizes = vec![0usize; 1 << bits.get()];
    for &p in &partition_of {
        sizes[usize::from(p)] += 1;
    }
    let mut partitions: Vec<Vec<Indexed>> = sizes.into_iter().map(Vec::with_capacity).collect();
    for (index, (tuple, p)) in tuples.into_iter().zip(partition_of).enumerate() {
        partitions[usize::from(p)].push((index, tuple));
    }
    partitions
}

/// Builds one partition into a scratch root of the real root's kind, by
/// the serial build's own `insert_at`. Returns the scratch root and, for
/// each key it holds, the input index of the tuple that introduced it and
/// the key's hash, in arrival order.
fn build_scratch_root<H: HashStrategy, P: PruningPolicy>(
    partition: Vec<Indexed>, arity: usize, load_factor: LoadFactor,
) -> (HashTrieNode<P>, Vec<(usize, u64)>) {
    let mut scratch = HashTrie::<H, P>::make_root(arity);
    let mut first_seen = Vec::new();
    for (index, tuple) in partition {
        let key = tuple[0];
        let keys_before = scratch.len();
        HashTrie::<H, P>::insert_at(&mut scratch, 0, arity, tuple, load_factor);
        if scratch.len() > keys_before {
            first_seen.push((index, H::hash(key)));
        }
    }
    (scratch, first_seen)
}

/// Moves every entry out of `scratch` into `out`, tagged with the input
/// index at which its key first appeared. Bucket positions are read before
/// the table is consumed, and each bucket is taken exactly once.
fn take_in_arrival_order<V>(
    scratch: HashTable<V>, first_seen: &[(usize, u64)], out: &mut Vec<Arrival<V>>,
) {
    let positions: Vec<usize> = first_seen
        .iter()
        .map(|&(_, hash)| scratch.index_of(hash).expect("every arrival is in its scratch table"))
        .collect();
    let mut buckets = scratch.into_buckets();
    for (&(first, hash), position) in first_seen.iter().zip(positions) {
        let entry = buckets[position].take().expect("each bucket is taken once");
        debug_assert_eq!(entry.hash, hash);
        out.push((first, hash, entry.value));
    }
}

/// Inserts the merged entries into the real root in the order their keys
/// first appeared in the input — the order the serial build inserts them —
/// so the root's buckets, length and capacity are the serial build's.
fn insert_in_first_appearance_order<V>(
    root: &mut HashTable<V>, mut arrivals: Vec<Arrival<V>>, load_factor: LoadFactor,
) {
    // First indices are distinct, so the unstable sort is deterministic.
    arrivals.sort_unstable_by_key(|&(first, _, _)| first);
    for (_, hash, value) in arrivals {
        root.entry_or_insert_with(hash, load_factor, || value);
    }
}
```

- [ ] **Step 7: Run the tests**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds radix::tests`
Expected: all 6 tests PASS.

Then run the whole crate, to confirm the serial path is untouched:
Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds`
Expected: PASS (all existing tests unchanged).

- [ ] **Step 8: Clippy and docs for the crate**

Run: `RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo clippy -p kermit-ds --all-targets`
Run: `RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc -p kermit-ds --no-deps`
Expected: no warnings. If clippy flags `ptr_arg` despite the allow, or `type_complexity`, fix it as the lint says. Keep `&Vec` where `capacity()` is read.

- [ ] **Step 9: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/radix.rs kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/mod.rs kermit-ds/src/test_support.rs
git commit -m "feat(kermit-ds): radix-partitioned HashTrie build, identical to serial (#91)" -m "<attribution lines>"
```

- [ ] **Step 10: Mutant checks that the identity test bites (two mutants, one at a time)**

Mutant A, which drops the merge order: in `insert_in_first_appearance_order`, delete the `arrivals.sort_unstable_by_key(…)` line.
Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds radix::tests`
Expected: the identity tests FAIL with a `bucket N` mismatch. Restore with `git checkout -- kermit-ds/src/ds/hash_trie/radix.rs`.

Mutant B, which makes the partition unstable: in `build_scratch_root`, change `for (index, tuple) in partition {` to `for (index, tuple) in partition.into_iter().rev() {`.
Run the same command. Expected: the identity tests FAIL, on a chain mismatch or an arrival-order mismatch. Restore with `git checkout -- kermit-ds/src/ds/hash_trie/radix.rs`.

Re-run `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds radix::tests` and expect PASS. If either mutant survives, stop and report: the test is not checking what the spec says it checks.

---

### Task 4: `BuiltWith` for the hash family, and DS-layer aliases

**Files:**
- Modify: `kermit-ds/src/built_with.rs` (imports, a new impl after the `TrieIterable` impl, tests)
- Modify: `kermit-ds/tests/hash_trie_tests.rs`, `kermit-ds/tests/parquet_tests.rs`

- [ ] **Step 1: Write the failing spy test**

In `built_with.rs`'s `tests` module, change the `use` to:

```rust
    use {
        super::*,
        crate::ds::{ColumnTrie, ColumnTrieBuildMode, HashTrie, HashTrieBuildMode, RadixBits},
        kermit_iters::{HashTrieIterable, HashTrieIterator},
    };
```

Then append:

```rust
    /// The hash-family counterpart of [`Spy`]: counts its calls and asks
    /// for a radix build.
    struct HashSpy;

    impl BuildModeProvider<HashTrieBuildMode> for HashSpy {
        fn build_mode() -> HashTrieBuildMode {
            CALLS.with(|calls| calls.set(calls.get() + 1));
            HashTrieBuildMode::Radix(RadixBits::new(2).unwrap())
        }
    }

    /// A hash-family `BuiltWith` asks its provider once per build, never for
    /// an empty relation, and stays navigable through `HashTrieIterable`.
    #[test]
    fn hash_family_asks_the_provider_and_forwards_iteration() {
        CALLS.with(|calls| calls.set(0));
        let _empty = BuiltWith::<HashTrie, HashSpy>::new(2.into());
        assert_eq!(CALLS.with(|calls| calls.get()), 0);
        let r = BuiltWith::<HashTrie, HashSpy>::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(CALLS.with(|calls| calls.get()), 1);
        let mut iter = r.hash_trie_iter();
        assert!(iter.open());
        assert_eq!(iter.size(), 2);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds built_with::tests`
Expected: compile error, because `BuiltWith<HashTrie, HashSpy>` does not implement `HashTrieIterable`.

- [ ] **Step 3: Add the forward**

In `built_with.rs`, change the `kermit_iters` import to:

```rust
    kermit_iters::{
        HasOptimizationAxes, HashTrieIterable, HashTrieIterator, JoinIterable, TrieIterable,
        TrieIterator,
    },
```

and add after `impl<R: TrieIterable, P> TrieIterable for BuiltWith<R, P>`:

```rust
impl<R: HashTrieIterable, P> HashTrieIterable for BuiltWith<R, P> {
    fn hash_trie_iter(&self) -> impl HashTrieIterator { self.inner.hash_trie_iter() }
}
```

In the module doc, replace the sentence `A hash-family BuildMode would also need a \`HashTrieIterable\` forward, as \`Configured\` has.` with `It also forwards \`HashTrieIterable\`, as \`Configured\` does, for HashTrie's build modes.`

- [ ] **Step 4: Run it**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds built_with::tests`
Expected: PASS (5 tests).

- [ ] **Step 5: Add the DS-layer aliases**

In `kermit-ds/tests/hash_trie_tests.rs`, extend the `kermit_ds` import with `define_build_mode_provider, BuiltWith, HashTrieBuildMode, RadixBits`, then add after the `HashTrieMod10Dense` invocation:

```rust
// ── BuildMode: the radix build ──────────────────────────────────────────
// Every build mode builds the identical trie (issue #91), so the iterator
// contract must hold unchanged. Two bits make four partitions, so the 3–5
// tuple fixtures spread over several partitions with several keys in each.
define_build_mode_provider!(
    Radix2,
    HashTrieBuildMode,
    HashTrieBuildMode::Radix(RadixBits::new(2).unwrap())
);

type HashTrieSipRadix2 = BuiltWith<HashTrieSip, Radix2>;

hash_trie_test_suite!(HashTrieSipRadix2, SipHashStrategy);
```

In `kermit-ds/tests/parquet_tests.rs`, add `HashTrieBuildMode, RadixBits` to the `kermit_ds` import (`define_build_mode_provider` and `BuiltWith` are already imported for `ColumnTrieIncremental`), then append after `parquet_test_suite!(HashTrieSipDense, sorted_tuples_dense);`:

```rust
// …and under the radix BuildMode, which must load the same trie (issue #91).
define_build_mode_provider!(
    Radix2,
    HashTrieBuildMode,
    HashTrieBuildMode::Radix(RadixBits::new(2).unwrap())
);

type HashTrieSipRadix2 = BuiltWith<HashTrieSip, Radix2>;

fn sorted_tuples_radix(relation: &HashTrieSipRadix2) -> Vec<Vec<usize>> {
    // `BuiltWith` derefs to the inner `HashTrie`, as `Configured` does.
    sorted_tuples(relation)
}

parquet_test_suite!(HashTrieSipRadix2, sorted_tuples_radix);
```

If the existing `kermit_ds` import in `parquet_tests.rs` lacks `BuiltWith` or `define_build_mode_provider`, add them.

- [ ] **Step 6: Run the integration tests**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --test hash_trie_tests --test parquet_tests`
Expected: PASS, including new `hashtriesipradix2::…` and `parquet_hashtriesipradix2::…` tests.

- [ ] **Step 7: Phase 1 gate**

Run, in order:
- `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds`, which must PASS;
- `RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo clippy -p kermit-ds --all-targets`, which must be clean;
- `RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc -p kermit-ds --no-deps`, which must be clean;
- `nix develop --command cargo fmt --all -- --check`. If it reports diffs, run `nix develop --command cargo fmt --all` and include the result in this commit.

Run Miri on `kermit-ds` detached. It takes several minutes:
```bash
setsid nohup bash -c 'MIRIFLAGS="-Zmiri-disable-isolation" CARGO_BUILD_JOBS=2 cargo miri test -p kermit-ds > /tmp/claude-1000/miri-kermit-ds-91.log 2>&1; echo EXIT=$? >> /tmp/claude-1000/miri-kermit-ds-91.log' > /dev/null 2>&1 &
```
Check the log later. It must end `EXIT=0`. Do not wait on it before Task 5, but it must be green before Task 10.

- [ ] **Step 8: Commit**

```bash
git add kermit-ds/src/built_with.rs kermit-ds/tests/hash_trie_tests.rs kermit-ds/tests/parquet_tests.rs
git commit -m "test(kermit-ds): radix-built HashTrie through BuiltWith and the DS suites (#91)" -m "<attribution lines>"
```

---

## Phase 2 — `kermit` binary

### Task 5: The cell, the families and the axis

The old flag shape stays in this task: `BuildModes.hash_trie` is always the default until Task 6.

**Files:**
- Modify: `kermit/src/options.rs` (new `BuildModes`, `DsChoices.build`, `DsChoices::resolve`, two tests)
- Modify: `kermit/src/execution.rs` (imports, `Execution::HashHtj`, `for_pair`, `for_structure`, `HashTrieFamily`, `HashHtj::new`, tests)
- Modify: `kermit/src/bench/ds.rs:229-240`, `kermit/src/bench/run.rs:357-365`, `kermit/src/main.rs:666-673`
- Modify: `kermit/tests/cli_column_trie_build_mode.rs`

- [ ] **Step 1: Write the failing family tests in `execution.rs`**

In `execution.rs`'s `tests` module, add to the `use` block:
`crate::options::BuildModes,` `kermit_ds::{ConfigurableRelation, RadixBits},` `kermit_iters::LayoutOption,`.
`ConfigurableRelation` moves from the top-level import into the tests in Step 3. `HashStrategy` and, after Step 3, `HashTrieBuildMode` come in through `super::*` from the top-level imports.

Replace the test `only_column_trie_families_report_their_build_mode` with:

```rust
    /// Every `ColumnTrie` and `HashTrie` family reports the mode it builds
    /// with; `TreeTrie` has a single build and carries no such axis.
    #[test]
    fn families_with_a_build_mode_report_it() {
        let radix = HashTrieBuildMode::Radix(RadixBits::new(4).unwrap());
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
        assert!(SortedTrieFamily::<TreeTrie>::default()
            .build_mode_axes()
            .is_empty());
        assert!(TrieLftj::<TreeTrie>::new((), Optimiser::Lexicographic)
            .build_mode_axes()
            .is_empty());
        assert_eq!(
            HashTrieFamily::<SipHashStrategy, NoPruning>::default().build_mode_axes(),
            build_mode_axis("serial")
        );
        assert_eq!(
            HashTrieFamily::<SipHashStrategy, NoPruning>::new(HashTrieConfig::default(), radix)
                .build_mode_axes(),
            build_mode_axis("radix:4")
        );
        assert_eq!(
            HashHtj::<SipHashStrategy, NoPruning>::new(
                HashTrieConfig::default(),
                radix,
                Optimiser::Lexicographic
            )
            .build_mode_axes(),
            build_mode_axis("radix:4")
        );
    }

    thread_local! {
        static HASHES: Cell<usize> = const { Cell::new(0) };
    }

    /// SipHash that counts its calls. The radix build hashes each tuple's
    /// first attribute once more than the serial build, and the two build
    /// identical tries, so the count is the only way to see which one ran.
    #[derive(Copy, Clone, Default, Debug)]
    struct CountingHash;

    impl LayoutOption for CountingHash {
        const NAME: &'static str = "counting";
    }

    impl HashStrategy for CountingHash {
        fn hash(key: usize) -> u64 {
            HASHES.set(HASHES.get() + 1);
            SipHashStrategy::hash(key)
        }
    }

    /// Both modes build identical tries, so only a spy can see whether the
    /// family's mode reached the build, on every route a relation is built.
    #[test]
    fn hash_families_build_with_their_mode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("r.csv");
        std::fs::write(&path, "a,b\n1,2\n1,3\n2,4\n").expect("write csv");
        let header = || RelationHeader::new_positional("r", 2);
        let tuples = || vec![vec![1, 2], vec![1, 3], vec![2, 4]];
        let config = HashTrieConfig::default();
        let structure = |mode| HashTrieFamily::<CountingHash, NoPruning>::new(config, mode);
        let join = |mode| {
            HashHtj::<CountingHash, NoPruning>::new(config, mode, Optimiser::Lexicographic)
        };
        let routes: [(&str, &dyn Fn(HashTrieBuildMode)); 5] = [
            ("HashTrieFamily::build_relation", &|mode| {
                structure(mode).build_relation(header(), tuples());
            }),
            ("HashTrieFamily::load_with_tuples", &|mode| {
                structure(mode).load_with_tuples(&path).expect("load");
            }),
            ("HashHtj::build_relation", &|mode| {
                join(mode).build_relation(header(), tuples());
            }),
            ("HashHtj::load", &|mode| {
                join(mode).load(&path).expect("load");
            }),
            ("HashHtj::build_from_tuples", &|mode| {
                join(mode).build_from_tuples(vec![(header(), tuples())]);
            }),
        ];
        let hashes = |build: &dyn Fn()| {
            HASHES.set(0);
            build();
            HASHES.get()
        };
        let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
        for (route, build) in routes {
            let serial = hashes(&|| build(HashTrieBuildMode::Serial));
            let radixed = hashes(&|| build(radix));
            assert!(
                radixed > serial,
                "{route}: the radix build hashed {radixed} times and the serial build {serial}: \
                 the mode did not reach the build"
            );
        }
    }
```

Replace the test `sweep_attaches_the_build_mode_to_the_column_trie_cell_only` with:

```rust
    /// Each structure's `--ds-build` mode reaches its own cell of a sweep
    /// and no other.
    #[test]
    fn sweep_attaches_each_build_mode_to_its_own_cell() {
        let radix = HashTrieBuildMode::Radix(RadixBits::new(4).unwrap());
        let choices = DsChoices {
            build: BuildModes {
                column_trie: ColumnTrieBuildMode::Incremental,
                hash_trie: radix,
            },
            ..DsChoices::default()
        };
        let sweep = Sweep::expand(&all_structures(), &all_algorithms(), choices);
        assert!(sweep
            .cells
            .contains(&Execution::TrieLftj(SortedTrie::ColumnTrie {
                seek: SeekChoice::Galloping,
                build: ColumnTrieBuildMode::Incremental
            })));
        assert!(sweep
            .cells
            .contains(&Execution::TrieLftj(SortedTrie::TreeTrie {
                seek: SeekChoice::Galloping
            })));
        assert!(sweep.cells.contains(&Execution::HashHtj {
            hasher: HasherChoice::Sip,
            pruning: PruningChoice::Off,
            config: HashTrieConfig::default(),
            build: radix,
        }));
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --lib execution::tests`
Expected: compile errors. `BuildModes` is missing, `HashTrieFamily::new` takes one argument, and `HashHtj` has no `build` field.

- [ ] **Step 3: Implement**

3a. **`options.rs`**: add `HashTrieBuildMode` to the `kermit_ds` import. Add this after `validate_build_choices`:

```rust
/// The build mode of every structure that has one: what `--ds-build`
/// resolves to, with each structure's default where no pair names it.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BuildModes {
    /// Reaches the column-trie cell only.
    pub column_trie: ColumnTrieBuildMode,
    /// Reaches the hash-trie cell only.
    pub hash_trie: HashTrieBuildMode,
}
```

In `DsChoices`, replace the `build` field and its doc with:

```rust
    /// `--ds-build`; each structure's mode reaches its own cell.
    pub build: BuildModes,
```

In `DsChoices::resolve`, replace `build: build.column_trie_build_resolved(),` with:

```rust
            build: BuildModes {
                column_trie: build.column_trie_build_resolved(),
                hash_trie: HashTrieBuildMode::default(),
            },
```

In the options test `ds_choices_resolve_carries_the_build_mode`, replace the two assertions on `choices.build` / `DsChoices::default().build` with:

```rust
        assert_eq!(choices.build.column_trie, ColumnTrieBuildMode::Incremental);
        assert_eq!(DsChoices::default().build, BuildModes::default());
        assert_eq!(BuildModes::default().column_trie, ColumnTrieBuildMode::Bulk);
        assert_eq!(BuildModes::default().hash_trie, HashTrieBuildMode::Serial);
```

(The test module uses `super::*`, so `BuildModes` and `HashTrieBuildMode` are in scope.)

3b. **`execution.rs` top level:**
- In the `kermit_ds` import, remove `ConfigurableRelation` and add `HashTrieBuildMode`.
- Change the comment above `enum Execution` to: `// \`Copy\` relies on \`HashTrieConfig\`, \`ColumnTrieBuildMode\` and \`HashTrieBuildMode\` being \`Copy\`; a future Config or BuildMode carrying heap data would have to drop it here and clone the cells instead.`, keeping the existing line wrapping style.
- In `Execution::HashHtj`, add after `config`:
  ```rust
        /// The `--ds-build` mode every relation is built with.
        build: HashTrieBuildMode,
  ```
  Then extend the variant's doc: `` …and the runtime values by `--ds-config`, built by the `--ds-build` mode `build`. ``
- In `for_pair` and `for_structure`:
  - the `ColumnTrie` arms become `build: build.column_trie`;
  - the `HashTrie` arms become `Execution::HashHtj { hasher, pruning, config, build: build.hash_trie }`;
  - in `for_pair`'s doc, replace `and the column-trie cell's build mode` with `and each structure's build mode`.

3c. **`HashTrieFamily`:**

```rust
pub struct HashTrieFamily<H, P> {
    config: HashTrieConfig,
    build: HashTrieBuildMode,
    _layout: PhantomData<(H, P)>,
}

impl<H, P> HashTrieFamily<H, P> {
    /// The family building every relation with the `--ds-config` values
    /// `config`, by the `--ds-build` mode `build`.
    pub fn new(config: HashTrieConfig, build: HashTrieBuildMode) -> Self {
        Self {
            config,
            build,
            _layout: PhantomData,
        }
    }
}

impl<H, P> Default for HashTrieFamily<H, P> {
    fn default() -> Self { Self::new(HashTrieConfig::default(), HashTrieBuildMode::default()) }
}
```

In its `RelationFamily` impl:
- `execution()` adds `build: self.build,`.
- `build_relation` becomes `HashTrie::<H, P>::from_tuples_with_config_and_build_mode(header, self.config, self.build, tuples)`.
- `build_mode_axes` becomes:

```rust
    /// Every `HashTrie` report says which build made it, so kermit-lab can
    /// read a `HashTrie` report without the axis as the `serial` build, the
    /// only one before issue #91.
    fn build_mode_axes(&self) -> BTreeMap<String, serde_json::Value> {
        BTreeMap::from([(
            "ds_build_mode".to_string(),
            serde_json::Value::from(self.build.axis_value()),
        )])
    }
```

Also update the struct doc. It reads "Carries the `--ds-config` runtime values every relation is built with"; make it "Carries the `--ds-config` runtime values and the `--ds-build` mode every relation is built with".

3d. **`HashHtj::new`:**

```rust
    /// Creates the family for the `--ds-config` values `config` and the
    /// `--ds-build` mode `build`, planned by `optimiser`. The
    /// `--ds-layout-hasher` / `--ds-layout-pruning` labels are *not*
    /// parameters: [`execution`](RelationFamily::execution) derives them from
    /// `H` and `P`, so a report cannot name a Layout the family was not
    /// monomorphised over.
    pub fn new(config: HashTrieConfig, build: HashTrieBuildMode, optimiser: Optimiser) -> Self {
        Self {
            structure: HashTrieFamily::new(config, build),
            optimiser: optimiser.instantiate(),
        }
    }
```

3e. **Dispatch arms:**
- In `kermit/src/bench/ds.rs`, `kermit/src/bench/run.rs` and `kermit/src/main.rs` (`load_query_runner`), add `build,` to each `Execution::HashHtj { hasher, pruning, config }` pattern.
- Pass it on: `HashTrieFamily::<H, P>::new(config, build)` in `ds.rs`, and `HashHtj::<H, P>::new(config, build, optimiser)` in `run.rs` and `main.rs`.

3f. **Remaining `execution.rs` test call sites.** These are mechanical; the compiler lists each one:
- `execution_axes_round_trip_through_for_pair`: add `build: HashTrieBuildMode::Serial,` to the expected `Execution::HashHtj { … }`.
- `for_structure_agrees_with_for_pair`: replace `for build in [ColumnTrieBuildMode::Incremental, ColumnTrieBuildMode::Bulk] {` with
  ```rust
        let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
        let builds = [ColumnTrieBuildMode::Incremental, ColumnTrieBuildMode::Bulk]
            .into_iter()
            .flat_map(|column_trie| {
                [HashTrieBuildMode::Serial, radix].map(|hash_trie| BuildModes {
                    column_trie,
                    hash_trie,
                })
            });
        for build in builds {
  ```
  The `DsChoices { …, build }` literal inside is unchanged.
- `structure_markers_agree_with_join_families`: make the two calls `HashTrieFamily::<…>::new(config, radix)` and `HashHtj::<…>::new(config, radix, Optimiser::Lexicographic)`, with `let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());` above them.
- `families_report_their_own_execution`: make it `HashHtj::<…>::new(config, radix, Optimiser::Lexicographic)` with the same `radix` binding, and add `build: radix,` to the expected cell.
- `hash_family_labels_are_derived_from_its_layout_types`, `hash_family_builds_relations_with_its_config`, `hash_family_load_honours_its_config`, `hash_family_build_relation_honours_its_config`, `pruned_family_reports_the_pruning_layout` and `count_agrees_with_join_in_every_family`: insert `HashTrieBuildMode::Serial,` as the second argument of each `HashHtj::<…>::new(…)`.

- [ ] **Step 4: Update the ColumnTrie CLI test for HashTrie's new axis**

In `kermit/tests/cli_column_trie_build_mode.rs`:
- Edit the module doc's last two sentences to read: `The other structures: TreeTrie has a single build and carries no such axis; HashTrie reports its own (\`serial\` by default — see \`cli_hash_trie_build_mode.rs\`).`
- Replace `cli_bench_ds_other_structures_have_no_build_mode` with:

```rust
#[test]
fn cli_bench_ds_tree_trie_has_no_build_mode() {
    let (output, report) = bench_ds("tree-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let axes = axes_of(&report);
    assert!(axes.get("ds_build_mode").is_none(), "{axes}");
}
```

- In `cli_bench_run_sweep_reports_build_mode_only_on_column_trie` (rename it `cli_bench_run_sweep_reports_each_structures_build_mode`) and in `cli_bench_run_sweep_carries_build_mode_only_to_column_trie_cells`, replace the `if … else` over `data_structure` with:

```rust
        match axes["data_structure"].as_str() {
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | Some("HashTrie") => assert_eq!(axes["ds_build_mode"], "serial", "{axes}"),
            | _ => assert!(axes.get("ds_build_mode").is_none(), "{axes}"),
        }
```

In the second test the ColumnTrie arm expects `"incremental"`. Its spelling of `--ds-build` changes in Task 6.

- [ ] **Step 5: Run**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --lib`
Expected: PASS, including `families_with_a_build_mode_report_it`, `hash_families_build_with_their_mode` and `sweep_attaches_each_build_mode_to_its_own_cell`.

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_column_trie_build_mode`
Expected: PASS.

Run: `RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo clippy -p kermit --all-targets`
Expected: clean. In particular there is no unused import of `ConfigurableRelation`, because it now lives only in the tests.

- [ ] **Step 6: Mutant check for the spy**

Commit first (Step 7). Then in `HashTrieFamily::build_relation`, replace `self.build` with `HashTrieBuildMode::Serial`.
Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --lib hash_families_build_with_their_mode`
Expected: FAIL on the `HashTrieFamily::build_relation` route. Restore with `git checkout -- kermit/src/execution.rs`.

- [ ] **Step 7: Commit (before Step 6's mutant)**

```bash
git add kermit/src/options.rs kermit/src/execution.rs kermit/src/bench/ds.rs kermit/src/bench/run.rs kermit/src/main.rs kermit/tests/cli_column_trie_build_mode.rs
git commit -m "feat(kermit): HashTrie cells and families carry a build mode, reported as ds_build_mode (#91)" -m "<attribution lines>"
```

---

### Task 6: Keyed `--ds-build`

**Files:**
- Modify: `kermit/src/options.rs` (`DsFlag`, `BuildChoices`, `validate_build_choices`, `DsChoices::resolve`, helpers, tests)
- Modify: `kermit/src/bench/run.rs` (the `resolve_sweep` test table)
- Modify: `kermit/tests/cli_column_trie_build_mode.rs`, `kermit/tests/cli_bench_run_ds_flag_reach.rs`
- Create: `kermit/tests/cli_hash_trie_build_mode.rs`
- Modify: `python/kermit-lab/tests/test_contract.py:79`

- [ ] **Step 1: Write the failing unit tests in `options.rs`**

In the `tests` module, add a helper and the two new tests:

```rust
    fn build(pairs: &[&str]) -> BuildChoices {
        BuildChoices {
            ds_build: pairs.iter().map(|pair| pair.to_string()).collect(),
        }
    }

    #[test]
    fn build_choices_resolve_keyed_pairs_per_structure() {
        let radix8 = HashTrieBuildMode::Radix(kermit_ds::RadixBits::new(8).unwrap());
        assert_eq!(build(&[]).resolved().unwrap(), BuildModes::default());
        assert_eq!(build(&["hash-trie=serial"]).resolved().unwrap(), BuildModes::default());
        assert_eq!(build(&["hash-trie=radix:8"]).resolved().unwrap(), BuildModes {
            column_trie: ColumnTrieBuildMode::Bulk,
            hash_trie: radix8,
        });
        assert_eq!(
            build(&["column-trie=incremental", "hash-trie=radix:8"])
                .resolved()
                .unwrap(),
            BuildModes {
                column_trie: ColumnTrieBuildMode::Incremental,
                hash_trie: radix8,
            }
        );
    }

    /// Every malformed `--ds-build` is a usage error naming what is wrong.
    #[test]
    fn build_choices_reject_malformed_pairs() {
        let cases: &[(&[&str], &str)] = &[
            (&["incremental"], "did you mean --ds-build column-trie=incremental?"),
            (&["radix:8"], "did you mean --ds-build hash-trie=radix:8?"),
            (&["fast"], "for example --ds-build"),
            (&["tree-trie=bulk"], "tree-trie has a single build process"),
            (&["b-tree=bulk"], "unknown structure \"b-tree\""),
            (
                &["hash-trie=serial", "hash-trie=radix:4"],
                "hash-trie given more than once",
            ),
            (&["hash-trie=bulk"], "unknown hash-trie build mode \"bulk\""),
            (
                &["column-trie=radix:8"],
                "unknown mode \"radix:8\"; expected incremental or bulk",
            ),
            (&["hash-trie=radix"], "radix needs a bit count"),
            (&["hash-trie=radix:0"], "between 1 and 16"),
            (&["hash-trie=radix:17"], "between 1 and 16"),
            (&["hash-trie=radix:x"], "whole number"),
        ];
        for &(pairs, expected) in cases {
            let msg = build(pairs).resolved().unwrap_err().to_string();
            assert!(msg.contains("--ds-build"), "{pairs:?}: {msg}");
            assert!(msg.contains(expected), "{pairs:?}: {msg}");
        }
    }
```

Rewrite the three existing build tests to the keyed form:

```rust
    #[test]
    fn validate_build_choices_accepts_each_key_on_its_structure_or_all() {
        use IndexStructureSelector::{All, ColumnTrie, HashTrie, TreeTrie};
        let column = build(&["column-trie=incremental"]);
        let hash = build(&["hash-trie=radix:4"]);
        for (choices, key, home) in [(&column, "column-trie", ColumnTrie), (&hash, "hash-trie", HashTrie)] {
            assert!(validate_build_choices(home, choices).is_ok());
            assert!(validate_build_choices(All, choices).is_ok());
            for sel in [TreeTrie, ColumnTrie, HashTrie] {
                if sel == home {
                    continue;
                }
                let msg = validate_build_choices(sel, choices).unwrap_err().to_string();
                assert!(msg.contains(&format!("--ds-build {key}")), "{msg}");
                assert!(msg.contains(&format!("--indexstructure {key}")), "{msg}");
            }
        }
        for sel in [TreeTrie, ColumnTrie, HashTrie, All] {
            assert!(validate_build_choices(sel, &BuildChoices::default()).is_ok());
        }
    }

    #[test]
    fn ds_choices_resolve_carries_the_build_mode() {
        let radix = HashTrieBuildMode::Radix(kermit_ds::RadixBits::new(4).unwrap());
        let choices = DsChoices::resolve(
            IndexStructureSelector::All,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build(&["column-trie=incremental", "hash-trie=radix:4"]),
        )
        .unwrap();
        assert_eq!(choices.build, BuildModes {
            column_trie: ColumnTrieBuildMode::Incremental,
            hash_trie: radix,
        });
        assert_eq!(DsChoices::default().build, BuildModes::default());
        assert_eq!(BuildModes::default().column_trie, ColumnTrieBuildMode::Bulk);
        assert_eq!(BuildModes::default().hash_trie, HashTrieBuildMode::Serial);
        assert!(DsChoices::resolve(
            IndexStructureSelector::TreeTrie,
            &LayoutChoices::default(),
            &ConfigChoices::default(),
            &build(&["column-trie=incremental"]),
        )
        .is_err());
    }
```

(`IndexStructureSelector` derives `PartialEq`, at `main.rs:112`.)

In `ds_flag_given_lists_exactly_the_flags_passed`:
- replace the `let build = BuildChoices { column_trie_build: … };` with `let builds = build(&["column-trie=incremental", "hash-trie=radix:4"]);`. Name it `builds`, not `build`, so the `build` helper stays callable below.
- pass `&builds` in place of `&build` to the two `DsFlag::given` calls that used it;
- the two expected vectors end `DsFlag::Build(IndexStructure::ColumnTrie), DsFlag::Build(IndexStructure::HashTrie)` in place of `DsFlag::Build`;
- append:
  ```rust
        // Malformed pairs yield no flag; `resolved` rejects them.
        assert_eq!(
            DsFlag::given(
                &LayoutChoices::default(),
                &ConfigChoices::default(),
                &build(&["incremental", "tree-trie=bulk", "b-tree=x"])
            ),
            vec![]
        );
  ```

In `unreached_flag_is_the_first_flag_no_structure_has`, replace every `DsFlag::Build` with `DsFlag::Build(IndexStructure::ColumnTrie)`, and add:

```rust
        // A hash-trie build pair reaches no sorted trie.
        assert_eq!(
            unreached_flag(&[DsFlag::Build(IndexStructure::HashTrie)], &sorted),
            Some(DsFlag::Build(IndexStructure::HashTrie))
        );
```

In `kermit/src/bench/run.rs`, inside `resolve_sweep_rejects_a_flag_the_algorithm_leaves_without_a_cell`, add before `let rows`:

```rust
        let column = Build(kermit_ds::IndexStructure::ColumnTrie);
        let hash = Build(kermit_ds::IndexStructure::HashTrie);
```

Then in the table, `Build` becomes `column` in the LFTJ "reached" and HTJ "unreached" rows. `hash` is added to the LFTJ "unreached" and HTJ "reached" rows, and both go in the `All` row.

- [ ] **Step 2: Run to verify failure**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --lib options::tests`
Expected: compile errors: no field `ds_build`, no method `resolved`, and `DsFlag::Build` takes no argument.

- [ ] **Step 3: Implement**

3a. **`DsFlag`**: change the variant to

```rust
    /// `--ds-build <structure>=…`: one per pair, each reaching only its
    /// structure.
    Build(IndexStructure),
```

In `structures()`:

```rust
            | Self::Build(IndexStructure::ColumnTrie) => &[IndexStructure::ColumnTrie],
            | Self::Build(IndexStructure::HashTrie) => &[IndexStructure::HashTrie],
            // Never constructed: `BuildChoices::given` emits no tree-trie pair,
            // since it has a single build. Were it built, it would reach nothing.
            | Self::Build(IndexStructure::TreeTrie) => &[],
```

Make `structures_label` use a new free helper, and add that helper beside `unreached_flag`:

```rust
/// `ds` as `-i` spells it: `"hash-trie"`.
fn cli_name(ds: IndexStructure) -> String {
    ds.to_possible_value()
        .expect("every IndexStructure is a CLI value")
        .get_name()
        .to_owned()
}
```

`structures_label` becomes:

```rust
    pub(crate) fn structures_label(self) -> String {
        self.structures()
            .iter()
            .map(|&ds| cli_name(ds))
            .collect::<Vec<_>>()
            .join(" or ")
    }
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
            | Self::Build(ds) => write!(f, "--ds-build {}", cli_name(*ds)),
        }
    }
}
```

3b. **`BuildChoices`**: replace the whole struct, its impl and `validate_build_choices` with:

```rust
/// BuildMode-axis CLI choices, flattened beside [`LayoutChoices`] and
/// [`ConfigChoices`] into `bench ds`, `bench run` and `bench join`. One flag,
/// `--ds-build`, takes comma-separated `structure=mode` pairs, resolved per
/// structure by [`resolved`](Self::resolved). Every build mode builds the
/// same structure, so the flag changes build time only. `kermit join` takes
/// no `--ds-build`, for the same reason it takes no `--ds-config`: it cannot
/// change a query's answers.
#[derive(Args, Clone, Debug, Default)]
pub(crate) struct BuildChoices {
    /// How each named structure is built from its tuples, as
    /// `structure=mode` pairs: `column-trie=bulk|incremental` (default
    /// `bulk`; `incremental` is the build before the one-pass bulk build) and
    /// `hash-trie=serial|radix:<bits>` (default `serial`; bits in 1..=16).
    /// A pair is only valid when `--indexstructure` selects its structure
    /// (or `all`).
    #[arg(long = "ds-build", value_name = "STRUCTURE=MODE,...", value_delimiter = ',')]
    ds_build: Vec<String>,
}

impl BuildChoices {
    /// The structures that have a build mode, as `--ds-build` keys.
    const STRUCTURES: &'static [IndexStructure] =
        &[IndexStructure::ColumnTrie, IndexStructure::HashTrie];

    /// One [`DsFlag::Build`] per pair whose key names a structure with a
    /// build mode, each reaching only that structure. Malformed pairs yield
    /// no flag here; [`resolved`](Self::resolved) rejects them.
    fn given(&self) -> Vec<DsFlag> {
        self.ds_build
            .iter()
            .filter_map(|pair| {
                let (key, _) = pair.split_once('=')?;
                let ds = <IndexStructure as ValueEnum>::from_str(key, false).ok()?;
                Self::STRUCTURES.contains(&ds).then_some(DsFlag::Build(ds))
            })
            .collect()
    }

    /// Resolves the pairs into each structure's mode, starting from the
    /// defaults. A bare mode, a structure without build modes, an unknown
    /// structure or mode, and a repeated structure are usage errors.
    pub(crate) fn resolved(&self) -> anyhow::Result<BuildModes> {
        let mut modes = BuildModes::default();
        let mut seen = Vec::new();
        for pair in &self.ds_build {
            let Some((key, mode)) = pair.split_once('=') else {
                anyhow::bail!(
                    "--ds-build expects structure=mode pairs; got {pair:?}{}",
                    bare_mode_hint(pair)
                );
            };
            let ds = <IndexStructure as ValueEnum>::from_str(key, false).map_err(|_| {
                anyhow::anyhow!(
                    "--ds-build: unknown structure {key:?}; structures with build modes: {}",
                    Self::structures_label()
                )
            })?;
            if !Self::STRUCTURES.contains(&ds) {
                anyhow::bail!(
                    "--ds-build: {key} has a single build process; structures with build modes: \
                     {}",
                    Self::structures_label()
                );
            }
            if seen.contains(&ds) {
                anyhow::bail!("--ds-build: {key} given more than once");
            }
            seen.push(ds);
            match ds {
                | IndexStructure::ColumnTrie => {
                    modes.column_trie = <ColumnTrieBuildMode as ValueEnum>::from_str(mode, false)
                        .map_err(|_| {
                        anyhow::anyhow!(
                            "--ds-build column-trie: unknown mode {mode:?}; expected {}",
                            column_trie_mode_names().join(" or ")
                        )
                    })?;
                },
                | IndexStructure::HashTrie => {
                    modes.hash_trie = mode
                        .parse()
                        .map_err(|why| anyhow::anyhow!("--ds-build hash-trie: {why}"))?;
                },
                | IndexStructure::TreeTrie => unreachable!("rejected above: no build modes"),
            }
        }
        Ok(modes)
    }

    /// [`STRUCTURES`](Self::STRUCTURES) as `-i` spells them.
    fn structures_label() -> String {
        Self::STRUCTURES
            .iter()
            .map(|&ds| cli_name(ds))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// `ColumnTrieBuildMode`'s modes as `--ds-build column-trie=` spells them.
fn column_trie_mode_names() -> Vec<String> {
    ColumnTrieBuildMode::value_variants()
        .iter()
        .map(|mode| {
            mode.to_possible_value()
                .expect("every ColumnTrieBuildMode is a CLI value")
                .get_name()
                .to_owned()
        })
        .collect()
}

/// The rest of the error for a bare `--ds-build` mode: the keyed spelling
/// for each structure that has that mode, or an example when none does.
fn bare_mode_hint(mode: &str) -> String {
    let mut keyed = Vec::new();
    if <ColumnTrieBuildMode as ValueEnum>::from_str(mode, false).is_ok() {
        keyed.push(format!("column-trie={mode}"));
    }
    if mode.parse::<HashTrieBuildMode>().is_ok() {
        keyed.push(format!("hash-trie={mode}"));
    }
    if keyed.is_empty() {
        "; for example --ds-build column-trie=incremental,hash-trie=radix:8".to_owned()
    } else {
        format!("; did you mean --ds-build {}?", keyed.join(" or "))
    }
}

/// Rejects a `--ds-build` pair whose structure `indexstructure` does not
/// select, so a report can never carry a `ds_build_mode` the build ignored.
/// Same discipline as [`validate_config_choices`].
pub(crate) fn validate_build_choices(
    indexstructure: IndexStructureSelector, build: &BuildChoices,
) -> anyhow::Result<()> {
    validate_ds_flags(indexstructure, &build.given())
}
```

3c. In `DsChoices::resolve`, replace the `BuildModes { … }` literal from Task 5 with `build: build.resolved()?,`. `DsFlag::given` keeps `given.extend(build.given());`, which works for a `Vec`.

Make sure `BuildModes` sits next to `BuildChoices`: move it here if Task 5 placed it elsewhere.

- [ ] **Step 4: Run the unit tests**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --lib`
Expected: PASS, including `build_choices_resolve_keyed_pairs_per_structure`, `build_choices_reject_malformed_pairs`, the rewritten build tests, `unreached_flag_is_the_first_flag_no_structure_has` and `resolve_sweep_rejects_a_flag_the_algorithm_leaves_without_a_cell`.

- [ ] **Step 5: Update the CLI integration tests to the keyed spelling**

In `kermit/tests/cli_column_trie_build_mode.rs`:
- `"--ds-build", "incremental"` becomes `"--ds-build", "column-trie=incremental"`, in all three places.
- Replace `cli_bench_ds_rejects_ds_build_off_column_trie` with:

```rust
#[test]
fn cli_bench_ds_rejects_a_column_trie_pair_off_column_trie() {
    for ds in ["tree-trie", "hash-trie"] {
        let (output, _) = bench_ds(ds, &["--ds-build", "column-trie=bulk"]);
        assert!(!output.status.success(), "{ds} accepted a column-trie pair");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--ds-build column-trie"), "{ds}: {stderr}");
    }
}

/// The pre-#91 bare spelling is a usage error that names the keyed one.
#[test]
fn cli_bench_ds_rejects_the_bare_spelling_with_a_hint() {
    let (output, _) = bench_ds("column-trie", &["--ds-build", "incremental"]);
    assert!(!output.status.success(), "bare --ds-build was accepted");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("column-trie=incremental"), "{stderr}");
}
```

In `kermit/tests/cli_bench_run_ds_flag_reach.rs`:
- `ds_build_is_rejected_when_htj_leaves_no_column_trie_cell` passes `&["--ds-build", "column-trie=incremental"]` and expects flag text `"--ds-build column-trie"`.
- Add:

```rust
#[test]
fn ds_build_hash_trie_is_rejected_when_lftj_leaves_no_hash_trie_cell() {
    let (output, report) = narrowed_sweep("leapfrog-triejoin", &["--ds-build", "hash-trie=radix:4"]);
    assert_rejected_unreached(&output, &report, "--ds-build hash-trie");
}
```

- In `every_flag_reaches_its_cell_under_all_all`, the `--ds-build` value becomes `"column-trie=incremental,hash-trie=radix:2"`, and the `Some("HashTrie")` arm gains `assert_eq!(axes["ds_build_mode"], "radix:2", "{axes}");`.

- [ ] **Step 6: Write the HashTrie CLI smoke test**

Create `kermit/tests/cli_hash_trie_build_mode.rs`:

```rust
//! CLI smoke test for `HashTrie`'s `ds_build_mode` axis (issue #91). Every
//! `HashTrie` report says which build made its relations: `serial` by
//! default, `radix:<bits>` under `--ds-build hash-trie=radix:<bits>`. Every
//! mode builds the identical trie, so only the axis (and build time) shows
//! which ran.

mod common;

use common::cli::{axes_of, bench_ds, bench_join, bench_run, reports_of};

#[test]
fn cli_bench_ds_hash_trie_records_serial_build_mode() {
    let (output, report) = bench_ds("hash-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "serial");
}

#[test]
fn cli_bench_ds_with_radix_build_records_axis() {
    let (output, report) = bench_ds("hash-trie", &["--ds-build", "hash-trie=radix:4"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "radix:4");
}

#[test]
fn cli_bench_join_with_radix_build_records_axis() {
    let (output, report) = bench_join("hash-trie", "hash-triejoin", &[
        "-m",
        "space",
        "--ds-build",
        "hash-trie=radix:4",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_build_mode"], "radix:4");
}

#[test]
fn cli_bench_run_sweep_carries_each_build_mode_to_its_cell() {
    let (output, report) = bench_run("triangle", &[
        "-i",
        "all",
        "-a",
        "all",
        "-m",
        "space",
        "--ds-build",
        "hash-trie=radix:4",
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
            | Some("HashTrie") => assert_eq!(axes["ds_build_mode"], "radix:4", "{axes}"),
            | Some("ColumnTrie") => assert_eq!(axes["ds_build_mode"], "bulk", "{axes}"),
            | _ => assert!(axes.get("ds_build_mode").is_none(), "{axes}"),
        }
    }
}

#[test]
fn cli_bench_ds_rejects_malformed_hash_trie_modes() {
    for mode in ["radix", "radix:0", "radix:17", "bulk"] {
        let pair = format!("hash-trie={mode}");
        let (output, _) = bench_ds("hash-trie", &["--ds-build", &pair]);
        assert!(!output.status.success(), "{pair} was accepted");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--ds-build hash-trie"), "{pair}: {stderr}");
    }
}
```

- [ ] **Step 7: The kermit-lab contract test**

In `python/kermit-lab/tests/test_contract.py`, in `test_bench_ds_column_trie_reports_its_build_mode`, change `"--ds-build", "incremental",` to `"--ds-build", "column-trie=incremental",`.

- [ ] **Step 8: Run**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_column_trie_build_mode --test cli_bench_run_ds_flag_reach --test cli_hash_trie_build_mode`
Expected: PASS.

Run: `CARGO_BUILD_JOBS=2 cargo build -p kermit && cd python/kermit-lab && KERMIT_BIN=$(git rev-parse --show-toplevel)/target/debug/kermit uv run pytest tests/test_contract.py -q; cd -`
Expected: PASS, not skipped.

Run: `RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo clippy -p kermit --all-targets`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add kermit/src/options.rs kermit/src/bench/run.rs kermit/tests/cli_column_trie_build_mode.rs kermit/tests/cli_bench_run_ds_flag_reach.rs kermit/tests/cli_hash_trie_build_mode.rs python/kermit-lab/tests/test_contract.py
git commit -m "feat(cli)!: --ds-build takes structure=mode pairs, adding hash-trie=radix:<bits> (#91)" -m "The bare form (--ds-build incremental) is rejected with a hint naming the keyed spelling (column-trie=incremental)." -m "<attribution lines>"
```

---

### Task 7: Join suites under the radix build

**Files:**
- Modify: `kermit/tests/common/utils.rs` (imports, a `JoinEntry` impl)
- Modify: `kermit/tests/join_tests.rs` (imports, provider, four invocations)

- [ ] **Step 1: Add the invocations (they fail to compile first)**

In `kermit/tests/join_tests.rs`, add `HashTrieBuildMode, RadixBits` to the `kermit_ds` import, then append after the ColumnTrie BuildMode block:

```rust
// ── BuildMode axis: HashTrie's build ────────────────────────────────────
// The plain HashTrie invocations above build `serial` (the default); these
// run the radix build, which must build the identical trie (issue #91). Two
// bits make four partitions, so the 3–5 tuple fixtures spread over several
// partitions with several keys in each. Sip/off and Fx/on cover both
// hashers and both pruning policies.
define_build_mode_provider!(
    Radix2,
    HashTrieBuildMode,
    HashTrieBuildMode::Radix(RadixBits::new(2).unwrap())
);

define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    LexicographicOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    CardinalityOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    LexicographicOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    CardinalityOptimiser,
    Radix2
);
```

- [ ] **Step 2: Run to verify failure**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests radix2`
Expected: compile error. `HashTriejoin: JoinEntry<BuiltWith<HashTrie<…>, Radix2>>` is not satisfied.

- [ ] **Step 3: Add the `JoinEntry` impl**

In `kermit/tests/common/utils.rs`, extend the `kermit_ds` import to:

```rust
    kermit_ds::{
        BuildModeProvider, BuiltWith, Cardinality, ConfigProvider, Configured, HashTrie,
        HashTrieBuildMode, HashTrieConfig, PruningPolicy, Relation,
    },
```

and add after the `Configured` impl:

```rust
impl<H: HashStrategy, P: PruningPolicy, B: BuildModeProvider<HashTrieBuildMode>>
    JoinEntry<BuiltWith<HashTrie<H, P>, B>> for HashTriejoin
{
    fn join(
        relations: &BTreeMap<String, BuiltWith<HashTrie<H, P>, B>>, query: JoinQuery,
        optimiser: &dyn QueryOptimiser,
    ) -> Result<Vec<Vec<usize>>, JoinError> {
        hash_join::<BuiltWith<HashTrie<H, P>, B>, H>(relations, query, optimiser)
    }

    fn count(
        relations: &BTreeMap<String, BuiltWith<HashTrie<H, P>, B>>, query: JoinQuery,
        optimiser: &dyn QueryOptimiser,
    ) -> Result<usize, JoinError> {
        let mut rows = 0;
        hash_join_for_each::<BuiltWith<HashTrie<H, P>, B>, H>(
            relations,
            query,
            optimiser,
            |_| rows += 1,
        )?;
        Ok(rows)
    }
}
```

- [ ] **Step 4: Run**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests radix2`
Expected: PASS. That is 4 modules × the standard patterns; check that the count printed is 4× the per-suite pattern count.

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests`
Expected: PASS (all suites).

- [ ] **Step 5: Commit**

```bash
git add kermit/tests/common/utils.rs kermit/tests/join_tests.rs
git commit -m "test(kermit): join suites under the radix HashTrie build, both optimisers (#91)" -m "<attribution lines>"
```

---

## Phase 3 — kermit-lab and docs

### Task 8: The `serial` back-fill

**Files:**
- Modify: `python/kermit-lab/kermit_lab/defaults.py:23-38`
- Modify: `python/kermit-lab/tests/test_defaults.py:86-95`

- [ ] **Step 1: Update the test first**

Replace `test_build_mode_backfills_column_trie_rows_only` with:

```python
def test_build_mode_backfills_column_trie_and_hash_trie_rows() -> None:
    """Each structure with a build mode back-fills its own pre-axis build:
    ColumnTrie's pre-#84 ``incremental``, HashTrie's pre-#91 ``serial``.
    A row that carries the axis keeps it, and TreeTrie (single build) stays
    NaN."""
    df = pd.DataFrame({
        "data_structure": ["ColumnTrie", "ColumnTrie", "TreeTrie", "HashTrie", "HashTrie"],
        "ds_build_mode": [pd.NA, "bulk", pd.NA, pd.NA, "radix:8"],
    })
    out = apply_axis_defaults(df)
    assert list(out["ds_build_mode"].iloc[[0, 1, 3, 4]]) == [
        "incremental", "bulk", "serial", "radix:8",
    ]
    assert pd.isna(out["ds_build_mode"].iloc[2])
    assert SCOPED_AXIS_DEFAULTS[("ds_build_mode", "ColumnTrie")] == "incremental"
    assert SCOPED_AXIS_DEFAULTS[("ds_build_mode", "HashTrie")] == "serial"
```

- [ ] **Step 2: Run to verify failure**

Run: `cd python/kermit-lab && uv run pytest tests/test_defaults.py -q; cd -`
Expected: FAIL. The HashTrie row is still NaN, and the `KeyError` on `("ds_build_mode", "HashTrie")` follows.

- [ ] **Step 3: Add the default**

In `defaults.py`'s `SCOPED_AXIS_DEFAULTS`, after the ColumnTrie `ds_build_mode` entry:

```python
    # HashTrie built one insert per tuple before issue #91. Every HashTrie
    # report since carries the axis ("serial" by default).
    ("ds_build_mode", "HashTrie"): "serial",
```

- [ ] **Step 4: Run the whole kermit-lab suite**

Run: `cd python/kermit-lab && KERMIT_BIN=$(git rev-parse --show-toplevel)/target/debug/kermit uv run pytest -q; cd -`
Expected: PASS. If `test_render_all.py`'s build-mode test now sees a `serial` value because a fixture builds HashTrie rows without the axis, update its expected set to include `"serial"`. That is the intended new behaviour; cite #91 in a comment.

- [ ] **Step 5: Commit**

```bash
git add python/kermit-lab/kermit_lab/defaults.py python/kermit-lab/tests/test_defaults.py
git commit -m "feat(kermit-lab): back-fill serial on HashTrie rows without ds_build_mode (#91)" -m "<attribution lines>"
```

(Add `python/kermit-lab/tests/test_render_all.py` to the `git add` if Step 4 changed it.)

---

### Task 9: Documentation

**Files:** `docs/data-structures/hash-trie.md`, `docs/data-structures/column-trie.md`, `docs/specs/optimization-standard.md`, `docs/specs/bench-report-schema.md`, `USAGE.md`, `BENCHMARKING.md`, `ARCHITECTURE.md`, `docs/specs/benchmarking-architecture.md`, `CLAUDE.md`.

These changes are comment- and doc-only. Verify them with `cargo doc` and `grep`; they need no test runs.

- [ ] **Step 1: `hash-trie.md` § Build modes**

Replace the whole `### Build modes` section (currently "*None in this release.* …") with:

```markdown
### Build modes

Every mode builds the identical trie — the same buckets, the same capacities, the same `heap_size_bytes` — so the mode changes the `insertion` and `end_to_end` timings and nothing else (issue #91).

| Mode | `--ds-build` | Method |
|---|---|---|
| `Serial` (default) | `hash-trie=serial` | one `insert_at` per tuple, in input order (Algorithm 2) |
| `Radix(K)` | `hash-trie=radix:K`, K in 1..=16 | radix-partition on the top K bits of the first attribute's hash, build each partition into a scratch root, merge (SIGMOD 2020 §3.3.2) |

**The radix build** ([`radix.rs`](../../kermit-ds/src/ds/hash_trie/radix.rs)):
1. **Partition.** A histogram pass, then a stable scatter pass, puts every tuple in one of 2^K partitions with its input index.
2. **Scratch roots.** Each non-empty partition is built into a scratch root of the real root's kind, by the serial build's own `insert_at`. Whenever the scratch root gains a key, the build records the input index that introduced it.
3. **Merge.** Every scratch entry moves into the real root, sorted by that first-appearance index.

**Why it is identical.**
- A table's final layout depends only on the order in which its *new* keys arrive, because `entry_or_insert_with` returns an existing entry before its resize check.
- The merge inserts the root's distinct keys in first-appearance order, as the serial build does.
- Each root key's subtrie is built by the same `insert_at` calls, on the same tuples in the same order, because the partition is stable. That covers every `Singleton` and unprune, and every chain and capacity.

**Cost.**
- *Possible gain:* all n per-tuple probes and descents stay inside one partition, roughly 1/2^K of the trie. Only the D merge inserts, one per distinct first-attribute hash, touch the real root at random.
- *Paid:* the two partition passes, a second hash of each first attribute, an O(D log D) sort, and transient memory of 2 + 32 bytes per tuple plus the scratch tables.

**#66.** A scratch root receives keys that share their top K hash bits. The per-capacity multiplier makes that shared prefix a constant offset in the product's top bits, so the partition does not cluster. This is pinned by `absorbing_one_radix_partition_costs_no_more_than_unrestricted_keys` in [`hash_table.rs`](../../kermit-ds/src/ds/hash_trie/hash_table.rs).

- **Axis:** `ds_build_mode` (`serial` / `radix:K`), on every HashTrie report, emitted by the bench family that ran the build. kermit-lab reads a HashTrie row without the axis as `serial`, the only build before the axis existed.
- **API:** `HashTrieBuildMode` through `BuildModeRelation::from_tuples_with_build_mode`. Use `HashTrie::from_tuples_with_config_and_build_mode` to set the load factor too. `Relation::from_tuples` uses `Serial`.
- **Tests:**
  - identity: the `radix_builds_the_serial_trie_*` tests in `radix.rs` (array-level, capacities included, over arity, pruning, hasher, K, load factor and input);
  - the DS suites: the `HashTrieSipRadix2` alias in `kermit-ds/tests/{hash_trie,parquet}_tests.rs`;
  - the join suites: `define_multiway_join_test_suite_for_build_mode!` with `Radix2` in `kermit/tests/join_tests.rs`, under both optimisers.
- **Measured effect:** see "Radix build A/B" below (Task 11 fills it in).
```

Also, in the `## Complexity` table, add a row after `from_tuples(n)`:

```markdown
| `from_tuples` under `radix:K` | O(n · a + D log D) | O(n · a) | the serial inserts, plus the partition passes, a second first-attribute hash per tuple and a sort of the D distinct root keys; builds the identical trie |
```

Finally, in the `from_tuples(n)` row's notes, replace `no batch optimization in this first cut` with `the default \`serial\` build mode`.

- [ ] **Step 2: `column-trie.md`**

In the Build mode table, change the `--ds-build` column to `column-trie=bulk` / `column-trie=incremental`, and add below the table:

```markdown
`--ds-build` takes `structure=mode` pairs (issue #91), so a ColumnTrie mode is spelled `--ds-build column-trie=incremental`; the bare `--ds-build incremental` is rejected with that hint.
```

- [ ] **Step 3: `optimization-standard.md`**

- Line 141 area. Replace `The BuildMode category has one consumer, ColumnTrie:` and its code block with:

  ````markdown
  The BuildMode category has two consumers, ColumnTrie and HashTrie. `--ds-build` takes `structure=mode` pairs:

  ```bash
  # The pre-#84 ColumnTrie build, to reproduce its insertion numbers
  kermit bench run triangle -i column-trie -a leapfrog-triejoin --ds-build column-trie=incremental

  # HashTrie's radix-partitioned build (#91), 2^8 partitions
  kermit bench run triangle -i hash-trie -a hash-triejoin --ds-build hash-trie=radix:8
  ```
  ````

- In the same section, change `` `--ds-build <mode>[:<params>]` for BuildMode `` to `` `--ds-build <structure>=<mode>[:<params>],...` for BuildMode ``.
- In the BuildMode table, change `Examples (potential)` to `| Examples | ColumnTrie bulk / incremental ✓, HashTrie radix partitioning ✓, parallel build |`.
- Line 369. Replace `Only \`ColumnTrie\` implements it today.` with `\`ColumnTrie\` and \`HashTrie\` implement it. HashTrie also has a Config, so it adds one inherent constructor that takes both, \`from_tuples_with_config_and_build_mode\`, since each trait's constructor fixes the other axis to its default.`
- Walkthrough step 6, lines 636-639. Replace it with:

  ```markdown
  6. **Add the CLI.** `--ds-build` takes `structure=mode` pairs (`BuildChoices`,
     `BuildModes`, `DsChoices.build`). A new consumer adds its key to
     `BuildChoices::STRUCTURES`, a field to `BuildModes`, a match arm in
     `BuildChoices::resolved` and in `DsFlag::structures`
     (`DsFlag::Build(IndexStructure)`), and a `build` slot on its `Execution` cell.
  ```

- § What's implemented. Change "Five optimizations are implemented — three Layout dimensions, one Config value and one BuildMode:" to "Six optimizations are implemented — three Layout dimensions, one Config value and two BuildModes:". Add the table row `| HashTrie radix-partitioned build (serial / radix:K) | BuildMode | \`ds_build_mode\` | §3.3.2 (issue #91) |`. Change the line after the table to say `define_multiway_join_test_suite_for_build_mode!` covers both BuildMode consumers. Delete the `| Radix partitioning | BuildMode | Medium | §3.3.2 |` row from "Available to add".

- [ ] **Step 4: `bench-report-schema.md`**

Replace the `ds_build_mode` bullet's last two sentences, from `Emitted only on ColumnTrie reports` through `(--ds-build incremental).`, with:

```markdown
  Emitted on ColumnTrie and HashTrie reports, by the bench family that ran
  the build rather than by the relation, since every build mode builds the
  same structure. ColumnTrie values: `bulk` (default) and `incremental`
  (`--ds-build column-trie=incremental`). HashTrie values: `serial` (default)
  and `radix:<bits>` (`--ds-build hash-trie=radix:<bits>`, bits in 1..=16).
```

and append a history row:

```markdown
| 3 (no bump) | 2026-10-05 | Every HashTrie report carries `ds_build_mode` (#91): `serial` (the only build before) or `radix:<bits>`. Every mode builds the identical trie, so no metric changes meaning and `schema_version` stays `3`; kermit-lab back-fills `serial` on earlier HashTrie rows. `--ds-build` now takes `structure=mode` pairs. |
```

- [ ] **Step 5: `USAGE.md`, `BENCHMARKING.md`, `ARCHITECTURE.md`, `benchmarking-architecture.md`**

- `USAGE.md` around line 198. Replace the paragraph and the example with:

  ````markdown
  `--ds-build` chooses how each structure is built from its tuples, as
  `structure=mode` pairs: `column-trie=bulk|incremental` (default `bulk`) and
  `hash-trie=serial|radix:<bits>` (default `serial`; bits in 1..=16). Every mode
  builds the identical structure, so the flag changes the `insertion` and
  `end-to-end` timings only; the report records it as `ds_build_mode`. A pair is
  valid only when `-i` selects its structure (or `all`). `bench run` and
  `bench join` accept it too.

  ```sh
  kermit bench ds -r data.csv -i column-trie -m insertion --ds-build column-trie=incremental
  kermit bench ds -r data.csv -i hash-trie -m insertion --ds-build hash-trie=radix:8
  ```
  ````

- `BENCHMARKING.md:96`: change "ColumnTrie's `--ds-build` mode" to "each structure's `--ds-build` mode".
- `BENCHMARKING.md:213-216`:
  - change "`--ds-build` works the same way for ColumnTrie's build (`bulk` by default, `incremental` for the build before issue #84)" to "`--ds-build` works the same way for the build modes: ColumnTrie's (`column-trie=bulk` by default, `column-trie=incremental` for the build before issue #84) and HashTrie's (`hash-trie=serial` by default, `hash-trie=radix:<bits>` for the radix-partitioned build, #91)";
  - change "`--ds-build incremental` against the default" to "e.g. `--ds-build column-trie=incremental` against the default";
  - after "kermit-lab back-fills `incremental` on pre-#84 ColumnTrie reports" add ", and `serial` on pre-#91 HashTrie reports".
- `BENCHMARKING.md:264`: `--ds-build incremental` becomes `--ds-build column-trie=incremental`.
- `ARCHITECTURE.md:88`: `` `Execution::HashHtj { hasher, pruning, config }` `` becomes `` `Execution::HashHtj { hasher, pruning, config, build }` `` (`build` carries `--ds-build hash-trie=…`).
- `docs/specs/benchmarking-architecture.md:64`: no change needed, because it names the flag only. Leave it.

- [ ] **Step 6: `CLAUDE.md`**

- Line 40. Replace it with two lines:
  ```
  cargo run -- bench run triangle -i column-trie -a leapfrog-triejoin --ds-build column-trie=incremental  # BuildMode axis (the pre-#84 build)
  cargo run -- bench run triangle -i hash-trie -a hash-triejoin --ds-build hash-trie=radix:8  # BuildMode axis (radix-partitioned build, #91)
  ```
- Line 134 (`BuildModeRelation`). Replace "implemented only by `ColumnTrie` (`ColumnTrieBuildMode`: `bulk` default, `incremental` = the pre-#84 sort-then-insert build)" with "implemented by `ColumnTrie` (`ColumnTrieBuildMode`: `bulk` default, `incremental` = the pre-#84 sort-then-insert build) and `HashTrie` (`HashTrieBuildMode`: `serial` default, `radix:<bits>` = radix-partition on the first attribute's hash, scratch roots merged in first-appearance order, #91; `from_tuples_with_config_and_build_mode` takes the load factor too)".
- Line 220. Change `` `--ds-build <mode>` `` to `` `--ds-build <structure>=<mode>,...` ``. After "A new flag needs a `DsFlag` variant, …" add the sentence: "`--ds-build` is keyed per structure (`DsFlag::Build(IndexStructure)`); a new BuildMode consumer adds its key to `BuildChoices::STRUCTURES` and a field to `BuildModes`."
- Line 280. Change `` `HashHtj { hasher, pruning, config }` `` to `` `HashHtj { hasher, pruning, config, build }` ``.

- [ ] **Step 7: Verify that no stale bare spelling remains**

Run: `grep -rn -- "--ds-build incremental\|--ds-build bulk\|--ds-build <mode>" --include='*.md' --include='*.rs' --include='*.py' . | grep -v "target/\|docs/superpowers/\|docs/specs/2026-\|docs/audits/"`
Expected: no matches, except the integration test that deliberately passes the bare form (`cli_bench_ds_rejects_the_bare_spelling_with_a_hint`) and the spec's own decision table. Fix anything else.

Run: `RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc --workspace --no-deps`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add docs/data-structures/hash-trie.md docs/data-structures/column-trie.md docs/specs/optimization-standard.md docs/specs/bench-report-schema.md USAGE.md BENCHMARKING.md ARCHITECTURE.md CLAUDE.md
git commit -m "docs: HashTrie's radix build mode and the keyed --ds-build (#91)" -m "<attribution lines>"
```

---

### Task 10: The full gate

- [ ] **Step 1: Format**

Run: `nix develop --command cargo fmt --all -- --check`
Expected: clean. If it is not, run `nix develop --command cargo fmt --all` and commit as `style: cargo fmt (#91)`.

- [ ] **Step 2: Tests, clippy, docs**

Run: `CARGO_BUILD_JOBS=2 cargo test --workspace`, which must PASS. `e2e_watdiv` can flake under the parallel run; if it fails, re-run it alone before blaming this branch.
Run: `RUSTFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo clippy --all-targets`, which must be clean.
Run: `RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc --workspace --no-deps`, which must be clean.

- [ ] **Step 3: Miri**

Confirm the Task 4 log ends `EXIT=0`. Then run the CI set detached:
```bash
setsid nohup bash -c 'MIRIFLAGS="-Zmiri-disable-isolation" CARGO_BUILD_JOBS=2 cargo miri test --workspace --exclude kermit --exclude kermit-bench > /tmp/claude-1000/miri-91.log 2>&1; echo EXIT=$? >> /tmp/claude-1000/miri-91.log' > /dev/null 2>&1 &
```
Expected: `EXIT=0`, after about 5–6 minutes.

- [ ] **Step 4: kermit-lab with the real binary**

Run: `CARGO_BUILD_JOBS=2 cargo build -p kermit && cd python/kermit-lab && KERMIT_BIN=$(git rev-parse --show-toplevel)/target/debug/kermit uv run pytest -q; cd -`
Expected: PASS, with the contract tests run, not skipped.

- [ ] **Step 5: Report the gate's results to the user before Phase 4.** Do not push or merge. Landing is the user's call.

---

## Phase 4 — Measurement (issue criterion 4)

### Task 11: `insertion` A/B, serial against radix

Run this only after telling the user and confirming the host is quiet: no other cargo builds or benchmarks, and peer loom sessions paused.

- [ ] **Step 1: Build a release binary and copy it out of `target/`**

Run:
```bash
CARGO_BUILD_JOBS=2 cargo build --release -p kermit
mkdir -p /tmp/claude-1000/ab-91 && cp target/release/kermit /tmp/claude-1000/ab-91/kermit-$(git rev-parse --short HEAD)
```

Copying out means an edit in the worktree cannot trigger a rebuild mid-run.

- [ ] **Step 2: Choose the two relations**

The first is `friendof` from the WatDiv cache. The #84 measurement used 4,491,142 tuples; confirm the row count of the candidate.
```bash
ls -la ~/.cache/kermit/benchmarks/watdiv-stress-100-test-1/friendof.parquet
uv run --with pyarrow python - <<'EOF'
import pyarrow.parquet as pq, glob, os
best = None
for path in sorted(glob.glob(os.path.expanduser("~/.cache/kermit/benchmarks/watdiv-stress-100-test-1/*.parquet"))):
    t = pq.read_table(path)
    if t.num_columns < 2 or t.num_rows == 0:
        continue
    n = t.num_rows
    d = len(set(t.column(0).to_pylist()))
    print(f"{os.path.basename(path)}: n={n} d={d} d/n={d/n:.3f}")
    if d / n >= 0.5 and (best is None or n > best[1]):
        best = (path, n, d)
print("D~n pick:", best)
EOF
```

Record `friendof`'s n, D and D/n, and the D≈n pick (the largest relation with D/n ≥ 0.5). Reading the cache is fine; nothing writes to it.

- [ ] **Step 3: Run the arms, with alternating order and ≥5 replicates**

Run five replicates per relation. Odd replicates run the arms forwards and even ones backwards, so drift cannot favour one arm. Set `B` to the binary copied in Step 1 and `RELS` to the two paths chosen in Step 2:
```bash
B=/tmp/claude-1000/ab-91/kermit-<sha>
RELS="$HOME/.cache/kermit/benchmarks/watdiv-stress-100-test-1/friendof.parquet <D≈n pick from Step 2>"
OUT=/tmp/claude-1000/ab-91/reports; mkdir -p $OUT
for R in $RELS; do
  rel=$(basename "$R" .parquet)
  for i in 1 2 3 4 5; do
    arms="serial radix:4 radix:8 radix:12"
    [ $((i % 2)) -eq 0 ] && arms="radix:12 radix:8 radix:4 serial"
    for arm in $arms; do
      tag=$(echo "$arm" | tr ':' '-')
      "$B" bench --name "ab91-$rel-$tag-r$i" --report-json "$OUT/$rel-$tag-r$i.json" \
        ds -r "$R" -i hash-trie -m insertion space --ds-build "hash-trie=$arm"
    done
  done
done
```

Sample `ps -eo pid,pcpu,comm --sort=-pcpu | head` every 30 s in a second shell throughout. Note any contention, and re-run any replicate that overlaps it.

- [ ] **Step 4: Summarise**

Use kermit-lab:
```bash
cd python/kermit-lab && uv run python - <<'EOF'
import kermit_lab as kl, glob
df = kl.load(sorted(glob.glob("/tmp/claude-1000/ab-91/reports/*.json")))
ins = df[df["metric"] == "insertion"]
print(ins.groupby(["relation_path", "ds_build_mode"])["time"].describe()[["count", "50%", "min", "max"]])
sp = df[df["metric"].str.startswith("space")]
print(sp.groupby(["relation_path", "ds_build_mode"])["value" if "value" in sp else "time"].unique())
EOF
cd -
```

Adjust the column names to what `kl.load` returns; `kl.discover_opt_columns(df)` lists them. Space must be identical across arms. If it is not, stop: identity is broken.

- [ ] **Step 5: Record**

In `hash-trie.md`, add a `#### Radix build A/B` subsection under § Build modes, and point the "Measured effect" bullet at it. Give, per relation:
- n, D and D/n;
- the median and min–max of `insertion` per arm, and each radix arm's median ratio to `serial`;
- the binary SHA, the date, and the host-quietness note.

Then draft the issue comment for #91 in the final report: the same table, plus one line of interpretation. **Do not post it.**

- [ ] **Step 6: Commit and hand over**

```bash
git add docs/data-structures/hash-trie.md
git commit -m "docs(hash-trie): radix build insertion A/B against serial (#91)" -m "<attribution lines>"
```

Report to the user:
- the table;
- the drafted issue comment;
- the decision they now own: keep `radix:K` (a null or negative result is still an ablation data point) or drop it.

---

## Spec coverage check (done while writing)

| Spec requirement | Task |
|---|---|
| `HashTrieBuildMode`, `RadixBits` 1..=16, `axis_value`/`FromStr`, errors naming the accepted forms | 1 |
| Combined constructor; `Serial` delegates to the unchanged serial build; `BuildModeRelation` with the default config | 3 |
| Stable histogram + scatter partition, `u16` ids, exact-size partitions | 3 (`partition`) |
| Scratch roots via the unchanged `insert_at`; arrivals by `len()` growth | 3 (`build_scratch_root`) |
| Positions read before consuming, consuming accessor, merge sorted by first index | 2 (`into_buckets`), 3 |
| Identity test over the full matrix + mutant checks | 3 |
| #66 probe-count guard | 2 |
| `BuiltWith` `HashTrieIterable` forward + DS aliases | 4 |
| `Execution::HashHtj { build }`, families, axis on every HashTrie report, dispatch | 5 |
| Counting-hash spy over every build route | 5 |
| Keyed `--ds-build`, error table, `DsFlag::Build(IndexStructure)`, #86 reach | 6 |
| Join suites, Radix2 on Sip/off and Fx/on, both optimisers | 7 |
| kermit-lab `serial` back-fill, contract test keyed | 6, 8 |
| Docs list | 9 |
| Gate | 10 |
| A/B (friendof + a D≈n relation, ≥5 alternating replicates, space equal, draft comment, user decides) | 11 |
