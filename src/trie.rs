/// Trie node
pub struct Node {}

/// Trie root
pub struct Trie {
    children: Vec<Node>,
}

impl Trie {
    /// Construct an empty Trie
    pub fn new() -> Trie {
        Trie { children: vec![] }
    }
    /// Returns true iff the Trie has no children
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}
