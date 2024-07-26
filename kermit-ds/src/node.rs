use std::collections::VecDeque;
use std::ops::{Index, IndexMut};

/// Trie node
pub struct Node<KT: PartialOrd + PartialEq  + Clone> {
    /// Maximum height (1-based index of key in tuple)
    arity: usize,
    /// Key for tuple value
    key: KT,
    /// Children
    children: Vec<Node<KT>>,
}

impl<KT: PartialOrd + PartialEq  + Clone> Index<usize> for Node<KT> {
    type Output = Node<KT>;

    fn index(&self, index: usize) -> &Self::Output {
        &self.children()[index]
    }
}

impl<KT: PartialOrd + PartialEq  + Clone> IndexMut<usize> for Node<KT> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.children_mut()[index]
    }
}

impl<KT: PartialOrd + PartialEq  + Clone> Node<KT> {
    /// Construct a Node with a tuple-value key
    fn new(key: KT) -> Node<KT> {
        Node {
            arity: 0,
            key,
            children: vec![],
        }
    }

    /// Returns the Node's key
    pub fn key(&self) -> &KT {
        &self.key
    }
}

pub trait TrieFields<KT: PartialOrd + PartialEq  + Clone> {
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

impl<KT: PartialOrd + PartialEq  + Clone> TrieFields<KT> for Node<KT> {
    fn children(&self) -> &Vec<Node<KT>> {
        &self.children
    }
    fn arity(&self) -> usize {
        self.arity
    }
}

pub(crate) trait Internal<KT: PartialOrd + PartialEq  + Clone>: TrieFields<KT> {
    fn children_mut(&mut self) -> &mut Vec<Node<KT>>;

    fn insert_linear(&mut self, tuple: Vec<KT>) {
        if tuple.is_empty() {
            return;
        }

        let mut current_children = self.children_mut();

        for key in tuple.into_iter() {
            if current_children.is_empty() {
                current_children.push(Node::new(key));
                current_children = current_children[0].children_mut();
            } else {
                for i in (0..current_children.len()).rev() {
                    if &key == current_children[i].key() {
                        current_children = current_children[i].children_mut();
                        break;
                    } else if &key > current_children[i].key() {
                        if i == current_children.len() - 1 {
                            current_children.push(Node::new(key));
                            current_children = current_children[i + 1].children_mut();
                            break;
                        } else {
                            current_children.insert(i, Node::new(key));
                            current_children = current_children[i].children_mut();
                            break;
                        }
                    } else if i == 0 {
                        current_children.insert(0, Node::new(key));
                        current_children = current_children[0].children_mut();
                        break;
                    }
                }
            }
        }
    }


    fn insert_binary(&mut self, tuple: Vec<KT>) {
        if tuple.is_empty() {
            return;
        }

        let mut current_children = self.children_mut();

        for key in tuple.into_iter() {
            if current_children.is_empty() {
                current_children.push(Node::new(key));
                current_children = current_children[0].children_mut();
            } else {
                let mut l: usize = 0;
                let mut r: usize = current_children.len() - 1;
                while l <= r {
                    let m: usize = (l + r) / 2;
                    if current_children[m].key() < &key {
                        l = m + 1;
                    } else if current_children[m].key() > &key {
                        if m == 0 {
                            current_children.insert(0, Node::new(key));
                            current_children = current_children[0].children_mut();
                            break;
                        }
                        r = m - 1;
                    } else {
                        current_children = current_children[m].children_mut();
                        break;
                    }

                    if l > r {
                        if l < current_children.len() {
                            current_children.insert(l, Node::new(key));
                            current_children = current_children[l].children_mut();
                        } else {
                            current_children.push(Node::new(key));
                            current_children = current_children[l].children_mut();
                        }
                        break;
                    }
                }
            }
        }
    }

    fn search_linear(&self, tuple: Vec<KT>) -> Option<&Node<KT>> {

        if tuple.is_empty() {
            return None;
        }

        let mut current_children = self.children();

        for key in tuple.into_iter() {
            if current_children.is_empty() {
                return None;
            } else {
                for i in 0..current_children.len() {
                    if &key == current_children[i].key() {
                        if current_children[i].children().is_empty() {
                            return Some(&current_children[i]);
                        }
                        current_children = current_children[i].children();
                        break;
                    }
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

impl<KT: PartialOrd + PartialEq  + Clone> Internal<KT> for Node<KT> {
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
        let node = {
            let child = Node::new(2);
            Node {
                arity: child.arity() + 1,
                key: 1,
                children: vec![child],
            }
        };
        assert_eq!(node.key(), &1);
        assert_eq!(node.arity(), 1);
        assert_eq!(node.children()[0].key(), &2);
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
    fn node_insert_linear() {
        let mut node = Node::new(3);

        // Basic
        node.insert_linear(vec![2, 3, 1]);
        assert_eq!(node[0].key(), &2);
        assert_eq!(node[0][0].key(), &3);
        assert_eq!(node[0][0][0].key(), &1);

        // First level

        // Left Top
        node.insert_linear(vec![1, 3, 4]);
        assert_eq!(node[0].key(), &1);
        assert_eq!(node[0][0].key(), &3);
        assert_eq!(node[0][0][0].key(), &4);

        // Right top
        node.insert_linear(vec![3, 3, 4]);
        assert_eq!(node[2].key(), &3);
        assert_eq!(node[2][0].key(), &3);
        assert_eq!(node[2][0][0].key(), &4);
    }

    #[test]
    fn node_insert_binary() {
        let mut node = Node::new(3);

        // Basic
        node.insert_binary(vec![2, 3, 1]);
        assert_eq!(node[0].key(), &2);
        assert_eq!(node[0][0].key(), &3);
        assert_eq!(node[0][0][0].key(), &1);

        // First level

        // Left Top
        node.insert_binary(vec![1, 3, 4]);
        assert_eq!(node[0].key(), &1);
        assert_eq!(node[0][0].key(), &3);
        assert_eq!(node[0][0][0].key(), &4);

        // Right top
        node.insert_binary(vec![3, 3, 4]);
        assert_eq!(node[2].key(), &3);
        assert_eq!(node[2][0].key(), &3);
        assert_eq!(node[2][0][0].key(), &4);
    }

}
