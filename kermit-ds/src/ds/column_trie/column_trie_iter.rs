use {
    super::implementation::ColumnTrie,
    crate::relation::Relation,
    kermit_derive::IntoTrieIter,
    kermit_iters::{KeyType, LinearIterator, TrieIterable, TrieIterator, TrieIteratorWrapper},
};

#[derive(IntoTrieIter)]
pub struct ColumnTrieIter<'a, KT: KeyType> {
    /// Index of current layer.
    layer_number: usize,
    /// Index of interval start at current layer
    interval_i: usize,
    /// Relative index based on interval
    rel_data_i: usize,
    /// Relative data at current layer
    rel_data: Option<&'a [KT]>,
    /// The trie being iterated.
    trie: &'a ColumnTrie<KT>,
}

impl<'a, KT: KeyType> ColumnTrieIter<'a, KT> {
    pub fn new(trie: &'a ColumnTrie<KT>) -> Self {
        ColumnTrieIter {
            interval_i: 0,
            rel_data: None,
            rel_data_i: 0,
            layer_number: 0,
            trie,
        }
    }
}

impl<KT: KeyType> LinearIterator for ColumnTrieIter<'_, KT> {
    type KT = KT;

    fn key(&self) -> Option<Self::KT> {
        if let Some(data) = self.rel_data {
            data.get(self.rel_data_i).copied()
        } else {
            None
        }
    }

    fn next(&mut self) -> Option<Self::KT> {
        if let Some(data) = self.rel_data {
            if self.rel_data_i > data.len() - 1 {
                return None;
            }
            self.rel_data_i += 1;
            data.get(self.rel_data_i).copied()
        } else {
            None
        }
    }

    fn seek(&mut self, seek_key: Self::KT) -> bool {
        while let Some(key) = self.next() {
            if key >= seek_key {
                break;
            }
        }
        self.at_end()
    }

    fn at_end(&self) -> bool {
        if let Some(data) = self.rel_data {
            self.rel_data_i >= data.len()
        } else {
            true
        }
    }
}

impl<KT: KeyType> TrieIterator for ColumnTrieIter<'_, KT> {
    fn open(&mut self) -> bool {
        if self.layer_number == self.trie.header().arity() {
            // If at leaf, return false
            false
        } else if self.layer_number == 0 {
            let next_layer = self.trie.layer(0);
            if next_layer.data.is_empty() {
                // If the first layer is empty, we are in an empty trie and return false
                return false;
            }
            // If at root, initialize the first layer
            self.layer_number = 1;
            self.rel_data_i = 0;
            self.interval_i = 0;
            self.rel_data = Some(&self.trie.layer(0).data);
            true
        } else {
            // If not at root or leaf, compute next interval index
            let curr_layer = self.trie.layer(self.layer_number - 1);
            let prev_start_index = curr_layer.interval[self.interval_i];
            self.interval_i = prev_start_index + self.rel_data_i;
            // increment layer number
            self.layer_number += 1;
            // get new layer
            let next_layer = self.trie.layer(self.layer_number - 1);
            // get next start and end indices
            let next_start_index = next_layer.interval[self.interval_i];
            let next_end_index = if self.interval_i + 1 < next_layer.interval.len() {
                next_layer.interval[self.interval_i + 1]
            } else {
                next_layer.data.len()
            };
            // set new relative data
            self.rel_data = Some(&next_layer.data[next_start_index..next_end_index]);
            self.rel_data_i = 0;
            true
        }
    }

    fn up(&mut self) -> bool {
        if self.layer_number == 0 {
            // If already at root, cannot go up
            false
        } else if self.layer_number == 1 {
            // If moving to root, reset all indices
            self.layer_number = 0;
            self.interval_i = 0;
            self.rel_data_i = 0;
            self.rel_data = None;
            true
        } else {
            // If moving up, decrement layer index
            self.layer_number -= 1;
            let layer = self.trie.layer(self.layer_number - 1);
            // Our global data index is interval_i, so we must find the start index

            // Data index of parent is equivalent to current interval index
            let data_index = self.interval_i;
            // We need to find the interval index of the data in the previous layer
            // The start indexes are ordered, so we need to find the point at which the
            // data index is less than the current start index. Then the previous index
            // is our new interval index
            for (i, start_index) in layer.interval.iter().enumerate() {
                if data_index < *start_index {
                    break;
                } else {
                    self.interval_i = i;
                }
            }

            // Our new start index is at the new interval index
            let start_index = layer.interval[self.interval_i];
            // The end index is either the next start index, or the length of the data
            let end_index = if self.interval_i + 1 < layer.interval.len() {
                layer.interval[self.interval_i + 1]
            } else {
                layer.data.len()
            };

            // Set new relative data
            self.rel_data = Some(&layer.data[start_index..end_index]);
            self.rel_data_i = data_index - start_index;

            true
        }
    }
}

/// Implementation of the `TrieIterable` trait for `TreeTrie`.
impl<KT: KeyType> TrieIterable for ColumnTrie<KT> {
    fn trie_iter(&self) -> impl TrieIterator<KT = KT> + IntoIterator<Item = Vec<KT>> {
        ColumnTrieIter::new(self)
    }
}
