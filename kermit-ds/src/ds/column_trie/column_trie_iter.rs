use {
    super::implementation::ColumnTrie,
    crate::relation::Relation,
    kermit_derive::IntoTrieIter,
    kermit_iters::{LinearIterator, TrieIterable, TrieIterator, TrieIteratorWrapper},
};

/// Iterator over a [`ColumnTrie`].
///
/// # Position model
///
/// The iterator's position is the triple `(depth, interval_i, rel_data_i)`:
///
/// - `depth` — depth in the trie. `0` = root/uninitialised; `1..=arity`
///   selects a data layer. The matching trie layer is
///   `trie.layer(depth - 1)`.
/// - `interval_i` — index into the current layer's `interval` array.
///   Identifies *which parent element's children* we are scanning (the
///   interval array maps each parent in the layer above to the start of its
///   children in this layer's `data`).
/// - `rel_data_i` — offset within `rel_data`, the slice of `data` carved out
///   by the active interval. **Relative**, not a global data index.
///
/// `open()` descends one level: it derives the new `interval_i` from the
/// parent's interval start plus our current relative offset, then slices
/// the new layer's data between that interval's start and end. `up()`
/// reverses this by searching the parent layer's interval array for the
/// interval that owns our current global data index.
#[derive(IntoTrieIter)]
pub struct ColumnTrieIter<'a> {
    /// Current depth in the trie. `0` = root/uninitialised; `1..=arity`
    /// indexes into `trie.layer(depth - 1)`.
    depth: usize,
    /// Index into the current layer's `interval` array — selects which
    /// parent element's children we are iterating over.
    interval_i: usize,
    /// Offset within `rel_data` (relative to the interval start, not a
    /// global index).
    rel_data_i: usize,
    /// Slice of the current layer's `data` bounded by the active interval.
    /// `None` when positioned at the root (depth 0).
    rel_data: Option<&'a [usize]>,
    /// The trie being iterated.
    trie: &'a ColumnTrie,
}

impl<'a> ColumnTrieIter<'a> {
    /// Creates a new iterator positioned at the root (depth 0). Call
    /// [`open`](TrieIterator::open) to descend to the first data layer.
    pub fn new(trie: &'a ColumnTrie) -> Self {
        ColumnTrieIter {
            interval_i: 0,
            rel_data: None,
            rel_data_i: 0,
            depth: 0,
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
            // `rel_data` is sorted within each interval (ColumnTrie
            // invariant). For a sorted slice, `partition_point(|x| x <
            // target)` returns the index of the first element ≥ target —
            // exactly what `seek` needs.
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
        if self.depth == self.trie.header().arity() {
            // Already at a leaf — nothing to descend into.
            return false;
        }
        if self.depth == 0 {
            // Root → first data layer. An empty trie has an empty layer
            // with `intervals = []`, so guard against that before computing
            // the children slice (which would index `interval[0]`). When
            // non-empty, the first layer has a single interval at 0 and
            // its children-of-root slice is the entire `data` array.
            let first_layer = self.trie.layer(0);
            if first_layer.is_empty() {
                return false;
            }
            self.depth = 1;
            self.interval_i = 0;
            self.rel_data_i = 0;
            self.rel_data = Some(first_layer.child_data(0));
            return true;
        }

        // Descend one level. The element we sit on in the current layer is
        // at *global* data index `parent_start + rel_data_i`, where
        // `parent_start = interval[interval_i]` of the current layer. That
        // global index is exactly the interval index in the *child* layer:
        // each parent data element maps to one entry in the child's
        // interval array.
        //
        // Worked example. Depths 1 → 2 of:
        //
        //     layer 0: data=[10, 20]      interval=[0]
        //     layer 1: data=[1, 2, 3]     interval=[0, 2]
        //
        // Sitting on `20` at depth 1: interval_i=0, rel_data_i=1.
        // `parent_start = layer0.interval[0] = 0`, so the new interval_i
        // is `0 + 1 = 1`, pointing at `interval[1] = 2` in layer 1 — the
        // start of `20`'s children (the slice `[3]`).
        let parent_layer = self.trie.layer(self.depth - 1);
        let parent_start = parent_layer.intervals()[self.interval_i];
        self.interval_i = parent_start + self.rel_data_i;
        self.depth += 1;

        let child_layer = self.trie.layer(self.depth - 1);
        self.rel_data = Some(child_layer.child_data(self.interval_i));
        self.rel_data_i = 0;
        true
    }

    fn up(&mut self) -> bool {
        if self.depth == 0 {
            // Already at the root.
            return false;
        }
        if self.depth == 1 {
            // Returning to the root resets all coordinates.
            self.depth = 0;
            self.interval_i = 0;
            self.rel_data_i = 0;
            self.rel_data = None;
            return true;
        }

        self.depth -= 1;
        let parent_layer = self.trie.layer(self.depth - 1);

        // The element we were sitting on at the deeper level was at *global*
        // data index `interval_i` in the parent layer (this is the inverse
        // of the `parent_start + rel_data_i = interval_i` derivation in
        // `open`). To position the iterator we need to find which parent
        // interval owns this data index — i.e. the largest `i` such that
        // `parent.interval[i] <= data_index`. The interval array is sorted
        // ascending and starts at 0, so this lookup always succeeds.
        let data_index = self.interval_i;
        for (i, &start_index) in parent_layer.intervals().iter().enumerate() {
            if data_index < start_index {
                break;
            }
            self.interval_i = i;
        }

        let parent_start = parent_layer.intervals()[self.interval_i];
        self.rel_data = Some(parent_layer.child_data(self.interval_i));
        self.rel_data_i = data_index - parent_start;
        true
    }
}

/// Implementation of the `TrieIterable` trait for `ColumnTrie`.
impl TrieIterable for ColumnTrie {
    fn trie_iter(&self) -> impl TrieIterator + IntoIterator<Item = Vec<usize>> {
        ColumnTrieIter::new(self)
    }
}
