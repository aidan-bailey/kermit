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
//! number of trie descents is therefore the same at both sizes, so any
//! per-descent allocation cancels out — the descent tests below pin that
//! separately — and the only term that could differ is one that scales with
//! the result. The shared `X` makes HashTriejoin verify a real join
//! condition at the leaf. Each cell runs one unmeasured join first, so a
//! one-time lazy initialisation on the join path cannot masquerade as
//! per-row allocation.
//!
//! Issue #79 extends the same check to `bench ds`, which scans a single
//! relation: the `*_scan_*` tests drive the exact walk
//! `RelationFamily::for_each_tuple` uses for each structure and Layout —
//! `TrieIteratorWrapper::advance` for the sorted tries,
//! `HashTrie::for_each_tuple` for the hash trie — over 100 and 100,000
//! stored tuples.
//!
//! Issue #83 adds the orthogonal check: the `*_descent_count` tests hold
//! the result at 10 rows and grow the number of trie descents 1000x with
//! dead-end prefixes, each of which descends, fails one level further down
//! and ascends again. LFTJ used to build a new inner leapfrog, two
//! allocations, on every `open` and `up`, so its count grew with the
//! descents; it now refills one in place. The HashTriejoin cells hold the
//! other side of every LFTJ-vs-HashTriejoin comparison to the same
//! standard.
//!
//! Issue #80 adds the seek strategies: the existing sorted cells run the
//! default (`galloping`), and the `linear` / `binary` cells below hold the
//! other two to the same standard: a strategy that allocated per seek would
//! show up in both checks. The scan cells are not multiplied, because the
//! scan never seeks.
//!
//! Issue #92 adds the lazy expansion Layout. Its join cells measure a
//! *second* join over the same relations, because a cold lazy join
//! allocates once per child it expands, by design. The scan never expands,
//! so its lazy cells measure the trie as built.
//!
//! Issue #93 added reordered copies; they are now just indexes. The
//! `*_over_a_permuted_index_*` cells read a reversed `R` through an index
//! whose columns are permuted into plan order, and the streamed `execute`
//! over it must allocate nothing per row either.
//!
//! Since the per-query index build every cell prepares its query and builds
//! its indexes outside the measured region, and measures only the streamed
//! `kermit::db::execute` over them.

use {
    kermit::db::{build, execute, prepare, Catalog, Indexes, Prepared},
    kermit_algos::{
        ColumnOrderPolicy, HashTriejoin, JoinAlgo, JoinQuery, LeapfrogTriejoin,
        LexicographicOptimiser, Planner, StatisticsLevel,
    },
    kermit_ds::{
        BinarySeek, ColumnTrie, EagerExpansion, ExpansionPolicy, HashTrie, LazyExpansion,
        LinearSeek, NoPruning, PruningPolicy, Relation, RelationHeader, SingletonPruning, TreeTrie,
        Tuples,
    },
    kermit_iters::{
        FxHashStrategy, HashStrategy, SipHashStrategy, TrieIterable, TrieIteratorWrapper,
    },
};

const QUERY: &str = "Q(X, Y, Z) :- R(X, Y), S(X, Z).";

/// Distinct `X` values; fixed, so the number of descents is fixed.
const XS: usize = 10;

/// `S` fan-out per `X` for the small (100-row) and large (100,000-row)
/// results.
const SMALL: usize = 10;
const LARGE: usize = 10_000;

/// A workload's relations as the catalog takes them: header and flat tuples.
type Inputs = Vec<(RelationHeader, Tuples)>;

/// The relation `name` of arity `arity` holding `rows`.
fn relation(name: &str, arity: usize, rows: Vec<Vec<usize>>) -> (RelationHeader, Tuples) {
    (
        RelationHeader::new_positional(name, arity),
        Tuples::from(rows),
    )
}

/// `R = {(x, 0)}` and `S = {(x, z) : z < fan_out}` for each of the `XS`
/// keys, so the join has `XS * fan_out` rows.
fn relations(fan_out: usize) -> Inputs {
    let r: Vec<Vec<usize>> = (0..XS).map(|x| vec![x, 0]).collect();
    vec![relation("R", 2, r), relation("S", 2, s_tuples(fan_out))]
}

