# Singleton Pruning Config Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land singleton pruning on `HashTrie` as the first Config consumer of the optimization standard, with the config-injection seam, the `with_config` test macro, the `--ds-config` CLI flag, and the `ds_config_singleton_pruning` bench axis.

**Architecture:** A runtime `HashTrieConfig` value reaches `HashTrie` through a new `ConfigurableRelation` trait (only `HashTrie` implements it). Pruning adds a `Singleton(Vec<usize>)` node variant built at insert time and emulated transparently by a two-variant iterator frame, so `HashTriejoin` and the `HashTrieIterator` trait are untouched. A generic `Configured<R, P>` wrapper lifts a config value to a type so the existing macro suites can name a configured relation.

**Tech Stack:** Rust nightly workspace (see `rust-toolchain.toml`), clap 4 derive, `paste` for macro hygiene, serde_json for axes, Python/uv for `kermit-lab`.

**Spec:** `docs/specs/2026-09-08-singleton-pruning-config-design.md`. Read it first.

**Conventions that apply to every task:**

- Run `cargo fmt --all` only inside `nix develop` (stable rustfmt rewrites 30+ files). Use `nix develop --command cargo fmt --all`.
- CI treats warnings as errors: `cargo clippy --all-targets` and `cargo doc --workspace` must be clean. Backtick code-like identifiers in `///` comments or clippy `doc_markdown` fails.
- Commit messages are conventional commits. End each with:
  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_0113pPzevVrcidsKvjdYT5VH
  ```
- Do not modify `TreeTrie`, `ColumnTrie`, `HashTriejoin`, or `kermit-iters/src/hash_trie.rs`.

---

## File structure

| File | Responsibility | Action |
|---|---|---|
| `kermit-ds/src/ds/hash_trie/config.rs` | `HashTrieConfig` + `ConfigOption` impl | Create |
| `kermit-ds/src/ds/hash_trie/node.rs` | `HashTrieNode` gains `Singleton` | Modify |
| `kermit-ds/src/ds/hash_trie/implementation.rs` | config field, `ConfigurableRelation`, pruning insert, axes | Modify |
| `kermit-ds/src/ds/hash_trie/hash_trie_iter.rs` | `Frame` enum, singleton emulation | Modify |
| `kermit-ds/src/ds/hash_trie/mod.rs` | register `config`, export `HashTrieConfig` | Modify |
| `kermit-ds/src/ds/mod.rs` | re-export `HashTrieConfig` | Modify |
| `kermit-ds/src/relation.rs` | `ConfigurableRelation` trait; `read_csv` / `read_parquet` helpers | Modify |
| `kermit-ds/src/configured.rs` | `Configured<R, P>`, `ConfigProvider`, `define_config_provider!` | Create |
| `kermit-ds/src/lib.rs` | crate-root exports | Modify |
| `kermit-ds/tests/hash_trie_tests.rs` | DS-level suites on pruned aliases, collision unprune test | Modify |
| `kermit/tests/common/macros.rs` | `define_multiway_join_test_suite_with_config!` | Modify |
| `kermit/tests/join_tests.rs` | pruning-on invocations | Modify |
| `kermit/src/execution.rs` | `Execution::HashHtj { hasher, config }`, `HashHtj` carries config | Modify |
| `kermit/src/main.rs` | `ConfigChoices`, parser, validation, dispatch | Modify |
| `kermit/tests/cli_hash_trie_config_choice.rs` | CLI smoke test | Create |
| `python/kermit-lab/kermit_lab/defaults.py` | axis default | Modify |
| `python/kermit-lab/tests/test_defaults.py` | axis default test | Modify |
| `docs/data-structures/hash-trie.md`, `docs/specs/optimization-standard.md`, `CLAUDE.md` | docs | Modify |

---

### Task 1: `HashTrieConfig`

**Files:**
- Create: `kermit-ds/src/ds/hash_trie/config.rs`
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs`
- Modify: `kermit-ds/src/ds/mod.rs:10`
- Modify: `kermit-ds/src/lib.rs:25-30`

- [ ] **Step 1: Write the failing test**

Create `kermit-ds/src/ds/hash_trie/config.rs` with only the test module:

```rust
//! Runtime configuration flags for [`HashTrie`](super::HashTrie) — the
//! Config category of the optimization standard
//! (`docs/specs/optimization-standard.md`).

#[cfg(test)]
mod tests {
    use {super::*, kermit_iters::ConfigOption, serde_json::Value};

    #[test]
    fn default_config_has_pruning_off() {
        assert!(!HashTrieConfig::default().singleton_pruning);
    }

    #[test]
    fn axes_report_singleton_pruning_suffix_and_value() {
        let on = HashTrieConfig {
            singleton_pruning: true,
        };
        assert_eq!(on.axes(), vec![("singleton_pruning", Value::Bool(true))]);
        assert_eq!(
            HashTrieConfig::default().axes(),
            vec![("singleton_pruning", Value::Bool(false))]
        );
    }
}
```

Register the module in `kermit-ds/src/ds/hash_trie/mod.rs` (add after `mod hash_table;` block):

```rust
mod config;
```

and change the export line to:

```rust
pub use {config::HashTrieConfig, implementation::HashTrie};
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kermit-ds config::tests`
Expected: compile error, `HashTrieConfig` not found.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` block in `config.rs`:

```rust
use {kermit_iters::ConfigOption, serde_json::Value};

/// Runtime flags read by `HashTrie` while it is built.
///
/// One type covers every configuration (unlike the Layout parameter `H`,
/// which monomorphises). The flag is read at insert time; the iterator
/// branches on the node variant it finds, never on this struct.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashTrieConfig {
    /// Singleton pruning (SIGMOD 2020 §3.3.1, Figure 5): a subtrie holding
    /// exactly one tuple is stored as that tuple instead of one hash table
    /// per remaining level. Bench axis `ds_config_singleton_pruning`.
    pub singleton_pruning: bool,
}

impl ConfigOption for HashTrieConfig {
    fn axes(&self) -> Vec<(&'static str, Value)> {
        vec![("singleton_pruning", Value::Bool(self.singleton_pruning))]
    }
}
```

Export from `kermit-ds/src/ds/mod.rs` line 10:

```rust
pub use {
    column_trie::ColumnTrie,
    hash_trie::{HashTrie, HashTrieConfig},
    tree_trie::TreeTrie,
};
```

Export from `kermit-ds/src/lib.rs` (the `pub use` block):

```rust
pub use {
    cardinality::Cardinality,
    ds::{ColumnTrie, HashTrie, HashTrieConfig, IndexStructure, TreeTrie},
    heap_size::HeapSize,
    relation::{ModelType, Projectable, Relation, RelationError, RelationFileExt, RelationHeader},
};
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kermit-ds config::tests`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/config.rs kermit-ds/src/ds/hash_trie/mod.rs kermit-ds/src/ds/mod.rs kermit-ds/src/lib.rs
git commit -m "feat(ds): add HashTrieConfig with the singleton_pruning flag"
```

---

### Task 2: `ConfigurableRelation` and the config field on `HashTrie`

**Files:**
- Modify: `kermit-ds/src/relation.rs` (after the `Relation` trait, before `RelationFileExt`)
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs`
- Modify: `kermit-ds/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `kermit-ds/src/ds/hash_trie/implementation.rs`:

```rust
    #[test]
    fn with_config_stores_config_and_new_uses_default() {
        use crate::relation::ConfigurableRelation;
        let on = HashTrieConfig {
            singleton_pruning: true,
        };
        let trie: HashTrie = HashTrie::with_config(2.into(), on);
        assert_eq!(*trie.config(), on);
        let plain: HashTrie = HashTrie::new(2.into());
        assert_eq!(*plain.config(), HashTrieConfig::default());
    }

    #[test]
    fn project_preserves_config() {
        use crate::relation::{ConfigurableRelation, Projectable};
        let on = HashTrieConfig {
            singleton_pruning: true,
        };
        let trie: HashTrie =
            HashTrie::from_tuples_with_config(2.into(), on, vec![vec![1, 2], vec![3, 4]]);
        let projected = trie.project(vec![1]);
        assert_eq!(*projected.config(), on);
        let mut got = projected.collect_tuples();
        got.sort();
        assert_eq!(got, vec![vec![2], vec![4]]);
    }

    #[test]
    fn optimization_axes_include_config_flag() {
        use {crate::relation::ConfigurableRelation, kermit_iters::HasOptimizationAxes};
        let on = HashTrieConfig {
            singleton_pruning: true,
        };
        let trie: HashTrie = HashTrie::with_config(2.into(), on);
        let axes = trie.optimization_axes();
        assert_eq!(
            axes.get("ds_config_singleton_pruning"),
            Some(&serde_json::Value::Bool(true))
        );
        assert_eq!(
            axes.get("ds_layout_hasher"),
            Some(&serde_json::Value::String("sip".into()))
        );
        let plain: HashTrie = HashTrie::new(2.into());
        assert_eq!(
            plain.optimization_axes().get("ds_config_singleton_pruning"),
            Some(&serde_json::Value::Bool(false))
        );
    }
```

Add `use super::config::HashTrieConfig;` to the `tests` module's imports is unnecessary because `use super::*` will pick it up once the implementation imports it.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kermit-ds implementation::tests`
Expected: compile errors (`ConfigurableRelation` not found, `with_config` missing).

- [ ] **Step 3: Add the trait to `kermit-ds/src/relation.rs`**

Insert immediately after the closing brace of `pub trait Relation { ... }` (line 274) and before the `RelationFileExt` doc comment:

```rust
/// A [`Relation`] with runtime configuration — the Config category of the
/// optimization standard (`docs/specs/optimization-standard.md`).
///
/// `Relation::new` / `Relation::from_tuples` have no parameter through which
/// a config value could travel, so this extension trait adds the
/// config-carrying constructors. Implementors route `Relation::new` through
/// [`with_config`](Self::with_config) with `Self::Config::default()`, so a
/// relation built through the plain trait is the default configuration.
///
/// Only structures with a Config axis implement this; structures without
/// one are not required to.
pub trait ConfigurableRelation: Relation {
    /// The runtime flags this structure reads.
    type Config: kermit_iters::ConfigOption;

    /// Creates an empty relation matching `header` that will honour
    /// `config` for every subsequent insert.
    fn with_config(header: RelationHeader, config: Self::Config) -> Self;

    /// Creates a relation populated with `tuples` under `config`. Same
    /// contract as [`Relation::from_tuples`].
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    fn from_tuples_with_config(
        header: RelationHeader, config: Self::Config, tuples: Vec<Vec<usize>>,
    ) -> Self;

    /// The configuration this relation was built with.
    fn config(&self) -> &Self::Config;
}
```

Export it from `kermit-ds/src/lib.rs`:

```rust
    relation::{
        ConfigurableRelation, ModelType, Projectable, Relation, RelationError, RelationFileExt,
        RelationHeader,
    },
```

- [ ] **Step 4: Add the config field and impls to `HashTrie`**

In `kermit-ds/src/ds/hash_trie/implementation.rs`, change the imports to:

```rust
use {
    super::{config::HashTrieConfig, node::HashTrieNode},
    crate::relation::{ConfigurableRelation, Relation, RelationHeader},
    kermit_iters::{ConfigOption, HashStrategy, JoinIterable, SipHashStrategy},
    std::marker::PhantomData,
};
```

Change the struct:

```rust
pub struct HashTrie<H: HashStrategy = SipHashStrategy> {
    header: RelationHeader,
    root: HashTrieNode,
    /// Number of stored tuples (multiset: duplicates count); maintained
    /// by `insert` and `from_tuples`.
    tuple_count: usize,
    /// Runtime flags, fixed at construction; read by `insert_at`.
    config: HashTrieConfig,
    _hasher: PhantomData<H>,
}
```

