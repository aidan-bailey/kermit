//! Every query shape under every variable order: whichever order the
//! planner picks, the indexes `prepare` names and `build` copies answer
//! the query.
//!
//! The shapes are the 16 standard join patterns' and a few constant cases,
//! run over seeded random relations (a small domain of 3 values and 8-12
//! tuples per relation, so duplicates are likely and joins dense), plus the
//! six queries of the retired
//! `column_orders_any.rs` with their pinned rows. Each query runs under
//! every order of its variables, forced by `FixedOrder`, through
//! `kermit::db::{prepare, build, execute}` directly, on one alias per
//! structure: `TreeTrie` and `ColumnTrie` under `LeapfrogTriejoin`,
//! `HashTrie` under `HashTriejoin`. Rows are compared with the
//! nested-loop oracle, which shares nothing with the executors.

mod common;

use {
    common::{
        oracle::{oracle, Lcg, Relations},
        orders::{every_order, FixedOrder},
    },
    kermit::db::{build, execute, prepare, Catalog},
    kermit_algos::{
        canonical_names, rewrite_placeholders, CatalogStats, HashTriejoin, JoinAlgo, JoinQuery,
        LeapfrogTriejoin, Planner, QueryOptimiser, StatisticsLevel,
    },
    kermit_ds::{
        Cardinality, ColumnTrie, HashTrie, LazyExpansion, NoPruning, Relation, RelationHeader,
        SingletonPruning, TreeTrie, Tuples,
    },
    kermit_iters::SipHashStrategy,
};

type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieSipLazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;

fn names(list: &[&str]) -> Vec<String> { list.iter().map(|s| (*s).to_string()).collect() }

#[test]
fn every_order_lists_each_permutation_once() {
    let variables = names(&["A", "B", "C"]);
    let orders = every_order(&variables);
    assert_eq!(orders.len(), 6);
    let mut distinct = orders.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 6, "an order was listed twice: {orders:?}");
    for order in &orders {
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, variables, "{order:?} is not a permutation");
    }
}

#[test]
fn every_order_of_no_variables_is_the_empty_order() {
    assert_eq!(every_order(&[]), vec![Vec::<String>::new()]);
}

#[test]
fn every_order_of_five_variables_is_120() {
    assert_eq!(every_order(&names(&["A", "B", "C", "D", "E"])).len(), 120);
}

#[test]
fn fixed_order_binds_the_listed_variables_first() {
    // Canonical order is the head's, X Y Z, so Z is 2 and X is 0.
    let query: JoinQuery = "Q(X, Y, Z) :- r(X, Y), s(Y, Z).".parse().unwrap();
    let plan = FixedOrder(names(&["Z", "X"])).plan(&query, &CatalogStats::default());
    assert_eq!(plan.variable_ordering, vec![2, 0, 1]);
}

#[test]
fn fixed_order_leaves_unlisted_variables_in_canonical_order() {
    // `K0` is a body-only variable, as a rewritten `_` is.
    let query: JoinQuery = "Q(X) :- r(X, K0).".parse().unwrap();
    let plan = FixedOrder(names(&["K0"])).plan(&query, &CatalogStats::default());
    assert_eq!(plan.variable_ordering, vec![1, 0]);
    let plan = FixedOrder(Vec::new()).plan(&query, &CatalogStats::default());
    assert_eq!(plan.variable_ordering, vec![0, 1]);
}

#[test]
#[should_panic(expected = "variable W is not in the query")]
fn fixed_order_panics_on_an_unknown_variable() {
    let query: JoinQuery = "Q(X) :- r(X).".parse().unwrap();
    FixedOrder(names(&["W"])).plan(&query, &CatalogStats::default());
}

#[test]
#[should_panic(expected = "variable X is listed twice")]
fn fixed_order_panics_on_a_repeated_variable() {
    let query: JoinQuery = "Q(X) :- r(X).".parse().unwrap();
    FixedOrder(names(&["X", "X"])).plan(&query, &CatalogStats::default());
}

