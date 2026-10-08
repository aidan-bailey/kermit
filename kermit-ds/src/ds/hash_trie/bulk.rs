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
//! Two things that are kermit's:
//!
//! - A list is a `Vec<RowId>` per bucket: 4-byte ids of rows in the trie's
//!   buffer (`Tuples`, #111). Umbra threads its lists through an 8-byte chain
//!   pointer reserved in each materialised tuple (§3.3.2); that needs the
//!   partitioned copy of the buffer (#101 layer 3).
//! - Line 3's size is a Config value whose default (`grow`) is not the paper's;
//!   `child-capacity=tuples` at `load-factor=0.8` is the paper's sizing
//!   exactly. The root's is `root-capacity` (#88), and each child's is
//!   `child-capacity`: 4 buckets that grow (`grow`), or sized once from its
//!   list (`tuples`).
//!
//! Under `child-capacity=grow`, Algorithm 2 builds the trie the per-tuple
//! build (`insert_at`, the `incremental` mode, which requires `grow`)
//! builds, array for array. A table's layout depends only on the order its
//! new keys arrive. Here every list keeps input order, because a `Vec` push
//! appends, so each table receives its keys in the order `insert_at` sends
//! them.

use {
    super::{
        config::HashTrieConfig,
        expansion::{ExpansionPolicy, PendingChild},
        hash_table::HashTable,
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::{PruningPolicy, SingletonPayload},
    },
    crate::morsel::row_id,
    kermit_iters::{HashStrategy, RowId, Tuples},
};

/// A list of row ids in input order: Algorithm 2's `L`. The rows live in
/// the trie's buffer.
pub(super) type TupleList = Vec<RowId>;

/// Every row id of `tuples`, in input order: Algorithm 2's first list,
/// which is never materialised. `row_id` (in `morsel.rs`) converts the
/// length; a batch never holds more than `RowId::MAX` rows.
pub(super) fn all_rows(tuples: &Tuples) -> std::ops::Range<RowId> { 0..row_id(tuples.len()) }

impl<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> HashTrie<H, P, E> {
    /// Algorithm 2: the table at `depth` over the rows of `tuples` that
    /// `list` names, allocated at `2^log2_capacity` buckets.
    pub(super) fn build(
        tuples: &Tuples, depth: usize, arity: usize, list: impl IntoIterator<Item = RowId>,
        log2_capacity: u32, config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        let lists = Self::group(tuples, depth, list, log2_capacity, config);
        Self::build_nested(tuples, depth, arity, lists, config)
    }

    /// Lines 3–7: a table of `2^log2_capacity` buckets holding `list`'s row
    /// ids, each pushed onto the list in the bucket of its row's attribute
    /// hash at `depth`. The table grows under the load factor if its keys
    /// outgrow it, as `insert_at`'s tables do.
    pub(super) fn group(
        tuples: &Tuples, depth: usize, list: impl IntoIterator<Item = RowId>, log2_capacity: u32,
        config: HashTrieConfig,
    ) -> HashTable<TupleList> {
        let mut lists = HashTable::with_log2_capacity(log2_capacity);
        for row in list {
            let hash = H::hash(tuples.row(row)[depth]);
            lists
                .entry_or_insert_with(hash, config.load_factor, Vec::new)
                .push(row);
        }
        lists
    }

    /// Lines 8–15: each bucket's list replaced by the node built from it,
    /// in bucket order (line 9). At the last attribute each list is stored
    /// itself (line 15), so the table of lists is the leaf.
    pub(super) fn build_nested(
        tuples: &Tuples, depth: usize, arity: usize, lists: HashTable<TupleList>,
        config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        // `>=`, as `make_root_sized`'s `arity <= 1`, so the unsupported nullary
        // root is a leaf under both builds.
        if depth + 1 >= arity {
            return HashTrieNode::Leaf(lists);
        }
        HashTrieNode::Inner(lists.map(|list| Self::child(tuples, depth + 1, arity, list, config)))
    }

    /// The node a bucket's `list` becomes, at `depth`: a `Singleton` when
    /// pruning is on and one tuple lives below (§3.3.1), an `Unexpanded`
    /// child under lazy expansion (§3.3.1; `HashTrie::resolve` builds its
    /// table by [`build_child_table`](Self::build_child_table) on the first
    /// probe), and otherwise the table line 11 builds.
    pub(super) fn child(
        tuples: &Tuples, depth: usize, arity: usize, mut list: TupleList, config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        if P::ENABLED && list.len() == 1 {
            return HashTrieNode::Singleton(P::Payload::from_row(list[0]));
        }
        if E::LAZY {
            // `insert_at` starts a pending list as `vec![row]`, capacity 1,
            // or after an unprune as `Vec::with_capacity(2)`. A list grown
            // from empty reaches capacity 4 on its first push (std's
            // smallest non-zero capacity for a 4-byte `RowId`, as for the
            // 24-byte tuples the lists held before #111), and from there
            // both grow alike. Shrinking the two short cases keeps the trie
            // byte-identical to `incremental`'s, so `space` cannot move, and
            // keeps a one-row pending list at one slot. Both the first
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
            return HashTrieNode::Unexpanded(E::Pending::from_rows(list));
        }
        Self::build_child_table(tuples, depth, arity, list, config)
    }

