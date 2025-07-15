use {
    crate::relation::Relation,
    kermit_iters::{join_iterable::JoinIterable, key_type::KeyType},
    std::fmt,
};

pub struct ColumnTrieLayer<KT: KeyType> {
    pub data: Vec<KT>,
    pub interval: Vec<usize>,
}

pub struct ColumnTrie<KT: KeyType> {
    pub arity: usize,
    pub layers: Vec<ColumnTrieLayer<KT>>,
}

impl<KT: KeyType> ColumnTrie<KT> {
    pub fn layer(&self, layer_i: usize) -> &ColumnTrieLayer<KT> { &self.layers[layer_i] }

    fn internal_insert(&mut self, interval_index: usize, tuples: &[KT]) -> bool {
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

        let layer_i = self.arity - tuples.len();
        if let Some((k, rest)) = tuples.split_first() {
            // There are still keys to insert
            let layer = &mut self.layers[layer_i];

            if layer.data.is_empty() {
                // layer is empty, so we can just add the key and continue
                layer.data.push(*k);
                layer.interval.push(0);
                self.internal_insert(0, rest)
            } else {
                // layer is not empty, so we must find the place to insert it
                let start_index = layer.interval[interval_index];
                let end_index = if interval_index == layer.interval.len() - 1 {
                    layer.data.len()
                } else {
                    layer.interval[interval_index + 1]
                };

                for i in start_index..end_index {
                    if layer.data[i] == *k {
                        // key exists in data, so we can just continue
                        return self.internal_insert(i, rest);
                    } else if *k < layer.data[i] {
                        // we need to insert at position i
                        layer.data.insert(i, *k);
                        // now we increment all intervals after this index
                        for j in (interval_index + 1)..layer.interval.len() {
                            layer.interval[j] += 1;
                        }
                        // if this is the last layer, we're finished
                        return if rest.is_empty() {
                            true
                        } else {
                            add_interval(&mut self.layers[layer_i + 1], i);
                            return self.internal_insert(i, rest);
                        };
                    }
                }

                // key is greater than all existing keys, so we add it to the end (at end index)
                if end_index == layer.data.len() {
                    // if we're at the end, we have to push
                    layer.data.push(*k);
                } else {
                    // otherwise insert
                    layer.data.insert(end_index, *k);
                    // increment all intervals after this index
                    for j in interval_index + 1..layer.interval.len() {
                        layer.interval[j] += 1;
                    }
                }
                if rest.is_empty() {
                    // if there are no more layers, we are done
                    return true;
                }
                add_interval(&mut self.layers[layer_i + 1], end_index);
                self.internal_insert(end_index, rest)
            }
        } else {
            // If there are no keys to insert, we are done
            true
        }
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
    fn new(arity: usize) -> Self {
        ColumnTrie {
            arity,
            layers: (0..arity)
                .map(|_| ColumnTrieLayer::<KT> {
                    data: vec![],
                    interval: vec![],
                })
                .collect::<Vec<_>>(),
        }
    }

    fn from_tuples(tuples: Vec<Vec<Self::KT>>) -> Self {
        if tuples.is_empty() {
            Self::new(0)
        } else {
            let mut trie = Self::new(tuples[0].len());
            for tuple in tuples {
                trie.insert(tuple);
            }
            trie
        }
    }

    fn arity(&self) -> usize { self.arity }

    fn insert(&mut self, tuple: Vec<Self::KT>) -> bool {
        assert!(
            tuple.len() == self.arity,
            "Tuple length must match the arity of the trie."
        );
        self.internal_insert(0, &tuple)
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
    use {super::ColumnTrie, crate::relation::Relation as _, kermit_iters::key_type::KeyType};

    #[test]
    fn test_insert() {
        let mut trie = ColumnTrie::<usize>::new(2);
        trie.insert(vec![2, 3]);
        println!("{trie}");
        trie.insert(vec![3, 1]);
        println!("{trie}");
        trie.insert(vec![1, 2]);
        println!("{trie}");
        println!("potato")
    }
}
