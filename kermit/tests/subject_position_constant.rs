//! Regression test for subject-position constants, covering **both** join
//! algorithms.
//!
//! A constant in a binary predicate's *first* (subject) attribute — e.g.
//! `Q(X) :- edge(c1, X).` ("successors of node 1") — must return the same
//! answer as the symmetric object-position query. This exercises
//! `kermit::db`'s `prepare` → `build` → `execute` end to end through the
//! harness's `JoinEntry`, where the global attribute order is *derived* from
//! the query and the constant is pushed into the filter of the index its atom
//! reads. The macro-generated join suites run through the same lifecycle, but
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
    common::utils::{catalog_of, join_under_planner, Inputs, JoinEntry},
    kermit_algos::{
        ColumnOrderPolicy, HashTriejoin, IndexSpec, JoinQuery, LeapfrogTriejoin, Optimiser, Planner,
    },
    kermit_ds::{Cardinality, Filter, HashTrie, Relation, RelationHeader, TreeTrie, Tuples},
    kermit_iters::SipHashStrategy,
    std::collections::BTreeMap,
};

type HashTrieSip = HashTrie<SipHashStrategy>;

const EDGES: [[usize; 2]; 3] = [[1, 2], [1, 3], [2, 4]];

fn edges() -> Vec<Vec<usize>> { EDGES.iter().map(|e| e.to_vec()).collect() }

/// `query` over edge = {(1,2), (1,3), (2,4)} as `R` through `JA`, planned
/// by `optimiser` under `policy`: the rows, and the index specs the query
/// built.
fn join_with_specs<R: Relation + Cardinality, JA: JoinEntry<R>>(
    query: &str, optimiser: Optimiser, policy: ColumnOrderPolicy,
) -> (Vec<Vec<usize>>, Vec<IndexSpec>) {
    let planner = Planner::new(optimiser.instantiate(), policy);
    let header = RelationHeader::new_positional("edge", 2);
    let inputs: Inputs = BTreeMap::from([("edge".to_string(), (header, Tuples::from(edges())))]);
    let catalog = catalog_of(&inputs, planner.required_statistics());
    let q: JoinQuery = query.parse().expect("parse");
    join_under_planner::<R, JA>(&catalog, q, &planner).unwrap()
}

/// The rows of [`join_with_specs`].
fn join<R: Relation + Cardinality, JA: JoinEntry<R>>(
    query: &str, optimiser: Optimiser, policy: ColumnOrderPolicy,
) -> Vec<Vec<usize>> {
    join_with_specs::<R, JA>(query, optimiser, policy).0
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

/// A constant or a repeated variable is not a relation to join: `prepare`
/// pushes it into the filter of the one index its atom reads, which keeps
/// only the columns that bind a variable. Each query binds one variable of
/// `edge`, so the index has one column whatever order a plan prefers.
#[test]
fn constants_and_repeats_become_the_filter_of_the_index_an_atom_reads() {
    let cases = [
        (
            "Q(X) :- edge(c1, X).",
            Filter::new(vec![(0, 1)], vec![]),
            vec![1],
        ),
        (
            "Q(X) :- edge(X, c4).",
            Filter::new(vec![(1, 4)], vec![]),
            vec![0],
        ),
        (
            "Q(X) :- edge(X, X).",
            Filter::new(vec![], vec![(0, 1)]),
            vec![0],
        ),
    ];
    for &policy in ColumnOrderPolicy::value_variants() {
        for &optimiser in Optimiser::value_variants() {
            for (query, filter, columns) in &cases {
                let want = vec![IndexSpec {
                    relation: "edge".to_string(),
                    filter: filter.clone(),
                    columns: columns.clone(),
                }];
                let (_, lftj) =
                    join_with_specs::<TreeTrie, LeapfrogTriejoin>(query, optimiser, policy);
                assert_eq!(lftj, want, "LFTJ {query}: {optimiser:?} / {policy:?}");
                let (_, hash) =
                    join_with_specs::<HashTrieSip, HashTriejoin>(query, optimiser, policy);
                assert_eq!(hash, want, "HTJ {query}: {optimiser:?} / {policy:?}");
            }
        }
    }
}