/// `S = {(x, z) : z < fan_out}` for each of the `XS` keys: `XS * fan_out`
/// tuples.
fn s_tuples(fan_out: usize) -> Vec<Vec<usize>> {
    (0..XS)
        .flat_map(|x| (0..fan_out).map(move |z| vec![x, z]))
        .collect()
}

/// The planner every cell uses. Lexicographic, so the plan binds the
/// head's variables in head order.
fn planner() -> Planner { Planner::new(Box::new(LexicographicOptimiser), ColumnOrderPolicy::Any) }

/// A query prepared and its indexes built, untimed: what `execute` is
/// handed.
struct Built<R> {
    prepared: Prepared,
    indexes: Indexes<R>,
}

/// Prepares `query` over `inputs` and builds the indexes it reads as `R`.
fn prebuilt<R: Relation>(query: &str, inputs: Inputs) -> Built<R> {
    let catalog = Catalog::new(inputs, StatisticsLevel::TupleCounts);
    let query: JoinQuery = query.parse().unwrap();
    let prepared = prepare(&catalog, &query, &planner()).unwrap();
    let indexes = build::<R>(&catalog, &prepared);
    Built {
        prepared,
        indexes,
    }
}

/// Allocations made by one streamed `execute` over `built`, after checking
/// that it produced `rows` rows.
fn streamed_allocations<R: Relation, JA: JoinAlgo<R>>(built: &Built<R>, rows: usize) -> u64 {
    let mut produced = 0usize;
    let info = allocation_counter::measure(|| {
        execute::<R, JA>(&built.prepared, &built.indexes, |tuple| {
            std::hint::black_box(tuple);
            produced += 1;
        });
    });
    assert_eq!(produced, rows, "the join must produce every row");
    info.count_total
}

/// Allocations made by one streamed join of `query` over `inputs` as `R`
/// through `JA`, built beforehand.
fn join_allocations<R: Relation, JA: JoinAlgo<R>>(query: &str, inputs: Inputs, rows: usize) -> u64 {
    streamed_allocations::<R, JA>(&prebuilt::<R>(query, inputs), rows)
}

/// Allocations made by a *second* streamed join over the same lazy
/// indexes. The first join expands every child it reaches, so the second
/// allocates only what the join path itself does. A cold lazy join
/// allocates once per child it expands, by design (issue #92), so the
/// per-row and per-descent checks apply warm. The eager cells' unmeasured
/// first call builds fresh indexes, which would leave a lazy cell cold,
/// hence this helper.
fn warm_join_allocations<R: Relation, JA: JoinAlgo<R>>(
    query: &str, inputs: Inputs, rows: usize,
) -> u64 {
    let built = prebuilt::<R>(query, inputs);
    streamed_allocations::<R, JA>(&built, rows);
    streamed_allocations::<R, JA>(&built, rows)
}

/// Allocations made by one streamed LFTJ join of `QUERY`, after checking
/// that it produced every row.
fn lftj_allocations<Rel: TrieIterable + Relation>(fan_out: usize) -> u64 {
    join_allocations::<Rel, LeapfrogTriejoin>(QUERY, relations(fan_out), XS * fan_out)
}

/// Allocations made by one streamed HashTriejoin join of `QUERY` over
/// `HashTrie<H, P>`, after checking that it produced every row.
fn htj_allocations<H: HashStrategy, P: PruningPolicy>(fan_out: usize) -> u64 {
    join_allocations::<HashTrie<H, P>, HashTriejoin>(QUERY, relations(fan_out), XS * fan_out)
}

/// [`htj_allocations`] for the lazy Layout, measured warm.
fn htj_lazy_allocations<H: HashStrategy, P: PruningPolicy>(fan_out: usize) -> u64 {
    warm_join_allocations::<HashTrie<H, P, LazyExpansion>, HashTriejoin>(
        QUERY,
        relations(fan_out),
        XS * fan_out,
    )
}

/// `R` reversed: `{(0, x)}`, so [`REVERSED_QUERY`] binds its second column
/// first and its plan reads it through an index whose columns are
/// permuted.
fn reversed_r() -> Vec<Vec<usize>> { (0..XS).map(|x| vec![0, x]).collect() }

