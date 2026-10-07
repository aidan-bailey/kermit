//! The `parallel:N` and `presized:N` builds of a [`HashTrie`](super::HashTrie):
//! `parallel:N` is the radix build's three steps (`radix.rs`), with the
//! partition and build steps run on N threads by the morsel-driven helpers
//! of `crate::morsel` (Leis et al., SIGMOD 2014; SIGMOD 2020 §3.3.2; issue
//! #94).
//!
//! 1. **Partition.** [`scatter`] moves every tuple, with its input position,
//!    into one of P partitions by the top log₂P bits of its first attribute's
//!    hash; P is four per thread, rounded up to a power of two.
//! 2. **Build.** [`dispatch`] groups each non-empty partition into a scratch
//!    root and builds its children, as `bulk` does
//!    (`radix::build_scratch_root`), then takes the scratch root's entries out,
//!    each tagged with the input position at which its key first appeared.
//! 3. **Merge.** The calling thread inserts every entry into the real root in
//!    first-appearance order, by a k-way merge of the partitions' lists.
//!
//! The result is the bulk trie, bucket for bucket and capacity for
//! capacity, for the reasons the radix build's is. Each partition lists its
//! tuples in input order (`scatter` keeps it), so each child is built from
//! the list `bulk` builds it from; and the root receives its new keys in
//! the bulk order. Equal 64-bit hashes share a root entry and also a
//! partition, because the partition is a function of the hash.
//!
//! `presized:N`
//! (`docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`;
//! spelled `parallel:N` under `root-capacity=tuples` until 2026-10-07) is
//! the paper's build of a presized root ([`fill_presized_root`]), and
//! requires `root-capacity=tuples` (#88). The partitions are contiguous runs
//! of the root's fixed-size regions, by each tuple's home bucket, and each
//! worker pushes every tuple of its partition onto its bucket's list in its
//! run of a presized table of lists. A tuple whose key's probe would cross
//! its region's end is deferred, and the calling thread pushes the deferred
//! tuples afterwards, in input order. After the tail, each worker builds the
//! children of its run's buckets by Algorithm 2. Root keys may then sit in
//! other buckets than the bulk build's, so this trie is equivalent rather
//! than identical (Amendment 2): the root has the same capacity, occupied
//! buckets and total displacement, and every subtrie is identical. Regions,
//! not threads, decide which tuples are deferred, so the trie is the same
//! for every N.

use {
    super::{
        bulk::TupleList,
        config::{HashTrieConfig, LoadFactor},
        expansion::ExpansionPolicy,
        hash_table::{home_bucket, BucketRun, HashTable, Overflow, RunEntry},
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::PruningPolicy,
        radix::{self, Arrival},
    },
    crate::morsel::{
        dispatch, scatter, Partition, Positioned, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD,
    },
    kermit_iters::HashStrategy,
    std::{cmp::Reverse, collections::BinaryHeap},
};

/// One parallel build, as the test hooks record it.
#[cfg(any(test, feature = "test-hooks"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParallelBuild {
    /// The `N` of `parallel:N` or `presized:N`.
    pub threads: usize,
    /// The size of each partition, empty ones included.
    pub partition_sizes: Vec<usize>,
    /// `None` for `parallel:N` (the exact build). For `presized:N`, the
    /// number of tuples deferred to the calling thread.
    pub deferred: Option<usize>,
}

#[cfg(any(test, feature = "test-hooks"))]
thread_local! {
    /// Every parallel build on this thread. Every build mode builds an
    /// equivalent trie, so only this record can tell a test which build
    /// ran and where it put its tuples. Other crates' tests read it through
    /// `test_hooks` (the `test-hooks` feature).
    static PARALLEL_BUILDS: std::cell::RefCell<Vec<ParallelBuild>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Takes this thread's record of parallel builds, oldest first, leaving it
/// empty.
#[cfg(any(test, feature = "test-hooks"))]
pub(crate) fn take_parallel_builds() -> Vec<ParallelBuild> {
    PARALLEL_BUILDS.with(|builds| builds.take())
}

/// One partition's root entries, each tagged with the input position at
/// which its key first appeared, in that order. The root is `Inner` for
/// arity ≥ 2, so its values are child nodes (subtries, or under pruning and
/// lazy expansion their `Singleton`s and pending lists), and `Leaf` for
/// arity 1, so its values are chains.
enum Entries<P: PruningPolicy, E: ExpansionPolicy> {
    Children(Vec<Arrival<HashTrieNode<P, E>>>),
    Chains(Vec<Arrival<Vec<Vec<usize>>>>),
}

impl<P: PruningPolicy, E: ExpansionPolicy> Entries<P, E> {
    /// Moves every entry out of a finished scratch root, in the order its
    /// keys arrived (`first_seen`, from `radix::build_scratch_root`).
    fn take_from(scratch: HashTrieNode<P, E>, first_seen: &[(usize, u64)]) -> Self {
        match scratch {
            | HashTrieNode::Inner(table) => {
                let mut children = Vec::with_capacity(first_seen.len());
                radix::take_in_arrival_order(table, first_seen, &mut children);
                Self::Children(children)
            },
            | HashTrieNode::Leaf(table) => {
                let mut chains = Vec::with_capacity(first_seen.len());
                radix::take_in_arrival_order(table, first_seen, &mut chains);
                Self::Chains(chains)
            },
            | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
                unreachable!("a root is never pruned or unexpanded")
            },
        }
    }

    fn into_children(self) -> Vec<Arrival<HashTrieNode<P, E>>> {
        match self {
            | Self::Children(children) => children,
            | Self::Chains(_) => unreachable!("an Inner root's scratch roots are Inner"),
        }
    }

    fn into_chains(self) -> Vec<Arrival<Vec<Vec<usize>>>> {
        match self {
            | Self::Chains(chains) => chains,
            | Self::Children(_) => unreachable!("a Leaf root's scratch roots are Leaf"),
        }
    }
}

