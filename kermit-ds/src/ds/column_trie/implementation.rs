use {
    crate::relation::{Relation, RelationHeader},
    kermit_iters::JoinIterable,
    std::fmt,
};

/// A single level of a [`ColumnTrie`].
///
/// Keys at this depth are stored in `data`. The `interval` array maps each
/// parent element (by its position in the parent layer's `data`) to the start
/// offset of its children within this layer's `data`. The children of parent
/// element `i` span `data[interval[i]..interval[i+1]]` (or to the end of
/// `data` for the last parent).
///
/// The two backing `Vec`s are kept private so the cross-field invariants
/// (sortedness, interval-into-data) cannot be broken by external mutation.
/// All read access is through the methods below; mutation goes through
/// [`insert_key_and_shift_intervals`](Self::insert_key_and_shift_intervals)
/// and [`add_interval`](Self::add_interval).
pub struct ColumnTrieLayer {
    /// Sorted keys at this trie depth.
    data: Vec<usize>,
    /// Maps each parent element to the start index of its children in
    /// `data`.
    interval: Vec<usize>,
}

impl ColumnTrieLayer {
    /// Returns the data index range `start..end` for the children of the
    /// element at `interval_index`.
    pub(super) fn data_range(&self, interval_index: usize) -> std::ops::Range<usize> {
        let start = self.interval[interval_index];
        let end = if interval_index + 1 < self.interval.len() {
            self.interval[interval_index + 1]
        } else {
            self.data.len()
        };
        start..end
    }

    /// Returns the slice of `data` containing the children of the element at
    /// `interval_index`. Equivalent to `&self.data[self.data_range(i)]`.
    pub(super) fn child_data(&self, interval_index: usize) -> &[usize] {
        let range = self.data_range(interval_index);
        &self.data[range]
    }

    /// Returns the layer's interval array. Used by the iterator to recover
    /// the parent interval index on `up()`.
    pub(super) fn intervals(&self) -> &[usize] { &self.interval }

    /// Returns `true` if this layer holds no keys.
    pub(super) fn is_empty(&self) -> bool { self.data.is_empty() }

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
            // A freshly-branched parent starts with an empty child interval, so
            // it shares its successor's start offset until keys are inserted
            // under it — hence duplicating `interval[i]` at position `i`.
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
    /// keys found at column `i` of the tuples, grouped by parent. Private
    /// because the cross-layer invariants (`interval` lengths matching
    /// parent distinct-key counts, sortedness within sibling intervals)
    /// must not be mutated piecemeal — read access goes through
    /// [`ColumnTrie::layer`].
    layers: Vec<ColumnTrieLayer>,
    /// Number of distinct tuples stored; maintained by `insert`.
    tuple_count: usize,
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
    /// `interval_index` tracks our position in each layer's interval
    /// array, identifying which parent group the new key belongs to.
    ///
    /// At each layer, [`step_layer`](Self::step_layer) decides what
    /// happens: the duplicate-key case continues to the next layer at
    /// the existing key's position (so a tuple sharing a prefix with
    /// an already-inserted tuple still has its remaining levels
    /// inserted under the right parent); the last-layer case stops
    /// after placing the key; the insert / append cases hand back the
    /// new `interval_index` for the next layer.
    ///
    /// Returns `true` iff the tuple was not already present — i.e. at
    /// least one layer took an insert/append/empty-push branch rather
    /// than the equality branch. A duplicate tuple matches an existing
    /// key at every layer and this returns `false`.
    fn internal_insert(&mut self, tuple: &[usize]) -> bool {
        let arity = self.header().arity();
        let mut interval_index = 0;
        let mut all_matched = true;
        for (layer_i, &k) in tuple.iter().enumerate() {
            let is_last_layer = layer_i == arity - 1;
            let (step, matched_existing) =
                self.step_layer(layer_i, k, interval_index, is_last_layer);
            all_matched &= matched_existing;
            match step {
                | LayerStep::Stop => return !all_matched,
                | LayerStep::Recurse {
                    next_interval_index,
                } => interval_index = next_interval_index,
            }
        }
        !all_matched
    }

