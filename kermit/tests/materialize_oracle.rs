//! `materialize` on kermit's own cells against the nested-loop oracle. For
//! each structure under its algorithm, each shape of `common::shapes` runs
//! over seeded random relations under every optimiser and a forced reversed
//! order, through `kermit::db::{prepare, build, execute_assignments,
//! materialize_count}`:
//! - the assignments `execute_assignments` lends equal, as they come, the
//!   oracle's rows for the full query (`full_query(query, &prepared)`), bag
//!   semantics: its head is `canonical_names(prepared.query())`, the order the
//!   assignments' columns are in;
//! - `materialize_count` returns those rows sorted and counts the distinct
//!   ones, which are the same set under set and bag semantics.
//!
//! Contents are compared, not only counts: on fixtures this small a wrong
//! join condition can still produce the right count.

mod common;

use {
    clap::ValueEnum,
    common::{
        oracle::{oracle, Lcg, Relations},
        orders::FixedOrder,
        shapes::{catalog_at, full_query, random_data, shapes, SEEDS},
    },
    kermit::db::{build, execute_assignments, materialize_count, prepare, Catalog},
    kermit_algos::{
        canonical_names, rewrite_placeholders, HashTriejoin, JoinAlgo, JoinQuery, LeapfrogTriejoin,
        Optimiser, Planner, StatisticsLevel,
    },
    kermit_ds::{Cardinality, ColumnTrie, HashTrie, Relation, TreeTrie, Tuples},
    kermit_iters::SipHashStrategy,
};

/// Every optimiser, then the reversed canonical order of the rewritten
/// query's variables, which puts the body-only variables of a `_` or a narrow
/// head before the head's in some plans.
fn planners(query: &JoinQuery) -> Vec<(String, Planner)> {
    let mut planners: Vec<(String, Planner)> = Optimiser::value_variants()
        .iter()
        .map(|optimiser| {
            (
                optimiser.axis_value().to_string(),
                Planner::new(optimiser.instantiate()),
            )
        })
        .collect();
    let reversed: Vec<String> = canonical_names(&rewrite_placeholders(query.clone()))
        .into_iter()
        .rev()
        .collect();
    planners.push((
        "reversed".to_string(),
        Planner::new(Box::new(FixedOrder(reversed))),
    ));
    planners
}

/// What one case showed, for the cell's bounds.
struct Facts {
    /// The assignments were wider than the head.
    wider: bool,
    /// Rows in the oracle's bag for the full query.
    bag_len: usize,
    /// Distinct rows among them.
    distinct_len: usize,
}

/// One query over one catalog, planned by `planner`, as `R` through `JA`.
/// The oracle runs `full_query(query, &prepared)`, whose head is the
/// executed query's canonical order, so its rows compare with the
/// assignments as they come.
fn check_case<R: Relation + Cardinality, JA: JoinAlgo<R>>(
    relations: &Relations, catalog: &Catalog, query: &JoinQuery, planner: &Planner, case: &str,
) -> Facts {
    let prepared =
        prepare(catalog, query, planner, R::TRIE_LEVELS).unwrap_or_else(|e| panic!("{case}: {e}"));
    let full = full_query(query, &prepared);
    let bag = oracle::<R>(relations, &full);
    let mut distinct = bag.clone();
    distinct.dedup();

    let indexes = build::<R>(catalog, &prepared);
    let width = prepared.assignment_width();
    assert_eq!(
        width,
        canonical_names(&full).len(),
        "{case}: the assignment width"
    );
    let mut rows = Tuples::new(width);
    execute_assignments::<R, JA>(&prepared, &indexes, |row| rows.push(row));

    let executed = rows.to_vecs();
    let mut got = executed.clone();
    got.sort();
    assert_eq!(got, bag, "{case}: execute_assignments");

    let (sorted, count) = materialize_count(rows);
    let sorted = sorted.to_vecs();
    assert!(
        sorted.windows(2).all(|pair| pair[0] <= pair[1]),
        "{case}: materialize_count did not sort"
    );
    assert_eq!(sorted, got, "{case}: materialize_count changed the rows");
    assert_eq!(count, distinct.len() as u64, "{case}: the distinct count");
    Facts {
        wider: width > prepared.head_len(),
        bag_len: bag.len(),
        distinct_len: distinct.len(),
    }
}

