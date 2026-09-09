//! Regression test for subject-position constants in the const-view rewrite
//! path, covering **both** join algorithms.
//!
//! A constant in a binary predicate's *first* (subject) attribute — e.g.
//! `Q(X) :- edge(c1, X).` ("successors of node 1") — must return the same
//! answer as the symmetric object-position query. This exercises
//! `rewrite_atoms` → optimiser planning → join end-to-end via the real
//! engine wiring (`lftj_join` for LFTJ, `hash_join` for the hash family), the
//! only paths where the global attribute order is *derived* from the query —
//! the macro-generated join suites supply a valid order by hand and so never
//! cover this.
//!
//! Root cause this guards against: the triejoin descends each relation one
//! physical column per depth, so the global variable order must bind every
//! relation's variables in physical column order. A subject-position constant
//! introduces a fresh variable that is physically first but appears late,
//! inverting that order and silently yielding 0 results until the optimiser's
//! `topological_order` enforces a valid descent order.

use {
    kermit::db::{hash_join, lftj_join},
    kermit_algos::{JoinQuery, LeapfrogTriejoin, LexicographicOptimiser},
    kermit_ds::{HashTrie, Relation, TreeTrie},
    kermit_iters::SipHashStrategy,
    std::collections::BTreeMap,
};

type HashTrieSip = HashTrie<SipHashStrategy>;

/// edge = {(1,2), (1,3), (2,4)} as a `TreeTrie` relation map (LFTJ path).
fn edge_tries() -> BTreeMap<String, TreeTrie> {
    let edge = TreeTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
    BTreeMap::from([("edge".to_string(), edge)])
}

/// edge = {(1,2), (1,3), (2,4)} as a `HashTrie` relation map (hash path).
fn edge_rels() -> BTreeMap<String, HashTrieSip> {
    let edge = HashTrieSip::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]);
    BTreeMap::from([("edge".to_string(), edge)])
}

fn lftj(query: &str) -> Vec<Vec<usize>> {
    let q: JoinQuery = query.parse().expect("parse");
    lftj_join::<TreeTrie, LeapfrogTriejoin>(&edge_tries(), q, &LexicographicOptimiser)
}

fn hash(query: &str) -> Vec<Vec<usize>> {
    let q: JoinQuery = query.parse().expect("parse");
    hash_join::<HashTrieSip, SipHashStrategy>(&edge_rels(), q, &LexicographicOptimiser)
}

/// First column (the head variable `X`) of every result tuple, sorted.
fn head_col(mut rows: Vec<Vec<usize>>) -> Vec<usize> {
    let mut out: Vec<usize> = rows.iter_mut().map(|r| r[0]).collect();
    out.sort_unstable();
    out
}

// Q(X) :- edge(c1, X).  — successors of node 1 are {2, 3}.
#[test]
fn subject_position_constant_lftj() {
    assert_eq!(head_col(lftj("Q(X) :- edge(c1, X).")), vec![2, 3]);
}

#[test]
fn subject_position_constant_hash() {
    assert_eq!(head_col(hash("Q(X) :- edge(c1, X).")), vec![2, 3]);
}

// Q(X) :- edge(X, c4).  — predecessors of node 4 are {2}. Control case:
// object-position constants already worked; pin that they still do.
#[test]
fn object_position_constant_lftj() {
    assert_eq!(head_col(lftj("Q(X) :- edge(X, c4).")), vec![2]);
}

#[test]
fn object_position_constant_hash() {
    assert_eq!(head_col(hash("Q(X) :- edge(X, c4).")), vec![2]);
}
