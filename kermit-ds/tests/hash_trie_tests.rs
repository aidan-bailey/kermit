//! Iterator-contract, construction, and collision suites for `HashTrie` — the
//! hash-family counterpart of `trie_tests.rs`.
//!
//! `HashTrie` implements `HashTrieIterable` rather than `TrieIterable`, so it
//! cannot be passed to `relation_trie_test_suite!`; `hash_trie_test_suite!`
//! (in `common/macros.rs`) mirrors that suite for the hash-family contract.
//! One invocation per Layout alias, matching `kermit/tests/join_tests.rs`,
//! plus one under a deliberately colliding strategy so the contract is also
//! exercised with clustered buckets and shared leaf chains.

use {
    kermit_ds::{
        define_config_provider, Configured, HashTrie, HashTrieConfig, LoadFactor, SingletonPruning,
    },
    kermit_iters::{FxHashStrategy, HashStrategy, LayoutOption, SipHashStrategy},
};
mod common;

type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;

/// Test-only strategy that forces real hash collisions: `hash(k) = k mod 10`,
/// so keys congruent modulo 10 (e.g. `2` and `12`) share a bucket at every
/// level, and tuples whose attributes all collide share a leaf chain. Every
/// hash is below 10, so all of them also map to bucket 0 of a fresh table
/// and resolve through linear probing — the pathological layout the
/// `HashTable` docs describe.
///
/// A twin lives in `kermit-algos/src/hash_triejoin.rs` (tests), where it
/// drives `verify_and_construct` against an actual false positive. The two
/// are deliberately not shared: neither crate should ship a colliding
/// strategy in its public API.
#[derive(Copy, Clone, Default, Debug)]
pub struct Mod10HashStrategy;

impl LayoutOption for Mod10HashStrategy {
    const NAME: &'static str = "mod10";
}

impl HashStrategy for Mod10HashStrategy {
    fn hash(key: usize) -> u64 { (key % 10) as u64 }
}

type HashTrieMod10 = HashTrie<Mod10HashStrategy>;

// ── Layout variant: singleton pruning on ────────────────────────────────
//
// Each hasher alias also runs with the pruning Layout on, so the iterator
// contract holds on emulated (pruned) levels as well as materialised ones.
// `Mod10` is the important case: full-collision tuples must unprune into a
// shared leaf chain.
type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieFxPruned = HashTrie<FxHashStrategy, SingletonPruning>;
type HashTrieMod10Pruned = HashTrie<Mod10HashStrategy, SingletonPruning>;

// ── Config variant: a dense load factor ─────────────────────────────────
define_config_provider!(NinetyPercent, HashTrieConfig, HashTrieConfig {
    load_factor: LoadFactor::percent(90).unwrap(),
});

type HashTrieSipDense = Configured<HashTrieSip, NinetyPercent>;

hash_trie_test_suite!(HashTrieSip, SipHashStrategy);

hash_trie_test_suite!(HashTrieFx, FxHashStrategy);

// The contract fixtures use values that are distinct modulo 10, so the same
// suite must hold when unrelated keys *would* collide — every bucket here is
// reached by linear probing from bucket 0.
hash_trie_test_suite!(HashTrieMod10, Mod10HashStrategy);

hash_trie_test_suite!(HashTrieSipPruned, SipHashStrategy);

hash_trie_test_suite!(HashTrieFxPruned, FxHashStrategy);

hash_trie_test_suite!(HashTrieMod10Pruned, Mod10HashStrategy);

hash_trie_test_suite!(HashTrieSipDense, SipHashStrategy);

/// What the structure does when two distinct values really do hash to the
/// same `u64`. These pin the "leaf chains preserve hash collisions"
/// invariant from `docs/data-structures/hash-trie.md`: the trie itself never
/// compares values, so a colliding key is indistinguishable from a match
/// until `HashTriejoin::verify_and_construct` inspects the leaf tuples.
mod hash_trie_collisions {
    use {
        super::*,
        kermit_ds::Relation,
        kermit_iters::{HashTrieIterable, HashTrieIterator},
    };

    fn h(key: usize) -> u64 { Mod10HashStrategy::hash(key) }

    fn sorted(chain: &[Vec<usize>]) -> Vec<Vec<usize>> {
        let mut tuples = chain.to_vec();
        tuples.sort();
        tuples
    }

