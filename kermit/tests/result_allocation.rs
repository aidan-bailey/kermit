//! Issue #65: counting a join's result allocates nothing per result row.
//!
//! `bench run`'s `iteration` metric times a join whose rows are counted
//! through a `black_box` sink. If that path allocated per row, the timed
//! region would again include result materialisation, and memory would
//! again grow with the result. Each test here counts the calling thread's
//! allocations (via `allocation-counter`, which installs the counting global
//! allocator for this test binary only) during one streamed join at two
//! result sizes 1000x apart, and requires the two counts to be equal.
//!
//! **What this isolates.** The query is `Q(X, Y, Z) :- R(X, Y), S(X, Z).`
//! with `R` fixed at 10 tuples, and only `S`'s fan-out per `X` grows. The
//! number of trie descents is therefore the same at both sizes, so
//! per-descent allocation — LFTJ's `update_iters` builds a `Vec` on every
//! `open`/`up`, which is algorithm cost, not materialisation — cancels out.
//! The only term that could differ is one that scales with the result. The
//! shared `X` makes HashTriejoin verify a real join condition at the leaf.
//! Each cell runs one unmeasured join first, so a one-time lazy
//! initialisation on the join path cannot masquerade as per-row allocation.

use {
    kermit::db::{hash_join_for_each, lftj_join_for_each},
    kermit_algos::{JoinQuery, LeapfrogTriejoin, LexicographicOptimiser},
    kermit_ds::{
        Cardinality, ColumnTrie, HashTrie, NoPruning, PruningPolicy, Relation, SingletonPruning,
        TreeTrie,
    },
    kermit_iters::{FxHashStrategy, HashStrategy, SipHashStrategy, TrieIterable},
    std::collections::BTreeMap,
};

const QUERY: &str = "Q(X, Y, Z) :- R(X, Y), S(X, Z).";

/// Distinct `X` values; fixed, so the number of descents is fixed.
const XS: usize = 10;

/// `S` fan-out per `X` for the small (100-row) and large (100,000-row)
/// results.
const SMALL: usize = 10;
const LARGE: usize = 10_000;

/// `R = {(x, 0)}` and `S = {(x, z) : z < fan_out}` for each of the `XS`
/// keys, so the join has `XS * fan_out` rows.
fn relations<Rel: Relation>(fan_out: usize) -> BTreeMap<String, Rel> {
    let r: Vec<Vec<usize>> = (0..XS).map(|x| vec![x, 0]).collect();
    let s: Vec<Vec<usize>> = (0..XS)
        .flat_map(|x| (0..fan_out).map(move |z| vec![x, z]))
        .collect();
    BTreeMap::from([
        ("R".to_string(), Rel::from_tuples(2.into(), r)),
        ("S".to_string(), Rel::from_tuples(2.into(), s)),
    ])
}

/// Allocations made by one streamed LFTJ join, after checking that it
/// produced every row.
fn lftj_allocations<Rel: TrieIterable + Cardinality + Relation>(fan_out: usize) -> u64 {
    let relations = relations::<Rel>(fan_out);
    let query: JoinQuery = QUERY.parse().unwrap();
    let mut rows = 0usize;
    let info = allocation_counter::measure(|| {
        lftj_join_for_each::<Rel, LeapfrogTriejoin>(
            &relations,
            query,
            &LexicographicOptimiser,
            |tuple| {
                std::hint::black_box(tuple);
                rows += 1;
            },
        );
    });
    assert_eq!(rows, XS * fan_out, "the join must produce every row");
    info.count_total
}

/// Allocations made by one streamed HashTriejoin join over
/// `HashTrie<H, P>`, after checking that it produced every row.
fn htj_allocations<H: HashStrategy, P: PruningPolicy>(fan_out: usize) -> u64 {
    let relations = relations::<HashTrie<H, P>>(fan_out);
    let query: JoinQuery = QUERY.parse().unwrap();
    let mut rows = 0usize;
    let info = allocation_counter::measure(|| {
        hash_join_for_each::<HashTrie<H, P>, H>(
            &relations,
            query,
            &LexicographicOptimiser,
            |tuple| {
                std::hint::black_box(tuple);
                rows += 1;
            },
        );
    });
    assert_eq!(rows, XS * fan_out, "the join must produce every row");
    info.count_total
}

fn assert_flat(cell: &str, small: u64, large: u64) {
    assert_eq!(
        small, large,
        "{cell}: a 1000x larger result changed the allocation count from {small} to {large}; the \
         streamed join is allocating per result row"
    );
}

#[test]
fn tree_trie_lftj_allocates_independently_of_result_size() {
    lftj_allocations::<TreeTrie>(SMALL);
    assert_flat(
        "TreeTrie/LFTJ",
        lftj_allocations::<TreeTrie>(SMALL),
        lftj_allocations::<TreeTrie>(LARGE),
    );
}

#[test]
fn column_trie_lftj_allocates_independently_of_result_size() {
    lftj_allocations::<ColumnTrie>(SMALL);
    assert_flat(
        "ColumnTrie/LFTJ",
        lftj_allocations::<ColumnTrie>(SMALL),
        lftj_allocations::<ColumnTrie>(LARGE),
    );
}

#[test]
fn hash_trie_sip_allocates_independently_of_result_size() {
    htj_allocations::<SipHashStrategy, NoPruning>(SMALL);
    assert_flat(
        "HashTrie<Sip>/HTJ",
        htj_allocations::<SipHashStrategy, NoPruning>(SMALL),
        htj_allocations::<SipHashStrategy, NoPruning>(LARGE),
    );
}

#[test]
fn hash_trie_fx_allocates_independently_of_result_size() {
    htj_allocations::<FxHashStrategy, NoPruning>(SMALL);
    assert_flat(
        "HashTrie<Fx>/HTJ",
        htj_allocations::<FxHashStrategy, NoPruning>(SMALL),
        htj_allocations::<FxHashStrategy, NoPruning>(LARGE),
    );
}

#[test]
fn hash_trie_sip_pruned_allocates_independently_of_result_size() {
    htj_allocations::<SipHashStrategy, SingletonPruning>(SMALL);
    assert_flat(
        "HashTrie<Sip, Pruned>/HTJ",
        htj_allocations::<SipHashStrategy, SingletonPruning>(SMALL),
        htj_allocations::<SipHashStrategy, SingletonPruning>(LARGE),
    );
}

#[test]
fn hash_trie_fx_pruned_allocates_independently_of_result_size() {
    htj_allocations::<FxHashStrategy, SingletonPruning>(SMALL);
    assert_flat(
        "HashTrie<Fx, Pruned>/HTJ",
        htj_allocations::<FxHashStrategy, SingletonPruning>(SMALL),
        htj_allocations::<FxHashStrategy, SingletonPruning>(LARGE),
    );
}
