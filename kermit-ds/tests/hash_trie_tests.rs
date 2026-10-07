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
        define_build_mode_provider, define_config_provider, BuiltWith, ConfigurableRelation,
        Configured, HashTrie, HashTrieBuildMode, HashTrieConfig, LazyExpansion, LoadFactor,
        NoPruning, PruningPolicy, RadixBits, RootCapacity, SingletonPruning, Threads,
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
    ..HashTrieConfig::default()
});

type HashTrieSipDense = Configured<HashTrieSip, NinetyPercent>;
// A dense table under the colliding strategy: every hash lands in bucket 0
// and resolves by probing, so a 90 % cap stresses the probe loops hardest.
type HashTrieMod10Dense = Configured<HashTrieMod10, NinetyPercent>;

// ── Config variant: a presized root ─────────────────────────────────────
// Under `root-capacity=tuples` the root is sized once from the tuple count
// (#88), so the contract must hold over a root sparser than the grown one.
define_config_provider!(PresizedRoot, HashTrieConfig, HashTrieConfig {
    root_capacity: RootCapacity::Tuples,
    ..HashTrieConfig::default()
});

type HashTrieSipPresized = Configured<HashTrieSip, PresizedRoot>;

// ── Layout variant: lazy child expansion ────────────────────────────────
//
// Each hasher × pruning alias again, with every child below the root built
// on the first `open` that reaches it. The traversal and lookup suites
// therefore expand nodes mid-iteration on every descent.
type HashTrieSipLazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;
type HashTrieFxLazy = HashTrie<FxHashStrategy, NoPruning, LazyExpansion>;
type HashTrieMod10Lazy = HashTrie<Mod10HashStrategy, NoPruning, LazyExpansion>;
type HashTrieSipPrunedLazy = HashTrie<SipHashStrategy, SingletonPruning, LazyExpansion>;
type HashTrieFxPrunedLazy = HashTrie<FxHashStrategy, SingletonPruning, LazyExpansion>;
type HashTrieMod10PrunedLazy = HashTrie<Mod10HashStrategy, SingletonPruning, LazyExpansion>;
// Expansion reads the load factor too: a lazy child is built under the cap
// the trie was configured with.
type HashTrieSipDenseLazy = Configured<HashTrieSipLazy, NinetyPercent>;

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

hash_trie_test_suite!(HashTrieMod10Dense, Mod10HashStrategy);

hash_trie_test_suite!(HashTrieSipLazy, SipHashStrategy);

hash_trie_test_suite!(HashTrieFxLazy, FxHashStrategy);

hash_trie_test_suite!(HashTrieMod10Lazy, Mod10HashStrategy);

hash_trie_test_suite!(HashTrieSipPrunedLazy, SipHashStrategy);

hash_trie_test_suite!(HashTrieFxPrunedLazy, FxHashStrategy);

hash_trie_test_suite!(HashTrieMod10PrunedLazy, Mod10HashStrategy);

hash_trie_test_suite!(HashTrieSipDenseLazy, SipHashStrategy);

hash_trie_test_suite!(HashTrieSipPresized, SipHashStrategy);

// ── BuildMode: the radix build ──────────────────────────────────────────
// Every build mode builds the identical trie (issue #91), so the iterator
// contract must hold unchanged, eager or lazy. Two bits make four
// partitions, so the 3–5 tuple fixtures spread over several partitions
// with several keys in each.
define_build_mode_provider!(
    Radix2,
    HashTrieBuildMode,
    HashTrieBuildMode::Radix(RadixBits::new(2).unwrap())
);

type HashTrieSipRadix2 = BuiltWith<HashTrieSip, Radix2>;
type HashTrieSipLazyRadix2 = BuiltWith<HashTrieSipLazy, Radix2>;

hash_trie_test_suite!(HashTrieSipRadix2, SipHashStrategy);

hash_trie_test_suite!(HashTrieSipLazyRadix2, SipHashStrategy);

