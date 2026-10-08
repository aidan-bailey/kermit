//! The `radix:K` build of a [`HashTrie`](super::HashTrie): copy the rows
//! into `2^K` partitions on their first attribute's hash
//! (`morsel::partition_rows`), group each partition into a scratch root and
//! build its children by Algorithm 2 (`bulk.rs`), then merge the scratch
//! roots into the real one (SIGMOD 2020 §3.3.2; issues #91, #101).
//!
//! The copy is the paper's materialisation step: "the tuples are
//! partitioned based on the hash values of the first join key attribute"
//! (§3.3.2). It becomes the trie's buffer and the input is freed, so each
//! partition's rows are one contiguous range of the buffer, in input order,
//! and every row a partition's build reads is local. One round of `2^K`
//! partitions is kermit's choice; the paper adapts a two-pass radix
//! partitioning (Balkesen et al.).
//!
//! The result is equivalent to the bulk build's trie (Amendment 2): the
//! same tables bucket for bucket and capacity for capacity, and the same
//! tuples in every chain, singleton and pending list, under the copy's row
//! ids. A table's final layout depends only on the order in which its
//! *new* keys arrive — `HashTable::entry_or_insert_with` returns an
//! existing entry before its resize check — so the merge inserts the
//! distinct root keys in the order they first appear in the input, as the
//! bulk build does. The copy records each row's input position (`origin`),
//! and each key is tagged with the position of the row that introduced it.
//! Below the root nothing needs care: the copy is stable, so each root
//! key's child is built from the same tuples, in input order, as in the
//! bulk build.

use {
    super::{
        build_mode::RadixBits,
        bulk::TupleList,
        config::{HashTrieConfig, LoadFactor},
        expansion::ExpansionPolicy,
        hash_table::HashTable,
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::PruningPolicy,
    },
    crate::morsel::{partition_rows, Threads},
    kermit_iters::{HashStrategy, RowId, Tuples},
};

/// A root entry ready to merge: the input position of the row whose key
/// first introduced it, its hash, and its finished value.
pub(super) type Arrival<V> = (RowId, u64, V);

/// Fills the empty `root` with the rows of `tuples` by the `radix:bits`
/// build and returns the trie's buffer: the rows copied into partition
/// order. Every row must have `arity` attributes; the caller checks.
pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    tuples: Tuples, root: &mut HashTrieNode<P, E>, arity: usize, bits: RadixBits,
    config: HashTrieConfig,
) -> Tuples {
    // 1. Partition, on this thread: copy the rows into `2^bits` partitions on
    //    the top `bits` of their first attribute's hash. Each partition is one
    //    range of the copy, in input order. The whole batch is one morsel:
    //    `partition_rows` keeps a count and a range per (partition, morsel)
    //    pair, which one thread gains nothing from, and at `radix:16` over 10^7
    //    rows in `MORSEL_TUPLES`-row morsels would be about 40 million pairs.
    let shift = 64 - u32::from(bits.get());
    let one = Threads::new(1).expect("one thread is a valid count");
    let one_morsel = tuples.len().max(1);
    let partitioned = partition_rows(one, &tuples, one_morsel, 1 << bits.get(), |row| {
        (H::hash(row[0]) >> shift) as usize
    });
    // The copy is the buffer from here on: free the input before the build,
    // so the build holds the rows once.
    drop(tuples);
    // 2. Build each partition into a scratch root. One of the two lists stays
    //    empty: the root is `Inner` for arity ≥ 2 (values are subtries, or
    //    under lazy expansion their pending lists) and `Leaf` for arity 1
    //    (values are chains).
    let mut subtries = Vec::new();
    let mut chains = Vec::new();
    for partition in 0..partitioned.partitions() {
        let ids = partitioned.ids(partition);
        if ids.is_empty() {
            continue;
        }
        let (scratch, first_seen) = build_scratch_root::<H, P, E>(
            &partitioned.tuples,
            ids,
            &partitioned.origin,
            arity,
            config,
        );
        match scratch {
            | HashTrieNode::Inner(table) => {
                take_in_arrival_order(table, &first_seen, &mut subtries)
            },
            | HashTrieNode::Leaf(table) => take_in_arrival_order(table, &first_seen, &mut chains),
            | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
                unreachable!("a root is never pruned or unexpanded")
            },
        }
    }
    // 3. Merge, in first-appearance order.
    match root {
        | HashTrieNode::Inner(table) => {
            insert_in_first_appearance_order(table, subtries, config.load_factor)
        },
        | HashTrieNode::Leaf(table) => {
            insert_in_first_appearance_order(table, chains, config.load_factor)
        },
        | HashTrieNode::Singleton(_) | HashTrieNode::Unexpanded(_) => {
            unreachable!("a root is never pruned or unexpanded")
        },
    }
    partitioned.tuples
}

