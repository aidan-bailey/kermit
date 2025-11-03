use {
    crate::relation::{Relation, RelationHeader},
    kermit_iters::{Joinable, TrieIterable},
    std::fmt,
};

pub struct ColumnTrieLayer {
    pub data: Vec<usize>,
    pub interval: Vec<usize>,
}

pub struct ColumnTrie {
    header: RelationHeader,
    pub layers: Vec<ColumnTrieLayer>,
}

impl ColumnTrie {
    pub fn layer(&self, layer_i: usize) -> &ColumnTrieLayer { &self.layers[layer_i] }

    fn internal_insert(&mut self, tuple: &[usize]) -> bool {
        /// Adds an interval to a layer at some index.
        fn add_interval(layer: &mut ColumnTrieLayer, i: usize) {
            if i == layer.interval.len() {
                // If the index is greater than the length of the layer, we push a new interval
                layer.interval.push(layer.data.len());
            } else {
                // Otherwise, we insert the interval at the specified index
                layer.interval.insert(i, layer.interval[i]);
            }
        }

        let mut interval_index = 0;
        'layer_loop: for (layer_i, &k) in tuple.iter().enumerate() {
            // There are still keys to insert

            if self.layers[layer_i].data.is_empty() {
                // layer is empty, so we can just add the key and continue
                self.layers[layer_i].data.push(k);
                self.layers[layer_i].interval.push(0);
                interval_index = 0;
            } else {
                // layer is not empty, so we must find the place to insert it
                let start_index = self.layers[layer_i].interval[interval_index];
                let end_index = if interval_index == self.layers[layer_i].interval.len() - 1 {
                    self.layers[layer_i].data.len()
                } else {
                    self.layers[layer_i].interval[interval_index + 1]
                };

                for i in start_index..end_index {
                    if self.layers[layer_i].data[i] == k {
                        // key exists in data, so we can just continue
                        continue 'layer_loop;
                    } else if k < self.layers[layer_i].data[i] {
                        // we need to insert at position i
                        self.layers[layer_i].data.insert(i, k);
                        // now we increment all intervals after this index
                        for j in (interval_index + 1)..self.layers[layer_i].interval.len() {
                            self.layers[layer_i].interval[j] += 1;
                        }
                        // if this is the last layer, we're finished
                        if layer_i == self.header().arity() - 1 {
                            return true;
                        }
                        add_interval(&mut self.layers[layer_i + 1], i);
                        interval_index = i;
                        continue 'layer_loop;
                    }
                }

                // key is greater than all existing keys, so we add it to the end (at end index)
                if end_index == self.layers[layer_i].data.len() {
                    // if we're at the end, we have to push
                    self.layers[layer_i].data.push(k);
                } else {
                    // otherwise insert
                    self.layers[layer_i].data.insert(end_index, k);
                    // increment all intervals after this index
                    for j in interval_index + 1..self.layers[layer_i].interval.len() {
                        self.layers[layer_i].interval[j] += 1;
                    }
                }
                if layer_i == self.header().arity() - 1 {
                    // if there are no more layers, we are done
                    return true;
                }
                add_interval(&mut self.layers[layer_i + 1], end_index);
                interval_index = end_index;
            }
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

impl Joinable for ColumnTrie {}

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
                .map(|_| ColumnTrieLayer { data: vec![], interval: vec![] })
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