Replace the `Relation` impl's `new` and `from_tuples` with delegations and add the `ConfigurableRelation` impl:

```rust
impl<H: HashStrategy> Relation for HashTrie<H> {
    fn header(&self) -> &RelationHeader { &self.header }

    fn new(header: RelationHeader) -> Self { Self::with_config(header, HashTrieConfig::default()) }

    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        Self::from_tuples_with_config(header, HashTrieConfig::default(), tuples)
    }

    fn insert(&mut self, tuple: Vec<usize>) {
        assert_eq!(
            tuple.len(),
            self.header.arity(),
            "tuple arity {} does not match relation arity {}",
            tuple.len(),
            self.header.arity()
        );
        let arity = self.header.arity();
        Self::insert_at(&mut self.root, 0, arity, tuple);
        self.tuple_count += 1;
    }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) {
        for tuple in tuples {
            self.insert(tuple);
        }
    }
}

impl<H: HashStrategy> ConfigurableRelation for HashTrie<H> {
    type Config = HashTrieConfig;

    fn with_config(header: RelationHeader, config: HashTrieConfig) -> Self {
        let root = Self::make_root(header.arity());
        Self {
            header,
            root,
            tuple_count: 0,
            config,
            _hasher: PhantomData,
        }
    }

    fn from_tuples_with_config(
        header: RelationHeader, config: HashTrieConfig, tuples: Vec<Vec<usize>>,
    ) -> Self {
        let arity = header.arity();
        let mut trie = Self::with_config(header, config);
        for tuple in tuples {
            assert_eq!(
                tuple.len(),
                arity,
                "from_tuples: tuple arity {} does not match header arity {}",
                tuple.len(),
                arity,
            );
            Self::insert_at(&mut trie.root, 0, arity, tuple);
            // from_tuples bypasses insert(), so count here. If this loop is
            // ever refactored to route through insert(), drop this increment
            // or the counter double-counts.
            trie.tuple_count += 1;
        }
        trie
    }

    fn config(&self) -> &HashTrieConfig { &self.config }
}
```

(`insert_at` keeps its current signature in this task; Task 3 threads the flag through.)

In `Projectable::project`, replace the final line `HashTrie::<H>::from_tuples(new_header, projected_tuples)` with:

```rust
        HashTrie::<H>::from_tuples_with_config(new_header, self.config, projected_tuples)
```

Replace the `HasOptimizationAxes` impl:

```rust
impl<H: HashStrategy> kermit_iters::HasOptimizationAxes for HashTrie<H> {
    /// The layout axis `ds_layout_hasher` (the strategy's
    /// `LayoutOption::NAME`, e.g. `"sip"` / `"fxhash"`) plus one
    /// `ds_config_<flag>` axis per [`HashTrieConfig`] flag.
    fn optimization_axes(&self) -> std::collections::BTreeMap<String, serde_json::Value> {
        let mut axes = std::collections::BTreeMap::new();
        axes.insert(
            "ds_layout_hasher".to_string(),
            serde_json::Value::String(<H as kermit_iters::LayoutOption>::NAME.to_string()),
        );
        for (suffix, value) in self.config.axes() {
            axes.insert(format!("ds_config_{suffix}"), value);
        }
        axes
    }
}
```

Update the module doc at the top of the file: after the sentence ending `under \`ds_layout_hasher\`.` add:

```rust
//!
//! Runtime flags live in [`HashTrieConfig`] (the Config axis, emitted as
//! `ds_config_<flag>`); they reach the trie through
//! [`ConfigurableRelation`](crate::relation::ConfigurableRelation).
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p kermit-ds`
Expected: all pass, including the three new tests.

- [ ] **Step 6: Commit**

```bash
git add kermit-ds/src/relation.rs kermit-ds/src/lib.rs kermit-ds/src/ds/hash_trie/implementation.rs
git commit -m "feat(ds): add ConfigurableRelation and carry HashTrieConfig on HashTrie"
```

---