const REVERSED_QUERY: &str = "Q(X, Y, Z) :- R(Y, X), S(X, Z).";

/// [`REVERSED_QUERY`] prepared over the reversed `R` and `S`, its indexes
/// built before anything is measured. The plan binds `X` first, so the
/// index of `R` lists `R`'s second column first: the assertion below fails
/// if the cell stopped reading a permuted index.
fn reversed<Rel: Relation>(fan_out: usize) -> Built<Rel> {
    let inputs = vec![
        relation("R", 2, reversed_r()),
        relation("S", 2, s_tuples(fan_out)),
    ];
    let built = prebuilt::<Rel>(REVERSED_QUERY, inputs);
    let reads_r: Vec<_> = built
        .prepared
        .indexes()
        .iter()
        .filter(|spec| spec.relation == "R")
        .collect();
    assert_eq!(reads_r.len(), 1, "the reversed R is read through one index");
    assert_eq!(
        reads_r[0].columns,
        vec![1, 0],
        "the index of R must list its second column first"
    );
    built
}

/// Allocations of one streamed LFTJ join of [`REVERSED_QUERY`] over its
/// prebuilt permuted index, after checking it produced every row.
fn lftj_reversed_allocations<Rel: TrieIterable + Relation>(fan_out: usize) -> u64 {
    streamed_allocations::<Rel, LeapfrogTriejoin>(&reversed::<Rel>(fan_out), XS * fan_out)
}

/// The HashTriejoin twin of [`lftj_reversed_allocations`].
fn htj_reversed_allocations<H: HashStrategy, P: PruningPolicy>(fan_out: usize) -> u64 {
    streamed_allocations::<HashTrie<H, P>, HashTriejoin>(
        &reversed::<HashTrie<H, P>>(fan_out),
        XS * fan_out,
    )
}

