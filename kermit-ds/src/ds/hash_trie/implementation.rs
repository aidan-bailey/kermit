//! `HashTrie`: a hash-based trie storing a relation as nested hash tables,
//! one per attribute. Implements `Relation`, `JoinIterable`, `Projectable`,
//! `HeapSize`, and `HashTrieIterable`.

use {
    super::node::HashTrieNode,
    crate::relation::{Relation, RelationHeader},
    kermit_iters::JoinIterable,
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
}

impl JoinIterable for HashTrie {}

impl Relation for HashTrie {
    fn header(&self) -> &RelationHeader { &self.header }

    fn new(header: RelationHeader) -> Self {
        let root = Self::make_root(header.arity());
        Self { header, root }
    }

    fn from_tuples(_header: RelationHeader, _tuples: Vec<Vec<usize>>) -> Self {
        unimplemented!("Task 3.4");
    }

    fn insert(&mut self, _tuple: Vec<usize>) { unimplemented!("Task 3.3"); }

    fn insert_all(&mut self, _tuples: Vec<Vec<usize>>) { unimplemented!("Task 3.4"); }
}

impl crate::relation::Projectable for HashTrie {
    fn project(&self, _columns: Vec<usize>) -> Self { unimplemented!("Task 3.7"); }
}

impl crate::heap_size::HeapSize for HashTrie {
    fn heap_size_bytes(&self) -> usize { unimplemented!("Task 3.6"); }
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
}
