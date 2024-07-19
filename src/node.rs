use std::collections::VecDeque;

/// Trie node
pub struct Node<KT: Ord> {
    /// Maximum height (1-based index of key in tuple)
    arity: usize,
    /// Key for tuple value
    key: KT,
    /// Children
    children: Vec<Node<KT>>,
}

impl<KT: Ord> Node<KT> {
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

    fn with_tuple_deque(key: KT, mut tuple: VecDeque<KT>) -> Node<KT> {
        if let Some(next_key) = tuple.pop_front() {
            let child = Node::with_tuple_deque(next_key, tuple);
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
    fn arity(&self) -> usize;
}

impl<KT: Ord> TrieFields<KT> for Node<KT> {
    fn children(&self) -> &Vec<Node<KT>> {
        &self.children
    }
    fn arity(&self) -> usize {
        self.arity
    }
}

pub(crate) trait Internal<KT: Ord>: TrieFields<KT> {
    fn children_mut(&mut self) -> &mut Vec<Node<KT>>;

    fn insert_deque(&mut self, mut tuple: VecDeque<KT>) {
        if let Some(key) = tuple.pop_front() {
            if self.is_empty() {
                let node = Node::with_tuple_deque(key, tuple);
                self.children_mut().push(node);
            } else {
                let mut i = 0;
                for child in self.children_mut() {
                    if key == *child.key() {
                        return child.insert_deque(tuple);
                    } else if key > *child.key() {
                        i += 1
                    } else {
                        let node = Node::with_tuple_deque(key, tuple);
                        self.children_mut().insert(i, node);
                        return;
                    }
                }
                self.children_mut().push(Node::with_tuple_deque(key, tuple));
            }
        }
    }

    fn search_deque(&self, mut tuple: VecDeque<KT>) -> Option<&Node<KT>> {
        if let Some(key) = tuple.pop_front() {
            for child in self.children() {
                if key == *child.key() {
                    return if tuple.is_empty() {
                        Some(&child)
                    } else {
                        child.search_deque(tuple)
                    };
                }
            }
        }
        None
    }

    fn remove_deque(&mut self, mut tuple: VecDeque<KT>) {
        if let Some(key) = tuple.pop_front() {
            for i in 0..self.size() {
                let child = &mut self.children_mut()[i];
                if key == *child.key() {
                    child.remove_deque(tuple);
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

#[cfg(test)]
mod tests {

    use super::*;

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
    fn node_new() {
        let node = Node::new(1);
        assert_eq!(node.key(), &1);
        assert_eq!(node.arity(), 0);
    }

    #[test]
    fn node_with_tuple() {
        let node = Node::with_tuple_deque(1, vec![2, 3, 1].into());

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
}