/// A `_` is a plan variable once `prepare` has run, so the placeholder
/// shapes run every order of it too: `V0, K0` is 2 orders, `V0, V2, K0` is 6.
#[test]
fn a_placeholder_is_ordered_like_a_variable() {
    let orders = |text: &str| {
        let query: JoinQuery = text.parse().unwrap();
        every_order(&canonical_names(&rewrite_placeholders(query))).len()
    };
    assert_eq!(orders("Q(V0) :- r0(V0, _), r1(V0)."), 2);
    assert_eq!(orders("Q(V0, V2) :- r0(V0, _, V2), r1(V2)."), 6);
}

/// The values in the random relations, and so the constants `c0`..`c2` the
/// shapes use.
const DOMAIN: usize = 3;

/// Seeds per shape. Each seed gives the shape's relations other tuples,
/// duplicates included.
const SEEDS: std::ops::Range<u64> = 0..6;

/// A query over relations `r0`, `r1`, …, with the arity of each.
struct Shape {
    name: &'static str,
    query: &'static str,
    arities: &'static [usize],
}

fn shape(name: &'static str, query: &'static str, arities: &'static [usize]) -> Shape {
    Shape {
        name,
        query,
        arities,
    }
}

/// The 16 standard patterns' shapes (those whose fixtures differ only in
/// data share a shape and keep their names), then the constant cases.
fn shapes() -> Vec<Shape> {
    vec![
        shape("unary", "Q(V0) :- r0(V0).", &[1]),
        shape(
            "triangle",
            "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2), r2(V0, V2).",
            &[2, 2, 2],
        ),
        shape(
            "chain",
            "Q(V0, V1, V2, V3) :- r0(V0, V1), r1(V1, V2), r2(V2, V3).",
            &[2, 2, 2],
        ),
        shape("star", "Q(V0, V1, V2) :- r0(V0, V1), r1(V0, V2).", &[2, 2]),
        shape("selfjoin", "Q(V0, V1, V2) :- r0(V0, V1), r0(V1, V2).", &[2]),
        shape("existential", "Q(V0) :- r0(V0), r1(V0).", &[1, 1]),
        shape(
            "empty-result",
            "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2).",
            &[2, 2],
        ),
        shape("single-relation", "Q(V0, V1) :- r0(V0, V1).", &[2]),
        shape(
            "four-way-chain",
            "Q(V0, V1, V2, V3, V4) :- r0(V0, V1), r1(V1, V2), r2(V2, V3), r3(V3, V4).",
            &[2, 2, 2, 2],
        ),
        shape(
            "wide-fanout",
            "Q(V0, V1, V2) :- r0(V0, V1), r1(V0, V2).",
            &[2, 2],
        ),
        shape("dead-end", "Q(V0, V1, V2) :- r0(V0, V1), r1(V1, V2).", &[
            2, 2,
        ]),
        shape(
            "deep-dead-end",
            "Q(V0, V1, V2, V3) :- r0(V0, V1), r1(V1, V2), r2(V2, V3).",
            &[2, 2, 2],
        ),
        shape("diagonal", "Q(V0) :- r0(V0, V0).", &[2]),
        shape(
            "repeated-nonadjacent",
            "Q(V0, V1) :- r0(V0, V1, V0), r1(V1).",
            &[3, 1],
        ),
        shape("trailing-placeholder", "Q(V0) :- r0(V0, _), r1(V0).", &[
            2, 1,
        ]),
        shape(
            "middle-placeholder",
            "Q(V0, V2) :- r0(V0, _, V2), r1(V2).",
            &[3, 1],
        ),
        shape("constant-object", "Q(X) :- r0(X, c1).", &[2]),
        shape("constant-with-repeat", "Q(X) :- r0(X, X, c1).", &[3]),
        shape(
            "constant-in-a-join",
            "Q(X, Z) :- r0(X, Y), r1(Y, c2), r2(Y, Z).",
            &[2, 2, 2],
        ),
        shape("constant-subject", "Q(Y) :- r0(c1, Y), r1(Y).", &[2, 1]),
        shape("atom-of-only-constants", "Q(X) :- r0(X), r1(c0, c1).", &[
            1, 2,
        ]),
        shape(
            "mixed-constants-and-repeats",
            "Q(X, Z) :- r0(X, c1, Y), r1(Y, Z, Z), r2(c0, c1).",
            &[3, 3, 2],
        ),
    ]
}

