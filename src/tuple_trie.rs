use crate::node::{Internal, Node, TrieFields};

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

    pub fn insert(&mut self, keys: Vec<KT>) -> Result<(), &'static str> {
        if keys.len() != self.arity {
            return Err("Arity doesn't match.");
        }
        self.insert_deque(keys.into());
        Ok(())
    }

    pub fn search(&self, keys: Vec<KT>) -> Result<Option<&Node<KT>>, &'static str> {
        if keys.len() != self.arity {
            return Err("Arity doesn't match.");
        }
        Ok(self.search_deque(keys.into()))
    }

    pub fn remove(&mut self, keys: Vec<KT>) -> Result<(), &'static str> {
        if keys.len() != self.arity {
            return Err("Arity doesn't match.");
        }
        self.remove_deque(keys.into());
        Ok(())
    }
}

impl<KT: Ord> TrieFields<KT> for Trie<KT> {
    fn children(&self) -> &Vec<Node<KT>> {
        &self.children
    }
    fn arity(&self) -> usize {
        self.arity
    }
}

impl<KT: Ord> Internal<KT> for Trie<KT> {
    fn children_mut(&mut self) -> &mut Vec<Node<KT>> {
        &mut self.children
    }
}
