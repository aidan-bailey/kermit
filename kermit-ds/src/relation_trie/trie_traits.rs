use crate::key_type::KeyType;

pub trait Node {
    type KT: KeyType;
    fn new(key: Self::KT) -> Self;
    fn key(&self) -> &Self::KT;
}

pub trait TrieFields {
    type NodeType: Node + TrieFields;

    /// Returns the children of the Node
    fn children(&self) -> &Vec<Self::NodeType>;

    fn children_mut(&mut self) -> &mut Vec<Self::NodeType>;

    /// Returns true iff the Node has no children
    fn is_empty(&self) -> bool { self.children().is_empty() }

    #[cfg(test)]
    fn size(&self) -> usize { self.children().len() }

    #[cfg(test)]
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
                    if &key == current_children[i].key() {
                        current_children = current_children[i].children_mut();
                        break;
                    } else if &key > current_children[i].key() {
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