fn assert_flat(cell: &str, small: u64, large: u64) {
    assert_eq!(
        small, large,
        "{cell}: 1000x more rows changed the allocation count from {small} to {large}; the \
         streamed path is allocating per row"
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
fn tree_trie_lftj_over_a_permuted_index_allocates_independently_of_result_size() {
    lftj_reversed_allocations::<TreeTrie>(SMALL);
    assert_flat(
        "TreeTrie/LFTJ/permuted index",
        lftj_reversed_allocations::<TreeTrie>(SMALL),
        lftj_reversed_allocations::<TreeTrie>(LARGE),
    );
}

#[test]
fn hash_trie_sip_over_a_permuted_index_allocates_independently_of_result_size() {
    htj_reversed_allocations::<SipHashStrategy, NoPruning>(SMALL);
    assert_flat(
        "HashTrie<Sip>/HTJ/permuted index",
        htj_reversed_allocations::<SipHashStrategy, NoPruning>(SMALL),
        htj_reversed_allocations::<SipHashStrategy, NoPruning>(LARGE),
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
fn tree_trie_linear_lftj_allocates_independently_of_result_size() {
    lftj_allocations::<TreeTrie<LinearSeek>>(SMALL);
    assert_flat(
        "TreeTrie<LinearSeek>/LFTJ",
        lftj_allocations::<TreeTrie<LinearSeek>>(SMALL),
        lftj_allocations::<TreeTrie<LinearSeek>>(LARGE),
    );
}

#[test]
fn tree_trie_binary_lftj_allocates_independently_of_result_size() {
    lftj_allocations::<TreeTrie<BinarySeek>>(SMALL);
    assert_flat(
        "TreeTrie<BinarySeek>/LFTJ",
        lftj_allocations::<TreeTrie<BinarySeek>>(SMALL),
        lftj_allocations::<TreeTrie<BinarySeek>>(LARGE),
    );
}

#[test]
fn column_trie_linear_lftj_allocates_independently_of_result_size() {
    lftj_allocations::<ColumnTrie<LinearSeek>>(SMALL);
    assert_flat(
        "ColumnTrie<LinearSeek>/LFTJ",
        lftj_allocations::<ColumnTrie<LinearSeek>>(SMALL),
        lftj_allocations::<ColumnTrie<LinearSeek>>(LARGE),
    );
}

#[test]
fn column_trie_binary_lftj_allocates_independently_of_result_size() {
    lftj_allocations::<ColumnTrie<BinarySeek>>(SMALL);
    assert_flat(
        "ColumnTrie<BinarySeek>/LFTJ",
        lftj_allocations::<ColumnTrie<BinarySeek>>(SMALL),
        lftj_allocations::<ColumnTrie<BinarySeek>>(LARGE),
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

#[test]
fn hash_trie_sip_lazy_allocates_independently_of_result_size() {
    assert_flat(
        "HashTrie<Sip, Lazy>/HTJ",
        htj_lazy_allocations::<SipHashStrategy, NoPruning>(SMALL),
        htj_lazy_allocations::<SipHashStrategy, NoPruning>(LARGE),
    );
}

#[test]
fn hash_trie_fx_lazy_allocates_independently_of_result_size() {
    assert_flat(
        "HashTrie<Fx, Lazy>/HTJ",
        htj_lazy_allocations::<FxHashStrategy, NoPruning>(SMALL),
        htj_lazy_allocations::<FxHashStrategy, NoPruning>(LARGE),
    );
}

#[test]
fn hash_trie_sip_pruned_lazy_allocates_independently_of_result_size() {
    assert_flat(
        "HashTrie<Sip, Pruned, Lazy>/HTJ",
        htj_lazy_allocations::<SipHashStrategy, SingletonPruning>(SMALL),
        htj_lazy_allocations::<SipHashStrategy, SingletonPruning>(LARGE),
    );
}

#[test]
fn hash_trie_fx_pruned_lazy_allocates_independently_of_result_size() {
    assert_flat(
        "HashTrie<Fx, Pruned, Lazy>/HTJ",
        htj_lazy_allocations::<FxHashStrategy, SingletonPruning>(SMALL),
        htj_lazy_allocations::<FxHashStrategy, SingletonPruning>(LARGE),
    );
}

// ── `bench ds` scans (issue #79) ────────────────────────────────────────

/// Allocations made by one scan of a sorted trie holding `S`, through the
/// walk `RelationFamily::for_each_tuple` drives, after checking that it
/// lent every tuple.
fn sorted_scan_allocations<Rel: TrieIterable + Relation>(fan_out: usize) -> u64 {
    let rel = Rel::from_tuples(2.into(), s_tuples(fan_out));
    let mut tuples = 0usize;
    let info = allocation_counter::measure(|| {
        let mut walk = TrieIteratorWrapper::new(rel.trie_iter());
        while let Some(tuple) = walk.advance() {
            std::hint::black_box(tuple);
            tuples += 1;
        }
    });
    assert_eq!(tuples, XS * fan_out, "the scan must visit every tuple");
    info.count_total
}

/// Allocations made by one scan of `HashTrie<H, P, E>` holding `S`,
/// through `HashTrie::for_each_tuple`, after checking that it lent every
/// tuple.
fn hash_scan_allocations<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    fan_out: usize,
) -> u64 {
    let rel = HashTrie::<H, P, E>::from_tuples(2.into(), s_tuples(fan_out));
    let mut tuples = 0usize;
    let info = allocation_counter::measure(|| {
        rel.for_each_tuple(|tuple| {
            std::hint::black_box(tuple);
            tuples += 1;
        });
    });
    assert_eq!(tuples, XS * fan_out, "the scan must visit every tuple");
    info.count_total
}

#[test]
fn tree_trie_scan_allocates_independently_of_relation_size() {
    sorted_scan_allocations::<TreeTrie>(SMALL);
    assert_flat(
        "TreeTrie scan",
        sorted_scan_allocations::<TreeTrie>(SMALL),
        sorted_scan_allocations::<TreeTrie>(LARGE),
    );
}

#[test]
fn column_trie_scan_allocates_independently_of_relation_size() {
    sorted_scan_allocations::<ColumnTrie>(SMALL);
    assert_flat(
        "ColumnTrie scan",
        sorted_scan_allocations::<ColumnTrie>(SMALL),
        sorted_scan_allocations::<ColumnTrie>(LARGE),
    );
}

#[test]
fn hash_trie_sip_scan_allocates_independently_of_relation_size() {
    hash_scan_allocations::<SipHashStrategy, NoPruning, EagerExpansion>(SMALL);
    assert_flat(
        "HashTrie<Sip> scan",
        hash_scan_allocations::<SipHashStrategy, NoPruning, EagerExpansion>(SMALL),
        hash_scan_allocations::<SipHashStrategy, NoPruning, EagerExpansion>(LARGE),
    );
}

#[test]
fn hash_trie_fx_scan_allocates_independently_of_relation_size() {
    hash_scan_allocations::<FxHashStrategy, NoPruning, EagerExpansion>(SMALL);
    assert_flat(
        "HashTrie<Fx> scan",
        hash_scan_allocations::<FxHashStrategy, NoPruning, EagerExpansion>(SMALL),
        hash_scan_allocations::<FxHashStrategy, NoPruning, EagerExpansion>(LARGE),
    );
}

#[test]
fn hash_trie_sip_pruned_scan_allocates_independently_of_relation_size() {
    hash_scan_allocations::<SipHashStrategy, SingletonPruning, EagerExpansion>(SMALL);
    assert_flat(
        "HashTrie<Sip, Pruned> scan",
        hash_scan_allocations::<SipHashStrategy, SingletonPruning, EagerExpansion>(SMALL),
        hash_scan_allocations::<SipHashStrategy, SingletonPruning, EagerExpansion>(LARGE),
    );
}

#[test]
fn hash_trie_fx_pruned_scan_allocates_independently_of_relation_size() {
    hash_scan_allocations::<FxHashStrategy, SingletonPruning, EagerExpansion>(SMALL);
    assert_flat(
        "HashTrie<Fx, Pruned> scan",
        hash_scan_allocations::<FxHashStrategy, SingletonPruning, EagerExpansion>(SMALL),
        hash_scan_allocations::<FxHashStrategy, SingletonPruning, EagerExpansion>(LARGE),
    );
}

#[test]
fn hash_trie_sip_lazy_scan_allocates_independently_of_relation_size() {
    hash_scan_allocations::<SipHashStrategy, NoPruning, LazyExpansion>(SMALL);
    assert_flat(
        "HashTrie<Sip, Lazy> scan",
        hash_scan_allocations::<SipHashStrategy, NoPruning, LazyExpansion>(SMALL),
        hash_scan_allocations::<SipHashStrategy, NoPruning, LazyExpansion>(LARGE),
    );
}

#[test]
fn hash_trie_sip_pruned_lazy_scan_allocates_independently_of_relation_size() {
    hash_scan_allocations::<SipHashStrategy, SingletonPruning, LazyExpansion>(SMALL);
    assert_flat(
        "HashTrie<Sip, Pruned, Lazy> scan",
        hash_scan_allocations::<SipHashStrategy, SingletonPruning, LazyExpansion>(SMALL),
        hash_scan_allocations::<SipHashStrategy, SingletonPruning, LazyExpansion>(LARGE),
    );
}

// ── Trie descents (issue #83) ───────────────────────────────────────────

/// The column orders admit only the descent `X`, `Y`, `Z`: `R` binds `X`
/// before `Y`, and `S` binds `Y` before `Z`.
const DESCENT_QUERY: &str = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(Z).";

/// Rows the descent workload produces, whatever its number of dead ends.
const DESCENT_ROWS: usize = 10;

/// Dead-end `X` keys for the few- and many-descent runs.
const FEW_DEAD_ENDS: usize = 10;
const MANY_DEAD_ENDS: usize = 10_000;

/// `X = 0` yields the `DESCENT_ROWS` rows `(0, 0, z)`. Each dead-end key
/// `x` in `1..=dead_ends` matches `R` and `S` on `Y = x`, then fails at
/// `Z`: `S(x, ·) = {DESCENT_ROWS}`, which `T` lacks. So every dead end
/// costs a successful descent, a failed one and an ascent, and adds no
/// row.
fn descent_relations(dead_ends: usize) -> Inputs {
    let r: Vec<Vec<usize>> = (0..=dead_ends).map(|x| vec![x, x]).collect();
    let s: Vec<Vec<usize>> = (0..DESCENT_ROWS)
        .map(|z| vec![0, z])
        .chain((1..=dead_ends).map(|x| vec![x, DESCENT_ROWS]))
        .collect();
    let t: Vec<Vec<usize>> = (0..DESCENT_ROWS).map(|z| vec![z]).collect();
    vec![
        relation("R", 2, r),
        relation("S", 2, s),
        relation("T", 1, t),
    ]
}

/// Allocations made by one streamed LFTJ join of the descent workload.
fn lftj_descent_allocations<Rel: TrieIterable + Relation>(dead_ends: usize) -> u64 {
    join_allocations::<Rel, LeapfrogTriejoin>(
        DESCENT_QUERY,
        descent_relations(dead_ends),
        DESCENT_ROWS,
    )
}

/// Allocations made by one streamed HashTriejoin join of the descent
/// workload over `HashTrie<H, P>`.
fn htj_descent_allocations<H: HashStrategy, P: PruningPolicy>(dead_ends: usize) -> u64 {
    join_allocations::<HashTrie<H, P>, HashTriejoin>(
        DESCENT_QUERY,
        descent_relations(dead_ends),
        DESCENT_ROWS,
    )
}

/// [`htj_descent_allocations`] for the lazy Layout, measured warm.
fn htj_lazy_descent_allocations<H: HashStrategy, P: PruningPolicy>(dead_ends: usize) -> u64 {
    warm_join_allocations::<HashTrie<H, P, LazyExpansion>, HashTriejoin>(
        DESCENT_QUERY,
        descent_relations(dead_ends),
        DESCENT_ROWS,
    )
}

fn assert_flat_in_descents(cell: &str, few: u64, many: u64) {
    assert_eq!(
        few, many,
        "{cell}: 1000x more dead-end descents changed the allocation count from {few} to {many}; \
         the join is allocating per descent"
    );
}

#[test]
fn tree_trie_lftj_allocates_independently_of_descent_count() {
    lftj_descent_allocations::<TreeTrie>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "TreeTrie/LFTJ",
        lftj_descent_allocations::<TreeTrie>(FEW_DEAD_ENDS),
        lftj_descent_allocations::<TreeTrie>(MANY_DEAD_ENDS),
    );
}

#[test]
fn column_trie_lftj_allocates_independently_of_descent_count() {
    lftj_descent_allocations::<ColumnTrie>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "ColumnTrie/LFTJ",
        lftj_descent_allocations::<ColumnTrie>(FEW_DEAD_ENDS),
        lftj_descent_allocations::<ColumnTrie>(MANY_DEAD_ENDS),
    );
}

#[test]
fn tree_trie_linear_lftj_allocates_independently_of_descent_count() {
    lftj_descent_allocations::<TreeTrie<LinearSeek>>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "TreeTrie<LinearSeek>/LFTJ",
        lftj_descent_allocations::<TreeTrie<LinearSeek>>(FEW_DEAD_ENDS),
        lftj_descent_allocations::<TreeTrie<LinearSeek>>(MANY_DEAD_ENDS),
    );
}

#[test]
fn tree_trie_binary_lftj_allocates_independently_of_descent_count() {
    lftj_descent_allocations::<TreeTrie<BinarySeek>>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "TreeTrie<BinarySeek>/LFTJ",
        lftj_descent_allocations::<TreeTrie<BinarySeek>>(FEW_DEAD_ENDS),
        lftj_descent_allocations::<TreeTrie<BinarySeek>>(MANY_DEAD_ENDS),
    );
}

#[test]
fn column_trie_linear_lftj_allocates_independently_of_descent_count() {
    lftj_descent_allocations::<ColumnTrie<LinearSeek>>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "ColumnTrie<LinearSeek>/LFTJ",
        lftj_descent_allocations::<ColumnTrie<LinearSeek>>(FEW_DEAD_ENDS),
        lftj_descent_allocations::<ColumnTrie<LinearSeek>>(MANY_DEAD_ENDS),
    );
}

