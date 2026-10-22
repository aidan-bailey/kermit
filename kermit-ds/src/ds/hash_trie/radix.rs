//! The `radix:K` build of a [`HashTrie`](super::HashTrie): copy the rows
//! into `2^K` partitions on their first attribute's hash
//! ([`partition_by_first_hash`]), group each partition into a scratch root
//! and build its children by Algorithm 2 (`bulk.rs`), then merge the scratch
//! roots into the real one ([`build_and_merge`]; SIGMOD 2020 §3.3.2; issues
//! #91, #101). `parallel:N` (`parallel.rs`) runs the same steps on N
//! threads; `radix:K` runs them on the calling thread, with the whole batch
//! as one morsel.
//!
//! The copy is the paper's materialisation step: "the tuples are
//! partitioned based on the hash values of the first join key attribute"
//! (§3.3.2). It becomes the trie's buffer and the input is freed, so each
//! partition's rows are one contiguous range of the buffer, in input order,
//! and every row a partition's build reads is local. One round of `2^K`
//! partitions is kermit's choice; the paper adapts a two-pass radix
//! partitioning (Balkesen et al.).
//!
//! Every list is a [`Chain`] threaded through the trie's links, one per row
//! of the copy ([`ChainLinks`]): the paper's chain pointer (§3.3.2, Figure
//! 3), kept as a 4-byte id in an array beside the buffer rather than 8 bytes
//! inside each tuple (kermit's). A partition's scratch root threads the
//! partition's rows into its root chains, and Algorithm 2 re-threads each
//! chain into the next level's, so building a partition writes the links of
//! its own rows and of no others: partitions build on separate threads with
//! one writer per link. The merge moves `Chain` values and finished
//! children, whose ids name rows of the copy, and writes no link.
//!
//! The result is equivalent to the bulk build's trie (Amendment 2): the
//! same tables bucket for bucket and capacity for capacity, and the same
//! tuples in every chain, singleton and pending chain, under the copy's row
//! ids. A table's final layout depends only on the order in which its
//! *new* keys arrive — `HashTable::entry_or_insert_with` returns an
//! existing entry before its resize check — so the merge inserts the
//! distinct root keys in the order they first appear in the input, as the
//! bulk build does. The copy records each row's input position (`origin`),
//! each key is tagged with the position of the row that introduced it, and
//! [`merge_in_first_appearance_order`] merges the partitions' tagged lists
//! k-way on that tag. Below the root nothing needs care: the copy is
//! stable, so each root key's child is built from the same tuples, in input
//! order, as in the bulk build.

use {
    super::{
        build_mode::RadixBits,
        chain::{self, Chain},
        config::{HashTrieConfig, LoadFactor},
        expansion::ExpansionPolicy,
        hash_table::HashTable,
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::PruningPolicy,
    },
    crate::morsel::{dispatch, partition_rows, Partitioned, Threads},
    kermit_iters::{ChainLinks, HashStrategy, RowId, TupleView, Tuples},
    std::{cmp::Reverse, collections::BinaryHeap},
};

/// A root entry ready to merge: the input position of the row whose key
/// first introduced it, its hash, and its finished value.
pub(super) type Arrival<V> = (RowId, u64, V);

/// One partition's root entries, each tagged with the input position at
/// which its key first appeared, in that order. The root is `Inner` for
/// arity ≥ 2, so its values are child nodes (subtries, or under pruning and
/// lazy expansion their `Singleton`s and pending chains), and `Leaf` for
/// arity 1, so its values are chains. Either way every id in them names a
/// row of the copy, so they move into the real root as they are.
pub(super) enum Entries<P: PruningPolicy, E: ExpansionPolicy> {
    Children(Vec<Arrival<HashTrieNode<P, E>>>),
    Chains(Vec<Arrival<Chain>>),
}

