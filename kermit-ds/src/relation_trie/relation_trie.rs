use crate::{
    key_type::KeyType,
    relation::Relation,
    relation_trie::trie::{Internal, TrieNode, TrieFields},
};

/// Trie data structure for relations.
#[derive(Clone, Debug)]
pub struct RelationTrie<KT>
where
    KT: KeyType,
{
    /// Cardinality of the trie.
    cardinality: usize,
    /// Children of the trie root.
    children: Vec<TrieNode<KT>>,
}

impl<KT: KeyType> Relation for RelationTrie<KT> {
    type KT = KT;

    fn cardinality(&self) -> usize { self.cardinality }

    fn insert(&mut self, tuple: Vec<KT>) -> bool {
        if tuple.len() != self.cardinality {
            panic!("Arity doesn't match.");
        }
        self.insert_internal(tuple)
    }

    fn insert_all(&mut self, tuples: Vec<Vec<KT>>) -> bool {
        for tuple in tuples {
            if !self.insert(tuple) {
                panic!("Failed to insert tuple.")
            }
        }
        true
    }
}

/// Trie implementation.
impl<KT> RelationTrie<KT>
where
    KT: KeyType,
{
    /// Construct an empty Trie.
    ///
    /// # Panics
    /// If `cardinality` is less than 1.
    pub fn new(cardinality: usize) -> RelationTrie<KT> {
        assert!(cardinality > 0, "Cardinality must be greater than 0.");
        RelationTrie {
            cardinality,
            children: vec![],
        }
    }

    /// Construct a Trie from a list of tuples.
    ///
    /// # Panics
    /// If any tuple does not have a matching `cardinality`.
    pub fn from_tuples(cardinality: usize, tuples: Vec<Vec<KT>>) -> RelationTrie<KT> {
        assert!(tuples.iter().all(|tuple| tuple.len() == cardinality));
        let mut trie = RelationTrie::new(cardinality);
        for tuple in tuples {
            if !trie.insert(tuple) {
                panic!("Failed to build from tuples.");
            }
        }
        trie
    }

    // TODO: Rename this method
    /// Construct a Trie from a list of tuples.
    ///
    /// Optimising the insertion through sorting the input tuples before
    /// constructing the Trie.
    ///
    /// # Panics
    /// If any tuple does not have a matching `cardinality`.
    pub fn from_mut_tuples(cardinality: usize, mut tuples: Vec<Vec<KT>>) -> RelationTrie<KT> {
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
        RelationTrie::from_tuples(cardinality, tuples)
    }
}

impl<KT: KeyType> TrieFields<KT> for RelationTrie<KT> {
    fn children(&self) -> &Vec<TrieNode<KT>> { &self.children }
}

impl<KT: KeyType> Internal<KT> for RelationTrie<KT> {
    fn children_mut(&mut self) -> &mut Vec<TrieNode<KT>> { &mut self.children }
}
