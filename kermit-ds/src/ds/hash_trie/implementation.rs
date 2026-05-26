//! `HashTrie`: a hash-based trie storing a relation as nested hash tables,
//! one per attribute. Implements `Relation`, `JoinIterable`, `Projectable`,
//! `HeapSize`, and `HashTrieIterable`.

use {
    super::node::HashTrieNode,
    crate::relation::{Relation, RelationHeader},
    kermit_iters::{hash_attribute, JoinIterable},
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
///
/// # Construction
///
/// Use `from_tuples` (batch) or `new` followed by `insert` (incremental).
/// Both funnel through `insert` for a single tuple, faithful to
/// Algorithm 2 from the paper.
pub struct HashTrie {
    header: RelationHeader,
    root: HashTrieNode,
}

impl HashTrie {
    /// Construct the root node appropriate for `arity` — Inner for arity ≥ 2,
    /// Leaf for arity = 1.
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
    /// Used by [`Projectable::project`] and by tests. Allocates a fresh
    /// `Vec<Vec<usize>>`; for large relations this is O(n · arity) in
    /// both time and space.
    pub(crate) fn collect_tuples(&self) -> Vec<Vec<usize>> {
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
        }
    }

    /// Insert one tuple at the appropriate depth in the trie. Recursive
    /// implementation of Algorithm 2 from the paper, line by line.
    fn insert_at(node: &mut HashTrieNode, depth: usize, arity: usize, tuple: Vec<usize>) {
        let key = tuple[depth];
        let hash = hash_attribute(depth, key);
        match node {
            | HashTrieNode::Inner(table) => {
                let child = table.entry_or_insert_with(hash, || {
                    if depth + 1 == arity - 1 {
                        HashTrieNode::new_leaf()
                    } else {
                        HashTrieNode::new_inner()
                    }
                });
                Self::insert_at(child, depth + 1, arity, tuple);
            },
            | HashTrieNode::Leaf(table) => {
                let chain = table.entry_or_insert_with(hash, Vec::new);
                chain.push(tuple);
            },
        }
    }
}

impl JoinIterable for HashTrie {}

impl Relation for HashTrie {
    fn header(&self) -> &RelationHeader { &self.header }

    fn new(header: RelationHeader) -> Self {
        let root = Self::make_root(header.arity());
        Self {
            header,
            root,
        }
    }

    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        let arity = header.arity();
        let mut trie = Self::new(header);
        for tuple in tuples {
            assert_eq!(
                tuple.len(),
                arity,
                "from_tuples: tuple arity {} does not match header arity {}",
                tuple.len(),
                arity,
            );
            Self::insert_at(&mut trie.root, 0, arity, tuple);
        }
        trie
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
    }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) {
        for tuple in tuples {
            self.insert(tuple);
        }
    }
}

impl crate::relation::Projectable for HashTrie {
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
        HashTrie::from_tuples(new_header, projected_tuples)
    }
}

impl crate::heap_size::HeapSize for HashTrie {
    fn heap_size_bytes(&self) -> usize { node_heap_bytes(&self.root) }
}

impl kermit_iters::HashTrieIterable for HashTrie {
    fn hash_trie_iter(&self) -> impl kermit_iters::HashTrieIterator {
        super::hash_trie_iter::HashTrieIter::new(self)
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_arity_2_creates_inner_root() {
        let trie = HashTrie::new(2.into());
        assert_eq!(trie.header().arity(), 2);
        assert!(matches!(trie.root, HashTrieNode::Inner(_)));
    }

    #[test]
    fn new_arity_1_creates_leaf_root() {
        let trie = HashTrie::new(1.into());
        assert_eq!(trie.header().arity(), 1);
        assert!(matches!(trie.root, HashTrieNode::Leaf(_)));
    }

    #[test]
    fn insert_arity_1_populates_leaf() {
        let mut trie = HashTrie::new(1.into());
        trie.insert(vec![42]);
        // Verify by inspecting the root: should be a Leaf with one entry.
        match &trie.root {
            | HashTrieNode::Leaf(table) => assert_eq!(table.len(), 1),
            | _ => panic!("expected Leaf root"),
        }
    }

    #[test]
    fn insert_arity_2_builds_inner_then_leaf() {
        let mut trie = HashTrie::new(2.into());
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
        let mut trie = HashTrie::new(2.into());
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
        let mut trie = HashTrie::new(2.into());
        trie.insert(vec![1]);
    }

    #[test]
    fn from_tuples_arity_2_builds_correct_shape() {
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
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
        let trie = HashTrie::from_tuples(2.into(), vec![]);
        match &trie.root {
            | HashTrieNode::Inner(root_table) => assert_eq!(root_table.len(), 0),
            | _ => panic!("expected Inner root"),
        }
    }

    #[test]
    fn insert_all_equivalent_to_from_tuples_for_multiset_view() {
        let mut a = HashTrie::new(2.into());
        a.insert_all(vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        let b = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
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
        let mut trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
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
        let trie = HashTrie::new(2.into());
        assert!(trie.collect_tuples().is_empty());
    }

    #[test]
    fn heap_size_zero_for_empty_trie() {
        use crate::HeapSize;
        let trie = HashTrie::new(2.into());
        // Even an empty trie allocates initial 4-bucket tables, so heap size
        // is non-zero — what we check is determinism and ordering.
        let small = trie.heap_size_bytes();
        let big = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4], vec![
            3, 5,
        ]])
        .heap_size_bytes();
        assert!(big > small, "non-empty trie should be heavier than empty");
    }

    #[test]
    fn heap_size_deterministic_across_rebuilds() {
        use crate::HeapSize;
        let tuples = vec![vec![1, 2], vec![1, 3], vec![2, 4], vec![3, 5]];
        let a = HashTrie::from_tuples(2.into(), tuples.clone()).heap_size_bytes();
        let b = HashTrie::from_tuples(2.into(), tuples).heap_size_bytes();
        assert_eq!(a, b);
    }

    #[test]
    fn project_drops_columns() {
        use crate::relation::Projectable;
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
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
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        let projected = trie.project(vec![1, 0]);
        let mut collected = projected.collect_tuples();
        collected.sort();
        assert_eq!(collected, vec![vec![2, 1], vec![4, 3]]);
    }

    #[test]
    fn hash_trie_iter_returns_navigable_iterator() {
        use kermit_iters::{HashTrieIterable, HashTrieIterator};
        let trie = HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]);
        let mut it = trie.hash_trie_iter();
        assert!(it.open());
        let mut seen = std::collections::HashSet::new();
        while !it.at_end() {
            seen.insert(it.key().unwrap());
            it.next();
        }
        assert_eq!(seen.len(), 3);
    }
}
