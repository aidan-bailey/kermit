use {
    crate::relation::{Relation, RelationHeader},
    kermit_iters::{JoinIterable, TrieIterable},
    std::ops::{Index, IndexMut},
};

/// A node in the trie data structure.
#[derive(Clone, Debug)]
pub struct TrieNode {
    key: usize,
    children: Vec<TrieNode>,
}

impl TrieNode {
    pub(crate) fn new(key: usize) -> Self {
        Self {
            key,
            children: vec![],
        }
    }

    pub(crate) fn key(&self) -> usize { self.key }

    pub(crate) fn children(&self) -> &Vec<TrieNode> { &self.children }

    pub(crate) fn children_mut(&mut self) -> &mut Vec<TrieNode> { &mut self.children }

    pub(crate) fn insert_internal(&mut self, tuple: Vec<usize>) -> bool {
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

impl Index<usize> for TrieNode {
    type Output = TrieNode;

    fn index(&self, index: usize) -> &Self::Output { &self.children[index] }
}

impl IndexMut<usize> for TrieNode {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output { &mut self.children[index] }
}

/// Trie data structure for relations.
#[derive(Clone, Debug)]
pub struct TreeTrie {
    header: RelationHeader,
    children: Vec<TrieNode>,
}

impl TreeTrie {
    pub(crate) fn children(&self) -> &Vec<TrieNode> { &self.children }

    pub(crate) fn children_mut(&mut self) -> &mut Vec<TrieNode> { &mut self.children }

    pub(crate) fn insert_internal(&mut self, tuple: Vec<usize>) -> bool {
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

impl Relation for TreeTrie {
    fn header(&self) -> &RelationHeader { &self.header }

    fn new(header: RelationHeader) -> Self {
        Self {
            header,
            children: vec![],
        }
    }

    fn from_tuples(header: RelationHeader, mut tuples: Vec<Vec<usize>>) -> Self {
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

    fn insert(&mut self, tuple: Vec<usize>) -> bool {
        if tuple.len() != self.header().arity() {
            panic!("Arity doesn't match.");
        }
        self.insert_internal(tuple)
    }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) -> bool {
        for tuple in tuples {
            if !self.insert(tuple) {
                panic!("Failed to insert tuple.");
            }
        }
        true
    }
}

impl JoinIterable for TreeTrie {}

impl crate::relation::Projectable for TreeTrie {
    fn project(&self, columns: Vec<usize>) -> Self {
        // Create a new header based on the current header but with projected attributes
        let current_header = self.header();
        let projected_attrs: Vec<String> = columns
            .iter()
            .filter_map(|&col_idx| current_header.attrs().get(col_idx).cloned())
            .collect();

        let new_header = if projected_attrs.is_empty() {
            // If no named attributes, create a positional header
            crate::relation::RelationHeader::new_nameless_positional(columns.len())
        } else {
            // Create a header with the projected attributes
            crate::relation::RelationHeader::new_nameless(projected_attrs)
        };

        // Collect all tuples from the current relation using the iterator
        let all_tuples: Vec<Vec<usize>> = self.trie_iter().into_iter().collect();

        // Project each tuple to the specified columns
        let projected_tuples: Vec<Vec<usize>> = all_tuples
            .into_iter()
            .map(|tuple| columns.iter().map(|&col_idx| tuple[col_idx]).collect())
            .collect();

        // Create new relation from projected tuples
        Self::from_tuples(new_header, projected_tuples)
    }
}
