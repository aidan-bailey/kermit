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
//! Each case runs under every optimiser and column-order policy: the shape
//! exists to guard descent order, and each optimiser orders it differently.
//! Under `--column-orders any` an optimiser that binds `X` before the
//! constant's fresh variable reads `edge` through a reordered copy instead
//! of being forced into the stored order.

mod common;

use {
    clap::ValueEnum,
    common::utils::{join_under_planner, Inputs, JoinEntry},
    kermit_algos::{
        ColumnOrderPolicy, HashTriejoin, JoinQuery, LeapfrogTriejoin, Optimiser, Planner,
    },
    kermit_ds::{Cardinality, HashTrie, Relation, RelationHeader, TreeTrie},
    kermit_iters::SipHashStrategy,
    std::collections::BTreeMap,
};

type HashTrieSip = HashTrie<SipHashStrategy>;

const EDGES: [[usize; 2]; 3] = [[1, 2], [1, 3], [2, 4]];

fn edges() -> Vec<Vec<usize>> { EDGES.iter().map(|e| e.to_vec()).collect() }

/// `query` over edge = {(1,2), (1,3), (2,4)} as `R` through `JA`, planned
/// by `optimiser` under `policy`.
fn join<R: Relation + Cardinality, JA: JoinEntry<R>>(
    query: &str, optimiser: Optimiser, policy: ColumnOrderPolicy,
) -> Vec<Vec<usize>> {
    let planner = Planner::new(optimiser.instantiate(), policy);
    let header = RelationHeader::new_positional("edge", 2);
    let inputs: Inputs = BTreeMap::from([("edge".to_string(), (header.clone(), edges()))]);
    let store = BTreeMap::from([("edge".to_string(), R::from_tuples(header, edges()))]);
    let mut database = JA::database(store, planner.required_statistics());
    let q: JoinQuery = query.parse().expect("parse");
    let (rows, _) = join_under_planner::<R, JA>(&mut database, &inputs, q, &planner).unwrap();
    rows
}

/// `query` as a `TreeTrie` (LFTJ path).
fn lftj(query: &str, optimiser: Optimiser, policy: ColumnOrderPolicy) -> Vec<Vec<usize>> {
    join::<TreeTrie, LeapfrogTriejoin>(query, optimiser, policy)
}

/// The same over a `HashTrie` (hash path).
fn hash(query: &str, optimiser: Optimiser, policy: ColumnOrderPolicy) -> Vec<Vec<usize>> {
    join::<HashTrieSip, HashTriejoin>(query, optimiser, policy)
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
    for &policy in ColumnOrderPolicy::value_variants() {
        for &optimiser in Optimiser::value_variants() {
            let got = head_col(lftj("Q(X) :- edge(c1, X).", optimiser, policy));
            assert_eq!(got, vec![2, 3], "{optimiser:?} / {policy:?}");
        }
    }
}

#[test]
fn subject_position_constant_hash() {
    for &policy in ColumnOrderPolicy::value_variants() {
        for &optimiser in Optimiser::value_variants() {
            let got = head_col(hash("Q(X) :- edge(c1, X).", optimiser, policy));
            assert_eq!(got, vec![2, 3], "{optimiser:?} / {policy:?}");
        }
    }
}

// Q(X) :- edge(X, c4).  — predecessors of node 4 are {2}. Control case:
// object-position constants already worked; pin that they still do.
#[test]
fn object_position_constant_lftj() {
    for &policy in ColumnOrderPolicy::value_variants() {
        for &optimiser in Optimiser::value_variants() {
            let got = head_col(lftj("Q(X) :- edge(X, c4).", optimiser, policy));
            assert_eq!(got, vec![2], "{optimiser:?} / {policy:?}");
        }
    }
}

#[test]
fn object_position_constant_hash() {
    for &policy in ColumnOrderPolicy::value_variants() {
        for &optimiser in Optimiser::value_variants() {
            let got = head_col(hash("Q(X) :- edge(X, c4).", optimiser, policy));
            assert_eq!(got, vec![2], "{optimiser:?} / {policy:?}");
        }
    }
}