impl<P: PruningPolicy, E: ExpansionPolicy> Entries<P, E> {
    /// Moves every entry out of a finished scratch root, in the order its
    /// keys arrived (`first_seen`, from [`build_scratch_root`]).
    fn take_from(scratch: HashTrieNode<P, E>, first_seen: &[(RowId, u64)]) -> Self {
        match scratch {
            | HashTrieNode::Inner(table) => {
                let mut children = Vec::with_capacity(first_seen.len());
                take_in_arrival_order(table, first_seen, &mut children);
                Self::Children(children)
            },
            | HashTrieNode::Leaf(table) => {
                let mut chains = Vec::with_capacity(first_seen.len());
                take_in_arrival_order(table, first_seen, &mut chains);
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

    fn into_chains(self) -> Vec<Arrival<Chain>> {
        match self {
            | Self::Chains(chains) => chains,
            | Self::Children(_) => unreachable!("a Leaf root's scratch roots are Leaf"),
        }
    }
}

/// Fills the empty `root` with the rows of `tuples` by the `radix:bits`
/// build, threading every list through `links` (one link per row of
/// `tuples`), and returns the trie's buffer: the rows copied into partition
/// order. Every row must have `arity` attributes; the caller checks.
pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    tuples: Tuples, links: &ChainLinks, root: &mut HashTrieNode<P, E>, arity: usize,
    bits: RadixBits, config: HashTrieConfig,
) -> Tuples {
    if tuples.is_empty() {
        // The bulk build of nothing is the empty root over the empty batch:
        // copy nothing and start no worker, as `parallel:N` does.
        return tuples;
    }
    // 1. Partition, on this thread: copy the rows into `2^bits` partitions on
    //    the top `bits` of their first attribute's hash. Each partition is one
    //    range of the copy, in input order. The whole batch is one morsel:
    //    `partition_rows` keeps a count and a range per (partition, morsel)
    //    pair, which one thread gains nothing from, and at `radix:16` over 10^7
    //    rows in `MORSEL_TUPLES`-row morsels would be about 40 million pairs.
    let one = Threads::new(1).expect("one thread is a valid count");
    let one_morsel = tuples.len();
    let partitioned = partition_by_first_hash::<H>(one, &tuples, one_morsel, u32::from(bits.get()));
    // The copy is the buffer from here on: free the input before the build,
    // so the build holds the rows once.
    drop(tuples);
    // 2 and 3, on this thread.
    build_and_merge::<H, P, E>(partitioned, links, root, arity, one, config)
}

/// Step 1 of `radix:K` and `parallel:N`: copies the rows of `tuples` into
/// `2^bits` partitions on the top `bits` bits of their first attribute's
/// hash, each partition's rows in input order, with each copied row's input
/// position in `origin` (`morsel::partition_rows`, on `threads` workers over
/// morsels of `morsel_rows` rows). `bits` must be in `1..=16`.
pub(super) fn partition_by_first_hash<H: HashStrategy>(
    threads: Threads, tuples: &Tuples, morsel_rows: usize, bits: u32,
) -> Partitioned {
    debug_assert!(
        (1..=16).contains(&bits),
        "partition_by_first_hash: {bits} bits, not 1 to 16"
    );
    let shift = 64 - bits;
    partition_rows(
        threads,
        &TupleView::all(tuples),
        morsel_rows,
        1 << bits,
        |row| (H::hash(row[0]) >> shift) as usize,
    )
}

/// Steps 2 and 3 of `radix:K` and `parallel:N`. Each non-empty partition
/// of `partitioned` is built into a scratch root ([`build_scratch_root`])
/// on one of `threads` workers, which threads that partition's rows, and
/// only those, through `links`; then the calling thread merges the scratch
/// roots' entries into the empty `root` in first-appearance order
/// ([`merge_in_first_appearance_order`]), writing no link. Returns the
/// copy, the trie's buffer: every id in `root` names a row of it, and
/// `links` holds one link per row of it.
pub(super) fn build_and_merge<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    partitioned: Partitioned, links: &ChainLinks, root: &mut HashTrieNode<P, E>, arity: usize,
    threads: Threads, config: HashTrieConfig,
) -> Tuples {
    // An empty partition would build and drop an empty scratch root.
    let ranges: Vec<_> = (0..partitioned.partitions())
        .map(|p| partitioned.ids(p))
        .filter(|ids| !ids.is_empty())
        .collect();
    // 2. Build: each partition's id range into a scratch root, whose entries
    //    then leave in the order their keys arrived, each tagged with its key's
    //    first input position (`origin`), not its id. A partition's rows are in
    //    exactly one of these builds, so each link has one writer, and the
    //    relaxed atomics need no lock between the workers.
    let (rows, origin) = (&partitioned.tuples, &partitioned.origin);
    let entries = dispatch(threads, ranges, |ids| {
        let (scratch, first_seen) =
            build_scratch_root::<H, P, E>(rows, links, ids, origin, arity, config);
        Entries::take_from(scratch, &first_seen)
    });
    // Every key's first input position is in its entry now, so nothing reads
    // `origin` or `offsets` again: free them before the merge.
    drop(partitioned.origin);
    drop(partitioned.offsets);
    // 3. Merge, on this thread, in first-appearance order. The workers were
    //    joined inside `dispatch`, so every link they wrote is visible here.
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
    partitioned.tuples
}

/// Builds one partition, the rows `ids` of `tuples`, into a scratch root of
/// the real root's kind by Algorithm 2 (`bulk.rs`): each row pushed onto
/// the chain in its key's bucket, threaded through `links` (lines 3–7),
/// recording for each key the input position of the row that introduced it
/// (`origin[id]`) and the key's hash, in arrival order; then each key's
/// child built from its chain, as the bulk build builds it (lines 8–15).
/// `ids` must list the partition in input order. Only the links of `ids`'
/// rows are written, so partitions can build on separate threads.
pub(super) fn build_scratch_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    tuples: &Tuples, links: &ChainLinks, ids: impl IntoIterator<Item = RowId>, origin: &[RowId],
    arity: usize, config: HashTrieConfig,
) -> (HashTrieNode<P, E>, Vec<(RowId, u64)>) {
    // Lines 3–7 at the root. The scratch root starts at 4 buckets and
    // grows: its entries move into the real root, so its size is never
    // seen. `push_row` starts a chain exactly when the row's key is new.
    let mut lists: HashTable<Chain> = HashTable::new();
    let mut first_seen = Vec::new();
    for row in ids {
        let hash = H::hash(tuples.row(row)[0]);
        if chain::push_row(&mut lists, hash, config.load_factor, links, row) {
            first_seen.push((origin[row as usize], hash));
        }
    }
    // Lines 8–15: each chain re-threaded into its child's chains.
    let scratch = HashTrie::<H, P, E>::build_nested(tuples, links, 0, arity, lists, config);
    (scratch, first_seen)
}

