use {
    crate::relation::{Relation, RelationHeader},
    kermit_iters::{join_iterable::JoinIterable, key_type::KeyType},
    std::fmt,
};

pub struct ColumnTrieLayer<KT: KeyType> {
    pub data: Vec<KT>,
    pub interval: Vec<usize>,
}

pub struct ColumnTrie<KT: KeyType> {
    header: RelationHeader,
    pub layers: Vec<ColumnTrieLayer<KT>>,
}

impl<KT: KeyType> ColumnTrie<KT> {
    pub fn layer(&self, layer_i: usize) -> &ColumnTrieLayer<KT> { &self.layers[layer_i] }

    fn internal_insert(&mut self, tuple: &[KT]) -> bool {
        /// Adds an interval to a layer at some index.
        fn add_interval<KT: KeyType>(layer: &mut ColumnTrieLayer<KT>, i: usize) {
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

impl<KT: KeyType> fmt::Display for ColumnTrie<KT> {
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

impl<KT: KeyType> JoinIterable for ColumnTrie<KT> {
    type KT = KT;
}

impl<KT: KeyType> Relation for ColumnTrie<KT> {

    fn header(&self) -> &RelationHeader {
        &self.header
    }

    fn new(header: RelationHeader) -> Self {
        ColumnTrie {
            layers: (0..header.arity())
                .map(|_| ColumnTrieLayer::<KT> {
                    data: vec![],
                    interval: vec![],
                })
                .collect::<Vec<_>>(),
            header,
        }
    }

    fn from_tuples(header: RelationHeader, mut tuples: Vec<Vec<Self::KT>>) -> Self {
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

    fn insert(&mut self, tuple: Vec<Self::KT>) -> bool {
        debug_assert!(
            tuple.len() == self.header().arity(),
            "Tuple length must match the arity of the trie."
        );
        self.internal_insert(&tuple)
    }

    fn insert_all(&mut self, tuples: Vec<Vec<Self::KT>>) -> bool {
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
    use {super::ColumnTrie, crate::relation::{Relation as _, RelationHeader}};

    #[test]
    fn test_insert() {
        let mut trie = ColumnTrie::<usize>::new(2.into());
        trie.insert(vec![2, 3]);
        println!("{trie}");
        trie.insert(vec![3, 1]);
        println!("{trie}");
        trie.insert(vec![1, 2]);
        println!("{trie}");
        println!("potato")
    }
}
