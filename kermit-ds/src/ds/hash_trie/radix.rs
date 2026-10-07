//! The `radix:K` build of a [`HashTrie`](super::HashTrie): radix-partition
//! the tuples on their first attribute's hash, group each partition into a
//! scratch root and build its children by Algorithm 2 (`bulk.rs`), then merge
//! the scratch roots into the real one (SIGMOD 2020 §3.3.2; issue #91).
//!
//! The result is the trie the bulk build makes, bucket for bucket and
//! capacity for capacity. A table's final layout depends only on the order
//! in which its *new* keys arrive — `HashTable::entry_or_insert_with`
//! returns an existing entry before its resize check — so the merge inserts
//! the distinct root keys in the order they first appear in the input, as
//! the bulk build does. Below the root nothing needs care: the partition
//! is stable, so each root key's child is built from the same list, in input
//! order, as in the bulk build.

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
    kermit_iters::HashStrategy,
};

/// A tuple with its position in the input, which the merge orders by.
type Indexed = (usize, Vec<usize>);

/// A root entry ready to merge: the input position at which its key first
/// appeared, its hash, and its finished value.
pub(super) type Arrival<V> = (usize, u64, V);

/// Fills the empty `root` with `tuples` by the `radix:bits` build. Every
/// tuple must have `arity` attributes; the caller checks.
pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    root: &mut HashTrieNode<P, E>, arity: usize, tuples: Vec<Vec<usize>>, bits: RadixBits,
    config: HashTrieConfig,
) {
    // One of the two stays empty: the root is `Inner` for arity ≥ 2 (values
    // are subtries, or under lazy expansion their pending lists) and `Leaf`
    // for arity 1 (values are chains).
    let mut subtries = Vec::new();
    let mut chains = Vec::new();
    for partition in partition::<H>(tuples, bits) {
        if partition.is_empty() {
            continue;
        }
        let (scratch, first_seen) = build_scratch_root::<H, P, E>(partition, arity, config);
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
}

/// Splits `tuples` into `2^bits` partitions on the top `bits` of
/// `H::hash(tuple[0])`. A histogram pass sizes each partition exactly; a
/// scatter pass then moves every tuple into its partition with its input
/// index. Stable — each partition keeps input order — which is what keeps
/// every child identical to the bulk build's.
fn partition<H: HashStrategy>(tuples: Vec<Vec<usize>>, bits: RadixBits) -> Vec<Vec<Indexed>> {
    let shift = 64 - u32::from(bits.get());
    // `bits <= 16`, so a partition number fits a `u16`.
    let partition_of: Vec<u16> = tuples
        .iter()
        .map(|tuple| (H::hash(tuple[0]) >> shift) as u16)
        .collect();
    let mut sizes = vec![0usize; 1 << bits.get()];
    for &p in &partition_of {
        sizes[usize::from(p)] += 1;
    }
    let mut partitions: Vec<Vec<Indexed>> = sizes.into_iter().map(Vec::with_capacity).collect();
    for (index, (tuple, p)) in tuples.into_iter().zip(partition_of).enumerate() {
        partitions[usize::from(p)].push((index, tuple));
    }
    partitions
}

/// Builds one partition into a scratch root of the real root's kind by
/// Algorithm 2 (`bulk.rs`): the partition's tuples grouped at the root,
/// recording for each key the input index of the tuple that introduced it
/// and the key's hash, in arrival order; then each key's child built from
/// its list, as the bulk build builds it.
pub(super) fn build_scratch_root<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    partition: impl IntoIterator<Item = Indexed>, arity: usize, config: HashTrieConfig,
) -> (HashTrieNode<P, E>, Vec<(usize, u64)>) {
    // Lines 3–7 at the root. The scratch root starts at 4 buckets and
    // grows: its entries move into the real root, so its size is never
    // seen.
    let mut lists: HashTable<TupleList> = HashTable::new();
    let mut first_seen = Vec::new();
    for (index, tuple) in partition {
        let hash = H::hash(tuple[0]);
        let keys_before = lists.len();
        lists
            .entry_or_insert_with(hash, config.load_factor, Vec::new)
            .push(tuple);
        if lists.len() > keys_before {
            first_seen.push((index, hash));
        }
    }
    // Lines 8–15.
    let scratch = HashTrie::<H, P, E>::build_nested(0, arity, lists, config);
    (scratch, first_seen)
}

/// Moves every entry out of `scratch` into `out`, tagged with the input
/// index at which its key first appeared. Bucket positions are read before
/// the table is consumed, and each bucket is taken exactly once.
pub(super) fn take_in_arrival_order<V>(
    scratch: HashTable<V>, first_seen: &[(usize, u64)], out: &mut Vec<Arrival<V>>,
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
    // First indices are distinct, so the unstable sort is deterministic.
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
                identity::{assert_same_trie, configs, inputs, rows},
                pruning::{NoPruning, SingletonPruning},
            },
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

    /// `radix:K` builds the trie `bulk` builds, for every arity, load
    /// factor, root capacity, bit count and input.
    fn check_identity<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
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
                            "{}/{}/{} arity {arity}, load {}%, root {}, radix:{bits}, {input}",
                            H::NAME,
                            P::NAME,
                            E::NAME,
                            config.load_factor.numerator(),
                            config.root_capacity.axis_value(),
                        );
                        assert_same_trie(&build(HashTrieBuildMode::Bulk), &build(radix), &label);
                    }
                }
            }
        }
    }

    #[test]
    fn radix_builds_the_bulk_trie_under_siphash() {
        check_identity::<SipHashStrategy, NoPruning, EagerExpansion>();
        check_identity::<SipHashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<SipHashStrategy, NoPruning, LazyExpansion>();
        check_identity::<SipHashStrategy, SingletonPruning, LazyExpansion>();
    }

    #[test]
    fn radix_builds_the_bulk_trie_under_fxhash() {
        check_identity::<FxHashStrategy, NoPruning, EagerExpansion>();
        check_identity::<FxHashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<FxHashStrategy, NoPruning, LazyExpansion>();
        check_identity::<FxHashStrategy, SingletonPruning, LazyExpansion>();
    }

    /// Every hash is below 10, so every tuple lands in partition 0, and
    /// distinct keys share full hashes, root entries and leaf chains.
    #[test]
    fn radix_builds_the_bulk_trie_under_colliding_hashes() {
        check_identity::<Mod10HashStrategy, NoPruning, EagerExpansion>();
        check_identity::<Mod10HashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<Mod10HashStrategy, NoPruning, LazyExpansion>();
        check_identity::<Mod10HashStrategy, SingletonPruning, LazyExpansion>();
    }

    #[test]
    #[should_panic(expected = "tuple arity")]
    fn radix_build_rejects_a_wrong_arity() {
        let _: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            HashTrieConfig::default(),
            HashTrieBuildMode::Radix(RadixBits::new(4).unwrap()),
            vec![vec![1, 2], vec![3]],
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
        assert_same_trie(&plain, &built, "from_tuples vs radix:2");
    }
}