### Task 3: `Singleton` node variant and pruning insert

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/node.rs`
- Modify: `kermit-ds/src/ds/hash_trie/implementation.rs`
- Modify: `kermit-ds/src/ds/hash_trie/mod.rs` (dead-code comment)

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `implementation.rs`:

```rust
    // ── Singleton pruning ──────────────────────────────────────────────

    const PRUNE: HashTrieConfig = HashTrieConfig {
        singleton_pruning: true,
    };

    fn pruned(arity: usize, tuples: Vec<Vec<usize>>) -> HashTrie {
        use crate::relation::ConfigurableRelation;
        HashTrie::from_tuples_with_config(arity.into(), PRUNE, tuples)
    }

    /// Walks `node`, asserting the pruning invariant at every inner bucket
    /// and returning the number of tuples stored below `node`.
    ///
    /// Invariant: under pruning a child is `Singleton` iff exactly one
    /// tuple lives below it; without pruning no `Singleton` exists.
    fn check_pruning_invariant(node: &HashTrieNode, prune: bool) -> usize {
        match node {
            | HashTrieNode::Singleton(_) => {
                assert!(prune, "Singleton found with pruning off");
                1
            },
            | HashTrieNode::Leaf(table) => table.iter().map(|(_, chain)| chain.len()).sum(),
            | HashTrieNode::Inner(table) => table
                .iter()
                .map(|(_, child)| {
                    let below = check_pruning_invariant(child, prune);
                    if prune {
                        assert_eq!(
                            matches!(child, HashTrieNode::Singleton(_)),
                            below == 1,
                            "child holding {below} tuple(s) has wrong pruning state"
                        );
                    }
                    below
                })
                .sum(),
        }
    }

    #[test]
    fn pruning_off_never_creates_singletons() {
        let trie: HashTrie =
            HashTrie::from_tuples(3.into(), vec![vec![1, 2, 3], vec![1, 2, 4], vec![5, 6, 7]]);
        assert_eq!(check_pruning_invariant(&trie.root, false), 3);
    }

    #[test]
    fn pruning_on_single_tuple_subtries_are_singletons() {
        // 1 → {2 → {3}}: after the root, everything under hash(1) is one tuple.
        let trie = pruned(3, vec![vec![1, 2, 3], vec![5, 6, 7]]);
        assert_eq!(check_pruning_invariant(&trie.root, true), 2);
        match &trie.root {
            | HashTrieNode::Inner(t) => {
                assert_eq!(t.len(), 2);
                for (_, child) in t.iter() {
                    assert!(matches!(child, HashTrieNode::Singleton(_)));
                }
            },
            | _ => panic!("expected Inner root"),
        }
    }

    #[test]
    fn unprune_when_second_tuple_diverges_one_level_down() {
        // Both share attr 0; diverge at attr 1 → child is Inner with two
        // Singleton grandchildren.
        let trie = pruned(3, vec![vec![1, 2, 3], vec![1, 4, 5]]);
        assert_eq!(check_pruning_invariant(&trie.root, true), 2);
        let mut got = trie.collect_tuples();
        got.sort();
        assert_eq!(got, vec![vec![1, 2, 3], vec![1, 4, 5]]);
    }

    #[test]
    fn unprune_when_second_tuple_shares_hashes_to_the_leaf() {
        // Share attrs 0 and 1; differ at the leaf attr → chain of two distinct
        // tuples under a Leaf table.
        let trie = pruned(3, vec![vec![1, 2, 3], vec![1, 2, 4]]);
        assert_eq!(check_pruning_invariant(&trie.root, true), 2);
        let mut got = trie.collect_tuples();
        got.sort();
        assert_eq!(got, vec![vec![1, 2, 3], vec![1, 2, 4]]);
    }

    #[test]
    fn unprune_on_exact_duplicate_keeps_multiset() {
        let trie = pruned(2, vec![vec![1, 2], vec![1, 2]]);
        assert_eq!(check_pruning_invariant(&trie.root, true), 2);
        let mut got = trie.collect_tuples();
        got.sort();
        assert_eq!(got, vec![vec![1, 2], vec![1, 2]]);
    }

    #[test]
    fn incremental_insert_unprunes_like_bulk_build() {
        use crate::relation::ConfigurableRelation;
        let mut trie: HashTrie = HashTrie::with_config(3.into(), PRUNE);
        trie.insert(vec![1, 2, 3]);
        assert_eq!(check_pruning_invariant(&trie.root, true), 1);
        trie.insert(vec![1, 2, 4]);
        assert_eq!(check_pruning_invariant(&trie.root, true), 2);
        trie.insert(vec![9, 9, 9]);
        assert_eq!(check_pruning_invariant(&trie.root, true), 3);
        assert_eq!(trie.tuple_count, 3);
    }

    #[test]
    fn pruned_shape_is_insertion_order_independent() {
        use crate::heap_size::HeapSize;
        let tuples = vec![vec![1, 2, 3], vec![1, 2, 4], vec![1, 5, 6], vec![7, 8, 9]];
        let forward = pruned(3, tuples.clone());
        let backward = pruned(3, tuples.into_iter().rev().collect());
        assert_eq!(forward.heap_size_bytes(), backward.heap_size_bytes());
        let (mut a, mut b) = (forward.collect_tuples(), backward.collect_tuples());
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    #[test]
    fn pruning_shrinks_heap_size_for_sparse_fanout() {
        use crate::heap_size::HeapSize;
        // Every tuple has a distinct first attribute, so under pruning the
        // root's children are all singletons: no per-level tables at all.
        let tuples: Vec<Vec<usize>> = (0..64).map(|i| vec![i, i + 1000, i + 2000]).collect();
        let plain: HashTrie = HashTrie::from_tuples(3.into(), tuples.clone());
        let compact = pruned(3, tuples);
        assert!(
            compact.heap_size_bytes() < plain.heap_size_bytes(),
            "pruned {} >= plain {}",
            compact.heap_size_bytes(),
            plain.heap_size_bytes()
        );
    }

    #[test]
    fn pruned_and_plain_collect_the_same_tuples() {
        let tuples = vec![vec![1, 2, 3], vec![1, 2, 4], vec![1, 5, 6], vec![7, 8, 9]];
        let plain: HashTrie = HashTrie::from_tuples(3.into(), tuples.clone());
        let compact = pruned(3, tuples);
        let (mut a, mut b) = (plain.collect_tuples(), compact.collect_tuples());
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kermit-ds implementation::tests`
Expected: compile error, no variant `Singleton`.

- [ ] **Step 3: Add the variant to `node.rs`**

Replace the enum and update the module doc:

```rust
//! Recursive node type for the hash trie. Inner levels carry child node
//! tables; the deepest (leaf) level carries tuple chains; a pruned subtrie
//! carries its single tuple directly.
//!
//! The table variant is fixed by depth: inner nodes live at depths
//! `0..arity-1`, the leaf node at depth `arity-1`. A `Singleton` may stand
//! in for either at any depth `1..arity` when singleton pruning is on and
//! exactly one tuple lives below that bucket (SIGMOD 2020 §3.3.1, Figure 5).
//! No runtime check polices the depth rule — `HashTrie::insert_at`
//! constructs the right variant based on the caller's known arity.

use super::hash_table::HashTable;

/// A node in a hash trie.
pub(crate) enum HashTrieNode {
    /// Inner level: hash table whose values are child nodes.
    Inner(HashTable<HashTrieNode>),
    /// Leaf level: hash table whose values are tuple chains. Each chain
    /// holds the full materialized tuples whose attribute hashes match the
    /// path of hashes from the root to this bucket.
    Leaf(HashTable<Vec<Vec<usize>>>),
    /// Pruned subtrie: exactly one tuple lives below this point, so the
    /// remaining levels are not materialised. The iterator emulates them
    /// from the tuple (see `hash_trie_iter.rs`). Never the root.
    Singleton(Vec<usize>),
}
```

Add a `Singleton` arm to each of the five accessors (`buckets_len`, `next_occupied`, `hash_at`, `index_of`, `len`). Each arm is identical:

```rust
            | HashTrieNode::Singleton(_) => Self::singleton_is_not_a_table(),
```

and add this helper inside `impl HashTrieNode`, above the accessors:

```rust
    /// The table accessors below are only meaningful on `Inner` / `Leaf`.
    /// `HashTrieIter` never places a `Singleton` in a table frame, so
    /// reaching this is a broken internal invariant, not a user error.
    fn singleton_is_not_a_table() -> ! {
        panic!("HashTrieNode table accessor called on a Singleton (pruned subtrie)")
    }
```

Update the accessor block comment to mention that the singleton arm panics.

- [ ] **Step 4: Thread the flag through `insert_at` in `implementation.rs`**

Replace `insert_at` with:

```rust
    /// Insert one tuple at the appropriate depth in the trie. Recursive
    /// implementation of Algorithm 2 from the paper, line by line, plus the
    /// singleton-pruning extension of §3.3.1 (Figure 5) when `prune` is on.
    fn insert_at(node: &mut HashTrieNode, depth: usize, arity: usize, tuple: Vec<usize>, prune: bool) {
        let key = tuple[depth];
        let hash = H::hash(key);
        match node {
            | HashTrieNode::Inner(table) => {
                if prune && table.get(hash).is_none() {
                    // Fresh bucket: the subtrie below holds exactly one tuple,
                    // so store the tuple itself instead of one table per
                    // remaining level.
                    table.entry_or_insert_with(hash, || HashTrieNode::Singleton(tuple));
                    return;
                }
                let child_is_leaf = Self::is_leaf_depth(depth + 1, arity);
                let child = table.entry_or_insert_with(hash, || {
                    // The child lives at `depth + 1`; it is the leaf when that
                    // is the last attribute.
                    if child_is_leaf {
                        HashTrieNode::new_leaf()
                    } else {
                        HashTrieNode::new_inner()
                    }
                });
                if matches!(child, HashTrieNode::Singleton(_)) {
                    // Unprune: a second tuple has arrived, so the subtrie no
                    // longer holds exactly one. Expand it back into a table
                    // and re-insert the evicted tuple ahead of the new one;
                    // the recursion re-prunes wherever the two diverge.
                    let table_node = if child_is_leaf {
                        HashTrieNode::new_leaf()
                    } else {
                        HashTrieNode::new_inner()
                    };
                    let HashTrieNode::Singleton(evicted) = std::mem::replace(child, table_node)
                    else {
                        unreachable!("matched Singleton above")
                    };
                    Self::insert_at(child, depth + 1, arity, evicted, prune);
                }
                Self::insert_at(child, depth + 1, arity, tuple, prune);
            },
            | HashTrieNode::Leaf(table) => {
                let chain = table.entry_or_insert_with(hash, Vec::new);
                chain.push(tuple);
            },
            | HashTrieNode::Singleton(_) => {
                unreachable!("insert_at descends through Inner/Leaf only; singletons are unpruned by the parent")
            },
        }
    }
```

Update both call sites (`insert` and `from_tuples_with_config`) to pass the flag:

```rust
        Self::insert_at(&mut self.root, 0, arity, tuple, self.config.singleton_pruning);
```

and in `from_tuples_with_config`:

```rust
            Self::insert_at(&mut trie.root, 0, arity, tuple, config.singleton_pruning);
```

Add the `Singleton` arm to `collect_at`:

```rust
            | HashTrieNode::Singleton(tuple) => out.push(tuple.clone()),
```

Add the `Singleton` arm to `node_heap_bytes`:

```rust
        | HashTrieNode::Singleton(tuple) => tuple.capacity() * std::mem::size_of::<usize>(),
```

`HashTable::get` is now used in production; in `kermit-ds/src/ds/hash_trie/mod.rs` update the comment above `#[allow(dead_code)]` to say only `get_mut` is still reserved.

Update the struct doc's `# Invariants` list in `implementation.rs` with:

```rust
/// - With `config.singleton_pruning` on, a child node is `Singleton` iff
///   exactly one tuple lives below it (order-independent). With it off,
///   no `Singleton` exists and the structure is identical to pre-pruning
///   builds.
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p kermit-ds`
Expected: all pass. (The iterator tests still pass because they never build with pruning on.)

- [ ] **Step 6: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/node.rs kermit-ds/src/ds/hash_trie/implementation.rs kermit-ds/src/ds/hash_trie/mod.rs
git commit -m "feat(ds): singleton pruning at HashTrie build time (paper Fig. 5)"
```

---

### Task 4: Iterator frames and singleton emulation

**Files:**
- Modify: `kermit-ds/src/ds/hash_trie/hash_trie_iter.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `hash_trie_iter.rs`:

```rust
    // ── Singleton emulation ────────────────────────────────────────────

    use crate::{ds::hash_trie::config::HashTrieConfig, relation::ConfigurableRelation};

    const PRUNE: HashTrieConfig = HashTrieConfig {
        singleton_pruning: true,
    };

    fn h(k: usize) -> u64 { <SipHashStrategy as HashStrategy>::hash(k) }

    #[test]
    fn singleton_frames_emulate_one_entry_tables_down_to_the_leaf() {
        let trie: HashTrie = HashTrie::from_tuples_with_config(3.into(), PRUNE, vec![vec![1, 2, 3]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open()); // root table, bucket for hash(1)
        assert_eq!(it.key(), Some(h(1)));
        assert!(it.leaf_tuples().is_none());

        assert!(it.open()); // singleton frame at depth 1
        assert_eq!(it.key(), Some(h(2)));
        assert_eq!(it.size(), 1);
        assert!(!it.at_end());
        assert!(it.leaf_tuples().is_none());

        assert!(it.open()); // singleton frame at depth 2 (leaf depth)
        assert_eq!(it.key(), Some(h(3)));
        assert_eq!(it.leaf_tuples(), Some(&[vec![1, 2, 3]][..]));
        assert!(!it.open()); // no level below the leaf

        assert!(it.up());
        assert_eq!(it.key(), Some(h(2)));
    }

    #[test]
    fn singleton_next_exhausts_and_open_on_exhausted_fails() {
        let trie: HashTrie = HashTrie::from_tuples_with_config(2.into(), PRUNE, vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        it.open();
        assert_eq!(it.next(), None);
        assert!(it.at_end());
        assert!(it.key().is_none());
        assert!(it.leaf_tuples().is_none());
        assert!(!it.open());
    }

    #[test]
    fn singleton_lookup_hit_positions_and_miss_exhausts() {
        let trie: HashTrie = HashTrie::from_tuples_with_config(2.into(), PRUNE, vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        it.open();
        assert!(!it.lookup(h(999)));
        assert!(it.at_end());
        assert!(it.lookup(h(2)));
        assert!(!it.at_end());
        assert_eq!(it.key(), Some(h(2)));
    }

    #[test]
    fn open_from_table_into_singleton_child() {
        // Two tuples diverging at attr 0: root has two singleton children.
        let trie: HashTrie =
            HashTrie::from_tuples_with_config(2.into(), PRUNE, vec![vec![1, 2], vec![3, 4]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open());
        let mut leaves = Vec::new();
        while !it.at_end() {
            assert!(it.open());
            leaves.extend_from_slice(it.leaf_tuples().expect("singleton at leaf depth"));
            it.up();
            it.next();
        }
        leaves.sort();
        assert_eq!(leaves, vec![vec![1, 2], vec![3, 4]]);
    }

    #[test]
    fn pruned_and_plain_iterators_yield_identical_hash_paths() {
        fn walk(node_iter: &mut dyn HashTrieIterator, arity: usize, path: &mut Vec<u64>, out: &mut Vec<Vec<u64>>) {
            while let Some(k) = node_iter.key() {
                path.push(k);
                if path.len() == arity {
                    out.push(path.clone());
                } else {
                    assert!(node_iter.open());
                    walk(node_iter, arity, path, out);
                    node_iter.up();
                }
                path.pop();
                node_iter.next();
            }
        }
        let tuples = vec![vec![1, 2, 3], vec![1, 2, 4], vec![1, 5, 6], vec![7, 8, 9]];
        let plain: HashTrie = HashTrie::from_tuples(3.into(), tuples.clone());
        let compact: HashTrie = HashTrie::from_tuples_with_config(3.into(), PRUNE, tuples);
        let mut a = Vec::new();
        let mut b = Vec::new();
        let mut ia = HashTrieIter::new(&plain);
        let mut ib = HashTrieIter::new(&compact);
        assert!(ia.open());
        assert!(ib.open());
        walk(&mut ia, 3, &mut Vec::new(), &mut a);
        walk(&mut ib, 3, &mut Vec::new(), &mut b);
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kermit-ds hash_trie_iter::tests`
Expected: the four singleton tests fail (`open` returns false on a singleton child, or the table accessors panic on a `Singleton`).

- [ ] **Step 3: Rewrite the iterator**

Replace everything in `hash_trie_iter.rs` above `#[cfg(test)]` with:

```rust
//! `HashTrieIter` — navigates a `HashTrie` via the [`HashTrieIterator`]
//! interface.
//!
//! # Position model
//!
//! `stack` carries one [`Frame`] per opened level, root first. The deepest
//! frame is the iterator's current position. Empty stack = not yet opened
//! (pre-root).
//!
//! A [`Frame::Table`] is `(node, bucket_index)` on an `Inner` / `Leaf`
//! node. A [`Frame::Singleton`] emulates the one-entry table a pruned level
//! would have held (see `node.rs`): its key is `H::hash(tuple[depth])`,
//! computed once when the frame is pushed, and `exhausted` plays the role
//! of the past-end bucket index.
//!
//! On every `open`, we descend either into the root (when the stack is
//! empty) or into the child of the current bucket, then advance to the
//! first occupied bucket within that node. If the node has no occupied
//! buckets, the new frame sits past-end and `at_end` returns true.

use {
    super::{implementation::HashTrie, node::HashTrieNode},
    crate::relation::Relation,
    kermit_iters::{HashStrategy, HashTrieIterator, SipHashStrategy},
};

/// One opened level of the trie.
enum Frame<'a> {
    /// A bucket within an `Inner` or `Leaf` node — never a `Singleton`.
    Table { node: &'a HashTrieNode, idx: usize },
    /// Level `depth` of a pruned subtrie holding `tuple`.
    /// `hash == H::hash(tuple[depth])`.
    Singleton {
        tuple: &'a Vec<usize>,
        depth: usize,
        hash: u64,
        exhausted: bool,
    },
}

impl Frame<'_> {
    fn at_end(&self) -> bool {
        match self {
            | Frame::Table { node, idx } => *idx >= node.buckets_len(),
            | Frame::Singleton { exhausted, .. } => *exhausted,
        }
    }
}

/// What `open` found below the current position.
enum Descent<'a> {
    /// A table node to descend into.
    Node(&'a HashTrieNode),
    /// Stay inside a pruned subtrie: emulate `depth + 1` for `tuple`.
    Deeper { tuple: &'a Vec<usize>, depth: usize },
    /// Nothing below (leaf level, empty bucket, or exhausted).
    Blocked,
}

/// Stack-based iterator over a [`HashTrie`].
///
/// Generic over the same [`HashStrategy`] `H` as the underlying
/// [`HashTrie`]; `H` hashes the emulated keys of pruned levels, so the
/// keys a singleton frame reports agree with the ones a materialised
/// table would have stored.
///
/// See the module docs for the position model.
pub struct HashTrieIter<'a, H: HashStrategy = SipHashStrategy> {
    stack: Vec<Frame<'a>>,
    trie: &'a HashTrie<H>,
}

impl<'a, H: HashStrategy> HashTrieIter<'a, H> {
    /// Construct a fresh iterator positioned before the root.
    pub(crate) fn new(trie: &'a HashTrie<H>) -> Self {
        Self {
            stack: Vec::new(),
            trie,
        }
    }

    fn arity(&self) -> usize { self.trie.header().arity() }

    /// The frame that opening `child` at `depth` produces, positioned on
    /// its first entry (or past-end if it has none).
    fn frame_for(child: &'a HashTrieNode, depth: usize) -> Frame<'a> {
        match child {
            | HashTrieNode::Singleton(tuple) => Self::singleton_frame(tuple, depth),
            | table => Frame::Table {
                node: table,
                idx: table.next_occupied(0),
            },
        }
    }

    fn singleton_frame(tuple: &'a Vec<usize>, depth: usize) -> Frame<'a> {
        Frame::Singleton {
            tuple,
            depth,
            hash: H::hash(tuple[depth]),
            exhausted: false,
        }
    }

    /// Where `open` would go from the current position.
    ///
    /// Matches on `*node` / copies `*tuple` so the returned references carry
    /// the trie lifetime `'a`, not the shorter borrow of `self.stack`.
    fn descent(&self) -> Descent<'a> {
        match self.stack.last() {
            | None => Descent::Node(self.trie.root()),
            | Some(Frame::Table { node, idx }) => match *node {
                | HashTrieNode::Inner(t) => match t.value_at(*idx) {
                    | Some(child) => Descent::Node(child),
                    | None => Descent::Blocked, // current bucket empty / past-end
                },
                | HashTrieNode::Leaf(_) | HashTrieNode::Singleton(_) => Descent::Blocked,
            },
            | Some(Frame::Singleton {
                tuple,
                depth,
                exhausted,
                ..
            }) => {
                if *exhausted || depth + 1 >= self.arity() {
                    Descent::Blocked
                } else {
                    Descent::Deeper {
                        tuple: *tuple,
                        depth: *depth,
                    }
                }
            },
        }
    }
}

