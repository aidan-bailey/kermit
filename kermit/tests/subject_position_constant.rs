//! Regression test for subject-position constants in the const-view rewrite
//! path, covering **both** join algorithms.
//!
//! A constant in a binary predicate's *first* (subject) attribute — e.g.
//! `Q(X) :- edge(c1, X).` ("successors of node 1") — must return the same
//! answer as the symmetric object-position query. This exercises
//! `rewrite_atoms` → optimiser planning → join end-to-end via the real
//! engine wiring (`lftj_join` for LFTJ, `hash_join` for the hash family),
//! where the global attribute order is *derived* from the query. The
//! macro-generated join suites now run through the same entry points, but
//! none of their patterns puts a constant in the subject position, so this
//! test remains the one that pins that shape.
//!
//! Root cause this guards against: the triejoin descends each relation one
//! physical column per depth, so the global variable order must bind every
//! relation's variables in physical column order. A subject-position constant
//! introduces a fresh variable that is physically first but appears late,
//! inverting that order and silently yielding 0 results until the optimiser's
//! `topological_order` enforces a valid descent order.
//!
//! Each case runs under every optimiser: the shape exists to guard descent
//! order, and each optimiser orders it differently.

use {
    clap::ValueEnum,
    kermit::db::{hash_join, lftj_join, Database, HashFamily, SortedFamily},
    kermit_algos::{JoinQuery, LeapfrogTriejoin, Optimiser},
    kermit_ds::{HashTrie, Relation, TreeTrie},
    kermit_iters::SipHashStrategy,
    std::collections::BTreeMap,
};

type HashTrieSip = HashTrie<SipHashStrategy>;

const EDGES: [[usize; 2]; 3] = [[1, 2], [1, 3], [2, 4]];

fn edges() -> Vec<Vec<usize>> { EDGES.iter().map(|e| e.to_vec()).collect() }

/// `query` over edge = {(1,2), (1,3), (2,4)} as a `TreeTrie` (LFTJ path),
/// planned by `optimiser`.
fn lftj(query: &str, optimiser: Optimiser) -> Vec<Vec<usize>> {
    let planner = optimiser.instantiate();
    let edge: TreeTrie = TreeTrie::from_tuples(2.into(), edges());
    let database = Database::new::<SortedFamily>(
        BTreeMap::from([("edge".to_string(), edge)]),
        planner.required_statistics(),
    );
    let q: JoinQuery = query.parse().expect("parse");
    lftj_join::<TreeTrie, LeapfrogTriejoin>(&database, q, planner.as_ref()).unwrap()
}

/// The same over a `HashTrie` (hash path).
fn hash(query: &str, optimiser: Optimiser) -> Vec<Vec<usize>> {
    let planner = optimiser.instantiate();
    let edge = HashTrieSip::from_tuples(2.into(), edges());
    let database = Database::new::<HashFamily<SipHashStrategy>>(
        BTreeMap::from([("edge".to_string(), edge)]),
        planner.required_statistics(),
    );
    let q: JoinQuery = query.parse().expect("parse");
    hash_join::<HashTrieSip, SipHashStrategy>(&database, q, planner.as_ref()).unwrap()
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
    for &optimiser in Optimiser::value_variants() {
        let got = head_col(lftj("Q(X) :- edge(c1, X).", optimiser));
        assert_eq!(got, vec![2, 3], "{optimiser:?}");
    }
}

#[test]
fn subject_position_constant_hash() {
    for &optimiser in Optimiser::value_variants() {
        let got = head_col(hash("Q(X) :- edge(c1, X).", optimiser));
        assert_eq!(got, vec![2, 3], "{optimiser:?}");
    }
}

// Q(X) :- edge(X, c4).  — predecessors of node 4 are {2}. Control case:
// object-position constants already worked; pin that they still do.
#[test]
fn object_position_constant_lftj() {
    for &optimiser in Optimiser::value_variants() {
        let got = head_col(lftj("Q(X) :- edge(X, c4).", optimiser));
        assert_eq!(got, vec![2], "{optimiser:?}");
    }
}

#[test]
fn object_position_constant_hash() {
    for &optimiser in Optimiser::value_variants() {
        let got = head_col(hash("Q(X) :- edge(X, c4).", optimiser));
        assert_eq!(got, vec![2], "{optimiser:?}");
    }
}
