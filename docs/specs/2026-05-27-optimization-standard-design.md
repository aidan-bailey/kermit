# Optimization Standard — Design

**Date:** 2026-05-27
**Status:** Design
**Predecessor:** [`2026-05-26-hash-triejoin-design.md`](2026-05-26-hash-triejoin-design.md) introduced HashTrie; the spec there explicitly deferred all paper-side optimizations to a follow-up.

## 1. Goal

Establish a kermit-wide standard for adding optimizations to data structures and algorithms — vocabulary, Rust shapes, CLI surface, bench-report axes, and test obligations.

The standard turns "add an optimization" from an ad-hoc per-DS decision into a recognizable, repeatable pattern that future contributors follow by default. It also makes ablation analysis tractable in `kermit-lab` by enforcing a stable axis naming convention.

Validating the standard against a real first consumer: a Layout-category optimization for HashTrie (the hash function choice — `SipHashStrategy` vs `FxHashStrategy`).

## 2. Scope

**In scope.**

- A new trait family in `kermit-iters::optimization` defining three structural categories (Layout, Config, BuildMode) plus an umbrella `HasOptimizationAxes` trait.
- A naming convention for bench-report axes: `ds_layout_*`, `ds_config_*`, `ds_build_mode`.
- A CLI convention: `--ds-layout-<dim>`, `--ds-config <flag>=<value>,…`, `--ds-build <mode>[:<params>]`.
- Bench-report integration: the reporter merges `optimization_axes()` into `BenchReport.axes`.
- Test scaffolding: type aliases for Layout, planned `define_multiway_join_test_suite_with_config!` and `define_multiway_join_test_suite_for_build_mode!` macros for Config and BuildMode.
- First consumer: refactor HashTrie's hash function (currently hard-coded to `DefaultHasher` via the no-op `hash_attribute`) into a generic `HashTrie<H: HashStrategy>` with `SipHashStrategy` and `FxHashStrategy` concrete types.
- Documentation: a normative `docs/specs/optimization-standard.md` (this spec), CLAUDE.md extension, `bench-report-schema.md` update, per-DS doc update.

**Out of scope** (each deferred to follow-up specs):

- All other paper optimizations (singleton pruning, lazy child expansion, pointer tagging, parallel build, radix partitioning) — they become Config or BuildMode consumers of the standard once it lands.
- The Config and BuildMode test macros are *specified* here but not implemented (no first consumer yet).
- Algorithm-side optimization axes (`algo_*` prefix) — reserved in the namespace but no algorithm needs them yet.
- A proc-macro for auto-deriving `HasOptimizationAxes` from component traits — speculative without a second consumer.
- A `--ds-layout-all` / `--ds-config-all` / `--ds-build-all` sweep syntax — one config per invocation; sweeps are user-scripted via shell loops.

## 3. Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Formality of the standard | Trait-level infrastructure (vs. documentation-only or macro-driven) | Compile-time enforcement of the contract via traits, without over-engineering with proc macros. |
| First consumer | Hasher choice (Layout category) | Isolated, no algorithm change, exercises the Layout half of the standard end-to-end; removes the dead `hash_attribute` function. |
| Trait location | `kermit-iters::optimization` (new module) and `kermit-iters::hash_strategy` (new module) | Peer of existing trait families; both consumed by `kermit-ds` (implementors) and `kermit` (binary). |
| Layout selection | Compile-time generic type parameter | Each layout combination is a distinct monomorphized type. Zero runtime overhead. |
| Config selection | Runtime struct of flags | Branches at hot-path call sites; cheap; supports binary-size-friendly multi-flag toggling without combinatorial monomorphization. |
| BuildMode selection | Construction-time value (enum or builder method) | Affects how the structure is built but not its in-memory representation. |
| Singleton iterator coupling | Precomputed hash, no generic | The singleton hashes once at construction. Storing the precomputed hash decouples it from the strategy type and avoids propagating `H` through `HashTrieIterKind<'a, R>`. |
| CLI surface | Separate flag per dimension (`--ds-layout-hasher fxhash`) | Compatible with clap's `ValueEnum`; scales to multiple Layout dimensions without grammar drift. |
| Bench-report schema | Stays at `schema_version: 2`; adds keys under `ds_layout_*` / `ds_config_*` / `ds_build_*` prefixes | Adding optional keys to an open map is non-breaking. |
| `hash_attribute` legacy function | Removed | Phase 7 made it a no-op (`depth` parameter ignored). The new `H::hash(key)` call sites replace it directly. |
| Type-alias convention for tests | `HashTrieSip`, `HashTrieFx` for the layout variants | Lets `define_multiway_join_test_suite!` keep taking `:ident` without changes. |

