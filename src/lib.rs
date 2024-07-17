pub mod trie;

#[cfg(test)]
mod tests {
    use crate::trie::{Node, Trie, Children};

    #[test]
    fn trie_new() {
        let empty_tri = Trie::<u64>::new();
        assert!(empty_tri.is_empty());
    }

    #[test]
    fn node_new() {
        let node = Node::new(1);
        assert_eq!(node.key(), &1);
    }

    #[test]
    fn node_children_mut() {
        let mut node = Node::new(1);
        node.children_mut().push(Node::new(2));
        assert_eq!(node.children().len(), 1);
        node.children_mut().clear();
        assert_eq!(node.children().len(), 0);
    }
}