/// Every shape over `SEEDS` random relations, each under every planner.
/// Returns how many cases held a duplicate full assignment in the oracle's
/// bag. The final bounds catch a generator that stops producing answers
/// (every comparison would be `[] == []`) and a suite whose assignments are
/// never wider than the head.
fn check_cell<R: Relation + Cardinality, JA: JoinAlgo<R>>() -> usize {
    let (mut total, mut with_rows, mut wider, mut duplicated) = (0, 0, 0, 0);
    for (index, shape) in shapes().iter().enumerate() {
        let query: JoinQuery = shape.query.parse().expect("a shape parses");
        for seed in SEEDS {
            let relations = random_data(&mut Lcg(1000 * index as u64 + seed), shape.arities);
            let catalog = catalog_at(&relations, StatisticsLevel::ColumnDistinct);
            for (label, planner) in planners(&query) {
                let case = format!("{} seed {seed} under {label}", shape.name);
                let facts = check_case::<R, JA>(&relations, &catalog, &query, &planner, &case);
                total += 1;
                with_rows += usize::from(facts.bag_len > 0);
                wider += usize::from(facts.wider);
                duplicated += usize::from(facts.distinct_len < facts.bag_len);
            }
        }
    }
    assert!(
        with_rows * 4 >= total,
        "only {with_rows} of {total} cases returned a row"
    );
    assert!(
        wider > 0,
        "no case had a body-only variable, so the head was never narrower than the assignment"
    );
    duplicated
}

/// An atom of only constants matching a tuple held `copies` times (once for a
/// set, three times for the hash trie's bag): the bag holds `2 × copies`
/// assignments, the distinct count is 2, so a count that forgot to
/// deduplicate (or deduplicated the wrong thing) fails here. The random
/// shapes' `r1(c0, c1)` matches at most one row.
fn check_duplicated_constant_atom<R: Relation + Cardinality, JA: JoinAlgo<R>>(copies: usize) {
    let relations: Relations = vec![
        ("r0".to_string(), 1, vec![vec![1], vec![2]]),
        ("r1".to_string(), 2, vec![
            vec![0, 1],
            vec![0, 1],
            vec![0, 1],
            vec![0, 2],
        ]),
    ];
    let query: JoinQuery = "Q(X) :- r0(X), r1(c0, c1).".parse().unwrap();
    let catalog = catalog_at(&relations, StatisticsLevel::ColumnDistinct);
    for (label, planner) in planners(&query) {
        let case = format!("duplicated constant atom under {label}");
        let facts = check_case::<R, JA>(&relations, &catalog, &query, &planner, &case);
        assert_eq!(facts.bag_len, 2 * copies, "{case}: the oracle's bag");
        assert_eq!(facts.distinct_len, 2, "{case}: the distinct rows");
    }
}

#[test]
fn tree_trie_materializes_the_oracles_assignments() {
    assert_eq!(
        check_cell::<TreeTrie, LeapfrogTriejoin>(),
        0,
        "a set holds no duplicate full assignment"
    );
}

#[test]
fn column_trie_materializes_the_oracles_assignments() {
    assert_eq!(
        check_cell::<ColumnTrie, LeapfrogTriejoin>(),
        0,
        "a set holds no duplicate full assignment"
    );
}

#[test]
fn hash_trie_materializes_the_oracles_assignments() {
    assert!(
        check_cell::<HashTrie<SipHashStrategy>, HashTriejoin>() > 0,
        "the random relations hold duplicate rows, which a bag answers twice"
    );
}

#[test]
fn tree_trie_counts_a_duplicated_constant_atom_once() {
    check_duplicated_constant_atom::<TreeTrie, LeapfrogTriejoin>(1);
}

#[test]
fn column_trie_counts_a_duplicated_constant_atom_once() {
    check_duplicated_constant_atom::<ColumnTrie, LeapfrogTriejoin>(1);
}

#[test]
fn hash_trie_counts_a_duplicated_constant_atom_once_after_deduplicating() {
    check_duplicated_constant_atom::<HashTrie<SipHashStrategy>, HashTriejoin>(3);
}
