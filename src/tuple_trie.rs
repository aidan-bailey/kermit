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

    pub fn from_tuples(arity: usize, tuples: Vec<Vec<KT>>) -> Trie<KT> {
        let mut trie = Trie::new(arity);
        for tuple in tuples {
            trie.insert(tuple).unwrap();
        }
        trie
    }

    pub fn insert(&mut self, mut tuple: Vec<KT>) -> Result<(), &'static str> {
        if tuple.len() != self.arity {
            return Err("Arity doesn't match.");
        }
        tuple.reverse();
        self.insert_deque(tuple);
        Ok(())
    }

    pub fn search(&self, tuple: Vec<KT>) -> Result<Option<&Node<KT>>, &'static str> {
        if tuple.len() != self.arity {
            return Err("Arity doesn't match.");
        }
        Ok(self.search_deque(tuple.into()))
    }

    pub fn remove(&mut self, tuple: Vec<KT>) -> Result<(), &'static str> {
        if tuple.len() != self.arity {
            return Err("Arity doesn't match.");
        }
        self.remove_deque(tuple.into());
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
