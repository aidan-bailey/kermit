//! The `radix:K` build of a [`HashTrie`](super::HashTrie): radix-partition
//! the tuples on their first attribute's hash, build each partition into a
//! scratch root, then merge the scratch roots into the real one (SIGMOD 2020
//! §3.3.2; issue #91).
//!
//! The result is the trie the serial build makes, bucket for bucket and
//! capacity for capacity. A table's final layout depends only on the order
//! in which its *new* keys arrive — `HashTable::entry_or_insert_with`
//! returns an existing entry before its resize check — so the merge inserts
//! the distinct root keys in the order they first appear in the input, as
//! the serial build does. Below the root nothing needs care: the partition
//! is stable, so each root key's subtrie is built by the same `insert_at`
//! calls, on the same tuples in the same order, as in the serial build.

use {
    super::{
        build_mode::RadixBits, config::LoadFactor, hash_table::HashTable, implementation::HashTrie,
        node::HashTrieNode, pruning::PruningPolicy,
    },
    kermit_iters::HashStrategy,
};

/// A tuple with its position in the input, which the merge orders by.
type Indexed = (usize, Vec<usize>);

/// A root entry ready to merge: the input position at which its key first
/// appeared, its hash, and its finished value.
type Arrival<V> = (usize, u64, V);

/// Fills the empty `root` with `tuples` by the `radix:bits` build. Every
/// tuple must have `arity` attributes; the caller checks.
pub(super) fn fill_root<H: HashStrategy, P: PruningPolicy>(
    root: &mut HashTrieNode<P>, arity: usize, tuples: Vec<Vec<usize>>, bits: RadixBits,
    load_factor: LoadFactor,
) {
    // One of the two stays empty: the root is `Inner` for arity ≥ 2 (values
    // are subtries) and `Leaf` for arity 1 (values are chains).
    let mut subtries = Vec::new();
    let mut chains = Vec::new();
    for partition in partition::<H>(tuples, bits) {
        if partition.is_empty() {
            continue;
        }
        let (scratch, first_seen) = build_scratch_root::<H, P>(partition, arity, load_factor);
        match scratch {
            | HashTrieNode::Inner(table) => {
                take_in_arrival_order(table, &first_seen, &mut subtries)
            },
            | HashTrieNode::Leaf(table) => take_in_arrival_order(table, &first_seen, &mut chains),
            | HashTrieNode::Singleton(_) => unreachable!("a root is never pruned"),
        }
    }
    match root {
        | HashTrieNode::Inner(table) => {
            insert_in_first_appearance_order(table, subtries, load_factor)
        },
        | HashTrieNode::Leaf(table) => insert_in_first_appearance_order(table, chains, load_factor),
        | HashTrieNode::Singleton(_) => unreachable!("a root is never pruned"),
    }
}

/// Splits `tuples` into `2^bits` partitions on the top `bits` of
/// `H::hash(tuple[0])`. A histogram pass sizes each partition exactly; a
/// scatter pass then moves every tuple into its partition with its input
/// index. Stable — each partition keeps input order — which is what keeps
/// every subtrie identical to the serial build's.
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

/// Builds one partition into a scratch root of the real root's kind, by
/// the serial build's own `insert_at`. Returns the scratch root and, for
/// each key it holds, the input index of the tuple that introduced it and
/// the key's hash, in arrival order.
fn build_scratch_root<H: HashStrategy, P: PruningPolicy>(
    partition: Vec<Indexed>, arity: usize, load_factor: LoadFactor,
) -> (HashTrieNode<P>, Vec<(usize, u64)>) {
    let mut scratch = HashTrie::<H, P>::make_root(arity);
    let mut first_seen = Vec::new();
    for (index, tuple) in partition {
        let key = tuple[0];
        let keys_before = scratch.len();
        HashTrie::<H, P>::insert_at(&mut scratch, 0, arity, tuple, load_factor);
        if scratch.len() > keys_before {
            first_seen.push((index, H::hash(key)));
        }
    }
    (scratch, first_seen)
}