impl<H: HashStrategy> HashTrieIterator for HashTrieIter<'_, H> {
    fn key(&self) -> Option<u64> {
        match self.stack.last()? {
            | Frame::Table { node, idx } => node.hash_at(*idx),
            | Frame::Singleton { hash, exhausted, .. } => (!exhausted).then_some(*hash),
        }
    }

    fn next(&mut self) -> Option<u64> {
        match self.stack.last_mut()? {
            | Frame::Table { node, idx } => {
                // Start from idx + 1 (paper's "advance"), find next occupied
                // or past-end.
                *idx = node.next_occupied(*idx + 1);
                if *idx >= node.buckets_len() {
                    return None;
                }
                node.hash_at(*idx)
            },
            | Frame::Singleton { exhausted, .. } => {
                *exhausted = true;
                None
            },
        }
    }

    fn lookup(&mut self, hash: u64) -> bool {
        let Some(frame) = self.stack.last_mut() else {
            return false;
        };
        match frame {
            | Frame::Table { node, idx } => match node.index_of(hash) {
                | Some(i) => {
                    *idx = i;
                    true
                },
                | None => {
                    // Move to past-end; the caller's loop should exit.
                    *idx = node.buckets_len();
                    false
                },
            },
            | Frame::Singleton {
                hash: own,
                exhausted,
                ..
            } => {
                *exhausted = hash != *own;
                !*exhausted
            },
        }
    }

    fn size(&self) -> usize {
        match self.stack.last() {
            | Some(Frame::Table { node, .. }) => node.len(),
            | Some(Frame::Singleton { .. }) => 1,
            | None => 0,
        }
    }

    fn at_end(&self) -> bool { self.stack.last().is_none_or(Frame::at_end) }

    fn open(&mut self) -> bool {
        // No `at_end` guard (unlike `ColumnTrieIter::open`): the stack top
        // holds a resolved node, so a past-end bucket yields `Blocked` and
        // open returns false, with no offset arithmetic to overshoot.
        let depth = self.stack.len();
        let frame = match self.descent() {
            | Descent::Node(child) => Self::frame_for(child, depth),
            | Descent::Deeper { tuple, depth: d } => Self::singleton_frame(tuple, d + 1),
            | Descent::Blocked => return false,
        };
        let opened = !frame.at_end();
        self.stack.push(frame);
        opened
    }

    fn up(&mut self) -> bool { self.stack.pop().is_some() }

    fn leaf_tuples(&self) -> Option<&[Vec<usize>]> {
        match self.stack.last()? {
            | Frame::Table { node, idx } => match *node {
                | HashTrieNode::Leaf(t) => t.value_at(*idx).map(|v| v.as_slice()),
                | HashTrieNode::Inner(_) | HashTrieNode::Singleton(_) => None,
            },
            | Frame::Singleton {
                tuple,
                depth,
                exhausted,
                ..
            } => (!exhausted && depth + 1 == self.arity()).then(|| std::slice::from_ref(*tuple)),
        }
    }
}
```

Note: `Option::is_none_or` is stable since Rust 1.82; the workspace is on nightly so it is available. If clippy suggests otherwise, write `self.stack.last().map_or(true, Frame::at_end)`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kermit-ds`
Expected: all pass, including the existing iterator tests and the five new ones.

- [ ] **Step 5: Run clippy on the crate**

Run: `cargo clippy -p kermit-ds --all-targets -- -Dwarnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add kermit-ds/src/ds/hash_trie/hash_trie_iter.rs
git commit -m "feat(ds): emulate pruned levels in HashTrieIter with singleton frames"
```

---

### Task 5: `Configured<R, P>` and `ConfigProvider`

**Files:**
- Create: `kermit-ds/src/configured.rs`
- Modify: `kermit-ds/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `kermit-ds/src/configured.rs` with the test module only:

```rust
//! [`Configured`]: lifts a runtime [`ConfigurableRelation::Config`] value to
//! the type level.
//!
//! Test suites in this workspace are macro-generated and name a relation by
//! a single type identifier (`define_multiway_join_test_suite!(HashTrieSip, …)`).
//! A Config value has no type-level identity, so `Configured<R, P>` pairs a
//! relation `R` with a zero-sized marker `P: ConfigProvider<R::Config>` that
//! supplies the value. The wrapper delegates every trait to `R`; only the
//! two constructors differ, routing through `P::config()`.
//!
//! This is test scaffolding shipped in the library so that `kermit-ds` and
//! `kermit` integration tests share one definition. The data structure
//! itself never sees the marker — its config stays a runtime value, which
//! is what makes it Config rather than Layout.

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            ds::{HashTrie, HashTrieConfig},
            relation::{ConfigurableRelation, Projectable, Relation},
        },
        kermit_iters::{HasOptimizationAxes, HashTrieIterable, HashTrieIterator, SipHashStrategy},
    };

    crate::define_config_provider!(PruningOn, HashTrieConfig, HashTrieConfig {
        singleton_pruning: true,
    });

    type Pruned = Configured<HashTrie<SipHashStrategy>, PruningOn>;

    #[test]
    fn constructors_inject_the_provider_config() {
        let a = Pruned::new(2.into());
        assert!(a.config().singleton_pruning);
        let b = Pruned::from_tuples(2.into(), vec![vec![1, 2]]);
        assert!(b.config().singleton_pruning);
        assert_eq!(b.collect_tuples(), vec![vec![1, 2]]);
    }

    #[test]
    fn delegated_traits_reach_the_inner_relation() {
        let r = Pruned::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(r.header().arity(), 2);
        assert_eq!(crate::Cardinality::tuple_count(&r), 2);
        assert!(crate::HeapSize::heap_size_bytes(&r) > 0);
        let mut it = r.hash_trie_iter();
        assert!(it.open());
        assert_eq!(it.size(), 2);
        assert_eq!(
            r.optimization_axes().get("ds_config_singleton_pruning"),
            Some(&serde_json::Value::Bool(true))
        );
        let p = r.project(vec![0]);
        assert!(p.config().singleton_pruning);
    }
}
```

Register in `kermit-ds/src/lib.rs`: add `mod configured;` after `mod cardinality;` and add to the `pub use` block:

```rust
    configured::{ConfigProvider, Configured},
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kermit-ds configured::tests`
Expected: compile errors (`Configured`, `define_config_provider!` missing).

- [ ] **Step 3: Write the implementation**

Insert above `#[cfg(test)]` in `configured.rs`:

```rust
use {
    crate::{
        cardinality::Cardinality,
        heap_size::HeapSize,
        relation::{ConfigurableRelation, Projectable, Relation, RelationHeader},
    },
    kermit_iters::{
        HasOptimizationAxes, HashTrieIterable, HashTrieIterator, JoinIterable, TrieIterable,
        TrieIterator,
    },
    serde_json::Value,
    std::{collections::BTreeMap, marker::PhantomData, ops::Deref},
};

/// A zero-sized marker that names one configuration value.
pub trait ConfigProvider<C> {
    /// The configuration this marker stands for.
    fn config() -> C;
}

/// Declares a [`ConfigProvider`] marker type.
///
/// ```ignore
/// define_config_provider!(PruningOn, HashTrieConfig, HashTrieConfig { singleton_pruning: true });
/// type HashTrieSipPruned = Configured<HashTrie<SipHashStrategy>, PruningOn>;
/// ```
#[macro_export]
macro_rules! define_config_provider {
    ($name:ident, $config:ty, $value:expr) => {
        #[derive(Copy, Clone, Debug, Default)]
        pub struct $name;

        impl $crate::ConfigProvider<$config> for $name {
            fn config() -> $config { $value }
        }
    };
}

/// `R` built with the configuration `P::config()`.
///
/// Derefs to `R`, so inherent methods (`HashTrie::collect_tuples`) are
/// reachable; the trait impls below forward to `R`'s.
pub struct Configured<R, P> {
    inner: R,
    _provider: PhantomData<P>,
}

impl<R, P> Configured<R, P> {
    fn wrap(inner: R) -> Self {
        Self {
            inner,
            _provider: PhantomData,
        }
    }

    /// Unwraps the configured relation.
    pub fn into_inner(self) -> R { self.inner }
}

impl<R, P> Deref for Configured<R, P> {
    type Target = R;

    fn deref(&self) -> &R { &self.inner }
}

impl<R: JoinIterable, P> JoinIterable for Configured<R, P> {}

impl<R: Projectable, P> Projectable for Configured<R, P> {
    fn project(&self, columns: Vec<usize>) -> Self { Self::wrap(self.inner.project(columns)) }
}

impl<R, P> Relation for Configured<R, P>
where
    R: ConfigurableRelation,
    P: ConfigProvider<R::Config>,
{
    fn header(&self) -> &RelationHeader { self.inner.header() }

    fn new(header: RelationHeader) -> Self { Self::wrap(R::with_config(header, P::config())) }

    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        Self::wrap(R::from_tuples_with_config(header, P::config(), tuples))
    }

    fn insert(&mut self, tuple: Vec<usize>) { self.inner.insert(tuple) }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) { self.inner.insert_all(tuples) }
}

