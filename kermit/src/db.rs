//! Join entry points bridging a parsed Datalog query to a relation store.
//!
//! Each iterator family has one free function — [`lftj_join`] for sorted
//! tries under a [`TrieIterable`]-family algorithm, [`hash_join`] for
//! [`HashTrieIterable`] structures under [`HashTriejoin`] — over the same
//! shape of relation store, a `BTreeMap<String, R>` keyed by relation name.
//! Both share one private body and differ only in how a relation, a
//! constant and a selection view are wrapped for the algorithm (the
//! [`JoinFamily`] trait).
//! Runtime selection of the (structure, algorithm) cell lives in the CLI's
//! `execution` module, not here.

use {
    kermit_algos::{
        is_const_predicate, is_selection_predicate, rewrite_atoms, rewrite_repeated_variables,
        CatalogStats, ColumnEquality, HashTrieIterKind, HashTriejoin, JoinAlgo, JoinQuery,
        QueryOptimiser, SingletonHashTrieIter, SingletonTrieIter, TrieIterKind,
    },
    kermit_ds::Cardinality,
    kermit_iters::{HashStrategy, HashTrieIterable, JoinIterable, TrieIterable},
    std::collections::{BTreeMap, HashMap},
};

/// How one iterator family wraps a stored relation, a constant atom and an
/// equality-selection view into the [`JoinIterable`] its algorithms
/// consume.
///
/// The two wrapper enums ([`TrieIterKind`] and [`HashTrieIterKind`]) are
/// structurally identical; the only real asymmetry is that the hash family
/// must hash the constant with the same [`HashStrategy`] its relations were
/// built with, which is why the hash implementor carries `H`.
pub trait JoinFamily<R> {
    /// The wrapper handed to the algorithm; borrows the relation for `'a`.
    type Wrapper<'a>: JoinIterable
    where
        R: 'a;

    /// Wraps a borrowed relation.
    fn wrap_relation(relation: &R) -> Self::Wrapper<'_>;

    /// Wraps the singleton relation `{id}` standing in for a constant atom.
    fn wrap_const<'a>(id: usize) -> Self::Wrapper<'a>
    where
        R: 'a;

    /// Wraps a borrowed relation viewed through column `equalities`, standing
    /// in for an atom that repeated a variable.
    fn wrap_selection(relation: &R, equalities: Vec<ColumnEquality>) -> Self::Wrapper<'_>;
}

/// [`JoinFamily`] for sorted tries: [`TrieIterKind`] wrappers.
pub struct SortedFamily;

impl<R: TrieIterable> JoinFamily<R> for SortedFamily {
    type Wrapper<'a>
        = TrieIterKind<'a, R>
    where
        R: 'a;

    fn wrap_relation(relation: &R) -> Self::Wrapper<'_> { TrieIterKind::Relation(relation) }

    fn wrap_const<'a>(id: usize) -> Self::Wrapper<'a>
    where
        R: 'a,
    {
        TrieIterKind::Singleton(SingletonTrieIter::new(id))
    }

    fn wrap_selection(relation: &R, equalities: Vec<ColumnEquality>) -> Self::Wrapper<'_> {
        TrieIterKind::Selection {
            relation,
            equalities,
        }
    }
}

/// [`JoinFamily`] for hash tries: [`HashTrieIterKind`] wrappers whose
/// constant singletons are hashed with `H`.
pub struct HashFamily<H>(std::marker::PhantomData<H>);

impl<R: HashTrieIterable, H: HashStrategy> JoinFamily<R> for HashFamily<H> {
    type Wrapper<'a>
        = HashTrieIterKind<'a, R>
    where
        R: 'a;

    fn wrap_relation(relation: &R) -> Self::Wrapper<'_> { HashTrieIterKind::Relation(relation) }

    fn wrap_const<'a>(id: usize) -> Self::Wrapper<'a>
    where
        R: 'a,
    {
        HashTrieIterKind::Singleton(SingletonHashTrieIter::new(id, H::hash(id)))
    }

    fn wrap_selection(relation: &R, equalities: Vec<ColumnEquality>) -> Self::Wrapper<'_> {
        HashTrieIterKind::Selection {
            relation,
            equalities,
        }
    }
}