    /// Inserts `k` at `layer_i` within the parent group selected by
    /// `interval_index`. Returns [`LayerStep::Stop`] only when this
    /// is the last layer and the key has been placed. Returns
    /// [`LayerStep::Recurse`] in every other case — including when
    /// the key is already present (the existing key's position
    /// becomes the next layer's `interval_index`, so any remaining
    /// tuple components are inserted under the right parent).
    ///
    /// The second tuple element is `true` iff the key was already
    /// present in the parent group (the equality branch fired), and
    /// `false` for every insert / append / empty-push branch.
    ///
    /// Pre-condition: `interval_index` must be a valid index into
    /// `self.layers[layer_i]`'s interval-bookkeeping arrays for the
    /// parent group. Violating this panics on the inner index.
    fn step_layer(
        &mut self, layer_i: usize, k: usize, interval_index: usize, is_last_layer: bool,
    ) -> (LayerStep, bool) {
        if self.layers[layer_i].data.is_empty() {
            self.layers[layer_i].data.push(k);
            self.layers[layer_i].interval.push(0);
            return (
                LayerStep::Recurse {
                    next_interval_index: 0,
                },
                false,
            );
        }

        let range = self.layers[layer_i].data_range(interval_index);

        // Search for the key within the current interval's data range.
        for i in range.clone() {
            if self.layers[layer_i].data[i] == k {
                return (
                    LayerStep::Recurse {
                        next_interval_index: i,
                    },
                    true,
                );
            }
            if k < self.layers[layer_i].data[i] {
                self.layers[layer_i].insert_key_and_shift_intervals(i, k, interval_index);
                if is_last_layer {
                    return (LayerStep::Stop, false);
                }
                self.layers[layer_i + 1].add_interval(i);
                return (
                    LayerStep::Recurse {
                        next_interval_index: i,
                    },
                    false,
                );
            }
        }

        // Key is larger than all existing keys in the interval — append.
        let insert_pos = range.end;
        if insert_pos == self.layers[layer_i].data.len() {
            self.layers[layer_i].data.push(k);
        } else {
            self.layers[layer_i].insert_key_and_shift_intervals(insert_pos, k, interval_index);
        }
        if is_last_layer {
            return (LayerStep::Stop, false);
        }
        self.layers[layer_i + 1].add_interval(insert_pos);
        (
            LayerStep::Recurse {
                next_interval_index: insert_pos,
            },
            false,
        )
    }
}

