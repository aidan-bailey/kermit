use {
    super::implementation::ColumnTrie,
    crate::relation::Relation,
    kermit_derive::IntoTrieIter,
    kermit_iters::{LinearIterator, TrieIterable, TrieIterator, TrieIteratorWrapper},
};

/// Iterator over a [`ColumnTrie`] that traverses the trie layer by layer.
///
/// The iterator maintains a position using three coordinates:
/// - `layer_number`: the current depth (0 = root/uninitialised, 1 = first data
///   layer, …).
/// - `interval_i`: index into the *current* layer's `interval` array,
///   identifying the parent element whose children we are scanning. The
///   interval array maps each parent element to the start offset of its
///   children in the layer's `data` array.
/// - `rel_data_i`: offset *within* the active `rel_data` slice (i.e. relative
///   to the interval bounds, not a global data index).
///
/// `open()` descends one layer: it computes `interval_i = parent_interval_start
/// + rel_data_i` to find the child interval, then slices the next layer's data
/// between that interval's start and end. `up()` reverses this by scanning the
/// parent layer's interval array to recover the previous `interval_i` and
/// `rel_data_i`.
#[derive(IntoTrieIter)]
pub struct ColumnTrieIter<'a> {
    /// Current depth in the trie (0 = root/uninitialised, 1..arity = data
    /// layers).
    layer_number: usize,
    /// Index into the current layer's `interval` array, identifying which
    /// parent element's children we are iterating over.
    interval_i: usize,
    /// Offset within `rel_data` — the position relative to the start of the
    /// current interval, not a global index into the layer's data.
    rel_data_i: usize,
    /// Slice of the current layer's data bounded by the active interval.
    /// `None` when positioned at the root (layer 0).
    rel_data: Option<&'a [usize]>,
    /// The trie being iterated.
    trie: &'a ColumnTrie,
}

impl<'a> ColumnTrieIter<'a> {
    pub fn new(trie: &'a ColumnTrie) -> Self {
        ColumnTrieIter {
            interval_i: 0,
            rel_data: None,
            rel_data_i: 0,
            layer_number: 0,
            trie,
        }
    }
}

impl LinearIterator for ColumnTrieIter<'_> {
    fn key(&self) -> Option<usize> {
        if let Some(data) = self.rel_data {
            data.get(self.rel_data_i).copied()
        } else {
            None
        }
    }

    fn next(&mut self) -> Option<usize> {
        if let Some(data) = self.rel_data {
            if self.rel_data_i >= data.len() {
                return None;
            }
            self.rel_data_i += 1;
            data.get(self.rel_data_i).copied()
        } else {
            None
        }
    }

    fn seek(&mut self, seek_key: usize) -> bool {
        if self.at_end() {
            return false;
        }
        if let Some(data) = self.rel_data {
            // Binary search within the remaining portion of the sorted slice
            // to find the first key >= seek_key.
            let remaining = &data[self.rel_data_i..];
            let offset = remaining.partition_point(|&k| k < seek_key);
            self.rel_data_i += offset;
            !self.at_end()
        } else {
            false
        }
    }

    fn at_end(&self) -> bool {
        if let Some(data) = self.rel_data {
            self.rel_data_i >= data.len()
        } else {
            true
        }
    }
}

impl TrieIterator for ColumnTrieIter<'_> {
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
            // Descend into the children of the element at the current position.
            // The global data index of the current element is the interval start
            // plus our relative offset. This becomes our interval index in the
            // next layer, whose interval array maps each parent data element to
            // the start of its children.
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

/// Implementation of the `TrieIterable` trait for `ColumnTrie`.
impl TrieIterable for ColumnTrie {
    fn trie_iter(&self) -> impl TrieIterator + IntoIterator<Item = Vec<usize>> {
        ColumnTrieIter::new(self)
    }
}
