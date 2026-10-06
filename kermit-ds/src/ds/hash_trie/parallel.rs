//! The `parallel:N` build of a [`HashTrie`](super::HashTrie): the radix
//! build's three steps (`radix.rs`), with the partition and build steps run
//! on N threads by the morsel-driven helpers of `crate::morsel` (Leis et
//! al., SIGMOD 2014; SIGMOD 2020 §3.3.2; issue #94).
//!
//! 1. **Partition.** [`scatter`] moves every tuple, with its input position,
//!    into one of P partitions by the top log₂P bits of its first attribute's
//!    hash; P is four per thread, rounded up to a power of two.
//! 2. **Build.** [`dispatch`] builds each non-empty partition into a scratch
//!    root by the serial build's own `insert_at`, then takes the scratch root's
//!    entries out, each tagged with the input position at which its key first
//!    appeared.
//! 3. **Merge.** The calling thread inserts every entry into the real root in
//!    first-appearance order, by a k-way merge of the partitions' lists.
//!
//! The result is the serial trie, bucket for bucket and capacity for
//! capacity, for the reasons the radix build's is. Each partition lists its
//! tuples in input order (`scatter` keeps it), so each subtrie receives the
//! serial build's `insert_at` calls; and the root receives its new keys in
//! the serial order. Equal 64-bit hashes share a root entry and also a
//! partition, because the partition is a function of the hash.

use {
    super::{
        config::LoadFactor,
        expansion::ExpansionPolicy,
        hash_table::HashTable,
        node::HashTrieNode,
        pruning::PruningPolicy,
        radix::{self, Arrival},
    },
    crate::morsel::{dispatch, scatter, Partition, Threads, MORSEL_TUPLES, PARTITIONS_PER_THREAD},
    kermit_iters::HashStrategy,
    std::{cmp::Reverse, collections::BinaryHeap},
};

#[cfg(any(test, feature = "test-hooks"))]
thread_local! {
    /// The `(threads, partition sizes)` of every parallel build on this
    /// thread: the size of each of its P partitions, empty ones (which the
    /// build skips) included. Every build mode builds the same trie, so only
    /// this record can tell a test which build ran, and where it put its
    /// tuples. Other crates' tests read it through `test_hooks` (the
    /// `test-hooks` feature).
    static PARALLEL_BUILDS: std::cell::RefCell<Vec<(usize, Vec<usize>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Takes this thread's record of parallel builds, oldest first, leaving it
/// empty: the `(threads, partition sizes)` of each.
#[cfg(feature = "test-hooks")]
pub(crate) fn take_parallel_builds() -> Vec<(usize, Vec<usize>)> {
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
    load_factor: LoadFactor,
) {
    fill_root_in_morsels::<H, P, E>(root, arity, tuples, threads, MORSEL_TUPLES, load_factor);
}

/// [`fill_root`] with the morsel size as a parameter, so tests can cut a
/// small input into many morsels.
fn fill_root_in_morsels<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    root: &mut HashTrieNode<P, E>, arity: usize, tuples: Vec<Vec<usize>>, threads: Threads,
    morsel_tuples: usize, load_factor: LoadFactor,
) {
    if tuples.is_empty() {
        // The serial build of nothing is the empty root: start no worker.
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
        builds.borrow_mut().push((threads.get(), sizes));
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
            radix::build_scratch_root::<H, P, E>(partition.into_tuples(), arity, load_factor);
        Entries::take_from(scratch, &first_seen)
    });
    // 3. Merge, on this thread, in first-appearance order.
    match root {
        | HashTrieNode::Inner(table) => {
            let lists = entries.into_iter().map(Entries::into_children).collect();
            merge_in_first_appearance_order(table, lists, load_factor);
        },
        | HashTrieNode::Leaf(table) => {
            let lists = entries.into_iter().map(Entries::into_chains).collect();
            merge_in_first_appearance_order(table, lists, load_factor);
        },
        | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
            unreachable!("a root is never pruned or unexpanded")
        },
    }
}