/// Result of one layer step in [`ColumnTrie::internal_insert`]. The
/// caller's loop branches on this instead of using a labelled
/// `continue` from inside the inner search.
enum LayerStep {
    /// The key was placed and this was the last layer — no further
    /// layers should be visited.
    Stop,
    /// Continue to the next layer with the supplied `interval_index`.
    /// Used both when a fresh key was inserted (the next layer's
    /// parent group is the new key) and when the key was already
    /// present (the next layer's parent group is the existing key).
    Recurse {
        /// Index of the just-inserted key within `self.layers[layer_i].data`,
        /// which becomes the parent-group key for layer `layer_i + 1`.
        next_interval_index: usize,
    },
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
        crate::relation::project_via_trie_iter(self, columns)
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
            tuple_count: 0,
        }
    }

    fn from_tuples(header: RelationHeader, mut tuples: Vec<Vec<usize>>) -> Self {
        if tuples.is_empty() {
            Self::new(header)
        } else {
            let arity = tuples[0].len();
            assert_eq!(
                arity,
                header.arity(),
                "from_tuples: tuple arity {arity} does not match header arity {}",
                header.arity()
            );
            // Reproduces the derived `Vec<usize>` lexicographic order (kept
            // hand-rolled here rather than `sort_unstable()`).
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

    /// Inserts a single tuple. Duplicate tuples are silently absorbed.
    ///
    /// # Panics
    ///
    /// Panics if `tuple.len()` does not match the relation's arity.
    fn insert(&mut self, tuple: Vec<usize>) {
        assert_eq!(
            tuple.len(),
            self.header().arity(),
            "tuple arity must match relation arity"
        );
        if self.internal_insert(&tuple) {
            self.tuple_count += 1;
        }
    }

    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) {
        for tuple in tuples {
            self.insert(tuple);
        }
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

impl crate::cardinality::Cardinality for ColumnTrie {
    fn tuple_count(&self) -> usize { self.tuple_count }
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

    /// Pinning test: inserting into an empty trie creates one data slot
    /// and one interval at the root.
    #[test]
    fn empty_trie_first_insert_initialises_layer() {
        let mut trie = ColumnTrie::new(2.into());
        trie.insert(vec![5, 7]);
        let collected: Vec<Vec<usize>> = trie.trie_iter().into_iter().collect();
        assert_eq!(collected, vec![vec![5, 7]]);
    }

    /// Pinning test: a duplicate insert is a no-op (no new data, no
    /// shifted intervals). Without this pin, an extraction that returns
    /// the wrong "what to do next" variant could either double-insert or
    /// crash on the inner search.
    #[test]
    fn duplicate_insert_is_noop() {
        let mut trie = ColumnTrie::new(2.into());
        trie.insert(vec![1, 2]);
        trie.insert(vec![1, 2]);
        let collected: Vec<Vec<usize>> = trie.trie_iter().into_iter().collect();
        assert_eq!(collected, vec![vec![1, 2]]);
    }

    /// Pinning test: insert-before within the first interval at layer 0.
    /// `interval_index` stays 0 here because we're in the root.
    #[test]
    fn insert_before_first_key_in_first_interval() {
        let mut trie = ColumnTrie::new(2.into());
        trie.insert(vec![5, 50]);
        trie.insert(vec![3, 30]);
        let mut collected: Vec<Vec<usize>> = trie.trie_iter().into_iter().collect();
        collected.sort();
        assert_eq!(collected, vec![vec![3, 30], vec![5, 50]]);
    }

    /// Pinning test: insert-before that triggers
    /// `insert_key_and_shift_intervals` on a NON-FIRST interval. The
    /// shift must propagate to subsequent intervals' start positions.
    /// This is the canonical case the audit flagged.
    #[test]
    fn insert_before_in_non_first_interval_shifts_subsequent_intervals() {
        let mut trie = ColumnTrie::new(2.into());
        // Build a trie with two top-level groups: 1 -> {10, 20} and 5 -> {50, 60}
        trie.insert(vec![1, 10]);
        trie.insert(vec![1, 20]);
        trie.insert(vec![5, 50]);
        trie.insert(vec![5, 60]);
        // Now insert (1, 15) — slots in the middle of the first group's
        // child interval, which must shift the second group's child
        // interval start.
        trie.insert(vec![1, 15]);
        let mut collected: Vec<Vec<usize>> = trie.trie_iter().into_iter().collect();
        collected.sort();
        assert_eq!(collected, vec![
            vec![1, 10],
            vec![1, 15],
            vec![1, 20],
            vec![5, 50],
            vec![5, 60],
        ]);
    }

    /// Pinning test: append at the very end of the layer's `data` array
    /// uses `data.push()` (the line-170 path) rather than
    /// `insert_key_and_shift_intervals`. Confirms both paths produce
    /// equivalent observable output.
    #[test]
    fn append_at_end_of_data_extends_data() {
        let mut trie = ColumnTrie::new(2.into());
        trie.insert(vec![1, 10]);
        trie.insert(vec![2, 20]);
        trie.insert(vec![3, 30]);
        let mut collected: Vec<Vec<usize>> = trie.trie_iter().into_iter().collect();
        collected.sort();
        assert_eq!(collected, vec![vec![1, 10], vec![2, 20], vec![3, 30]]);
    }

    /// Pinning test: arity-1 trie exercises `is_last_layer == true` from
    /// the very first iteration; the function must `return` from inside
    /// the loop instead of falling through to `add_interval` on the
    /// non-existent next layer.
    #[test]
    fn single_layer_trie_inserts_without_panic() {
        let mut trie = ColumnTrie::new(1.into());
        trie.insert(vec![3]);
        trie.insert(vec![1]);
        trie.insert(vec![2]);
        let mut collected: Vec<Vec<usize>> = trie.trie_iter().into_iter().collect();
        collected.sort();
        assert_eq!(collected, vec![vec![1], vec![2], vec![3]]);
    }

    /// Pinning test: arity-3 trie with branching at every layer. Catches
    /// regressions where a refactor mishandles the interval_index handoff
    /// between layer N's insertion and layer N+1's `add_interval` call.
    #[test]
    fn arity_three_trie_with_branching_at_every_layer() {
        let mut trie = ColumnTrie::new(3.into());
        let tuples = vec![
            vec![1, 10, 100],
            vec![1, 10, 200],
            vec![1, 20, 100],
            vec![2, 10, 100],
            vec![2, 30, 300],
        ];
        for t in &tuples {
            trie.insert(t.clone());
        }
        let mut collected: Vec<Vec<usize>> = trie.trie_iter().into_iter().collect();
        collected.sort();
        let mut expected = tuples.clone();
        expected.sort();
        assert_eq!(collected, expected);
    }

    /// Round-trip: insert a fixed-seed pseudorandom set of tuples and
    /// verify `trie_iter().collect()` returns them sorted-and-deduped.
    /// Catches structural corruption that none of the targeted tests
    /// thought to check.
    #[test]
    fn random_inserts_round_trip_to_sorted_deduped_input() {
        // Linear-congruential PRNG so we don't add a `rand` dev-dep.
        // Seed and constants are arbitrary but fixed.
        let mut state: u64 = 0x00C0_FFEE_DEAD_BEEF_u64;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize
        };
        let arity = 3;
        let n = 500;
        let mut tuples: Vec<Vec<usize>> = (0..n)
            .map(|_| (0..arity).map(|_| next() % 50).collect())
            .collect();
        let mut trie = ColumnTrie::new(arity.into());
        for t in &tuples {
            trie.insert(t.clone());
        }
        // Expected: sorted, deduped.
        tuples.sort();
        tuples.dedup();
        let mut collected: Vec<Vec<usize>> = trie.trie_iter().into_iter().collect();
        collected.sort();
        assert_eq!(collected, tuples);
    }
}

#[cfg(test)]
mod cardinality_tests {
    use {super::*, crate::cardinality::Cardinality, kermit_iters::TrieIterable};

    #[test]
    fn empty_relation_has_zero_tuples() {
        let trie = ColumnTrie::new(2.into());
        assert_eq!(trie.tuple_count(), 0);
    }

    #[test]
    fn tuple_count_matches_iteration_count() {
        let trie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        assert_eq!(trie.tuple_count(), 3);
        assert_eq!(trie.trie_iter().into_iter().count(), 3);
    }

    #[test]
    fn duplicate_insert_does_not_inflate_count() {
        let mut trie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        trie.insert(vec![1, 2]); // exact duplicate — absorbed
        assert_eq!(trie.tuple_count(), 1);
        trie.insert(vec![1, 3]); // shared prefix, new tuple
        assert_eq!(trie.tuple_count(), 2);
        trie.insert(vec![0, 9]); // insert-before-existing path
        assert_eq!(trie.tuple_count(), 3);
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

    // `from_tuples` is a pure function of its inputs, so two builds from the
    // same tuples must produce the same heap layout. Pinning this rules out
    // hash-randomised allocation or capacity jitter sneaking in via a future
    // change to the construction path.
    #[test]
    fn heap_size_is_deterministic_across_rebuilds() {
        let tuples: Vec<Vec<usize>> = (0..50).map(|i| vec![i, i + 1]).collect();
        let a = ColumnTrie::from_tuples(2.into(), tuples.clone());
        let b = ColumnTrie::from_tuples(2.into(), tuples);
        assert_eq!(a.heap_size_bytes(), b.heap_size_bytes());
    }
}
