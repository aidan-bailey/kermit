pub mod tuple_trie;

#[cfg(test)]
mod tests {
    use crate::tuple_trie::{Trie, TrieFields};

    #[test]
    fn trie_new() {
        let empty_tri = Trie::<u64>::new(1);
        assert_eq!(empty_tri.arity(), 1);
        assert!(empty_tri.is_empty());
    }

    #[test]
    fn trie_insert() {
        let mut trie = Trie::<u64>::new(2);

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
    fn trie_search() {
        let mut trie = Trie::<u64>::new(2);

        let _ = trie.insert(vec![1, 2]);
        let _ = trie.insert(vec![0, 2]);
        let _ = trie.insert(vec![1, 1]);

        assert!(trie.search(vec![1, 2]).unwrap().is_some());
        assert!(trie.search(vec![0, 2]).unwrap().is_some());
        assert!(trie.search(vec![1, 1]).unwrap().is_some());
        assert!(trie.search(vec![0, 1]).unwrap().is_none());
    }

    #[test]
    fn trie_remove() {
        let mut trie = Trie::<u64>::new(2);

        let _ = trie.insert(vec![1, 2]);
        let _ = trie.insert(vec![0, 2]);
        let _ = trie.insert(vec![1, 1]);

        assert!(trie.search(vec![1, 2]).unwrap().is_some());
        let _ = trie.remove(vec![1, 2]);
        assert!(trie.search(vec![1, 2]).unwrap().is_none());
    }

}