impl<R, P> ConfigurableRelation for Configured<R, P>
where
    R: ConfigurableRelation,
    P: ConfigProvider<R::Config>,
{
    type Config = R::Config;

    fn with_config(header: RelationHeader, config: R::Config) -> Self {
        Self::wrap(R::with_config(header, config))
    }

    fn from_tuples_with_config(
        header: RelationHeader, config: R::Config, tuples: Vec<Vec<usize>>,
    ) -> Self {
        Self::wrap(R::from_tuples_with_config(header, config, tuples))
    }

    fn config(&self) -> &R::Config { self.inner.config() }
}

impl<R: HeapSize, P> HeapSize for Configured<R, P> {
    fn heap_size_bytes(&self) -> usize { self.inner.heap_size_bytes() }
}

impl<R: Cardinality, P> Cardinality for Configured<R, P> {
    fn tuple_count(&self) -> usize { self.inner.tuple_count() }
}

impl<R: HashTrieIterable, P> HashTrieIterable for Configured<R, P> {
    fn hash_trie_iter(&self) -> impl HashTrieIterator { self.inner.hash_trie_iter() }
}

impl<R: TrieIterable, P> TrieIterable for Configured<R, P> {
    fn trie_iter(&self) -> impl TrieIterator + IntoIterator<Item = Vec<usize>> {
        self.inner.trie_iter()
    }
}

impl<R: HasOptimizationAxes, P> HasOptimizationAxes for Configured<R, P> {
    fn optimization_axes(&self) -> BTreeMap<String, Value> { self.inner.optimization_axes() }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kermit-ds configured::tests`
Expected: 2 passed.

- [ ] **Step 5: Run clippy and doc**

Run: `cargo clippy -p kermit-ds --all-targets -- -Dwarnings && RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-ds --no-deps`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add kermit-ds/src/configured.rs kermit-ds/src/lib.rs
git commit -m "feat(ds): add Configured<R, P> to lift a Config value to a type for test suites"
```

---

### Task 6: DS-level suites on pruned aliases

**Files:**
- Modify: `kermit-ds/tests/hash_trie_tests.rs`

- [ ] **Step 1: Add the pruned aliases and suite invocations**

Change the imports at the top of `hash_trie_tests.rs` to:

```rust
use {
    kermit_ds::{define_config_provider, Configured, HashTrie, HashTrieConfig},
    kermit_iters::{FxHashStrategy, HashStrategy, LayoutOption, SipHashStrategy},
};
```

After `type HashTrieMod10 = HashTrie<Mod10HashStrategy>;` add:

```rust
// ── Config variant: singleton pruning on ────────────────────────────────
//
// Each Layout alias also runs under the one Config flag, so the iterator
// contract holds on emulated (pruned) levels as well as materialised ones.
// `Mod10` is the important case: full-collision tuples must unprune into a
// shared leaf chain.
define_config_provider!(PruningOn, HashTrieConfig, HashTrieConfig {
    singleton_pruning: true,
});

type HashTrieSipPruned = Configured<HashTrieSip, PruningOn>;
type HashTrieFxPruned = Configured<HashTrieFx, PruningOn>;
type HashTrieMod10Pruned = Configured<HashTrieMod10, PruningOn>;
```

After the three existing `hash_trie_test_suite!` invocations add:

```rust
hash_trie_test_suite!(HashTrieSipPruned, SipHashStrategy);

hash_trie_test_suite!(HashTrieFxPruned, FxHashStrategy);

hash_trie_test_suite!(HashTrieMod10Pruned, Mod10HashStrategy);
```

The Parquet suites live in `kermit-ds/tests/parquet_tests.rs` (lines 24-26 invoke `parquet_test_suite!(HashTrieSip, sorted_tuples)` and `(HashTrieFx, sorted_tuples)`). In that file, add the same provider and aliases (`define_config_provider!(PruningOn, …)`, `type HashTrieSipPruned = Configured<HashTrieSip, PruningOn>;`, `type HashTrieFxPruned = …`) with the matching imports, then add:

```rust
parquet_test_suite!(HashTrieSipPruned, sorted_tuples);

parquet_test_suite!(HashTrieFxPruned, sorted_tuples);
```

`sorted_tuples` is the existing collector fn in that file; because `Configured` derefs to `HashTrie`, `relation.collect_tuples()` resolves. If the collector is typed on `&HashTrieSip` rather than generic, add a second collector `fn sorted_tuples_pruned(relation: &HashTrieSipPruned) -> Vec<Vec<usize>>` with the same body (and one for `Fx`).

- [ ] **Step 2: Add the collision unprune test**

Inside `mod hash_trie_collisions`, append:

```rust
    /// Under pruning, two tuples that collide on every attribute start as
    /// one `Singleton` and must unprune into a shared leaf chain of two —
    /// the same shape the unpruned trie builds, so `verify_and_construct`
    /// still sees both candidates.
    #[test]
    fn full_collision_unprunes_into_shared_leaf_chain() {
        let trie = HashTrieMod10Pruned::from_tuples(2.into(), vec![vec![1, 2], vec![11, 12]]);
        let mut it = trie.hash_trie_iter();
        assert!(it.open());
        assert_eq!(it.size(), 1, "both tuples share hash(1) == hash(11)");
        assert!(it.open());
        assert_eq!(it.size(), 1, "both tuples share hash(2) == hash(12)");
        let mut chain = it.leaf_tuples().expect("leaf chain").to_vec();
        chain.sort();
        assert_eq!(chain, vec![vec![1, 2], vec![11, 12]]);
    }
```

- [ ] **Step 3: Run the suites**

Run: `cargo test -p kermit-ds --test hash_trie_tests --test parquet_tests`
Expected: all pass. The pruned `Mod10` suite is the most likely to expose an emulation bug; if a traversal test fails there, the bug is in Task 4's `descent()` or `lookup`, not in the test.

- [ ] **Step 4: Commit**

```bash
git add kermit-ds/tests/hash_trie_tests.rs kermit-ds/tests/parquet_tests.rs
git commit -m "test(ds): run HashTrie iterator, Parquet and collision suites with pruning on"
```

---

### Task 7: Join-level `with_config` macro and invocations

**Files:**
- Modify: `kermit/tests/common/macros.rs` (append after `define_multiway_join_test_suite!`)
- Modify: `kermit/tests/join_tests.rs`

- [ ] **Step 1: Add the macro**

Append to `kermit/tests/common/macros.rs`:

```rust
/// The Config-axis counterpart of [`define_multiway_join_test_suite!`]
/// prescribed by `docs/specs/optimization-standard.md`.
///
/// Declares `type <Relation><Provider> = Configured<Relation, Provider>;`
/// and runs the 11 standard join patterns against it. `Provider` is a
/// marker declared with `kermit_ds::define_config_provider!`.
///
/// ```ignore
/// define_config_provider!(PruningOn, HashTrieConfig, HashTrieConfig { singleton_pruning: true });
/// define_multiway_join_test_suite_with_config!(HashTrieSip, HashTriejoin, LexicographicOptimiser, PruningOn);
/// // → tests named e.g. `triangle_hashtriesippruningon_hashtriejoin_lexicographicoptimiser`
/// ```
#[macro_export]
macro_rules! define_multiway_join_test_suite_with_config {
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
                // Each invocation gets its own module so the alias can be
                // declared once per (relation, algorithm, optimiser, provider)
                // without colliding with a sibling invocation's alias.
                mod [<with_config_ $relation_type:lower _ $join_algorithm:lower _ $optimiser:lower _ $provider:lower>] {
                    use super::*;

                    type [<$relation_type $provider>] =
                        kermit_ds::Configured<$relation_type, $provider>;

                    $crate::define_multiway_join_test_suite!(
                        [<$relation_type $provider>], $join_algorithm, $optimiser
                    );
                }
            }
        )+
    };
}
```

`$join_algorithm` and `$optimiser` are `ident` (not `ty`) so `paste!` can lower-case them into the module name; every existing invocation passes bare identifiers, so this is not a restriction in practice.

- [ ] **Step 2: Invoke it in `join_tests.rs`**

Replace the file with:

```rust
mod common;

use {
    kermit_algos::{CardinalityOptimiser, HashTriejoin, LeapfrogTriejoin, LexicographicOptimiser},
    kermit_ds::{define_config_provider, ColumnTrie, HashTrie, HashTrieConfig, TreeTrie},
    kermit_iters::{FxHashStrategy, SipHashStrategy},
};

type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;

define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin, CardinalityOptimiser);

define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin, CardinalityOptimiser);

define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, CardinalityOptimiser);

define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, CardinalityOptimiser);

// ── Config axis: singleton pruning ──────────────────────────────────────
// The four invocations above are the `ds_config_singleton_pruning: false`
// baseline; these four are the alternate, per the standard's Config test
// obligation (baseline + ≥1 alternate per flag).
define_config_provider!(PruningOn, HashTrieConfig, HashTrieConfig {
    singleton_pruning: true,
});

define_multiway_join_test_suite_with_config!(
    HashTrieSip, HashTriejoin, LexicographicOptimiser, PruningOn
);
define_multiway_join_test_suite_with_config!(
    HashTrieSip, HashTriejoin, CardinalityOptimiser, PruningOn
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx, HashTriejoin, LexicographicOptimiser, PruningOn
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx, HashTriejoin, CardinalityOptimiser, PruningOn
);
```

- [ ] **Step 3: Run the join suites**

Run: `cargo test -p kermit --test join_tests`
Expected: 44 new tests pass (11 patterns × 4) alongside the existing 88.

- [ ] **Step 4: Commit**

```bash
git add kermit/tests/common/macros.rs kermit/tests/join_tests.rs
git commit -m "test(kermit): add define_multiway_join_test_suite_with_config! and pruning-on suites"
```

---

### Task 8: File readers returning tuples

**Files:**
- Modify: `kermit-ds/src/relation.rs:320-430`

- [ ] **Step 1: Write the failing test**

Append to the `tests` module in `relation.rs`:

```rust
    #[test]
    fn read_csv_returns_header_and_tuples() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.csv");
        std::fs::write(&path, "a,b\n1,2\n3,4\n").unwrap();
        let (header, tuples) = read_csv(&path).unwrap();
        assert_eq!(header.name(), "edge");
        assert_eq!(header.arity(), 2);
        assert_eq!(tuples, vec![vec![1, 2], vec![3, 4]]);
    }
```

`kermit-ds/Cargo.toml` has no `tempfile` dev-dependency yet; add it:

```toml
[dev-dependencies]
tempfile = "3"
```

(merge into the existing `[dev-dependencies]` table if one is present).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kermit-ds read_csv_returns`
Expected: `read_csv` not found.

- [ ] **Step 3: Factor the readers**

Replace the blanket impl with two free functions plus a thin impl:

