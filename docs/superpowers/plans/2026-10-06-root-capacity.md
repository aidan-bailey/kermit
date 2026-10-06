# Root Capacity Config (#88) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `--ds-config root-capacity=grow|tuples` to `HashTrie`. Under `tuples`, a build from n tuples sizes the root once, so that it never grows. Under `grow`, the default, nothing changes.

**Architecture:** A `RootCapacity` enum becomes the second field of `HashTrieConfig`.

- `hash_table::log2_capacity_for(keys, lf)` is the pure sizing function.
- `HashTable::with_log2_capacity(p)` is the sized constructor.
- `HashTrie::with_config_for(header, config, tuple_count)` is the one helper every root-creating path calls.

Child tables, lazy expansion and the radix build's scratch roots are untouched. The spec is `docs/specs/2026-10-06-capacity-hint-design.md`. The user approved it, and the `hash-trie-parallel` session confirmed its Decision section.

**Tech Stack:** Rust nightly workspace (`kermit-ds`, `kermit` binary), clap, serde_json; Python `kermit-lab` (pandas, pytest via uv).

---

## Ground rules (read before Task 1)

- **Shell.** Run every cargo command inside `nix develop`, in the foreground, with `CARGO_BUILD_JOBS=2`. A background memory monitor kills large parallel builds:
  `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie::hash_table`.
  Before any multi-minute run (`cargo test` on the whole workspace, Miri), check `pgrep -a kermit`. If a `kermit bench` process is timing, wait until it finishes.
- **Format** only with `nix develop --command cargo fmt --all`. Stable rustfmt rewrites dozens of files.
- **Commits.** Use conventional commits ending `(#88)`, followed by the two trailer lines:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01JRkiMxoczrRC2wDKRphkU2
  ```
  Never amend. Never push. Stage files by name, never `git add -A`, because new files are marked intent-to-add here.
- **Scope (CLAUDE.md Priority 6).** Touch HashTrie only. Under `grow`, every HashTrie must build exactly as it does today.
- **Doc comments** are linted by clippy `doc_markdown`. Backtick every code identifier in `///` and `//!` text.
- `kermit/src/{options,execution}.rs` are binary files, which `cargo doc` never lints. Lint them with
  `nix develop --command cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings`.
  That command has 2 known pre-existing errors; don't add a third.

## File structure

| File | Responsibility | Change |
|---|---|---|
| `kermit-ds/src/ds/hash_trie/identity.rs` | Test-only array-level trie comparison and shared inputs | **Create** (moved out of `radix.rs` tests), plus `configs()` |
| `kermit-ds/src/ds/hash_trie/hash_table.rs` | Open-addressing table | `INITIAL_LOG2_CAPACITY`, `log2_capacity_for`, `with_log2_capacity` |
| `kermit-ds/src/ds/hash_trie/node.rs` | Node enum | `new_table_sized` |
| `kermit-ds/src/ds/hash_trie/config.rs` | Config values | `RootCapacity`, `ParseRootCapacityError`, `root_capacity` field, `root_log2_capacity`, axis |
| `kermit-ds/src/ds/hash_trie/implementation.rs` | `HashTrie` | `make_root_sized`, `with_config_for`, wire three constructors, `root_capacity` test module |
| `kermit-ds/src/ds/hash_trie/radix.rs` | `radix:K` build | Tests import from `identity`; `check_identity` loops over `configs()` |
| `kermit-ds/src/ds/hash_trie/mod.rs`, `kermit-ds/src/ds/mod.rs`, `kermit-ds/src/lib.rs` | Re-exports | `RootCapacity`, `ParseRootCapacityError`; `#[cfg(test)] mod identity;` |
| `kermit/src/options.rs` | CLI `--ds-config` | `root-capacity` key |
| `kermit/src/execution.rs` | Bench families | Family-level presize test |
| `kermit/tests/cli_hash_trie_config_choice.rs` | CLI smoke | 4 tests |
| `kermit/tests/join_tests.rs`, `kermit-ds/tests/{hash_trie_tests,parquet_tests}.rs` | Suites | `PresizedRoot` provider and aliases |
| `python/kermit-lab/kermit_lab/defaults.py`, `python/kermit-lab/tests/test_defaults.py` | Back-fill | `"grow"` default |
| `docs/data-structures/hash-trie.md`, `docs/specs/optimization-standard.md`, `docs/specs/bench-report-schema.md`, `CLAUDE.md` | Docs | Document the value |

Every `HashTrieConfig { load_factor: … }` struct literal in code breaks in Task 3, because the struct gains a field. Task 3 Step 6 fixes all of them at once.

---

### Task 1: Move the radix identity checks into a shared test module

This is a pure move, with no behaviour change. It mirrors the `hash-trie-parallel` branch's `identity.rs`, so the eventual merge stays small. Tasks 4 and 5 need these helpers outside `radix.rs`.

**Files:**
- Create: `kermit-ds/src/ds/hash_trie/identity.rs`
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs` (module list)
- Modify: `kermit-ds/src/ds/hash_trie/radix.rs` (the `#[cfg(test)] mod tests` block, from its `use` through `fn inputs`)

- [ ] **Step 1: Create `identity.rs`** with the helpers cut from `radix.rs`'s test module. The bodies are unchanged; only visibility changes, to `pub(super)`.

```rust
//! Array-level identity checks for the HashTrie builds (#91, #88). Every
//! build mode must build the trie the serial build does, bucket for bucket
//! and capacity for capacity (the BuildMode rule of
//! `docs/specs/optimization-standard.md`), and the root-capacity Config must
//! leave every subtrie as it was, so the radix build's tests and the
//! root-capacity tests share these assertions and inputs.

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

// `&Vec`, not a slice: the comparison reads `capacity()`.
#[allow(clippy::ptr_arg)]
fn assert_same_chain(a: &Vec<Vec<usize>>, b: &Vec<Vec<usize>>, path: &str) {
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

- [ ] **Step 2: Register the module** in `kermit-ds/src/ds/hash_trie/mod.rs`, between `mod hash_trie_iter;` and `mod implementation;`:

```rust
mod hash_trie_iter;
#[cfg(test)]
mod identity;
mod implementation;
```

- [ ] **Step 3: Cut the moved items out of `radix.rs`'s test module.** Delete `assert_same_table`, `assert_same_chain`, `assert_same_node`, `assert_same_trie`, `rows`, `RANDOM_TUPLES`, `LOAD_PERCENTS` and `inputs`, with their doc comments. Keep `BITS`, `check_identity` and the `#[test]` fns. Replace the module's `use` block with:

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

(`HeapSize`, `Cardinality`, `PendingChild`, `SingletonPayload` and `Lcg` were used only by the moved helpers.)

- [ ] **Step 4: Run the radix tests**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie::radix`
Expected: PASS, the same 5 tests as before. Also run `nix develop --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit-ds --all-targets` and expect no warnings: an unused import would be one.

- [ ] **Step 5: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/identity.rs kermit-ds/src/ds/hash_trie/mod.rs kermit-ds/src/ds/hash_trie/radix.rs
git commit -m "refactor(hash-trie): share the radix build's identity checks (#88)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JRkiMxoczrRC2wDKRphkU2"
```

---

