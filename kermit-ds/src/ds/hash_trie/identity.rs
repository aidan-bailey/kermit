//! Identity checks for the HashTrie builds (#91, #94, #88, #101). Every
//! build mode must build the trie the bulk build does, the root-capacity
//! Config must leave every subtrie as it was, and the `presized:N` build
//! must be equivalent (Amendment 2), so the radix, parallel and
//! root-capacity tests share these assertions and inputs.
//!
//! A trie's singletons hold row ids into its buffer (#111), and its leaf and
//! pending chains are lists of such rows threaded through its links (#101
//! B2), so two tries compare at one of two levels:
//!
//! - **Array level**, [`assert_same_trie`] and [`assert_same_node`]: equal ids.
//!   Every chain is the same `Chain` (head, tail and length) and walks the same
//!   ids, each through its own trie's links. Equal ids name equal tuples only
//!   over equal buffers, so `assert_same_trie` compares the buffers first, then
//!   the links arrays whole. For builds that keep their input batch as their
//!   buffer, as `bulk` and `incremental` do.
//! - **Content level**, [`assert_same_contents`] and
//!   [`assert_same_node_contents`]: every table bucket for bucket, as at the
//!   array level, and every chain and singleton naming the same tuples in the
//!   same order, each read through its own trie's buffer and links ([`Rows`]);
//!   the buffers hold the same rows at the same capacity. For builds that may
//!   store their rows in another order than they arrived in (#101 layer 3's
//!   partitioned copy), whose ids, and so links, differ.
//!
//! `presized:N` may place root keys in other buckets (Amendment 2):
//! [`assert_equivalent_root`] checks its root rule over roots built from
//! one input batch, with children compared by id, and
//! [`assert_equivalent_trie`] checks whole tries, with children compared
//! by contents.

use {
    super::{
        chain::Chain,
        config::{ChildCapacity, HashTrieConfig, LoadFactor, RootCapacity},
        expansion::{ExpansionPolicy, PendingChild},
        hash_table::{home_bucket, HashTable},
        implementation::HashTrie,
        node::HashTrieNode,
        pruning::{PruningPolicy, SingletonPayload},
    },
    crate::{cardinality::Cardinality, heap_size::HeapSize, test_support::Lcg},
    kermit_iters::{ChainLinks, HashStrategy, RowId, Tuples},
};

/// A trie's rows as its chains name them: its buffer and its links. A
/// content-level comparison reads each side's chains through its own pair.
#[derive(Clone, Copy)]
pub(super) struct Rows<'t> {
    tuples: &'t Tuples,
    links: &'t ChainLinks,
}

impl<'t> Rows<'t> {
    /// The rows of `tuples`, as the chains threaded through `links` name
    /// them.
    pub(super) fn new(tuples: &'t Tuples, links: &'t ChainLinks) -> Self {
        Self {
            tuples,
            links,
        }
    }

    /// `trie`'s buffer and links.
    pub(super) fn of<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
        trie: &'t HashTrie<H, P, E>,
    ) -> Self {
        Self::new(trie.tuples(), trie.links())
    }

    /// The rows `chain` names, in chain order.
    fn of_chain(self, chain: &Chain) -> Vec<&'t [usize]> {
        let Rows {
            tuples,
            links,
        } = self;
        chain.walk(links).map(|row| tuples.row(row)).collect()
    }

    /// The row `row` names.
    fn row(self, row: RowId) -> &'t [usize] { self.tuples.row(row) }
}

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

/// Two chains, each walked through its own trie's links: the same `Chain`
/// (head, tail and length), and the same row ids in chain order.
fn assert_same_chain(a_links: &ChainLinks, a: &Chain, b_links: &ChainLinks, b: &Chain, path: &str) {
    assert_eq!(a, b, "{path}: chain");
    assert_eq!(
        a.walk(a_links).collect::<Vec<_>>(),
        b.walk(b_links).collect::<Vec<_>>(),
        "{path}: chain ids"
    );
}