/// The radix bits for `threads`: [`PARTITIONS_PER_THREAD`] partitions per
/// thread, rounded up to a power of two. From 2 bits (one thread) to 12
/// ([`Threads::MAX`]).
fn partition_bits(threads: Threads) -> u32 {
    (PARTITIONS_PER_THREAD * threads.get())
        .next_power_of_two()
        .trailing_zeros()
}

/// Fills the empty `root` with `tuples` by the `parallel:threads` build.
/// Every tuple must have `arity` attributes; the caller checks.
pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    root: &mut HashTrieNode<P, E>, arity: usize, tuples: Vec<Vec<usize>>, threads: Threads,
    config: HashTrieConfig,
) {
    fill_root_in_morsels::<H, P, E>(root, arity, tuples, threads, MORSEL_TUPLES, config);
}

/// [`fill_root`] with the morsel size as a parameter, so tests can cut a
/// small input into many morsels.
fn fill_root_in_morsels<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    root: &mut HashTrieNode<P, E>, arity: usize, tuples: Vec<Vec<usize>>, threads: Threads,
    morsel_tuples: usize, config: HashTrieConfig,
) {
    if tuples.is_empty() {
        // The bulk build of nothing is the empty root: start no worker.
        return;
    }
    // 1. Partition, on `threads` workers, by the top bits of the first
    //    attribute's hash; each partition keeps input order.
    let bits = partition_bits(threads);
    let shift = 64 - bits;
    let partitions = scatter(threads, tuples, morsel_tuples, 1 << bits, |tuple| {
        (H::hash(tuple[0]) >> shift) as usize
    });
    #[cfg(any(test, feature = "test-hooks"))]
    PARALLEL_BUILDS.with(|builds| {
        let sizes = partitions.iter().map(Partition::len).collect();
        builds.borrow_mut().push(ParallelBuild {
            threads: threads.get(),
            partition_sizes: sizes,
            deferred: None,
        });
    });
    // An empty partition would build and drop an empty scratch root; the
    // radix build skips them too.
    let partitions: Vec<Partition> = partitions
        .into_iter()
        .filter(|partition| partition.len() > 0)
        .collect();
    // 2. Build, on `threads` workers: each partition into a scratch root, whose
    //    entries then leave in the order their keys arrived.
    let entries = dispatch(threads, partitions, |partition| {
        let (scratch, first_seen) =
            radix::build_scratch_root::<H, P, E>(partition.into_tuples(), arity, config);
        Entries::take_from(scratch, &first_seen)
    });
    // 3. Merge, on this thread, in first-appearance order.
    match root {
        | HashTrieNode::Inner(table) => {
            let lists = entries.into_iter().map(Entries::into_children).collect();
            merge_in_first_appearance_order(table, lists, config.load_factor);
        },
        | HashTrieNode::Leaf(table) => {
            let lists = entries.into_iter().map(Entries::into_chains).collect();
            merge_in_first_appearance_order(table, lists, config.load_factor);
        },
        | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
            unreachable!("a root is never pruned or unexpanded")
        },
    }
}

/// Inserts every partition's entries into the real root in the order their
/// keys first appeared in the input, the order the bulk build inserts
/// them, so the root's buckets, length and capacity are the bulk
/// build's. Each list is already in that order, so a k-way merge suffices:
/// a heap pop and push per entry over at most P lists, where the radix
/// build sorts every entry. It runs on the calling thread: the build's
/// sequential step.
fn merge_in_first_appearance_order<V>(
    root: &mut HashTable<V>, lists: Vec<Vec<Arrival<V>>>, load_factor: LoadFactor,
) {
    let mut lists: Vec<_> = lists
        .into_iter()
        .map(|list| list.into_iter().peekable())
        .collect();
    // Each list's next first-appearance position, smallest on top. Positions
    // are distinct, so no two heads tie.
    let mut heads: BinaryHeap<Reverse<(usize, usize)>> = lists
        .iter_mut()
        .enumerate()
        .filter_map(|(list, entries)| entries.peek().map(|&(first, ..)| Reverse((first, list))))
        .collect();
    while let Some(Reverse((_, list))) = heads.pop() {
        let (_, hash, value) = lists[list].next().expect("a head names a non-empty list");
        root.entry_or_insert_with(hash, load_factor, || value);
        if let Some(&(first, ..)) = lists[list].peek() {
            heads.push(Reverse((first, list)));
        }
    }
}