/// Moves every entry out of `scratch` into `out`, tagged with the input
/// index at which its key first appeared. Bucket positions are read before
/// the table is consumed, and each bucket is taken exactly once.
fn take_in_arrival_order<V>(
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
/// first appeared in the input — the order the serial build inserts them —
/// so the root's buckets, length and capacity are the serial build's.
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
            cardinality::Cardinality,
            ds::hash_trie::{
                build_mode::HashTrieBuildMode,
                config::HashTrieConfig,
                pruning::{NoPruning, SingletonPayload, SingletonPruning},
            },
            heap_size::HeapSize,
            relation::{BuildModeRelation, ConfigurableRelation, Relation},
            test_support::{Lcg, Mod10HashStrategy},
        },
        kermit_iters::{FxHashStrategy, SipHashStrategy},
    };

    /// Asserts that two tables hold the same buckets: the same capacity
    /// (array length and allocation), the same length, the same hash in
    /// every bucket, and values that `same_value` accepts.
    fn assert_same_table<V>(
        a: &HashTable<V>, b: &HashTable<V>, path: &str, same_value: &dyn Fn(&V, &V, &str),
    ) {
        assert_eq!(a.buckets_len(), b.buckets_len(), "{path}: capacity");
        assert_eq!(
            a.shell_heap_bytes(),
            b.shell_heap_bytes(),
            "{path}: bucket allocation"
        );
        assert_eq!(a.len(), b.len(), "{path}: len");
        for idx in 0..a.buckets_len() {
            assert_eq!(a.hash_at(idx), b.hash_at(idx), "{path}: bucket {idx}");
            if let (Some(x), Some(y)) = (a.value_at(idx), b.value_at(idx)) {
                same_value(x, y, &format!("{path}/{idx}"));
            }
        }
    }

    // `&Vec`, not a slice: the comparison reads `capacity()`.
    #[allow(clippy::ptr_arg)]
    fn assert_same_chain(a: &Vec<Vec<usize>>, b: &Vec<Vec<usize>>, path: &str) {
        assert_eq!(a, b, "{path}: chain");
        assert_eq!(a.capacity(), b.capacity(), "{path}: chain capacity");
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            assert_eq!(x.capacity(), y.capacity(), "{path}: tuple {i} capacity");
        }
    }

    fn assert_same_node<P: PruningPolicy>(a: &HashTrieNode<P>, b: &HashTrieNode<P>, path: &str) {
        match (a, b) {
            | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
                assert_same_table(x, y, path, &|x, y, path| assert_same_node(x, y, path))
            },
            | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
                assert_same_table(x, y, path, &|x, y, path| assert_same_chain(x, y, path))
            },
            | (HashTrieNode::Singleton(x), HashTrieNode::Singleton(y)) => {
                assert_eq!(x.tuple(), y.tuple(), "{path}: singleton");
                assert_eq!(
                    x.tuple().capacity(),
                    y.tuple().capacity(),
                    "{path}: singleton capacity"
                );
            },
            | _ => panic!("{path}: node variants differ"),
        }
    }

    /// The array-level identity the standard requires of a BuildMode:
    /// every table's buckets and capacity, every chain and tuple capacity,
    /// every singleton, the heap size and the tuple count.
    fn assert_same_trie<H: HashStrategy, P: PruningPolicy>(
        a: &HashTrie<H, P>, b: &HashTrie<H, P>, label: &str,
    ) {
        assert_same_node(a.root(), b.root(), label);
        assert_eq!(
            a.heap_size_bytes(),
            b.heap_size_bytes(),
            "{label}: heap size"
        );
        assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
    }

    /// Rows of up to three columns, cut to `arity`.
    fn rows(arity: usize, rows: &[[usize; 3]]) -> Vec<Vec<usize>> {
        rows.iter().map(|row| row[..arity].to_vec()).collect()
    }

    /// Enough distinct first values to double the root several times, with
    /// repeats so subtries and chains hold several tuples.
    const RANDOM_TUPLES: usize = if cfg!(miri) {
        200
    } else {
        3_000
    };

    /// Bit counts under test. Sixteen bits make 65,536 partitions, too slow
    /// to set up a thousand times under Miri.
    const BITS: &[u8] = if cfg!(miri) {
        &[1, 4]
    } else {
        &[1, 4, 16]
    };

    fn inputs(arity: usize) -> Vec<(&'static str, Vec<Vec<usize>>)> {
        let mut lcg = Lcg(0x91);
        let random = (0..RANDOM_TUPLES)
            .map(|_| {
                let row = [
                    lcg.next_usize() % (RANDOM_TUPLES / 4),
                    lcg.next_usize() % 50,
                    lcg.next_usize() % 7,
                ];
                row[..arity].to_vec()
            })
            .collect();
        vec![
            ("empty", vec![]),
            ("one tuple", rows(arity, &[[1, 2, 3]])),
            (
                "duplicates",
                rows(arity, &[[1, 2, 3], [1, 2, 3], [1, 2, 3]]),
            ),
            (
                "interleaved",
                rows(arity, &[
                    [1, 2, 3],
                    [2, 3, 4],
                    [1, 5, 6],
                    [3, 1, 1],
                    [2, 3, 9],
                    [1, 2, 7],
                ]),
            ),
            ("random", random),
        ]
    }

    /// `radix:K` builds the trie `serial` builds, for every arity, load
    /// factor, bit count and input.
    fn check_identity<H: HashStrategy, P: PruningPolicy>() {
        for arity in 1..=3 {
            for percent in [70, 50] {
                let config = HashTrieConfig {
                    load_factor: LoadFactor::percent(percent).unwrap(),
                };
                for &bits in BITS {
                    let radix = HashTrieBuildMode::Radix(RadixBits::new(bits).unwrap());
                    for (input, tuples) in inputs(arity) {
                        let build = |mode| {
                            HashTrie::<H, P>::from_tuples_with_config_and_build_mode(
                                arity.into(),
                                config,
                                mode,
                                tuples.clone(),
                            )
                        };
                        let label = format!(
                            "{}/{} arity {arity}, load {percent}%, radix:{bits}, {input}",
                            H::NAME,
                            P::NAME
                        );
                        assert_same_trie(&build(HashTrieBuildMode::Serial), &build(radix), &label);
                    }
                }
            }
        }
    }

    #[test]
    fn radix_builds_the_serial_trie_under_siphash() {
        check_identity::<SipHashStrategy, NoPruning>();
        check_identity::<SipHashStrategy, SingletonPruning>();
    }

    #[test]
    fn radix_builds_the_serial_trie_under_fxhash() {
        check_identity::<FxHashStrategy, NoPruning>();
        check_identity::<FxHashStrategy, SingletonPruning>();
    }

    /// Every hash is below 10, so every tuple lands in partition 0, and
    /// distinct keys share full hashes, root entries and leaf chains.
    #[test]
    fn radix_builds_the_serial_trie_under_colliding_hashes() {
        check_identity::<Mod10HashStrategy, NoPruning>();
        check_identity::<Mod10HashStrategy, SingletonPruning>();
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
