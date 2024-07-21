use std::collections::VecDeque;

/// Trie node
pub struct Node<KT: PartialOrd + PartialEq> {
    /// Maximum height (1-based index of key in tuple)
    arity: usize,
    /// Key for tuple value
    key: KT,
    /// Children
    children: Vec<Node<KT>>,
}

impl<KT: PartialOrd + PartialEq> Node<KT> {
    /// Construct a Node with a tuple-value key
    fn new(key: KT) -> Node<KT> {
        Node {
            arity: 0,
            key,
            children: vec![],
        }
    }

    /// Construct a Node with a tuple-value key and a child
    fn with_child(key: KT, child: Node<KT>) -> Node<KT> {
        Node {
            arity: child.arity() + 1,
            key,
            children: vec![child],
        }
    }

    /// Construct a Node with a tuple-value key and a child
    ///
    /// EXPECTS KEYS TO BE REVERSED!!!!
    fn with_reverse_keys(key: KT, mut keys: Vec<KT>) -> Node<KT> {
        if let Some(next_key) = keys.pop() {
            let child = Node::with_reverse_keys(next_key, keys);
            let node = Node::with_child(key, child);
            node
        } else {
            Node::new(key)
        }
    }

    fn with_deque_tuple(key: KT, mut tuple: VecDeque<KT>) -> Node<KT> {
        if let Some(next_key) = tuple.pop_back() {
            let child = Node::with_deque_tuple(next_key, tuple);
            let node = Node::with_child(key, child);
            node
        } else {
            Node::new(key)
        }
    }

    /// Returns the Node's key
    pub fn key(&self) -> &KT {
        &self.key
    }
}

pub trait TrieFields<KT: PartialOrd + PartialEq> {
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
    fn arity(&self) -> usize;
}

impl<KT: PartialOrd + PartialEq> TrieFields<KT> for Node<KT> {
    fn children(&self) -> &Vec<Node<KT>> {
        &self.children
    }
    fn arity(&self) -> usize {
        self.arity
    }
}

pub(crate) trait Internal<KT: PartialOrd + PartialEq>: TrieFields<KT> {
    fn children_mut(&mut self) -> &mut Vec<Node<KT>>;

    fn insert_deque(&mut self, mut keys: Vec<KT>) {
        if let Some(key) = keys.pop() {
            if self.is_empty() {
                let node = Node::with_reverse_keys(key, keys);
                self.children_mut().push(node);
            } else {
                let mut l: usize = 0;
                let mut r: usize = self.children().len() - 1;
                while l <= r {
                    let m: usize = (l + r) / 2;
                    if self.children()[m].key() < &key {
                        l = m + 1;
                    } else if self.children()[m].key() > &key {
                        if m == 0 {
                            self.children_mut()
                                .insert(m, Node::with_reverse_keys(key, keys));
                            return;
                        }
                        r = m - 1;
                    } else {
                        return self.children_mut()[m].insert_deque(keys);
                    }
                }

                if l < self.children().len() {
                    self.children_mut()
                        .insert(l, Node::with_reverse_keys(key, keys));
                } else {
                    self.children_mut().push(Node::with_reverse_keys(key, keys));
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

impl<KT: PartialOrd + PartialEq> Internal<KT> for Node<KT> {
    fn children_mut(&mut self) -> &mut Vec<Node<KT>> {
        &mut self.children
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    // Node implementation tests

    #[test]
    fn node_new() {
        let node = Node::new(1);
        assert_eq!(node.key(), &1);
        assert_eq!(node.arity(), 0);
    }

    #[test]
    fn node_with_child() {
        let node = Node::with_child(1, Node::new(2));
        assert_eq!(node.key(), &1);
        assert_eq!(node.arity(), 1);
        assert_eq!(node.children()[0].key(), &2);
    }

    #[test]
    fn node_with_keys_deque() {
        let node = Node::with_reverse_keys(1, vec![1, 3, 2].into());
        assert_eq!(node.key(), &1);
        assert_eq!(node.arity(), 3);
        assert_eq!(node.children()[0].key(), &2);
        assert_eq!(node.children()[0].arity(), 2);
        assert_eq!(node.children()[0].children()[0].key(), &3);
        assert_eq!(node.children()[0].children()[0].arity(), 1);
        assert_eq!(node.children()[0].children()[0].children()[0].key(), &1);
        assert_eq!(node.children()[0].children()[0].children()[0].arity(), 0);
    }

    // TrieFields implementation tests

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
    fn node_is_empty() {
        let node = Node::new(1);
        assert!(node.is_empty());
    }

    // Internal implementation tests

    #[test]
    fn node_insert_deque() {
        let mut node = Node::new(1);

        // Basic
        node.insert_deque(vec![1, 3, 2].into());
        assert_eq!(node.children()[0].key(), &2);
        assert_eq!(node.children()[0].arity(), 2);
        assert_eq!(node.children()[0].children()[0].key(), &3);
        assert_eq!(node.children()[0].children()[0].arity(), 1);
        assert_eq!(node.children()[0].children()[0].children()[0].key(), &1);
        assert_eq!(node.children()[0].children()[0].children()[0].arity(), 0);

        // First level

        // Left Top
        node.insert_deque(vec![4, 3, 1].into());
        assert_eq!(node.children()[0].key(), &1);
        assert_eq!(node.children()[0].arity(), 2);
        assert_eq!(node.children()[0].children()[0].key(), &3);
        assert_eq!(node.children()[0].children()[0].arity(), 1);
        assert_eq!(node.children()[0].children()[0].children()[0].key(), &4);
        assert_eq!(node.children()[0].children()[0].children()[0].arity(), 0);

        // Right top
        node.insert_deque(vec![4, 3, 3].into());
        assert_eq!(node.children()[2].key(), &3);
        assert_eq!(node.children()[2].arity(), 2);
        assert_eq!(node.children()[2].children()[0].key(), &3);
        assert_eq!(node.children()[2].children()[0].arity(), 1);
        assert_eq!(node.children()[2].children()[0].children()[0].key(), &4);
        assert_eq!(node.children()[2].children()[0].children()[0].arity(), 0);
    }
}