    #[test]
    fn strategy_collides_as_intended() {
        assert_eq!(h(2), h(12));
        assert_eq!(h(1), h(11));
        assert_ne!(h(1), h(2));
    }

    #[test]
    fn colliding_first_attributes_share_one_root_bucket() {
        // 1 and 11 collide, so both tuples descend through one root bucket
        // into one child, which then separates them on the second attribute.
        let trie = HashTrieMod10::from_tuples(2.into(), vec![vec![1, 2], vec![11, 3]]);
        let mut iter = trie.hash_trie_iter();
        assert!(iter.open());
        assert_eq!(iter.size(), 1);
        assert_eq!(iter.key(), Some(h(1)));
        assert!(iter.open());
        assert_eq!(iter.size(), 2);
        assert!(iter.lookup(h(2)));
        assert_eq!(iter.leaf_tuples().map(sorted), Some(vec![vec![1, 2]]));
        assert!(iter.lookup(h(3)));
        assert_eq!(iter.leaf_tuples().map(sorted), Some(vec![vec![11, 3]]));
    }

    #[test]
    fn full_signature_collision_shares_a_leaf_chain() {
        // Every attribute collides: (1, 2) and (11, 12) have the same hash
        // path, so they cohabit one leaf chain — equality is deferred.
        let trie = HashTrieMod10::from_tuples(2.into(), vec![vec![1, 2], vec![11, 12]]);
        let mut iter = trie.hash_trie_iter();
        assert!(iter.open());
        assert_eq!(iter.size(), 1);
        assert!(iter.open());
        assert_eq!(iter.size(), 1);
        assert_eq!(
            iter.leaf_tuples().map(sorted),
            Some(vec![vec![1, 2], vec![11, 12]])
        );
    }

    #[test]
    fn lookup_cannot_distinguish_colliding_keys() {
        // The false positive `verify_and_construct` exists for: probing for a
        // value that is *not* stored succeeds because its hash is.
        let trie = HashTrieMod10::from_tuples(2.into(), vec![vec![1, 2]]);
        let mut iter = trie.hash_trie_iter();
        assert!(iter.open());
        assert!(iter.lookup(h(11)));
        assert!(iter.open());
        assert_eq!(iter.leaf_tuples().map(sorted), Some(vec![vec![1, 2]]));
    }

    #[test]
    fn size_counts_distinct_hashes_not_distinct_values() {
        let trie = HashTrieMod10::from_tuples(1.into(), vec![vec![1], vec![11], vec![21], vec![2]]);
        let mut iter = trie.hash_trie_iter();
        assert!(iter.open());
        assert_eq!(iter.size(), 2);
        assert!(iter.lookup(h(1)));
        assert_eq!(
            iter.leaf_tuples().map(sorted),
            Some(vec![vec![1], vec![11], vec![21]])
        );
        assert!(iter.lookup(h(2)));
        assert_eq!(iter.leaf_tuples().map(sorted), Some(vec![vec![2]]));
    }

    #[test]
    fn collect_tuples_recovers_colliding_tuples() {
        let tuples = vec![vec![1, 2], vec![11, 12], vec![21, 2], vec![1, 12]];
        let trie = HashTrieMod10::from_tuples(2.into(), tuples.clone());
        let mut collected = trie.collect_tuples();
        collected.sort();
        let mut expected = tuples;
        expected.sort();
        assert_eq!(collected, expected);
    }

    /// Under pruning, two tuples that collide on every attribute start as
    /// one `Singleton` and must unprune into a shared leaf chain of two —
    /// the same shape the unpruned trie builds, so `verify_and_construct`
    /// still sees both candidates.
    #[test]
    fn full_collision_unprunes_into_shared_leaf_chain() {
        let trie = HashTrieMod10Pruned::from_tuples(2.into(), vec![vec![1, 2], vec![11, 12]]);
        let mut it = trie.hash_trie_iter();
        assert!(it.open());
        assert_eq!(it.size(), 1, "both tuples share hash(1) == hash(11)");
        assert!(it.open());
        assert_eq!(it.size(), 1, "both tuples share hash(2) == hash(12)");
        let mut chain = it.leaf_tuples().expect("leaf chain").to_vec();
        chain.sort();
        assert_eq!(chain, vec![vec![1, 2], vec![11, 12]]);
    }
}