/// Buckets per region of a presized root. A probe during the parallel fill
/// never crosses a region's end, and a key whose probe would is deferred to
/// the calling thread. Regions, not partitions, decide that, so the layout
/// is the same for every thread count. 4096 buckets keep the deferred share
/// small at load factors up to 0.95, and keep a region's buckets within a
/// worker's cache.
const REGION_BUCKETS: usize = 4096;

/// Fills the presized, empty `root` with `tuples` by `presized:threads`,
/// the paper's partitioned build
/// (`docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`),
/// with every child built by Algorithm 2 (#107), and returns it. Every
/// tuple must have `arity` attributes; the caller checks.
pub(super) fn fill_presized_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    root: HashTrieNode<P, E>, arity: usize, tuples: Vec<Vec<usize>>, threads: Threads,
    config: HashTrieConfig,
) -> HashTrieNode<P, E> {
    fill_presized_root_in::<H, P, E>(
        root,
        arity,
        tuples,
        threads,
        MORSEL_TUPLES,
        REGION_BUCKETS,
        config,
    )
}

/// [`fill_presized_root`] with the morsel and region sizes as parameters,
/// so tests can cut a small input into many morsels and regions.
///
/// 1. **Partition**: `scatter` by the top bits of each tuple's home bucket, so
///    partition k is a contiguous run of regions, in input order.
/// 2. **Group** (Algorithm 2, lines 4–7): each worker takes a partition with
///    its run of a presized table of tuple lists, and pushes every tuple onto
///    the list in its bucket. Tuples whose key's probe runs off its region are
///    deferred.
/// 3. **Tail**: the calling thread pushes the deferred tuples in input order,
///    with ordinary probing. The paper does not say how a probe crossing a
///    partition's end is handled; this is kermit's answer.
/// 4. **Children** (lines 8–12): each worker builds the children of its run's
///    buckets, by the bulk build's `child`. The paper does not say how the
///    recursion is spread over threads; one run per worker is kermit's choice.
fn fill_presized_root_in<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    root: HashTrieNode<P, E>, arity: usize, tuples: Vec<Vec<usize>>, threads: Threads,
    morsel_tuples: usize, region_buckets: usize, config: HashTrieConfig,
) -> HashTrieNode<P, E> {
    if tuples.is_empty() {
        return root;
    }
    // Only the root's capacity is used: its tuples are grouped into a table
    // of lists of that size, which step 4 turns into the root. The empty
    // root is dropped before that table is allocated.
    let log2 = root.buckets_len().trailing_zeros();
    drop(root);
    let capacity = 1usize << log2;
    let region_buckets = region_buckets.min(capacity);
    let parts = (PARTITIONS_PER_THREAD * threads.get())
        .next_power_of_two()
        .min(capacity / region_buckets);
    let shift = log2 - parts.trailing_zeros();
    // 1. Partition by the home bucket's top bits: whole runs of regions.
    let partitions = scatter(threads, tuples, morsel_tuples, parts, |tuple| {
        home_bucket(H::hash(tuple[0]), log2) >> shift
    });
    #[cfg(any(test, feature = "test-hooks"))]
    let partition_sizes: Vec<usize> = partitions.iter().map(Partition::len).collect();
    // 2. Group each run of regions in parallel.
    let mut lists: HashTable<TupleList> = HashTable::with_log2_capacity(log2);
    let mut deferred = fill_runs(
        &mut lists,
        threads,
        partitions,
        region_buckets,
        |run, tuple| push_in_run::<H>(run, tuple),
    );
    #[cfg(any(test, feature = "test-hooks"))]
    PARALLEL_BUILDS.with(|builds| {
        builds.borrow_mut().push(ParallelBuild {
            threads: threads.get(),
            partition_sizes,
            deferred: Some(deferred.len()),
        });
    });
    // 3. The tail, in input order. The table is presized for every tuple, so it
    //    does not grow here.
    deferred.sort_unstable_by_key(|&(position, _)| position);
    for (_, tuple) in deferred {
        lists
            .entry_or_insert_with(H::hash(tuple[0]), config.load_factor, Vec::new)
            .push(tuple);
    }
    // 4. Every bucket's child, a run per worker. At arity 1 the lists are the
    //    chains, and the table of lists is the root. `arity <= 1`, as
    //    `build_nested` decides at depth 0 (`depth + 1 >= arity`).
    if arity <= 1 {
        return HashTrieNode::Leaf(lists);
    }
    HashTrieNode::Inner(lists.map_in_runs(parts, |runs| {
        dispatch(threads, runs, |run| {
            run.map(|list| HashTrie::<H, P, E>::child(1, arity, list, config));
        });
    }))
}

/// Algorithm 2's lines 6–7 inside one run of a presized table of lists:
/// `presized:N`'s root step, the same for every arity. Returns the tuple if
/// its key's probe ran off its region, for the calling thread to push
/// afterwards.
fn push_in_run<H: HashStrategy>(
    run: &mut BucketRun<'_, TupleList>, tuple: Vec<usize>,
) -> Result<(), Vec<usize>> {
    match run.entry(H::hash(tuple[0])) {
        | Ok(RunEntry::Occupied(list)) => list.push(tuple),
        | Ok(RunEntry::Vacant(bucket)) => bucket.insert(Vec::new()).push(tuple),
        | Err(Overflow) => return Err(tuple),
    }
    Ok(())
}