## 4. Trait family

New module `kermit-iters/src/optimization.rs`:

```rust
use {serde_json::Value, std::collections::BTreeMap};

/// Marker trait for a compile-time layout option (e.g., pointer encoding,
/// hash strategy). Implementing types are typically zero-sized.
pub trait LayoutOption {
    /// Short identifier used in CLI selectors and bench-report axis values.
    /// Must be stable across kermit releases — bench reports embed it.
    const NAME: &'static str;
}

/// Trait for runtime configuration options.
pub trait ConfigOption: Default + Clone {
    /// Serialize the config as `(suffix, value)` pairs.
    /// Bench reporter prepends `ds_config_` (or `algo_config_`) to each suffix.
    fn axes(&self) -> Vec<(&'static str, Value)>;
}

/// Trait for construction-time build modes.
pub trait BuildMode {
    /// String identifier for the bench-report `ds_build_mode` axis.
    /// Format: `"<mode>[:<params>]"`, e.g. `"serial"`, `"parallel:8"`, `"radix:4"`.
    fn axis_value(&self) -> String;
}

/// Umbrella trait consumed by the bench reporter to populate optimization
/// axes in `BenchReport.axes`. A DS or algorithm that adopts the standard
/// implements this; the implementation typically composes results from
/// `LayoutOption::NAME`, `ConfigOption::axes`, and `BuildMode::axis_value`
/// under the correct key prefixes.
pub trait HasOptimizationAxes {
    fn optimization_axes(&self) -> BTreeMap<String, Value>;
}
```

### Key-naming convention (normative)

| Category | Axis key | Value type |
|---|---|---|
| Layout (data structure) | `ds_layout_<dim>` | string (the `LayoutOption::NAME`) |
| Config (data structure) | `ds_config_<flag>` | JSON value (bool, string, number) |
| BuildMode (data structure) | `ds_build_mode` | string `"<mode>[:<params>]"` |
| Layout (algorithm) | `algo_layout_<dim>` | string |
| Config (algorithm) | `algo_config_<flag>` | JSON value |
| BuildMode (algorithm) | `algo_build_mode` | string |

The `algo_*` prefixes are reserved but unused in this first consumer.

## 5. First consumer — hasher choice for HashTrie

### 5.1 `HashStrategy` trait + concrete strategies

New module `kermit-iters/src/hash_strategy.rs`:

```rust
use crate::optimization::LayoutOption;

pub trait HashStrategy: LayoutOption + Copy + Default + 'static {
    /// Hash a `usize` key value to a `u64`.
    fn hash(key: usize) -> u64;
}

#[derive(Copy, Clone, Default, Debug)]
pub struct SipHashStrategy;

impl LayoutOption for SipHashStrategy {
    const NAME: &'static str = "sip";
}

impl HashStrategy for SipHashStrategy {
    fn hash(key: usize) -> u64 {
        use std::{collections::hash_map::DefaultHasher, hash::{Hash, Hasher}};
        let mut h = DefaultHasher::new();
        key.hash(&mut h);
        h.finish()
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct FxHashStrategy;

impl LayoutOption for FxHashStrategy {
    const NAME: &'static str = "fxhash";
}

impl HashStrategy for FxHashStrategy {
    fn hash(key: usize) -> u64 {
        use {rustc_hash::FxHasher, std::hash::{Hash, Hasher}};
        let mut h = FxHasher::default();
        key.hash(&mut h);
        h.finish()
    }
}
```

New dependency: `rustc-hash` in `kermit-iters/Cargo.toml`. Small (~300 LOC), well-known (used by the Rust compiler itself).

