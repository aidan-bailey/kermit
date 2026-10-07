//! The `bulk` build of a [`HashTrie`](super::HashTrie): Algorithm 2 of
//! Freitag et al., *Adopting Worst-Case Optimal Joins in Relational
//! Database Systems* (VLDB 2020, §3.2.2), issue #107.
//!
//! ```text
//!  1 function build(i, L)
//!  2   if i ≤ n then
//!  3     M ← allocateHashtable(2^⌈log2(1.25·|L|)⌉)
//!  4     while L is not empty do
//!  5       t ← pop next tuple from L
//!  6       B ← lookupBucket(M, h_i(π_vi(t)))
//!  7       push t onto the linked list stored in B
//!  8     i_next ← index of the next attribute in E_j
//!  9     foreach populated bucket B in M do
//! 10       L_next ← extract linked list stored in B
//! 11       M_next ← build(i_next, L_next)
//! 12       store M_next in B
//! 13     return M
//! 14   else
//! 15     return L
//! ```
//!
//! [`HashTrie::group`] is lines 3–7 and [`HashTrie::build_nested`] lines
//! 8–15. At the last attribute, lines 9–12 store each list itself (line 15),
//! so the table of lists is the leaf and its lists are the chains.
//! [`HashTrie::child`] decides what a bucket's list becomes: a pruned
//! `Singleton` (§3.3.1), an `Unexpanded` child under lazy expansion
//! (§3.3.1), or the table line 11 builds.
//!
//! Two differences from the paper, both kermit's:
//!
//! - A list is a `Vec` per bucket. Umbra threads its lists through an 8-byte
//!   chain pointer reserved in each materialised tuple (§3.3.2), which needs
//!   contiguous tuple storage (#101).
//! - Line 3's size: the root's comes from `root-capacity` (#88); every child
//!   starts at 4 buckets and grows, as under the per-tuple build.
//!
//! Algorithm 2 builds the trie the per-tuple build (`insert_at`, the
//! `incremental` mode) builds, array for array. A table's layout depends
//! only on the order its new keys arrive. Here every list keeps input order,
//! because a `Vec` push appends, so each table receives its keys in the
//! order `insert_at` sends them.

use {
    super::{
        config::HashTrieConfig,
        expansion::{ExpansionPolicy, PendingChild},
        hash_table::{HashTable, INITIAL_LOG2_CAPACITY},
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::{PruningPolicy, SingletonPayload},
    },
    kermit_iters::HashStrategy,
};

