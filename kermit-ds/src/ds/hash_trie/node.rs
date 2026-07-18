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

    // Variant-agnostic forwarding accessors. Both variants wrap a
    // `HashTable<V>`, and these queries are independent of the value type
    // `V` (they read bucket structure and stored hashes, not values), so a
    // single accessor spares every call site the identical `match` on the
    // node variant.

    /// Bucket-array length of this node's table (a power of two).
    pub(crate) fn buckets_len(&self) -> usize {
        match self {
            | HashTrieNode::Inner(t) => t.buckets_len(),
            | HashTrieNode::Leaf(t) => t.buckets_len(),
        }
    }

    /// First occupied bucket index at or after `start`; `buckets_len()` if
    /// none exists at or after `start`.
    pub(crate) fn next_occupied(&self, start: usize) -> usize {
        match self {
            | HashTrieNode::Inner(t) => t.next_occupied(start),
            | HashTrieNode::Leaf(t) => t.next_occupied(start),
        }
    }

    /// Hash stored at bucket `idx`, or `None` if the bucket is empty.
    pub(crate) fn hash_at(&self, idx: usize) -> Option<u64> {
        match self {
            | HashTrieNode::Inner(t) => t.hash_at(idx),
            | HashTrieNode::Leaf(t) => t.hash_at(idx),
        }
    }

    /// Index of the bucket containing `hash`, or `None` if absent.
    pub(crate) fn index_of(&self, hash: u64) -> Option<usize> {
        match self {
            | HashTrieNode::Inner(t) => t.index_of(hash),
            | HashTrieNode::Leaf(t) => t.index_of(hash),
        }
    }

    /// Number of occupied buckets (distinct hashes) at this node.
    pub(crate) fn len(&self) -> usize {
        match self {
            | HashTrieNode::Inner(t) => t.len(),
            | HashTrieNode::Leaf(t) => t.len(),
        }
    }
}