    /// The table a bucket's `list` becomes at `depth` when it is built
    /// (line 11): eagerly by [`child`](Self::child), or on the first probe by
    /// `HashTrie::resolve` under lazy expansion. One definition, so a child
    /// expanded from a list is the eager child built from that list, size
    /// included.
    pub(super) fn build_child_table(
        tuples: &Tuples, depth: usize, arity: usize, list: TupleList, config: HashTrieConfig,
    ) -> HashTrieNode<P, E> {
        let log2_capacity = config.child_log2_capacity(list.len());
        Self::build(tuples, depth, arity, list, log2_capacity, config)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            ds::hash_trie::{
                build_mode::{HashTrieBuildMode, RadixBits},
                config::{ChildCapacity, LoadFactor, RootCapacity},
                expansion::{EagerExpansion, LazyExpansion},
                identity::{assert_same_trie, incremental_configs, inputs},
                pruning::{NoPruning, SingletonPruning},
            },
            morsel::Threads,
            relation::ConfigurableRelation,
            test_support::{Lcg, Mod10HashStrategy},
        },
        kermit_iters::{FxHashStrategy, SipHashStrategy},
    };

    fn label<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
        arity: usize, config: HashTrieConfig, input: &str,
    ) -> String {
        format!(
            "{}/{}/{} arity {arity}, load {}%, root {}, children {}, {input}",
            H::NAME,
            P::NAME,
            E::NAME,
            config.load_factor.numerator(),
            config.root_capacity.axis_value(),
            config.child_capacity.axis_value(),
        )
    }

    /// Algorithm 2 builds the trie the per-tuple build makes, array for
    /// array: capacities, chains, singletons and pending lists included.
    /// The `interleaved` input gives an arity-2 trie a one-tuple and a
    /// two-tuple child, the two lengths whose pending lists `child` shrinks.
    fn check_identity<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for config in incremental_configs() {
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
    /// so distinct tuples meet in one chain, which must list their ids in
    /// input order as the per-tuple build does. Under SipHash or FxHash a
    /// chain holds only equal tuples, so only a colliding fixture can see
    /// its order.
    #[test]
    fn bulk_keeps_chain_order_under_colliding_hashes() {
        let tuples = vec![vec![1, 3], vec![11, 13], vec![1, 13], vec![11, 3], vec![
            1, 3,
        ]];
        fn check<P: PruningPolicy, E: ExpansionPolicy>(tuples: &[Vec<usize>]) {
            for config in incremental_configs() {
                let incremental = HashTrie::<Mod10HashStrategy, P, E>::from_tuples_incrementally(
                    2.into(),
                    config,
                    Tuples::from(tuples.to_vec()),
                );
                let bulk = HashTrie::<Mod10HashStrategy, P, E>::from_tuples_in_bulk(
                    2.into(),
                    config,
                    Tuples::from(tuples.to_vec()),
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
        // The eager trie's one chain holds all five ids, in input order.
        let bulk: HashTrie<Mod10HashStrategy> = HashTrie::from_tuples_in_bulk(
            2.into(),
            HashTrieConfig::default(),
            Tuples::from(tuples),
        );
        let HashTrieNode::Inner(root) = bulk.root() else {
            panic!("arity 2 has an Inner root")
        };
        assert_eq!(root.len(), 1, "1 and 11 share a root bucket");
        let Some((_, HashTrieNode::Leaf(leaf))) = root.iter().next() else {
            panic!("the root's one child is the leaf")
        };
        assert_eq!(leaf.len(), 1, "3 and 13 share a leaf chain");
        let Some((_, chain)) = leaf.iter().next() else {
            panic!("one chain")
        };
        assert_eq!(chain, &vec![0, 1, 2, 3, 4]);
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
                for (input, tuples) in [
                    ("random", Tuples::from(random)),
                    ("half one key", Tuples::from(skewed)),
                ] {
                    for config in incremental_configs() {
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

    /// The tuples stored below `node`, built or pending.
    fn tuples_below<P: PruningPolicy, E: ExpansionPolicy>(node: &HashTrieNode<P, E>) -> usize {
        match node {
            | HashTrieNode::Inner(table) => {
                table.iter().map(|(_, child)| tuples_below(child)).sum()
            },
            | HashTrieNode::Leaf(table) => table.iter().map(|(_, chain)| chain.len()).sum(),
            | HashTrieNode::Singleton(_) => 1,
            | HashTrieNode::Unexpanded(pending) => match pending.built() {
                | Some(built) => tuples_below(built),
                | None => pending.pending().len(),
            },
        }
    }

    /// Every table below `node` has the capacity `child_log2_capacity` gives
    /// the tuples below it, expanding lazy children on the way by `resolve`,
    /// as a probe would. A table starts at that capacity and growth only
    /// enlarges it, so equality also shows that it never grew.
    fn assert_children_sized<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
        trie: &HashTrie<H, P, E>, node: &HashTrieNode<P, E>, depth: usize, label: &str,
    ) {
        let HashTrieNode::Inner(table) = node else {
            return;
        };
        for (hash, child) in table.iter() {
            let child = trie.resolve(child, depth + 1);
            if matches!(child, HashTrieNode::Inner(_) | HashTrieNode::Leaf(_)) {
                assert_eq!(
                    child.buckets_len(),
                    1 << trie.config().child_log2_capacity(tuples_below(child)),
                    "{label}: the child at depth {} under {hash:#x}",
                    depth + 1
                );
                assert_children_sized(trie, child, depth + 1, label);
            }
        }
    }

    /// Under `child-capacity=tuples` every mode sizes every child once from
    /// its list, eager or lazy, pruned or not. Miri leaves out the threaded
    /// modes.
    fn check_children_sized<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>() {
        let percents: &[u8] = if cfg!(miri) {
            &[70]
        } else {
            &[50, 70, 80]
        };
        for &percent in percents {
            for root_capacity in [RootCapacity::Grow, RootCapacity::Tuples] {
                let config = HashTrieConfig {
                    load_factor: LoadFactor::percent(percent).unwrap(),
                    root_capacity,
                    child_capacity: ChildCapacity::Tuples,
                };
                let mut modes = vec![
                    HashTrieBuildMode::Bulk,
                    HashTrieBuildMode::Radix(RadixBits::new(2).unwrap()),
                ];
                if !cfg!(miri) {
                    modes.push(HashTrieBuildMode::Parallel(Threads::new(2).unwrap()));
                    if root_capacity == RootCapacity::Tuples {
                        modes.push(HashTrieBuildMode::Presized(Threads::new(2).unwrap()));
                    }
                }
                for arity in 1..=3 {
                    for (input, tuples) in inputs(arity) {
                        for &mode in &modes {
                            let trie = HashTrie::<H, P, E>::from_tuples_with_config_and_build_mode(
                                arity.into(),
                                config,
                                mode,
                                tuples.clone(),
                            );
                            let label =
                                format!("{} {mode:?}", label::<H, P, E>(arity, config, input));
                            assert_children_sized(&trie, trie.root(), 0, &label);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tuples_sizes_every_child_from_its_list() {
        check_children_sized::<SipHashStrategy, NoPruning, EagerExpansion>();
        check_children_sized::<SipHashStrategy, SingletonPruning, EagerExpansion>();
        check_children_sized::<SipHashStrategy, NoPruning, LazyExpansion>();
        check_children_sized::<SipHashStrategy, SingletonPruning, LazyExpansion>();
        check_children_sized::<FxHashStrategy, SingletonPruning, LazyExpansion>();
        check_children_sized::<Mod10HashStrategy, NoPruning, EagerExpansion>();
    }

    /// `incremental` creates a child on its first tuple, before its list is
    /// known, so it cannot size it. The CLI rejects the pair first; here it
    /// is a broken invariant, like a wrong arity.
    #[test]
    #[should_panic(expected = "hash-trie=incremental requires child-capacity=grow")]
    fn incremental_requires_growing_children() {
        let _: HashTrie = HashTrie::from_tuples_with_config_and_build_mode(
            2.into(),
            HashTrieConfig {
                child_capacity: ChildCapacity::Tuples,
                ..HashTrieConfig::default()
            },
            HashTrieBuildMode::Incremental,
            vec![vec![1, 2]],
        );
    }

    /// A batch of another arity. (A batch of mixed arities never reaches a
    /// build: `Tuples::from` refuses it.)
    #[test]
    #[should_panic(expected = "does not match header arity")]
    fn bulk_build_rejects_a_wrong_arity() {
        let _: HashTrie = HashTrie::from_tuples_in_bulk(
            2.into(),
            HashTrieConfig::default(),
            Tuples::from(vec![vec![1, 2, 3]]),
        );
    }
}
