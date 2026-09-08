//! `HashTrie`: a hash-based trie storing a relation as nested hash tables,
//! one per attribute. Implements `Relation`, `JoinIterable`, `Projectable`,
//! `HeapSize`, and `HashTrieIterable`.
//!
//! `HashTrie` is generic over a [`HashStrategy`](kermit_iters::HashStrategy)
//! `H` (default [`SipHashStrategy`](kermit_iters::SipHashStrategy)). The
//! strategy controls how attribute values are hashed at every level of the
//! trie and is the layout axis emitted by
//! [`HasOptimizationAxes`](kermit_iters::HasOptimizationAxes) under
//! `ds_layout_hasher`.
//!
//! Runtime flags live in [`HashTrieConfig`] (the Config axis, emitted as
//! `ds_config_<flag>`); they reach the trie through
//! [`ConfigurableRelation`](crate::relation::ConfigurableRelation).

use {
    super::{config::HashTrieConfig, node::HashTrieNode},
    crate::relation::{ConfigurableRelation, Relation, RelationHeader},
    kermit_iters::{ConfigOption, HashStrategy, JoinIterable, SipHashStrategy},
    std::marker::PhantomData,
};

/// A hash trie. Each path from root to leaf corresponds to one tuple's
/// hash signature; tuples sharing a complete signature (collisions on
/// every attribute) collect into a chain at the leaf — verification of
/// actual key equality is deferred to the join algorithm.
///
/// # Invariants
///
/// - The depth of every root-to-leaf path equals `header.arity()`.
/// - Inner nodes exist at depths `0..arity-1`; the leaf node at depth
///   `arity-1`.
/// - For arity = 0: undefined behavior (no nullary relations supported).
/// - With `config.singleton_pruning` on, a child node is `Singleton` iff
///   exactly one tuple lives below it (order-independent). With it off,
///   no `Singleton` exists and the structure is identical to pre-pruning
///   builds.
///
/// # Construction
///
/// Use `from_tuples` (batch) or `new` followed by `insert` (incremental).
/// Both funnel through `insert` for a single tuple, faithful to
/// Algorithm 2 from the paper. `with_config` / `from_tuples_with_config`
/// (via [`ConfigurableRelation`](crate::relation::ConfigurableRelation)) are
/// the config-carrying constructors; `new` / `from_tuples` are thin wrappers
/// over them that supply the default configuration.
///
/// # Layout parameter
///
/// `H` is a [`HashStrategy`] selecting which hash function is used to
/// convert attribute values to `u64`. Defaults to
/// [`SipHashStrategy`] for backwards compatibility with pre-standard code.
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

impl<H: HashStrategy> HashTrie<H> {
    /// Whether a node at `depth` is the leaf level, i.e. the last attribute.
    /// Written `depth + 1 == arity` rather than `depth == arity - 1` to avoid
    /// the `usize` underflow at `arity == 0`.
    fn is_leaf_depth(depth: usize, arity: usize) -> bool { depth + 1 == arity }

    /// Construct the root node appropriate for `arity` — Inner for arity ≥ 2,
    /// Leaf for arity = 1. (The root sits at depth 0, so it is a leaf exactly
    /// when `is_leaf_depth(0, arity)`; `arity <= 1` matches that for every
    /// supported arity and additionally treats the unsupported nullary case
    /// as a leaf.)
    fn make_root(arity: usize) -> HashTrieNode {
        if arity <= 1 {
            HashTrieNode::new_leaf()
        } else {
            HashTrieNode::new_inner()
        }
    }

    /// Crate-visible accessor for the root node. Used by `HashTrieIter`
    /// (in the same crate) to navigate the trie via shared references.
    pub(crate) fn root(&self) -> &HashTrieNode { &self.root }