/// The one join body: const-view rewrite, selection rewrite, wrapper map,
/// statistics, plan, execute.
///
/// `label` names the entry point in the unknown-relation panic so a CLI
/// user can tell which family rejected the query.
///
/// # Panics
///
/// Panics if the query references a relation name not present in
/// `relations`, or if it contains a malformed constant atom.
fn run_join<'a, R, F, JA>(
    relations: &'a BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    label: &str,
) -> Vec<Vec<usize>>
where
    R: Cardinality + 'a,
    F: JoinFamily<R>,
    JA: JoinAlgo<F::Wrapper<'a>>,
{
    // Const first, then selection: the second pass then sees an atom-free
    // body, and the two share one fresh-variable counter.
    let (rewritten, const_specs) = rewrite_atoms(query).expect("malformed constant atom in query");
    let (rewritten, selection_specs) = rewrite_repeated_variables(rewritten);

    let lookup = |name: &str| -> &'a R {
        relations.get(name).unwrap_or_else(|| {
            panic!(
                "{label}: query body references unknown relation {name:?}; known relations: {:?}",
                relations.keys().collect::<Vec<_>>(),
            )
        })
    };

    let mut wrappers: HashMap<String, F::Wrapper<'a>> = HashMap::new();
    for pred in &rewritten.body {
        if wrappers.contains_key(&pred.name) {
            continue;
        }
        // Const_* and Select_* predicates are synthetic — created by the
        // rewrites above and materialised from their specs below. They
        // aren't expected to live in `relations`.
        if is_const_predicate(&pred.name) || is_selection_predicate(&pred.name) {
            continue;
        }
        wrappers.insert(pred.name.clone(), F::wrap_relation(lookup(&pred.name)));
    }
    for (name, id) in const_specs {
        wrappers.entry(name).or_insert_with(|| F::wrap_const(id));
    }
    for spec in &selection_specs {
        let base = lookup(&spec.relation);
        wrappers.insert(
            spec.name.clone(),
            F::wrap_selection(base, spec.equalities.clone()),
        );
    }

    let ds_map: HashMap<String, &F::Wrapper<'a>> =
        wrappers.iter().map(|(k, v)| (k.clone(), v)).collect();

    // Stats + planning run per join — inside benchmarks' measured region —
    // so this stays O(#predicates) on top of O(1) tuple_count() reads. A
    // selection view reports its base relation's count: an upper bound,
    // which is the conservative value for a cardinality-driven planner.
    let base_of: HashMap<&str, &str> = selection_specs
        .iter()
        .map(|s| (s.name.as_str(), s.relation.as_str()))
        .collect();
    let stats = CatalogStats::for_query(&rewritten, |name| {
        let base = base_of.get(name).copied().unwrap_or(name);
        relations.get(base).map(Cardinality::tuple_count)
    });
    let plan = optimiser.plan(&rewritten, &stats);

    JA::join_iter(&plan, rewritten, ds_map).collect()
}

/// Sorted-family join entry point: runs `query` over `relations` with the
/// [`TrieIterable`]-family algorithm `JA` (normally
/// [`LeapfrogTriejoin`](kermit_algos::LeapfrogTriejoin)), planned by
/// `optimiser`. Mirror of [`hash_join`].
///
/// # Panics
///
/// Panics if the query references a relation name not present in
/// `relations`, or if it contains a malformed constant atom.
pub fn lftj_join<R, JA>(
    relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
) -> Vec<Vec<usize>>
where
    R: TrieIterable + Cardinality,
    JA: for<'a> JoinAlgo<TrieIterKind<'a, R>>,
{
    run_join::<R, SortedFamily, JA>(relations, query, optimiser, "lftj_join")
}

