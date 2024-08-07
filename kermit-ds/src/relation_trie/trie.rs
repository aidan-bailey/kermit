use {
    crate::relation_trie::node::{Internal, Node, TrieFields},
    std::ops::{Index, IndexMut},
};

/// Trie root
#[derive(Clone, Debug)]
pub struct RelationTrie<KT: PartialOrd + PartialEq + Clone> {
    cardinality: usize,
    children: Vec<Node<KT>>,
}

impl<KT: PartialOrd + PartialEq + Clone> Index<usize> for RelationTrie<KT> {
    type Output = Node<KT>;

    fn index(&self, index: usize) -> &Self::Output { &self.children()[index] }
}

impl<KT: PartialOrd + PartialEq + Clone> IndexMut<usize> for RelationTrie<KT> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output { &mut self.children_mut()[index] }
}

impl<KT: PartialOrd + PartialEq + Clone> RelationTrie<KT> {
    /// Construct an empty Trie
    pub fn new(cardinality: usize) -> RelationTrie<KT> {
        RelationTrie {
            cardinality,
            children: vec![],
        }
    }

    pub fn from_tuples(cardinality: usize, tuples: Vec<Vec<KT>>) -> RelationTrie<KT> {
        let mut trie = RelationTrie::new(cardinality);
        for tuple in tuples {
            trie.insert(tuple).unwrap();
        }
        trie
    }

    pub fn from_tuples_presort(cardinality: usize, mut tuples: Vec<Vec<KT>>) -> RelationTrie<KT> {
        tuples.sort_unstable_by(|a, b| {
            for i in 0..a.len() {
                if a[i] < b[i] {
                    return std::cmp::Ordering::Less;
                } else if a[i] > b[i] {
                    return std::cmp::Ordering::Greater;
                }
            }
            std::cmp::Ordering::Equal
        });
        let mut trie = RelationTrie::new(cardinality);
        for tuple in tuples {
            trie.insert(tuple).unwrap();
        }
        trie
    }

    pub fn insert(&mut self, tuple: Vec<KT>) -> Result<(), &'static str> {
        if tuple.len() != self.cardinality {
            return Err("Arity doesn't match.");
        }
        self.insert_linear(tuple);
        Ok(())
    }

    pub fn search(&self, tuple: Vec<KT>) -> Result<Option<&Node<KT>>, &'static str> {
        if tuple.len() != self.cardinality {
            return Err("Arity doesn't match.");
        }
        Ok(self.search_linear(tuple))
    }

    pub fn remove(&mut self, tuple: Vec<KT>) -> Result<(), &'static str> {
        if tuple.len() != self.cardinality {
            return Err("Arity doesn't match.");
        }
        self.remove_deque(tuple.into());
        Ok(())
    }
}

impl<KT: PartialOrd + PartialEq + Clone> TrieFields<KT> for RelationTrie<KT> {
    fn children(&self) -> &Vec<Node<KT>> { &self.children }

    fn cardinality(&self) -> usize { self.cardinality }
}

impl<KT: PartialOrd + PartialEq + Clone> Internal<KT> for RelationTrie<KT> {
    fn children_mut(&mut self) -> &mut Vec<Node<KT>> { &mut self.children }
}
