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

impl HashTrieNode {
    pub(crate) fn new_inner() -> Self { HashTrieNode::Inner(HashTable::new()) }

    pub(crate) fn new_leaf() -> Self { HashTrieNode::Leaf(HashTable::new()) }

    /// The table accessors below are only meaningful on `Inner` / `Leaf`.
    /// `HashTrieIter` never places a `Singleton` in a table frame, so
    /// reaching this is a broken internal invariant, not a user error.
    fn singleton_is_not_a_table() -> ! {
        panic!("HashTrieNode table accessor called on a Singleton (pruned subtrie)")
    }

    // Variant-agnostic forwarding accessors. The two table variants wrap a
    // `HashTable<V>`, and these queries are independent of the value type
    // `V` (they read bucket structure and stored hashes, not values), so a
    // single accessor spares every call site the identical `match` on the
    // node variant. The `Singleton` arm panics: a pruned subtrie has no
    // table to forward to.

    /// Bucket-array length of this node's table (a power of two).
    pub(crate) fn buckets_len(&self) -> usize {
        match self {
            | HashTrieNode::Inner(t) => t.buckets_len(),
            | HashTrieNode::Leaf(t) => t.buckets_len(),
            | HashTrieNode::Singleton(_) => Self::singleton_is_not_a_table(),
        }
    }

    /// First occupied bucket index at or after `start`; `buckets_len()` if
    /// none exists at or after `start`.
    pub(crate) fn next_occupied(&self, start: usize) -> usize {
        match self {
            | HashTrieNode::Inner(t) => t.next_occupied(start),
            | HashTrieNode::Leaf(t) => t.next_occupied(start),
            | HashTrieNode::Singleton(_) => Self::singleton_is_not_a_table(),
        }
    }

    /// Hash stored at bucket `idx`, or `None` if the bucket is empty.
    pub(crate) fn hash_at(&self, idx: usize) -> Option<u64> {
        match self {
            | HashTrieNode::Inner(t) => t.hash_at(idx),
            | HashTrieNode::Leaf(t) => t.hash_at(idx),
            | HashTrieNode::Singleton(_) => Self::singleton_is_not_a_table(),
        }
    }

    /// Index of the bucket containing `hash`, or `None` if absent.
    pub(crate) fn index_of(&self, hash: u64) -> Option<usize> {
        match self {
            | HashTrieNode::Inner(t) => t.index_of(hash),
            | HashTrieNode::Leaf(t) => t.index_of(hash),
            | HashTrieNode::Singleton(_) => Self::singleton_is_not_a_table(),
        }
    }

    /// Number of occupied buckets (distinct hashes) at this node.
    pub(crate) fn len(&self) -> usize {
        match self {
            | HashTrieNode::Inner(t) => t.len(),
            | HashTrieNode::Leaf(t) => t.len(),
            | HashTrieNode::Singleton(_) => Self::singleton_is_not_a_table(),
        }
    }
}