```rust
/// Reads a CSV file into a header (attribute names from the header row,
/// relation name from the file stem) and its tuples. Shared by
/// [`RelationFileExt::from_csv`] and by callers that need to build with a
/// non-default configuration
/// ([`ConfigurableRelation::from_tuples_with_config`]).
///
/// # Errors
///
/// Same conditions as [`RelationFileExt::from_csv`].
pub fn read_csv<P: AsRef<Path>>(
    filepath: P,
) -> Result<(RelationHeader, Vec<Vec<usize>>), RelationError> {
    let path = filepath.as_ref();
    let file = File::open(path)?;

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .delimiter(b',')
        .double_quote(false)
        .escape(Some(b'\\'))
        .flexible(false)
        .comment(Some(b'#'))
        .from_reader(file);

    let attrs: Vec<String> = rdr.headers()?.iter().map(|s| s.to_string()).collect();
    let header = RelationHeader::new(file_stem(path), attrs);

    let mut tuples = Vec::new();
    for (row_idx, result) in rdr.records().enumerate() {
        let record = result?;
        let mut tuple: Vec<usize> = Vec::with_capacity(record.len());
        for (col_idx, field) in record.iter().enumerate() {
            let value = field.parse::<usize>().map_err(|_| {
                RelationError::InvalidData(format!(
                    "row {row_idx}, column {col_idx}: cannot parse {:?} as usize",
                    field,
                ))
            })?;
            tuple.push(value);
        }
        tuples.push(tuple);
    }
    Ok((header, tuples))
}

/// Reads a Parquet file into a header (column names from the schema,
/// relation name from the file stem) and its tuples. Counterpart of
/// [`read_csv`].
///
/// # Errors
///
/// Same conditions as [`RelationFileExt::from_parquet`].
pub fn read_parquet<P: AsRef<Path>>(
    filepath: P,
) -> Result<(RelationHeader, Vec<Vec<usize>>), RelationError> {
    let path = filepath.as_ref();
    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let attrs: Vec<String> = builder
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .collect();
    let header = RelationHeader::new(file_stem(path), attrs);
    let reader = builder.build()?;

    let mut tuples = Vec::new();
    for batch_result in reader {
        let batch = batch_result?;
        let num_rows = batch.num_rows();
        let num_cols = batch.num_columns();
        for row_idx in 0..num_rows {
            let mut tuple: Vec<usize> = Vec::with_capacity(num_cols);
            for col_idx in 0..num_cols {
                let column = batch.column(col_idx);
                let int_array = column.as_primitive::<arrow::datatypes::Int64Type>();
                if let Ok(value) = usize::try_from(int_array.value(row_idx)) {
                    tuple.push(value);
                } else {
                    return Err(RelationError::InvalidData(
                        "failed to convert Parquet value to usize".into(),
                    ));
                }
            }
            tuples.push(tuple);
        }
    }
    Ok((header, tuples))
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Blanket implementation of `RelationFileExt` for any type that
/// implements `Relation`.
impl<R> RelationFileExt for R
where
    R: Relation,
{
    fn from_csv<P: AsRef<Path>>(filepath: P) -> Result<Self, RelationError> {
        let (header, tuples) = read_csv(filepath)?;
        Ok(R::from_tuples(header, tuples))
    }

    fn from_parquet<P: AsRef<Path>>(filepath: P) -> Result<Self, RelationError> {
        let (header, tuples) = read_parquet(filepath)?;
        Ok(R::from_tuples(header, tuples))
    }
}
```

Keep the existing `use` lines for `File`, `Path`, `ParquetRecordBatchReaderBuilder`, `arrow` as they are. Export from `kermit-ds/src/lib.rs`:

```rust
    relation::{
        read_csv, read_parquet, ConfigurableRelation, ModelType, Projectable, Relation,
        RelationError, RelationFileExt, RelationHeader,
    },
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kermit-ds`
Expected: all pass (including the existing `from_csv_*` / `from_parquet_*` tests and the parquet suites in `tests/`).

- [ ] **Step 5: Commit**

```bash
git add kermit-ds/src/relation.rs kermit-ds/src/lib.rs
git commit -m "refactor(ds): expose read_csv/read_parquet so callers can build with a config"
```

---

### Task 9: `Execution` carries the config

**Files:**
- Modify: `kermit/src/execution.rs`
- Modify: `kermit/src/main.rs` (only the three `Execution::HashHtj` / `HashHtj::new` call sites and `resolve_sweep`; the CLI flag itself is Task 10)

- [ ] **Step 1: Update the execution tests**

In the `tests` module of `execution.rs`, update the two assertions that name `Execution::HashHtj(HasherChoice::Fxhash)`:

```rust
        assert_eq!(
            Execution::for_pair(
                IndexStructure::HashTrie,
                JoinAlgorithm::HashTriejoin,
                HasherChoice::Fxhash,
                HashTrieConfig::default(),
            ),
            Some(Execution::HashHtj {
                hasher: HasherChoice::Fxhash,
                config: HashTrieConfig::default(),
            })
        );
```

and in `families_report_their_own_execution`:

```rust
        let config = HashTrieConfig {
            singleton_pruning: true,
        };
        let hash = HashHtj::<kermit_iters::FxHashStrategy>::new(
            HasherChoice::Fxhash,
            config,
            Optimiser::Lexicographic,
        );
        assert_eq!(
            hash.execution(),
            Execution::HashHtj {
                hasher: HasherChoice::Fxhash,
                config,
            }
        );
```

Also update the other `Execution::for_pair(ds, algo, HasherChoice::Fxhash)` call in the cross-product test to pass `HashTrieConfig::default()` as a fourth argument. Add a new test:

```rust
    /// The config reaches the relations the family builds, so the report's
    /// `ds_config_*` axes describe the structure that actually ran.
    #[test]
    fn hash_family_builds_relations_with_its_config() {
        let config = HashTrieConfig {
            singleton_pruning: true,
        };
        let family = HashHtj::<kermit_iters::SipHashStrategy>::new(
            HasherChoice::Sip,
            config,
            Optimiser::Lexicographic,
        );
        let header = RelationHeader::new("r", vec!["a".to_string(), "b".to_string()]);
        let engine = family.build_from_tuples(vec![(header, vec![vec![1, 2]])]);
        let rel = &engine["r"];
        assert_eq!(
            HashHtj::<kermit_iters::SipHashStrategy>::optimization_axes(rel)
                .get("ds_config_singleton_pruning"),
            Some(&serde_json::Value::Bool(true))
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kermit --bin kermit execution::tests`
Expected: compile errors.

- [ ] **Step 3: Update `execution.rs`**

Add `HashTrieConfig` and `ConfigurableRelation` to the `kermit_ds` import list. Change the enum variant and its docs:

```rust
    /// `HashTrie<H>` joined by Hash Triejoin through [`hash_join`], with
    /// `H` chosen by `--ds-layout-hasher` and the runtime flags by
    /// `--ds-config`.
    HashHtj {
        hasher: HasherChoice,
        config: HashTrieConfig,
    },
```

`for_pair` gains a `config: HashTrieConfig` parameter and returns `Some(Execution::HashHtj { hasher, config })`. `index_structure` / `algorithm` match on `Self::HashHtj { .. }`. `Sweep::expand` gains `config: HashTrieConfig` and forwards it to `for_pair`; update its doc: "`hasher` and `config` are attached to every `HashTrie` cell."

`HashHtj<H>`:

```rust
pub struct HashHtj<H> {
    hasher: HasherChoice,
    config: HashTrieConfig,
    optimiser: Box<dyn kermit_algos::QueryOptimiser>,
    _strategy: PhantomData<H>,
}

impl<H> HashHtj<H> {
    /// Creates the family for the `--ds-layout-hasher` choice `hasher`
    /// (which must be the choice `H` was monomorphised from) and the
    /// `--ds-config` flags `config`, planned by `optimiser`.
    pub fn new(hasher: HasherChoice, config: HashTrieConfig, optimiser: Optimiser) -> Self {
        Self {
            hasher,
            config,
            optimiser: optimiser.instantiate(),
            _strategy: PhantomData,
        }
    }
}
```

In the `ExecutionFamily` impl:

```rust
    fn execution(&self) -> Execution {
        Execution::HashHtj {
            hasher: self.hasher,
            config: self.config,
        }
    }
```

and `build_from_tuples` uses `HashTrie::<H>::from_tuples_with_config(header, self.config, tuples)`.

`build(&self, relations: Vec<HashTrie<H>>)` receives relations loaded from files by the generic runner via `RelationFileExt` (default config). Check `main.rs::run_benchmark` for where `F::Rel::from_csv` / `from_parquet` are called (search `from_csv`). Add to the `ExecutionFamily` trait:

```rust
    /// Loads one relation file into `Self::Rel`, honouring the family's
    /// configuration. The default is the plain `RelationFileExt` path.
    fn load(&self, path: &Path) -> anyhow::Result<Self::Rel> {
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        match extension.to_lowercase().as_str() {
            | "csv" => Self::Rel::from_csv(path).map_err(|e| anyhow::anyhow!("Failed to load relation: {e}")),
            | "parquet" => Self::Rel::from_parquet(path).map_err(|e| anyhow::anyhow!("Failed to load relation: {e}")),
            | _ => anyhow::bail!("Unsupported file extension: {extension}"),
        }
    }
```

and override it in `HashHtj<H>`:

```rust
    fn load(&self, path: &Path) -> anyhow::Result<HashTrie<H>> {
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let (header, tuples) = match extension.to_lowercase().as_str() {
            | "csv" => kermit_ds::read_csv(path),
            | "parquet" => kermit_ds::read_parquet(path),
            | _ => anyhow::bail!("Unsupported file extension: {extension}"),
        }
        .map_err(|e| anyhow::anyhow!("Failed to load relation: {e}"))?;
        Ok(HashTrie::<H>::from_tuples_with_config(header, self.config, tuples))
    }
```

The runner currently loads through the free fn `load_relation_file::<F::Rel>(path)` at `kermit/src/main.rs:978`. In `run_benchmark`, replace each `load_relation_file::<F::Rel>(&path)?` call with `family.load(&path)?`. The default `load` above reproduces `load_relation_file`'s behaviour, so move that function's body into the trait default and delete the free fn (or keep it and have the default call it; either is fine, but do not leave two copies of the extension match). Add `use std::path::Path;` to the `execution.rs` imports.

- [ ] **Step 4: Update the three `main.rs` call sites**

In `dispatch_run_bench`:

```rust
        | Execution::HashHtj {
            hasher: hasher @ HasherChoice::Sip,
            config,
        } => run_benchmark(
            &HashHtj::<SipHashStrategy>::new(hasher, config, optimiser),
            benchmark,
            optimiser,
            metrics,
            queries_per_build,
            query_filter,
            bench_args,
        ),
        | Execution::HashHtj {
            hasher: hasher @ HasherChoice::Fxhash,
            config,
        } => run_benchmark(
            &HashHtj::<FxHashStrategy>::new(hasher, config, optimiser),
            benchmark,
            optimiser,
            metrics,
            queries_per_build,
            query_filter,
            bench_args,
        ),
```

