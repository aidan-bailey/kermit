pub mod tuple_trie;

#[cfg(test)]
mod tests {
    use crate::tuple_trie::{Children, Node, Trie};

    #[test]
    fn node_new() {
        let node = Node::new(1);
        assert_eq!(node.key(), &1);
    }

    #[test]
    fn node_with_keys() {
        let node = Node::with_keys(1, vec![2, 3, 1]);

        assert_eq!(node.key(), &1);
        assert_eq!(node.size(), 1);

        assert_eq!(node.children()[0].key(), &2);
        assert_eq!(node.children()[0].size(), 1);

        assert_eq!(node.children()[0].children()[0].key(), &3);
        assert_eq!(node.children()[0].children()[0].size(), 1);

        assert_eq!(node.children()[0].children()[0].children()[0].key(), &1);
        assert_eq!(node.children()[0].children()[0].children()[0].size(), 0);
    }

    #[test]
    fn node_is_empty() {
        let node = Node::new(1);
        assert!(node.is_empty());
    }

    #[test]
    fn node_size() {
        let mut node = Node::new(1);
        node.children_mut().push(Node::new(2));
        node.children_mut().push(Node::new(3));
        assert_eq!(node.size(), 2);
    }

    #[test]
    fn node_height() {
        let mut node = Node::new(1);
        node.children_mut().push(Node::new(2));
        assert_eq!(node.height(), 1);
    }

    #[test]
    fn trie_new() {
        let empty_tri = Trie::<u64>::new();
        assert!(empty_tri.is_empty());
    }

    #[test]
    fn trie_insert() {
        let mut trie = Trie::<u64>::new();

        trie.insert(vec![1, 2]);

        assert_eq!(trie.size(), 1);
        // check first level child
        let child = &trie.children()[0];
        assert_eq!(child.key(), &1);
        assert_eq!(child.size(), 1);
        // check second level child
        let child = &child.children()[0];
        assert_eq!(child.key(), &2);
        assert_eq!(child.size(), 0);

        trie.insert(vec![0, 2]);

        assert_eq!(trie.size(), 2);
        // check first level child
        let child = &trie.children()[0];
        assert_eq!(child.key(), &0);
        assert_eq!(child.size(), 1);
        // check second level child
        let child = &child.children()[0];
        assert_eq!(child.key(), &2);
        assert_eq!(child.size(), 0);

        trie.insert(vec![1, 1]);

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
        let mut trie = Trie::<u64>::new();

        trie.insert(vec![1, 2]);
        trie.insert(vec![0, 2]);
        trie.insert(vec![1, 1]);

        assert!(trie.search(vec![1, 2]).is_some());
        assert!(trie.search(vec![0, 2]).is_some());
        assert!(trie.search(vec![1, 1]).is_some());
        assert!(trie.search(vec![0, 1]).is_none());
    }

    #[test]
    fn trie_remove() {
        let mut trie = Trie::<u64>::new();

        trie.insert(vec![1, 2]);
        trie.insert(vec![0, 2]);
        trie.insert(vec![1, 1]);

        assert!(trie.search(vec![1, 2]).is_some());
        trie.remove(vec![1, 2]);
        assert!(trie.search(vec![1, 2]).is_none());
    }
}
