use {
    super::build_mode::ColumnTrieBuildMode,
    crate::relation::{BuildModeRelation, Relation, RelationHeader},
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
/// All read access is through the methods below. Incremental `insert`
/// mutates through
/// [`insert_key_and_shift_intervals`](Self::insert_key_and_shift_intervals)
/// and [`add_interval`](Self::add_interval); the bulk build only appends,
/// through [`push_key`](Self::push_key) and
/// [`open_interval`](Self::open_interval).
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

    /// Appends `key` to the end of this layer's data. Used by the bulk
    /// build, where every key arrives in its final position.
    fn push_key(&mut self, key: usize) { self.data.push(key); }

    /// Opens a new child interval at the current end of `data`: for a
    /// parent just appended to the layer above, or, on the root layer, the
    /// trie's single root interval.
    fn open_interval(&mut self) { self.interval.push(self.data.len()); }
}

/// A column-oriented trie that stores a relation as parallel arrays per
/// level.
///
/// Unlike [`TreeTrie`](crate::ds::TreeTrie), which uses pointer-based nodes,
/// `ColumnTrie` flattens each trie level into a `ColumnTrieLayer` with
/// `data` and `interval` arrays. This layout avoids per-node allocation
/// overhead and is more cache-friendly for large relations, at the cost of
/// more expensive incremental inserts: `insert` must shift the offsets of
/// later intervals when an earlier layer grows. `from_tuples` builds every
/// layer in one pass (under the default [`ColumnTrieBuildMode::Bulk`]) and
/// pays no such cost.
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
    /// Number of distinct tuples stored; maintained by `insert` and by the bulk
    /// build.
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
    /// Returns `true` iff the tuple was newly inserted — i.e. at least one
    /// layer took an insert/append/empty-push branch rather than the equality
    /// branch. A duplicate tuple matches an existing key at every layer and
    /// this returns `false`.
    fn internal_insert(&mut self, tuple: &[usize]) -> bool {
        let arity = self.header().arity();
        let mut interval_index = 0;
        // Set once any layer places a key rather than matching an existing
        // one. Tracked positively (vs. accumulating "all matched" and negating)
        // so the return value reads directly.
        let mut created = false;
        for (layer_i, &k) in tuple.iter().enumerate() {
            let is_last_layer = layer_i == arity - 1;
            let (step, matched_existing) =
                self.step_layer(layer_i, k, interval_index, is_last_layer);
            created |= !matched_existing;
            match step {
                | LayerStep::Stop => return created,
                | LayerStep::Recurse {
                    next_interval_index,
                } => interval_index = next_interval_index,
            }
        }
        created
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

    /// Builds a trie from sorted tuples by inserting them one at a time —
    /// the build before issue #84, kept as
    /// [`ColumnTrieBuildMode::Incremental`] so its measurements can be
    /// reproduced. Each `insert` scans its interval from the start, so this
    /// is O(n · a · b).
    fn from_sorted_by_insertion(header: RelationHeader, sorted: Vec<Vec<usize>>) -> Self {
        let mut trie = Self::new(header);
        for tuple in sorted {
            trie.insert(tuple);
        }
        trie
    }

    /// Builds a trie from tuples already sorted lexicographically, in one
    /// pass: append keys layer by layer, and open a new child interval
    /// wherever the prefix changes.
    ///
    /// Each tuple shares some leading keys with its predecessor; those keys
    /// are already stored. So the tuple appends one key to every layer from
    /// the first differing depth down, and every key it appends above the
    /// last layer is a new parent, whose child interval the layer below
    /// opens first. A tuple equal to its predecessor appends nothing.
    ///
    /// Produces exactly the arrays, and the capacities, of inserting the
    /// same tuples one at a time: on sorted input `insert` only ever
    /// appends, so the two builds perform the same pushes in the same
    /// order. Never pre-size these `Vec`s — `heap_size_bytes` sums their
    /// capacities.
    ///
    /// The input must be sorted; debug builds check it.
    ///
    /// O(n · a) for n tuples of arity a.
    fn from_sorted(header: RelationHeader, sorted: Vec<Vec<usize>>) -> Self {
        let mut trie = Self::new(header);
        if sorted.is_empty() {
            return trie;
        }
        let arity = trie.header.arity();
        // A non-empty trie's root layer holds exactly one interval.
        if let Some(root) = trie.layers.first_mut() {
            root.open_interval();
        }
        let mut previous: Option<Vec<usize>> = None;
        for tuple in sorted {
            // The depth at which this tuple leaves its predecessor's path:
            // the number of leading keys the two share.
            let divergence_depth = previous
                .as_deref()
                .map_or(0, |prev| common_prefix_len(prev, &tuple));
            if let Some(prev) = previous.as_deref() {
                debug_assert!(
                    prev <= tuple.as_slice(),
                    "from_sorted: tuples are not sorted"
                );
            }
            if divergence_depth == arity {
                // Equal to its predecessor, so already stored.
                continue;
            }
            for (depth, &key) in tuple.iter().enumerate().skip(divergence_depth) {
                let layer = &mut trie.layers[depth];
                if depth > divergence_depth {
                    // The key just appended one layer up is a new parent,
                    // so its children start here.
                    layer.open_interval();
                }
                layer.push_key(key);
            }
            trie.tuple_count += 1;
            previous = Some(tuple);
        }
        trie
    }
}

