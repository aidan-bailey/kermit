# Root Capacity: a HashTrie Config That Presizes the Root

**Date:** 2026-10-06
**Status:** Design approved (brainstormed 2026-10-06); not yet implemented
**Scope:** Issue #88. `HashTrieConfig` gains a second Config value,
`root_capacity`. At its default, `grow`, the root starts at 4 buckets and
doubles, as today. At `tuples`, a build from a known set of tuples sizes
the root once, from the tuple count, so the root never grows during that
build. Child tables are unchanged under both values.
**Related:** the `hash-trie-parallel` session's paper-faithful parallel
build, which needs the root's final size before it partitions (see
[Coordination](#coordination-with-the-parallel-build)); #91 (the `radix:K`
build, whose identity tests this extends); #92 (lazy child expansion).

## Motivation

Every HashTrie hash table starts at 4 buckets (`HashTable::new`). It
doubles whenever an insert would push it past the load-factor cap, and each
doubling rehashes every entry. A build from a known set of tuples already
knows how many keys the root can hold at most, so it can skip those
doublings.

A second, stronger reason comes from the parallel build. When the root
grows from 4 buckets, its final size depends on D, the number of distinct
first-attribute hashes, and D is known only after grouping. When the root
is sized from the tuple count n, its final size is a pure function of n and
the load factor. That function can be evaluated before any partitioning, so
a parallel build can partition the input straight into the root's bucket
regions.

## What the paper does

Algorithm 2, line 3, is the same in the VLDB 2020 paper and in TUM-I2082:

> `M ← allocateHashtable(2^⌈log2(1.25·|L|)⌉)`

The paper sizes every table once, before inserting anything, from the
number of *tuples* `|L|` in the list it is building, and never resizes it.
Distinct hashes cannot outnumber tuples, so a table stays at or below 80 %
occupancy by construction. Algorithm 2 is recursive: it groups the tuples
into the root's buckets (lines 4–7), then builds each child from its
bucket's list (lines 8–12), so every child is sized from its own `|L|`.
Under lazy expansion the chain length rides in 14 spare bits of the tagged
child pointer, so a child can be sized when it is expanded (TUM-I2082
§3.3.1). TUM-I2082 §3.3.2 builds the root in parallel, with lock-free
atomic inserts into the presized table. That is only possible because the
table cannot resize.

The paper has no numeric capacity hint; the tuple count is its only sizing
rule.

## Decision

This is the section the parallel-build design depends on.

1. **Values.** `--ds-config root-capacity=grow|tuples`. `grow` is the
   default. There is no numeric form: the paper has none, one fixed N means
   nothing across relations of different sizes, and a root that starts at N
   still grows once D exceeds about N times the load factor, so its final
   size would depend on D again.
2. **Root only.** Child tables keep growing from 4 under both values.
   Per-level sizing needs the paper's group-then-recurse build, because the
   serial `insert_at` creates a child before its final `|L|` is known. That
   build belongs to the parallel-build design, not to #88.
3. **The sizing function.** Under `tuples`, a build from n tuples gives the
   root `2^p` buckets, where p is the smallest integer ≥ 2 with
   `n · 100 ≤ 2^p · percent` (`percent` being the load factor). The resize
   test fires when `(len + 1) · 100 > capacity · percent`, and `len + 1 ≤ D ≤ n`,
   so the root never grows during the build. At `load-factor=0.8` the
   function is the paper's `⌈log2(1.25·n)⌉`, apart from the 4-bucket floor:
   the paper allocates 2 buckets for one tuple.
4. **Which builds presize.** The root is presized by every constructor that
   is given its tuples: `from_tuples_with_config` (serial), the `radix:K`
   arm of `from_tuples_with_config_and_build_mode`, and anything built on
   them (`project`, `Configured`, the bench families). A trie created empty
   (`new`, `with_config`) is sized for n = 0, which is 4 buckets under
   either value, and so is the incremental `insert` path. An `insert` after
   a build may still grow the root.
5. **One helper.** Every path that creates a root calls
   `HashTrie::with_config_for(header, config, tuple_count)`. `with_config`
   is `with_config_for(header, config, 0)`. No build arm computes a capacity
   itself.