/// Two nodes at the array level: every table bucket for bucket, every chain
/// the same `Chain` walking the same ids (`a`'s through `a_links`, `b`'s
/// through `b_links`), every singleton the same row id, and every pending
/// chain the same.
pub(super) fn assert_same_node<P: PruningPolicy, E: ExpansionPolicy>(
    a_links: &ChainLinks, a: &HashTrieNode<P, E>, b_links: &ChainLinks, b: &HashTrieNode<P, E>,
    path: &str,
) {
    match (a, b) {
        | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
            assert_same_table(x, y, path, &|x, y, path| {
                assert_same_node(a_links, x, b_links, y, path)
            })
        },
        | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
            assert_same_table(x, y, path, &|x, y, path| {
                assert_same_chain(a_links, x, b_links, y, path)
            })
        },
        | (HashTrieNode::Singleton(x), HashTrieNode::Singleton(y)) => {
            assert_eq!(x.row(), y.row(), "{path}: singleton");
        },
        | (HashTrieNode::Unexpanded(x), HashTrieNode::Unexpanded(y)) => {
            // Building expands nothing, so both children still hold the
            // pending chains `insert_at` or `child` left them.
            let (Some(x), Some(y)) = (x.pending(), y.pending()) else {
                panic!("{path}: expanded during the build")
            };
            assert_same_chain(a_links, &x, b_links, &y, &format!("{path}: pending"));
        },
        | _ => panic!("{path}: node variants differ"),
    }
}

/// The premise of every id comparison: the two tries own equal buffers,
/// row for row and at the same capacity.
fn assert_same_buffer<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_eq!(a.tuples(), b.tuples(), "{label}: buffer");
    assert_eq!(
        a.tuples().heap_size_bytes(),
        b.tuples().heap_size_bytes(),
        "{label}: buffer capacity"
    );
}

/// The array-level identity the standard requires of a BuildMode: the
/// buffer, the links array whole, every table's buckets and capacity, every
/// chain, singleton and pending chain, the heap size and the tuple count.
/// The links are compared before any node, so a build that threads a row
/// differently fails at `: links`.
pub(super) fn assert_same_trie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_same_buffer(a, b, label);
    assert_eq!(a.links().to_vec(), b.links().to_vec(), "{label}: links");
    assert_same_node(a.links(), a.root(), b.links(), b.root(), label);
    assert_eq!(
        a.heap_size_bytes(),
        b.heap_size_bytes(),
        "{label}: heap size"
    );
    assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
}

/// Two chains, each read through its own trie's buffer and links: the same
/// rows, in the same order. The ids may differ.
fn assert_same_chain_contents(a: Rows<'_>, x: &Chain, b: Rows<'_>, y: &Chain, path: &str) {
    assert_eq!(a.of_chain(x), b.of_chain(y), "{path}: chain rows");
}

/// [`assert_same_node`] by contents: every table bucket for bucket, as
/// there, but every chain, singleton and pending chain compared by the
/// tuples it names, `x`'s read through `a` and `y`'s through `b`.
pub(super) fn assert_same_node_contents<P: PruningPolicy, E: ExpansionPolicy>(
    a: Rows<'_>, x: &HashTrieNode<P, E>, b: Rows<'_>, y: &HashTrieNode<P, E>, path: &str,
) {
    match (x, y) {
        | (HashTrieNode::Inner(s), HashTrieNode::Inner(t)) => {
            assert_same_table(s, t, path, &|s, t, path| {
                assert_same_node_contents(a, s, b, t, path)
            })
        },
        | (HashTrieNode::Leaf(s), HashTrieNode::Leaf(t)) => {
            assert_same_table(s, t, path, &|s, t, path| {
                assert_same_chain_contents(a, s, b, t, path)
            })
        },
        | (HashTrieNode::Singleton(s), HashTrieNode::Singleton(t)) => {
            assert_eq!(a.row(*s.row()), b.row(*t.row()), "{path}: singleton row");
        },
        | (HashTrieNode::Unexpanded(s), HashTrieNode::Unexpanded(t)) => {
            // As in `assert_same_node`: building expands nothing.
            let (Some(s), Some(t)) = (s.pending(), t.pending()) else {
                panic!("{path}: expanded during the build")
            };
            assert_same_chain_contents(a, &s, b, &t, &format!("{path}: pending"));
        },
        | _ => panic!("{path}: node variants differ"),
    }
}