/// A list of tuples in input order: Algorithm 2's `L`.
pub(super) type TupleList = Vec<Vec<usize>>;

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> HashTrie<H, P, E> {
    /// Algorithm 2: the table at `depth` over `list`, allocated at
    /// `2^log2_capacity` buckets.
    pub(super) fn build(
        depth: usize, arity: usize, list: TupleList, log2_capacity: u32, config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        let lists = Self::group(depth, list, log2_capacity, config);
        Self::build_nested(depth, arity, lists, config)
    }

    /// Lines 3–7: a table of `2^log2_capacity` buckets holding `list`'s
    /// tuples, each pushed onto the list in the bucket of its attribute's
    /// hash at `depth`. The table grows under the load factor if its keys
    /// outgrow it, as `insert_at`'s tables do.
    pub(super) fn group(
        depth: usize, list: TupleList, log2_capacity: u32, config: HashTrieConfig,
    ) -> HashTable<TupleList> {
        let mut lists = HashTable::with_log2_capacity(log2_capacity);
        for tuple in list {
            let hash = H::hash(tuple[depth]);
            lists
                .entry_or_insert_with(hash, config.load_factor, Vec::new)
                .push(tuple);
        }
        lists
    }

    /// Lines 8–15: each bucket's list replaced by the node built from it,
    /// in bucket order (line 9). At the last attribute each list is stored
    /// itself (line 15), so the table of lists is the leaf.
    pub(super) fn build_nested(
        depth: usize, arity: usize, lists: HashTable<TupleList>, config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        // `>=`, as `make_root`'s `arity <= 1`, so the unsupported nullary
        // root is a leaf under both builds.
        if depth + 1 >= arity {
            return HashTrieNode::Leaf(lists);
        }
        HashTrieNode::Inner(lists.map(|list| Self::child(depth + 1, arity, list, config)))
    }

    /// The node a bucket's `list` becomes, at `depth`: a `Singleton` when
    /// pruning is on and one tuple lives below (§3.3.1), an `Unexpanded`
    /// child under lazy expansion (§3.3.1; `HashTrie::resolve` builds its
    /// table by [`build`](Self::build) on the first probe), and otherwise the
    /// table line 11 builds.
    pub(super) fn child(
        depth: usize, arity: usize, mut list: TupleList, config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        if P::ENABLED && list.len() == 1 {
            let tuple = list.pop().expect("the list holds one tuple");
            return HashTrieNode::Singleton(P::Payload::from_tuple(tuple));
        }
        if E::LAZY {
            // `insert_at` starts a pending list as `vec![tuple]`, capacity
            // 1, or after an unprune as `Vec::with_capacity(2)`. A list grown
            // from empty reaches capacity 4 on its first push, and from there
            // both grow alike. Shrinking the two short cases keeps the trie
            // byte-identical to `incremental`'s, so `space` cannot move, and
            // keeps a one-tuple pending list at one slot. Both the first
            // capacity of 4 and `shrink_to`'s exact result are std
            // implementation details; the identity tests pin them.
            let first_capacity = if P::ENABLED {
                2
            } else {
                1
            };
            if list.len() == first_capacity {
                list.shrink_to(first_capacity);
            }
            return HashTrieNode::Unexpanded(E::Pending::from_tuples(list));
        }
        Self::build_child_table(depth, arity, list, config)
    }

    /// The table a bucket's `list` becomes at `depth` when it is built
    /// (line 11): eagerly by [`child`](Self::child), or on the first probe by
    /// `HashTrie::resolve` under lazy expansion. One definition, so an
    /// expanded child is the eager child, size included.
    pub(super) fn build_child_table(
        depth: usize, arity: usize, list: TupleList, config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        Self::build(depth, arity, list, INITIAL_LOG2_CAPACITY, config)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            ds::hash_trie::{
                expansion::{EagerExpansion, LazyExpansion},
                identity::{assert_same_trie, configs, inputs},
                pruning::{NoPruning, SingletonPruning},
            },
            test_support::{Lcg, Mod10HashStrategy},
        },
        kermit_iters::{FxHashStrategy, SipHashStrategy},
    };

    fn label<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
        arity: usize, config: HashTrieConfig, input: &str,
    ) -> String {
        format!(
            "{}/{}/{} arity {arity}, load {}%, root {}, {input}",
            H::NAME,
            P::NAME,
            E::NAME,
            config.load_factor.numerator(),
            config.root_capacity.axis_value(),
        )
    }

    /// Algorithm 2 builds the trie the per-tuple build makes, array for
    /// array: capacities, chains, singletons and pending lists included.
    /// The `interleaved` input gives an arity-2 trie a one-tuple and a
    /// two-tuple child, the two lengths whose pending lists `child` shrinks.
    fn check_identity<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for config in configs() {
                for (input, tuples) in inputs(arity) {
                    let incremental = HashTrie::<H, P, E>::from_tuples_incrementally(
                        arity.into(),
                        config,
                        tuples.clone(),
                    );
                    let bulk =
                        HashTrie::<H, P, E>::from_tuples_in_bulk(arity.into(), config, tuples);
                    assert_same_trie(&incremental, &bulk, &label::<H, P, E>(arity, config, input));
                }
            }
        }
    }

    #[test]
    fn bulk_builds_the_incremental_trie_under_siphash() {
        check_identity::<SipHashStrategy, NoPruning, EagerExpansion>();
        check_identity::<SipHashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<SipHashStrategy, NoPruning, LazyExpansion>();
        check_identity::<SipHashStrategy, SingletonPruning, LazyExpansion>();
    }

    #[test]
    fn bulk_builds_the_incremental_trie_under_fxhash() {
        check_identity::<FxHashStrategy, NoPruning, EagerExpansion>();
        check_identity::<FxHashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<FxHashStrategy, NoPruning, LazyExpansion>();
        check_identity::<FxHashStrategy, SingletonPruning, LazyExpansion>();
    }

    /// Distinct keys share full hashes, so one bucket's list holds several
    /// keys' tuples and leaf chains hold colliding tuples.
    #[test]
    fn bulk_builds_the_incremental_trie_under_colliding_hashes() {
        check_identity::<Mod10HashStrategy, NoPruning, EagerExpansion>();
        check_identity::<Mod10HashStrategy, SingletonPruning, EagerExpansion>();
        check_identity::<Mod10HashStrategy, NoPruning, LazyExpansion>();
        check_identity::<Mod10HashStrategy, SingletonPruning, LazyExpansion>();
    }

    /// Chain order, explicitly: under the colliding strategy, first keys 1
    /// and 11 share a root bucket and last keys 3 and 13 share a leaf chain,
    /// so distinct tuples meet in one chain, which must list them in input
    /// order as the per-tuple build does. Under SipHash or FxHash a chain
    /// holds only equal tuples, so only a colliding fixture can see its order.
    #[test]
    fn bulk_keeps_chain_order_under_colliding_hashes() {
        let tuples = vec![vec![1, 3], vec![11, 13], vec![1, 13], vec![11, 3], vec![
            1, 3,
        ]];
        fn check<P: PruningPolicy, E: ExpansionPolicy>(tuples: &[Vec<usize>]) {
            for config in configs() {
                let incremental = HashTrie::<Mod10HashStrategy, P, E>::from_tuples_incrementally(
                    2.into(),
                    config,
                    tuples.to_vec(),
                );
                let bulk = HashTrie::<Mod10HashStrategy, P, E>::from_tuples_in_bulk(
                    2.into(),
                    config,
                    tuples.to_vec(),
                );
                assert_same_trie(
                    &incremental,
                    &bulk,
                    &label::<Mod10HashStrategy, P, E>(2, config, "colliding chain"),
                );
            }
        }
        check::<NoPruning, EagerExpansion>(&tuples);
        check::<SingletonPruning, EagerExpansion>(&tuples);
        check::<NoPruning, LazyExpansion>(&tuples);
        check::<SingletonPruning, LazyExpansion>(&tuples);
        // The eager trie's one chain holds all five tuples, in input order.
        let bulk: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples_in_bulk(2.into(), HashTrieConfig::default(), tuples.clone());
        assert_eq!(bulk.collect_tuples(), tuples);
    }

    /// Arity 4 and a first key holding half the tuples, which the shared
    /// inputs leave out.
    #[test]
    #[cfg_attr(miri, ignore = "thousands of inserts")]
    fn bulk_builds_the_incremental_trie_on_wide_and_skewed_inputs() {
        fn check<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
            let mut lcg = Lcg(0x107);
            for arity in 1..=4 {
                let random: Vec<Vec<usize>> = (0..4_000)
                    .map(|_| (0..arity).map(|_| lcg.next_usize() % 300).collect())
                    .collect();
                let skewed: Vec<Vec<usize>> = (0..4_000)
                    .map(|i| {
                        let first = if i % 2 == 0 {
                            7
                        } else {
                            lcg.next_usize() % 300
                        };
                        std::iter::once(first)
                            .chain((1..arity).map(|_| lcg.next_usize() % 20))
                            .collect()
                    })
                    .collect();
                for (input, tuples) in [("random", random), ("half one key", skewed)] {
                    for config in configs() {
                        let incremental = HashTrie::<H, P, E>::from_tuples_incrementally(
                            arity.into(),
                            config,
                            tuples.clone(),
                        );
                        let bulk = HashTrie::<H, P, E>::from_tuples_in_bulk(
                            arity.into(),
                            config,
                            tuples.clone(),
                        );
                        assert_same_trie(
                            &incremental,
                            &bulk,
                            &label::<H, P, E>(arity, config, input),
                        );
                    }
                }
            }
        }
        check::<SipHashStrategy, NoPruning, EagerExpansion>();
        check::<SipHashStrategy, SingletonPruning, LazyExpansion>();
        check::<FxHashStrategy, NoPruning, LazyExpansion>();
        check::<FxHashStrategy, SingletonPruning, EagerExpansion>();
    }

    #[test]
    #[should_panic(expected = "does not match header arity")]
    fn bulk_build_rejects_a_wrong_arity() {
        let _: HashTrie = HashTrie::from_tuples_in_bulk(2.into(), HashTrieConfig::default(), vec![
            vec![1, 2],
            vec![3],
        ]);
    }
}
