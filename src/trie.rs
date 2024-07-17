/// Trie node
pub struct Node<KT: Ord> {
    key: KT
}

impl<KT: Ord> Node<KT> {
    /// Construct a node with a key
    pub fn new(key: KT) -> Node<KT> {
        Node { key }
    }
    /// Get the key
    pub fn key(&self) -> &KT {
        &self.key
    }
}

/// Trie root
pub struct Trie<KT: Ord> {
    children: Vec<Node<KT>>,
}

impl<KT: Ord> Trie<KT> {
    /// Construct an empty Trie
    pub fn new() -> Trie<KT> {
        Trie { children: vec![] }
    }
    /// Returns true iff the Trie has no children
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}