6. **Build modes agree within a config.** Under either value, every build
   mode builds an equivalent trie: the same contents and the same
   capacities, which is the BuildMode rule. Serial and `radix:K` go
   further and agree bucket for bucket. Both insert root keys in
   first-appearance order, and a root that never grows has a layout that
   depends only on that order, so the two presized roots are identical.
7. **The default changes nothing.** Under `grow` the root is built by
   `HashTable::with_log2_capacity(2)`, which is what `HashTable::new()` did.
   Every array, capacity and `heap_size_bytes` value is unchanged. A test
   pins this (see [Tests](#tests)).
8. **Crate-visible names**, for the parallel build to build on:
   - `hash_table::log2_capacity_for(keys: usize, load_factor: LoadFactor) -> u32`:
     the smallest p ≥ 2 with `keys · 100 ≤ 2^p · percent`. This is the
     general piece; per-level sizing can reuse it.
   - `HashTrieConfig::root_log2_capacity(self, tuple_count: usize) -> u32`:
     2 under `Grow`, and `log2_capacity_for(tuple_count, self.load_factor)`
     under `Tuples`.
   - `HashTable::with_log2_capacity(log2_capacity: u32) -> HashTable<V>`:
     an empty table of `2^log2_capacity` buckets. `HashTable::new()` is
     `with_log2_capacity(2)`.
   - `HashTrie::with_config_for(header, config, tuple_count)`: the one
     root-creating helper, `pub(super)`.

## Classification

**Config.** The value replaces one constant, the root's starting
`log2_capacity` of 2, on a path every build already takes (root creation).
It is read once per trie, not once per insert, and it adds no node variant,
no per-table field and no branch in any probe loop. A run at the default
builds its root exactly as before. The non-user tax is one `match` per trie
construction plus one byte in the `HashTrieConfig` each trie already
carries.

It is not a BuildMode. A BuildMode must build the same structure, and
`tuples` changes the root's capacity, so it changes `space` and may change
`iteration`. Those are exactly the metrics a Config is allowed to move.

## Design

### Types (`kermit-ds/src/ds/hash_trie/config.rs`)

```rust
/// How large a build makes the root table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RootCapacity {
    /// Start at 4 buckets and double as keys arrive: the only behaviour
    /// before #88.
    #[default]
    Grow,
    /// Size the root once, from the number of tuples the build is given,
    /// so it never grows during that build (Algorithm 2, line 3).
    Tuples,
}

pub struct HashTrieConfig {
    pub load_factor: LoadFactor,
    pub root_capacity: RootCapacity,   // new
}
```

`RootCapacity` implements `FromStr` and has an `axis_value()`, with a
round-trip test, as `HashTrieBuildMode` does. Its parse error names both
values: `expected grow or tuples, got "x"`. `HashTrieConfig::axes()` adds
`("root_capacity", "grow" | "tuples")`, so every HashTrie report carries
`ds_config_root_capacity`. `RootCapacity` is re-exported beside
`HashTrieConfig` and `LoadFactor`.

Adding a field breaks every `HashTrieConfig { load_factor: … }` literal
(about 30 sites in code and tests). Each gains `..HashTrieConfig::default()`.
The historical plans and specs under `docs/` keep their literals as
written; `optimization-standard.md`'s walkthrough is updated, since it is
the live recipe.

### Sizing (`hash_table.rs`)

`log2_capacity_for(keys, load_factor)` computes, in `u128` so that it is
total for every `usize` input, `needed = ⌈keys · den / num⌉`, then returns
`max(2, ⌈log2(needed)⌉)`. It panics with a capacity-overflow message if the
result reaches 64, the limit of `MULTIPLIERS`. That can only happen for
inputs no `Vec` could allocate.

`HashTable::with_log2_capacity(p)` builds `2^p` empty buckets with the same
`collect` that `new()` uses, so the allocation's capacity is exact, and
asserts `1 ≤ p < 64`. `bucket_index` shifts by `64 - p`, so p = 0 would
shift by 64.

### Construction (`implementation.rs`)

| Path | Tuple count passed | Root under `tuples` |
|---|---|---|
| `Relation::new`, `ConfigurableRelation::with_config` | 0 | 4 buckets |
| `Relation::insert` / `insert_all` after `new` | (no new root) | grows as today |
| `from_tuples_with_config` (serial) | `tuples.len()` | presized |
| `from_tuples_with_config_and_build_mode`, `Radix` arm | `tuples.len()` | presized |
| `project` → `from_tuples_with_config` | projected count | presized |
| `radix::build_scratch_root` | (scratch, consumed by the merge) | 4 buckets, unchanged |
| `expand_level` (lazy children) | (child) | unchanged |

`make_root(arity)` keeps its signature for the radix scratch roots and
delegates to a sized variant, `make_root_sized(arity, log2_capacity)`, which
`with_config_for` calls. Leaving the scratch path alone keeps `radix.rs`'s
build steps untouched.

### CLI (`kermit/src/options.rs`)

`ConfigChoices::HASH_TRIE_KEYS` becomes `["load-factor", "root-capacity"]`,
with a match arm that parses through `RootCapacity::from_str`. Errors read
`--ds-config root-capacity: expected grow or tuples, got "x"`. The flag's
help text names both keys. `DsFlag::Config` already lists only HashTrie, so
validation does not change.

### Report and kermit-lab

`ds_config_root_capacity` is a new `axes` key with the value `"grow"` or
`"tuples"`. It is additive, so `schema_version` stays 3, and
`bench-report-schema.md` gets a changelog row. kermit-lab back-fills
`"grow"` on earlier HashTrie rows (`SCOPED_AXIS_DEFAULTS`), with a test
beside the existing back-fill tests.

## Tests

1. **Default identity (acceptance).** For every Layout alias (Sip/Fx ×
   pruning off/on × eager/lazy), arities 1–3 and the radix suite's inputs,
   `from_tuples_with_config(default)` is array-identical to `with_config`
   followed by one `insert` per tuple: every table's capacity, allocation,
   length and buckets, every chain, and `heap_size_bytes`. The second path
   is the loop `from_tuples_with_config` ran before #88 and never sees a
   tuple count, so the test shows the default build does not presize. The
   existing `new_starts_empty_with_4_buckets` still pins the 4-bucket start.
2. **The root never grows under `tuples`.** For every Layout alias, at load
   factors 50 %, 70 % and 95 %:
   - Unary relations with every key distinct (D = n, the worst case), and
     random arity-2 and arity-3 inputs. The root's final `buckets_len()`
     equals `2^root_log2_capacity(n)`. A table never shrinks, so equal
     final and initial capacities mean it never grew.
   - For D = n and load factors ≥ 25 %, the presized capacity equals the
     capacity `grow` reaches, so in that case `tuples` changes when the root
     reaches its size, not the size itself. Below 25 % a single doubling
     can leave a small table over its cap (see
     [Out of scope](#out-of-scope)), so the comparison stops there.
   - Every subtrie under `tuples` is array-identical to the subtrie under
     `grow` for the same root key, which pins "root only".
   - `with_config` under `tuples` starts at 4 buckets.
3. **`log2_capacity_for`.** For n in 0..2000 and load factors from 1 % to
   99 %, the result is sufficient (`n · 100 ≤ 2^p · percent`) and minimal
   (`p = 2`, or `2^(p-1)` is insufficient). At 80 % it equals
   `⌈log2(1.25·n)⌉` for n ≥ 2. A table presized by it takes `keys` distinct
   inserts without growing. `with_log2_capacity(p)` has `2^p` buckets, and
   `new()` equals `with_log2_capacity(2)`.
4. **Build-mode identity.** `radix.rs`'s `check_identity` loops over
   `LOAD_PERCENTS × {Grow, Tuples}` instead of `LOAD_PERCENTS`, so
   `serial == radix:K` is pinned at array level under both values. Under
   Miri the matrix doubles from one config to two, which is still small.
5. **Join suites.** A `PresizedRoot` provider
   (`root_capacity: RootCapacity::Tuples`, default load factor) through
   `define_multiway_join_test_suite_with_config!`, mirroring `HalfFull`: Sip
   and Fx under all three optimisers, SipLazy under lexicographic and
   cost-based, and the same rows under `AnyOrders`.
6. **Structure suites.** `HashTrieSipPresized = Configured<HashTrieSip, PresizedRoot>`
   through `hash_trie_test_suite!` (`kermit-ds/tests/hash_trie_tests.rs`)
   and `parquet_test_suite!` (`kermit-ds/tests/parquet_tests.rs`), as
   `NinetyPercent` is.
7. **Config unit tests.** The default is `Grow`; `axes()` reports
   `"grow"` / `"tuples"`; `FromStr` round-trips every value; malformed
   values fail with both forms named.
8. **CLI.** In `kermit/tests/cli_hash_trie_config_choice.rs`:
   - `root-capacity=tuples` records `ds_config_root_capacity: "tuples"`.
   - The default records `"grow"`.
   - A malformed value is a usage error naming `grow or tuples`.
   - `load-factor=0.5,root-capacity=tuples` records both axes.

   The `options.rs` unit tests gain parse and reject cases, plus a sample
   row in the per-key table test.

## Documentation

- `docs/data-structures/hash-trie.md` § Config flags: a **Root capacity**
  entry with the same sub-bullets as the load factor (where the value is
  read, CLI, default, Rust, axis value, expected effect, and the paper's
  formula). § Invariants: the root may be presized. § Testing: the new
  aliases.
- `docs/specs/optimization-standard.md`:
  - the "Initial capacity hint" row moves to the landed table as
    "Root capacity (grow / tuples)", Config, `ds_config_root_capacity`,
    §3.2.2 Alg. 2 line 3 (root only);
  - the count above the table becomes "two Config values";
  - the Config table's examples become "Load-factor cap ✓, root capacity ✓,
    hash seed";
  - the walkthrough literals gain `..HashTrieConfig::default()`.
- `docs/specs/bench-report-schema.md`: a changelog row.
- `CLAUDE.md`:
  - the `ConfigurableRelation` bullet and the "Config values are build-time
    for `HashTrie`" gotcha, both of which say the load factor is the only
    value;
  - a build-command example: `--ds-config root-capacity=tuples`.

## Expected effects (unmeasured)

- **`insertion`** falls when D is large: the root skips every rehash, which
  in total moves about D entries, plus the allocations.
- **`space`** rises when n ≫ D. The root is up to n/D times larger than
  under `grow`, and `heap_size_bytes` counts every bucket. On a
  heavily duplicated first attribute (n = 10⁷, D = 10²) the root becomes
  2^24 buckets at 70 %, where `grow` would give 2^8.
- **`iteration`** may slow at the root when n ≫ D, because the root walk
  (`next_occupied`) skips more empty buckets. Children are unchanged.
- **At D = n** the capacity is the same as under `grow`. Only slot
  placement can differ, since no rehash reorders the entries, so `space` is
  unchanged.

## Out of scope

- **Per-level sizing.** Belongs to the parallel-build design (Decision 2).
- **A numeric capacity hint.** Not in the paper (Decision 1).
- **Pre-existing: below 25 %, one doubling can leave a table over its cap.**
  `entry_or_insert_with` doubles once per insert. For load factors under
  25 % at small capacities, one doubling is not enough. At 10 %, a 4-bucket
  table holding its first key grows to 8 buckets and sits at 12.5 %. The
  table soon catches up and probing still terminates, so this is harmless.
  But it contradicts the documented cap. It is filed separately as
  [#104](https://github.com/aidan-bailey/kermit/issues/104) rather than
  fixed here (Priority 6).

## Coordination with the parallel build

The `hash-trie-parallel` branch (unpushed) adds
`HashTrieBuildMode::Parallel`, refactors the Radix arm into
`HashTrie::from_tuples_partitioned(header, config, tuples, fill)`, and moves
`radix.rs`'s identity helpers into a test-only `identity.rs`. Whichever
branch lands second resolves the conflicts:

- `from_tuples_partitioned` must call `with_config_for(header, config,
  tuples.len())` where it now calls `with_config`. That is the only
  call-site change.
- Every `HashTrieConfig { load_factor: … }` literal on that branch (its
  `identity.rs` and parallel tests) needs `..HashTrieConfig::default()`.
- `check_identity`'s config loop (Test 4) moves into `identity.rs`, and the
  parallel build's identity tests should run under both values.