/// Builds one partition, the rows `ids` of `tuples`, into a scratch root of
/// the real root's kind by Algorithm 2 (`bulk.rs`): the ids grouped at the
/// root, recording for each key the input position of the row that
/// introduced it (`origin[id]`) and the key's hash, in arrival order; then
/// each key's child built from its list, as the bulk build builds it.
/// `ids` must list the partition in input order.
pub(super) fn build_scratch_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    tuples: &Tuples, ids: impl IntoIterator<Item = RowId>, origin: &[RowId], arity: usize,
    config: HashTrieConfig,
) -> (HashTrieNode<P, E>, Vec<(RowId, u64)>) {
    // Lines 3–7 at the root. The scratch root starts at 4 buckets and
    // grows: its entries move into the real root, so its size is never
    // seen.
    let mut lists: HashTable<TupleList> = HashTable::new();
    let mut first_seen = Vec::new();
    for row in ids {
        let hash = H::hash(tuples.row(row)[0]);
        let keys_before = lists.len();
        lists
            .entry_or_insert_with(hash, config.load_factor, Vec::new)
            .push(row);
        if lists.len() > keys_before {
            first_seen.push((origin[row as usize], hash));
        }
    }
    // Lines 8–15.
    let scratch = HashTrie::<H, P, E>::build_nested(tuples, 0, arity, lists, config);
    (scratch, first_seen)
}

/// Moves every entry out of `scratch` into `out`, tagged with the input
/// position of the row whose key first introduced it. Bucket positions are
/// read before the table is consumed, and each bucket is taken exactly
/// once.
pub(super) fn take_in_arrival_order<V>(
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

/// Inserts the merged entries into the real root in the order their keys
/// first appeared in the input — the order the bulk build inserts them —
/// so the root's buckets, length and capacity are the bulk build's.
fn insert_in_first_appearance_order<V>(
    root: &mut HashTable<V>, mut arrivals: Vec<Arrival<V>>, load_factor: LoadFactor,
) {
    // Input positions are distinct (`origin` is a permutation), so the
    // unstable sort is deterministic.
    arrivals.sort_unstable_by_key(|&(first, ..)| first);
    for (_, hash, value) in arrivals {
        root.entry_or_insert_with(hash, load_factor, || value);
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
    /// design), and every chain, singleton and pending list holds the same
    /// tuples in the same order under the copy's row ids. The heap size is
    /// compared too: `space` must not move with the mode.
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
    /// row for row and at the input's capacity. The random input spreads
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
                    let expected = partition_rows(one, &tuples, one_morsel, 1 << bits, |row| {
                        (SipHashStrategy::hash(row[0]) >> shift) as usize
                    });
                    if input == "random" {
                        assert_ne!(expected.tuples, tuples, "{label}: the copy reorders");
                    }
                    let built: HashTrie =
                        HashTrie::from_tuples_with_build_mode(arity.into(), radix, tuples.clone());
                    assert_eq!(built.tuples(), &expected.tuples, "{label}: buffer");
                    assert_eq!(
                        built.tuples().value_capacity(),
                        tuples.value_capacity(),
                        "{label}: capacity"
                    );
                }
            }
        }
    }
}