### 5.2 `HashTrie<H>` becomes generic

```rust
pub struct HashTrie<H: HashStrategy = SipHashStrategy> {
    header: RelationHeader,
    root: HashTrieNode,
    _hasher: PhantomData<H>,
}

impl<H: HashStrategy> HashTrie<H> {
    fn insert_at(node: &mut HashTrieNode, depth: usize, arity: usize, tuple: Vec<usize>) {
        let key = tuple[depth];
        let hash = H::hash(key);  // was: hash_attribute(depth, key)
        // ... rest unchanged ...
    }
}

impl<H: HashStrategy> HasOptimizationAxes for HashTrie<H> {
    fn optimization_axes(&self) -> BTreeMap<String, Value> {
        let mut axes = BTreeMap::new();
        axes.insert("ds_layout_hasher".to_string(), H::NAME.into());
        axes
    }
}
```

The default `<H = SipHashStrategy>` preserves backwards compatibility — existing `HashTrie` references resolve to `HashTrie<SipHashStrategy>` without any call-site changes.

### 5.3 `SingletonHashTrieIter` decouples from the strategy

```rust
pub struct SingletonHashTrieIter {
    value: usize,
    hash: u64,  // precomputed by caller using the relevant HashStrategy
    chain: Vec<Vec<usize>>,
    state: State,
}

impl SingletonHashTrieIter {
    pub fn new(value: usize, hash: u64) -> Self {
        Self {
            value,
            hash,
            chain: vec![vec![value]],
            state: State::Root,
        }
    }
}
```

Callers (CLI dispatch in `kermit/src/db.rs::hash_join<H>`) compute `H::hash(value)` and pass it in. The iterator stays type-erased over `H`.

### 5.4 `hash_attribute` removed

The legacy free function in `kermit-iters/src/hash_trie.rs` is deleted. Phase 7 had already made its `depth` parameter a no-op; the new `H::hash(key)` call sites replace it directly. No call site in the codebase keeps a reference to it after the refactor.

## 6. CLI integration

### 6.1 CLI surface

```bash
# Defaults to Sip
cargo run -- bench run triangle -i hash-trie -a hash-triejoin

# Pick fxhash
cargo run -- bench run triangle -i hash-trie -a hash-triejoin --ds-layout-hasher fxhash
```

New flag `--ds-layout-hasher <choice>`:
- Type: `clap::ValueEnum` with variants `Sip` (default) and `Fxhash`.
- Validation: rejected at parse time when the chosen DS isn't HashTrie.

### 6.2 `LayoutChoices` struct

```rust
#[derive(Clone, Debug, Default, clap::Args)]
pub struct LayoutChoices {
    #[arg(long = "ds-layout-hasher", value_enum, default_value_t = HasherChoice::Sip)]
    pub hash_trie_hasher: HasherChoice,
}

#[derive(Copy, Clone, Debug, Default, clap::ValueEnum)]
pub enum HasherChoice {
    #[default]
    Sip,
    Fxhash,
}
```

Flattened into the `bench ds` / `bench run` argument structs via `#[command(flatten)]`.

### 6.3 Dispatch — monomorphizing over the chosen strategy

```rust
(IndexStructure::HashTrie, JoinAlgorithm::HashTriejoin) => {
    match layout.hash_trie_hasher {
        HasherChoice::Sip => run_benchmark_hash::<SipHashStrategy>(
            benchmark, query, &bench_args,
        )?,
        HasherChoice::Fxhash => run_benchmark_hash::<FxHashStrategy>(
            benchmark, query, &bench_args,
        )?,
    }
}
```

### 6.4 Validation

Clap-level conditional validation rejects:
- `-i tree-trie --ds-layout-hasher fxhash` (hash function not applicable to TreeTrie).
- `-i column-trie --ds-layout-hasher fxhash` (same for ColumnTrie).

## 7. Bench-report integration

### 7.1 Hookup point

In `run_ds_bench`, `run_benchmark`, `run_ds_bench_hash`, `run_benchmark_hash` in `kermit/src/main.rs`, append one line where the `axes` map is built:

