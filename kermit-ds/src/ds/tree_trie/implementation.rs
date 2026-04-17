use {
    crate::relation::{Relation, RelationHeader},
    kermit_iters::{JoinIterable, TrieIterable},
    std::ops::{Index, IndexMut},
};

/// Insert a tuple into a sorted list of children nodes, recursing for remaining
/// keys.
fn insert_into_children(children: &mut Vec<TrieNode>, tuple: Vec<usize>) -> bool {
    if tuple.is_empty() {
        return true;
    }

    let mut key_iter = tuple.into_iter();

    if let Some(key) = key_iter.next() {
        let insert_pos = children.binary_search_by(|node| node.key().cmp(&key));

        match insert_pos {
            | Ok(pos) => {
                // Key exists, continue with its children
                insert_into_children(children[pos].children_mut(), key_iter.collect())
            },
            | Err(pos) => {
                // Key doesn't exist, insert new node
                let mut new_node = TrieNode::new(key);
                insert_into_children(new_node.children_mut(), key_iter.collect());
                children.insert(pos, new_node);
                true
            },
        }
    } else {
        true
    }
}

/// A node in the pointer-based trie.
///
/// Each node stores a single `usize` key and owns a sorted list of child nodes.
/// Leaf nodes have an empty `children` vector.
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
}

impl Index<usize> for TrieNode {
    type Output = TrieNode;

    fn index(&self, index: usize) -> &Self::Output { &self.children[index] }
}

impl IndexMut<usize> for TrieNode {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output { &mut self.children[index] }
}

/// A pointer-based trie that stores a relation as a tree of `TrieNode`s.
///
/// Each tuple `[k₀, k₁, …, kₙ₋₁]` is encoded as a root-to-leaf path where
/// every level corresponds to one column. Children at each level are kept in
/// sorted order by key, so insertion uses binary search and
/// [`TreeTrieIter`](crate::ds::TreeTrie) can seek forward without backtracking
/// — the invariant [`LeapfrogTriejoinIter`] relies on.
///
/// # Invariants
///
/// - The depth of every root-to-leaf path equals
///   [`RelationHeader::arity`](crate::RelationHeader::arity).
/// - `TrieNode::children` at every level is sorted ascending by key with no
///   duplicates among siblings.
///
/// # When to prefer
///
/// Compared to [`ColumnTrie`](crate::ds::ColumnTrie), `TreeTrie` is simpler
/// and allocates a node per key — preferable for small relations, pedagogical
/// use, and tests. For large relations, the column-oriented layout in
/// [`ColumnTrie`](crate::ds::ColumnTrie) tends to be more cache-friendly. See
/// [`ARCHITECTURE.md`](https://github.com/AlexStrickland/kermit/blob/master/ARCHITECTURE.md)
/// for a deeper comparison.
///
/// # Example
///
/// ```
/// use kermit_ds::{Relation, TreeTrie};
///
/// let trie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
/// assert_eq!(trie.header().arity(), 2);
/// ```
///
/// [`LeapfrogTriejoinIter`]: https://docs.rs/kermit-algos
#[derive(Clone, Debug)]
pub struct TreeTrie {
    header: RelationHeader,
    children: Vec<TrieNode>,
}

impl TreeTrie {
    pub(crate) fn children(&self) -> &Vec<TrieNode> { &self.children }
}

impl Relation for TreeTrie {
    fn header(&self) -> &RelationHeader { &self.header }

    fn new(header: RelationHeader) -> Self {
        Self {
            header,
            children: vec![],
        }
    }

    /// Builds a `TreeTrie` from a batch of tuples.
    ///
    /// Tuples are sorted lexicographically before insertion, which is the most
    /// efficient input order for the sorted-children invariant.
    ///
    /// # Panics
    ///
    /// Panics if the input tuples have mixed arity, or if any tuple's arity
    /// does not match `header.arity()` (propagated from
    /// [`insert`](Self::insert)).
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

    /// Inserts a single tuple, preserving the sorted-children invariant.
    ///
    /// # Panics
    ///
    /// Panics if `tuple.len()` does not match the arity of the relation.
    fn insert(&mut self, tuple: Vec<usize>) -> bool {
        if tuple.len() != self.header().arity() {
            panic!("Arity doesn't match.");
        }
        insert_into_children(&mut self.children, tuple)
    }

    /// Inserts every tuple in `tuples`.
    ///
    /// # Panics
    ///
    /// Panics if any tuple's arity does not match the relation's arity
    /// (propagated from [`insert`](Self::insert)).
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

impl crate::heap_size::HeapSize for TreeTrie {
    fn heap_size_bytes(&self) -> usize {
        fn node_heap_bytes(node: &TrieNode) -> usize {
            let vec_capacity_bytes = node.children().capacity() * std::mem::size_of::<TrieNode>();
            vec_capacity_bytes + node.children().iter().map(node_heap_bytes).sum::<usize>()
        }

        let root_capacity_bytes = self.children().capacity() * std::mem::size_of::<TrieNode>();
        root_capacity_bytes + self.children().iter().map(node_heap_bytes).sum::<usize>()
    }
}

#[cfg(test)]
mod heap_size_tests {
    use {
        super::*,
        crate::{HeapSize, Relation},
    };

    #[test]
    fn empty_tree_trie_heap_size() {
        let trie = TreeTrie::new(2.into());
        assert_eq!(trie.heap_size_bytes(), 0);
    }

    #[test]
    fn single_tuple_tree_trie_heap_size() {
        let trie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        assert!(trie.heap_size_bytes() > 0);
    }

    #[test]
    fn more_tuples_means_more_heap() {
        let small = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let large =
            TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4], vec![
                3, 5,
            ]]);
        assert!(large.heap_size_bytes() > small.heap_size_bytes());
    }
}