/// Two content-equal tries' buffers: the same rows as a multiset, at the
/// same capacity. The order of their rows, and so every id, may differ.
fn assert_same_buffer_contents(a: &Tuples, b: &Tuples, label: &str) {
    let sorted = |tuples: &Tuples| {
        let mut copy = tuples.clone();
        copy.sort();
        copy
    };
    assert_eq!(sorted(a), sorted(b), "{label}: buffer rows");
    assert_eq!(
        a.value_capacity(),
        b.value_capacity(),
        "{label}: buffer capacity"
    );
}

/// The content-level identity a build that stores its rows in another
/// order owes `bulk` (#101 layer 3):
///
/// - every table bucket for bucket, the root included, so this is also the
///   check that a build keeps `bulk`'s root;
/// - every chain, singleton and pending chain naming the same tuples in the
///   same order, each read through its own trie's buffer and links;
/// - buffers holding the same rows at the same capacity;
/// - and the heap size and tuple count.
///
/// The walk runs first, so a row that differs is reported where it sits.
pub(super) fn assert_same_contents<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_same_node_contents(Rows::of(a), a.root(), Rows::of(b), b.root(), label);
    assert_same_buffer_contents(a.tuples(), b.tuples(), label);
    assert_eq!(
        a.heap_size_bytes(),
        b.heap_size_bytes(),
        "{label}: heap size"
    );
    assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
}

