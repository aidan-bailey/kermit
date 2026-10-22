use {
    super::build_mode::TreeTrieBuildMode,
    crate::{
        morsel::{dispatch, row_id, scatter_rows, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD},
        relation::{BuildModeRelation, Relation, RelationHeader},
        seek::{seek_axes, GallopingSeek, SeekStrategy},
    },
    kermit_iters::{HasOptimizationAxes, JoinIterable, RowId, TupleView, Tuples},
    serde_json::Value,
    std::{
        collections::BTreeMap,
        marker::PhantomData,
        ops::{Index, IndexMut},
    },
};

/// Inserts a tuple into a sorted list of children nodes, recursing for the
/// remaining keys on the rest of the slice, so no level copies the suffix.
/// Duplicate tuples are silently absorbed: when a key already exists at this
/// level we descend into its children instead of allocating a new node. Returns
/// `true` iff the tuple was not already present (some level created a new
/// node).
fn insert_into_children(children: &mut Vec<TrieNode>, tuple: &[usize]) -> bool {
    let Some((&key, rest)) = tuple.split_first() else {
        // Exhausted every key along an already-existing path — duplicate.
        return false;
    };

    match children.binary_search_by(|node| node.key().cmp(&key)) {
        | Ok(pos) => insert_into_children(children[pos].children_mut(), rest),
        | Err(pos) => {
            let mut new_node = TrieNode::new(key);
            insert_into_children(new_node.children_mut(), rest);
            children.insert(pos, new_node);
            true
        },
    }
}

/// A node in the pointer-based trie.
///
/// Each node stores a single `usize` key and owns a sorted list of child nodes.
/// Leaf nodes have an empty `children` vector.
#[derive(Clone, Debug)]
pub struct TrieNode {
    key: usize,
    children: Vec<TrieNode>,
}

impl TrieNode {
    pub(crate) fn new(key: usize) -> Self {
        Self {
            key,
            children: vec![],
        }
    }

    pub(crate) fn key(&self) -> usize { self.key }

    pub(crate) fn children(&self) -> &Vec<TrieNode> { &self.children }

    pub(crate) fn children_mut(&mut self) -> &mut Vec<TrieNode> { &mut self.children }
}

impl Index<usize> for TrieNode {
    type Output = TrieNode;

    fn index(&self, index: usize) -> &Self::Output { &self.children[index] }
}

impl IndexMut<usize> for TrieNode {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output { &mut self.children[index] }
}

/// A pointer-based trie that stores a relation as a tree of `TrieNode`s.
///
/// Each tuple `[k₀, k₁, …, kₙ₋₁]` is encoded as a root-to-leaf path where
/// every level corresponds to one column. Children at each level are kept in
/// sorted order by key, so insertion uses binary search and
/// `TreeTrieIter` can seek forward without backtracking
/// — the invariant [`LeapfrogTriejoinIter`] relies on.
///
/// # Invariants
///
/// - The depth of every root-to-leaf path equals
///   [`RelationHeader::arity`](crate::RelationHeader::arity).
/// - `TrieNode::children` at every level is sorted ascending by key with no
///   duplicates among siblings.
///
/// # When to prefer
///
/// Compared to [`ColumnTrie`](crate::ds::ColumnTrie), `TreeTrie` is simpler
/// and allocates a node per key — preferable for small relations, pedagogical
/// use, and tests. For large relations, the column-oriented layout in
/// [`ColumnTrie`](crate::ds::ColumnTrie) tends to be more cache-friendly. See
/// [`ARCHITECTURE.md`](https://github.com/aidan-bailey/kermit/blob/master/ARCHITECTURE.md)
/// for a deeper comparison.
///
/// # Seek strategy
///
/// `S` picks how the iterator's `seek` searches the siblings it has not yet
/// passed (see [`SeekStrategy`]); it changes no stored data. The default,
/// [`GallopingSeek`], probes the current position, then doubles its stride
/// and binary-searches the bracket it finds: O(log d) in the distance a seek
/// moves. It was the fastest strategy on #80's probe set. Name
/// `TreeTrie<BinarySeek>` ([`BinarySeek`](crate::BinarySeek)) for the
/// `partition_point` search plain `TreeTrie` used before then. Bench axis
/// `ds_layout_seek`.
///
/// # Example
///
/// ```
/// use kermit_ds::{Relation, TreeTrie};
///
/// let trie: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
/// assert_eq!(trie.header().arity(), 2);
/// ```
///
/// [`LeapfrogTriejoinIter`]: https://docs.rs/kermit-algos
#[derive(Clone, Debug)]
pub struct TreeTrie<S: SeekStrategy = GallopingSeek> {
    header: RelationHeader,
    children: Vec<TrieNode>,
    /// Number of distinct tuples stored; maintained by `insert`.
    tuple_count: usize,
    /// The seek strategy: a type-level choice, zero-sized.
    _seek: PhantomData<S>,
}

