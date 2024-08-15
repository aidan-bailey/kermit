pub mod node;
pub mod trie;
pub mod trie_builder;
pub mod trie_iter;
pub mod variable_type;

#[cfg(test)]
mod tests {
    use {
        crate::{
            relation::Relation,
            relation_trie::{node::TrieFields, trie::RelationTrie, trie_builder::TrieBuilder},
        },
        kermit_iters::{
            linear::LinearIterator,
            trie::{TrieIterable, TrieIterator},
        },
    };

    #[test]
    fn trie_new() {
        let empty_tri = RelationTrie::<u64>::new(1);
        assert_eq!(empty_tri.cardinality(), 1);
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
    fn trie_iterator() {
        let trie = TrieBuilder::<u64>::new(3)
            .add_tuple(vec![1, 3, 4])
            .add_tuple(vec![1, 3, 5])
            .add_tuple(vec![1, 4, 6])
            .add_tuple(vec![1, 4, 8])
            .add_tuple(vec![1, 4, 9])
            .add_tuple(vec![1, 5, 2])
            .add_tuple(vec![3, 5, 2])
            .build();
        let mut iter = trie.trie_iter();

        assert_eq!(iter.open().unwrap(), &1);
        assert_eq!(iter.open().unwrap(), &3);
        assert_eq!(iter.open().unwrap(), &4);
        assert_eq!(iter.next().unwrap(), &5);
        assert_eq!(iter.up().unwrap(), &3);
        assert_eq!(iter.next().unwrap(), &4);
        assert_eq!(iter.open().unwrap(), &6);
        assert!(iter.seek(&9).is_some());
        assert_eq!(iter.up().unwrap(), &4);
        assert_eq!(iter.up().unwrap(), &1);
        assert_eq!(iter.next().unwrap(), &3);
        assert_eq!(iter.open().unwrap(), &5);
        assert_eq!(iter.open().unwrap(), &2);
        assert!(iter.open().is_none());
    }
}
