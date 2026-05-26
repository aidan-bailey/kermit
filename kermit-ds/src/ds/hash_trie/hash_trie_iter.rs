//! `HashTrieIter` — navigates a `HashTrie` via the [`HashTrieIterator`]
//! interface.
//!
//! # Position model
//!
//! `stack` carries `(node, bucket_index)` pairs from the root down to the
//! current depth. The deepest entry is the iterator's current position.
//! Empty stack = not yet opened (pre-root).
//!
//! On every `open`, we descend either into the root (when the stack is
//! empty) or into the child of the current bucket, then advance to the
//! first occupied bucket within that node. If the node has no occupied
//! buckets, the new stack entry sits at `bucket_index = capacity`
//! (i.e., past-end) and `at_end` returns true.

use {
    super::{implementation::HashTrie, node::HashTrieNode},
    kermit_iters::HashTrieIterator,
};

/// Stack-based iterator over a [`HashTrie`].
///
/// See the module docs for the position model.
pub struct HashTrieIter<'a> {
    stack: Vec<(&'a HashTrieNode, usize)>,
    trie: &'a HashTrie,
}

impl<'a> HashTrieIter<'a> {
    /// Construct a fresh iterator positioned before the root.
    pub(crate) fn new(trie: &'a HashTrie) -> Self {
        Self {
            stack: Vec::new(),
            trie,
        }
    }

    /// Return the bucket-array length for a node (used for at-end checks).
    fn node_capacity(node: &HashTrieNode) -> usize {
        match node {
            | HashTrieNode::Inner(t) => t.buckets_len(),
            | HashTrieNode::Leaf(t) => t.buckets_len(),
        }
    }

    /// Find the first occupied bucket index in a node, starting from
    /// `start`. Returns the index, or `node.capacity()` if none exists.
    fn first_occupied_from(node: &HashTrieNode, start: usize) -> usize {
        match node {
            | HashTrieNode::Inner(t) => t.next_occupied(start),
            | HashTrieNode::Leaf(t) => t.next_occupied(start),
        }
    }

    /// Return the current bucket's child node, if any. Only meaningful when
    /// the deepest node is `Inner` and its bucket index is occupied.
    fn current_inner_child(&self) -> Option<&'a HashTrieNode> {
        let (node, idx) = *self.stack.last()?;
        match node {
            | HashTrieNode::Inner(t) => t.value_at(idx),
            | HashTrieNode::Leaf(_) => None,
        }
    }
}

impl HashTrieIterator for HashTrieIter<'_> {
    fn key(&self) -> Option<u64> { unimplemented!("Task 4.2") }

    fn next(&mut self) -> Option<u64> { unimplemented!("Task 4.2") }

    fn lookup(&mut self, _hash: u64) -> bool { unimplemented!("Task 4.3") }

    fn size(&self) -> usize { unimplemented!("Task 4.2") }

    fn at_end(&self) -> bool { unimplemented!("Task 4.2") }

    fn open(&mut self) -> bool {
        if self.stack.is_empty() {
            // Descend into the root via the crate-visible accessor.
            let root = self.trie.root();
            let start = Self::first_occupied_from(root, 0);
            self.stack.push((root, start));
            return start < Self::node_capacity(root);
        }
        // Descend into the child node at the current bucket.
        let child = match self.current_inner_child() {
            | Some(c) => c,
            | None => return false, // already at leaf or current bucket empty
        };
        let start = Self::first_occupied_from(child, 0);
        self.stack.push((child, start));
        start < Self::node_capacity(child)
    }

    fn up(&mut self) -> bool { unimplemented!("Task 4.4") }

    fn leaf_tuples(&self) -> Option<&[Vec<usize>]> { unimplemented!("Task 4.5") }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{ds::hash_trie::implementation::HashTrie, relation::Relation},
    };

    #[test]
    fn open_on_empty_trie_returns_false() {
        let trie = HashTrie::new(2.into());
        let mut it = HashTrieIter::new(&trie);
        assert!(!it.open());
    }

    #[test]
    fn open_descends_into_populated_root() {
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open());
    }

    #[test]
    fn open_then_open_descends_two_levels() {
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        assert!(it.open()); // depth 1 (root level)
        assert!(it.open()); // depth 2 (leaf level)
    }

    #[test]
    fn open_three_times_fails_on_arity_2() {
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        it.open();
        // Already at leaf — can't descend further.
        assert!(!it.open());
    }
}