#[test]
fn column_trie_binary_lftj_allocates_independently_of_descent_count() {
    lftj_descent_allocations::<ColumnTrie<BinarySeek>>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "ColumnTrie<BinarySeek>/LFTJ",
        lftj_descent_allocations::<ColumnTrie<BinarySeek>>(FEW_DEAD_ENDS),
        lftj_descent_allocations::<ColumnTrie<BinarySeek>>(MANY_DEAD_ENDS),
    );
}

#[test]
fn hash_trie_sip_allocates_independently_of_descent_count() {
    htj_descent_allocations::<SipHashStrategy, NoPruning>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "HashTrie<Sip>/HTJ",
        htj_descent_allocations::<SipHashStrategy, NoPruning>(FEW_DEAD_ENDS),
        htj_descent_allocations::<SipHashStrategy, NoPruning>(MANY_DEAD_ENDS),
    );
}

#[test]
fn hash_trie_fx_allocates_independently_of_descent_count() {
    htj_descent_allocations::<FxHashStrategy, NoPruning>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "HashTrie<Fx>/HTJ",
        htj_descent_allocations::<FxHashStrategy, NoPruning>(FEW_DEAD_ENDS),
        htj_descent_allocations::<FxHashStrategy, NoPruning>(MANY_DEAD_ENDS),
    );
}