/// Lends `table`'s runs to the workers: partition k fills run k through
/// `step`, in input order, and the tuples `step` hands back are returned
/// with their input positions.
fn fill_runs<V: Send>(
    table: &mut HashTable<V>, threads: Threads, partitions: Vec<Partition>, region_buckets: usize,
    step: impl Fn(&mut BucketRun<'_, V>, Vec<usize>) -> Result<(), Vec<usize>> + Sync,
) -> Vec<Positioned> {
    let parts = partitions.len();
    table.with_runs(parts, region_buckets, |runs| {
        let work: Vec<_> = partitions.into_iter().zip(runs).collect();
        dispatch(threads, work, |(partition, mut run)| {
            let mut deferred = Vec::new();
            for (position, tuple) in partition.into_tuples() {
                if let Err(tuple) = step(&mut run, tuple) {
                    deferred.push((position, tuple));
                }
            }
            deferred
        })
        .into_iter()
        .flatten()
        .collect()
    })
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            ds::hash_trie::{
                build_mode::{HashTrieBuildMode, RadixBits},
                config::{HashTrieConfig, RootCapacity},
                expansion::{EagerExpansion, LazyExpansion},
                identity::{
                    assert_equivalent_root, assert_equivalent_trie, assert_same_node,
                    assert_same_trie, inputs, LOAD_PERCENTS,
                },
                implementation::HashTrie,
                pruning::{NoPruning, SingletonPruning},
            },
            relation::{BuildModeRelation, ConfigurableRelation, Relation},
            test_support::{Lcg, Mod10HashStrategy},
        },
        kermit_iters::{FxHashStrategy, LayoutOption, SipHashStrategy},
    };

    fn threads(n: usize) -> Threads { Threads::new(n).expect("tests use a nonzero count") }

    /// Thread counts under test. Spawning threads is slow under Miri, so it
    /// runs two.
    const THREADS: &[usize] = if cfg!(miri) {
        &[2]
    } else {
        &[1, 2, 3, 8]
    };

    /// `parallel:N` builds the trie `bulk` builds, for every arity, root
    /// capacity, load factor, thread count and input; its `Tuples` loop is
    /// the array-level evidence that `parallel:N` under
    /// `root-capacity=tuples` is the exact merge build. Each case runs
    /// twice: through the public constructor (morsels of 16 384 tuples, the
    /// whole trie compared), and with morsels of 7 tuples, which make the
    /// partition step cut the input many times (the root compared, which
    /// holds the whole trie). Miri runs the second only.
    fn check_identity<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for root_capacity in [RootCapacity::Grow, RootCapacity::Tuples] {
                for &percent in LOAD_PERCENTS {
                    let config = HashTrieConfig {
                        load_factor: LoadFactor::percent(percent).unwrap(),
                        root_capacity,
                    };
                    for (input, tuples) in inputs(arity) {
                        let bulk = HashTrie::<H, P, E>::from_tuples_with_config(
                            arity.into(),
                            config,
                            tuples.clone(),
                        );
                        for &t in THREADS {
                            let label = format!(
                                "{}/{}/{} arity {arity}, {root_capacity:?}, load {percent}%, \
                                 parallel:{t}, {input}",
                                H::NAME,
                                P::NAME,
                                E::NAME
                            );
                            if !cfg!(miri) {
                                let built =
                                    HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
                                        arity.into(),
                                        config,
                                        HashTrieBuildMode::Parallel(threads(t)),
                                        tuples.clone(),
                                    );
                                assert_same_trie(&bulk, &built, &label);
                            }
                            let mut root = HashTrie::<H, P, E>::make_root_sized(
                                arity,
                                config.root_log2_capacity(tuples.len()),
                            );
                            fill_root_in_morsels::<H, P, E>(
                                &mut root,
                                arity,
                                tuples.clone(),
                                threads(t),
                                7,
                                config,
                            );
                            assert_same_node(bulk.root(), &root, &format!("{label}, morsels of 7"));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn parallel_builds_the_bulk_trie_under_siphash() {
        check_identity::<SipHashStrategy, NoPruning, EagerExpansion>();
        check_identity::<SipHashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<SipHashStrategy, NoPruning, LazyExpansion>();
        check_identity::<SipHashStrategy, SingletonPruning, LazyExpansion>();
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "threads are slow under Miri; the SipHash matrix runs the same code"
    )]
    fn parallel_builds_the_bulk_trie_under_fxhash() {
        check_identity::<FxHashStrategy, NoPruning, EagerExpansion>();
        check_identity::<FxHashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<FxHashStrategy, NoPruning, LazyExpansion>();
        check_identity::<FxHashStrategy, SingletonPruning, LazyExpansion>();
    }

    /// Every hash is below 10, so every tuple lands in partition 0, and
    /// distinct keys share full hashes, root entries and leaf chains.
    #[test]
    #[cfg_attr(
        miri,
        ignore = "threads are slow under Miri; the SipHash matrix runs the same code"
    )]
    fn parallel_builds_the_bulk_trie_under_colliding_hashes() {
        check_identity::<Mod10HashStrategy, NoPruning, EagerExpansion>();
        check_identity::<Mod10HashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<Mod10HashStrategy, NoPruning, LazyExpansion>();
        check_identity::<Mod10HashStrategy, SingletonPruning, LazyExpansion>();
    }

    #[test]
    fn partition_bits_give_four_partitions_per_thread_rounded_up() {
        assert_eq!(partition_bits(threads(1)), 2);
        assert_eq!(partition_bits(threads(2)), 3);
        assert_eq!(partition_bits(threads(3)), 4);
        assert_eq!(partition_bits(threads(8)), 5);
        assert_eq!(partition_bits(threads(Threads::MAX)), 12);
    }

    /// Inputs the shared matrix leaves out (the spec's § Testing): arity 4,
    /// a first key holding half the tuples, and enough tuples (39 768) that
    /// the public constructor cuts them into three morsels, two of them full
    /// (16 384 tuples).
    #[test]
    #[cfg_attr(miri, ignore = "tens of thousands of inserts")]
    fn parallel_builds_the_bulk_trie_on_large_and_skewed_inputs() {
        fn check<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
            let n = 2 * MORSEL_TUPLES + 7_000;
            let mut lcg = Lcg(0x94);
            for arity in 1..=4 {
                let random: Vec<Vec<usize>> = (0..n)
                    .map(|_| (0..arity).map(|_| lcg.next_usize() % 5_000).collect())
                    .collect();
                let skewed: Vec<Vec<usize>> = (0..n)
                    .map(|i| {
                        let first = if i % 2 == 0 {
                            7
                        } else {
                            lcg.next_usize() % 5_000
                        };
                        std::iter::once(first)
                            .chain((1..arity).map(|_| lcg.next_usize() % 50))
                            .collect()
                    })
                    .collect();
                for (input, tuples) in [("random", random), ("half one key", skewed)] {
                    let bulk = HashTrie::<H, P, E>::from_tuples(arity.into(), tuples.clone());
                    for t in [2, 5] {
                        let built = HashTrie::<H, P, E>::from_tuples_with_build_mode(
                            arity.into(),
                            HashTrieBuildMode::Parallel(threads(t)),
                            tuples.clone(),
                        );
                        let label = format!(
                            "{}/{}/{} arity {arity}, parallel:{t}, {input}",
                            H::NAME,
                            P::NAME,
                            E::NAME
                        );
                        assert_same_trie(&bulk, &built, &label);
                    }
                }
            }
        }
        check::<SipHashStrategy, NoPruning, EagerExpansion>();
        check::<FxHashStrategy, SingletonPruning, LazyExpansion>();
    }

    /// Hashes a key below 8 to the key in its top three bits, so under
    /// `parallel:2` (eight partitions) key k lands in partition k.
    #[derive(Copy, Clone, Default, Debug)]
    struct TopBitsHash;

    impl LayoutOption for TopBitsHash {
        const NAME: &'static str = "top-bits";
    }

    impl HashStrategy for TopBitsHash {
        fn hash(key: usize) -> u64 { (key as u64) << 61 }
    }

    /// Every mode builds the same trie, so only the records show which build
    /// ran (`bulk` and `incremental` record nothing here;
    /// `each_build_mode_runs_its_own_build` tells them apart), and that
    /// `parallel:N` spreads its work: N workers in both steps (two
    /// `run_workers` calls), and the tuples spread over the partitions.
    /// Eight first keys of eight tuples each fill the eight partitions of
    /// `parallel:2` evenly. `parallel:3` aims at 12 partitions, rounded up to
    /// 16, so key k lands in partition 2k: every other partition is empty,
    /// and the record lists those too. A second thread count is what shows
    /// that N itself reaches the build. The radix build partitions too, but
    /// serially.
    #[test]
    fn build_modes_reach_their_builds() {
        let tuples: Vec<Vec<usize>> = (0..64).map(|i| vec![i % 8, i]).collect();
        let bulk: HashTrie<TopBitsHash> = HashTrie::from_tuples(2.into(), tuples.clone());
        for (mode, builds, worker_runs) in [
            (HashTrieBuildMode::Bulk, vec![], vec![]),
            (HashTrieBuildMode::Incremental, vec![], vec![]),
            (
                HashTrieBuildMode::Radix(RadixBits::new(3).unwrap()),
                vec![],
                vec![],
            ),
            (
                HashTrieBuildMode::Parallel(threads(2)),
                vec![ParallelBuild {
                    threads: 2,
                    partition_sizes: vec![8; 8],
                    deferred: None,
                }],
                vec![2, 2],
            ),
            (
                HashTrieBuildMode::Parallel(threads(3)),
                vec![ParallelBuild {
                    threads: 3,
                    partition_sizes: [8, 0].repeat(8),
                    deferred: None,
                }],
                vec![3, 3],
            ),
        ] {
            PARALLEL_BUILDS.with(|builds| builds.borrow_mut().clear());
            crate::morsel::take_worker_runs();
            let built: HashTrie<TopBitsHash> =
                HashTrie::from_tuples_with_build_mode(2.into(), mode, tuples.clone());
            assert_eq!(PARALLEL_BUILDS.with(|b| b.take()), builds, "{mode:?}");
            assert_eq!(crate::morsel::take_worker_runs(), worker_runs, "{mode:?}");
            assert_same_trie(&bulk, &built, &format!("{mode:?}"));
        }
    }

    /// The parallel build of nothing is the empty trie, built without
    /// partitioning or starting a worker.
    #[test]
    fn parallel_build_of_nothing_starts_no_worker() {
        PARALLEL_BUILDS.with(|builds| builds.borrow_mut().clear());
        crate::morsel::take_worker_runs();
        let built: HashTrie = HashTrie::from_tuples_with_build_mode(
            2.into(),
            HashTrieBuildMode::Parallel(threads(4)),
            vec![],
        );
        assert_same_trie(&HashTrie::from_tuples(2.into(), vec![]), &built, "empty");
        assert!(PARALLEL_BUILDS.with(|b| b.take()).is_empty());
        assert!(crate::morsel::take_worker_runs().is_empty());
    }

    #[test]
    #[should_panic(expected = "does not match header arity")]
    fn parallel_build_rejects_a_wrong_arity() {
        let _: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            HashTrieConfig::default(),
            HashTrieBuildMode::Parallel(threads(2)),
            vec![vec![1, 2], vec![3]],
        );
    }

    /// Grouping alone, without threads: one run over the whole presized
    /// table of lists, the overflow pushed afterwards in input order, then
    /// `build_nested`. The root must be equivalent to bulk's (Amendment 2),
    /// and every child identical.
    fn check_one_run<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for &percent in LOAD_PERCENTS {
                let config = tuples_config(percent);
                for (input, tuples) in inputs(arity) {
                    let label = format!(
                        "{}/{}/{} arity {arity}, load {percent}%, {input}",
                        H::NAME,
                        P::NAME,
                        E::NAME
                    );
                    let bulk = HashTrie::<H, P, E>::from_tuples_with_config(
                        arity.into(),
                        config,
                        tuples.clone(),
                    );
                    let log2 = config.root_log2_capacity(tuples.len());
                    let mut lists: HashTable<TupleList> = HashTable::with_log2_capacity(log2);
                    let tail: Vec<Vec<usize>> = lists.with_runs(1, 1 << log2, |mut runs| {
                        tuples
                            .iter()
                            .cloned()
                            .filter_map(|t| push_in_run::<H>(&mut runs[0], t).err())
                            .collect()
                    });
                    for t in tail {
                        lists
                            .entry_or_insert_with(H::hash(t[0]), config.load_factor, Vec::new)
                            .push(t);
                    }
                    let root = HashTrie::<H, P, E>::build_nested(0, arity, lists, config);
                    assert_equivalent_root(bulk.root(), &root, &label);
                }
            }
        }
    }

    #[test]
    fn one_run_of_grouping_builds_the_bulk_root() {
        check_one_run::<SipHashStrategy, NoPruning, EagerExpansion>();
        check_one_run::<SipHashStrategy, SingletonPruning, EagerExpansion>();
        check_one_run::<SipHashStrategy, NoPruning, LazyExpansion>();
        check_one_run::<SipHashStrategy, SingletonPruning, LazyExpansion>();
        check_one_run::<FxHashStrategy, SingletonPruning, LazyExpansion>();
        check_one_run::<Mod10HashStrategy, SingletonPruning, EagerExpansion>();
    }

    /// Thread counts for the presized matrix; Miri runs two.
    const PRESIZED_THREADS: &[usize] = if cfg!(miri) {
        &[2]
    } else {
        &[1, 2, 3, 8]
    };

    /// Load factors under test, 80 % (the paper's sizing) included.
    const PRESIZED_LOAD_PERCENTS: &[u8] = if cfg!(miri) {
        &[70]
    } else {
        &[50, 70, 80, 95]
    };

    fn tuples_config(percent: u8) -> HashTrieConfig {
        HashTrieConfig {
            load_factor: LoadFactor::percent(percent).unwrap(),
            root_capacity: RootCapacity::Tuples,
        }
    }

    /// The presized build through the public constructor, and through the
    /// test entry with morsels of 7 and 8-bucket regions, so small inputs
    /// span many regions and overflow. Both must be equivalent to bulk
    /// under `Tuples` (Amendment 2), and identical to their own `presized:1`
    /// for every N.
    fn check_presized<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
        inputs: &dyn Fn(usize) -> Vec<(&'static str, Vec<Vec<usize>>)>,
        arities: std::ops::RangeInclusive<usize>,
    ) {
        for arity in arities {
            for &percent in PRESIZED_LOAD_PERCENTS {
                let config = tuples_config(percent);
                for (input, tuples) in inputs(arity) {
                    let bulk = HashTrie::<H, P, E>::from_tuples_with_config(
                        arity.into(),
                        config,
                        tuples.clone(),
                    );
                    // `presized:1`'s two builds, which every N must match.
                    let mut first = None;
                    for &t in PRESIZED_THREADS {
                        let label = format!(
                            "{}/{}/{} arity {arity}, load {percent}%, presized:{t}, {input}",
                            H::NAME,
                            P::NAME,
                            E::NAME
                        );
                        let built = HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
                            arity.into(),
                            config,
                            HashTrieBuildMode::Presized(threads(t)),
                            tuples.clone(),
                        );
                        assert_equivalent_trie(&bulk, &built, &label);
                        let log2 = config.root_log2_capacity(tuples.len());
                        let small = fill_presized_root_in::<H, P, E>(
                            HashTrie::<H, P, E>::make_root_sized(arity, log2),
                            arity,
                            tuples.clone(),
                            threads(t),
                            7,
                            8,
                            config,
                        );
                        assert_equivalent_root(
                            bulk.root(),
                            &small,
                            &format!("{label}, small regions"),
                        );
                        match &first {
                            | None => first = Some((built, small)),
                            | Some((built_1, small_1)) => {
                                assert_same_trie(
                                    built_1,
                                    &built,
                                    &format!("{label} vs presized:1"),
                                );
                                assert_same_node(
                                    small_1,
                                    &small,
                                    &format!("{label} vs presized:1, small regions"),
                                );
                            },
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn presized_parallel_builds_are_equivalent_under_siphash() {
        let shared = |arity: usize| inputs(arity);
        check_presized::<SipHashStrategy, NoPruning, EagerExpansion>(&shared, 1..=3);
        check_presized::<SipHashStrategy, SingletonPruning, LazyExpansion>(&shared, 1..=3);
        // Miri runs the two above, which cover both pruning policies and both
        // expansion policies; the threads and the region runs are the same
        // code under the other two.
        if !cfg!(miri) {
            check_presized::<SipHashStrategy, SingletonPruning, EagerExpansion>(&shared, 1..=3);
            check_presized::<SipHashStrategy, NoPruning, LazyExpansion>(&shared, 1..=3);
        }
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "threads are slow under Miri; the SipHash matrix runs the same code"
    )]
    fn presized_parallel_builds_are_equivalent_under_fxhash_and_colliding_hashes() {
        let shared = |arity: usize| inputs(arity);
        check_presized::<FxHashStrategy, NoPruning, EagerExpansion>(&shared, 1..=3);
        check_presized::<FxHashStrategy, SingletonPruning, EagerExpansion>(&shared, 1..=3);
        check_presized::<FxHashStrategy, NoPruning, LazyExpansion>(&shared, 1..=3);
        check_presized::<FxHashStrategy, SingletonPruning, LazyExpansion>(&shared, 1..=3);
        check_presized::<Mod10HashStrategy, NoPruning, EagerExpansion>(&shared, 1..=3);
        check_presized::<Mod10HashStrategy, SingletonPruning, EagerExpansion>(&shared, 1..=3);
        check_presized::<Mod10HashStrategy, NoPruning, LazyExpansion>(&shared, 1..=3);
        check_presized::<Mod10HashStrategy, SingletonPruning, LazyExpansion>(&shared, 1..=3);
    }

    /// Arity 4, a first key holding half the tuples, and three morsels at
    /// the real morsel size.
    #[test]
    #[cfg_attr(miri, ignore = "tens of thousands of inserts")]
    fn presized_parallel_builds_are_equivalent_on_large_and_skewed_inputs() {
        let large = |arity: usize| {
            let n = 2 * MORSEL_TUPLES + 7_000;
            let mut lcg = Lcg(0x94);
            let random: Vec<Vec<usize>> = (0..n)
                .map(|_| (0..arity).map(|_| lcg.next_usize() % 5_000).collect())
                .collect();
            let skewed: Vec<Vec<usize>> = (0..n)
                .map(|i| {
                    let first = if i % 2 == 0 {
                        7
                    } else {
                        lcg.next_usize() % 5_000
                    };
                    std::iter::once(first)
                        .chain((1..arity).map(|_| lcg.next_usize() % 50))
                        .collect()
                })
                .collect();
            vec![("random", random), ("half one key", skewed)]
        };
        check_presized::<SipHashStrategy, NoPruning, EagerExpansion>(&large, 1..=4);
        check_presized::<FxHashStrategy, SingletonPruning, LazyExpansion>(&large, 1..=4);
    }

    /// Every key homes at bucket 7, the last of the first 8-bucket region of
    /// a 32-bucket root (16 tuples at 70 %): the first key takes bucket 7,
    /// and every later distinct key overflows to the tail.
    #[derive(Copy, Clone, Default, Debug)]
    struct EndOfRegionHash;

    impl LayoutOption for EndOfRegionHash {
        const NAME: &'static str = "end-of-region";
    }

    impl HashStrategy for EndOfRegionHash {
        fn hash(key: usize) -> u64 { super::super::hash_table::hash_with_home(7, 5, key as u64) }
    }

    #[test]
    fn keys_that_cannot_fit_their_region_go_to_the_tail() {
        let config = tuples_config(70);
        let tuples: Vec<Vec<usize>> = (0..16).map(|i| vec![i % 4, i]).collect();
        assert_eq!(
            config.root_log2_capacity(tuples.len()),
            5,
            "a 32-bucket root"
        );
        let bulk: HashTrie<EndOfRegionHash> =
            HashTrie::from_tuples_with_config(2.into(), config, tuples.clone());
        PARALLEL_BUILDS.with(|b| b.borrow_mut().clear());
        let root = fill_presized_root_in::<EndOfRegionHash, NoPruning, EagerExpansion>(
            HashTrie::<EndOfRegionHash>::make_root_sized(2, 5),
            2,
            tuples,
            threads(2),
            7,
            8,
            config,
        );
        // Four distinct keys, each with four tuples: the first key's
        // tuples stay in the region, the other three keys' twelve go to
        // the tail.
        let builds = PARALLEL_BUILDS.with(|b| b.take());
        assert_eq!(builds.len(), 1);
        assert_eq!(builds[0].deferred, Some(12));
        assert_equivalent_root(bulk.root(), &root, "end of region");
    }

    /// `presized:N` reaches its own path, at the thread count asked: three
    /// worker runs (scatter, the regions' lists, then the regions'
    /// children), and a record with a deferred count.
    #[test]
    fn presized_build_reaches_its_own_path() {
        let tuples: Vec<Vec<usize>> = (0..64).map(|i| vec![i % 16, i]).collect();
        PARALLEL_BUILDS.with(|b| b.borrow_mut().clear());
        crate::morsel::take_worker_runs();
        let _: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            tuples_config(70),
            HashTrieBuildMode::Presized(threads(3)),
            tuples,
        );
        let builds = PARALLEL_BUILDS.with(|b| b.take());
        assert_eq!(builds.len(), 1);
        assert_eq!(builds[0].threads, 3);
        assert!(builds[0].deferred.is_some(), "the presized record");
        assert_eq!(builds[0].partition_sizes.iter().sum::<usize>(), 64);
        assert_eq!(crate::morsel::take_worker_runs(), vec![3, 3, 3]);
    }

    /// `parallel:N` is the exact merge build under every root capacity: a
    /// presized root changes the root's size, not the process, and the
    /// result is identical to bulk's under the same config.
    #[test]
    fn parallel_build_merges_under_every_root_capacity() {
        let tuples: Vec<Vec<usize>> = (0..64).map(|i| vec![i % 16, i]).collect();
        for root_capacity in [RootCapacity::Grow, RootCapacity::Tuples] {
            let config = HashTrieConfig {
                root_capacity,
                ..HashTrieConfig::default()
            };
            let bulk: HashTrie =
                HashTrie::from_tuples_with_config(2.into(), config, tuples.clone());
            PARALLEL_BUILDS.with(|b| b.borrow_mut().clear());
            crate::morsel::take_worker_runs();
            let built: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
                2.into(),
                config,
                HashTrieBuildMode::Parallel(threads(3)),
                tuples.clone(),
            );
            let builds = PARALLEL_BUILDS.with(|b| b.take());
            assert_eq!(builds.len(), 1, "{root_capacity:?}");
            assert_eq!(builds[0].threads, 3, "{root_capacity:?}");
            assert_eq!(
                builds[0].deferred, None,
                "{root_capacity:?}: the merge record"
            );
            assert_eq!(
                crate::morsel::take_worker_runs(),
                vec![3, 3],
                "{root_capacity:?}"
            );
            assert_same_trie(&bulk, &built, &format!("{root_capacity:?}"));
        }
    }

    /// The prerequisite at the library boundary: the CLI rejects this pair
    /// first, so a call that reaches here is a broken invariant.
    #[test]
    #[should_panic(expected = "hash-trie=presized:2 requires root-capacity=tuples")]
    fn presized_build_requires_a_presized_root() {
        let _: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            HashTrieConfig::default(),
            HashTrieBuildMode::Presized(threads(2)),
            vec![vec![1, 2]],
        );
    }

    /// Roots dense enough that many keys overflow their 8-bucket region,
    /// with 32 regions, more than any thread count here cuts partitions, so
    /// N changes how regions are grouped. The trie must still be the same for
    /// every N: regions alone decide what is deferred, and the tail goes in
    /// input order. The matrix's small inputs make at most four partitions
    /// at every N, and its large ones fill their roots to a few percent, so
    /// neither has a tail whose order or contents could depend on N.
    #[test]
    #[cfg_attr(miri, ignore = "compares thread counts; Miri runs one")]
    fn presized_parallel_builds_are_the_same_for_every_n_on_dense_roots() {
        let dense = |arity: usize| {
            let mut lcg = Lcg(0xD5);
            let mut tuple = |first: usize| -> Vec<usize> {
                std::iter::once(first)
                    .chain((1..arity).map(|_| lcg.next_usize() % 4))
                    .collect()
            };
            let distinct: Vec<Vec<usize>> = (0..240).map(&mut tuple).collect();
            let pairs: Vec<Vec<usize>> = (0..240).map(|i| tuple(i / 2)).collect();
            vec![("240 distinct keys", distinct), ("120 keys twice", pairs)]
        };
        PARALLEL_BUILDS.with(|b| b.borrow_mut().clear());
        check_presized::<SipHashStrategy, NoPruning, EagerExpansion>(&dense, 1..=3);
        check_presized::<FxHashStrategy, SingletonPruning, LazyExpansion>(&dense, 1..=3);
        let builds = PARALLEL_BUILDS.with(|b| b.take());
        let partition_counts: std::collections::BTreeSet<usize> =
            builds.iter().map(|b| b.partition_sizes.len()).collect();
        assert!(
            partition_counts.is_superset(&[4, 8, 16, 32].into()),
            "the thread counts did not cut the regions differently: {partition_counts:?}"
        );
        assert!(
            builds.iter().any(|b| b.deferred.is_some_and(|d| d > 0)),
            "nothing was deferred; the test proves nothing"
        );
    }
}