/// Inserts every partition's entries into the real root in the order their
/// keys first appeared in the input, the order the serial build inserts
/// them, so the root's buckets, length and capacity are the serial
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

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            ds::hash_trie::{
                build_mode::{HashTrieBuildMode, RadixBits},
                config::HashTrieConfig,
                expansion::{EagerExpansion, LazyExpansion},
                hash_table::HashTable,
                identity::{
                    assert_equivalent_root, assert_same_node, assert_same_trie, inputs,
                    LOAD_PERCENTS,
                },
                implementation::HashTrie,
                node::HashTrieNode,
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

    /// `parallel:N` builds the trie `serial` builds, for every arity, load
    /// factor, thread count and input. Each case runs twice: through the
    /// public constructor (morsels of 16 384 tuples, the whole trie
    /// compared), and with morsels of 7 tuples, which make the partition
    /// step cut the input many times (the root compared, which holds the
    /// whole trie). Miri runs the second only.
    fn check_identity<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for &percent in LOAD_PERCENTS {
                let config = HashTrieConfig {
                    load_factor: LoadFactor::percent(percent).unwrap(),
                };
                for (input, tuples) in inputs(arity) {
                    let serial = HashTrie::<H, P, E>::from_tuples_with_config(
                        arity.into(),
                        config,
                        tuples.clone(),
                    );
                    for &t in THREADS {
                        let label = format!(
                            "{}/{}/{} arity {arity}, load {percent}%, parallel:{t}, {input}",
                            H::NAME,
                            P::NAME,
                            E::NAME
                        );
                        if !cfg!(miri) {
                            let built = HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
                                arity.into(),
                                config,
                                HashTrieBuildMode::Parallel(threads(t)),
                                tuples.clone(),
                            );
                            assert_same_trie(&serial, &built, &label);
                        }
                        let mut root = HashTrie::<H, P, E>::make_root(arity);
                        fill_root_in_morsels::<H, P, E>(
                            &mut root,
                            arity,
                            tuples.clone(),
                            threads(t),
                            7,
                            config.load_factor,
                        );
                        assert_same_node(serial.root(), &root, &format!("{label}, morsels of 7"));
                    }
                }
            }
        }
    }

    #[test]
    fn parallel_builds_the_serial_trie_under_siphash() {
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
    fn parallel_builds_the_serial_trie_under_fxhash() {
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
    fn parallel_builds_the_serial_trie_under_colliding_hashes() {
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
    fn parallel_builds_the_serial_trie_on_large_and_skewed_inputs() {
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
                    let serial = HashTrie::<H, P, E>::from_tuples(arity.into(), tuples.clone());
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
                        assert_same_trie(&serial, &built, &label);
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
    /// ran, and that `parallel:N` spreads its work: N workers in both steps
    /// (two `run_workers` calls), and the tuples spread over the partitions.
    /// Eight first keys of eight tuples each fill the eight partitions of
    /// `parallel:2` evenly. `parallel:3` aims at 12 partitions, rounded up to
    /// 16, so key k lands in partition 2k: every other partition is empty,
    /// and the record lists those too. A second thread count is what shows
    /// that N itself reaches the build. The radix build partitions too, but
    /// serially.
    #[test]
    fn build_modes_reach_their_builds() {
        let tuples: Vec<Vec<usize>> = (0..64).map(|i| vec![i % 8, i]).collect();
        let serial: HashTrie<TopBitsHash> = HashTrie::from_tuples(2.into(), tuples.clone());
        for (mode, builds, worker_runs) in [
            (HashTrieBuildMode::Serial, vec![], vec![]),
            (
                HashTrieBuildMode::Radix(RadixBits::new(3).unwrap()),
                vec![],
                vec![],
            ),
            (
                HashTrieBuildMode::Parallel(threads(2)),
                vec![(2, vec![8; 8])],
                vec![2, 2],
            ),
            (
                HashTrieBuildMode::Parallel(threads(3)),
                vec![(3, [8, 0].repeat(8))],
                vec![3, 3],
            ),
        ] {
            PARALLEL_BUILDS.with(|builds| builds.borrow_mut().clear());
            crate::morsel::take_worker_runs();
            let built: HashTrie<TopBitsHash> =
                HashTrie::from_tuples_with_build_mode(2.into(), mode, tuples.clone());
            assert_eq!(PARALLEL_BUILDS.with(|b| b.take()), builds, "{mode:?}");
            assert_eq!(crate::morsel::take_worker_runs(), worker_runs, "{mode:?}");
            assert_same_trie(&serial, &built, &format!("{mode:?}"));
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

    /// The log2 capacity of a built trie's root.
    fn root_log2<P: PruningPolicy, E: ExpansionPolicy>(root: &HashTrieNode<P, E>) -> u32 {
        match root {
            | HashTrieNode::Inner(table) => table.buckets_len().trailing_zeros(),
            | HashTrieNode::Leaf(table) => table.buckets_len().trailing_zeros(),
            | _ => unreachable!("a root is never pruned or unexpanded"),
        }
    }

    /// The mirrored root step, alone: one run over the whole presized root,
    /// the overflow inserted afterwards in input order. Below the root it
    /// must build serial's trie exactly; the root is equivalent
    /// (Amendment 2). A drift between `insert_at`'s root level and its
    /// mirror shows here before any threading is involved. The root is
    /// presized to the capacity serial's root grew to, which linear
    /// probing's order independence makes equivalent.
    fn check_root_step<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for &percent in LOAD_PERCENTS {
                let config = HashTrieConfig {
                    load_factor: LoadFactor::percent(percent).unwrap(),
                };
                for (input, tuples) in inputs(arity) {
                    let label = format!(
                        "{}/{}/{} arity {arity}, load {percent}%, {input}",
                        H::NAME,
                        P::NAME,
                        E::NAME
                    );
                    let serial = HashTrie::<H, P, E>::from_tuples_with_config(
                        arity.into(),
                        config,
                        tuples.clone(),
                    );
                    let log2 = root_log2(serial.root());
                    let mut root: HashTrieNode<P, E> = if arity == 1 {
                        HashTrieNode::Leaf(HashTable::with_log2_capacity(log2))
                    } else {
                        HashTrieNode::Inner(HashTable::with_log2_capacity(log2))
                    };
                    let lf = config.load_factor;
                    let tail: Vec<Vec<usize>> = match &mut root {
                        | HashTrieNode::Inner(table) => {
                            table.with_runs(1, 1 << log2, |mut runs| {
                                tuples
                                    .iter()
                                    .cloned()
                                    .filter_map(|t| {
                                        HashTrie::<H, P, E>::insert_at_inner_root_in_run(
                                            &mut runs[0],
                                            arity,
                                            t,
                                            lf,
                                        )
                                        .err()
                                    })
                                    .collect()
                            })
                        },
                        | HashTrieNode::Leaf(table) => table.with_runs(1, 1 << log2, |mut runs| {
                            tuples
                                .iter()
                                .cloned()
                                .filter_map(|t| {
                                    HashTrie::<H, P, E>::insert_at_leaf_root_in_run(&mut runs[0], t)
                                        .err()
                                })
                                .collect()
                        }),
                        | _ => unreachable!("a root is never pruned or unexpanded"),
                    };
                    for t in tail {
                        HashTrie::<H, P, E>::insert_at(&mut root, 0, arity, t, lf);
                    }
                    assert_equivalent_root(serial.root(), &root, &label);
                }
            }
        }
    }

    #[test]
    fn the_root_step_builds_the_serial_trie_below_the_root() {
        check_root_step::<SipHashStrategy, NoPruning, EagerExpansion>();
        check_root_step::<SipHashStrategy, SingletonPruning, EagerExpansion>();
        check_root_step::<SipHashStrategy, NoPruning, LazyExpansion>();
        check_root_step::<SipHashStrategy, SingletonPruning, LazyExpansion>();
        check_root_step::<FxHashStrategy, SingletonPruning, LazyExpansion>();
        check_root_step::<Mod10HashStrategy, SingletonPruning, EagerExpansion>();
    }
}