/// The number of leading positions at which `a` and `b` agree.
fn common_prefix_len(a: &[usize], b: &[usize]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
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
        /// Index within `self.layers[layer_i].data` of the key — whether
        /// freshly inserted or matched as already present — that becomes the
        /// parent-group key for layer `layer_i + 1`.
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

    /// Builds with the default [`ColumnTrieBuildMode`], `Bulk`.
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self {
        Self::from_tuples_with_build_mode(header, ColumnTrieBuildMode::default(), tuples)
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

impl BuildModeRelation for ColumnTrie {
    type BuildMode = ColumnTrieBuildMode;

    /// Sorts the tuples, then builds the layers by `mode`: one pass over
    /// the sorted tuples for `Bulk` (see `from_sorted`), one `insert` per
    /// tuple for `Incremental` (see `from_sorted_by_insertion`).
    ///
    /// # Panics
    ///
    /// Panics if any tuple's length does not equal `header.arity()`.
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: ColumnTrieBuildMode, mut tuples: Vec<Vec<usize>>,
    ) -> Self {
        let arity = header.arity();
        // Checked before the sort: its comparator indexes `b` by `a`'s
        // length, so a shorter tuple would panic there with an index error
        // instead of this message.
        for tuple in &tuples {
            assert_eq!(
                tuple.len(),
                arity,
                "from_tuples: tuple arity {} does not match header arity {arity}",
                tuple.len()
            );
        }
        // The derived `Vec<usize>` lexicographic order, kept hand-rolled as
        // in TreeTrie's `from_tuples`: the sort then costs the same in every
        // ColumnTrie build and in TreeTrie's, so a change in the `insertion`
        // metric measures the build routine alone.
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
        match mode {
            | ColumnTrieBuildMode::Incremental => Self::from_sorted_by_insertion(header, tuples),
            | ColumnTrieBuildMode::Bulk => Self::from_sorted(header, tuples),
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
        super::{ColumnTrie, ColumnTrieBuildMode},
        crate::{
            relation::{BuildModeRelation, Projectable, Relation as _},
            HeapSize,
        },
        kermit_iters::TrieIterable,
    };

    /// Linear-congruential generator, so the randomised tests need no `rand`
    /// dev-dependency. The constants are Knuth's MMIX ones.
    struct Lcg(u64);

    impl Lcg {
        fn next_usize(&mut self) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) as usize
        }
    }

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
        // Build a trie with two top-level groups: 1 -> {10, 20} and 5 -> {50,
        // 60}
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
        let mut rng = Lcg(0x00C0_FFEE_DEAD_BEEF_u64);
        let arity = 3;
        let n = 500;
        let mut tuples: Vec<Vec<usize>> = (0..n)
            .map(|_| (0..arity).map(|_| rng.next_usize() % 50).collect())
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

    /// The build before issue #84, kept as the oracle: one `insert` per
    /// tuple, in the order given.
    fn insert_one_by_one(arity: usize, tuples: &[Vec<usize>]) -> ColumnTrie {
        let mut trie = ColumnTrie::new(arity.into());
        for tuple in tuples {
            trie.insert(tuple.clone());
        }
        trie
    }

    /// Asserts two tries are identical down to each `Vec`'s capacity, which
    /// `heap_size_bytes` sums.
    fn assert_identical(actual: &ColumnTrie, expected: &ColumnTrie, case: &str) {
        assert_eq!(
            actual.layers.len(),
            expected.layers.len(),
            "{case}: layer count"
        );
        for (depth, (a, e)) in actual.layers.iter().zip(&expected.layers).enumerate() {
            assert_eq!(a.data, e.data, "{case}: layer {depth} data");
            assert_eq!(a.interval, e.interval, "{case}: layer {depth} interval");
            assert_eq!(
                a.data.capacity(),
                e.data.capacity(),
                "{case}: layer {depth} data capacity"
            );
            assert_eq!(
                a.interval.capacity(),
                e.interval.capacity(),
                "{case}: layer {depth} interval capacity"
            );
        }
        assert_eq!(
            actual.tuple_count, expected.tuple_count,
            "{case}: tuple_count"
        );
        assert_eq!(
            actual.heap_size_bytes(),
            expected.heap_size_bytes(),
            "{case}: heap_size_bytes"
        );
    }

    /// Every build mode builds exactly the same trie: the same arrays, the
    /// same capacities, the same count and heap size (issue #84). Small key
    /// ranges make duplicates and long shared prefixes common.
    #[test]
    fn bulk_and_incremental_builds_are_identical() {
        let seeds: &[u64] = if cfg!(miri) {
            &[1]
        } else {
            &[1, 2, 3, 0x00C0_FFEE_DEAD_BEEF]
        };
        let sizes: &[usize] = if cfg!(miri) {
            &[0, 1, 2, 17]
        } else {
            &[0, 1, 2, 17, 200]
        };
        for &seed in seeds {
            let mut rng = Lcg(seed);
            for arity in 1..=4 {
                for key_range in [2, 5, 50] {
                    for &n in sizes {
                        let tuples: Vec<Vec<usize>> = (0..n)
                            .map(|_| (0..arity).map(|_| rng.next_usize() % key_range).collect())
                            .collect();
                        let case = format!("seed {seed}, arity {arity}, keys < {key_range}, n {n}");
                        let bulk = ColumnTrie::from_tuples_with_build_mode(
                            arity.into(),
                            ColumnTrieBuildMode::Bulk,
                            tuples.clone(),
                        );
                        let incremental = ColumnTrie::from_tuples_with_build_mode(
                            arity.into(),
                            ColumnTrieBuildMode::Incremental,
                            tuples.clone(),
                        );
                        assert_identical(&bulk, &incremental, &case);
                        // The layout depends only on the tuple set, so
                        // inserting in arrival order agrees too.
                        assert_identical(
                            &bulk,
                            &insert_one_by_one(arity, &tuples),
                            &format!("{case}, unsorted inserts"),
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn bulk_build_of_duplicates_stores_one_tuple() {
        let tuples = vec![vec![3, 1, 4]; 5];
        let bulk = ColumnTrie::from_tuples(3.into(), tuples.clone());
        assert_eq!(bulk.tuple_count, 1);
        assert_identical(&bulk, &insert_one_by_one(3, &tuples), "all duplicates");
    }

    #[test]
    fn bulk_build_of_no_tuples_is_the_empty_trie() {
        let bulk = ColumnTrie::from_tuples(2.into(), vec![]);
        assert!(bulk
            .layers
            .iter()
            .all(|layer| layer.data.is_empty() && layer.interval.is_empty()));
        assert_identical(&bulk, &ColumnTrie::new(2.into()), "empty");
    }

    /// Arity 0 keeps the result it had before issue #84: no layers and a
    /// count of 0, because `insert` stores nothing for an empty tuple.
    #[test]
    fn bulk_build_of_nullary_tuples_stores_nothing() {
        let tuples = vec![vec![]; 3];
        let bulk = ColumnTrie::from_tuples(0.into(), tuples.clone());
        assert!(bulk.layers.is_empty());
        assert_eq!(bulk.tuple_count, 0);
        assert_identical(&bulk, &insert_one_by_one(0, &tuples), "arity 0");
    }

    /// Pins the worked example in `docs/data-structures/column-trie.md`.
    #[test]
    fn bulk_build_matches_the_documented_example() {
        let trie = ColumnTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        assert_eq!(trie.layers[0].data, vec![1, 2]);
        assert_eq!(trie.layers[0].interval, vec![0]);
        assert_eq!(trie.layers[1].data, vec![2, 3, 4]);
        assert_eq!(trie.layers[1].interval, vec![0, 2]);
    }

    #[test]
    #[should_panic(expected = "does not match header arity")]
    fn bulk_build_rejects_a_tuple_of_the_wrong_arity() {
        ColumnTrie::from_tuples(2.into(), vec![vec![1, 2], vec![3]]);
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
        // Layers Vec is allocated with arity capacity, but data/interval Vecs
        // are empty
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