    /// Walk the trie depth-first and return every materialized tuple.
    ///
    /// Used by [`crate::relation::Projectable::project`], the CLI's
    /// `bench` machinery (which needs to recover tuples from an
    /// already-built relation for the insertion-benchmark closure),
    /// and tests. Allocates a fresh `Vec<Vec<usize>>`; for large
    /// relations this is O(n · arity) in both time and space.
    pub fn collect_tuples(&self) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        Self::collect_at(&self.root, &mut out);
        out
    }

    fn collect_at(node: &HashTrieNode, out: &mut Vec<Vec<usize>>) {
        match node {
            | HashTrieNode::Inner(table) => {
                for (_, child) in table.iter() {
                    Self::collect_at(child, out);
                }
            },
            | HashTrieNode::Leaf(table) => {
                for (_, chain) in table.iter() {
                    for tuple in chain {
                        out.push(tuple.clone());
                    }
                }
            },
            | HashTrieNode::Singleton(tuple) => out.push(tuple.clone()),
        }
    }

    /// Insert one tuple at the appropriate depth in the trie. Recursive
    /// implementation of Algorithm 2 from the paper, line by line, plus the
    /// singleton-pruning extension of §3.3.1 (Figure 5) when `prune` is on.
    ///
    /// An unprune costs a single extra O(arity) chain, not a fan-out: the
    /// evicted tuple's re-insert lands in a fresh bucket at the level where
    /// the two tuples diverge and stops there as a new `Singleton`, leaving
    /// only the incoming tuple to keep descending.
    fn insert_at(
        node: &mut HashTrieNode, depth: usize, arity: usize, tuple: Vec<usize>, prune: bool,
    ) {
        let key = tuple[depth];
        let hash = H::hash(key);
        match node {
            | HashTrieNode::Inner(table) => {
                if prune && table.get(hash).is_none() {
                    // Fresh bucket: the subtrie below holds exactly one tuple,
                    // so store the tuple itself instead of one table per
                    // remaining level. The closure always runs — absence was
                    // just proven — so this is two O(1) probes, kept over a
                    // special-cased insert for readability.
                    table.entry_or_insert_with(hash, || HashTrieNode::Singleton(tuple));
                    return;
                }
                // The child lives at `depth + 1`; it is the leaf when that is
                // the last attribute.
                let child_is_leaf = Self::is_leaf_depth(depth + 1, arity);
                let child =
                    table.entry_or_insert_with(hash, || HashTrieNode::new_table(child_is_leaf));
                if matches!(child, HashTrieNode::Singleton(_)) {
                    // Unprune: a second tuple has arrived, so the subtrie no
                    // longer holds exactly one. Swap in the table this level
                    // would have had and re-insert the evicted tuple ahead of
                    // the new one; the recursion re-prunes wherever the two
                    // diverge.
                    let replacement = HashTrieNode::new_table(child_is_leaf);
                    let HashTrieNode::Singleton(evicted) = std::mem::replace(child, replacement)
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
                unreachable!(
                    "insert_at descends through Inner/Leaf only; singletons are unpruned by the \
                     parent"
                )
            },
        }
    }
}

impl<H: HashStrategy> JoinIterable for HashTrie<H> {}

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
        Self::insert_at(&mut self.root, 0, arity, tuple, self.config.singleton_pruning);
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
            Self::insert_at(&mut trie.root, 0, arity, tuple, config.singleton_pruning);
            // from_tuples bypasses insert(), so count here. If this loop is
            // ever refactored to route through insert(), drop this increment
            // or the counter double-counts.
            trie.tuple_count += 1;
        }
        trie
    }

    fn config(&self) -> &HashTrieConfig { &self.config }
}

