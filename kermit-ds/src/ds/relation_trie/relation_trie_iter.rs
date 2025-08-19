//! This module provides a `TrieIterator` implementation for the `RelationTrie`
//! data structure.

use {
    super::{implementation::RelationTrie, trie_node::TrieNode, trie_traits::TrieFields},
    crate::shared::nodes::Node,
    kermit_derive::IntoTrieIter,
    kermit_iters::{
        key_type::KeyType,
        linear::LinearIterator,
        trie::{TrieIterable, TrieIterator, TrieIteratorWrapper},
    },
};

/// An iterator over the nodes of a `RelationTrie`.
#[derive(IntoTrieIter)]
struct RelationTrieIter<'a, KT: KeyType> {
    /// Current Node's index amongst its siblings.
    pos: usize,
    /// Trie that is being iterated.
    trie: &'a RelationTrie<KT>,
    /// Stack containing cursor's path down the trie.
    /// The tuples hold the Node and its index amongst its siblings.
    /// If the stack is empty, the cursor points to the root.
    /// If the stack is non-empty, the cursor points to the last element.
    stack: Vec<(&'a TrieNode<KT>, usize)>,
}

impl<'a, KT: KeyType> RelationTrieIter<'a, KT> {
    /// Construct a new Trie iterator.
    pub fn new(trie: &'a RelationTrie<KT>) -> Self {
        RelationTrieIter {
            pos: 0,
            trie,
            stack: Vec::new(),
        }
    }

    /// Get the siblings of the node pointed to by the cursor (including the
    /// node).
    ///
    /// Returns None if the cursor points to the root.
    fn siblings(&self) -> Option<&'a Vec<TrieNode<KT>>> {
        if self.stack.is_empty() {
            None
        } else if self.stack.len() == 1 {
            Some(self.trie.children())
        } else {
            Some(self.stack[self.stack.len() - 2].0.children())
        }
    }
}

impl<KT: KeyType> LinearIterator for RelationTrieIter<'_, KT> {
    type KT = KT;

    fn key(&self) -> Option<KT> { Some(self.siblings()?.get(self.pos)?.key()) }

    fn next(&mut self) -> Option<KT> {
        if let Some(siblings) = self.siblings() {
            self.pos += 1;
            if let Some(node) = siblings.get(self.pos) {
                self.stack.pop();
                self.stack.push((node, self.pos));
                return Some(node.key());
            }
        }
        None
    }

    fn seek(&mut self, seek_key: KT) -> bool {
        if self.at_end() {
            return false;
        }

        if let Some(current_key) = self.key() {
            if current_key > seek_key {
                panic!("The sought key must be ≥ the key at the current position.")
            } else {
                // If there exists a key, there should ALWAYS be at least one sibling
                // (i.e., the current node itself).
                let siblings = self
                    .siblings()
                    .expect("If there exists a key, there should ALWAYS be at least one sibling");

                while (!self.at_end()) && seek_key > siblings[self.pos].key() {
                    self.pos += 1;
                }

                if self.at_end() {
                    false
                } else {
                    self.stack.pop();
                    self.stack.push((&siblings[self.pos], self.pos));
                    true
                }
            }
        } else {
            false
        }
    }

    fn at_end(&self) -> bool {
        if let Some(siblings) = self.siblings() {
            self.pos == siblings.len()
        } else {
            true
        }
    }
}

impl<KT: KeyType> TrieIterator for RelationTrieIter<'_, KT> {
    fn open(&mut self) -> bool {
        if let Some((node, _)) = self.stack.last() {
            if let Some(child) = node.children().first() {
                self.stack.push((child, 0));
                self.pos = 0;
                true
            } else {
                false
            }
        } else if self.trie.is_empty() {
            false
        } else {
            self.stack.push((&self.trie.children()[0], 0));
            self.pos = 0;
            true
        }
    }

    fn up(&mut self) -> bool {
        if self.stack.pop().is_some() {
            self.pos = if let Some((_, i)) = self.stack.last() {
                *i
            } else {
                0
            };
            true
        } else {
            false
        }
    }
}

/// Implementation of the `TrieIterable` trait for `RelationTrie`.
impl<KT: KeyType> TrieIterable for RelationTrie<KT> {
    fn trie_iter(&self) -> impl TrieIterator<KT = KT> + IntoIterator<Item = Vec<KT>> {
        RelationTrieIter::new(self)
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::relation::Relation};

    #[test]
    fn test_relation_trie_iter() {
        let trie = RelationTrie::<i32>::from_tuples(2.into(), vec![
            vec![1, 2],
            vec![1, 3],
            vec![2, 4],
            vec![3, 5],
        ]);
        let iter = trie.trie_iter();
        for v in iter {
            assert!(
                !v.is_empty(),
                "Each iteration should yield a non-empty vector."
            );
        }
    }
}
