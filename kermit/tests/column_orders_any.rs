//! Queries `--column-orders stored` rejects with `CyclicAttributeOrder`, or
//! reads only as stored, and `any` answers over reordered copies (issue
//! #93), on every alias × algorithm × optimiser. Each pattern pins the rows
//! `any` returns and, where the query is cyclic, that `stored` still
//! rejects it.

mod common;

use {
    common::utils::{join_under_planner, Inputs, JoinEntry},
    kermit::db::JoinError,
    kermit_algos::{
        CardinalityOptimiser, ColumnOrderPolicy, CostBasedOptimiser, HashTriejoin, IndexSpec,
        JoinQuery, LeapfrogTriejoin, LexicographicOptimiser, Planner, QueryOptimiser,
    },
    kermit_ds::{
        BinarySeek, Cardinality, ColumnTrie, GallopingSeek, HashTrie, LazyExpansion, LinearSeek,
        NoPruning, Relation, RelationHeader, SingletonPruning, TreeTrie,
    },
    kermit_iters::{FxHashStrategy, SipHashStrategy},
    std::collections::BTreeMap,
};

type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;
type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieFxPruned = HashTrie<FxHashStrategy, SingletonPruning>;
type HashTrieSipLazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;
type HashTrieFxLazy = HashTrie<FxHashStrategy, NoPruning, LazyExpansion>;
type HashTrieSipPrunedLazy = HashTrie<SipHashStrategy, SingletonPruning, LazyExpansion>;
type HashTrieFxPrunedLazy = HashTrie<FxHashStrategy, SingletonPruning, LazyExpansion>;
type TreeTrieLinear = TreeTrie<LinearSeek>;
type TreeTrieBinary = TreeTrie<BinarySeek>;
type TreeTrieGalloping = TreeTrie<GallopingSeek>;
type ColumnTrieLinear = ColumnTrie<LinearSeek>;
type ColumnTrieBinary = ColumnTrie<BinarySeek>;
type ColumnTrieGalloping = ColumnTrie<GallopingSeek>;