`resolve_sweep` gains `config: HashTrieConfig` and passes it to `Sweep::expand`. `run_bench_run_command` passes `HashTrieConfig::default()` for now (Task 10 replaces it with the parsed flag). Import `kermit_ds::HashTrieConfig` in `main.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p kermit`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add kermit/src/execution.rs kermit/src/main.rs
git commit -m "feat(kermit): carry HashTrieConfig on the HashHtj execution cell"
```

---

### Task 10: `--ds-config` CLI flag

**Files:**
- Modify: `kermit/src/main.rs`

- [ ] **Step 1: Write the failing unit tests**

Append to the `tests` module at the bottom of `main.rs` (next to the `validate_layout_choices_*` tests):

```rust
    #[test]
    fn config_choices_parse_singleton_pruning() {
        let on = ConfigChoices {
            ds_config: vec!["singleton-pruning=true".into()],
        };
        assert_eq!(
            on.hash_trie_config_resolved().unwrap(),
            HashTrieConfig {
                singleton_pruning: true
            }
        );
        let off = ConfigChoices {
            ds_config: vec!["singleton-pruning=false".into()],
        };
        assert_eq!(
            off.hash_trie_config_resolved().unwrap(),
            HashTrieConfig::default()
        );
        assert_eq!(
            ConfigChoices::default().hash_trie_config_resolved().unwrap(),
            HashTrieConfig::default()
        );
    }

    #[test]
    fn config_choices_reject_unknown_key_and_bad_value() {
        let unknown = ConfigChoices {
            ds_config: vec!["lazy-expansion=true".into()],
        };
        let msg = unknown.hash_trie_config_resolved().unwrap_err().to_string();
        assert!(msg.contains("lazy-expansion"), "{msg}");
        assert!(msg.contains("singleton-pruning"), "should list accepted keys: {msg}");

        let bad = ConfigChoices {
            ds_config: vec!["singleton-pruning=yes".into()],
        };
        let msg = bad.hash_trie_config_resolved().unwrap_err().to_string();
        assert!(msg.contains("yes"), "{msg}");

        let malformed = ConfigChoices {
            ds_config: vec!["singleton-pruning".into()],
        };
        assert!(malformed.hash_trie_config_resolved().is_err());
    }

    #[test]
    fn validate_config_choices_rejects_flag_on_non_hash_trie() {
        let config = ConfigChoices {
            ds_config: vec!["singleton-pruning=true".into()],
        };
        assert!(validate_config_choices(IndexStructureSelector::HashTrie, &config).is_ok());
        assert!(validate_config_choices(IndexStructureSelector::All, &config).is_ok());
        for sel in [
            IndexStructureSelector::TreeTrie,
            IndexStructureSelector::ColumnTrie,
        ] {
            let msg = validate_config_choices(sel, &config)
                .unwrap_err()
                .to_string();
            assert!(msg.contains("--ds-config"), "{msg}");
        }
        assert!(validate_config_choices(
            IndexStructureSelector::TreeTrie,
            &ConfigChoices::default()
        )
        .is_ok());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kermit --bin kermit config_choices`
Expected: compile error, `ConfigChoices` missing.

- [ ] **Step 3: Add `ConfigChoices`**

Insert after `validate_layout_choices` in `main.rs`:

```rust
/// Config-axis CLI choices, flattened beside [`LayoutChoices`] into `bench
/// ds` and `bench run`. One flag, `--ds-config`, takes comma-separated
/// `key=value` pairs (the shape prescribed by
/// `docs/specs/optimization-standard.md`); the keys are resolved per index
/// structure by [`hash_trie_config_resolved`](Self::hash_trie_config_resolved).
#[derive(Args, Clone, Debug, Default)]
struct ConfigChoices {
    /// Runtime flags for the selected index structure, as `key=value`
    /// pairs. `HashTrie` accepts `singleton-pruning=true|false`. Only valid
    /// with `--indexstructure hash-trie` (or `all`).
    #[arg(long = "ds-config", value_name = "KEY=VALUE,...", value_delimiter = ',')]
    ds_config: Vec<String>,
}

impl ConfigChoices {
    const HASH_TRIE_KEYS: &'static [&'static str] = &["singleton-pruning"];

    /// Whether the user passed any `--ds-config` pair.
    fn explicit(&self) -> bool { !self.ds_config.is_empty() }

    /// Resolves the pairs into a [`HashTrieConfig`], starting from the
    /// default. Unknown keys and malformed values are usage errors.
    fn hash_trie_config_resolved(&self) -> anyhow::Result<HashTrieConfig> {
        let mut config = HashTrieConfig::default();
        for pair in &self.ds_config {
            let (key, value) = pair.split_once('=').ok_or_else(|| {
                anyhow::anyhow!("--ds-config expects key=value pairs; got {pair:?}")
            })?;
            match key {
                | "singleton-pruning" => {
                    config.singleton_pruning = value.parse::<bool>().map_err(|_| {
                        anyhow::anyhow!(
                            "--ds-config {key}: expected true or false, got {value:?}"
                        )
                    })?;
                },
                | other => anyhow::bail!(
                    "--ds-config: unknown key {other:?} for hash-trie; accepted keys: {}",
                    Self::HASH_TRIE_KEYS.join(", ")
                ),
            }
        }
        Ok(config)
    }
}

/// Rejects `--ds-config` on index structures that have no Config axis, so
/// a report can never carry a `ds_config_*` axis the structure ignored.
/// Same discipline as [`validate_layout_choices`].
fn validate_config_choices(
    indexstructure: IndexStructureSelector, config: &ConfigChoices,
) -> anyhow::Result<()> {
    if config.explicit()
        && !matches!(
            indexstructure,
            IndexStructureSelector::HashTrie | IndexStructureSelector::All
        )
    {
        anyhow::bail!(
            "--ds-config is only valid with --indexstructure hash-trie (or all); got \
             --indexstructure {indexstructure:?}"
        );
    }
    Ok(())
}
```

- [ ] **Step 4: Wire the flag into both subcommands**

In the `BenchSubcommand::Ds` and `BenchSubcommand::Run` variants, after `layout: LayoutChoices,` add:

```rust
        #[command(flatten)]
        config: ConfigChoices,
```

Destructure `config` in the `main` dispatch for both and pass it through. `run_ds_bench_command` and `run_bench_run_command` each gain `config: ConfigChoices`, call `validate_config_choices(indexstructure, &config)?;` right after the layout validation, and resolve `let hash_trie_config = config.hash_trie_config_resolved()?;`.

`run_bench_run_command` passes `hash_trie_config` to `resolve_sweep` (replacing the `HashTrieConfig::default()` placeholder from Task 9).

`run_ds_bench_command` passes `hash_trie_config` to `dispatch_ds_bench`, which gains a `config: HashTrieConfig` parameter and forwards it to both `run_ds_bench_hash::<…>` arms. `run_ds_bench_hash` gains `config: HashTrieConfig` and replaces its `from_csv` / `from_parquet` match with:

```rust
    let (header, tuples) = match extension.to_lowercase().as_str() {
        | "csv" => kermit_ds::read_csv(relation_path),
        | "parquet" => kermit_ds::read_parquet(relation_path),
        | _ => anyhow::bail!("Unsupported file extension: {extension}"),
    }
    .map_err(|e| anyhow::anyhow!("Failed to load relation: {e}"))?;
    let relation: HashTrie<H> = HashTrie::<H>::from_tuples_with_config(header, config, tuples);
```

Keep the subsequent `let tuples: Vec<Vec<usize>> = relation.collect_tuples();` and `let header = relation.header().clone();` lines unchanged (the benchmark closures use the relation's own tuple order). Two Criterion closures further down rebuild the relation with `HashTrie::<H>::from_tuples(h, t)`: the insertion metric (`|(h, t)| HashTrie::<H>::from_tuples(h, t)`, about 40 lines into the function) and the end-to-end metric (`let built = HashTrie::<H>::from_tuples(h, t);`, about 75 lines in). Change both to `HashTrie::<H>::from_tuples_with_config(h, config, t)` so the measured build is the configured one; `config` is `Copy`, so the closures can capture it by value.

Add `ConfigurableRelation` to the `kermit_ds` imports in `main.rs`.

- [ ] **Step 5: Run tests and clippy**

Run: `cargo test -p kermit && cargo clippy -p kermit --all-targets -- -Dwarnings`
Expected: all pass, clean.

- [ ] **Step 6: Commit**

```bash
git add kermit/src/main.rs
git commit -m "feat(kermit): add --ds-config with singleton-pruning for hash-trie benches"
```

---

### Task 11: CLI smoke test

**Files:**
- Create: `kermit/tests/cli_hash_trie_config_choice.rs`

- [ ] **Step 1: Write the test**

```rust
//! CLI smoke test: `bench ds` with `--ds-config singleton-pruning=true`
//! records the Config axis, and the flag is rejected where it cannot
//! apply. Mirrors `cli_hash_trie_hasher_choice.rs` for the Config category
//! of the optimization standard: CLI parser -> `ConfigChoices` ->
//! `run_ds_bench_hash::<H>` -> `HashTrie::from_tuples_with_config` ->
//! `HasOptimizationAxes::optimization_axes()` -> `BenchReport.axes` -> JSON.

use {
    std::{
        path::PathBuf,
        process::{Command, Output},
    },
    tempfile::NamedTempFile,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

fn edge_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/edge.csv")
}

/// Runs `bench ds` on `edge.csv` with the space metric (cheapest) and the
/// given index structure, appending `extra_args`. Returns the raw output
/// and the report path so callers can assert on either.
fn run_bench_ds(indexstructure: &str, extra_args: &[&str]) -> (Output, NamedTempFile) {
    let report = NamedTempFile::new().expect("failed to create temp report file");
    let mut cmd = Command::new(kermit_bin());
    cmd.arg("bench")
        .arg("--sample-size")
        .arg("10")
        .arg("--measurement-time")
        .arg("1")
        .arg("--warm-up-time")
        .arg("1")
        .arg("--report-json")
        .arg(report.path())
        .arg("ds")
        .arg("--relation")
        .arg(edge_fixture())
        .arg("--indexstructure")
        .arg(indexstructure)
        .arg("-m")
        .arg("space");
    for arg in extra_args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to execute kermit binary");
    (output, report)
}

fn axes_of(report: &NamedTempFile) -> serde_json::Value {
    let text = std::fs::read_to_string(report.path()).expect("report file should exist");
    let reports: Vec<serde_json::Value> =
        serde_json::from_str(&text).expect("report should be valid JSON array");
    reports[0]["axes"].clone()
}