impl<S: SeekStrategy> TreeTrie<S> {
    pub(crate) fn children(&self) -> &Vec<TrieNode> { &self.children }
}

/// First keys sampled per partition when the parallel build places its
/// splitters. More samples put the splitters nearer the true quantiles of
/// the first-key distribution, so partitions carry more even shares of the
/// tuples.
const SPLITTER_SAMPLES_PER_PARTITION: usize = 128;

/// Up to `partitions − 1` splitters for the parallel build: quantiles of a
/// sorted stride sample of the first output keys of the `rows` rows `view`
/// keeps (`rows` is `view.count()`, which the caller has already taken).
/// The sample keeps its duplicates, so the splitters share out the
/// *tuples*, not the distinct keys. A key heavier than one share repeats as
/// a splitter; repeats are merged, since all of a key's tuples must share a
/// partition. A tuple goes to partition
/// `splitters.partition_point(|&s| s <= tuple[0])`, so partitions are
/// ordered by first key. The splitters move work between partitions, never
/// the trie.
///
/// An unfiltered view's `k`-th kept row is source row `k`, so the sample
/// strides over source ids and reads only the sampled rows; a filtered view
/// is walked once, counting the rows it keeps.
///
/// Requires `rows > 0` and a view with at least one output column.
fn first_key_splitters(view: &TupleView<'_>, rows: usize, partitions: usize) -> Vec<usize> {
    let stride = (rows / (SPLITTER_SAMPLES_PER_PARTITION * partitions)).max(1);
    let mut first = vec![0; view.arity()];
    let mut sample: Vec<usize> = Vec::with_capacity(rows.div_ceil(stride));
    let mut take = |id: RowId| {
        view.write(id, &mut first);
        sample.push(first[0]);
    };
    if view.filter().is_none() {
        (0..row_id(rows)).step_by(stride).for_each(&mut take);
    } else {
        let mut seen = 0usize;
        for id in 0..row_id(view.source_rows()) {
            if view.matches(id) {
                if seen.is_multiple_of(stride) {
                    take(id);
                }
                seen += 1;
            }
        }
    }
    sample.sort_unstable();
    let mut splitters: Vec<usize> = (1..partitions)
        .map(|p| sample[p * sample.len() / partitions])
        .collect();
    splitters.dedup();
    splitters
}