/// A pattern: relations as `(name, arity, tuples)`, the query, and the
/// sorted rows `any` must return.
struct Pattern {
    relations: Vec<(&'static str, usize, Vec<Vec<usize>>)>,
    query: &'static str,
    rows: Vec<Vec<usize>>,
    /// Whether `stored` rejects the query with `CyclicAttributeOrder`.
    cyclic_under_stored: bool,
}

fn mutual_edges() -> Pattern {
    Pattern {
        relations: vec![("edge", 2, vec![
            vec![1, 2],
            vec![2, 1],
            vec![2, 3],
            vec![3, 2],
            vec![1, 3],
        ])],
        query: "Q(X, Y) :- edge(X, Y), edge(Y, X).",
        rows: vec![vec![1, 2], vec![2, 1], vec![2, 3], vec![3, 2]],
        cyclic_under_stored: true,
    }
}

/// #82's incoming star: acyclic under `stored`, and under `any` the head
/// puts `V0` first, so `lexicographic` reorients both atoms.
fn incoming_star() -> Pattern {
    Pattern {
        relations: vec![
            ("includes", 2, vec![vec![10, 1], vec![11, 1], vec![12, 2]]),
            ("purchasefor", 2, vec![vec![20, 1], vec![21, 2], vec![
                22, 3,
            ]]),
        ],
        query: "Q(V0, V2, V7) :- includes(V2, V0), purchasefor(V7, V0).",
        rows: vec![vec![1, 10, 20], vec![1, 11, 20], vec![2, 12, 21]],
        cyclic_under_stored: false,
    }
}

fn ternary_cycle() -> Pattern {
    Pattern {
        relations: vec![
            ("r", 3, vec![vec![1, 2, 3], vec![4, 5, 6], vec![1, 7, 9]]),
            ("s", 2, vec![vec![3, 1], vec![6, 4], vec![9, 2]]),
        ],
        query: "Q(X, Y, Z) :- r(X, Y, Z), s(Z, X).",
        rows: vec![vec![1, 2, 3], vec![4, 5, 6]],
        cyclic_under_stored: true,
    }
}

/// A repeated variable on a reoriented atom: the selection's equality
/// must follow its columns through the permutation.
fn repeat_on_reoriented_atom() -> Pattern {
    Pattern {
        relations: vec![
            ("r", 3, vec![
                vec![1, 1, 5],
                vec![1, 2, 5],
                vec![2, 2, 6],
                vec![3, 3, 7],
            ]),
            ("s", 2, vec![vec![5, 1], vec![6, 2], vec![7, 9]]),
        ],
        query: "Q(X, Y) :- r(X, X, Y), s(Y, X).",
        rows: vec![vec![1, 5], vec![2, 6]],
        cyclic_under_stored: true,
    }
}

/// A constant on a reoriented atom: `s(Y, c7, X)` rewrites to
/// `s(Y, K0, X), Const_c7(K0)`, so `X → Y → K0 → X` is a cycle.
fn constant_on_reoriented_atom() -> Pattern {
    Pattern {
        relations: vec![
            ("r", 2, vec![vec![1, 2], vec![3, 4], vec![5, 6]]),
            ("s", 3, vec![vec![2, 7, 1], vec![4, 8, 3], vec![6, 7, 9]]),
        ],
        query: "Q(X, Y) :- r(X, Y), s(Y, c7, X).",
        rows: vec![vec![1, 2]],
        cyclic_under_stored: true,
    }
}

/// Runs `pattern` over `R` through `JA` under `O`: `stored` rejects or
/// agrees, `any` returns the pinned rows. Returns the specs `any` built.
fn check<R: Relation + Cardinality, JA: JoinEntry<R>, O: QueryOptimiser + Default + 'static>(
    pattern: &Pattern,
) -> Vec<IndexSpec> {
    let inputs: Inputs = pattern
        .relations
        .iter()
        .map(|(name, arity, tuples)| {
            let header = RelationHeader::new_positional(*name, *arity);
            (name.to_string(), (header, tuples.clone()))
        })
        .collect();
    let query: JoinQuery = pattern.query.parse().unwrap();
    // One database per policy, so the two runs share no state.
    let run = |policy: ColumnOrderPolicy| {
        let planner = Planner::new(Box::new(O::default()), policy);
        let store: BTreeMap<String, R> = inputs
            .iter()
            .map(|(name, (header, tuples))| {
                (name.clone(), R::from_tuples(header.clone(), tuples.clone()))
            })
            .collect();
        let mut database = JA::database(store, planner.required_statistics());
        join_under_planner::<R, JA>(&mut database, &inputs, query.clone(), &planner)
    };

    let stored = run(ColumnOrderPolicy::Stored);
    if pattern.cyclic_under_stored {
        assert!(
            matches!(stored, Err(JoinError::CyclicAttributeOrder { .. })),
            "{}: stored must reject a cyclic query, got {stored:?}",
            pattern.query
        );
    } else {
        let (mut rows, specs) = stored.unwrap_or_else(|e| panic!("{}: {e}", pattern.query));
        rows.sort();
        assert_eq!(rows, pattern.rows, "{} under stored", pattern.query);
        assert!(specs.is_empty(), "stored never reads a copy");
    }

    let (mut rows, specs) =
        run(ColumnOrderPolicy::Any).unwrap_or_else(|e| panic!("{}: {e}", pattern.query));
    rows.sort();
    assert_eq!(rows, pattern.rows, "{} under any", pattern.query);
    specs
}

