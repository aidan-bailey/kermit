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
    fn key(&self) -> Option<u64> {
        let &(node, idx) = self.stack.last()?;
        match node {
            | HashTrieNode::Inner(t) => t.hash_at(idx),
            | HashTrieNode::Leaf(t) => t.hash_at(idx),
        }
    }

    fn next(&mut self) -> Option<u64> {
        let (node, idx) = {
            let entry = self.stack.last_mut()?;
            let cap = Self::node_capacity(entry.0);
            // Start from idx + 1 (paper's "advance"), find next occupied or
            // past-end.
            entry.1 = Self::first_occupied_from(entry.0, entry.1 + 1);
            if entry.1 >= cap {
                return None;
            }
            (entry.0, entry.1)
        };
        match node {
            | HashTrieNode::Inner(t) => t.hash_at(idx),
            | HashTrieNode::Leaf(t) => t.hash_at(idx),
        }
    }

    fn lookup(&mut self, hash: u64) -> bool {
        let entry = match self.stack.last_mut() {
            | Some(e) => e,
            | None => return false,
        };
        let idx = match entry.0 {
            | HashTrieNode::Inner(t) => t.index_of(hash),
            | HashTrieNode::Leaf(t) => t.index_of(hash),
        };
        match idx {
            | Some(i) => {
                entry.1 = i;
                true
            },
            | None => {
                // Move to past-end; the caller's loop should exit.
                entry.1 = Self::node_capacity(entry.0);
                false
            },
        }
    }

    fn size(&self) -> usize {
        match self.stack.last() {
            | Some(&(node, _)) => match node {
                | HashTrieNode::Inner(t) => t.len(),
                | HashTrieNode::Leaf(t) => t.len(),
            },
            | None => 0,
        }
    }

    fn at_end(&self) -> bool {
        match self.stack.last() {
            | Some(&(node, idx)) => idx >= Self::node_capacity(node),
            | None => true,
        }
    }

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

    fn up(&mut self) -> bool { self.stack.pop().is_some() }

    fn leaf_tuples(&self) -> Option<&[Vec<usize>]> {
        let &(node, idx) = self.stack.last()?;
        match node {
            | HashTrieNode::Leaf(t) => t.value_at(idx).map(|v| v.as_slice()),
            | HashTrieNode::Inner(_) => None,
        }
    }
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

    #[test]
    fn key_returns_hash_at_current_bucket() {
        let trie = HashTrie::from_tuples(1.into(), vec![vec![42]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        let h = it.key().expect("key after open should be Some");
        // Verify it matches the expected hash_attribute(0, 42).
        assert_eq!(h, kermit_iters::hash_attribute(0, 42));
    }

    #[test]
    fn at_end_after_advancing_past_last() {
        let trie = HashTrie::from_tuples(1.into(), vec![vec![42]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        assert!(!it.at_end());
        // Single tuple => one occupied bucket. next() advances past it.
        it.next();
        assert!(it.at_end());
        assert!(it.key().is_none());
    }

    #[test]
    fn next_iterates_through_all_occupied_buckets() {
        // Insert several tuples whose attribute-0 hashes are likely distinct.
        // Because we can't predict bucket order without inspecting hashes,
        // assert the *set* of yielded hashes matches the expected set.
        let trie = HashTrie::from_tuples(1.into(), vec![
            vec![1],
            vec![2],
            vec![3],
            vec![4],
            vec![5],
        ]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        let mut seen = std::collections::HashSet::new();
        while !it.at_end() {
            seen.insert(it.key().unwrap());
            it.next();
        }
        let expected: std::collections::HashSet<_> =
            (1..=5_usize).map(|k| kermit_iters::hash_attribute(0, k)).collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn size_at_root_returns_distinct_count() {
        let trie = HashTrie::from_tuples(2.into(), vec![
            vec![1, 10],
            vec![1, 20], // same attr-0 hash as above
            vec![2, 30],
            vec![3, 40],
        ]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        // Three distinct attribute-0 values => three root-level buckets.
        assert_eq!(it.size(), 3);
    }

    #[test]
    fn lookup_hits_existing_hash() {
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3, 4]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        let h = kermit_iters::hash_attribute(0, 3);
        assert!(it.lookup(h));
        assert_eq!(it.key(), Some(h));
    }

    #[test]
    fn lookup_misses_unknown_hash() {
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        let h = kermit_iters::hash_attribute(0, 99);
        assert!(!it.lookup(h));
    }

    #[test]
    fn up_returns_to_parent_depth() {
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open(); // depth 1
        let key_at_depth_1 = it.key();
        it.open(); // depth 2
        assert!(it.up()); // back to depth 1
        assert_eq!(it.key(), key_at_depth_1);
    }

    #[test]
    fn up_returns_false_at_root() {
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        // up() with empty stack
        assert!(!it.up());
        it.open();
        // up() with stack depth 1 — pops, stack now empty; up() returns true
        // because we did pop something
        assert!(it.up());
        // now stack is empty, up returns false
        assert!(!it.up());
    }

    #[test]
    fn leaf_tuples_returns_none_at_inner_level() {
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut it = HashTrieIter::new(&trie);
        it.open(); // at Inner level
        assert!(it.leaf_tuples().is_none());
    }

    #[test]
    fn leaf_tuples_returns_some_at_leaf_level() {
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3]]);
        let mut it = HashTrieIter::new(&trie);
        it.open();
        it.open();
        let chain = it.leaf_tuples().expect("leaf_tuples Some at leaf");
        // The chain has at least one tuple at this bucket position.
        assert!(!chain.is_empty());
    }

    #[test]
    fn leaf_tuples_none_when_no_leaf_in_stack() {
        let trie = HashTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let it = HashTrieIter::new(&trie);
        // Pre-open — no stack entry at all.
        assert!(it.leaf_tuples().is_none());
    }
}
