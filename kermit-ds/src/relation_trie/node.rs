use std::ops::{Index, IndexMut};

/// Trie node
#[derive(Clone, Debug)]
pub struct Node<KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    /// Maximum height (1-based index of key in tuple)
    cardinality: usize,
    /// Key for tuple value
    key: KT,
    /// Children
    children: Vec<Node<KT>>,
}

impl<KT> Index<usize> for Node<KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    type Output = Node<KT>;

    fn index(&self, index: usize) -> &Self::Output { &self.children()[index] }
}

impl<KT> IndexMut<usize> for Node<KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    fn index_mut(&mut self, index: usize) -> &mut Self::Output { &mut self.children_mut()[index] }
}

impl<KT> Node<KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    /// Construct a Node with a tuple-value key
    fn new(key: KT) -> Node<KT> {
        Node {
            cardinality: 0,
            key,
            children: vec![],
        }
    }

    /// Returns the Node's key
    pub fn key(&self) -> &KT { &self.key }
}

pub trait TrieFields<KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    fn children(&self) -> &Vec<Node<KT>>;
    /// Returns true iff the Node has no children
    fn is_empty(&self) -> bool { self.children().is_empty() }
    fn size(&self) -> usize { self.children().len() }
    fn height(&self) -> usize {
        if let Some(child) = self.children().first() {
            1 + child.height()
        } else {
            0
        }
    }
    fn cardinality(&self) -> usize;
}

impl<KT> TrieFields<KT> for Node<KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    fn children(&self) -> &Vec<Node<KT>> { &self.children }

    fn cardinality(&self) -> usize { self.cardinality }
}

pub(crate) trait Internal<KT>: TrieFields<KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    fn children_mut(&mut self) -> &mut Vec<Node<KT>>;

    fn insert_internal(&mut self, tuple: Vec<KT>) {
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
}

impl<KT> Internal<KT> for Node<KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    fn children_mut(&mut self) -> &mut Vec<Node<KT>> { &mut self.children }
}

#[cfg(test)]
mod tests {

    use super::*;

    // Node implementation tests

    #[test]
    fn node_new() {
        let node = Node::new(1);
        assert_eq!(node.key(), &1);
        assert_eq!(node.cardinality(), 0);
    }

    #[test]
    fn node_with_child() {
        let node = {
            let child = Node::new(2);
            Node {
                cardinality: child.cardinality() + 1,
                key: 1,
                children: vec![child],
            }
        };
        assert_eq!(node.key(), &1);
        assert_eq!(node.cardinality(), 1);
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
        node.insert_internal(vec![2, 3, 1]);
        assert_eq!(node[0].key(), &2);
        assert_eq!(node[0][0].key(), &3);
        assert_eq!(node[0][0][0].key(), &1);

        // First level

        // Left Top
        node.insert_internal(vec![1, 3, 4]);
        assert_eq!(node[0].key(), &1);
        assert_eq!(node[0][0].key(), &3);
        assert_eq!(node[0][0][0].key(), &4);

        // Right top
        node.insert_internal(vec![3, 3, 4]);
        assert_eq!(node[2].key(), &3);
        assert_eq!(node[2][0].key(), &3);
        assert_eq!(node[2][0][0].key(), &4);
    }
}