// ── BuildMode: the parallel build ───────────────────────────────────────
// `parallel:2` builds eight partitions on two threads and must build the
// identical trie (issue #94), so the iterator contract holds unchanged,
// eager or lazy, pruned or not.
define_build_mode_provider!(
    HashParallel2,
    HashTrieBuildMode,
    HashTrieBuildMode::Parallel(Threads::new(2).expect("2 is not zero"))
);

type HashTrieSipParallel2 = BuiltWith<HashTrieSip, HashParallel2>;
type HashTrieSipLazyParallel2 = BuiltWith<HashTrieSipLazy, HashParallel2>;
type HashTrieFxPrunedParallel2 = BuiltWith<HashTrieFxPruned, HashParallel2>;

hash_trie_test_suite!(HashTrieSipParallel2, SipHashStrategy);

hash_trie_test_suite!(HashTrieSipLazyParallel2, SipHashStrategy);

hash_trie_test_suite!(HashTrieFxPrunedParallel2, FxHashStrategy);

// ── BuildMode × Config: the presized build ──────────────────────────────
// `presized:2` requires `root-capacity=tuples` (`PresizedRoot` above), so
// it is only ever stacked on a presized alias. It fills the root by region
// (the paper's build, #94); the trie is equivalent to serial's (Amendment
// 2), so the iterator contract holds unchanged.
define_build_mode_provider!(
    HashPresized2,
    HashTrieBuildMode,
    HashTrieBuildMode::Presized(Threads::new(2).expect("2 is not zero"))
);

type HashTrieSipLazyPresized = Configured<HashTrieSipLazy, PresizedRoot>;
type HashTrieFxPrunedPresized = Configured<HashTrieFxPruned, PresizedRoot>;
type HashTrieSipPresized2 = BuiltWith<HashTrieSipPresized, HashPresized2>;
type HashTrieSipLazyPresized2 = BuiltWith<HashTrieSipLazyPresized, HashPresized2>;
type HashTrieFxPrunedPresized2 = BuiltWith<HashTrieFxPrunedPresized, HashPresized2>;

hash_trie_test_suite!(HashTrieSipPresized2, SipHashStrategy);

hash_trie_test_suite!(HashTrieSipLazyPresized2, SipHashStrategy);

hash_trie_test_suite!(HashTrieFxPrunedPresized2, FxHashStrategy);

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

/// A lazy trie, probed, must be indistinguishable from the eager trie built
/// from the same tuples: the same keys, sizes, `at_end` and leaf chains, in
/// the same order, after every operation. Expansion re-inserts a child's
/// tuples in insertion order under the same load factor, so the expanded
/// table *is* the eager one, and this is exact equality, not a multiset
/// comparison. This is what makes an eager-vs-lazy timing a one-variable
/// comparison.
mod lazy_expansion {
    use {
        super::*,
        kermit_ds::Relation,
        kermit_iters::{HashTrieIterable, HashTrieIterator},
    };

    /// What a caller can observe after one operation.
    #[derive(Debug, PartialEq)]
    struct Obs {
        returned: Ret,
        key: Option<u64>,
        size: usize,
        at_end: bool,
        leaf: Option<Vec<Vec<usize>>>,
    }

    #[derive(Debug, PartialEq)]
    enum Ret {
        Bool(bool),
        Key(Option<u64>),
    }

    fn lcg(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *state >> 33
    }

    /// One pseudo-random operation, chosen from `state`, then the
    /// observation. The choice depends only on `state` and on observations
    /// equal so far, so two equal iterators take the same path.
    fn step<H: HashStrategy>(it: &mut impl HashTrieIterator, state: &mut u64) -> Obs {
        let returned = match lcg(state) % 4 {
            | 0 => Ret::Bool(it.open()),
            | 1 if !it.at_end() => Ret::Key(it.next()),
            | 2 => Ret::Bool(it.lookup(H::hash((lcg(state) % 10) as usize))),
            | _ => Ret::Bool(it.up()),
        };
        Obs {
            returned,
            key: it.key(),
            size: it.size(),
            at_end: it.at_end(),
            leaf: it.leaf_tuples().map(<[Vec<usize>]>::to_vec),
        }
    }

