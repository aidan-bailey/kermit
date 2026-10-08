use {
    super::build_mode::TreeTrieBuildMode,
    crate::{
        morsel::{dispatch, scatter, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD},
        relation::{BuildModeRelation, Relation, RelationHeader},
        seek::{seek_axes, GallopingSeek, SeekStrategy},
    },
    kermit_iters::{HasOptimizationAxes, JoinIterable, Tuples},
    serde_json::Value,
    std::{
        collections::BTreeMap,
        marker::PhantomData,
        ops::{Index, IndexMut},
    },
};

/// Inserts a tuple into a sorted list of children nodes, recursing for the
/// remaining keys. Duplicate tuples are silently absorbed: when a key already
/// exists at this level we descend into its children instead of allocating a
/// new node. Returns `true` iff the tuple was not already present (some
/// level created a new node).
fn insert_into_children(children: &mut Vec<TrieNode>, tuple: Vec<usize>) -> bool {
    let mut key_iter = tuple.into_iter();
    let Some(key) = key_iter.next() else {
        // Exhausted every key along an already-existing path — duplicate.
        return false;
    };

    match children.binary_search_by(|node| node.key().cmp(&key)) {
        | Ok(pos) => insert_into_children(children[pos].children_mut(), key_iter.collect()),
        | Err(pos) => {
            let mut new_node = TrieNode::new(key);
            insert_into_children(new_node.children_mut(), key_iter.collect());
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
/// sorted stride sample of the first keys. The sample keeps its duplicates,
/// so the splitters share out the *tuples*, not the distinct keys. A key
/// heavier than one share repeats as a splitter; repeats are merged, since
/// all of a key's tuples must share a partition. A tuple goes to partition
/// `splitters.partition_point(|&s| s <= tuple[0])`, so partitions are
/// ordered by first key. The splitters move work between partitions, never
/// the trie.
fn first_key_splitters(tuples: &[Vec<usize>], partitions: usize) -> Vec<usize> {
    let stride = (tuples.len() / (SPLITTER_SAMPLES_PER_PARTITION * partitions)).max(1);
    let mut sample: Vec<usize> = tuples
        .iter()
        .step_by(stride)
        .map(|tuple| tuple[0])
        .collect();
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
    /// the trie [`Relation::from_tuples`] builds, built on `threads` threads.
    fn from_tuples_parallel(
        header: RelationHeader, threads: Threads, tuples: Vec<Vec<usize>>,
    ) -> Self {
        Self::build_parallel(header, threads, MORSEL_TUPLES, tuples)
    }

    /// [`from_tuples_parallel`](Self::from_tuples_parallel) with the morsel
    /// size as a parameter, so tests can cut a small input into many
    /// morsels.
    ///
    /// 1. **Partition**: [`scatter`] the tuples into first-key ranges cut at
    ///    [`first_key_splitters`].
    /// 2. **Build**: [`dispatch`] each partition to a worker, which sorts it
    ///    and inserts its tuples one at a time, as the serial build does with
    ///    the whole input.
    /// 3. **Assemble**: push every partition's top-level nodes onto the root,
    ///    in key order, one at a time.
    ///
    /// Sorted order restricted to a key range is that range sorted, so each
    /// subtree receives the serial build's sequence of inserts, and the root
    /// grows by one push per first key, as the serial insert-at-the-end
    /// does. Every node and every `Vec` capacity therefore matches.
    fn build_parallel(
        header: RelationHeader, threads: Threads, morsel_tuples: usize, tuples: Vec<Vec<usize>>,
    ) -> Self {
        if tuples.is_empty() {
            return Self::new(header);
        }
        // The serial build's checks, with its messages.
        let arity = tuples[0].len();
        assert_eq!(
            arity,
            header.arity(),
            "from_tuples: tuple arity {arity} does not match header arity {}",
            header.arity()
        );
        assert!(tuples.iter().all(|tuple| tuple.len() == arity));
        if arity == 0 {
            // A nullary tuple inserts nothing, so the serial build of any
            // number of them is the empty trie.
            return Self::new(header);
        }

        let splitters = first_key_splitters(&tuples, PARTITIONS_PER_THREAD * threads.get());
        let partitions = scatter(
            threads,
            tuples,
            morsel_tuples,
            splitters.len() + 1,
            |tuple| splitters.partition_point(|&splitter| splitter <= tuple[0]),
        );
        #[cfg(any(test, feature = "test-hooks"))]
        PARALLEL_BUILDS.with(|builds| {
            let sizes = partitions.iter().map(|partition| partition.len()).collect();
            builds.borrow_mut().push((threads.get(), sizes));
        });
        let built = dispatch(threads, partitions, |partition| {
            let mut tuples: Vec<Vec<usize>> = Vec::with_capacity(partition.len());
            tuples.extend(partition.into_tuples().map(|(_, tuple)| tuple));
            // The serial build's hand-rolled comparator, kept as in
            // `from_tuples` (and ColumnTrie's build), so a comparison costs the
            // same in both builds and `parallel:N` against `serial` measures
            // the build process, not a cheaper sort.
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
            let mut nodes = Vec::new();
            let mut count = 0;
            for tuple in tuples {
                if insert_into_children(&mut nodes, tuple) {
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

    /// Builds a `TreeTrie` from a batch of tuples.
    ///
    /// Tuples are sorted lexicographically before insertion, which is the most
    /// efficient input order for the sorted-children invariant.
    ///
    /// # Panics
    ///
    /// Panics if the input tuples have mixed arity, or if any tuple's arity
    /// does not match `header.arity()` (propagated from
    /// [`insert`](Self::insert)).
    fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self {
        // One `Vec` per tuple, the form this build takes until it builds
        // from row slices (#111).
        let mut tuples = Tuples::into_vecs(tuples.into());
        if tuples.is_empty() {
            return Self::new(header);
        }

        let arity = tuples[0].len();
        assert_eq!(
            arity,
            header.arity(),
            "from_tuples: tuple arity {arity} does not match header arity {}",
            header.arity()
        );
        assert!(tuples.iter().all(|tuple| tuple.len() == arity));

        // Sort tuples for efficient insertion. Reproduces the derived
        // `Vec<usize>` lexicographic order (kept hand-rolled here rather than
        // `sort_unstable()`).
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
        // #111 interim: the recursion takes an owned `Vec`, removed in T5.
        if insert_into_children(&mut self.children, tuple.to_vec()) {
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

    /// Builds by `mode`: [`Relation::from_tuples`] for `Serial`, the
    /// morsel-driven build for `Parallel`. Both build the identical trie.
    ///
    /// # Panics
    ///
    /// As [`Relation::from_tuples`].
    fn from_tuples_with_build_mode(
        header: RelationHeader, mode: TreeTrieBuildMode, tuples: impl Into<Tuples>,
    ) -> Self {
        match mode {
            | TreeTrieBuildMode::Serial => Self::from_tuples(header, tuples),
            | TreeTrieBuildMode::Parallel(threads) => {
                // One `Vec` per tuple until the build reads rows (#111).
                Self::from_tuples_parallel(header, threads, Tuples::into_vecs(tuples.into()))
            },
        }
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
                                let parallel: TreeTrie = TreeTrie::build_parallel(
                                    arity.into(),
                                    threads(t),
                                    morsel,
                                    tuples.clone(),
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

    #[test]
    #[should_panic(expected = "does not match header arity")]
    fn parallel_build_rejects_a_first_tuple_of_the_wrong_arity() {
        let _: TreeTrie = TreeTrie::from_tuples_parallel(2.into(), threads(2), vec![vec![1, 2, 3]]);
    }

    #[test]
    #[should_panic(expected = "tuple.len() == arity")]
    fn parallel_build_rejects_mixed_arity() {
        let _: TreeTrie =
            TreeTrie::from_tuples_parallel(2.into(), threads(2), vec![vec![1, 2], vec![3]]);
    }

    /// The splitters are strictly increasing first keys, at most
    /// `partitions − 1` of them.
    #[test]
    fn splitters_are_increasing_first_keys() {
        let tuples: Vec<Vec<usize>> = (0..1000).map(|i| vec![(i * 7) % 100, i]).collect();
        let splitters = first_key_splitters(&tuples, 8);
        assert!(
            !splitters.is_empty() && splitters.len() <= 7,
            "{splitters:?}"
        );
        assert!(
            splitters.windows(2).all(|pair| pair[0] < pair[1]),
            "{splitters:?}"
        );
        assert!(splitters.iter().all(|&s| s < 100), "{splitters:?}");
        assert_eq!(first_key_splitters(&[vec![5, 1], vec![5, 2]], 4), vec![5]);
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
        let largest_partition = |tuples: &[Vec<usize>]| {
            let splitters = first_key_splitters(tuples, partitions);
            let mut counts = vec![0usize; splitters.len() + 1];
            for tuple in tuples {
                counts[splitters.partition_point(|&s| s <= tuple[0])] += 1;
            }
            counts.into_iter().max().unwrap()
        };

        // Uniform first keys: no key is heavy.
        let mut rng = Lcg(7);
        let uniform: Vec<Vec<usize>> = (0..100_000)
            .map(|_| vec![rng.next_usize() % 10_000])
            .collect();
        let share = uniform.len() / partitions;
        let largest = largest_partition(&uniform);
        assert!(
            2 * largest <= 3 * share,
            "uniform: largest {largest}, share {share}"
        );

        // Skewed first keys: key k appears 10 000 / (k + 1) times, so the
        // heaviest keys have the lowest ids.
        let skewed: Vec<Vec<usize>> = (0..1000usize)
            .flat_map(|k| std::iter::repeat_n(vec![k], 10_000 / (k + 1)))
            .collect();
        let share = skewed.len() / partitions;
        let heaviest = 10_000;
        let largest = largest_partition(&skewed);
        assert!(
            2 * largest <= 2 * heaviest + 3 * share,
            "skewed: largest {largest}, share {share}, heaviest key {heaviest}"
        );
    }
}
