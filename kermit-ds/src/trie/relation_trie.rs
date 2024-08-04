use crate::relation::Relational;
use crate::trie::node::{Internal, Node, TrieFields};

/// Trie root
#[derive(Clone, Debug)]
pub struct RelationTrie<KT: PartialOrd + PartialEq + Clone> {
    cardinality: usize,
    children: Vec<Node<KT>>,
}

impl<KT: PartialOrd + PartialEq + Clone> Relational<KT> for RelationTrie<KT> {
    fn new(cardinality: usize) -> Self {
        RelationTrie {
            cardinality,
            children: vec![],
        }
    }

    fn cardinality(&self) -> usize {
        self.cardinality
    }

    fn tuples(&self) -> Vec<Vec<&KT>> {
        let mut tuples = vec![];
        for child in &self.children {
            tuples.extend(child.traverse());
        }
        tuples
    }

    fn remove(&mut self, tuple: Vec<KT>) -> bool {
        if tuple.len() != self.cardinality {
            panic!("Arity doesn't match.")
        }
        self.remove_deque(tuple.into())
    }

    fn insert(&mut self, tuple: Vec<KT>) -> bool {
        if tuple.len() != self.cardinality {
            panic!("Arity doesn't match.")
        }
        self.insert_linear(tuple)
    }

    fn clear(&mut self) {
        self.children.clear();
    }
}

impl<KT: PartialOrd + PartialEq + Clone> RelationTrie<KT> {
    pub fn from_tuples_presort(arity: usize, mut tuples: Vec<Vec<KT>>) -> RelationTrie<KT> {
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
        let mut trie = RelationTrie::new(arity);
        for tuple in tuples {
            trie.insert(tuple);
        }
        trie
    }

    pub fn search(&self, tuple: Vec<KT>) -> Result<Option<&Node<KT>>, &'static str> {
        if tuple.len() != self.cardinality {
            return Err("Arity doesn't match.");
        }
        Ok(self.search_linear(tuple))
    }
}

impl<KT: PartialOrd + PartialEq + Clone> TrieFields<KT> for RelationTrie<KT> {
    fn children(&self) -> &Vec<Node<KT>> {
        &self.children
    }
}

impl<KT: PartialOrd + PartialEq + Clone> Internal<KT> for RelationTrie<KT> {
    fn children_mut(&mut self) -> &mut Vec<Node<KT>> {
        &mut self.children
    }
}