macro_rules! any_only_patterns {
    ($( $relation:ident, $algo:ident, $optimiser:ident ),+ $(,)?) => {
        $(
            paste::paste! {
                mod [<any_ $relation:lower _ $algo:lower _ $optimiser:lower>] {
                    use super::*;

                    #[test]
                    fn mutual_edges() {
                        check::<$relation, $algo, $optimiser>(&super::mutual_edges());
                    }

                    #[test]
                    fn incoming_star() {
                        check::<$relation, $algo, $optimiser>(&super::incoming_star());
                    }

                    #[test]
                    fn ternary_cycle() {
                        check::<$relation, $algo, $optimiser>(&super::ternary_cycle());
                    }

                    #[test]
                    fn repeat_on_reoriented_atom() {
                        check::<$relation, $algo, $optimiser>(&super::repeat_on_reoriented_atom());
                    }

                    #[test]
                    fn constant_on_reoriented_atom() {
                        check::<$relation, $algo, $optimiser>(
                            &super::constant_on_reoriented_atom(),
                        );
                    }
                }
            }
        )+
    };
}

any_only_patterns!(
    TreeTrieLinear,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    TreeTrieLinear,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    TreeTrieLinear,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    TreeTrieBinary,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    TreeTrieBinary,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    TreeTrieBinary,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    TreeTrieGalloping,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    TreeTrieGalloping,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    TreeTrieGalloping,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    ColumnTrieLinear,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    ColumnTrieLinear,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    ColumnTrieLinear,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    ColumnTrieBinary,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    ColumnTrieBinary,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    ColumnTrieBinary,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    ColumnTrieGalloping,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    ColumnTrieGalloping,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    ColumnTrieGalloping,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    HashTrieSip,
    HashTriejoin,
    LexicographicOptimiser,
    HashTrieSip,
    HashTriejoin,
    CardinalityOptimiser,
    HashTrieSip,
    HashTriejoin,
    CostBasedOptimiser,
    HashTrieFx,
    HashTriejoin,
    LexicographicOptimiser,
    HashTrieFx,
    HashTriejoin,
    CardinalityOptimiser,
    HashTrieFx,
    HashTriejoin,
    CostBasedOptimiser,
    HashTrieSipPruned,
    HashTriejoin,
    LexicographicOptimiser,
    HashTrieSipPruned,
    HashTriejoin,
    CardinalityOptimiser,
    HashTrieSipPruned,
    HashTriejoin,
    CostBasedOptimiser,
    HashTrieFxPruned,
    HashTriejoin,
    LexicographicOptimiser,
    HashTrieFxPruned,
    HashTriejoin,
    CardinalityOptimiser,
    HashTrieFxPruned,
    HashTriejoin,
    CostBasedOptimiser,
    HashTrieSipLazy,
    HashTriejoin,
    LexicographicOptimiser,
    HashTrieSipLazy,
    HashTriejoin,
    CardinalityOptimiser,
    HashTrieSipLazy,
    HashTriejoin,
    CostBasedOptimiser,
    HashTrieFxLazy,
    HashTriejoin,
    LexicographicOptimiser,
    HashTrieFxLazy,
    HashTriejoin,
    CardinalityOptimiser,
    HashTrieFxLazy,
    HashTriejoin,
    CostBasedOptimiser,
    HashTrieSipPrunedLazy,
    HashTriejoin,
    LexicographicOptimiser,
    HashTrieSipPrunedLazy,
    HashTriejoin,
    CardinalityOptimiser,
    HashTrieSipPrunedLazy,
    HashTriejoin,
    CostBasedOptimiser,
    HashTrieFxPrunedLazy,
    HashTriejoin,
    LexicographicOptimiser,
    HashTrieFxPrunedLazy,
    HashTriejoin,
    CardinalityOptimiser,
    HashTrieFxPrunedLazy,
    HashTriejoin,
    CostBasedOptimiser,
);

/// Under `lexicographic` the incoming star reorients both atoms (the
/// head puts `V0` first), so `any` builds two copies; the mutual-edge
/// query builds one.
#[test]
fn lexicographic_builds_the_expected_copies() {
    let specs = check::<TreeTrie, LeapfrogTriejoin, LexicographicOptimiser>(&incoming_star());
    let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Index_1_0_includes", "Index_1_0_purchasefor"]);
    let specs = check::<HashTrieSip, HashTriejoin, LexicographicOptimiser>(&mutual_edges());
    assert_eq!(specs, vec![IndexSpec::new("edge", vec![1, 0])]);
}