#[test]
fn cli_bench_ds_with_singleton_pruning_records_axis_true() {
    let (output, report) = run_bench_ds("hash-trie", &["--ds-config", "singleton-pruning=true"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let axes = axes_of(&report);
    assert_eq!(axes["ds_config_singleton_pruning"], true, "{axes}");
    assert_eq!(axes["ds_layout_hasher"], "sip", "{axes}");
}

#[test]
fn cli_bench_ds_default_config_records_axis_false() {
    let (output, report) = run_bench_ds("hash-trie", &[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(axes_of(&report)["ds_config_singleton_pruning"], false);
}

#[test]
fn cli_bench_ds_rejects_ds_config_on_tree_trie() {
    let (output, _) = run_bench_ds("tree-trie", &["--ds-config", "singleton-pruning=true"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-config"), "{stderr}");
}

#[test]
fn cli_bench_ds_rejects_unknown_config_key() {
    let (output, _) = run_bench_ds("hash-trie", &["--ds-config", "lazy-expansion=true"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("lazy-expansion"), "{stderr}");
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p kermit --test cli_hash_trie_config_choice`
Expected: 4 passed.

- [ ] **Step 3: Commit**

```bash
git add kermit/tests/cli_hash_trie_config_choice.rs
git commit -m "test(kermit): CLI smoke test for --ds-config singleton-pruning"
```

---

### Task 12: kermit-lab axis default

**Files:**
- Modify: `python/kermit-lab/kermit_lab/defaults.py:16-18`
- Modify: `python/kermit-lab/tests/test_defaults.py`

- [ ] **Step 1: Write the failing test**

Append to `python/kermit-lab/tests/test_defaults.py`:

```python
def test_singleton_pruning_defaults_to_false() -> None:
    df = pd.DataFrame({"ds_config_singleton_pruning": [pd.NA, True]})
    out = apply_axis_defaults(df)
    assert out["ds_config_singleton_pruning"].tolist() == [False, True]
    assert AXIS_DEFAULTS["ds_config_singleton_pruning"] is False
```

(Match the import style already used at the top of that file.)

- [ ] **Step 2: Run to verify it fails**

Run: `cd python/kermit-lab && uv run pytest tests/test_defaults.py -q`
Expected: KeyError on `AXIS_DEFAULTS[...]`.

- [ ] **Step 3: Add the default**

```python
AXIS_DEFAULTS: dict[str, object] = {
    "ds_layout_hasher": "sip",  # HashTrie's historical hash function
    "ds_config_singleton_pruning": False,  # pre-Config reports never pruned
}
```

- [ ] **Step 4: Run the kermit-lab suite**

Run: `cd python/kermit-lab && uv run pytest -q`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add python/kermit-lab/kermit_lab/defaults.py python/kermit-lab/tests/test_defaults.py
git commit -m "feat(kermit-lab): back-fill ds_config_singleton_pruning=false for pre-Config reports"
```

---

### Task 13: Documentation

**Files:**
- Modify: `docs/data-structures/hash-trie.md`
- Modify: `docs/specs/optimization-standard.md`
- Modify: `docs/specs/2026-09-08-singleton-pruning-config-design.md` (one line)
- Modify: `CLAUDE.md`

- [ ] **Step 1: `docs/data-structures/hash-trie.md`**

In the `## Representation` code block add the variant:

```rust
enum HashTrieNode {
    Inner(HashTable<HashTrieNode>),     // depths 0..arity-1
    Leaf(HashTable<Vec<Vec<usize>>>),   // depth arity-1
    Singleton(Vec<usize>),              // pruned subtrie (config.singleton_pruning), depths 1..arity
}
```

and after the paragraph on the iterator stack replace `The iterator \`HashTrieIter\` carries a \`Vec<(&HashTrieNode, usize)>\` stack` with:

> The iterator `HashTrieIter` carries a stack of frames from the root to the current depth. A table frame is `(&HashTrieNode, bucket)`; a singleton frame is `(&tuple, depth, hash, exhausted)` and emulates the one-entry table a pruned level would have held, hashing `tuple[depth]` once when pushed. The deepest frame is the iterator's current position.

Add to `## Invariants`:

> - **Pruned iff exactly one tuple.** With `HashTrieConfig::singleton_pruning` on, a child node is `Singleton` iff exactly one tuple lives below it; the shape is insertion-order independent, and a second tuple (including a duplicate or a full hash collision) unprunes the node back into tables. With the flag off, no `Singleton` exists and the structure is identical to pre-pruning builds. Pinned by `check_pruning_invariant` in `implementation.rs` tests.

Replace the `### Config flags` section:

```markdown
### Config flags

- **Singleton pruning** (`ds_config_singleton_pruning`): stores a subtrie
  that holds exactly one tuple as that tuple (paper §3.3.1, Figure 5)
  instead of one hash table per remaining level. Build-time decision;
  the iterator emulates the pruned levels transparently, so
  [`HashTriejoin`](../algorithms/hash-triejoin.md) is unchanged.
  - **CLI:** `-i hash-trie --ds-config singleton-pruning=true`
  - **Default:** `false` (byte-for-byte the pre-pruning structure).
  - **Rust:** `HashTrie::from_tuples_with_config(header, HashTrieConfig { singleton_pruning: true }, tuples)`
    via `ConfigurableRelation`; tests lift the value to a type with
    `Configured<HashTrie<H>, PruningOn>`.
  - **Bench axis value:** `true` / `false`.
  - **Expected effect:** space strictly smaller on sparse fan-out. Time
    trades one table probe per pruned level for one `H::hash`; expected to
    win under `fxhash` and be roughly neutral under `sip`. Test the
    hypothesis with the `ds_layout_hasher × ds_config_singleton_pruning`
    pivot in kermit-lab.

### Deferred follow-ups

- **Skip-levels short-circuit.** The paper's join verifies a singleton
  against the current bindings and skips the remaining levels. That is an
  algorithm-side change (`HashTrieIterator` would expose the singleton and
  `HashTriejoin` would branch on it) and the natural first consumer of the
  reserved `algo_config_*` prefix.
- Lazy child expansion (`ds_config_lazy_expansion`) needs interior
  mutability through `&self` probes; not started.
```

Update `### Build modes` to keep its current "none" text. Add to the `## Complexity` table a row: `HashTrieIterator::open()` into a pruned level: O(1) plus one `H::hash`.

- [ ] **Step 2: `docs/specs/optimization-standard.md`**

- In "The three categories → Config", change the concrete example from "(hypothetical, not yet implemented)" to point at the real implementation: `HashTrie::insert_at` reads `config.singleton_pruning`; the iterator branches on the `Singleton` node variant.
- In the Config row of the table, replace the test-obligation cell with: `Baseline + ≥1 alternate per flag via define_multiway_join_test_suite_with_config! (kermit/tests/common/macros.rs)`.
- In "What this looks like at the CLI", move `--ds-config singleton-pruning=true` from the hypothetical block to the "Today" block.
- In "The trait family", add a subsection:

```markdown
### `ConfigurableRelation` and `Configured<R, P>` (kermit-ds)

`Relation::new` / `from_tuples` have no parameter for a config value, so
`kermit_ds::ConfigurableRelation` adds `with_config` /
`from_tuples_with_config` / `config()`. Only structures with a Config axis
implement it. `kermit_ds::Configured<R, P>` wraps an `R: ConfigurableRelation`
with a zero-sized `P: ConfigProvider<R::Config>` so macro suites can name a
configured relation as one type; declare `P` with
`kermit_ds::define_config_provider!`.
```

- Replace walkthrough step 7 ("Add tests") with:

```rust
use kermit_ds::{define_config_provider, HashTrieConfig};

define_config_provider!(PruningOn, HashTrieConfig, HashTrieConfig { singleton_pruning: true });
define_multiway_join_test_suite_with_config!(HashTrieSip, HashTriejoin, LexicographicOptimiser, PruningOn);
```

and note the DS-level suites run on `Configured<HashTrieSip, PruningOn>` aliases too.

- In "Where to look in the code", add rows for `kermit-ds/src/configured.rs`, `kermit-ds/src/ds/hash_trie/config.rs`, and `kermit/tests/cli_hash_trie_config_choice.rs`.
- In "What's implemented today", move singleton pruning to the implemented list; keep the BuildMode macro note as "lands with the first BuildMode consumer".

- [ ] **Step 3: Spec amendment**

In `docs/specs/2026-09-08-singleton-pruning-config-design.md` Section 4 "Flag", replace `Wired on \`bench run\`, \`bench ds\`, and \`bench join\`.` with: `Wired on \`bench run\` and \`bench ds\`, the two subcommands that carry \`--ds-layout-hasher\`; \`join\` / \`bench join\` take neither flag family.`

- [ ] **Step 4: `CLAUDE.md`**

- Priorities item 1: replace "Config and BuildMode dimensions are to be tested via the prescribed (but not-yet-implemented) `define_multiway_join_test_suite_with_config!` and `define_multiway_join_test_suite_for_build_mode!` macros respectively; until the first Config/BuildMode consumer lands, write per-config/per-mode tests by hand." with: "Config dimensions are tested via `define_multiway_join_test_suite_with_config!` (baseline + one alternate per flag; see `kermit/tests/join_tests.rs` for the `PruningOn` precedent). BuildMode is tested via the prescribed (not-yet-implemented) `define_multiway_join_test_suite_for_build_mode!`; until the first BuildMode consumer lands, write per-mode tests by hand."
- "Adding an optimization" recipe, step 2: append "A Config on a data structure also implements `ConfigurableRelation` (`kermit-ds/src/relation.rs`) so the value can reach the constructor; `Configured<R, P>` lifts it to a type for the test macros."
- "Adding an optimization" step 4: change `--ds-config <flag>=<value>` to `--ds-config <flag>=<value>[,...]` and note `validate_config_choices` rejects it on structures without a Config axis.
- Gotchas: add "**Config flags are build-time for HashTrie**: `singleton_pruning` decides the node shape at insert, so a config must reach the constructor (`from_tuples_with_config`); there is no setter. `bench ds` therefore loads tuples via `kermit_ds::read_csv` / `read_parquet` and builds once with the config, rather than `from_csv` then rebuilding."
- Build Commands: add `cargo run -- bench run triangle -i hash-trie -a hash-triejoin --ds-config singleton-pruning=true  # Config axis`.

- [ ] **Step 5: Verify docs build**

Run: `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps && cargo clippy --all-targets -- -Dwarnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add docs/data-structures/hash-trie.md docs/specs/optimization-standard.md docs/specs/2026-09-08-singleton-pruning-config-design.md CLAUDE.md
git commit -m "docs: document singleton pruning, ConfigurableRelation, and the with_config suite"
```

---

### Task 14: Full verification, perf acceptance check, issue comment

**Files:** none new.

- [ ] **Step 1: Format**

Run: `nix develop --command cargo fmt --all && git diff --stat`
Expected: only files touched by this plan change. If `kermit-bench/src/definition.rs` or unrelated hunks in `kermit/src/main.rs` appear (known pre-existing drift), revert those hunks with `git checkout -p` before committing.

- [ ] **Step 2: Full CI-equivalent run**

```bash
cargo build --verbose
cargo test --verbose
cargo clippy --all-targets --verbose -- -Dwarnings
cargo fmt --all -- --check      # inside nix develop
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
MIRIFLAGS="-Zmiri-disable-isolation" cargo miri test -p kermit-ds -p kermit-algos -p kermit-iters
```

Expected: all green. `e2e_watdiv` may flake under parallel `cargo test`; rerun it in isolation before blaming this work.

- [ ] **Step 3: Performance acceptance check**

The working tree must be clean here (every task committed). `$SCRATCH` is the session scratchpad directory. The baseline is the spec commit's parent, checked out into a throwaway worktree that is removed afterwards (this is a temporary measurement checkout, not a loom session branch).

```bash
SCRATCH=<scratchpad dir>
BASE=$(git rev-parse 9229ff0~1)          # parent of the spec commit
git worktree add "$SCRATCH/kermit-base" "$BASE"
(cd "$SCRATCH/kermit-base" && cargo run --release -- bench run triangle -i hash-trie -a hash-triejoin --report-json "$SCRATCH/perf-base.json")
cargo run --release -- bench run triangle -i hash-trie -a hash-triejoin --report-json "$SCRATCH/perf-new.json"
cargo run --release -- bench run triangle -i hash-trie -a hash-triejoin --ds-config singleton-pruning=true --report-json "$SCRATCH/perf-pruned.json"
git worktree remove "$SCRATCH/kermit-base"
```

Then in `python/kermit-lab`:

```bash
uv run python -c "
import kermit_lab as kl
base = kl.load('$SCRATCH/perf-base.json')
new  = kl.load('$SCRATCH/perf-new.json')
print(kl.bootstrap_ratio_ci(new, base))
"
```

(Check `kl.bootstrap_ratio_ci`'s exact signature in `python/kermit-lab/kermit_lab/` before running; pass the time-metric rows for the `triangle` query if it expects series rather than frames.)

Expected: the confidence interval for default-config new/base includes 1. Record the interval and the pruned-vs-default numbers in the PR description. If the interval excludes 1, stop and report; the spec's overhead claim is wrong.

- [ ] **Step 4: Comment on #58**

```bash
gh issue comment 58 --repo aidan-bailey/kermit --body "Resolved via option 1: singleton pruning landed as the first Config consumer, with \`ConfigurableRelation\`, \`Configured<R, P>\`, \`define_multiway_join_test_suite_with_config!\`, \`--ds-config singleton-pruning=…\`, and the \`ds_config_singleton_pruning\` axis. Design: docs/specs/2026-09-08-singleton-pruning-config-design.md. BuildMode remains unexercised; its macro lands with the first BuildMode consumer."
```

Do not close the issue from the script; leave that to the PR that merges this branch.

- [ ] **Step 5: Final commit if formatting changed anything**

```bash
git add -u && git commit -m "style: cargo fmt" || true
```