### Task 2: Size a table for a known key count

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/hash_table.rs` (the `HashTable` struct doc, `HashTable::new`, new items above `impl<V> HashTable<V>`, and the test module)

- [ ] **Step 1: Write the failing tests.** Append them to `hash_table.rs`'s `mod tests`:

```rust
    #[test]
    fn with_log2_capacity_builds_that_many_empty_buckets() {
        for p in 1..=10 {
            let t: HashTable<u32> = HashTable::with_log2_capacity(p);
            assert_eq!(t.buckets_len(), 1 << p);
            assert_eq!(t.len(), 0);
            assert_eq!(t.log2_capacity, p);
            assert_eq!(
                t.shell_heap_bytes(),
                (1 << p) * std::mem::size_of::<Option<Entry<u32>>>()
            );
        }
        let new: HashTable<u32> = HashTable::new();
        assert_eq!(new.log2_capacity, INITIAL_LOG2_CAPACITY);
        assert_eq!(
            new.shell_heap_bytes(),
            HashTable::<u32>::with_log2_capacity(INITIAL_LOG2_CAPACITY).shell_heap_bytes()
        );
    }

    #[test]
    #[should_panic(expected = "log2 capacity must be in 1..64")]
    fn with_log2_capacity_rejects_zero() {
        let _: HashTable<u32> = HashTable::with_log2_capacity(0);
    }

    #[test]
    #[should_panic(expected = "log2 capacity must be in 1..64")]
    fn with_log2_capacity_rejects_sixty_four() {
        let _: HashTable<u32> = HashTable::with_log2_capacity(64);
    }

    /// `log2_capacity_for(keys, lf)` is the smallest log2 capacity of at
    /// least 2 whose table holds `keys` keys under the cap: sufficient, and
    /// one less would not be.
    #[test]
    fn log2_capacity_for_is_the_smallest_sufficient_capacity() {
        let most_keys = if cfg!(miri) { 64 } else { 2_000 };
        for percent in [1u8, 10, 25, 50, 70, 80, 95, 99] {
            let lf = LoadFactor::percent(percent).unwrap();
            for keys in 0..most_keys {
                let p = log2_capacity_for(keys, lf);
                let holds = |p: u32| keys * 100 <= (1usize << p) * usize::from(percent);
                assert!(p >= INITIAL_LOG2_CAPACITY, "{percent}%, {keys} keys: below 4 buckets");
                assert!(holds(p), "{percent}%, {keys} keys: 2^{p} is too small");
                assert!(
                    p == INITIAL_LOG2_CAPACITY || !holds(p - 1),
                    "{percent}%, {keys} keys: 2^{p} is not the smallest"
                );
            }
        }
    }

    /// At an 80 % cap the capacity is the paper's `2^⌈log2(1.25·|L|)⌉`
    /// (Algorithm 2, line 3), apart from the 4-bucket floor.
    #[test]
    fn log2_capacity_for_at_eighty_percent_is_the_papers_sizing() {
        let lf = LoadFactor::percent(80).unwrap();
        for keys in 2..2_000usize {
            // ⌈log2(1.25·keys)⌉ = ⌈log2(⌈5·keys/4⌉)⌉, since 2^p is a whole
            // number; for keys ≥ 2 it is at least 2, so the floor is moot.
            let paper = (5 * keys).div_ceil(4).next_power_of_two().trailing_zeros();
            assert_eq!(log2_capacity_for(keys, lf), paper, "{keys} keys");
        }
        // The paper allocates 2 buckets for one tuple; kermit never goes
        // below 4.
        assert_eq!(log2_capacity_for(1, lf), INITIAL_LOG2_CAPACITY);
    }

    #[test]
    #[should_panic(expected = "capacity overflow")]
    fn log2_capacity_for_rejects_an_unallocatable_capacity() {
        log2_capacity_for(usize::MAX, LoadFactor::percent(1).unwrap());
    }

    /// A table built at `log2_capacity_for(keys, lf)` takes `keys` distinct
    /// inserts without growing, at every load factor: the guarantee the
    /// root-capacity Config (#88) rests on.
    #[test]
    fn a_presized_table_never_grows_for_its_keys() {
        let most_keys = if cfg!(miri) { 100 } else { 1_000 };
        for percent in [1u8, 10, 25, 50, 70, 95, 99] {
            let lf = LoadFactor::percent(percent).unwrap();
            for keys in [0, 1, 2, 3, 7, most_keys] {
                let p = log2_capacity_for(keys, lf);
                let mut t: HashTable<usize> = HashTable::with_log2_capacity(p);
                // A full-period LCG, so every hash is distinct.
                let mut hash: u64 = 1;
                for k in 0..keys {
                    t.entry_or_insert_with(hash, lf, || k);
                    assert_eq!(
                        t.buckets_len(),
                        1 << p,
                        "{percent}%: grew on key {k} of {keys}"
                    );
                    hash = hash.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
                }
                assert_eq!(t.len(), keys);
            }
        }
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie::hash_table`
Expected: a compile error, because `with_log2_capacity`, `INITIAL_LOG2_CAPACITY` and `log2_capacity_for` are not defined.

- [ ] **Step 3: Implement.** Insert this above the `HashTable` struct doc:

```rust
/// The log2 capacity a table starts at unless it is built at another: 4
/// buckets.
pub(crate) const INITIAL_LOG2_CAPACITY: u32 = 2;

/// The smallest log2 capacity, at least [`INITIAL_LOG2_CAPACITY`], at which
/// a table holds `keys` keys without its load-factor cap firing: the
/// smallest `p ≥ 2` with `keys · den ≤ 2^p · num`.
///
/// [`HashTable::entry_or_insert_with`] grows when
/// `(len + 1) · den > capacity · num`, and `len + 1 ≤ keys` for every insert
/// up to the `keys`-th distinct hash. So a table built at this capacity
/// never grows while it receives at most `keys` distinct hashes. At an 80 %
/// cap this is the paper's `⌈log2(1.25·keys)⌉` (Algorithm 2, line 3), except
/// that it never goes below 4 buckets.
///
/// # Panics
///
/// If the table would need `2^64` buckets or more, which no `Vec` can hold.
pub(crate) fn log2_capacity_for(keys: usize, load_factor: LoadFactor) -> u32 {
    // `u128`, so `keys · den` cannot overflow for any `usize`.
    let num = load_factor.numerator() as u128;
    let den = load_factor.denominator() as u128;
    let buckets_needed = (keys as u128 * den).div_ceil(num);
    let p = buckets_needed
        .next_power_of_two()
        .trailing_zeros()
        .max(INITIAL_LOG2_CAPACITY);
    assert!(
        p < 64,
        "capacity overflow: {keys} keys at a {}% load factor need 2^{p} buckets",
        load_factor.numerator()
    );
    p
}
```

Change the struct doc's second sentence. The current text reads:

```
/// Capacity is always `2^log2_capacity`. The initial capacity is 4
/// (`log2_capacity = 2`). The table grows by doubling when occupancy
```

Change it to:

```
/// Capacity is always `2^log2_capacity`. A table starts at 4 buckets
/// (`log2_capacity = 2`) unless built at another capacity with
/// [`with_log2_capacity`](Self::with_log2_capacity). The table grows by
/// doubling when occupancy
```

Then replace `new` with:

```rust
    /// An empty table of 4 buckets.
    pub fn new() -> Self { Self::with_log2_capacity(INITIAL_LOG2_CAPACITY) }

    /// An empty table of `2^log2_capacity` buckets.
    ///
    /// # Panics
    ///
    /// Unless `1 ≤ log2_capacity < 64`: [`bucket_index`](Self::bucket_index)
    /// shifts by `64 - log2_capacity`, and `MULTIPLIERS` covers exponents
    /// below 64.
    pub fn with_log2_capacity(log2_capacity: u32) -> Self {
        assert!(
            (1..64).contains(&log2_capacity),
            "log2 capacity must be in 1..64, got {log2_capacity}"
        );
        let capacity = 1usize << log2_capacity;
        Self {
            log2_capacity,
            len: 0,
            buckets: (0..capacity).map(|_| None).collect(),
        }
    }
```

(`collect` from an exact-size range allocates exactly `capacity` slots, as `new` did, so `shell_heap_bytes` is unchanged at the default.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie::hash_table`
Expected: PASS, including the pre-existing `new_starts_empty_with_4_buckets` and `resizes_exactly_at_the_configured_cap`.

- [ ] **Step 5: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/hash_table.rs
git commit -m "feat(hash-trie): size a table for a known key count (#88)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JRkiMxoczrRC2wDKRphkU2"
```

---

### Task 3: The `root_capacity` Config value

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/config.rs`
- Modify: `kermit-ds/src/ds/hash_trie/node.rs` (after `new_table`)
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs`, `kermit-ds/src/ds/mod.rs:14-18`, `kermit-ds/src/lib.rs:41-46` (re-exports)
- Modify: every `HashTrieConfig { load_factor: … }` literal in code (Step 6)
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs` (the two `optimization_axes_*_reports_*` key lists)

- [ ] **Step 1: Write the failing tests.** Append them to `config.rs`'s `mod tests`:

```rust
    #[test]
    fn default_root_capacity_is_grow() {
        assert_eq!(HashTrieConfig::default().root_capacity, RootCapacity::Grow);
        assert_eq!(RootCapacity::default(), RootCapacity::Grow);
    }

    /// What a report's `ds_config_root_capacity` says is what
    /// `--ds-config root-capacity=…` parses back.
    #[test]
    fn root_capacity_axis_values_round_trip() {
        assert_eq!(RootCapacity::Grow.axis_value(), "grow");
        assert_eq!(RootCapacity::Tuples.axis_value(), "tuples");
        for value in [RootCapacity::Grow, RootCapacity::Tuples] {
            assert_eq!(value.axis_value().parse::<RootCapacity>(), Ok(value));
        }
    }

    #[test]
    fn malformed_root_capacities_name_both_values() {
        for bad in ["", "Grow", "tuple", "1024", "grow,tuples"] {
            let msg = bad.parse::<RootCapacity>().unwrap_err().to_string();
            assert!(msg.contains("expected grow or tuples"), "{bad:?}: {msg}");
            assert!(msg.contains(&format!("{bad:?}")), "{bad:?}: {msg}");
        }
    }

    #[test]
    fn axes_report_root_capacity_as_a_string() {
        let tuples = HashTrieConfig {
            root_capacity: RootCapacity::Tuples,
            ..HashTrieConfig::default()
        };
        assert!(tuples
            .axes()
            .contains(&("root_capacity", Value::from("tuples"))));
        assert!(HashTrieConfig::default()
            .axes()
            .contains(&("root_capacity", Value::from("grow"))));
    }

    #[test]
    fn root_log2_capacity_is_four_buckets_under_grow_and_sized_under_tuples() {
        let grow = HashTrieConfig::default();
        let tuples = HashTrieConfig {
            root_capacity: RootCapacity::Tuples,
            ..grow
        };
        for n in [0, 1, 3, 1_000, 1 << 20] {
            assert_eq!(grow.root_log2_capacity(n), INITIAL_LOG2_CAPACITY);
            assert_eq!(
                tuples.root_log2_capacity(n),
                log2_capacity_for(n, grow.load_factor)
            );
        }
        // 1,000 keys at 70 % need ⌈1000 / 0.7⌉ = 1,429 buckets: 2^11.
        assert_eq!(tuples.root_log2_capacity(1_000), 11);
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie::config`
Expected: a compile error, because `RootCapacity`, `root_capacity` and `root_log2_capacity` are not defined.

- [ ] **Step 3: Implement in `config.rs`.** Change the `use` line to:

```rust
use {
    super::hash_table::{log2_capacity_for, INITIAL_LOG2_CAPACITY},
    kermit_iters::ConfigOption,
    serde_json::Value,
    std::{fmt, str::FromStr},
};
```

Add this after `impl Default for LoadFactor`:

```rust
/// How large a build makes the root table (#88).
///
/// A value, not a shape: it replaces the root's starting capacity, a
/// constant on the path every build already takes, and is read once per
/// trie. Child tables are unaffected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RootCapacity {
    /// Start at 4 buckets and double as keys arrive: the only behaviour
    /// before #88.
    #[default]
    Grow,
    /// Size the root once, from the number of tuples the constructor is
    /// given, so that it never grows during that build: Algorithm 2,
    /// line 3 of the paper, applied to the root. A trie created empty is
    /// sized for no tuples, which is 4 buckets.
    Tuples,
}

impl RootCapacity {
    /// The value the bench axis reports and `--ds-config root-capacity=`
    /// parses.
    pub fn axis_value(self) -> &'static str {
        match self {
            | Self::Grow => "grow",
            | Self::Tuples => "tuples",
        }
    }
}

/// A string that names no [`RootCapacity`]. Its message names both values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseRootCapacityError(String);

impl fmt::Display for ParseRootCapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expected grow or tuples, got {:?}", self.0)
    }
}

impl std::error::Error for ParseRootCapacityError {}

/// Parses the strings [`RootCapacity::axis_value`] returns.
impl FromStr for RootCapacity {
    type Err = ParseRootCapacityError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            | "grow" => Ok(Self::Grow),
            | "tuples" => Ok(Self::Tuples),
            | other => Err(ParseRootCapacityError(other.to_owned())),
        }
    }
}
```

Replace the `HashTrieConfig` struct and its `ConfigOption` impl with:

```rust
/// Runtime values read by `HashTrie` while it is built.
///
/// A Config is a *value* on a path the code already takes (the resize
/// comparison runs on every insert; the root's capacity is set on every
/// build); replacing a constant with it adds no branch, so non-users pay
/// nothing. See the shape/value/process rule in
/// `docs/specs/optimization-standard.md`.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashTrieConfig {
    /// Occupancy cap before a level's table doubles. Bench axis
    /// `ds_config_load_factor` (e.g. `0.7`).
    pub load_factor: LoadFactor,
    /// How large a build makes the root. Bench axis
    /// `ds_config_root_capacity` (`"grow"` or `"tuples"`).
    pub root_capacity: RootCapacity,
}

impl HashTrieConfig {
    /// The root's log2 capacity for a build from `tuple_count` tuples: 4
    /// buckets under [`RootCapacity::Grow`], and under
    /// [`RootCapacity::Tuples`] the smallest capacity at which
    /// `tuple_count` keys never make the root grow
    /// (`hash_table::log2_capacity_for`). Distinct keys cannot outnumber
    /// tuples, so a root built at this capacity never grows during the
    /// build. This is a pure function of the tuple count and the load
    /// factor, so a partitioned build can compute it before partitioning.
    pub(crate) fn root_log2_capacity(self, tuple_count: usize) -> u32 {
        match self.root_capacity {
            | RootCapacity::Grow => INITIAL_LOG2_CAPACITY,
            | RootCapacity::Tuples => log2_capacity_for(tuple_count, self.load_factor),
        }
    }
}

impl ConfigOption for HashTrieConfig {
    fn axes(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("load_factor", Value::from(self.load_factor.as_f64())),
            ("root_capacity", Value::from(self.root_capacity.axis_value())),
        ]
    }
}
```

In `node.rs`, add this after `new_table`:

```rust
    /// [`new_table`](Self::new_table) at `2^log2_capacity` buckets instead
    /// of 4. Used for the root, which a build may size from its tuple count
    /// (`HashTrieConfig::root_log2_capacity`).
    pub(crate) fn new_table_sized(is_leaf: bool, log2_capacity: u32) -> Self {
        if is_leaf {
            HashTrieNode::Leaf(HashTable::with_log2_capacity(log2_capacity))
        } else {
            HashTrieNode::Inner(HashTable::with_log2_capacity(log2_capacity))
        }
    }
```

- [ ] **Step 4: Re-export.** In `kermit-ds/src/ds/hash_trie/mod.rs`:

```rust
    config::{
        HashTrieConfig, InvalidLoadFactor, LoadFactor, ParseRootCapacityError, RootCapacity,
    },
```

In `kermit-ds/src/ds/mod.rs`, add `ParseRootCapacityError` and `RootCapacity` to the `hash_trie::{…}` list. In `kermit-ds/src/lib.rs`, add them to the `ds::{…}` list. Keep both lists alphabetical, as rustfmt will reorder them anyway.

- [ ] **Step 5: Update the two exact-key-list tests** in `implementation.rs` (`optimization_axes_default_strategy_reports_sip` and `optimization_axes_fxhash_strategy_reports_fxhash`). Each expected vec becomes:

```rust
        assert_eq!(keys, vec![
            "ds_config_load_factor",
            "ds_config_root_capacity",
            "ds_layout_expansion",
            "ds_layout_hasher",
            "ds_layout_pruning"
        ]);
```

- [ ] **Step 6: Give every config literal the new field's default.** Each multi-line `HashTrieConfig { load_factor: X, }` literal in code gains `..HashTrieConfig::default(),` after its `load_factor` line. Run:

```bash
perl -0pi -e 's/(HashTrieConfig \{\n(\s*)load_factor: [^\n]*,\n)(\s*\})/$1$2..HashTrieConfig::default(),\n$3/g' \
  kermit-ds/src/configured.rs kermit-ds/src/ds/hash_trie/config.rs \
  kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/radix.rs \
  kermit-ds/tests/hash_trie_tests.rs kermit-ds/tests/parquet_tests.rs \
  kermit/src/execution.rs kermit/src/db/database.rs kermit/tests/join_tests.rs
grep -rn -A2 "HashTrieConfig {" kermit-ds/src kermit-ds/tests kermit/src kermit/tests | grep -v "^--$"
```

Inspect the grep output. Every literal must now end with `..HashTrieConfig::default(),` or set `root_capacity` explicitly. One site is a single line: the doc comment in `kermit/tests/common/macros.rs:496`. Edit it by hand to:

```rust
/// define_config_provider!(HalfFull, HashTrieConfig, HashTrieConfig { load_factor: LoadFactor::percent(50).unwrap(), ..HashTrieConfig::default() });
```

In `config.rs`'s own test `axes_report_load_factor_as_a_string`, the literal sits inside `mod tests`, where `HashTrieConfig` is in scope, so the rewrite compiles.

- [ ] **Step 7: Run the crate tests and check that the workspace compiles**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie`
Expected: PASS, including the 5 new config tests and the two updated key lists.
Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo build --all-targets`
Expected: builds. A `missing field root_capacity` error means Step 6 missed a literal; fix it the same way.

- [ ] **Step 8: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/config.rs kermit-ds/src/ds/hash_trie/node.rs \
  kermit-ds/src/ds/hash_trie/mod.rs kermit-ds/src/ds/mod.rs kermit-ds/src/lib.rs \
  kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/radix.rs \
  kermit-ds/src/configured.rs kermit-ds/tests/hash_trie_tests.rs kermit-ds/tests/parquet_tests.rs \
  kermit/src/execution.rs kermit/src/db/database.rs kermit/tests/join_tests.rs kermit/tests/common/macros.rs
git commit -m "feat(hash-trie): the root-capacity Config value (#88)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JRkiMxoczrRC2wDKRphkU2"
```

---

### Task 4: Presize the root from the tuple count

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs`. That means `make_root` (around line 113), `ConfigurableRelation::with_config` and `from_tuples_with_config` (around lines 384–410), the `Radix` arm of `from_tuples_with_config_and_build_mode` (around line 450), and a new nested test module at the end of `mod tests`.

- [ ] **Step 1: Write the failing tests.** Append this nested module inside `implementation.rs`'s `mod tests`, before its closing brace:

```rust
    /// The root-capacity Config (#88). Under `tuples` a build sizes the root
    /// once, from its tuple count. Under `grow`, the default, nothing
    /// changes.
    mod root_capacity {
        use {
            crate::{
                cardinality::Cardinality,
                ds::hash_trie::{
                    config::{HashTrieConfig, LoadFactor, RootCapacity},
                    expansion::{EagerExpansion, ExpansionPolicy, LazyExpansion},
                    identity::{assert_same_node, assert_same_trie, inputs},
                    implementation::HashTrie,
                    node::HashTrieNode,
                    pruning::{NoPruning, PruningPolicy, SingletonPruning},
                },
                relation::{ConfigurableRelation, Relation},
                test_support::Mod10HashStrategy,
            },
            kermit_iters::{FxHashStrategy, HashStrategy, SipHashStrategy},
        };

        fn config(percent: u8, root_capacity: RootCapacity) -> HashTrieConfig {
            HashTrieConfig {
                load_factor: LoadFactor::percent(percent).unwrap(),
                root_capacity,
            }
        }

        /// Load factors under test, in percent.
        const PERCENTS: &[u8] = &[50, 70, 95];

        /// Distinct keys in the D = n inputs. Small under Miri.
        const DISTINCT: usize = if cfg!(miri) {
            40
        } else {
            1_000
        };

        /// The root's capacity, as a log2.
        fn root_log2<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
            trie: &HashTrie<H, P, E>,
        ) -> u32 {
            trie.root().buckets_len().trailing_zeros()
        }

        fn label<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(rest: &str) -> String {
            format!("{}/{}/{} {rest}", H::NAME, P::NAME, E::NAME)
        }

        /// Runs `$check` under every pruning × expansion Layout for each
        /// hasher listed.
        macro_rules! under_every_layout {
            ($check:ident, $($hasher:ty),+) => {$(
                $check::<$hasher, NoPruning, EagerExpansion>();
                $check::<$hasher, SingletonPruning, EagerExpansion>();
                $check::<$hasher, NoPruning, LazyExpansion>();
                $check::<$hasher, SingletonPruning, LazyExpansion>();
            )+};
        }

        /// At the default, `from_tuples` builds the trie that `with_config`
        /// plus one `insert` per tuple builds. That second path is the loop
        /// `from_tuples_with_config` ran before #88, and it never sees a tuple
        /// count, so the default build does not presize.
        fn check_default_identity<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
            for arity in 1..=3 {
                for (input, tuples) in inputs(arity) {
                    let built = HashTrie::<H, P, E>::from_tuples(arity.into(), tuples.clone());
                    let mut inserted =
                        HashTrie::<H, P, E>::with_config(arity.into(), HashTrieConfig::default());
                    inserted.insert_all(tuples);
                    assert_same_trie(
                        &built,
                        &inserted,
                        &label::<H, P, E>(&format!("arity {arity}, {input}")),
                    );
                }
            }
        }

        #[test]
        fn the_default_builds_the_trie_it_built_before() {
            under_every_layout!(
                check_default_identity,
                SipHashStrategy,
                FxHashStrategy,
                Mod10HashStrategy
            );
        }

        /// Under `tuples` the root ends at `root_log2_capacity(n)`. Where the
        /// keys are fewer than the tuples, growth from 4 would stop below
        /// that, so the root was presized. Where every key is distinct
        /// (D = n, the most keys n tuples can bring), `hash_table`'s
        /// `a_presized_table_never_grows_for_its_keys` shows that a table at
        /// that capacity takes them all without growing.
        fn check_root_is_presized<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
            for &percent in PERCENTS {
                let config = config(percent, RootCapacity::Tuples);
                for n in [0, 1, 2, 3, DISTINCT] {
                    let unary = (0..n).map(|k| vec![k]).collect();
                    let trie = HashTrie::<H, P, E>::from_tuples_with_config(1.into(), config, unary);
                    assert_eq!(
                        root_log2(&trie),
                        config.root_log2_capacity(n),
                        "{}",
                        label::<H, P, E>(&format!("{percent}%, {n} distinct unary keys"))
                    );
                }
                for arity in 2..=3 {
                    for (input, tuples) in inputs(arity) {
                        let n = tuples.len();
                        let trie =
                            HashTrie::<H, P, E>::from_tuples_with_config(arity.into(), config, tuples);
                        assert_eq!(
                            root_log2(&trie),
                            config.root_log2_capacity(n),
                            "{}",
                            label::<H, P, E>(&format!("{percent}%, arity {arity}, {input}"))
                        );
                    }
                }
            }
        }

        #[test]
        fn tuples_sizes_the_root_from_the_tuple_count() {
            under_every_layout!(
                check_root_is_presized,
                SipHashStrategy,
                FxHashStrategy,
                Mod10HashStrategy
            );
        }

        /// With every first value distinct (D = n) and a load factor of at
        /// least 25 %, `tuples` presizes the root to the capacity `grow`
        /// reaches. It changes when the root reaches its size, not the size.
        /// Below 25 % one doubling can leave a small table over its cap
        /// (#104), so the two can differ there. The colliding strategy is
        /// left out, because its distinct keys share hashes (D < n).
        fn check_distinct_keys_reach_the_same_capacity<
            H: HashStrategy,
            P: PruningPolicy,
            E: ExpansionPolicy,
        >() {
            let most = if cfg!(miri) { 40 } else { 300 };
            for percent in [25, 50, 70, 95] {
                for n in 0..=most {
                    let tuples: Vec<Vec<usize>> = (0..n).map(|k| vec![k, k % 7]).collect();
                    let build = |root_capacity| {
                        HashTrie::<H, P, E>::from_tuples_with_config(
                            2.into(),
                            config(percent, root_capacity),
                            tuples.clone(),
                        )
                    };
                    assert_eq!(
                        root_log2(&build(RootCapacity::Tuples)),
                        root_log2(&build(RootCapacity::Grow)),
                        "{}",
                        label::<H, P, E>(&format!("{percent}%, {n} distinct first values"))
                    );
                }
            }
        }

        #[test]
        fn distinct_keys_reach_the_capacity_grow_reaches() {
            under_every_layout!(
                check_distinct_keys_reach_the_same_capacity,
                SipHashStrategy,
                FxHashStrategy
            );
        }

        /// `tuples` sizes the root only. Under it, every root entry holds the
        /// subtrie (or chain) it holds under `grow`, array for array.
        fn check_children_unchanged<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
            for &percent in PERCENTS {
                for arity in 1..=3 {
                    for (input, tuples) in inputs(arity) {
                        let label =
                            label::<H, P, E>(&format!("{percent}%, arity {arity}, {input}"));
                        let build = |root_capacity| {
                            HashTrie::<H, P, E>::from_tuples_with_config(
                                arity.into(),
                                config(percent, root_capacity),
                                tuples.clone(),
                            )
                        };
                        let (grow, presized) = (build(RootCapacity::Grow), build(RootCapacity::Tuples));
                        let (g, t) = (grow.root(), presized.root());
                        assert_eq!(g.len(), t.len(), "{label}: root keys");
                        assert_eq!(grow.tuple_count(), presized.tuple_count(), "{label}: tuples");
                        for idx in 0..g.buckets_len() {
                            let Some(hash) = g.hash_at(idx) else {
                                continue;
                            };
                            let at = t
                                .index_of(hash)
                                .unwrap_or_else(|| panic!("{label}: root key {hash:#x} missing"));
                            match (g, t) {
                                | (HashTrieNode::Inner(a), HashTrieNode::Inner(b)) => {
                                    assert_same_node(
                                        a.value_at(idx).unwrap(),
                                        b.value_at(at).unwrap(),
                                        &format!("{label}/{hash:#x}"),
                                    )
                                },
                                | (HashTrieNode::Leaf(a), HashTrieNode::Leaf(b)) => assert_eq!(
                                    a.value_at(idx),
                                    b.value_at(at),
                                    "{label}/{hash:#x}: chain"
                                ),
                                | _ => panic!("{label}: root kinds differ"),
                            }
                        }
                    }
                }
            }
        }

        #[test]
        fn tuples_leaves_every_subtrie_as_grow_builds_it() {
            under_every_layout!(
                check_children_unchanged,
                SipHashStrategy,
                FxHashStrategy,
                Mod10HashStrategy
            );
        }

        /// A trie created empty is sized for no tuples, which is 4 buckets under
        /// either value.
        #[test]
        fn a_trie_created_empty_starts_at_four_buckets() {
            for root_capacity in [RootCapacity::Grow, RootCapacity::Tuples] {
                let trie: HashTrie = HashTrie::with_config(2.into(), config(70, root_capacity));
                assert_eq!(trie.root().buckets_len(), 4, "{root_capacity:?}");
            }
        }

        /// `project` rebuilds through `from_tuples_with_config`, so a
        /// projection of a presized trie is presized for its own tuples. The
        /// projection keeps all 100 tuples (bag semantics) but holds only 3
        /// distinct values: grown, its root would stop at 2^3 buckets;
        /// presized for 100 tuples at 70 % it has 2^8.
        #[test]
        fn a_projection_is_presized_too() {
            use crate::relation::Projectable;
            let config = config(70, RootCapacity::Tuples);
            let tuples: Vec<Vec<usize>> = (0..100).map(|k| vec![k, k % 3]).collect();
            let trie: HashTrie = HashTrie::from_tuples_with_config(2.into(), config, tuples);
            let projected = trie.project(vec![1]);
            assert_eq!(projected.tuple_count(), 100);
            assert_eq!(root_log2(&projected), config.root_log2_capacity(100));
            assert_eq!(config.root_log2_capacity(100), 8);
        }
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie::implementation::tests::root_capacity`
Expected: two tests FAIL, because the root is not presized yet:
- `tuples_sizes_the_root_from_the_tuple_count`, on inputs with fewer distinct first values than tuples (`duplicates`, `random`) at 50 % and 70 %;
- `a_projection_is_presized_too`.

The other four PASS before and after the change: they pin behaviour the change must keep. A failure in one of them now means the test is wrong; stop and fix it before Step 3.

- [ ] **Step 3: Implement.** In `implementation.rs`:

1. Import the floor. Change `node::HashTrieNode,` in the top `use` to:

```rust
        hash_table::INITIAL_LOG2_CAPACITY,
        node::HashTrieNode,
```

2. Replace `make_root`'s body, keeping its doc comment:

```rust
    pub(super) fn make_root(arity: usize) -> HashTrieNode<P, E> {
        Self::make_root_sized(arity, INITIAL_LOG2_CAPACITY)
    }

    /// [`make_root`](Self::make_root) at `2^log2_capacity` buckets.
    fn make_root_sized(arity: usize, log2_capacity: u32) -> HashTrieNode<P, E> {
        HashTrieNode::new_table_sized(arity <= 1, log2_capacity)
    }

    /// An empty trie holding `config`, its root sized for a build from
    /// `tuple_count` tuples by [`HashTrieConfig::root_log2_capacity`]: 4
    /// buckets under `RootCapacity::Grow`, and under `Tuples` a capacity at
    /// which `tuple_count` keys never make it grow. Every constructor creates
    /// its root here, so no build path computes a capacity itself. A trie
    /// created empty passes 0.
    pub(super) fn with_config_for(
        header: RelationHeader, config: HashTrieConfig, tuple_count: usize,
    ) -> Self {
        let root = Self::make_root_sized(header.arity(), config.root_log2_capacity(tuple_count));
        Self {
            header,
            root,
            // Counts the tuples inserted so far; the builds add to it.
            tuple_count: 0,
            config,
            _layout: PhantomData,
        }
    }
```

3. In `impl ConfigurableRelation`, replace `with_config`'s body:

```rust
    fn with_config(header: RelationHeader, config: HashTrieConfig) -> Self {
        Self::with_config_for(header, config, 0)
    }
```

4. In `from_tuples_with_config`, replace `let mut trie = Self::with_config(header, config);` with:

```rust
        let mut trie = Self::with_config_for(header, config, tuples.len());
```

5. In the `Radix` arm, replace `let mut trie = Self::with_config(header, config);` with:

```rust
                let mut trie = Self::with_config_for(header, config, tuple_count);
```

6. In the `HashTrie` struct's `# Construction` doc section, add after "…which builds the identical trie.":

```rust
/// `HashTrieConfig::root_capacity` decides the root's starting size: 4
/// buckets (`grow`, the default), or sized once from the tuples a build is
/// given (`tuples`), so that it never grows during that build (#88).
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie`
Expected: PASS, all of `hash_trie`, including `radix` and the new `root_capacity` module.

- [ ] **Step 5: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/implementation.rs
git commit -m "feat(hash-trie): presize the root from the tuple count under root-capacity=tuples (#88)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JRkiMxoczrRC2wDKRphkU2"
```

---

### Task 5: `radix:K` builds the serial trie under both root capacities

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/identity.rs` (add `configs()`)
- Modify: `kermit-ds/src/ds/hash_trie/radix.rs` (`check_identity` and its `use`)

- [ ] **Step 1: Add `configs()`** to `identity.rs`, after `LOAD_PERCENTS`, and extend its `use`:

```rust
use {
    super::{
        config::{HashTrieConfig, LoadFactor, RootCapacity},
        expansion::{ExpansionPolicy, PendingChild},
        hash_table::HashTable,
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::{PruningPolicy, SingletonPayload},
    },
    crate::{cardinality::Cardinality, heap_size::HeapSize, test_support::Lcg},
    kermit_iters::HashStrategy,
};
```

```rust
/// The configs a build-mode identity test runs under: each load factor in
/// [`LOAD_PERCENTS`] with each root capacity. Every build mode must build
/// the same trie under every config (#88).
pub(super) fn configs() -> Vec<HashTrieConfig> {
    LOAD_PERCENTS
        .iter()
        .flat_map(|&percent| {
            [RootCapacity::Grow, RootCapacity::Tuples].map(|root_capacity| HashTrieConfig {
                load_factor: LoadFactor::percent(percent).unwrap(),
                root_capacity,
            })
        })
        .collect()
}
```

- [ ] **Step 2: Loop `check_identity` over `configs()`.** In `radix.rs`'s tests, change the import `identity::{assert_same_trie, inputs, rows, LOAD_PERCENTS},` to `identity::{assert_same_trie, configs, inputs, rows},`, and replace `check_identity` with:

```rust
    /// `radix:K` builds the trie `serial` builds, for every arity, load
    /// factor, root capacity, bit count and input.
    fn check_identity<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
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
                            "{}/{}/{} arity {arity}, load {}%, root {}, radix:{bits}, {input}",
                            H::NAME,
                            P::NAME,
                            E::NAME,
                            config.load_factor.numerator(),
                            config.root_capacity.axis_value(),
                        );
                        assert_same_trie(&build(HashTrieBuildMode::Serial), &build(radix), &label);
                    }
                }
            }
        }
    }
```

If `HashTrieConfig` and `LoadFactor` are now unused in `radix.rs`'s tests, drop them from the `use`. Clippy will say so. `LoadFactor` is still used by `fill_root`'s signature at module level through `super::*`.

- [ ] **Step 3: Mutation check, to verify the test can fail.** Temporarily change the `Radix` arm's `Self::with_config_for(header, config, tuple_count)` to `Self::with_config_for(header, config, 0)`. Run `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --lib hash_trie::radix` and expect FAIL with a `root tuples … capacity` label. Then restore the line exactly, and re-run: expect PASS. Confirm with `git diff kermit-ds/src/ds/hash_trie/implementation.rs` that the restore left no change.

- [ ] **Step 4: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/identity.rs kermit-ds/src/ds/hash_trie/radix.rs
git commit -m "test(hash-trie): radix:K builds the serial trie under both root capacities (#88)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JRkiMxoczrRC2wDKRphkU2"
```

---

### Task 6: CLI: `--ds-config root-capacity=grow|tuples`

**Files:**
- Modify: `kermit/src/options.rs`, at about lines 612–662 (`ConfigChoices`), plus its unit tests at about lines 1126–1205
- Modify: `kermit/tests/cli_hash_trie_config_choice.rs`

- [ ] **Step 1: Write the failing unit tests.** In `options.rs` `mod tests`, add these after `config_choices_reject_bad_load_factors`:

```rust
    #[test]
    fn config_choices_parse_root_capacity() {
        let tuples = ConfigChoices {
            ds_config: vec!["root-capacity=tuples".into()],
        };
        assert_eq!(
            tuples.hash_trie_config_resolved().unwrap().root_capacity,
            RootCapacity::Tuples
        );
        let both = ConfigChoices {
            ds_config: vec!["load-factor=0.5".into(), "root-capacity=grow".into()],
        };
        let resolved = both.hash_trie_config_resolved().unwrap();
        assert_eq!(resolved.root_capacity, RootCapacity::Grow);
        assert_eq!(resolved.load_factor, LoadFactor::percent(50).unwrap());
        assert_eq!(
            ConfigChoices::default()
                .hash_trie_config_resolved()
                .unwrap()
                .root_capacity,
            RootCapacity::Grow
        );
    }

    #[test]
    fn config_choices_reject_bad_root_capacities() {
        for bad in ["", "Grow", "presize", "1024"] {
            let choices = ConfigChoices {
                ds_config: vec![format!("root-capacity={bad}")],
            };
            let msg = choices.hash_trie_config_resolved().unwrap_err().to_string();
            assert!(msg.contains("root-capacity"), "{bad:?}: {msg}");
            assert!(msg.contains("expected grow or tuples"), "{bad:?}: {msg}");
        }
    }
```

In `every_advertised_hash_trie_key_is_accepted`, change `SAMPLE` to:

```rust
        const SAMPLE: &[(&str, &str)] = &[("load-factor", "0.5"), ("root-capacity", "tuples")];
```

The test module reaches `LoadFactor` through `super::*`, from the top-level `kermit_ds::{…}` import at `options.rs:10-13`. Step 3 adds `RootCapacity` to that same import, so the tests need no import of their own.

- [ ] **Step 2: Run them to verify they fail**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit options::tests::config_choices`
Expected: FAIL. `every_advertised_hash_trie_key_is_accepted` fails on the key list, and the two new tests fail with `unknown key "root-capacity"`.

- [ ] **Step 3: Implement.** In `options.rs`:

1. Add `RootCapacity` to the `kermit_ds::{…}` import at the top, beside `HashTrieConfig`.
2. Change the `ds_config` field's doc:

```rust
    /// Runtime values for the selected index structure, as `key=value`
    /// pairs. `HashTrie` accepts `load-factor=<decimal in (0, 1)>` and
    /// `root-capacity=grow|tuples`. Only valid with
    /// `--indexstructure hash-trie` (or `all`).
```

3. Change `HASH_TRIE_KEYS` to `&["load-factor", "root-capacity"]`.
4. Add a match arm after `"load-factor"`:

```rust
                | "root-capacity" => {
                    config.root_capacity = value
                        .parse::<RootCapacity>()
                        .map_err(|why| anyhow::anyhow!("--ds-config {key}: {why}"))?;
                },
```

- [ ] **Step 4: Run the unit tests to verify they pass**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit options::tests`
Expected: PASS.

- [ ] **Step 5: Write the CLI smoke tests.** Append to `kermit/tests/cli_hash_trie_config_choice.rs`:

```rust
#[test]
fn cli_bench_ds_with_root_capacity_records_axis() {
    let (output, report) = bench_ds("hash-trie", &["--ds-config", "root-capacity=tuples"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_config_root_capacity"], "tuples");
}

#[test]
fn cli_bench_ds_default_root_capacity_is_grow() {
    let (output, report) = bench_ds("hash-trie", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(axes_of(&report)["ds_config_root_capacity"], "grow");
}

#[test]
fn cli_bench_ds_rejects_a_malformed_root_capacity() {
    let (output, _) = bench_ds("hash-trie", &["--ds-config", "root-capacity=1024"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("expected grow or tuples"), "{stderr}");
}

#[test]
fn cli_bench_ds_takes_both_config_keys() {
    let (output, report) = bench_ds("hash-trie", &[
        "--ds-config",
        "load-factor=0.5,root-capacity=tuples",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let axes = axes_of(&report);
    assert_eq!(axes["ds_config_load_factor"], 0.5);
    assert_eq!(axes["ds_config_root_capacity"], "tuples");
}
```

Also update the file's `//!` header first line to: `` //! CLI smoke test: `bench ds` with `--ds-config` (`load-factor`, `root-capacity`) records ``.

- [ ] **Step 6: Run the CLI tests**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_hash_trie_config_choice`
Expected: PASS, 8 tests.

- [ ] **Step 7: Lint the binary docs**

Run: `nix develop --command cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings`
Expected: the same 2 pre-existing errors as before, and no new ones.

- [ ] **Step 8: Commit**

```bash
git add kermit/src/options.rs kermit/tests/cli_hash_trie_config_choice.rs
git commit -m "feat(cli): --ds-config root-capacity=grow|tuples (#88)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JRkiMxoczrRC2wDKRphkU2"
```

---

### Task 7: The bench family builds a presized root

`build_relation` is the one config-aware construction site of `bench run` and `bench ds`. This test shows that the axis a report carries describes the root that was built.

**Files:**
- Modify: `kermit/src/execution.rs` (the test module, after `hash_family_build_relation_honours_its_config`)

- [ ] **Step 1: Write the test.** Add it after `hash_family_build_relation_honours_its_config`, and add `RootCapacity` to the test module's `kermit_ds` imports:

```rust
    /// Under `root-capacity=tuples` the family's build sizes the root from
    /// the tuple count, so a report labelled `"tuples"` timed a presized
    /// root. 100 tuples sharing one first value hold one root key: the
    /// grown root keeps 4 buckets, while the presized one has room for 100.
    #[test]
    fn hash_family_build_relation_presizes_the_root_under_tuples() {
        let heap = |root_capacity| {
            let family = HashHtj::<kermit_iters::SipHashStrategy, NoPruning, EagerExpansion>::new(
                HashTrieConfig {
                    root_capacity,
                    ..HashTrieConfig::default()
                },
                HashTrieBuildMode::Serial,
                Planner::stored(LexicographicOptimiser),
            );
            let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
            let tuples = (0..100).map(|b| vec![1, b]).collect();
            family.build_relation(header, tuples).heap_size_bytes()
        };
        assert!(heap(RootCapacity::Tuples) > heap(RootCapacity::Grow));
    }
```

- [ ] **Step 2: Run it.** It passes immediately, because Tasks 3–4 already wired the build. Then run the mutation check: temporarily make `HashTrieConfig::root_log2_capacity`'s `Tuples` arm return `INITIAL_LOG2_CAPACITY`, and confirm this test FAILS. Restore the arm, and confirm with `git diff kermit-ds/src/ds/hash_trie/config.rs` that nothing changed.

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit execution::tests::hash_family_build_relation`
Expected: PASS after the restore.

- [ ] **Step 3: Commit**

```bash
git add kermit/src/execution.rs
git commit -m "test(kermit): the hash family presizes the root it builds (#88)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JRkiMxoczrRC2wDKRphkU2"
```

---

### Task 8: The standard suites under `root-capacity=tuples`

**Files:**
- Modify: `kermit/tests/join_tests.rs`, in three places: the imports (lines 3–16); a new block after the `HalfFull` invocations, which end at the `HashTrieSipLazy … CostBasedOptimiser … HalfFull` invocation before `// ── BuildMode axis`; and the column-orders block's aliases (around line 443) and its rows (before the final `);`)
- Modify: `kermit-ds/tests/hash_trie_tests.rs` (imports; after the `NinetyPercent` aliases; after `hash_trie_test_suite!(HashTrieSipDenseLazy, …)`)
- Modify: `kermit-ds/tests/parquet_tests.rs` (after `parquet_test_suite!(HashTrieSipDense, …)`)

- [ ] **Step 1: `join_tests.rs`.** Add `RootCapacity` to the `kermit_ds::{…}` import. After the last `HalfFull` invocation, add:

```rust
// ── Config axis: root capacity ──────────────────────────────────────────
// The default-config invocations above are the `ds_config_root_capacity:
// "grow"` baseline; these are the alternate (#88). Under `tuples` each
// fixture's root is sized once from its tuple count instead of growing from
// 4 buckets.
define_config_provider!(PresizedRoot, HashTrieConfig, HashTrieConfig {
    root_capacity: RootCapacity::Tuples,
    ..HashTrieConfig::default()
});

define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    LexicographicOptimiser,
    PresizedRoot
);
define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    CardinalityOptimiser,
    PresizedRoot
);
define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    CostBasedOptimiser,
    PresizedRoot
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    LexicographicOptimiser,
    PresizedRoot
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    CardinalityOptimiser,
    PresizedRoot
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    CostBasedOptimiser,
    PresizedRoot
);
// The root is never lazy, so a lazy trie's presized root holds unexpanded
// children like any other.
define_multiway_join_test_suite_with_config!(
    HashTrieSipLazy,
    HashTriejoin,
    LexicographicOptimiser,
    PresizedRoot
);
define_multiway_join_test_suite_with_config!(
    HashTrieSipLazy,
    HashTriejoin,
    CostBasedOptimiser,
    PresizedRoot
);
```

Beside `type HashTrieSipLazyHalfFull = …;` in the column-orders section, add:

```rust
type HashTrieSipPresized = Configured<HashTrieSip, PresizedRoot>;
type HashTrieFxPresized = Configured<HashTrieFx, PresizedRoot>;
type HashTrieSipLazyPresized = Configured<HashTrieSipLazy, PresizedRoot>;
```

Then append these rows to the `define_multiway_join_test_suite_with_column_orders!(…)` invocation, before its closing `);`:

```rust
    HashTrieSipPresized,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipPresized,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipPresized,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieFxPresized,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieFxPresized,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieFxPresized,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSipLazyPresized,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipLazyPresized,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
```

- [ ] **Step 2: `hash_trie_tests.rs`.** Add `RootCapacity` to the `kermit_ds::{…}` import. After `type HashTrieMod10Dense = …;`, add:

```rust
// ── Config variant: a presized root ─────────────────────────────────────
// Under `root-capacity=tuples` the root is sized once from the tuple count
// (#88), so the contract must hold over a root sparser than the grown one.
define_config_provider!(PresizedRoot, HashTrieConfig, HashTrieConfig {
    root_capacity: RootCapacity::Tuples,
    ..HashTrieConfig::default()
});

type HashTrieSipPresized = Configured<HashTrieSip, PresizedRoot>;
```

After `hash_trie_test_suite!(HashTrieSipDenseLazy, SipHashStrategy);`, add:

```rust
hash_trie_test_suite!(HashTrieSipPresized, SipHashStrategy);
```

- [ ] **Step 3: `parquet_tests.rs`.** Add `RootCapacity` to its `kermit_ds` import. After `parquet_test_suite!(HashTrieSipDense, sorted_tuples_dense);`, add:

```rust
// …and with a root presized from the tuple count (#88).
define_config_provider!(PresizedRoot, HashTrieConfig, HashTrieConfig {
    root_capacity: RootCapacity::Tuples,
    ..HashTrieConfig::default()
});

type HashTrieSipPresized = Configured<HashTrieSip, PresizedRoot>;

fn sorted_tuples_presized(relation: &HashTrieSipPresized) -> Vec<Vec<usize>> {
    sorted_tuples(relation)
}

parquet_test_suite!(HashTrieSipPresized, sorted_tuples_presized);
```

- [ ] **Step 4: Run the suites**

Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds --test hash_trie_tests --test parquet_tests`
Expected: PASS. New modules are named `…hashtriesippresized…`.
Run: `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit --test join_tests presizedroot`
Expected: PASS: 8 invocations × 16 patterns, plus the column-orders rows. To see the column-orders tests, also run `cargo test -p kermit --test join_tests presized`.

- [ ] **Step 5: Commit**

```bash
git add kermit/tests/join_tests.rs kermit-ds/tests/hash_trie_tests.rs kermit-ds/tests/parquet_tests.rs
git commit -m "test: the standard suites under root-capacity=tuples (#88)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JRkiMxoczrRC2wDKRphkU2"
```

---

### Task 9: kermit-lab back-fills `grow`

**Files:**
- Modify: `python/kermit-lab/kermit_lab/defaults.py:33-34`
- Modify: `python/kermit-lab/tests/test_defaults.py` (the parametrize list at around lines 23–32)

- [ ] **Step 1: Write the failing test.** Add this row to the `test_hash_trie_axes_backfill_hash_trie_rows_only` parametrize list:

```python
        # Every HashTrie root grew from 4 buckets before #88.
        ("ds_config_root_capacity", "grow", "tuples"),
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd python/kermit-lab && nix develop --command uv run pytest tests/test_defaults.py -q`
Expected: FAIL with a `KeyError` on `("ds_config_root_capacity", "HashTrie")`.

- [ ] **Step 3: Implement.** In `defaults.py`, after the `ds_config_load_factor` entry:

```python
    # Every HashTrie root grew from 4 buckets before issue #88.
    ("ds_config_root_capacity", "HashTrie"): "grow",
```

- [ ] **Step 4: Run kermit-lab's tests**

Run: `cd python/kermit-lab && nix develop --command uv run pytest -q`
Expected: PASS. The real-binary contract test needs `KERMIT_BIN`; if it skips locally, that's fine. CI runs it.

- [ ] **Step 5: Commit**

```bash
git add python/kermit-lab/kermit_lab/defaults.py python/kermit-lab/tests/test_defaults.py
git commit -m "feat(kermit-lab): back-fill ds_config_root_capacity = grow (#88)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JRkiMxoczrRC2wDKRphkU2"
```

---

### Task 10: Documentation

**Files:**
- Modify: `docs/data-structures/hash-trie.md`. That covers § Invariants (the "Load factor cap" bullet, around line 54), § Config flags (around line 275, after the load-factor entry), § Optimizations' BuildMode API line (around line 379), and § Testing (around line 483).
- Modify: `docs/specs/optimization-standard.md`. That covers How to classify (around line 66), the Config table's examples (line 101), walkthrough step 7's literal (around line 504), "Where to look" (after the "First Config consumer" row), and the catalogue (around lines 797–821).
- Modify: `docs/specs/bench-report-schema.md` (append a change-log row)
- Modify: `CLAUDE.md`: line 40 (build commands), line 141 (`ConfigurableRelation` bullet) and line 318 ("Config values are build-time" gotcha)

- [ ] **Step 1: `hash-trie.md`.**

(a) In § Invariants, append to the "Load factor cap" bullet: ` Every table starts at 4 buckets except, under `--ds-config root-capacity=tuples`, the root of a trie built from a known set of tuples, which is sized once for that tuple count (see [Config flags](#config-flags)).`

(b) In § Config flags, after the load-factor entry's last sub-bullet, add:

```markdown
- **Root capacity** (`ds_config_root_capacity`): how large a build makes
  the root table. Under `grow` (the default) the root starts at 4 buckets
  and doubles as keys arrive, like every other table. Under `tuples`, a
  trie built from a known set of n tuples sizes its root once, at the
  smallest power of two ≥ 4 with `n · 100 ≤ capacity · percent`. Distinct
  keys cannot outnumber tuples, so the root never grows during that build.
  This is Algorithm 2, line 3 of the paper (`2^⌈log2(1.25·|L|)⌉`) applied
  to the root: at `load-factor=0.8` the two agree exactly, except that the
  paper gives 2 buckets for one tuple. It is a *value* on a path every
  build takes (the root's starting capacity, read once per trie), so the
  default pays nothing for it. Child tables keep growing from 4 under both
  values, since the serial build creates a child before it knows how many
  tuples the child will hold.
  - **CLI:** `-i hash-trie --ds-config root-capacity=tuples` (combinable:
    `--ds-config load-factor=0.8,root-capacity=tuples`). Any other value is
    a usage error naming `grow` and `tuples`.
  - **Default:** `grow` (the only behaviour before #88).
  - **Rust:** `HashTrieConfig { root_capacity: RootCapacity::Tuples, ..HashTrieConfig::default() }`.
    Every constructor that is given its tuples presizes: the serial and
    `radix:K` builds, `project`, `Configured`, and the bench families
    through `build_relation`. A trie created empty (`new`, `with_config`)
    starts at 4 buckets, and an `insert` after a build may still grow the
    root. All of them create the root through `HashTrie::with_config_for`.
  - **Bench axis value:** the JSON string `"grow"` / `"tuples"`.
  - **Expected effect:** `insertion` falls when the first attribute has
    many distinct values, because the root skips every rehash. `space` rises
    when tuples outnumber distinct first values, because the root is up to
    n/D times the grown one (`heap_size_bytes` counts every bucket).
    `iteration` may slow at the root then, because there are more empty
    buckets to skip. When every first value is distinct, the capacity equals
    the grown one, and only slot placement differs. Unmeasured as of this
    writing.
```

(c) In § Optimizations, change the BuildMode API sentence "To set the load factor as well, use …" to "To set a Config value as well, use …".

(d) In § Testing, after "…for the Config axis." add: ` `HashTrieSipPresized = Configured<HashTrieSip, PresizedRoot>` runs the same suite with the root presized (`root-capacity=tuples`).` Then change the final sentence to: "At the join layer, `define_multiway_join_test_suite_with_config!` runs the same 16 patterns under the `HalfFull` load-factor provider and the `PresizedRoot` root-capacity provider."

- [ ] **Step 2: `optimization-standard.md`.**

(a) How to classify, last sentence: replace "the load-factor cap ✓, an initial-capacity hint, a hash seed." with "the load-factor cap ✓, the root capacity ✓ (#88), a hash seed."

(b) Config table: replace "| Examples (potential) | Load-factor cap ✓, initial capacity, hash seed |" with "| Examples (potential) | Load-factor cap ✓, root capacity ✓, hash seed |".

(c) After the Config section's load-factor "Concrete example" blockquote, add:

```markdown
> **Second concrete example (implemented).** The root capacity (#88). The root table's starting capacity was the constant 4 buckets; `HashTrieConfig::root_capacity` replaces it with a value read once per build: 4 under `grow`, or under `tuples` the smallest power of two that holds the build's tuple count under the load factor (Algorithm 2, line 3, applied to the root). One `match` per trie construction, no per-insert branch, no new node variant.
```

(d) In the walkthrough's step 7, the `define_config_provider!(HalfFull, …)` code block gains `..HashTrieConfig::default(),` after the `load_factor` line. Add one sentence after the block: "A config struct literal names the fields it sets and takes the rest from `..HashTrieConfig::default()`, so adding a value does not break every provider."

(e) "Where to look": after the "First Config consumer (load-factor cap)" row, add:

```markdown
| Second Config consumer (root capacity) | [`kermit-ds/src/ds/hash_trie/config.rs`](../../kermit-ds/src/ds/hash_trie/config.rs) (`RootCapacity`), [`hash_table.rs`](../../kermit-ds/src/ds/hash_trie/hash_table.rs) (`log2_capacity_for`) |
```

(f) Catalogue: change "Eight optimizations are implemented — four Layout dimensions, one Config value and three BuildModes:" to "Nine optimizations are implemented — four Layout dimensions, two Config values and three BuildModes:". After the "Load-factor cap" row of the implemented table, add:

```markdown
| Root capacity (grow / tuples) | Config | `ds_config_root_capacity` | §3.2.2, Alg. 2 line 3 (root only; issue #88) |
```

Then delete the "| Initial capacity hint | Config | Small | (kermit-specific) |" row from the "Available to add" table.

- [ ] **Step 3: `bench-report-schema.md`.** Append this change-log row:

```markdown
| 3 (no bump) | 2026-10-06 | Every HashTrie report carries `ds_config_root_capacity` (#88): `"grow"` (the default, the only behaviour before) or `"tuples"` (`--ds-config root-capacity=tuples`, the root sized once from the tuple count). Under `grow` every metric measures the same build as before, so `schema_version` stays `3`; kermit-lab back-fills `"grow"` on earlier HashTrie rows. `tuples` changes the root's capacity, so it may move `space` and `iteration` as well as the build metrics. |
```

- [ ] **Step 4: `CLAUDE.md`.**

(a) After the build-command line `… --ds-config load-factor=0.5  # Config axis`, add:

```bash
cargo run -- bench run triangle -i hash-trie -a hash-triejoin --ds-config root-capacity=tuples  # Config axis: root sized once from the tuple count (#88)
```

(b) In the `ConfigurableRelation` bullet, replace "(its Config value is the load-factor cap)" with "(its Config values are the load-factor cap and the root capacity)".

(c) In the "Config values are build-time for `HashTrie`" gotcha, replace "the one Config value is `HashTrieConfig::load_factor` (`--ds-config load-factor=0.5`, axis `ds_config_load_factor`, default 0.7), read only while the trie is built — `HashTrie::insert_at` passes it into the resize test in `HashTable::entry_or_insert_with`, which is the path that makes it a Config rather than a Layout (a value substituted for a constant, no new branch)." with:

"the Config values are `HashTrieConfig::load_factor` (`--ds-config load-factor=0.5`, axis `ds_config_load_factor`, default 0.7) and `HashTrieConfig::root_capacity` (`--ds-config root-capacity=tuples`, axis `ds_config_root_capacity`, default `grow`; #88), both read only while the trie is built. `HashTrie::insert_at` passes the load factor into the resize test in `HashTable::entry_or_insert_with`, and `HashTrie::with_config_for` (the one helper every root-creating path calls) reads the root capacity to size the root. Each is a value substituted for a constant, with no new branch, which is what makes it a Config rather than a Layout. A presized root needs the tuple count, so `tuples` only applies to constructors given their tuples (`from_tuples_with_config`, the `radix:K` arm); `new`/`with_config` start at 4 buckets."

- [ ] **Step 5: Verify the docs.** Check that the links resolve, and that the rustdoc/clippy lint the changed `///` text (Task 11 runs them in full). Here: `grep -n "initial-capacity\|Initial capacity hint" docs/specs/optimization-standard.md docs/data-structures/hash-trie.md CLAUDE.md` should print nothing.

- [ ] **Step 6: Commit**

```bash
git add docs/data-structures/hash-trie.md docs/specs/optimization-standard.md docs/specs/bench-report-schema.md CLAUDE.md
git commit -m "docs: the root-capacity Config (#88)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JRkiMxoczrRC2wDKRphkU2"
```

---

### Task 11: The CI gate

- [ ] **Step 1: Check that the host is quiet.** Run `pgrep -a kermit`. If a `kermit bench` process is running, wait for it to exit before Steps 3 and 6.

- [ ] **Step 2: Format.** `nix develop --command cargo fmt --all`, then `git diff --stat`. If anything moved, commit it as `style: rustfmt (#88)`.

- [ ] **Step 3: Lint and docs.**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
nix develop --command cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings   # 2 known pre-existing errors only
```

Expected: clippy and doc clean.

- [ ] **Step 4: Full test suite.** `nix develop --command env CARGO_BUILD_JOBS=2 cargo test`. Expect every test to PASS. If anything fails, stop and debug with superpowers:systematic-debugging before going further.

- [ ] **Step 5: kermit-lab.** `cd python/kermit-lab && nix develop --command uv run pytest -q` should PASS.

- [ ] **Step 6: Miri on `kermit-ds`, detached.** It takes about 25 minutes, and a foreground run risks the memory monitor:

```bash
setsid nohup nix develop --command env MIRIFLAGS=-Zmiri-disable-isolation CARGO_BUILD_JOBS=2 \
  cargo miri test -p kermit-ds > /tmp/claude-1000/-tb-Source-Academia-kermit--loom-worktrees-aidanb-hint-size-88-18dbe58a56b9fdc6/ffb64850-0ac2-45f3-ba2c-d3e1a68adac7/scratchpad/miri.log 2>&1 & disown
```

Wait for it to finish, then `tail -30` the log. Expected: `test result: ok` for every target.

- [ ] **Step 7: Report.** List the commits (`git log --oneline origin/master..HEAD`) and the gate results. Remind the user that nothing is pushed: landing follows the loom recipe (fetch, merge origin/master in, re-run the gate, push `HEAD:master`) and needs their word.
