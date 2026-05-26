//! Recursive node type for the hash trie. Inner levels carry child node
//! tables; the deepest (leaf) level carries tuple chains.
//!
//! The variant is fixed by depth: inner nodes live at depths `0..arity-1`,
//! the leaf node at depth `arity-1`. No runtime check polices this —
//! `HashTrie::insert_at` constructs the right variant based on the
//! caller's known arity.

use super::hash_table::HashTable;

/// A node in a hash trie.
pub(crate) enum HashTrieNode {
    /// Inner level: hash table whose values are child nodes.
    Inner(HashTable<HashTrieNode>),
    /// Leaf level: hash table whose values are tuple chains. Each chain
    /// holds the full materialized tuples whose attribute hashes match the
    /// path of hashes from the root to this bucket.
    Leaf(HashTable<Vec<Vec<usize>>>),
}

impl HashTrieNode {
    pub(crate) fn new_inner() -> Self { HashTrieNode::Inner(HashTable::new()) }

    pub(crate) fn new_leaf() -> Self { HashTrieNode::Leaf(HashTable::new()) }
}