/// Rows of up to three columns, cut to `arity`, as a batch whose capacity
/// is exactly its rows, so a batch and its clone have equal capacities.
pub(super) fn rows(arity: usize, rows: &[[usize; 3]]) -> Tuples {
    let mut tuples = Tuples::with_capacity(arity, rows.len());
    for row in rows {
        tuples.push(&row[..arity]);
    }
    tuples
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

/// The shared inputs, each a batch of `arity` whose capacity is exactly its
/// rows (see [`rows`]). The random rows are the draws made before #111, in
/// the same order.
pub(super) fn inputs(arity: usize) -> Vec<(&'static str, Tuples)> {
    let mut lcg = Lcg(0x91);
    let mut random = Tuples::with_capacity(arity, RANDOM_TUPLES);
    for _ in 0..RANDOM_TUPLES {
        let row = [
            lcg.next_usize() % (RANDOM_TUPLES / 4),
            lcg.next_usize() % 50,
            lcg.next_usize() % 7,
        ];
        random.push(&row[..arity]);
    }
    vec![
        ("empty", Tuples::new(arity)),
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
/// Every key's child or chain is array-identical, `a`'s walked through
/// `a_links` and `b`'s through `b_links`.
pub(super) fn assert_equivalent_root<P: PruningPolicy, E: ExpansionPolicy>(
    a_links: &ChainLinks, a: &HashTrieNode<P, E>, b_links: &ChainLinks, b: &HashTrieNode<P, E>,
    label: &str,
) {
    match (a, b) {
        | (HashTrieNode::Inner(x), HashTrieNode::Inner(y)) => {
            assert_equivalent_table(x, y, label, &|x, y, path| {
                assert_same_node(a_links, x, b_links, y, path)
            })
        },
        | (HashTrieNode::Leaf(x), HashTrieNode::Leaf(y)) => {
            assert_equivalent_table(x, y, label, &|x, y, path| {
                assert_same_chain(a_links, x, b_links, y, path)
            })
        },
        | _ => panic!("{label}: root variants differ"),
    }
}

/// [`assert_equivalent_root`]'s root rule with every key's child or chain
/// compared by contents ([`assert_same_node_contents`]), `x`'s read through
/// `a` and `y`'s through `b`: the rule for builds that may also store their
/// rows in another order (#101 layer 3).
fn assert_equivalent_root_contents<P: PruningPolicy, E: ExpansionPolicy>(
    a: Rows<'_>, x: &HashTrieNode<P, E>, b: Rows<'_>, y: &HashTrieNode<P, E>, label: &str,
) {
    match (x, y) {
        | (HashTrieNode::Inner(s), HashTrieNode::Inner(t)) => {
            assert_equivalent_table(s, t, label, &|s, t, path| {
                assert_same_node_contents(a, s, b, t, path)
            })
        },
        | (HashTrieNode::Leaf(s), HashTrieNode::Leaf(t)) => {
            assert_equivalent_table(s, t, label, &|s, t, path| {
                assert_same_chain_contents(a, s, b, t, path)
            })
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

/// `presized:N`'s rule for whole tries: [`assert_equivalent_root`]'s root
/// rule with every child compared by contents, buffers holding the same
/// rows at the same capacity, and the same heap size and tuple count.
pub(super) fn assert_equivalent_trie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    a: &HashTrie<H, P, E>, b: &HashTrie<H, P, E>, label: &str,
) {
    assert_same_buffer_contents(a.tuples(), b.tuples(), label);
    assert_equivalent_root_contents(Rows::of(a), a.root(), Rows::of(b), b.root(), label);
    assert_eq!(
        a.heap_size_bytes(),
        b.heap_size_bytes(),
        "{label}: heap size"
    );
    assert_eq!(a.tuple_count(), b.tuple_count(), "{label}: tuple count");
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            ds::hash_trie::{
                bulk::all_rows,
                expansion::{EagerExpansion, LazyExpansion},
                pruning::{NoPruning, SingletonPruning},
            },
            relation::{ConfigurableRelation, Relation},
            test_support::Mod10HashStrategy,
        },
        kermit_iters::SipHashStrategy,
    };

    /// The rows of [`inputs`]' "interleaved" batch, at arity 3.
    const INTERLEAVED: [[usize; 3]; 6] = [[1, 2, 3], [2, 3, 4], [1, 5, 6], [3, 1, 1], [2, 3, 9], [
        1, 2, 7,
    ]];

    /// Two builds of one input are the same at the content level. Every
    /// node variant meets the comparison: `Inner` and `Leaf` at every arity
    /// above 1, `Singleton` under pruning, `Unexpanded` under lazy
    /// expansion.
    fn check_accepts_the_same_trie<P: PruningPolicy, E: ExpansionPolicy>() {
        for arity in 1..=3 {
            for (input, tuples) in inputs(arity) {
                let label = format!("{}/{} arity {arity}, {input}", P::NAME, E::NAME);
                let a =
                    HashTrie::<SipHashStrategy, P, E>::from_tuples(arity.into(), tuples.clone());
                let b = HashTrie::<SipHashStrategy, P, E>::from_tuples(arity.into(), tuples);
                assert_same_contents(&a, &b, &label);
            }
        }
    }

    #[test]
    fn same_contents_accepts_the_same_trie() {
        check_accepts_the_same_trie::<NoPruning, EagerExpansion>();
        check_accepts_the_same_trie::<SingletonPruning, EagerExpansion>();
        check_accepts_the_same_trie::<NoPruning, LazyExpansion>();
        check_accepts_the_same_trie::<SingletonPruning, LazyExpansion>();
    }

    /// `tuples`' rows in reverse order, and the ids that name `tuples' rows
    /// in the reversed batch, in input order.
    fn reverse_rows(tuples: &Tuples) -> (Tuples, Vec<RowId>) {
        let mut reversed = Tuples::with_capacity(tuples.arity(), tuples.len());
        for row in all_rows(tuples).rev() {
            reversed.push(tuples.row(row));
        }
        (reversed, all_rows(tuples).rev().collect())
    }

    /// The bulk build's root (Algorithm 2 from depth 0, as
    /// `from_tuples_in_bulk` builds it) over the rows of `tuples` that `ids`
    /// names, in that order, with the links it threads them through.
    fn root_over<P: PruningPolicy, E: ExpansionPolicy>(
        tuples: &Tuples, arity: usize, ids: impl IntoIterator<Item = RowId>,
    ) -> (ChainLinks, HashTrieNode<P, E>) {
        let config = HashTrieConfig::default();
        let log2_capacity = config.root_log2_capacity(tuples.len());
        let links = ChainLinks::new(tuples.len());
        let root = HashTrie::<SipHashStrategy, P, E>::build(
            tuples,
            &links,
            0,
            arity,
            ids,
            log2_capacity,
            config,
        );
        (links, root)
    }

    /// Calls `check` on every input's root and on the root built over the
    /// reversed batch from its rows in input order: the same tuples under
    /// other ids, at every arity, each with its own buffer and links.
    fn for_each_reversed_root<P: PruningPolicy, E: ExpansionPolicy>(
        check: impl Fn(Rows<'_>, &HashTrieNode<P, E>, Rows<'_>, &HashTrieNode<P, E>, &str),
    ) {
        for arity in 1..=3 {
            for (input, tuples) in inputs(arity) {
                let label = format!("{}/{} arity {arity}, {input}", P::NAME, E::NAME);
                let (reversed, ids) = reverse_rows(&tuples);
                let (forward_links, forward) = root_over::<P, E>(&tuples, arity, all_rows(&tuples));
                let (backward_links, backward) = root_over::<P, E>(&reversed, arity, ids);
                check(
                    Rows::new(&tuples, &forward_links),
                    &forward,
                    Rows::new(&reversed, &backward_links),
                    &backward,
                    &label,
                );
            }
        }
    }

    #[test]
    fn same_node_contents_accepts_a_reversed_buffer() {
        for_each_reversed_root::<NoPruning, EagerExpansion>(assert_same_node_contents);
        for_each_reversed_root::<SingletonPruning, EagerExpansion>(assert_same_node_contents);
        for_each_reversed_root::<NoPruning, LazyExpansion>(assert_same_node_contents);
        for_each_reversed_root::<SingletonPruning, LazyExpansion>(assert_same_node_contents);
    }

    /// `assert_equivalent_trie`'s root rule reads each child through its
    /// own buffer and links, so a reversed buffer's root is equivalent to
    /// the forward one.
    #[test]
    fn equivalent_root_contents_accepts_a_reversed_buffer() {
        for_each_reversed_root::<NoPruning, EagerExpansion>(assert_equivalent_root_contents);
        for_each_reversed_root::<SingletonPruning, EagerExpansion>(assert_equivalent_root_contents);
        for_each_reversed_root::<NoPruning, LazyExpansion>(assert_equivalent_root_contents);
        for_each_reversed_root::<SingletonPruning, LazyExpansion>(assert_equivalent_root_contents);
    }

    /// Guards the test above. Compared by id, the reversed root differs,
    /// so that test passes on the tuples, not because the ids stayed put.
    #[test]
    #[should_panic(expected = ": chain")]
    fn array_level_rejects_a_reversed_buffer() {
        let tuples = rows(2, &INTERLEAVED);
        let (reversed, ids) = reverse_rows(&tuples);
        let (forward_links, forward) =
            root_over::<NoPruning, EagerExpansion>(&tuples, 2, all_rows(&tuples));
        let (backward_links, backward) = root_over::<NoPruning, EagerExpansion>(&reversed, 2, ids);
        assert_same_node(
            &forward_links,
            &forward,
            &backward_links,
            &backward,
            "reversed",
        );
    }

    /// `assert_same_trie` compares the links whole, before any node. Two
    /// builds of one input, with one link of the second rewritten (through
    /// `&`, as lazy expansion writes), fail there.
    #[test]
    #[should_panic(expected = ": links")]
    fn same_trie_rejects_another_link() {
        let a: HashTrie = HashTrie::from_tuples(2.into(), rows(2, &INTERLEAVED));
        let b: HashTrie = HashTrie::from_tuples(2.into(), rows(2, &INTERLEAVED));
        // Rows 0 and 5 are both `[1, 2]`, so they share a leaf chain.
        assert_eq!(b.links().get(0), 5, "the premise");
        b.links().set(0, ChainLinks::NIL);
        assert_same_trie(&a, &b, "rewritten");
    }

    /// [`INTERLEAVED`] against a copy whose row `index` has its last value
    /// raised by 10. `Mod10HashStrategy` hashes both values alike, so every
    /// table matches bucket for bucket and only the rows differ.
    fn check_rows_that_hash_alike<P: PruningPolicy, E: ExpansionPolicy>(index: usize) {
        let mut changed = INTERLEAVED;
        changed[index][2] += 10;
        let a = HashTrie::<Mod10HashStrategy, P, E>::from_tuples(3.into(), rows(3, &INTERLEAVED));
        let b = HashTrie::<Mod10HashStrategy, P, E>::from_tuples(3.into(), rows(3, &changed));
        assert_same_contents(&a, &b, "hash-alike");
    }

    /// Row 5, `[1, 2, 7]`, sits in a leaf chain.
    #[test]
    #[should_panic(expected = "chain rows")]
    fn same_contents_rejects_a_chain_row_that_hashes_alike() {
        check_rows_that_hash_alike::<NoPruning, EagerExpansion>(5);
    }

    /// Row 3, `[3, 1, 1]`, is the only tuple under key 3, so pruning makes
    /// it a `Singleton`.
    #[test]
    #[should_panic(expected = "singleton row")]
    fn same_contents_rejects_a_singleton_row_that_hashes_alike() {
        check_rows_that_hash_alike::<SingletonPruning, EagerExpansion>(3);
    }

    /// Under lazy expansion every root child is a pending chain.
    #[test]
    #[should_panic(expected = "pending: chain rows")]
    fn same_contents_rejects_a_pending_row_that_hashes_alike() {
        check_rows_that_hash_alike::<NoPruning, LazyExpansion>(3);
    }

    /// One chain holds the same two rows in the other order. The buffers
    /// hold the same multiset and every hash matches, so only the chain's
    /// order tells the tries apart.
    #[test]
    #[should_panic(expected = "chain rows")]
    fn same_contents_rejects_a_chain_in_another_order() {
        let a: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples(2.into(), rows(2, &[[1, 2, 0], [1, 12, 0]]));
        let b: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples(2.into(), rows(2, &[[1, 12, 0], [1, 2, 0]]));
        assert_same_contents(&a, &b, "reordered");
    }

    /// `assert_equivalent_trie` reads each side's chains through its own
    /// buffer and links. The pair of
    /// `same_contents_rejects_a_chain_in_another_order` has the same buffer
    /// multiset, capacity and root buckets, and equal ids in the chain, so
    /// only the rows those ids name tell the tries apart. (The hash-alike
    /// pair would fail the buffer check first.)
    #[test]
    #[should_panic(expected = "chain rows")]
    fn equivalent_trie_rejects_a_chain_in_another_order() {
        let a: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples(2.into(), rows(2, &[[1, 2, 0], [1, 12, 0]]));
        let b: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples(2.into(), rows(2, &[[1, 12, 0], [1, 2, 0]]));
        assert_equivalent_trie(&a, &b, "reordered");
    }

    /// `equivalent_trie_rejects_a_chain_in_another_order` at arity 1, where
    /// the root is a `Leaf`: `[1]` and `[11]` share hash 1, so they share
    /// the root's one chain, in the other order.
    #[test]
    #[should_panic(expected = "chain rows")]
    fn equivalent_trie_rejects_an_arity_1_chain_in_another_order() {
        let a: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples(1.into(), rows(1, &[[1, 0, 0], [11, 0, 0]]));
        let b: HashTrie<Mod10HashStrategy> =
            HashTrie::from_tuples(1.into(), rows(1, &[[11, 0, 0], [1, 0, 0]]));
        assert_equivalent_trie(&a, &b, "reordered");
    }

    /// The same trie over a buffer with spare capacity: equal nodes and
    /// rows, another capacity.
    #[test]
    #[should_panic(expected = "buffer capacity")]
    fn same_contents_rejects_another_buffer_capacity() {
        let exact = rows(2, &INTERLEAVED);
        let mut spare = Tuples::with_capacity(2, 64);
        for row in exact.rows() {
            spare.push(row);
        }
        let a: HashTrie = HashTrie::from_tuples(2.into(), exact);
        let b: HashTrie = HashTrie::from_tuples(2.into(), spare);
        assert_same_contents(&a, &b, "spare");
    }

    /// The root is compared bucket for bucket, too. The same rows under a
    /// root of another capacity (8 buckets grown, 16 sized for six tuples
    /// at 70 %) fail, so `assert_same_contents` is also the check that a
    /// build keeps `bulk`'s root.
    #[test]
    #[should_panic(expected = "root capacity: capacity")]
    fn same_contents_compares_the_root_bucket_for_bucket() {
        let build = |root_capacity| {
            let config = HashTrieConfig {
                root_capacity,
                ..HashTrieConfig::default()
            };
            HashTrie::<SipHashStrategy>::from_tuples_with_config(
                2.into(),
                config,
                rows(2, &INTERLEAVED),
            )
        };
        assert_same_contents(
            &build(RootCapacity::Grow),
            &build(RootCapacity::Tuples),
            "root capacity",
        );
    }
}
