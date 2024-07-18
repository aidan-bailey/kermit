use std::collections::VecDeque;

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
    pub fn with_keys_deque(key: KT, mut keys: VecDeque<KT>) -> Node<KT> {
        if let Some(next_key) = keys.pop_front() {
            let mut node = Node::new(key);
            let child = Node::with_keys_deque(next_key, keys);
            node.children_mut().push(child);
            node
        } else {
            Node::new(key)
        }
    }
    pub fn with_keys(key: KT, keys: Vec<KT>) -> Node<KT> {
        Node::with_keys_deque(key, keys.into())
    }
    pub fn key(&self) -> &KT {
        &self.key
    }
}

/// Trie root
pub struct Trie<KT: Ord> {
    arity: usize,
    children: Vec<Node<KT>>,
}

impl<KT: Ord> Trie<KT> {
    /// Construct an empty Trie
    pub fn new(arity: usize) -> Trie<KT> {
        Trie {
            arity,
            children: vec![],
        }
    }
    pub fn arity(&self) -> &usize {
        &self.arity
    }

    pub fn insert(&mut self, keys: Vec<KT>) {
        self.insert_deque(keys.into())
    }

    pub fn search(&self, keys: Vec<KT>) -> Option<&Node<KT>> {
        self.search_deque(keys.into())
    }

    pub fn remove(&mut self, keys: Vec<KT>) {
        self.remove_deque(keys.into())
    }
}

pub trait TrieFields<KT: Ord> {
    fn children(&self) -> &Vec<Node<KT>>;
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

impl<KT: Ord> TrieFields<KT> for Node<KT> {
    fn children(&self) -> &Vec<Node<KT>> {
        &self.children
    }
}

impl<KT: Ord> TrieFields<KT> for Trie<KT> {
    fn children(&self) -> &Vec<Node<KT>> {
        &self.children
    }
}

trait Internal<KT: Ord>: TrieFields<KT> {
    fn children_mut(&mut self) -> &mut Vec<Node<KT>>;

    fn insert_deque(&mut self, mut keys: VecDeque<KT>) {
        if let Some(key) = keys.pop_front() {
            if self.is_empty() {
                let node = Node::with_keys_deque(key, keys);
                self.children_mut().push(node);
            } else {
                let mut i = 0;
                for child in self.children_mut() {
                    if key == *child.key() {
                        return child.insert_deque(keys);
                    } else if key > *child.key() {
                        i += 1
                    } else {
                        let node = Node::with_keys_deque(key, keys);
                        self.children_mut().insert(i, node);
                        break;
                    }
                }
            }
        }
    }

    fn search_deque(&self, mut keys: VecDeque<KT>) -> Option<&Node<KT>> {
        if let Some(key) = keys.pop_front() {
            for child in self.children() {
                if key == *child.key() {
                    return if keys.is_empty() {
                        Some(&child)
                    } else {
                        child.search_deque(keys)
                    };
                }
            }
        }
        None
    }

    fn remove_deque(&mut self, mut keys: VecDeque<KT>) {
        if let Some(key) = keys.pop_front() {
            for i in 0..self.size() {
                let child = &mut self.children_mut()[i];
                if key == *child.key() {
                    child.remove_deque(keys);
                    if child.is_empty() {
                        self.children_mut().remove(i);
                    }
                    break;
                }
            }
        }
    }
}

impl<KT: Ord> Internal<KT> for Node<KT> {
    fn children_mut(&mut self) -> &mut Vec<Node<KT>> {
        &mut self.children
    }
}

impl<KT: Ord> Internal<KT> for Trie<KT> {
    fn children_mut(&mut self) -> &mut Vec<Node<KT>> {
        &mut self.children
    }
}