/// Hash-family join entry point: runs `query` over `relations` with
/// [`HashTriejoin`], planned by `optimiser`. Mirror of [`lftj_join`].
///
/// `H` selects the hash function used for any constant-atom singletons;
/// callers must thread the same `H` used when constructing the
/// `HashTrie<H>` relations so the algorithm sees matching hashes on both
/// sides of the intersection. Rust forbids defaults on free-function type
/// parameters (see issue #36887), so callers specify it via turbofish.
///
/// # Panics
///
/// Panics if the query references a relation name not present in
/// `relations`, or if it contains a malformed constant atom.
pub fn hash_join<R, H>(
    relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
) -> Vec<Vec<usize>>
where
    R: HashTrieIterable + Cardinality,
    H: HashStrategy,
{
    run_join::<R, HashFamily<H>, HashTriejoin>(relations, query, optimiser, "hash_join")
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_algos::{JoinQuery, LeapfrogTriejoin, LexicographicOptimiser},
        kermit_ds::{Relation, TreeTrie},
    };

    fn rels(entries: Vec<(&str, usize, Vec<Vec<usize>>)>) -> BTreeMap<String, TreeTrie> {
        entries
            .into_iter()
            .map(|(name, arity, tuples)| {
                (
                    name.to_string(),
                    TreeTrie::from_tuples(arity.into(), tuples),
                )
            })
            .collect()
    }

    #[test]
    fn test_join() {
        let relations = rels(vec![
            ("first", 1, vec![vec![1], vec![2], vec![3]]),
            ("second", 1, vec![vec![2], vec![3], vec![4]]),
        ]);
        let query: JoinQuery = "Q(X) :- first(X), second(X).".parse().unwrap();
        let mut got: Vec<usize> =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser)
                .iter()
                .map(|r| r[0])
                .collect();
        got.sort();
        assert_eq!(got, vec![2, 3]);
    }

    #[test]
    fn test_join_with_constant_filter() {
        let relations = rels(vec![("p", 2, vec![vec![1, 10], vec![2, 20], vec![3, 30]])]);
        let query: JoinQuery = "Q(X) :- p(X, c10).".parse().unwrap();
        let result =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser);
        let mut got: Vec<_> = result.iter().map(|r| r[0]).collect();
        got.sort();
        assert_eq!(
            got,
            vec![1],
            "expected only X=1 to pass the c10 filter, got {got:?}"
        );
    }

    /// `Q(X) :- r(X, X).` — the PROBLEMS.md repro. Before the selection
    /// rewrite this returned every first-column key of `r`.
    #[test]
    fn test_join_diagonal() {
        let relations = rels(vec![("r", 2, vec![
            vec![1, 1],
            vec![1, 2],
            vec![2, 3],
            vec![3, 3],
            vec![4, 5],
        ])]);
        let query: JoinQuery = "Q(X) :- r(X, X).".parse().unwrap();
        let mut got: Vec<usize> =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser)
                .iter()
                .map(|r| r[0])
                .collect();
        got.sort();
        assert_eq!(got, vec![1, 3]);
    }

    /// Both rewrites on one atom: the constant takes `K0`, the repeat
    /// `K1`, and the view enforces column 0 = column 2.
    #[test]
    fn test_join_const_and_repeat() {
        let relations = rels(vec![("r", 3, vec![
            vec![1, 5, 1],
            vec![1, 5, 2],
            vec![2, 6, 2],
            vec![3, 5, 3],
        ])]);
        let query: JoinQuery = "Q(X) :- r(X, c5, X).".parse().unwrap();
        let mut got: Vec<usize> =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser)
                .iter()
                .map(|r| r[0])
                .collect();
        got.sort();
        assert_eq!(got, vec![1, 3]);
    }

    /// Two views of one relation: `r(X, X)` becomes a selection view while
    /// `r(X, Y)` stays the base relation.
    #[test]
    fn test_join_mixed_occurrences() {
        let relations = rels(vec![("r", 2, vec![
            vec![1, 1],
            vec![1, 2],
            vec![2, 3],
            vec![3, 3],
        ])]);
        let query: JoinQuery = "Q(X, Y) :- r(X, X), r(X, Y).".parse().unwrap();
        let mut got: Vec<(usize, usize)> =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser)
                .iter()
                .map(|r| (r[0], r[1]))
                .collect();
        got.sort();
        assert_eq!(got, vec![(1, 1), (1, 2), (3, 3)]);
    }

    #[test]
    #[should_panic(expected = "unknown relation")]
    fn test_join_panics_on_missing_relation() {
        let relations: BTreeMap<String, TreeTrie> = BTreeMap::new();
        // `missing` was never added; previously the body predicate was
        // silently dropped, which could mask typos or load failures.
        let query: JoinQuery = "Q(X) :- missing(X).".parse().unwrap();
        lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser);
    }
}

#[cfg(test)]
mod hash_join_tests {
    use {
        super::*,
        kermit_algos::LexicographicOptimiser,
        kermit_ds::{HashTrie, Relation},
        kermit_iters::SipHashStrategy,
    };

    /// Pins the basic happy path: build two unary `HashTrie`s, run a
    /// straight intersection through the free function, verify the
    /// rewrite + dispatch + collect chain end-to-end.
    #[test]
    fn hash_join_unary_intersection() {
        let mut relations: BTreeMap<String, HashTrie> = BTreeMap::new();
        relations.insert(
            "R".to_string(),
            HashTrie::from_tuples(1.into(), vec![vec![1], vec![2], vec![3]]),
        );
        relations.insert(
            "S".to_string(),
            HashTrie::from_tuples(1.into(), vec![vec![2], vec![3], vec![4]]),
        );
        let q: JoinQuery = "Q(X) :- R(X), S(X).".parse().unwrap();
        let mut out = hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(
            &relations,
            q,
            &LexicographicOptimiser,
        );
        out.sort();
        assert_eq!(out, vec![vec![2], vec![3]]);
    }