/// One relation per arity, `r0`, `r1`, …, each with 8–12 tuples drawn from
/// `0..DOMAIN`, so duplicates are likely and joins are dense.
fn random_data(rng: &mut Lcg, arities: &[usize]) -> Relations {
    arities
        .iter()
        .enumerate()
        .map(|(i, &arity)| {
            let tuples = (0..8 + rng.below(5))
                .map(|_| (0..arity).map(|_| rng.below(DOMAIN)).collect())
                .collect();
            (format!("r{i}"), arity, tuples)
        })
        .collect()
}

/// `relations` as a catalog at the tuple-count level, which is all
/// `FixedOrder` reads. Each relation is built row by row, so an empty one
/// still has its arity.
fn catalog_for(relations: &Relations) -> Catalog {
    Catalog::new(
        relations
            .iter()
            .map(|(name, arity, tuples)| {
                let mut flat = Tuples::new(*arity);
                for tuple in tuples {
                    flat.push(tuple);
                }
                (RelationHeader::new_positional(name.as_str(), *arity), flat)
            })
            .collect(),
        StatisticsLevel::TupleCounts,
    )
}

/// The sorted rows of `query` over `catalog` as `R` through `JA`, planned
/// in `order`: `prepare`, `build`, `execute`. Asserts that the prepared
/// plan binds the forced variables first, so a planner that stopped
/// honouring `order` fails here instead of running one order many times.
fn rows_in_order<R: Relation, JA: JoinAlgo<R>>(
    catalog: &Catalog, label: &str, query: &JoinQuery, order: &[String],
) -> Vec<Vec<usize>> {
    let planner = Planner::new(Box::new(FixedOrder(order.to_vec())));
    let prepared = prepare(catalog, query, &planner)
        .unwrap_or_else(|e| panic!("{label} in order {order:?}: {e}"));
    let names = canonical_names(prepared.query());
    let bound: Vec<String> = prepared
        .plan()
        .variable_ordering
        .iter()
        .map(|&variable| names[variable].clone())
        .collect();
    assert_eq!(
        &bound[..order.len()],
        order,
        "{label}: the plan does not bind the forced variables first"
    );
    let indexes = build::<R>(catalog, &prepared);
    let mut rows = Tuples::new(prepared.head_len());
    execute::<R, JA>(&prepared, &indexes, |row| rows.push(row));
    rows.sort();
    rows.to_vecs()
}

/// Runs `text` over `relations` under every order of its variables, each
/// `_` counted as the plan variable `prepare` makes of it, and asserts each
/// answer equals the oracle's (and `pinned`, when given).
/// Returns whether the answer has any rows.
fn agrees_in_every_order<R: Relation + Cardinality, JA: JoinAlgo<R>>(
    relations: &Relations, case: &str, text: &str, pinned: Option<&[Vec<usize>]>,
) -> bool {
    let label = format!("{case}: {text}");
    let query: JoinQuery = text
        .parse()
        .unwrap_or_else(|e| panic!("{label}: unparsable: {e}"));
    let want = oracle::<R>(relations, &query);
    if let Some(pinned) = pinned {
        assert_eq!(
            want, pinned,
            "{label}: the oracle disagrees with the pinned rows"
        );
    }
    let catalog = catalog_for(relations);
    // `prepare` turns each `_` into a plan variable `K<n>`, so the orders
    // are listed from the rewritten query, with the same fresh names.
    let variables = canonical_names(&rewrite_placeholders(query.clone()));
    for order in every_order(&variables) {
        let got = rows_in_order::<R, JA>(&catalog, &label, &query, &order);
        assert_eq!(got, want, "{label} in order {order:?}");
    }
    !want.is_empty()
}

/// Every shape over `SEEDS` random relations. The final bound catches a
/// generator that stops producing queries with answers, which would turn
/// every comparison into `[] == []`.
fn run_shapes<R: Relation + Cardinality, JA: JoinAlgo<R>>() {
    let (mut with_rows, mut total) = (0, 0);
    for (index, shape) in shapes().iter().enumerate() {
        for seed in SEEDS {
            let mut rng = Lcg(1000 * index as u64 + seed);
            let relations = random_data(&mut rng, shape.arities);
            let case = format!("{} seed {seed}", shape.name);
            total += 1;
            with_rows += usize::from(agrees_in_every_order::<R, JA>(
                &relations,
                &case,
                shape.query,
                None,
            ));
        }
    }
    assert!(
        with_rows * 4 >= total,
        "only {with_rows} of {total} queries returned a row"
    );
}