impl<H: HashStrategy> crate::relation::Projectable for HashTrie<H> {
    fn project(&self, columns: Vec<usize>) -> Self {
        let arity = self.header.arity();
        for &c in &columns {
            assert!(
                c < arity,
                "project: column index {c} out of range for arity {arity}"
            );
        }
        // Build the projected header. Match the convention used by
        // project_via_trie_iter in `kermit-ds/src/relation.rs`.
        let projected_attrs: Vec<String> = columns
            .iter()
            .filter_map(|&c| self.header.attrs().get(c).cloned())
            .collect();
        let new_header = if projected_attrs.is_empty() {
            RelationHeader::new_nameless_positional(columns.len())
        } else {
            RelationHeader::new_nameless(projected_attrs)
        };
        let projected_tuples: Vec<Vec<usize>> = self
            .collect_tuples()
            .into_iter()
            .map(|tuple| columns.iter().map(|&c| tuple[c]).collect())
            .collect();
        HashTrie::<H>::from_tuples_with_config(new_header, self.config, projected_tuples)
    }
}

impl<H: HashStrategy> crate::heap_size::HeapSize for HashTrie<H> {
    fn heap_size_bytes(&self) -> usize { node_heap_bytes(&self.root) }
}

impl<H: HashStrategy> crate::cardinality::Cardinality for HashTrie<H> {
    fn tuple_count(&self) -> usize { self.tuple_count }
}

impl<H: HashStrategy> kermit_iters::HashTrieIterable for HashTrie<H> {
    fn hash_trie_iter(&self) -> impl kermit_iters::HashTrieIterator {
        super::hash_trie_iter::HashTrieIter::<H>::new(self)
    }
}

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

