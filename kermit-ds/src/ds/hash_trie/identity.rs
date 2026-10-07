//! Array-level identity checks for the HashTrie builds (#91, #94, #88).
//! Every build mode must build the trie the bulk build does, the
//! root-capacity Config must leave every subtrie as it was, and the
//! `presized:N` build must be equivalent (Amendment 2), so the
//! radix, parallel and root-capacity tests share these assertions and
//! inputs.

use {
    super::{
        config::{ChildCapacity, HashTrieConfig, LoadFactor, RootCapacity},
        expansion::{ExpansionPolicy, PendingChild},
        hash_table::{home_bucket, HashTable},
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::{PruningPolicy, SingletonPayload},
    },
    crate::{cardinality::Cardinality, heap_size::HeapSize, test_support::Lcg},
    kermit_iters::HashStrategy,
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

pub(super) fn assert_same_node<P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrieNode<P, E>, b: &HashTrieNode<P, E>, path: &str,
) {
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
        | (HashTrieNode::Unexpanded(x), HashTrieNode::Unexpanded(y)) => {
            // Building expands nothing, so both children are still the
            // pending lists `insert_at` appended to.
            assert!(
                x.built().is_none() && y.built().is_none(),
                "{path}: expanded during the build"
            );
            assert_same_chain(&x.pending(), &y.pending(), &format!("{path}: pending"));
        },
        | _ => panic!("{path}: node variants differ"),
    }
}

/// The array-level identity the standard requires of a BuildMode:
/// every table's buckets and capacity, every chain and tuple capacity,
/// every singleton and pending list, the heap size and the tuple count.
pub(super) fn assert_same_trie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
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
pub(super) fn rows(arity: usize, rows: &[[usize; 3]]) -> Vec<Vec<usize>> {
    rows.iter().map(|row| row[..arity].to_vec()).collect()
}

/// Enough distinct first values to double the root several times (three
/// times under Miri), with repeats so subtries and chains hold several
/// tuples. The builds are safe Rust, so Miri runs a small matrix: the full
/// one took almost five minutes there.
const RANDOM_TUPLES: usize = if cfg!(miri) {
    48
} else {
    3_000
};

/// Load factors under test, in percent. Miri runs the default only.
pub(super) const LOAD_PERCENTS: &[u8] = if cfg!(miri) {
    &[70]
} else {
    &[70, 50]
};

/// The configs a build-mode identity test runs under: each load factor in
/// [`LOAD_PERCENTS`] with each root capacity and each child capacity.
/// Every build mode must build the same trie under every config (#88,
/// #107).
pub(super) fn configs() -> Vec<HashTrieConfig> {
    let mut configs = Vec::new();
    for &percent in LOAD_PERCENTS {
        for root_capacity in [RootCapacity::Grow, RootCapacity::Tuples] {
            for child_capacity in [ChildCapacity::Grow, ChildCapacity::Tuples] {
                configs.push(HashTrieConfig {
                    load_factor: LoadFactor::percent(percent).unwrap(),
                    root_capacity,
                    child_capacity,
                });
            }
        }
    }
    configs
}

/// The [`configs`] the `incremental` build accepts: children that grow.
pub(super) fn incremental_configs() -> Vec<HashTrieConfig> {
    configs()
        .into_iter()
        .filter(|config| config.child_capacity == ChildCapacity::Grow)
        .collect()
}

pub(super) fn inputs(arity: usize) -> Vec<(&'static str, Vec<Vec<usize>>)> {
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

/// The equivalence Amendment 2 asks of a build that may place root keys in
/// other buckets. The root has the same capacity, allocation and length,
/// occupies the same buckets, and has the same total probe displacement.
/// Every key's child or chain is array-identical.
pub(super) fn assert_equivalent_root<P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrieNode<P, E>, b: &HashTrieNode<P, E>, label: &str,
) {
    match (a, b) {
        | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
            assert_equivalent_table(x, y, label, &|x, y, path| assert_same_node(x, y, path))
        },
        | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
            assert_equivalent_table(x, y, label, &|x, y, path| assert_same_chain(x, y, path))
        },
        | _ => panic!("{label}: root variants differ"),
    }
}

fn assert_equivalent_table<V>(
    a: &HashTable<V>, b: &HashTable<V>, path: &str, same_value: &dyn Fn(&V, &V, &str),
) {
    assert_eq!(a.buckets_len(), b.buckets_len(), "{path}: capacity");
    assert_eq!(
        a.shell_heap_bytes(),
        b.shell_heap_bytes(),
        "{path}: bucket allocation"
    );
    assert_eq!(a.len(), b.len(), "{path}: len");
    let cap = a.buckets_len();
    let log2 = cap.trailing_zeros();
    let occupied = |t: &HashTable<V>| {
        (0..cap)
            .filter(|&i| t.hash_at(i).is_some())
            .collect::<Vec<_>>()
    };
    assert_eq!(occupied(a), occupied(b), "{path}: occupied buckets");
    let displacement = |t: &HashTable<V>| {
        (0..cap)
            .filter_map(|i| t.hash_at(i).map(|h| (i + cap - home_bucket(h, log2)) % cap))
            .sum::<usize>()
    };
    assert_eq!(
        displacement(a),
        displacement(b),
        "{path}: total displacement"
    );
    for idx in 0..cap {
        if let (Some(hash), Some(x)) = (a.hash_at(idx), a.value_at(idx)) {
            let j = b
                .index_of(hash)
                .unwrap_or_else(|| panic!("{path}: hash {hash:#x} missing"));
            same_value(
                x,
                b.value_at(j).expect("index_of found it"),
                &format!("{path}/{hash:#x}"),
            );
        }
    }
}

/// [`assert_equivalent_root`] for whole tries, plus heap size and tuple
/// count.
pub(super) fn assert_equivalent_trie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_equivalent_root(a.root(), b.root(), label);
    assert_eq!(
        a.heap_size_bytes(),
        b.heap_size_bytes(),
        "{label}: heap size"
    );
    assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
}