#[cfg(any(test, feature = "test-hooks"))]
thread_local! {
    /// The `(threads, partition sizes)` of every parallel build on this
    /// thread. Every build mode builds the same trie, so only this record can
    /// tell a test which build ran, and where it put its tuples. Other
    /// crates' tests read it through `test_hooks` (the `test-hooks` feature).
    static PARALLEL_BUILDS: std::cell::RefCell<Vec<(usize, Vec<usize>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Takes this thread's record of parallel builds, oldest first, leaving it
/// empty: the `(threads, partition sizes)` of each.
#[cfg(feature = "test-hooks")]
pub(crate) fn take_parallel_builds() -> Vec<(usize, Vec<usize>)> {
    PARALLEL_BUILDS.with(|builds| builds.take())
}

impl<S: SeekStrategy> TreeTrie<S> {
    /// The `parallel:N` build (`docs/data-structures/parallel-build.md`):
    /// the trie [`Relation::from_view`] builds, built on `threads` threads.
    /// `morsel_rows` is the morsel size as a parameter, so tests can cut a
    /// small input into many morsels; [`BuildModeRelation`] passes
    /// [`MORSEL_TUPLES`].
    ///
    /// 1. **Partition**: [`scatter_rows`] the ids of the source rows the view
    ///    keeps into first-key ranges cut at [`first_key_splitters`]; the
    ///    source itself stays where it is.
    /// 2. **Build**: [`dispatch`] each partition to a worker, which gathers its
    ///    rows with [`TupleView::gather_sorted`]: the ids ascend, so the sort
    ///    of the 4-byte ids is a sort of nearly ordered input, and the rows are
    ///    then copied once, in output order, into a [`Tuples`] of exactly their
    ///    size, at every arity. The worker inserts its rows one at a time, as
    ///    the serial build does with the whole view. The gather copies each row
    ///    once (`arity` × 8 bytes), in parallel; with unordered first keys
    ///    every partition reads across the whole source, so read traffic grows
    ///    by up to the rows per cache line (about 4× at arity 2).
    /// 3. **Assemble**: push every partition's top-level nodes onto the root,
    ///    in key order, one at a time.
    ///
    /// Sorted order restricted to a key range is that range sorted, so each
    /// subtree receives the serial build's sequence of inserts, and the root
    /// grows by one push per first key, as the serial insert-at-the-end
    /// does. Every node and every `Vec` capacity therefore matches.
    ///
    /// # Panics
    ///
    /// As [`Relation::from_view`]: if the source has rows and the view's
    /// arity is not the header's.
    fn build_parallel(
        header: RelationHeader, threads: Threads, morsel_rows: usize, view: &TupleView<'_>,
    ) -> Self {
        crate::relation::check_view_arity(&header, view);
        if view.source_rows() == 0 {
            return Self::new(header);
        }
        let arity = view.arity();
        let rows = view.count();
        if arity == 0 || rows == 0 {
            // A nullary tuple inserts nothing, so the serial build of any
            // number of them is the empty trie; so is a view that keeps no row.
            return Self::new(header);
        }

        let splitters = first_key_splitters(view, rows, PARTITIONS_PER_THREAD * threads.get());
        let partitions = scatter_rows(threads, view, morsel_rows, splitters.len() + 1, |row| {
            splitters.partition_point(|&splitter| splitter <= row[0])
        });
        #[cfg(any(test, feature = "test-hooks"))]
        PARALLEL_BUILDS.with(|builds| {
            let sizes = partitions.iter().map(|partition| partition.len()).collect();
            builds.borrow_mut().push((threads.get(), sizes));
        });
        let built = dispatch(threads, partitions, |partition| {
            let mut ids = Vec::with_capacity(partition.len());
            ids.extend(partition.ids());
            // The same lexicographic order `materialize_sorted` gives the
            // serial build (and ColumnTrie's builds), so a comparison costs
            // the same in both builds and `parallel:N` against `serial`
            // measures the build process, not a cheaper sort.
            let gathered = view.gather_sorted(&mut ids);
            let mut nodes = Vec::new();
            let mut count = 0;
            for row in gathered.rows() {
                if insert_into_children(&mut nodes, row) {
                    count += 1;
                }
            }
            (nodes, count)
        });

        let mut trie = Self::new(header);
        for (nodes, count) in built {
            // One push per node, as the serial build inserts one node at a
            // time: `extend` or `append` would reserve in bulk and leave the
            // root with a capacity the serial build never has.
            for node in nodes {
                trie.children.push(node);
            }
            trie.tuple_count += count;
        }
        trie
    }
}

impl<S: SeekStrategy> Relation for TreeTrie<S> {
    fn header(&self) -> &RelationHeader { &self.header }

    fn new(header: RelationHeader) -> Self {
        Self {
            header,
            children: vec![],
            tuple_count: 0,
            _seek: PhantomData,
        }
    }

    /// Builds a `TreeTrie` from the rows a view keeps.
    ///
    /// Copies them once with [`TupleView::materialize_sorted`] (lexicographic,
    /// into a buffer of exactly their size, one copy at every arity), the
    /// most efficient input order for the sorted-children invariant: every
    /// new key then lands at the end of its sibling list. Then inserts each
    /// row as a slice; the scratch copy is freed when the build returns.
    ///
    /// # Panics
    ///
    /// Panics if the view's source has rows and the view's arity does not
    /// match `header.arity()`, even when the filter keeps none
    /// (`relation::check_view_arity`, which owns the message). An empty
    /// source builds the empty trie under any header.
    fn from_view(header: RelationHeader, view: TupleView<'_>) -> Self {
        crate::relation::check_view_arity(&header, &view);
        if view.source_rows() == 0 {
            return Self::new(header);
        }

        // One sort in every sorted-trie build (`parallel:N`'s workers and
        // ColumnTrie's builds run the same one), so a comparison costs the
        // same in each and `insertion` compares build routines, not sorts.
        let sorted = view.materialize_sorted();

        let mut trie = Self::new(header);
        for tuple in sorted.rows() {
            trie.insert(tuple);
        }
        trie
    }

    /// Builds a `TreeTrie` from a batch of tuples:
    /// [`from_view`](Self::from_view) over [`TupleView::all`].
    ///
    /// # Panics
    ///
    /// Panics if the batch is non-empty and its arity does not match
    /// `header.arity()`. A `Vec<Vec<usize>>` of mixed arities panics
    /// earlier, while converting to [`Tuples`].
    fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self {
        let tuples: Tuples = tuples.into();
        Self::from_view(header, TupleView::all(&tuples))
    }

    /// Inserts a single tuple, preserving the sorted-children invariant.
    /// Duplicate tuples are silently absorbed.
    ///
    /// # Panics
    ///
    /// Panics if `tuple.len()` does not match the arity of the relation.
    fn insert(&mut self, tuple: impl AsRef<[usize]>) {
        let tuple = tuple.as_ref();
        assert_eq!(
            tuple.len(),
            self.header().arity(),
            "tuple arity must match relation arity"
        );
        if insert_into_children(&mut self.children, tuple) {
            self.tuple_count += 1;
        }
    }

    /// Inserts every row of `tuples`.
    ///
    /// # Panics
    ///
    /// Panics if any tuple's arity does not match the relation's arity
    /// (propagated from [`insert`](Self::insert)).
    fn insert_all(&mut self, tuples: impl Into<Tuples>) {
        let tuples: Tuples = tuples.into();
        for tuple in tuples.rows() {
            self.insert(tuple);
        }
    }
}

impl<S: SeekStrategy> BuildModeRelation for TreeTrie<S> {
    type BuildMode = TreeTrieBuildMode;

    /// Builds by `mode`: [`Relation::from_view`] for `Serial`, the
    /// morsel-driven build for `Parallel`. Both build the identical trie.
    ///
    /// # Panics
    ///
    /// As [`Relation::from_view`].
    fn from_view_with_build_mode(
        header: RelationHeader, mode: TreeTrieBuildMode, view: TupleView<'_>,
    ) -> Self {
        crate::relation::check_view_arity(&header, &view);
        match mode {
            | TreeTrieBuildMode::Serial => Self::from_view(header, view),
            | TreeTrieBuildMode::Parallel(threads) => {
                Self::build_parallel(header, threads, MORSEL_TUPLES, &view)
            },
        }
    }

    /// [`from_view_with_build_mode`](Self::from_view_with_build_mode) over
    /// [`TupleView::all`].
    ///
    /// # Panics
    ///
    /// As [`Relation::from_tuples`].
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: TreeTrieBuildMode, tuples: impl Into<Tuples>,
    ) -> Self {
        let tuples: Tuples = tuples.into();
        Self::from_view_with_build_mode(header, mode, TupleView::all(&tuples))
    }
}

impl<S: SeekStrategy> JoinIterable for TreeTrie<S> {}

impl<S: SeekStrategy> crate::relation::Projectable for TreeTrie<S> {
    fn project(&self, columns: Vec<usize>) -> Self {
        crate::relation::project_via_trie_iter(self, columns)
    }
}

impl<S: SeekStrategy> crate::heap_size::HeapSize for TreeTrie<S> {
    fn heap_size_bytes(&self) -> usize {
        fn node_heap_bytes(node: &TrieNode) -> usize {
            let vec_capacity_bytes = node.children().capacity() * std::mem::size_of::<TrieNode>();
            vec_capacity_bytes + node.children().iter().map(node_heap_bytes).sum::<usize>()
        }

        let root_capacity_bytes = self.children().capacity() * std::mem::size_of::<TrieNode>();
        root_capacity_bytes + self.children().iter().map(node_heap_bytes).sum::<usize>()
    }
}

impl<S: SeekStrategy> crate::cardinality::Cardinality for TreeTrie<S> {
    fn tuple_count(&self) -> usize { self.tuple_count }
}

impl<S: SeekStrategy> HasOptimizationAxes for TreeTrie<S> {
    /// The layout axis `ds_layout_seek`: the strategy's `LayoutOption::NAME`.
    fn optimization_axes(&self) -> BTreeMap<String, Value> { seek_axes::<S>() }
}

#[cfg(test)]
mod heap_size_tests {
    use {
        super::*,
        crate::{HeapSize, Relation},
    };

    #[test]
    fn empty_tree_trie_heap_size() {
        let trie: TreeTrie = TreeTrie::new(2.into());
        assert_eq!(trie.heap_size_bytes(), 0);
    }

    #[test]
    fn single_tuple_tree_trie_heap_size() {
        let trie: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        assert!(trie.heap_size_bytes() > 0);
    }

    #[test]
    fn more_tuples_means_more_heap() {
        let small: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        let large: TreeTrie =
            TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4], vec![
                3, 5,
            ]]);
        assert!(large.heap_size_bytes() > small.heap_size_bytes());
    }

    // `from_tuples` is a pure function of its inputs, so two builds from the
    // same tuples must produce the same heap layout. Pinning this rules out
    // hash-randomised allocation or capacity jitter sneaking in via a future
    // change to the construction path.
    #[test]
    fn heap_size_is_deterministic_across_rebuilds() {
        let tuples = vec![vec![1, 2], vec![1, 3], vec![2, 4], vec![3, 5]];
        let a: TreeTrie = TreeTrie::from_tuples(2.into(), tuples.clone());
        let b: TreeTrie = TreeTrie::from_tuples(2.into(), tuples);
        assert_eq!(a.heap_size_bytes(), b.heap_size_bytes());
    }
}

