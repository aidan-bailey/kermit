use {
    crate::relation::Relation,
    kermit_iters::{join_iterable::JoinIterable, key_type::KeyType},
    std::fmt,
};

pub struct ColumnTrieLayer<KT: KeyType> {
    data: Vec<KT>,
    interval: Vec<usize>,
}

pub struct ColumnTrie<KT: KeyType> {
    arity: usize,
    layers: Vec<ColumnTrieLayer<KT>>,
}

impl<KT: KeyType> ColumnTrie<KT> {
    fn internal_insert(&mut self, interval_index: usize, tuples: &[KT]) -> bool {
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
                            // if not, we need to insert an interval in the next layer
                            let next_layer = &mut self.layers[layer_i + 1];
                            next_layer.interval.insert(i, next_layer.interval[i]);
                            // now we increment every interval in the next layer past this index
                            for j in (i + 1)..next_layer.interval.len() {
                                next_layer.interval[j] += 1;
                            }
                            // finally we insert the next value from the tuple
                            next_layer.data.insert(next_layer.interval[i], rest[0]);
                            // now we continue
                            return self.internal_insert(i, rest)
                        }

                    }
                }

                // key is greater than all existing keys, so we add it to the end (at end index)
                if end_index == layer.data.len() {
                    // if we're at the end, we have to push
                    layer.data.push(*k);
                    if rest.is_empty() {
                        // if there are no more layers, we are done
                        return true;
                    }
                    // add another interval for the next layer
                    let next_layer = &mut self.layers[layer_i + 1];
                    next_layer.interval.push(next_layer.data.len());
                    // push the data in the next layer
                    //next_layer.data.push(rest[0]);
                    // now we continue
                    self.internal_insert(end_index, rest)
                } else {
                    // otherwise insert
                    layer.data.insert(end_index, *k);
                    // increment all intervals after this index
                    for j in (end_index + 1)..layer.interval.len() {
                        layer.interval[j] += 1;
                    }
                    if rest.is_empty() {
                        // if there are no more layers, we are done
                        return true;
                    }
                    // add another interval for the next layer
                    let next_layer = &mut self.layers[layer_i + 1];
                    next_layer.interval.insert(end_index, next_layer.interval[end_index]);
                    // increment all intervals after this index
                    for j in (end_index + 1)..next_layer.interval.len() {
                        next_layer.interval[j] += 1;
                    }
                    // insert the data in the next layer
                    next_layer.data.insert(end_index, rest[0]);
                    //now we continue
                    self.internal_insert(end_index, rest)
                }
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

    fn from_tuples(tuples: Vec<Vec<Self::KT>>) -> Self { todo!() }

    fn arity(&self) -> usize { self.arity }

    fn insert(&mut self, tuple: Vec<Self::KT>) -> bool { self.internal_insert(0, &tuple) }

    fn insert_all(&mut self, tuples: Vec<Vec<Self::KT>>) -> bool { todo!() }
}

#[cfg(test)]
mod tests {
    use {super::ColumnTrie, crate::relation::Relation as _, kermit_iters::key_type::KeyType};

    #[test]
    fn test_insert() {
        let mut trie = ColumnTrie::<usize>::new(3);
        assert!(trie.insert(vec![1, 2, 3]));
        println!("{trie}");
        assert!(trie.insert(vec![1, 2, 4]));
        println!("{trie}");
        assert!(trie.insert(vec![1, 3, 5]));
        println!("{trie}");
        assert!(trie.insert(vec![1, 3, 6]));
        println!("{trie}");
        assert!(trie.insert(vec![2, 1, 2]));
        println!("{trie}");
    }
}
