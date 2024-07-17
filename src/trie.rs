/// Trie node
pub struct Node<KT: Ord> {
    key: KT,
    children: Vec<Node<KT>>,
}

impl<KT: Ord> Node<KT> {
    /// Construct a node with a key
    pub fn new(key: KT) -> Node<KT> {
        Node {
            key,
            children: vec![],
        }
    }
    pub fn key(&self) -> &KT {
        &self.key
    }
}

impl<KT: Ord> Children<KT> for Node<KT> {
    fn children(&self) -> &Vec<Node<KT>> {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Node<KT>> {
        &mut self.children
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
}

impl<KT: Ord> Children<KT> for Trie<KT> {
    fn children(&self) -> &Vec<Node<KT>> {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Node<KT>> {
        &mut self.children
    }
}

pub trait Children<KT: Ord> {
    fn children(&self) -> &Vec<Node<KT>>;
    fn children_mut(&mut self) -> &mut Vec<Node<KT>>;
    /// Returns true iff the Node has no children
    fn is_empty(&self) -> bool {
        self.children().is_empty()
    }
    fn size(&self) -> usize {
        self.children().len()
    }
    fn height(&self) -> usize {
        if let Some(child) = self.children().first() {
            1 + child.height()
        } else {
            0
        }
    }
}
