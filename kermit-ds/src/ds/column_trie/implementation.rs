use {
    crate::relation::{Relation, RelationHeader},
    kermit_iters::{JoinIterable, TrieIterable},
    std::fmt,
};

/// A single level of a [`ColumnTrie`].
///
/// Keys at this depth are stored in `data`. The `interval` array maps each
/// parent element (by its position in the parent layer's `data`) to the start
/// offset of its children within this layer's `data`. The children of parent
/// element `i` span `data[interval[i]..interval[i+1]]` (or to the end of
/// `data` for the last parent).
pub struct ColumnTrieLayer {
    /// Sorted keys at this trie depth.
    pub data: Vec<usize>,
    /// Maps each parent element to the start index of its children in `data`.
    pub interval: Vec<usize>,
}

impl ColumnTrieLayer {
    /// Returns the data index range `start..end` for the children of the
    /// element at `interval_index`.
    fn data_range(&self, interval_index: usize) -> std::ops::Range<usize> {
        let start = self.interval[interval_index];
        let end = if interval_index + 1 < self.interval.len() {
            self.interval[interval_index + 1]
        } else {
            self.data.len()
        };
        start..end
    }

    /// Inserts `key` at position `pos` in the data array and increments all
    /// interval entries after `interval_index` to account for the shift.
    fn insert_key_and_shift_intervals(&mut self, pos: usize, key: usize, interval_index: usize) {
        self.data.insert(pos, key);
        for j in (interval_index + 1)..self.interval.len() {
            self.interval[j] += 1;
        }
    }

    /// Adds an interval entry for a new child at position `i` in the next
    /// layer.
    fn add_interval(&mut self, i: usize) {
        if i == self.interval.len() {
            self.interval.push(self.data.len());
        } else {
            self.interval.insert(i, self.interval[i]);
        }
    }
}

/// A column-oriented trie that stores a relation as parallel arrays per
/// level.
///
/// Unlike [`TreeTrie`](crate::ds::TreeTrie), which uses pointer-based nodes,
/// `ColumnTrie` flattens each trie level into a `ColumnTrieLayer` with
/// `data` and `interval` arrays. This layout avoids per-node allocation
/// overhead and is more cache-friendly for large relations, at the cost of
/// more expensive inserts (keys in later layers must shift when earlier
/// layers grow).
///
/// # Invariants
///
/// - `layers.len() == header.arity()`.
/// - Each layer's `data` array is sorted ascending within every sibling
///   interval.
/// - `layers[i].interval.len()` equals the number of distinct keys in
///   `layers[i-1]` (or 1 for the root layer when non-empty).
/// - Every interval entry in layer `i` indexes into `layers[i].data` (except
///   that the last entry may equal `data.len()`).
///
/// # When to prefer
///
/// Prefer `ColumnTrie` for large, mostly-static relations where iteration
/// speed and compact layout matter. Prefer
/// [`TreeTrie`](crate::ds::TreeTrie) for small inputs or when you are
/// inserting one tuple at a time.
///
/// # Example
///
/// ```
/// use kermit_ds::{ColumnTrie, Relation};
///
/// let trie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
/// assert_eq!(trie.header().arity(), 2);
/// ```
pub struct ColumnTrie {
    header: RelationHeader,
    /// One layer per attribute/depth in the relation; `layers[i]` holds the
    /// keys found at column `i` of the tuples, grouped by parent.
    pub layers: Vec<ColumnTrieLayer>,
}

impl ColumnTrie {
    /// Returns a reference to the layer at the given depth.
    ///
    /// # Panics
    ///
    /// Panics if `layer_i >= self.layers.len()` (i.e. greater than or equal
    /// to the relation's arity).
    pub fn layer(&self, layer_i: usize) -> &ColumnTrieLayer { &self.layers[layer_i] }

    /// Walks down the layer hierarchy inserting one key per level. The
    /// `interval_index` tracks our position in each layer's interval array,
    /// identifying which parent group the new key belongs to.
    fn internal_insert(&mut self, tuple: &[usize]) -> bool {
        let arity = self.header().arity();
        let mut interval_index = 0;

        'layer_loop: for (layer_i, &k) in tuple.iter().enumerate() {
            let is_last_layer = layer_i == arity - 1;

            if self.layers[layer_i].data.is_empty() {
                self.layers[layer_i].data.push(k);
                self.layers[layer_i].interval.push(0);
                interval_index = 0;
                continue;
            }

            let range = self.layers[layer_i].data_range(interval_index);

            // Search for the key within the current interval's data range
            for i in range.clone() {
                if self.layers[layer_i].data[i] == k {
                    continue 'layer_loop;
                }
                if k < self.layers[layer_i].data[i] {
                    // Insert before the first larger key
                    self.layers[layer_i].insert_key_and_shift_intervals(i, k, interval_index);
                    if is_last_layer {
                        return true;
                    }
                    // Inserting at layer_i creates a new child group in layer_i+1
                    self.layers[layer_i + 1].add_interval(i);
                    interval_index = i;
                    continue 'layer_loop;
                }
            }

            // Key is larger than all existing keys in the interval — append
            let insert_pos = range.end;
            if insert_pos == self.layers[layer_i].data.len() {
                self.layers[layer_i].data.push(k);
            } else {
                self.layers[layer_i].insert_key_and_shift_intervals(insert_pos, k, interval_index);
            }
            if is_last_layer {
                return true;
            }
            // Appending at layer_i creates a new child group in layer_i+1
            self.layers[layer_i + 1].add_interval(insert_pos);
            interval_index = insert_pos;
        }
        true
    }
}

