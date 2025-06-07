use {
    super::{
        trie_node::TrieNode,
        trie_traits::{Internal, TrieFields},
    },
    crate::relation::Relation,
    kermit_iters::{join_iterable::JoinIterable, key_type::KeyType},
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
    /// Construct an empty Trie.
    ///
    /// # Panics
    /// If `cardinality` is less than 1.
    fn new(cardinality: usize) -> Self {
        assert!(cardinality > 0, "Cardinality must be greater than 0.");
        RelationTrie {
            cardinality,
            children: vec![],
        }
    }

    /// Construct a Trie from a list of tuples.
    ///
    /// # Notes
    ///
    /// Optimising the insertion through sorting the input tuples before
    /// constructing the Trie.
    ///
    /// # Panics
    /// If any tuple does not have a matching `cardinality`.
    fn from_tuples(cardinality: usize, mut tuples: Vec<Vec<KT>>) -> Self {
        assert!(tuples.iter().all(|tuple| tuple.len() == cardinality));
        tuples.sort_unstable_by(|a, b| {
            for i in 0..a.len() {
                match a[i].cmp(&b[i]) {
                    | std::cmp::Ordering::Less => return std::cmp::Ordering::Less,
                    | std::cmp::Ordering::Greater => return std::cmp::Ordering::Greater,
                    | std::cmp::Ordering::Equal => continue,
                }
            }
            std::cmp::Ordering::Equal
        });
        let mut trie = RelationTrie::new(cardinality);
        for tuple in tuples {
            if !trie.insert(tuple) {
                panic!("Failed to build from tuples.");
            }
        }
        trie
    }

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

impl<KT: KeyType> JoinIterable for RelationTrie<KT> {
    type KT = KT;
}

/// Trie implementation.
impl<KT> RelationTrie<KT> where KT: KeyType {}

impl<KT: KeyType> TrieFields for RelationTrie<KT> {
    type NodeType = TrieNode<KT>;

    fn children(&self) -> &Vec<TrieNode<KT>> { &self.children }

    fn children_mut(&mut self) -> &mut Vec<TrieNode<KT>> { &mut self.children }
}

impl<KT: KeyType> Internal for RelationTrie<KT> {}