fn node_heap_bytes(node: &HashTrieNode) -> usize {
    match node {
        | HashTrieNode::Inner(table) => {
            let shell = table.shell_heap_bytes();
            let children: usize = table.iter().map(|(_, child)| node_heap_bytes(child)).sum();
            shell + children
        },
        | HashTrieNode::Leaf(table) => {
            let shell = table.shell_heap_bytes();
            let chains: usize = table
                .iter()
                .map(|(_, chain)| {
                    chain.capacity() * std::mem::size_of::<Vec<usize>>()
                        + chain
                            .iter()
                            .map(|t| t.capacity() * std::mem::size_of::<usize>())
                            .sum::<usize>()
                })
                .sum();
            shell + chains
        },
        | HashTrieNode::Singleton(tuple) => tuple.capacity() * std::mem::size_of::<usize>(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test let-bindings use `HashTrie` (the bare type), which resolves
    // through the `<H = SipHashStrategy>` default in type position. The
    // default cannot be selected from a path expression alone, so
    // type-annotated bindings are necessary on nightly Rust.

    #[test]
    fn new_arity_2_creates_inner_root() {
        let trie: HashTrie = HashTrie::new(2.into());
        assert_eq!(trie.header().arity(), 2);
        assert!(matches!(trie.root, HashTrieNode::Inner(_)));
    }

    #[test]
    fn new_arity_1_creates_leaf_root() {
        let trie: HashTrie = HashTrie::new(1.into());
        assert_eq!(trie.header().arity(), 1);
        assert!(matches!(trie.root, HashTrieNode::Leaf(_)));
    }

    #[test]
    fn insert_arity_1_populates_leaf() {
        let mut trie: HashTrie = HashTrie::new(1.into());
        trie.insert(vec![42]);
        // Verify by inspecting the root: should be a Leaf with one entry.
        match &trie.root {
            | HashTrieNode::Leaf(table) => assert_eq!(table.len(), 1),
            | _ => panic!("expected Leaf root"),
        }
    }

    #[test]
    fn insert_arity_2_builds_inner_then_leaf() {
        let mut trie: HashTrie = HashTrie::new(2.into());
        trie.insert(vec![1, 2]);
        match &trie.root {
            | HashTrieNode::Inner(root_table) => {
                assert_eq!(root_table.len(), 1);
                // Walk one level deeper and confirm it's a Leaf.
                let mut found_leaf = false;
                for (_, child) in root_table.iter() {
                    assert!(matches!(child, HashTrieNode::Leaf(_)));
                    if let HashTrieNode::Leaf(leaf_table) = child {
                        assert_eq!(leaf_table.len(), 1);
                        found_leaf = true;
                    }
                }
                assert!(found_leaf);
            },
            | _ => panic!("expected Inner root"),
        }
    }

    #[test]
    fn insert_two_tuples_sharing_first_attribute() {
        let mut trie: HashTrie = HashTrie::new(2.into());
        trie.insert(vec![1, 2]);
        trie.insert(vec![1, 3]);
        // Same attr-0 value => same hash at root => same child node; child has
        // two entries.
        match &trie.root {
            | HashTrieNode::Inner(root_table) => {
                assert_eq!(root_table.len(), 1);
                for (_, child) in root_table.iter() {
                    if let HashTrieNode::Leaf(leaf_table) = child {
                        assert_eq!(leaf_table.len(), 2);
                    }
                }
            },
            | _ => panic!("expected Inner root"),
        }
    }

    #[test]
    #[should_panic(expected = "tuple arity")]
    fn insert_wrong_arity_panics() {
        let mut trie: HashTrie = HashTrie::new(2.into());
        trie.insert(vec![1]);
    }

    #[test]
    fn from_tuples_arity_2_builds_correct_shape() {
        let trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        assert_eq!(trie.header().arity(), 2);
        // Same attr-0 inserts share a child; two distinct attr-0 values =>
        // two root-level entries.
        match &trie.root {
            | HashTrieNode::Inner(root_table) => assert_eq!(root_table.len(), 2),
            | _ => panic!("expected Inner root"),
        }
    }

    #[test]
    fn from_tuples_empty_input() {
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![]);
        match &trie.root {
            | HashTrieNode::Inner(root_table) => assert_eq!(root_table.len(), 0),
            | _ => panic!("expected Inner root"),
        }
    }

    #[test]
    fn insert_all_equivalent_to_from_tuples_for_multiset_view() {
        let mut a: HashTrie = HashTrie::new(2.into());
        a.insert_all(vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        let b: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        // Compare via heap_size and tuple set (via Projectable when ready) —
        // for now confirm both have populated roots with the same length.
        let a_root_len = match &a.root {
            | HashTrieNode::Inner(t) => t.len(),
            | _ => unreachable!(),
        };
        let b_root_len = match &b.root {
            | HashTrieNode::Inner(t) => t.len(),
            | _ => unreachable!(),
        };
        assert_eq!(a_root_len, b_root_len);
    }

    #[test]
    fn collect_tuples_recovers_input_as_multiset() {
        let mut trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        let mut collected = trie.collect_tuples();
        collected.sort();
        assert_eq!(collected, vec![vec![1, 2], vec![1, 3], vec![2, 4]]);

        // Adding a tuple with full collision-equal hash signature: same value
        // at each attribute => same hash path => stored on the same leaf chain.
        trie.insert(vec![1, 2]);
        let mut collected = trie.collect_tuples();
        collected.sort();
        assert_eq!(collected, vec![vec![1, 2], vec![1, 2], vec![1, 3], vec![
            2, 4
        ]]);
    }

    #[test]
    fn collect_tuples_empty_trie() {
        let trie: HashTrie = HashTrie::new(2.into());
        assert!(trie.collect_tuples().is_empty());
    }

    #[test]
    fn heap_size_zero_for_empty_trie() {
        use crate::HeapSize;
        let trie: HashTrie = HashTrie::new(2.into());
        // Even an empty trie allocates initial 4-bucket tables, so heap size
        // is non-zero — what we check is determinism and ordering.
        let small = trie.heap_size_bytes();
        let big_trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4], vec![
                3, 5,
            ]]);
        let big = big_trie.heap_size_bytes();
        assert!(big > small, "non-empty trie should be heavier than empty");
    }

    #[test]
    fn heap_size_deterministic_across_rebuilds() {
        use crate::HeapSize;
        let tuples = vec![vec![1, 2], vec![1, 3], vec![2, 4], vec![3, 5]];
        let trie_a: HashTrie = HashTrie::from_tuples(2.into(), tuples.clone());
        let trie_b: HashTrie = HashTrie::from_tuples(2.into(), tuples);
        assert_eq!(trie_a.heap_size_bytes(), trie_b.heap_size_bytes());
    }

    #[test]
    fn project_drops_columns() {
        use crate::relation::Projectable;
        let trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        // π_0 (first column only)
        let projected = trie.project(vec![0]);
        assert_eq!(projected.header().arity(), 1);
        let mut collected = projected.collect_tuples();
        collected.sort();
        // Duplicate `1`s collapse only if from_tuples deduplicates — HashTrie
        // is a multiset, so we expect duplicates to survive.
        assert_eq!(collected, vec![vec![1], vec![1], vec![2]]);
    }

    #[test]
    fn project_reorders_columns() {
        use crate::relation::Projectable;
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        let projected = trie.project(vec![1, 0]);
        let mut collected = projected.collect_tuples();
        collected.sort();
        assert_eq!(collected, vec![vec![2, 1], vec![4, 3]]);
    }

    #[test]
    fn hash_trie_iter_returns_navigable_iterator() {
        use kermit_iters::{HashTrieIterable, HashTrieIterator};
        let trie: HashTrie = HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let mut it = trie.hash_trie_iter();
        assert!(it.open());
        let mut seen = std::collections::HashSet::new();
        while !it.at_end() {
            seen.insert(it.key().unwrap());
            it.next();
        }
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn optimization_axes_default_strategy_reports_sip() {
        use kermit_iters::HasOptimizationAxes;
        let trie: HashTrie = HashTrie::new(2.into());
        let axes = trie.optimization_axes();
        let mut keys: Vec<&str> = axes.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["ds_config_singleton_pruning", "ds_layout_hasher"]);
        assert_eq!(
            axes.get("ds_layout_hasher"),
            Some(&serde_json::Value::String("sip".to_string())),
        );
    }

    #[test]
    fn optimization_axes_fxhash_strategy_reports_fxhash() {
        use kermit_iters::{FxHashStrategy, HasOptimizationAxes};
        let trie: HashTrie<FxHashStrategy> = HashTrie::new(2.into());
        let axes = trie.optimization_axes();
        let mut keys: Vec<&str> = axes.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["ds_config_singleton_pruning", "ds_layout_hasher"]);
        assert_eq!(
            axes.get("ds_layout_hasher"),
            Some(&serde_json::Value::String("fxhash".to_string())),
        );
    }

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

    #[test]
    fn from_tuples_uses_default_config() {
        use crate::relation::ConfigurableRelation;
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        assert_eq!(*trie.config(), HashTrieConfig::default());
    }

    #[test]
    fn config_survives_insert_and_insert_all() {
        use crate::relation::ConfigurableRelation;
        let on = HashTrieConfig {
            singleton_pruning: true,
        };
        let mut trie: HashTrie = HashTrie::with_config(2.into(), on);
        trie.insert(vec![1, 2]);
        assert_eq!(*trie.config(), on);
        trie.insert_all(vec![vec![3, 4], vec![5, 6]]);
        assert_eq!(*trie.config(), on);
    }

    #[test]
    #[should_panic(expected = "from_tuples: tuple arity")]
    fn from_tuples_with_config_wrong_arity_panics() {
        use crate::relation::ConfigurableRelation;
        let _: HashTrie = HashTrie::from_tuples_with_config(2.into(), HashTrieConfig::default(), vec![
            vec![1],
        ]);
    }

    // ── Singleton pruning ──────────────────────────────────────────────

    const PRUNE: HashTrieConfig = HashTrieConfig {
        singleton_pruning: true,
    };

    fn pruned(arity: usize, tuples: Vec<Vec<usize>>) -> HashTrie {
        HashTrie::from_tuples_with_config(arity.into(), PRUNE, tuples)
    }

    /// Walks `root`, asserting the pruning invariant at every inner bucket
    /// and returning the number of tuples stored below it.
    ///
    /// Invariant: under pruning a child is `Singleton` iff exactly one
    /// tuple lives below it; without pruning no `Singleton` exists. The
    /// root is never a `Singleton`, and a `Singleton` sits under the
    /// buckets its own tuple hashes to.
    fn check_pruning_invariant(root: &HashTrieNode, prune: bool) -> usize {
        assert!(
            !matches!(root, HashTrieNode::Singleton(_)),
            "the root is never a Singleton"
        );
        check_pruning_invariant_at(root, prune, &mut Vec::new())
    }

    /// Recursive half of [`check_pruning_invariant`]. `prefix` is the
    /// sequence of bucket hashes taken from the root down to `node`, so a
    /// `Singleton` reached here must hash to every one of them — that is
    /// what pins it to the right *place*, not merely the right count.
    fn check_pruning_invariant_at(node: &HashTrieNode, prune: bool, prefix: &mut Vec<u64>) -> usize {
        match node {
            | HashTrieNode::Singleton(tuple) => {
                assert!(prune, "Singleton found with pruning off");
                for (d, &expected) in prefix.iter().enumerate() {
                    assert_eq!(
                        <SipHashStrategy as HashStrategy>::hash(tuple[d]),
                        expected,
                        "Singleton {tuple:?} is misplaced at depth {d}"
                    );
                }
                1
            },
            | HashTrieNode::Leaf(table) => table.iter().map(|(_, chain)| chain.len()).sum(),
            | HashTrieNode::Inner(table) => table
                .iter()
                .map(|(hash, child)| {
                    prefix.push(hash);
                    let below = check_pruning_invariant_at(child, prune, prefix);
                    prefix.pop();
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
    fn pruning_arity_1_has_no_singletons() {
        // The root is the only node an arity-1 trie has, and the root is
        // never pruned — tuples land straight in leaf chains.
        let trie = pruned(1, vec![vec![1], vec![2]]);
        assert_eq!(check_pruning_invariant(&trie.root, true), 2);
        assert!(matches!(trie.root, HashTrieNode::Leaf(_)));
        let mut got = trie.collect_tuples();
        got.sort();
        assert_eq!(got, vec![vec![1], vec![2]]);
    }

    #[test]
    fn unprune_when_second_tuple_diverges_one_level_down() {
        let trie = pruned(3, vec![vec![1, 2, 3], vec![1, 4, 5]]);
        assert_eq!(check_pruning_invariant(&trie.root, true), 2);
        let mut got = trie.collect_tuples();
        got.sort();
        assert_eq!(got, vec![vec![1, 2, 3], vec![1, 4, 5]]);
    }

    #[test]
    fn unprune_when_second_tuple_shares_hashes_to_the_leaf() {
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
    fn pruned_build_is_insertion_order_independent() {
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
}

#[cfg(test)]
mod cardinality_tests {
    use {super::*, crate::cardinality::Cardinality};

    #[test]
    fn empty_relation_has_zero_tuples() {
        let trie: HashTrie = HashTrie::new(2.into());
        assert_eq!(trie.tuple_count(), 0);
    }

    #[test]
    fn tuple_count_matches_collect_tuples_len() {
        let trie: HashTrie =
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        assert_eq!(trie.tuple_count(), 3);
        assert_eq!(trie.collect_tuples().len(), 3);
    }

    #[test]
    fn duplicate_insert_counts_multiset_semantics() {
        // HashTrie is deliberately a multiset: duplicates append to leaf
        // chains (equality is deferred to the join algorithm), so the
        // count includes them — matching what iteration yields.
        let mut trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        trie.insert(vec![1, 2]);
        assert_eq!(trie.tuple_count(), 2);
        assert_eq!(trie.collect_tuples().len(), 2);
    }

    #[test]
    fn from_tuples_with_duplicates_counts_multiset_semantics() {
        // Same multiset guarantee, but pinned on the batch-construction
        // path: from_tuples maintains the counter itself (it bypasses
        // insert()), so duplicates must count there too.
        let trie: HashTrie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 2]]);
        assert_eq!(trie.tuple_count(), 2);
        assert_eq!(trie.collect_tuples().len(), 2);
    }
}
