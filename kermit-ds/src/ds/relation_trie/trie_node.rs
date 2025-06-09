//! This module implements a node used within a trie.

use {
    super::trie_traits::{Internal, TrieFields},
    crate::shared::nodes::Node,
    kermit_iters::key_type::KeyType,
    std::ops::{Index, IndexMut},
};

/// The `TrieNode` struct represents a node in a trie data structure.
#[derive(Clone, Debug)]
pub struct TrieNode<KT>
where
    KT: KeyType,
{
    /// Value stored.
    key: KT,
    /// Children of node.
    children: Vec<TrieNode<KT>>,
}

impl<KT> Node for TrieNode<KT>
where
    KT: KeyType,
{
    type KT = KT;

    fn new(key: KT) -> TrieNode<KT> {
        TrieNode {
            key,
            children: vec![],
        }
    }

    fn key(&self) -> &KT { &self.key }
}

impl<KT: KeyType> TrieFields for TrieNode<KT> {
    type NodeType = TrieNode<KT>;

    fn children_mut(&mut self) -> &mut Vec<TrieNode<KT>> { &mut self.children }

    fn children(&self) -> &Vec<TrieNode<KT>> { &self.children }
}

impl<KT: KeyType> Internal for TrieNode<KT> {}

// INDEXING

impl<KT> Index<usize> for TrieNode<KT>
where
    KT: KeyType,
{
    type Output = TrieNode<KT>;

    fn index(&self, index: usize) -> &Self::Output { &self.children()[index] }
}

impl<KT> IndexMut<usize> for TrieNode<KT>
where
    KT: KeyType,
{
    fn index_mut(&mut self, index: usize) -> &mut Self::Output { &mut self.children_mut()[index] }
}

/////////////////////
// === TESTING === //
/////////////////////

#[cfg(test)]
mod tests {

    use super::*;

    // Node implementation tests

    #[test]
    fn node_new() {
        let node = TrieNode::new(1);
        assert_eq!(node.key(), &1);
    }

    #[test]
    fn node_with_child() {
        let node = {
            let child = TrieNode::new(2);
            TrieNode {
                key: 1,
                children: vec![child],
            }
        };
        assert_eq!(node.key(), &1);
        assert_eq!(node.children()[0].key(), &2);
    }

    // TrieFields implementation tests

    #[test]
    fn node_size() {
        let mut node = TrieNode::new(1);
        node.children_mut().push(TrieNode::new(2));
        node.children_mut().push(TrieNode::new(3));
        assert_eq!(node.size(), 2);
    }

    #[test]
    fn node_height() {
        let mut node = TrieNode::new(1);
        node.children_mut().push(TrieNode::new(2));
        assert_eq!(node.height(), 1);
    }

    #[test]
    fn node_is_empty() {
        let node = TrieNode::new(1);
        assert!(node.is_empty());
    }

    // Internal implementation tests
    #[test]
    fn node_insert_linear() {
        let mut node = TrieNode::new(3);
        assert_eq!(node.size(), 0); // Check initial size
        assert_eq!(node.height(), 0); // Check initial height

        // Basic
        node.insert_internal(vec![2, 3, 1]);
        assert_eq!(node[0].key(), &2);
        assert_eq!(node[0][0].key(), &3);
        assert_eq!(node[0][0][0].key(), &1);

        // First level

        // Left Top
        node.insert_internal(vec![1, 3, 4]);
        assert_eq!(node[0].key(), &1);
        assert_eq!(node[0][0].key(), &3);
        assert_eq!(node[0][0][0].key(), &4);

        // Right top
        node.insert_internal(vec![3, 3, 4]);
        assert_eq!(node[2].key(), &3);
        assert_eq!(node[2][0].key(), &3);
        assert_eq!(node[2][0][0].key(), &4);
    }
}