impl fmt::Display for ColumnTrie {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (layer_i, layer) in self.layers.iter().enumerate() {
            writeln!(f, "LAYER {layer_i}")?;
            write!(f, "Data: [")?;
            for (i, data) in layer.data.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{data}")?;
            }
            writeln!(f, "]")?;
            write!(f, "Interval: [")?;
            for (i, interval) in layer.interval.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{interval}")?;
            }
            writeln!(f, "]")?;
        }
        Ok(())
    }
}

impl JoinIterable for ColumnTrie {}

impl crate::relation::Projectable for ColumnTrie {
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

impl Relation for ColumnTrie {
    fn header(&self) -> &RelationHeader { &self.header }

    fn new(header: RelationHeader) -> Self {
        ColumnTrie {
            layers: (0..header.arity())
                .map(|_| ColumnTrieLayer {
                    data: vec![],
                    interval: vec![],
                })
                .collect::<Vec<_>>(),
            header,
        }
    }

    fn from_tuples(header: RelationHeader, mut tuples: Vec<Vec<usize>>) -> Self {
        if tuples.is_empty() {
            Self::new(header)
        } else {
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
                trie.insert(tuple);
            }
            trie
        }
    }

    /// Inserts a single tuple.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `tuple.len()` does not match the relation's
    /// arity. In release builds the check is elided and a mismatched tuple
    /// will produce a logically inconsistent trie.
    fn insert(&mut self, tuple: Vec<usize>) -> bool {
        debug_assert!(
            tuple.len() == self.header().arity(),
            "Tuple length must match the arity of the trie."
        );
        self.internal_insert(&tuple)
    }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) -> bool {
        for tuple in tuples {
            if !self.insert(tuple) {
                return false;
            }
        }
        true
    }
}

impl crate::heap_size::HeapSize for ColumnTrie {
    fn heap_size_bytes(&self) -> usize {
        let layers_vec_bytes = self.layers.capacity() * std::mem::size_of::<ColumnTrieLayer>();
        let layer_contents_bytes: usize = self
            .layers
            .iter()
            .map(|layer| {
                layer.data.capacity() * std::mem::size_of::<usize>()
                    + layer.interval.capacity() * std::mem::size_of::<usize>()
            })
            .sum();
        layers_vec_bytes + layer_contents_bytes
    }
}

#[cfg(test)]
mod tests {
    use {
        super::ColumnTrie,
        crate::relation::{Projectable, Relation as _},
        kermit_iters::TrieIterable,
    };

    #[test]
    fn test_insert() {
        let mut trie = ColumnTrie::new(2.into());
        trie.insert(vec![2, 3]);
        println!("{trie}");
        trie.insert(vec![3, 1]);
        println!("{trie}");
        trie.insert(vec![1, 2]);
        println!("{trie}");
        println!("potato")
    }

    #[test]
    fn test_project() {
        let mut trie = ColumnTrie::new(3.into());
        trie.insert(vec![1, 2, 3]);
        trie.insert(vec![4, 5, 6]);
        trie.insert(vec![7, 8, 9]);

        // Project to columns 0 and 2 (first and third columns)
        let projected = trie.project(vec![0, 2]);
        assert_eq!(projected.header().arity(), 2);

        // Collect all tuples from the projected relation using iterator
        let mut all_tuples: Vec<Vec<usize>> = projected.trie_iter().into_iter().collect();

        // Sort for comparison
        all_tuples.sort();
        assert_eq!(all_tuples, vec![vec![1, 3], vec![4, 6], vec![7, 9]]);
    }

    #[test]
    fn test_project_with_named_attributes() {
        // Create a relation with named attributes
        let header = crate::relation::RelationHeader::new_nameless(vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
        ]);
        let mut trie = ColumnTrie::new(header);
        trie.insert(vec![1, 2, 3]);
        trie.insert(vec![4, 5, 6]);

        // Project to columns 0 and 2 (first and third columns)
        let projected = trie.project(vec![0, 2]);
        assert_eq!(projected.header().arity(), 2);
        assert_eq!(projected.header().attrs(), &[
            "a".to_string(),
            "c".to_string()
        ]);

        // Collect all tuples from the projected relation using iterator
        let mut all_tuples: Vec<Vec<usize>> = projected.trie_iter().into_iter().collect();

        // Sort for comparison
        all_tuples.sort();
        assert_eq!(all_tuples, vec![vec![1, 3], vec![4, 6]]);
    }
}

#[cfg(test)]
mod heap_size_tests {
    use {
        super::*,
        crate::{HeapSize, Relation},
    };

    #[test]
    fn empty_column_trie_heap_size() {
        let trie = ColumnTrie::new(2.into());
        // Layers Vec is allocated with arity capacity, but data/interval Vecs are empty
        let expected = trie.layers.capacity() * std::mem::size_of::<ColumnTrieLayer>();
        assert_eq!(trie.heap_size_bytes(), expected);
    }

    #[test]
    fn single_tuple_column_trie_heap_size() {
        let trie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        assert!(trie.heap_size_bytes() > 0);
    }

    #[test]
    fn more_tuples_means_more_heap() {
        let small = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let large = ColumnTrie::from_tuples(2.into(), (0..100).map(|i| vec![i, i + 1]).collect());
        assert!(large.heap_size_bytes() > small.heap_size_bytes());
    }
}