    /// `Q(X) :- R(X, c5).` — pins the const-view rewrite path through
    /// `SingletonHashTrieIter`. The rewrite turns the atom `c5` into a
    /// synthetic unary predicate `Const_c5` backed by a singleton, which
    /// `hash_join` materialises into a [`HashTrieIterKind::Singleton`]
    /// wrapper alongside the relation.
    ///
    /// We deliberately place the constant in the trailing column. The
    /// hash-trie iter family descends through *physical* attribute positions
    /// in lockstep with the plan's *variable* ordering. `analyse` assigns the
    /// canonical numbering ("head vars first, then any extra body vars");
    /// the descent ordering is whatever the optimiser's plan returns, and for
    /// this trailing-constant shape the lexicographic plan coincides with the
    /// canonical numbering. The rewrite's fresh `K0` therefore lands at the
    /// tail of the variable ordering, matching the trie's physical layout.
    /// The LFTJ const test uses the same shape for the same reason.
    ///
    /// The algorithm emits tuples in `variable_ordering` order (all
    /// query variables, not just head vars), so we project to the head
    /// slot ourselves — mirroring [`tests::test_join_with_constant_filter`]
    /// for LFTJ above.
    #[test]
    fn hash_join_with_constant_atom() {
        let mut relations: BTreeMap<String, HashTrie> = BTreeMap::new();
        relations.insert(
            "R".to_string(),
            HashTrie::from_tuples(2.into(), vec![vec![1, 5], vec![2, 5], vec![3, 7]]),
        );
        let q: JoinQuery = "Q(X) :- R(X, c5).".parse().unwrap();
        let result = hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(
            &relations,
            q,
            &LexicographicOptimiser,
        );
        let mut got: Vec<usize> = result.iter().map(|r| r[0]).collect();
        got.sort();
        assert_eq!(
            got,
            vec![1, 2],
            "expected only X=1, X=2 to pass the c5 filter, got {got:?}"
        );
    }

    /// `Q(X) :- r(X, X).` — the PROBLEMS.md repro. Before the selection
    /// rewrite this panicked in `emit_leaf` ("at leaf level for every
    /// participating iter") because `r` was opened only once.
    #[test]
    fn hash_join_diagonal() {
        let mut relations: BTreeMap<String, HashTrie> = BTreeMap::new();
        relations.insert(
            "r".to_string(),
            HashTrie::from_tuples(2.into(), vec![
                vec![1, 1],
                vec![1, 2],
                vec![2, 3],
                vec![3, 3],
                vec![4, 5],
            ]),
        );
        let q: JoinQuery = "Q(X) :- r(X, X).".parse().unwrap();
        let result = hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(
            &relations,
            q,
            &LexicographicOptimiser,
        );
        let mut got: Vec<usize> = result.iter().map(|r| r[0]).collect();
        got.sort();
        assert_eq!(got, vec![1, 3]);
    }

    #[test]
    fn hash_join_const_and_repeat() {
        let mut relations: BTreeMap<String, HashTrie> = BTreeMap::new();
        relations.insert(
            "r".to_string(),
            HashTrie::from_tuples(3.into(), vec![
                vec![1, 5, 1],
                vec![1, 5, 2],
                vec![2, 6, 2],
                vec![3, 5, 3],
            ]),
        );
        let q: JoinQuery = "Q(X) :- r(X, c5, X).".parse().unwrap();
        let result = hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(
            &relations,
            q,
            &LexicographicOptimiser,
        );
        let mut got: Vec<usize> = result.iter().map(|r| r[0]).collect();
        got.sort();
        assert_eq!(got, vec![1, 3]);
    }

    #[test]
    fn hash_join_mixed_occurrences() {
        let mut relations: BTreeMap<String, HashTrie> = BTreeMap::new();
        relations.insert(
            "r".to_string(),
            HashTrie::from_tuples(2.into(), vec![vec![1, 1], vec![1, 2], vec![2, 3], vec![
                3, 3,
            ]]),
        );
        let q: JoinQuery = "Q(X, Y) :- r(X, X), r(X, Y).".parse().unwrap();
        let result = hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(
            &relations,
            q,
            &LexicographicOptimiser,
        );
        let mut got: Vec<(usize, usize)> = result.iter().map(|r| (r[0], r[1])).collect();
        got.sort();
        assert_eq!(got, vec![(1, 1), (1, 2), (3, 3)]);
    }
}
