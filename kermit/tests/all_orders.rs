//! Every query shape under every variable order (the per-query index
//! build's central claim). Task 13.3 adds the join tests below the
//! helpers' own tests.

mod common;

use {
    common::orders::{every_order, FixedOrder},
    kermit_algos::{CatalogStats, JoinQuery, QueryOptimiser},
};

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
