use crate::relation_trie::node::{Internal, Node, TrieFields};

/// Trie data structure for relations.
#[derive(Clone, Debug)]
pub struct RelationTrie<KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    /// Cardinality of the trie.
    cardinality: usize,
    /// Children of the trie root.
    children: Vec<Node<KT>>,
}

/// Trie implementation.
impl<KT> RelationTrie<KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
    /// Construct an empty Trie.
    ///
    /// # Examples
    /// ```
    /// use trie_rs::relation_trie::RelationTrie;
    /// let trie = RelationTrie::<usize>::new(2);
    /// ```
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
    /// # Examples
    ///
    /// ```
    /// use trie_rs::relation_trie::RelationTrie;
    /// let trie = RelationTrie::<usize>::from_tuples(2, vec![vec![1, 2], vec![3, 4]]);
    /// ```
    ///
    /// # Panics
    /// If any tuple does not have a matching `cardinality`.
    pub fn from_tuples(cardinality: usize, tuples: Vec<Vec<KT>>) -> RelationTrie<KT> {
        assert!(tuples.iter().all(|tuple| tuple.len() == cardinality));
        let mut trie = RelationTrie::new(cardinality);
        for tuple in tuples {
            trie.insert(tuple).unwrap();
        }
        trie
    }

    // TODO: Rename this method
    /// Construct a Trie from a list of tuples.
    ///
    /// Optimising the insertion through sorting the input tuples before constructing the Trie.
    ///
    /// # Examples
    /// ```
    /// use trie_rs::relation_trie::RelationTrie;
    /// let trie = RelationTrie::<usize>::from_tuples_presort(2, vec![vec![1, 2], vec![3, 4]]);
    /// ```
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

    /// Insert a tuple into the Trie.
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