    /// `n` tuples of `arity` attributes over `0..domain`: small enough to
    /// repeat prefixes and whole tuples (shared children, leaf chains,
    /// duplicates).
    fn random_tuples(seed: u64, n: usize, arity: usize, domain: u64) -> Vec<Vec<usize>> {
        let mut state = seed;
        (0..n)
            .map(|_| {
                (0..arity)
                    .map(|_| (lcg(&mut state) % domain) as usize)
                    .collect()
            })
            .collect()
    }

    fn assert_lazy_walks_like_eager<H: HashStrategy, P: PruningPolicy>(load_factor: u8) {
        let config = HashTrieConfig {
            load_factor: LoadFactor::percent(load_factor).unwrap(),
            ..HashTrieConfig::default()
        };
        for arity in [2, 3] {
            for seed in 1..=20u64 {
                let tuples = random_tuples(seed, 80, arity, 6);
                let eager =
                    HashTrie::<H, P>::from_tuples_with_config(arity.into(), config, tuples.clone());
                let lazy = HashTrie::<H, P, LazyExpansion>::from_tuples_with_config(
                    arity.into(),
                    config,
                    tuples,
                );
                let (mut e, mut l) = (eager.hash_trie_iter(), lazy.hash_trie_iter());
                let (mut se, mut sl) = (seed, seed);
                for i in 0..400 {
                    assert_eq!(
                        step::<H>(&mut e, &mut se),
                        step::<H>(&mut l, &mut sl),
                        "arity {arity}, seed {seed}, step {i}"
                    );
                }
            }
        }
    }

    #[test]
    fn sip_lazy_walks_like_eager() {
        assert_lazy_walks_like_eager::<SipHashStrategy, NoPruning>(70);
    }

    #[test]
    fn fx_lazy_walks_like_eager() { assert_lazy_walks_like_eager::<FxHashStrategy, NoPruning>(70); }

    #[test]
    fn mod10_lazy_walks_like_eager() {
        assert_lazy_walks_like_eager::<Mod10HashStrategy, NoPruning>(70);
    }

    #[test]
    fn sip_pruned_lazy_walks_like_eager() {
        assert_lazy_walks_like_eager::<SipHashStrategy, SingletonPruning>(70);
    }

    #[test]
    fn fx_pruned_lazy_walks_like_eager() {
        assert_lazy_walks_like_eager::<FxHashStrategy, SingletonPruning>(70);
    }

    #[test]
    fn mod10_pruned_lazy_walks_like_eager() {
        assert_lazy_walks_like_eager::<Mod10HashStrategy, SingletonPruning>(70);
    }

    /// Expansion reads the configured cap: a lazy child built under 50 % or
    /// 90 % must match the eager table built under the same cap.
    #[test]
    fn lazy_walks_like_eager_under_other_load_factors() {
        assert_lazy_walks_like_eager::<SipHashStrategy, NoPruning>(50);
        assert_lazy_walks_like_eager::<Mod10HashStrategy, SingletonPruning>(90);
    }

    /// Two iterators interleaved on one lazy trie: whichever reaches a child
    /// first expands it, and the other must then find the same table there.
    #[test]
    fn a_second_iterator_sees_the_first_ones_expansion() {
        let tuples = random_tuples(7, 80, 3, 6);
        let eager = HashTrieSip::from_tuples(3.into(), tuples.clone());
        let lazy = HashTrieSipLazy::from_tuples(3.into(), tuples);
        let (mut e1, mut e2) = (eager.hash_trie_iter(), eager.hash_trie_iter());
        let (mut l1, mut l2) = (lazy.hash_trie_iter(), lazy.hash_trie_iter());
        let (mut se1, mut sl1, mut se2, mut sl2) = (11u64, 11u64, 23u64, 23u64);
        for i in 0..400 {
            assert_eq!(
                step::<SipHashStrategy>(&mut e1, &mut se1),
                step::<SipHashStrategy>(&mut l1, &mut sl1),
                "iterator 1, step {i}"
            );
            assert_eq!(
                step::<SipHashStrategy>(&mut e2, &mut se2),
                step::<SipHashStrategy>(&mut l2, &mut sl2),
                "iterator 2, step {i}"
            );
        }
    }
}
