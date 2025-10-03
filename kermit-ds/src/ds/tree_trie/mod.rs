//! This module provides a [trie](https://en.wikipedia.org/wiki/Trie)-based implementation of a relation.

mod implementation;
mod tree_trie_iter;

pub use implementation::TreeTrie;

#[cfg(test)]
mod tests {
    use {
        super::implementation::*,
        crate::relation::{Builder, Projectable, Relation, RelationBuilder},
        kermit_iters::{LinearIterator, TrieIterable, TrieIterator},
    };

    #[test]
    fn trie_insert() {
        let mut trie = TreeTrie::<u64>::new(2.into());

        let _ = trie.insert(vec![1, 2]);

        assert_eq!(trie.children().len(), 1);
        // check first level child
        let child = &trie.children()[0];
        assert_eq!(child.key(), 1);
        assert_eq!(child.children().len(), 1);
        // check second level child
        let child = &child.children()[0];
        assert_eq!(child.key(), 2);
        assert_eq!(child.children().len(), 0);

        let _ = trie.insert(vec![0, 2]);

        assert_eq!(trie.children().len(), 2);
        // check first level child
        let child = &trie.children()[0];
        assert_eq!(child.key(), 0);
        assert_eq!(child.children().len(), 1);
        // check second level child
        let child = &child.children()[0];
        assert_eq!(child.key(), 2);
        assert_eq!(child.children().len(), 0);

        let _ = trie.insert(vec![1, 1]);

        assert_eq!(trie.children().len(), 2);
        // check first level child
        let child = &trie.children()[1];
        assert_eq!(child.key(), 1);
        assert_eq!(child.children().len(), 2);
        // check second level child
        let child = &child.children()[0];
        assert_eq!(child.key(), 1);
        assert_eq!(child.children().len(), 0);
    }

    #[test]
    fn linear_iterator() {
        let trie = Builder::<TreeTrie<u64>>::new(1.into())
            .add_tuple(vec![1])
            .add_tuple(vec![2])
            .add_tuple(vec![3])
            .add_tuple(vec![4])
            .add_tuple(vec![5])
            .build();
        let mut iter = trie.trie_iter();
        assert!(iter.key().is_none());
        assert!(iter.open());
        assert_eq!(iter.key(), Some(1));
        assert_eq!(iter.next(), Some(2));
        assert!(iter.seek(4));
        assert_eq!(iter.key(), Some(4));
        assert_eq!(iter.next(), Some(5));
    }

    #[test]
    fn test_tree_trie() {
        let trie = TreeTrie::<u64>::from_tuples(2.into(), vec![vec![2, 4], vec![3, 5]]);
        let mut iter = trie.trie_iter();

        assert!(iter.open());
        assert_eq!(iter.key(), Some(2));
        assert!(iter.open());
        assert_eq!(iter.key(), Some(4));

        assert!(iter.up());
        assert_eq!(iter.key(), Some(2));
        assert_eq!(iter.next(), Some(3));
        assert!(iter.open());
        assert_eq!(iter.key(), Some(5));
    }

    #[test]
    fn trie_iterator() {
        let trie = Builder::<TreeTrie<u64>>::new(3.into())
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
        assert_eq!(iter.key().unwrap(), 1);

        assert!(iter.open());
        assert_eq!(iter.key().unwrap(), 3);

        assert!(iter.open());
        assert_eq!(iter.key().unwrap(), 4);

        assert_eq!(iter.next().unwrap(), 5);

        assert!(iter.up());
        assert_eq!(iter.key().unwrap(), 3);

        assert_eq!(iter.next().unwrap(), 4);

        assert!(iter.open());
        assert_eq!(iter.key().unwrap(), 6);

        assert!(iter.seek(9));
        assert!(iter.up());
        assert_eq!(iter.key().unwrap(), 4);
        assert!(iter.up());
        assert_eq!(iter.key().unwrap(), 1);
        assert_eq!(iter.next().unwrap(), 3);

        assert!(iter.open());
        assert_eq!(iter.key().unwrap(), 5);

        assert!(iter.open());
        assert_eq!(iter.key().unwrap(), 2);

        assert!(!iter.open());
    }

    #[test]
    fn test_tree_trie_iter() {
        let trie =
            TreeTrie::<i32>::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4], vec![
                3, 5,
            ]]);
        let iter = trie.trie_iter();
        for v in iter {
            assert!(
                !v.is_empty(),
                "Each iteration should yield a non-empty vector."
            );
        }
    }

    #[test]
    fn test_project() {
        let trie =
            TreeTrie::<usize>::from_tuples(3.into(), vec![vec![1, 2, 3], vec![4, 5, 6], vec![
                7, 8, 9,
            ]]);

        // Project to columns 0 and 2 (first and third columns)
        let projected = trie.project(vec![0, 2]);
        assert_eq!(projected.header().arity(), 2);

        // Collect all tuples from the projected relation using iterator
        let mut all_tuples: Vec<Vec<usize>> = projected.trie_iter().into_iter().collect();

        // Sort for comparison
        all_tuples.sort();
        assert_eq!(all_tuples, vec![vec![1, 3], vec![4, 6], vec![7, 9]]);
    }

    #[test]
    fn test_project_with_named_attributes() {
        // Create a relation with named attributes
        let header = crate::relation::RelationHeader::new_nameless(vec![
            "x".to_string(),
            "y".to_string(),
            "z".to_string(),
        ]);
        let trie = TreeTrie::<usize>::from_tuples(header, vec![vec![1, 2, 3], vec![4, 5, 6]]);

        // Project to columns 0 and 2 (first and third columns)
        let projected = trie.project(vec![0, 2]);
        assert_eq!(projected.header().arity(), 2);
        assert_eq!(projected.header().attrs(), &[
            "x".to_string(),
            "z".to_string()
        ]);

        // Collect all tuples from the projected relation using iterator
        let mut all_tuples: Vec<Vec<usize>> = projected.trie_iter().into_iter().collect();

        // Sort for comparison
        all_tuples.sort();
        assert_eq!(all_tuples, vec![vec![1, 3], vec![4, 6]]);
    }
}
