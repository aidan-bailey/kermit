pub mod trie;

#[cfg(test)]
mod tests {
    use crate::trie::{Node, Trie, Children};

    #[test]
    fn node_new() {
        let node = Node::new(1);
        assert_eq!(node.key(), &1);
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

}
