use {
    crate::relation::{Relation, RelationHeader},
    kermit_iters::{Joinable, KeyType},
    std::ops::{Index, IndexMut},
};

/// A node in the trie data structure.
#[derive(Clone, Debug)]
pub struct TrieNode<KT: KeyType> {
    key: KT,
    children: Vec<TrieNode<KT>>,
}

impl<KT: KeyType> TrieNode<KT> {
    pub(crate) fn new(key: KT) -> Self {
        Self {
            key,
            children: vec![],
        }
    }

    pub(crate) fn key(&self) -> KT { self.key }

    pub(crate) fn children(&self) -> &Vec<TrieNode<KT>> { &self.children }

    pub(crate) fn children_mut(&mut self) -> &mut Vec<TrieNode<KT>> { &mut self.children }

    pub(crate) fn insert_internal(&mut self, tuple: Vec<KT>) -> bool {
        if tuple.is_empty() {
            return true;
        }

        let current_children = self.children_mut();
        let mut key_iter = tuple.into_iter();

        if let Some(key) = key_iter.next() {
            // Find insertion point or existing node
            let insert_pos = current_children.binary_search_by(|node| node.key().cmp(&key));

            match insert_pos {
                | Ok(pos) => {
                    // Key exists, continue with its children
                    current_children[pos].insert_internal(key_iter.collect())
                },
                | Err(pos) => {
                    // Key doesn't exist, insert new node
                    let mut new_node = TrieNode::new(key);
                    new_node.insert_internal(key_iter.collect());
                    current_children.insert(pos, new_node);
                    true
                },
            }
        } else {
            true
        }
    }
}

impl<KT: KeyType> Index<usize> for TrieNode<KT> {
    type Output = TrieNode<KT>;

    fn index(&self, index: usize) -> &Self::Output { &self.children[index] }
}

impl<KT: KeyType> IndexMut<usize> for TrieNode<KT> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output { &mut self.children[index] }
}

/// Trie data structure for relations.
#[derive(Clone, Debug)]
pub struct TreeTrie<KT: KeyType> {
    header: RelationHeader,
    children: Vec<TrieNode<KT>>,
}

impl<KT: KeyType> TreeTrie<KT> {
    pub(crate) fn children(&self) -> &Vec<TrieNode<KT>> { &self.children }

    pub(crate) fn children_mut(&mut self) -> &mut Vec<TrieNode<KT>> { &mut self.children }

    pub(crate) fn insert_internal(&mut self, tuple: Vec<KT>) -> bool {
        if tuple.is_empty() {
            return true;
        }

        let current_children = self.children_mut();
        let mut key_iter = tuple.into_iter();

        if let Some(key) = key_iter.next() {
            // Find insertion point or existing node
            let insert_pos = current_children.binary_search_by(|node| node.key().cmp(&key));

            match insert_pos {
                | Ok(pos) => {
                    // Key exists, continue with its children
                    current_children[pos].insert_internal(key_iter.collect())
                },
                | Err(pos) => {
                    // Key doesn't exist, insert new node
                    let mut new_node = TrieNode::new(key);
                    new_node.insert_internal(key_iter.collect());
                    current_children.insert(pos, new_node);
                    true
                },
            }
        } else {
            true
        }
    }
}

impl<KT: KeyType> Relation for TreeTrie<KT> {
    fn header(&self) -> &RelationHeader { &self.header }

    fn new(header: RelationHeader) -> Self {
        Self {
            header,
            children: vec![],
        }
    }

    fn from_tuples(header: RelationHeader, mut tuples: Vec<Vec<KT>>) -> Self {
        if tuples.is_empty() {
            return Self::new(header);
        }

        let arity = tuples[0].len();
        assert!(tuples.iter().all(|tuple| tuple.len() == arity));

        // Sort tuples for efficient insertion
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

        let mut trie = Self::new(header);
        for tuple in tuples {
            if !trie.insert(tuple) {
                panic!("Failed to build from tuples.");
            }
        }
        trie
    }

    fn insert(&mut self, tuple: Vec<KT>) -> bool {
        if tuple.len() != self.header().arity() {
            panic!("Arity doesn't match.");
        }
        self.insert_internal(tuple)
    }

    fn insert_all(&mut self, tuples: Vec<Vec<KT>>) -> bool {
        for tuple in tuples {
            if !self.insert(tuple) {
                panic!("Failed to insert tuple.");
            }
        }
        true
    }
}

impl<KT: KeyType> Joinable for TreeTrie<KT> {
    type KT = KT;
}
