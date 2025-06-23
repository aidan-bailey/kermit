//! This module provides a [trie](https://en.wikipedia.org/wiki/Trie)-based implementation of a relation.

mod implementation;
pub use implementation::RelationTrie;
mod relation_trie_iter;
mod trie_node;
mod trie_traits;

#[cfg(test)]
mod tests {
    use {
        crate::{
            ds::relation_trie::{implementation::RelationTrie, trie_traits::TrieFields},
            relation::{Builder, Relation, RelationBuilder},
            shared::nodes::Node,
        },
        kermit_iters::{
            linear::LinearIterator,
            trie::{TrieIterable, TrieIterator},
        },
    };

    #[test]
    fn trie_new() {
        let empty_tri = RelationTrie::<u64>::new(1);
        assert_eq!(empty_tri.arity(), 1);
        assert!(empty_tri.is_empty());
    }

    #[test]
    fn trie_insert() {
        let mut trie = RelationTrie::<u64>::new(2);

        let _ = trie.insert(vec![1, 2]);

        assert_eq!(trie.size(), 1);
        // check first level child
        let child = &trie.children()[0];
        assert_eq!(child.key(), &1);
        assert_eq!(child.size(), 1);
        // check second level child
        let child = &child.children()[0];
        assert_eq!(child.key(), &2);
        assert_eq!(child.size(), 0);

        let _ = trie.insert(vec![0, 2]);

        assert_eq!(trie.size(), 2);
        // check first level child
        let child = &trie.children()[0];
        assert_eq!(child.key(), &0);
        assert_eq!(child.size(), 1);
        // check second level child
        let child = &child.children()[0];
        assert_eq!(child.key(), &2);
        assert_eq!(child.size(), 0);

        let _ = trie.insert(vec![1, 1]);

        assert_eq!(trie.size(), 2);
        // check first level child
        let child = &trie.children()[1];
        assert_eq!(child.key(), &1);
        assert_eq!(child.size(), 2);
        // check second level child
        let child = &child.children()[0];
        assert_eq!(child.key(), &1);
        assert_eq!(child.size(), 0);
    }

    #[test]
    fn linear_iterator() {
        let trie = Builder::<RelationTrie<u64>>::new(1)
            .add_tuple(vec![1])
            .add_tuple(vec![2])
            .add_tuple(vec![3])
            .add_tuple(vec![4])
            .add_tuple(vec![5])
            .build();
        let mut iter = trie.trie_iter();
        assert!(iter.key().is_none());
        assert!(iter.open());
        assert_eq!(iter.key(), Some(&1));
        assert_eq!(iter.next(), Some(&2));
        assert!(iter.seek(&4));
        assert_eq!(iter.key(), Some(&4));
        assert_eq!(iter.next(), Some(&5));
    }

    #[test]
    fn test_relation_trie() {
        let trie = RelationTrie::<u64>::builder(2)
            .add_tuples(vec![vec![2, 4], vec![3, 5]])
            .build();
        let mut iter = trie.trie_iter();

        assert!(iter.open());
        assert_eq!(iter.key(), Some(&2));
        assert!(iter.open());
        assert_eq!(iter.key(), Some(&4));

        assert!(iter.up());
        assert_eq!(iter.key(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert!(iter.open());
        assert_eq!(iter.key(), Some(&5));
    }

    #[test]
    fn trie_iterator() {
        let trie = Builder::<RelationTrie<u64>>::new(3)
            .add_tuple(vec![1, 3, 4])
            .add_tuple(vec![1, 3, 5])
            .add_tuple(vec![1, 4, 6])
            .add_tuple(vec![1, 4, 8])
            .add_tuple(vec![1, 4, 9])
            .add_tuple(vec![1, 5, 2])
            .add_tuple(vec![3, 5, 2])
            .build();
        let mut iter = trie.trie_iter();

        assert!(iter.open());
        assert_eq!(iter.key().unwrap(), &1);

        assert!(iter.open());
        assert_eq!(iter.key().unwrap(), &3);

        assert!(iter.open());
        assert_eq!(iter.key().unwrap(), &4);

        assert_eq!(iter.next().unwrap(), &5);

        assert!(iter.up());
        assert_eq!(iter.key().unwrap(), &3);

        assert_eq!(iter.next().unwrap(), &4);

        assert!(iter.open());
        assert_eq!(iter.key().unwrap(), &6);

        assert!(iter.seek(&9));
        assert!(iter.up());
        assert_eq!(iter.key().unwrap(), &4);
        assert!(iter.up());
        assert_eq!(iter.key().unwrap(), &1);
        assert_eq!(iter.next().unwrap(), &3);

        assert!(iter.open());
        assert_eq!(iter.key().unwrap(), &5);

        assert!(iter.open());
        assert_eq!(iter.key().unwrap(), &2);

        assert!(!iter.open());
    }
}