/// Moves every entry out of `scratch` into `out`, tagged with the input
/// position of the row whose key first introduced it. Bucket positions are
/// read before the table is consumed, and each bucket is taken exactly
/// once.
fn take_in_arrival_order<V>(
    scratch: HashTable<V>, first_seen: &[(RowId, u64)], out: &mut Vec<Arrival<V>>,
) {
    let positions: Vec<usize> = first_seen
        .iter()
        .map(|&(_, hash)| {
            scratch
                .index_of(hash)
                .expect("every arrival is in its scratch table")
        })
        .collect();
    let mut buckets = scratch.into_buckets();
    for (&(first, hash), position) in first_seen.iter().zip(positions) {
        let entry = buckets[position].take().expect("each bucket is taken once");
        debug_assert_eq!(entry.hash, hash);
        out.push((first, hash, entry.value));
    }
}

/// Inserts every partition's entries into the real root in the order their
/// keys first appeared in the input, the order the bulk build inserts
/// them, so the root's buckets, length and capacity are the bulk build's.
/// Each entry's tag is an input position (`origin`, through
/// [`build_scratch_root`]), not an id into the copy: ids run
/// partition-major, so merging on them would insert each partition's keys
/// in turn. Each list is already in that order, so a k-way merge suffices:
/// a heap pop and push per entry over at most P lists, O(D log P) for D
/// root keys. It runs on the calling thread: the build's sequential step.
fn merge_in_first_appearance_order<V>(
    root: &mut HashTable<V>, lists: Vec<Vec<Arrival<V>>>, load_factor: LoadFactor,
) {
    let mut lists: Vec<_> = lists
        .into_iter()
        .map(|list| list.into_iter().peekable())
        .collect();
    // Each list's next first-appearance position, smallest on top. Positions
    // are distinct, so no two heads tie.
    let mut heads: BinaryHeap<Reverse<(RowId, usize)>> = lists
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
                build_mode::HashTrieBuildMode,
                config::HashTrieConfig,
                expansion::{EagerExpansion, LazyExpansion},
                identity::{assert_same_contents, assert_same_trie, configs, inputs, rows},
                pruning::{NoPruning, SingletonPruning},
            },
            heap_size::HeapSize,
            relation::{BuildModeRelation, ConfigurableRelation, Relation},
            test_support::Mod10HashStrategy,
        },
        kermit_iters::{FxHashStrategy, SipHashStrategy},
    };

    /// Bit counts under test. Sixteen bits make 65,536 partitions, too slow
    /// to set up a thousand times under Miri.
    const BITS: &[u8] = if cfg!(miri) {
        &[1, 4]
    } else {
        &[1, 4, 16]
    };

    /// `radix:K` builds a trie equivalent to `bulk`'s, for every arity, load
    /// factor, root capacity, child capacity, bit count and input.
    /// `assert_same_contents` compares every table bucket for bucket, so the
    /// root is `bulk`'s (Decision 3 of the partitioned-materialisation
    /// design), and every chain, singleton and pending chain holds the same
    /// tuples in the same order under the copy's row ids, each chain walked
    /// through its own trie's links. The heap size is compared too: `space`
    /// must not move with the mode, and every mode keeps one link per row.
    fn check_contents<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for config in configs() {
                for &bits in BITS {
                    let radix = HashTrieBuildMode::Radix(RadixBits::new(bits).unwrap());
                    for (input, tuples) in inputs(arity) {
                        let build = |mode| {
                            HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
                                arity.into(),
                                config,
                                mode,
                                tuples.clone(),
                            )
                        };
                        let label = format!(
                            "{}/{}/{} arity {arity}, load {}%, root {}, children {}, \
                             radix:{bits}, {input}",
                            H::NAME,
                            P::NAME,
                            E::NAME,
                            config.load_factor.numerator(),
                            config.root_capacity.axis_value(),
                            config.child_capacity.axis_value(),
                        );
                        let (bulk, built) = (build(HashTrieBuildMode::Bulk), build(radix));
                        assert_same_contents(&bulk, &built, &label);
                        assert_eq!(
                            bulk.heap_size_bytes(),
                            built.heap_size_bytes(),
                            "{label}: heap size"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn radix_builds_the_bulk_trie_under_siphash() {
        check_contents::<SipHashStrategy, NoPruning, EagerExpansion>();
        check_contents::<SipHashStrategy, SingletonPruning, EagerExpansion>();
        check_contents::<SipHashStrategy, NoPruning, LazyExpansion>();
        check_contents::<SipHashStrategy, SingletonPruning, LazyExpansion>();
    }

    #[test]
    fn radix_builds_the_bulk_trie_under_fxhash() {
        check_contents::<FxHashStrategy, NoPruning, EagerExpansion>();
        check_contents::<FxHashStrategy, SingletonPruning, EagerExpansion>();
        check_contents::<FxHashStrategy, NoPruning, LazyExpansion>();
        check_contents::<FxHashStrategy, SingletonPruning, LazyExpansion>();
    }

    /// Every hash is below 10, so every tuple lands in partition 0, and
    /// distinct keys share full hashes, root entries and leaf chains.
    #[test]
    fn radix_builds_the_bulk_trie_under_colliding_hashes() {
        check_contents::<Mod10HashStrategy, NoPruning, EagerExpansion>();
        check_contents::<Mod10HashStrategy, SingletonPruning, EagerExpansion>();
        check_contents::<Mod10HashStrategy, NoPruning, LazyExpansion>();
        check_contents::<Mod10HashStrategy, SingletonPruning, LazyExpansion>();
    }

    #[test]
    #[should_panic(expected = "tuple arity")]
    fn radix_build_rejects_a_wrong_arity() {
        let _: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            HashTrieConfig::default(),
            HashTrieBuildMode::Radix(RadixBits::new(4).unwrap()),
            vec![vec![1, 2, 3], vec![4, 5, 6]],
        );
    }

    /// The radix build of nothing is the empty trie over the input batch,
    /// built without partitioning or starting a worker, as `parallel:N`'s
    /// is (`parallel_build_of_nothing_starts_no_worker`).
    #[test]
    fn radix_build_of_nothing_starts_no_worker() {
        crate::morsel::take_worker_runs();
        let built: HashTrie = HashTrie::from_tuples_with_build_mode(
            2.into(),
            HashTrieBuildMode::Radix(RadixBits::new(4).unwrap()),
            vec![],
        );
        assert_same_trie(&HashTrie::from_tuples(2.into(), vec![]), &built, "empty");
        assert!(crate::morsel::take_worker_runs().is_empty());
    }

    /// `BuildModeRelation` builds with the default config, and its default
    /// mode is `Relation::from_tuples`.
    #[test]
    fn build_mode_relation_uses_the_default_config() {
        let radix = HashTrieBuildMode::Radix(RadixBits::new(2).unwrap());
        let tuples = rows(2, &[[1, 2, 0], [3, 4, 0], [1, 5, 0]]);
        let built: HashTrie =
            HashTrie::from_tuples_with_build_mode(2.into(), radix, tuples.clone());
        assert_eq!(*built.config(), HashTrieConfig::default());
        let plain: HashTrie = HashTrie::from_tuples(2.into(), tuples.clone());
        let default: HashTrie =
            HashTrie::from_tuples_with_build_mode(2.into(), HashTrieBuildMode::default(), tuples);
        assert_same_trie(&plain, &default, "from_tuples vs the default mode");
        assert_same_contents(&plain, &built, "from_tuples vs radix:2");
    }

    /// The copy is what `radix:K` adds, and the content tests cannot see
    /// it: a build that left the rows in arrival order would build the same
    /// contents. So the trie's buffer must be `partition_rows`' copy of its
    /// input, partitioned on the top `K` bits of the first attribute's hash,
    /// row for row and at exactly their size. The random input spreads
    /// over every bit count's partitions, so its copy is not the input and
    /// the test cannot pass vacuously.
    #[test]
    fn radix_keeps_the_partitioned_copy_as_its_buffer() {
        let one = Threads::new(1).expect("one thread is a valid count");
        for arity in 1..=3 {
            for &bits in BITS {
                let shift = 64 - u32::from(bits);
                let radix = HashTrieBuildMode::Radix(RadixBits::new(bits).unwrap());
                for (input, tuples) in inputs(arity) {
                    let label = format!("arity {arity}, radix:{bits}, {input}");
                    let one_morsel = tuples.len().max(1);
                    let expected = partition_rows(
                        one,
                        &TupleView::all(&tuples),
                        one_morsel,
                        1 << bits,
                        |row| (SipHashStrategy::hash(row[0]) >> shift) as usize,
                    );
                    if input == "random" {
                        assert_ne!(expected.tuples, tuples, "{label}: the copy reorders");
                    }
                    let built: HashTrie =
                        HashTrie::from_tuples_with_build_mode(arity.into(), radix, tuples.clone());
                    assert_eq!(built.tuples(), &expected.tuples, "{label}: buffer");
                    assert_eq!(
                        built.tuples().value_capacity(),
                        tuples.len() * arity,
                        "{label}: capacity"
                    );
                }
            }
        }
    }
}
