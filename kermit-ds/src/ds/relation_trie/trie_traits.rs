//! This module defines the traits for nodes and trie fields used in a trie
//! structure.

use crate::shared::nodes::Node;

/// The `TrieFields` trait defines the basic structure and operations for a trie
/// node.
pub trait TrieFields {
    type NodeType: Node + TrieFields;

    /// Returns a reference to the children of the node.
    fn children(&self) -> &Vec<Self::NodeType>;

    /// Returns a mutable reference to the children of the node.
    fn children_mut(&mut self) -> &mut Vec<Self::NodeType>;

    /// Returns true iff the Node has no children.
    fn is_empty(&self) -> bool { self.children().is_empty() }

    #[cfg(test)]
    /// Returns the node's number of children.
    fn size(&self) -> usize { self.children().len() }

    #[cfg(test)]
    /// Returns the height from the current node.
    fn height(&self) -> usize {
        if let Some(child) = self.children().first() {
            1 + child.height()
        } else {
            0
        }
    }
}

pub trait Internal: TrieFields {
    fn insert_internal(&mut self, tuple: Vec<<Self::NodeType as Node>::KT>) -> bool
    where
        Self::NodeType: TrieFields<NodeType = Self::NodeType>,
    {
        if tuple.is_empty() {
            return true;
        }

        let mut current_children = self.children_mut();

        for key in tuple.into_iter() {
            if current_children.is_empty() {
                current_children.push(Self::NodeType::new(key));
                current_children = current_children[0].children_mut();
            } else {
                for i in (0..current_children.len()).rev() {
                    if key == current_children[i].key() {
                        current_children = current_children[i].children_mut();
                        break;
                    } else if key > current_children[i].key() {
                        if i == current_children.len() - 1 {
                            current_children.push(Self::NodeType::new(key));
                            current_children = current_children[i + 1].children_mut();
                            break;
                        } else {
                            current_children.insert(i, Self::NodeType::new(key));
                            current_children = current_children[i].children_mut();
                            break;
                        }
                    } else if i == 0 {
                        current_children.insert(0, Self::NodeType::new(key));
                        current_children = current_children[0].children_mut();
                        break;
                    }
                }
            }
        }
        true
    }
}