#[cfg(test)]
mod cardinality_tests {
    use {super::*, crate::cardinality::Cardinality, kermit_iters::TrieIterable};

    #[test]
    fn empty_relation_has_zero_tuples() {
        let trie: TreeTrie = TreeTrie::new(2.into());
        assert_eq!(trie.tuple_count(), 0);
    }

    #[test]
    fn tuple_count_matches_iteration_count() {
        let trie: TreeTrie =
            TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
        assert_eq!(trie.tuple_count(), 3);
        assert_eq!(trie.trie_iter().into_iter().count(), 3);
    }

    #[test]
    fn duplicate_insert_does_not_inflate_count() {
        let mut trie: TreeTrie = TreeTrie::from_tuples(2.into(), vec![vec![1, 2]]);
        trie.insert(vec![1, 2]); // exact duplicate — absorbed
        assert_eq!(trie.tuple_count(), 1);
        trie.insert(vec![1, 3]); // shares prefix, genuinely new
        assert_eq!(trie.tuple_count(), 2);
    }
}

#[cfg(test)]
mod parallel_build_tests {
    use {
        super::*,
        crate::{heap_size::HeapSize, test_support::Lcg},
        kermit_iters::Filter,
    };

    fn threads(n: usize) -> Threads { Threads::new(n).expect("tests use a nonzero count") }