#[test]
fn hash_trie_sip_pruned_allocates_independently_of_descent_count() {
    htj_descent_allocations::<SipHashStrategy, SingletonPruning>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "HashTrie<Sip, Pruned>/HTJ",
        htj_descent_allocations::<SipHashStrategy, SingletonPruning>(FEW_DEAD_ENDS),
        htj_descent_allocations::<SipHashStrategy, SingletonPruning>(MANY_DEAD_ENDS),
    );
}

#[test]
fn hash_trie_fx_pruned_allocates_independently_of_descent_count() {
    htj_descent_allocations::<FxHashStrategy, SingletonPruning>(FEW_DEAD_ENDS);
    assert_flat_in_descents(
        "HashTrie<Fx, Pruned>/HTJ",
        htj_descent_allocations::<FxHashStrategy, SingletonPruning>(FEW_DEAD_ENDS),
        htj_descent_allocations::<FxHashStrategy, SingletonPruning>(MANY_DEAD_ENDS),
    );
}

#[test]
fn hash_trie_sip_lazy_allocates_independently_of_descent_count() {
    assert_flat_in_descents(
        "HashTrie<Sip, Lazy>/HTJ",
        htj_lazy_descent_allocations::<SipHashStrategy, NoPruning>(FEW_DEAD_ENDS),
        htj_lazy_descent_allocations::<SipHashStrategy, NoPruning>(MANY_DEAD_ENDS),
    );
}