```rust
let mut axes: BTreeMap<String, serde_json::Value> = BTreeMap::new();
// ... existing axes ...

// New: optimization axes from the standard
axes.extend(relation.optimization_axes());
```

`.extend()` merges with no risk of overwriting existing keys — the standard's prefix convention precludes collision.

### 7.2 Schema versioning

`schema_version` stays at `2`. Adding optional keys to an open map is non-breaking.

### 7.3 Documentation

`docs/specs/bench-report-schema.md` gets a new "Standard axis prefixes" section.

## 8. Test scaffolding

### 8.1 Layout — type aliases

```rust
type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx  = HashTrie<FxHashStrategy>;

define_multiway_join_test_suite!(HashTrieSip, HashTriejoin);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin);
```

Total test count after this change: **44** in `join_tests` (11 patterns × 4 DS/algo pairs).

### 8.2 Config macro — specified, not implemented

```rust
define_multiway_join_test_suite_with_config!(
    HashTrieSip, HashTriejoin,
    ("baseline",         HashTrieConfig::default()),
    ("singleton-pruning", HashTrieConfig { singleton_pruning: true, ..Default::default() }),
);
```

### 8.3 BuildMode macro — specified, not implemented

```rust
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip, HashTriejoin,
    HashTrieBuildMode::Parallel { threads: 8 },
);
```

### 8.4 CLI smoke test

New file `kermit/tests/cli_hash_trie_hasher_choice.rs` runs `bench ds -i hash-trie --ds-layout-hasher fxhash` and verifies the JSON report contains `ds_layout_hasher: "fxhash"`.

### 8.5 Test obligation summary

CLAUDE.md Priorities item 1 wording extension:

> Each Layout combination on a DS is a distinct test variant (e.g., `HashTrieSip`, `HashTrieFx`); each must run the 11 standard join patterns under every compatible algorithm. Config dimensions are tested with at least the default configuration and one alternate per flag, via `define_multiway_join_test_suite_with_config!`. BuildMode dimensions are tested for output equivalence with the default mode via `define_multiway_join_test_suite_for_build_mode!`.

## 9. Documentation artifacts

1. **`docs/specs/optimization-standard.md`** — the normative standard.
2. **`CLAUDE.md`** — extends "Extending the System" with the recipe + updates Priorities item 1.
3. **`docs/specs/bench-report-schema.md`** — new "Standard axis prefixes" section.
4. **`docs/data-structures/hash-trie.md`** — gains an "Optimizations" section listing the hasher choice.

## 10. Invariants

1. **Axis-key prefix discipline.** Every optimization axis emitted by a DS or algorithm's `optimization_axes()` impl uses one of the four prefixes: `ds_layout_`, `ds_config_`, `ds_build_mode`, `algo_*` (analogous).
2. **`LayoutOption::NAME` is stable.** Once a Layout option is published, its `NAME` constant is part of the public API.
3. **Default configurations match historical behavior.** Adding a new Config flag must default to `false`. Adding a new Layout dimension must provide a default that matches pre-standard behavior.
4. **`HasOptimizationAxes` is total.** Implementations must return a complete picture of the DS or algorithm's active optimizations.
5. **Singleton iterators use the same hash strategy as their relations.** The CLI dispatch threads a single `H: HashStrategy` through the entire query.

## 11. Out of scope (explicit)

- Singleton pruning, lazy expansion, pointer tagging, parallel build, radix partitioning — each becomes a Config or BuildMode consumer of the standard.
- Algorithm-side optimizations — the `algo_*` prefixes are reserved but no current algorithm needs them.
- Proc-macro for deriving `HasOptimizationAxes`.
- CLI sweep syntax (`--ds-layout-hasher all`).
- Config macro and BuildMode macro implementations.

## 12. References

- Predecessor spec: [`2026-05-26-hash-triejoin-design.md`](2026-05-26-hash-triejoin-design.md).
- Paper: Freitag, Bandle, Schmidt, Kemper, Neumann. *Combining Worst-Case Optimal and Traditional Binary Join Processing*. SIGMOD 2020.
- Existing schema: [`bench-report-schema.md`](bench-report-schema.md).
- Project conventions: `CLAUDE.md`.