    /// Asserts two child lists are identical, recursively, down to every
    /// `Vec`'s capacity, which `heap_size_bytes` sums.
    #[allow(clippy::ptr_arg)] // the capacity is part of what is compared
    fn assert_same_nodes(
        actual: &Vec<TrieNode>, expected: &Vec<TrieNode>, path: &mut Vec<usize>, case: &str,
    ) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "{case}: child count under {path:?}"
        );
        assert_eq!(
            actual.capacity(),
            expected.capacity(),
            "{case}: child capacity under {path:?}"
        );
        for (a, e) in actual.iter().zip(expected) {
            assert_eq!(a.key(), e.key(), "{case}: key under {path:?}");
            path.push(a.key());
            assert_same_nodes(a.children(), e.children(), path, case);
            path.pop();
        }
    }

    /// Asserts two tries are identical: the same nodes, capacities, count
    /// and heap size.
    fn assert_identical(actual: &TreeTrie, expected: &TreeTrie, case: &str) {
        assert_same_nodes(
            actual.children(),
            expected.children(),
            &mut Vec::new(),
            case,
        );
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

    /// `parallel:N` builds exactly the serial trie (issue #94). Small key
    /// ranges make duplicates and shared prefixes common, a key range of 1
    /// puts every tuple under one first key, arity 0 stores nothing, and
    /// morsels of 7 tuples make the partition step cut the input many times.
    #[test]
    fn parallel_builds_are_identical_to_serial() {
        let seeds: &[u64] = if cfg!(miri) {
            &[1]
        } else {
            &[1, 2, 3, 0x00C0_FFEE_DEAD_BEEF]
        };
        let sizes: &[usize] = if cfg!(miri) {
            &[0, 1, 17]
        } else {
            &[0, 1, 2, 17, 200, 1000]
        };
        let thread_counts: &[usize] = if cfg!(miri) {
            &[2]
        } else {
            &[1, 2, 3, 8]
        };
        let key_ranges: &[usize] = if cfg!(miri) {
            &[5]
        } else {
            &[1, 2, 5, 50]
        };
        for &seed in seeds {
            let mut rng = Lcg(seed);
            for arity in 0..=4 {
                for &key_range in key_ranges {
                    for &n in sizes {
                        let tuples: Vec<Vec<usize>> = (0..n)
                            .map(|_| (0..arity).map(|_| rng.next_usize() % key_range).collect())
                            .collect();
                        let serial: TreeTrie = TreeTrie::from_tuples(arity.into(), tuples.clone());
                        for &t in thread_counts {
                            for morsel in [7, MORSEL_TUPLES] {
                                let case = format!(
                                    "seed {seed}, arity {arity}, keys < {key_range}, n {n}, \
                                     threads {t}, morsel {morsel}"
                                );
                                let batch = Tuples::from(tuples.clone());
                                let parallel: TreeTrie = TreeTrie::build_parallel(
                                    arity.into(),
                                    threads(t),
                                    morsel,
                                    &TupleView::all(&batch),
                                );
                                assert_identical(&parallel, &serial, &case);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Both modes build the same trie, so only the test records show which
    /// build each mode ran, and that `parallel:N` spreads its work: N
    /// workers in both steps, and the tuples spread across the partitions.
    /// The record holds each partition's size, so a partition function that
    /// sent every tuple to one partition would show. Here 16 first keys of 4
    /// tuples each give splitters 2, 4, ..., 14: 8 partitions of 8 tuples.
    #[test]
    fn build_modes_reach_their_builds() {
        let tuples: Vec<Vec<usize>> = (0..64).map(|i| vec![i % 16, i]).collect();
        let serial: TreeTrie = TreeTrie::from_tuples(2.into(), tuples.clone());
        for (mode, builds, worker_runs) in [
            (TreeTrieBuildMode::Serial, vec![], vec![]),
            (
                TreeTrieBuildMode::Parallel(threads(2)),
                vec![(2, vec![8; 8])],
                vec![2, 2],
            ),
        ] {
            PARALLEL_BUILDS.with(|builds| builds.borrow_mut().clear());
            crate::morsel::take_worker_runs();
            let built: TreeTrie =
                TreeTrie::from_tuples_with_build_mode(2.into(), mode, tuples.clone());
            assert_eq!(PARALLEL_BUILDS.with(|b| b.take()), builds, "{mode:?}");
            assert_eq!(crate::morsel::take_worker_runs(), worker_runs, "{mode:?}");
            assert_identical(&built, &serial, &format!("{mode:?}"));
        }
    }

    /// Every build mode builds the trie that one `insert` per tuple, in
    /// arrival order and unsorted, builds. A node's children, and so every
    /// `Vec` capacity, depend only on the tuple set (`Vec::insert` grows a
    /// list exactly as `push` does), so no build's sort, and no form the
    /// tuples arrive in, can change the trie or its `heap_size_bytes`
    /// (#111). Arity 5 takes [`Tuples::sort`]'s permutation path, and the
    /// parallel build's gather of wide rows.
    #[test]
    fn builds_match_inserting_in_arrival_order() {
        let mut rng = Lcg(0x5EED);
        for arity in 0..=5 {
            for key_range in [1, 3, 50] {
                for n in [0, 1, 17, 300] {
                    let tuples: Vec<Vec<usize>> = (0..n)
                        .map(|_| (0..arity).map(|_| rng.next_usize() % key_range).collect())
                        .collect();
                    let mut one_by_one: TreeTrie = TreeTrie::new(arity.into());
                    for tuple in &tuples {
                        one_by_one.insert(tuple);
                    }
                    let case = format!("arity {arity}, keys < {key_range}, n {n}");
                    for mode in [
                        TreeTrieBuildMode::Serial,
                        TreeTrieBuildMode::Parallel(threads(3)),
                    ] {
                        let built: TreeTrie = TreeTrie::from_tuples_with_build_mode(
                            arity.into(),
                            mode,
                            tuples.clone(),
                        );
                        assert_identical(&built, &one_by_one, &format!("{case}, {mode:?}"));
                    }
                }
            }
        }
    }

    /// A batch is accepted when it is empty, whatever its arity (an empty
    /// literal carries none), or when its arity is the header's.
    #[test]
    fn an_empty_batch_of_any_arity_builds_the_empty_trie() {
        for mode in [
            TreeTrieBuildMode::Serial,
            TreeTrieBuildMode::Parallel(threads(2)),
        ] {
            let built: TreeTrie =
                TreeTrie::from_tuples_with_build_mode(2.into(), mode, Tuples::new(5));
            assert_identical(&built, &TreeTrie::new(2.into()), &format!("{mode:?}"));
        }
    }

    #[test]
    #[should_panic(expected = "does not match header arity")]
    fn parallel_build_rejects_a_first_tuple_of_the_wrong_arity() {
        let _: TreeTrie = TreeTrie::from_tuples_with_build_mode(
            2.into(),
            TreeTrieBuildMode::Parallel(threads(2)),
            vec![vec![1, 2, 3]],
        );
    }

    /// One source row of two columns. The two arity tests below read it
    /// through a one-column view whose filter keeps no row: the arity check is
    /// on the source having rows (the contract of every `from_view*`), not on
    /// the rows kept.
    fn one_row_two_columns() -> Tuples {
        let mut batch = Tuples::new(2);
        batch.push(&[1, 2]);
        batch
    }

    #[test]
    #[should_panic(expected = "from_tuples: tuple arity 1 does not match header arity 2")]
    fn serial_build_checks_the_arity_of_a_view_that_keeps_no_row() {
        let batch = one_row_two_columns();
        let view = TupleView::new(&batch, Filter::new(vec![(0, 99)], vec![]), vec![1]);
        let _: TreeTrie = TreeTrie::from_view(2.into(), view);
    }

    #[test]
    #[should_panic(expected = "from_tuples: tuple arity 1 does not match header arity 2")]
    fn parallel_build_checks_the_arity_of_a_view_that_keeps_no_row() {
        let batch = one_row_two_columns();
        let view = TupleView::new(&batch, Filter::new(vec![(0, 99)], vec![]), vec![1]);
        let _: TreeTrie = TreeTrie::build_parallel(2.into(), threads(2), 7, &view);
    }

    /// A view that keeps no row builds the empty trie, and the parallel
    /// build returns before it partitions anything.
    #[test]
    fn a_view_that_keeps_no_row_builds_the_empty_trie() {
        let mut batch = Tuples::new(2);
        for i in 0..20 {
            batch.push(&[i % 3, i]);
        }
        let view = || TupleView::new(&batch, Filter::new(vec![(0, 99)], vec![]), vec![1, 0]);
        PARALLEL_BUILDS.with(|builds| builds.borrow_mut().clear());
        let serial: TreeTrie = TreeTrie::from_view(2.into(), view());
        assert_identical(&serial, &TreeTrie::new(2.into()), "serial");
        let parallel: TreeTrie = TreeTrie::build_parallel(2.into(), threads(2), 7, &view());
        assert_identical(&parallel, &TreeTrie::new(2.into()), "parallel");
        assert!(
            PARALLEL_BUILDS.with(|builds| builds.take()).is_empty(),
            "an empty view is not partitioned"
        );
    }

    /// A filtered, permuted view builds the trie of the rows it keeps: the
    /// same nodes and capacities as inserting those rows one at a time in
    /// arrival order, which is independent of every build's sort and gather.
    /// The filter keeps rows whose column 0 is 1 and whose columns 1 and 2
    /// agree; the view reads the other columns in reverse. Source arities 3
    /// and 5 give output arities 2 and 4 (the sort in place), and 6 gives 5
    /// (the permutation path). Morsels of 7 rows cut the parallel scatter many
    /// times.
    #[test]
    fn views_build_the_trie_of_the_rows_they_keep() {
        let mut rng = Lcg(0xF117);
        for arity in [3usize, 5, 6] {
            for n in [0, 1, 40, 400] {
                let raw: Vec<Vec<usize>> = (0..n)
                    .map(|_| (0..arity).map(|_| rng.next_usize() % 3).collect())
                    .collect();
                let mut batch = Tuples::with_capacity(arity, n);
                for row in &raw {
                    batch.push(row);
                }
                let filter = Filter::new(vec![(0, 1)], vec![(1, 2)]);
                let columns: Vec<usize> = (1..arity).rev().collect();
                let view = || TupleView::new(&batch, filter.clone(), columns.clone());

                let mut expected: TreeTrie = TreeTrie::new((arity - 1).into());
                let mut kept = 0;
                for row in raw.iter().filter(|row| row[0] == 1 && row[1] == row[2]) {
                    expected.insert(columns.iter().map(|&c| row[c]).collect::<Vec<_>>());
                    kept += 1;
                }
                if n == 400 {
                    assert!(kept > 0, "arity {arity}: the filter keeps something");
                }

                let case = format!("arity {arity}, n {n}");
                let serial: TreeTrie = TreeTrie::from_view_with_build_mode(
                    (arity - 1).into(),
                    TreeTrieBuildMode::Serial,
                    view(),
                );
                assert_identical(&serial, &expected, &format!("{case}, serial"));
                for t in [1, 3] {
                    for morsel in [7, MORSEL_TUPLES] {
                        let parallel: TreeTrie = TreeTrie::build_parallel(
                            (arity - 1).into(),
                            threads(t),
                            morsel,
                            &view(),
                        );
                        let case = format!("{case}, threads {t}, morsel {morsel}");
                        assert_identical(&parallel, &expected, &case);
                    }
                }
            }
        }
    }

    /// The splitters are strictly increasing first keys, at most
    /// `partitions − 1` of them.
    #[test]
    fn splitters_are_increasing_first_keys() {
        let tuples = Tuples::from(
            (0..1000)
                .map(|i| vec![(i * 7) % 100, i])
                .collect::<Vec<_>>(),
        );
        let splitters = first_key_splitters(&TupleView::all(&tuples), tuples.len(), 8);
        assert!(
            !splitters.is_empty() && splitters.len() <= 7,
            "{splitters:?}"
        );
        assert!(
            splitters.windows(2).all(|pair| pair[0] < pair[1]),
            "{splitters:?}"
        );
        assert!(splitters.iter().all(|&s| s < 100), "{splitters:?}");
        let one_key = Tuples::from(vec![vec![5, 1], vec![5, 2]]);
        assert_eq!(
            first_key_splitters(&TupleView::all(&one_key), one_key.len(), 4),
            vec![5]
        );
    }

    /// The splitters share out the tuples, not the distinct keys: no
    /// partition holds much more than one share plus the run of its
    /// heaviest key (a key's tuples cannot be split). Spacing the splitters
    /// over distinct keys instead puts most of a skewed input in one
    /// partition.
    #[test]
    #[cfg_attr(miri, ignore = "large inputs; plain arithmetic, no threads")]
    fn splitters_share_out_the_tuples() {
        let partitions = 8;
        let largest_partition = |tuples: &Tuples| {
            let splitters = first_key_splitters(&TupleView::all(tuples), tuples.len(), partitions);
            let mut counts = vec![0usize; splitters.len() + 1];
            for row in tuples.rows() {
                counts[splitters.partition_point(|&s| s <= row[0])] += 1;
            }
            counts.into_iter().max().unwrap()
        };

        // Uniform first keys: no key is heavy.
        let mut rng = Lcg(7);
        let mut uniform = Tuples::with_capacity(1, 100_000);
        for _ in 0..100_000 {
            uniform.push(&[rng.next_usize() % 10_000]);
        }
        let share = uniform.len() / partitions;
        let largest = largest_partition(&uniform);
        assert!(
            2 * largest <= 3 * share,
            "uniform: largest {largest}, share {share}"
        );

        // Skewed first keys: key k appears 10 000 / (k + 1) times, so the
        // heaviest keys have the lowest ids.
        let mut skewed = Tuples::new(1);
        for k in 0..1000usize {
            for _ in 0..10_000 / (k + 1) {
                skewed.push(&[k]);
            }
        }
        let share = skewed.len() / partitions;
        let heaviest = 10_000;
        let largest = largest_partition(&skewed);
        assert!(
            2 * largest <= 2 * heaviest + 3 * share,
            "skewed: largest {largest}, share {share}, heaviest key {heaviest}"
        );
    }

    /// The splitters sample the rows a view keeps, through its output
    /// columns. Source rows are `[i, i % 2]`.
    #[test]
    fn splitters_sample_the_matching_rows_of_a_view() {
        let mut batch = Tuples::with_capacity(2, 1000);
        for i in 0..1000 {
            batch.push(&[i, i % 2]);
        }

        // Filtered, every row sampled: the kept rows are the even `i`, so the
        // sample is `0, 2, 4, …` and splitter `p` is `sample[p · 500 / 8]`.
        let even = TupleView::new(&batch, Filter::new(vec![(1, 0)], vec![]), vec![0]);
        assert_eq!(even.count(), 500);
        let expected: Vec<usize> = (1..8).map(|p| 2 * (p * 500 / 8)).collect();
        assert_eq!(first_key_splitters(&even, 500, 8), expected);

        // Unfiltered and permuted: the first output key is `i % 2`, 500
        // zeros then 500 ones once sorted.
        let flipped = TupleView::new(&batch, Filter::none(), vec![1, 0]);
        assert_eq!(first_key_splitters(&flipped, 1000, 8), vec![0, 1]);

        // A stride above 1 on both paths. 10 000 kept rows over 2
        // partitions sample every 39th kept row (257 samples); 20 000 rows
        // sample every 78th (257 samples). Splitter 1 is `sample[128]`: the
        // 4 992nd kept row (key 9 984), and source row 9 984 (key 9 984).
        let mut big = Tuples::with_capacity(2, 20_000);
        for i in 0..20_000 {
            big.push(&[i, i % 2]);
        }
        let filtered = TupleView::new(&big, Filter::new(vec![(1, 0)], vec![]), vec![0]);
        assert_eq!(first_key_splitters(&filtered, 10_000, 2), vec![9984]);
        let all = TupleView::new(&big, Filter::none(), vec![0]);
        assert_eq!(first_key_splitters(&all, 20_000, 2), vec![9984]);
    }
}