#[test]
fn hash_trie_fx_lazy_allocates_independently_of_descent_count() {
    assert_flat_in_descents(
        "HashTrie<Fx, Lazy>/HTJ",
        htj_lazy_descent_allocations::<FxHashStrategy, NoPruning>(FEW_DEAD_ENDS),
        htj_lazy_descent_allocations::<FxHashStrategy, NoPruning>(MANY_DEAD_ENDS),
    );
}

#[test]
fn hash_trie_sip_pruned_lazy_allocates_independently_of_descent_count() {
    assert_flat_in_descents(
        "HashTrie<Sip, Pruned, Lazy>/HTJ",
        htj_lazy_descent_allocations::<SipHashStrategy, SingletonPruning>(FEW_DEAD_ENDS),
        htj_lazy_descent_allocations::<SipHashStrategy, SingletonPruning>(MANY_DEAD_ENDS),
    );
}

#[test]
fn hash_trie_fx_pruned_lazy_allocates_independently_of_descent_count() {
    assert_flat_in_descents(
        "HashTrie<Fx, Pruned, Lazy>/HTJ",
        htj_lazy_descent_allocations::<FxHashStrategy, SingletonPruning>(FEW_DEAD_ENDS),
        htj_lazy_descent_allocations::<FxHashStrategy, SingletonPruning>(MANY_DEAD_ENDS),
    );
}