/// A pinned query: its relations, its text and its sorted rows.
type Pinned = (Relations, &'static str, Vec<Vec<usize>>);

fn rel(name: &str, arity: usize, tuples: Vec<Vec<usize>>) -> (String, usize, Vec<Vec<usize>>) {
    (name.to_string(), arity, tuples)
}

/// The six queries of `column_orders_any.rs`, which `--column-orders
/// stored` rejected or read only as stored, with the rows each returns.
fn pinned() -> Vec<Pinned> {
    // A symmetric relation (every edge stored in both directions, as
    // SNAP's `edge` is) under the 4-cycle: every closed walk of length
    // four (bag semantics).
    let undirected = [(1, 2), (2, 3), (3, 4), (4, 1), (1, 3)];
    let edges: Vec<Vec<usize>> = undirected
        .iter()
        .flat_map(|&(a, b)| [vec![a, b], vec![b, a]])
        .collect();
    let has = |a: usize, b: usize| edges.contains(&vec![a, b]);
    let mut cycle_rows: Vec<Vec<usize>> = Vec::new();
    for ab in &edges {
        for bc in edges.iter().filter(|e| e[0] == ab[1]) {
            for cd in edges.iter().filter(|e| e[0] == bc[1]) {
                if has(cd[1], ab[0]) {
                    cycle_rows.push(vec![ab[0], ab[1], bc[1], cd[1]]);
                }
            }
        }
    }
    cycle_rows.sort();

    vec![
        // Two atoms over one relation, each the other reversed.
        (
            vec![rel("edge", 2, vec![
                vec![1, 2],
                vec![2, 1],
                vec![2, 3],
                vec![3, 2],
                vec![1, 3],
            ])],
            "Q(X, Y) :- edge(X, Y), edge(Y, X).",
            vec![vec![1, 2], vec![2, 1], vec![2, 3], vec![3, 2]],
        ),
        // #82's incoming star: the head puts `V0` first, so both atoms
        // are read reversed.
        (
            vec![
                rel("includes", 2, vec![vec![10, 1], vec![11, 1], vec![12, 2]]),
                rel("purchasefor", 2, vec![vec![20, 1], vec![21, 2], vec![
                    22, 3,
                ]]),
            ],
            "Q(V0, V2, V7) :- includes(V2, V0), purchasefor(V7, V0).",
            vec![vec![1, 10, 20], vec![1, 11, 20], vec![2, 12, 21]],
        ),
        (
            vec![
                rel("r", 3, vec![vec![1, 2, 3], vec![4, 5, 6], vec![1, 7, 9]]),
                rel("s", 2, vec![vec![3, 1], vec![6, 4], vec![9, 2]]),
            ],
            "Q(X, Y, Z) :- r(X, Y, Z), s(Z, X).",
            vec![vec![1, 2, 3], vec![4, 5, 6]],
        ),
        // A repeated variable on an atom read in another column order.
        (
            vec![
                rel("r", 3, vec![
                    vec![1, 1, 5],
                    vec![1, 2, 5],
                    vec![2, 2, 6],
                    vec![3, 3, 7],
                ]),
                rel("s", 2, vec![vec![5, 1], vec![6, 2], vec![7, 9]]),
            ],
            "Q(X, Y) :- r(X, X, Y), s(Y, X).",
            vec![vec![1, 5], vec![2, 6]],
        ),
        // A constant on an atom read in another column order.
        (
            vec![
                rel("r", 2, vec![vec![1, 2], vec![3, 4], vec![5, 6]]),
                rel("s", 3, vec![vec![2, 7, 1], vec![4, 8, 3], vec![6, 7, 9]]),
            ],
            "Q(X, Y) :- r(X, Y), s(Y, c7, X).",
            vec![vec![1, 2]],
        ),
        (
            vec![rel("edge", 2, edges.clone())],
            "Q(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D), edge(D, A).",
            cycle_rows,
        ),
    ]
}

/// The pinned queries, each of which has rows (the loop would otherwise
/// compare empty with empty).
fn run_pinned<R: Relation + Cardinality, JA: JoinAlgo<R>>() {
    for (relations, text, rows) in pinned() {
        assert!(
            agrees_in_every_order::<R, JA>(&relations, "pinned", text, Some(&rows)),
            "{text}: a pinned query must return rows"
        );
    }
}

/// An atom of only constants matching a tuple held three times: a multiset
/// (`HashTrie`) returns each row of the rest of the query three times, a
/// set (the sorted tries) once. The random shapes' `r1(c0, c1)` matches at
/// most one row, so this is the only query that reaches the multiplicity.
fn run_duplicated_constant_atom<R: Relation + Cardinality, JA: JoinAlgo<R>>(copies: usize) {
    let relations: Relations = vec![
        rel("r0", 1, vec![vec![1], vec![2]]),
        rel("r1", 2, vec![vec![0, 1], vec![0, 1], vec![0, 1], vec![
            0, 2,
        ]]),
    ];
    let rows: Vec<Vec<usize>> = [1, 2]
        .iter()
        .flat_map(|&x| std::iter::repeat_n(vec![x], copies))
        .collect();
    assert!(
        agrees_in_every_order::<R, JA>(
            &relations,
            "duplicated constant atom",
            "Q(X) :- r0(X), r1(c0, c1).",
            Some(&rows)
        ),
        "the duplicated-constant query must return rows"
    );
}

#[test]
fn tree_trie_duplicated_constant_atom_under_every_order() {
    run_duplicated_constant_atom::<TreeTrie, LeapfrogTriejoin>(1);
}

#[test]
fn column_trie_duplicated_constant_atom_under_every_order() {
    run_duplicated_constant_atom::<ColumnTrie, LeapfrogTriejoin>(1);
}

#[test]
fn hash_trie_duplicated_constant_atom_under_every_order() {
    run_duplicated_constant_atom::<HashTrie<SipHashStrategy>, HashTriejoin>(3);
}

#[test]
fn pruned_hash_trie_duplicated_constant_atom_under_every_order() {
    run_duplicated_constant_atom::<HashTrieSipPruned, HashTriejoin>(3);
}

#[test]
fn lazy_hash_trie_duplicated_constant_atom_under_every_order() {
    run_duplicated_constant_atom::<HashTrieSipLazy, HashTriejoin>(3);
}

#[test]
fn tree_trie_shapes_under_every_order() { run_shapes::<TreeTrie, LeapfrogTriejoin>(); }

#[test]
fn column_trie_shapes_under_every_order() { run_shapes::<ColumnTrie, LeapfrogTriejoin>(); }

#[test]
fn hash_trie_shapes_under_every_order() { run_shapes::<HashTrie<SipHashStrategy>, HashTriejoin>(); }

#[test]
fn tree_trie_pinned_queries_under_every_order() { run_pinned::<TreeTrie, LeapfrogTriejoin>(); }

#[test]
fn column_trie_pinned_queries_under_every_order() { run_pinned::<ColumnTrie, LeapfrogTriejoin>(); }

#[test]
fn hash_trie_pinned_queries_under_every_order() {
    run_pinned::<HashTrie<SipHashStrategy>, HashTriejoin>();
}

#[test]
fn pruned_hash_trie_pinned_queries_under_every_order() {
    run_pinned::<HashTrieSipPruned, HashTriejoin>();
}

#[test]
fn lazy_hash_trie_pinned_queries_under_every_order() {
    run_pinned::<HashTrieSipLazy, HashTriejoin>();
}

/// The forced order reaches the index: `r0(X, Y)` read `X` then `Y` is
/// built over columns `[0, 1]`, and read `Y` then `X` over `[1, 0]`.
#[test]
fn a_forced_order_reverses_the_index_columns() {
    let relations: Relations = vec![rel("r0", 2, vec![vec![0, 1], vec![1, 2]])];
    let catalog = catalog_for(&relations);
    let query: JoinQuery = "Q(X, Y) :- r0(X, Y).".parse().unwrap();
    let columns = |order: [&str; 2]| {
        let planner = Planner::new(Box::new(FixedOrder(
            order.iter().map(|v| (*v).to_string()).collect(),
        )));
        let prepared = prepare(&catalog, &query, &planner).unwrap();
        prepared
            .indexes()
            .iter()
            .map(|spec| spec.columns.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(columns(["X", "Y"]), vec![vec![0, 1]]);
    assert_eq!(columns(["Y", "X"]), vec![vec![1, 0]]);
}
