//! Join entry points bridging a parsed Datalog query to a relation store.
//!
//! Each iterator family has one streaming free function —
//! [`lftj_join_for_each`] for sorted tries under a [`TrieIterable`]-family
//! algorithm, [`hash_join_for_each`] for [`HashTrieIterable`] structures
//! under [`HashTriejoin`] — plus a collecting wrapper ([`lftj_join`],
//! [`hash_join`]), all over the same shape of relation store, a
//! `BTreeMap<String, R>` keyed by relation name.
//! Both share one private body and differ only in how a relation, a
//! constant and a selection view are wrapped for the algorithm (the
//! [`JoinFamily`] trait).
//!
//! Every entry point is total: a query that cannot run over the store
//! returns a [`JoinError`] rather than panicking, identically for both
//! families. [`validate_query`] runs the same checks without joining, for
//! callers that want to reject a query before building or timing anything.
//! Runtime selection of the (structure, algorithm) cell lives in the CLI's
//! `execution` module, not here.

mod database;
mod validation;

pub use {
    database::Database,
    validation::{validate_query, JoinError, RelationArities},
};
use {
    kermit_algos::{
        is_const_predicate, is_selection_predicate, CatalogStats, ColumnEquality, HashTrieIterKind,
        HashTriejoin, JoinAlgo, JoinQuery, QueryOptimiser, RelationStats, SingletonHashTrieIter,
        SingletonTrieIter, TrieIterKind,
    },
    kermit_ds::{Cardinality, Relation},
    kermit_iters::{
        HashStrategy, HashTrieIterable, HashTrieIterator, JoinIterable, TrieIterable,
        TrieIteratorWrapper,
    },
    std::collections::{BTreeMap, HashMap},
    validation::{prepare, Prepared},
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

    /// Lends every tuple stored in `relation` to `visit`, in the family's
    /// native order, without allocating per tuple. The catalog walks
    /// relations through it to count distinct values, so it needs no
    /// structure-specific code.
    fn for_each_tuple(relation: &R, visit: impl FnMut(&[usize]));
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

    fn for_each_tuple(relation: &R, mut visit: impl FnMut(&[usize])) {
        let mut tuples = TrieIteratorWrapper::new(relation.trie_iter());
        while let Some(tuple) = tuples.advance() {
            visit(tuple);
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

    fn for_each_tuple(relation: &R, mut visit: impl FnMut(&[usize])) {
        for_each_hash_tuple(relation.hash_trie_iter(), &mut visit);
    }
}

/// Depth-first walk of a hash trie through its [`HashTrieIterator`]: the
/// `open` / `next` / `up` / `leaf_tuples` contract Hash Triejoin itself
/// relies on, so it serves every hash-family relation, pruned singletons
/// included.
fn for_each_hash_tuple(mut iter: impl HashTrieIterator, visit: &mut impl FnMut(&[usize])) {
    // From before the root, `open` enters the root level, and fails only on
    // an empty relation. `depth` counts the levels entered, so the walk is
    // over once it climbs back out of the root.
    if !iter.open() {
        return;
    }
    let mut depth = 1;
    loop {
        let at_leaf = match iter.leaf_tuples() {
            | Some(chain) => {
                for tuple in chain {
                    visit(tuple);
                }
                true
            },
            | None => false,
        };
        // A built trie has no empty inner node, so `open` succeeds on every
        // inner bucket.
        if !at_leaf && iter.open() {
            depth += 1;
            continue;
        }
        // Advance to the next bucket, climbing out of each exhausted level.
        while iter.next().is_none() {
            iter.up();
            depth -= 1;
            if depth == 0 {
                return;
            }
        }
    }
}

/// The one join body: validation and the query rewrites ([`prepare`]),
/// wrapper map, statistics, plan, execute, project.
///
/// Each result tuple is passed to `emit` as a borrowed slice of exactly
/// the head's columns; the result is never materialised here.
///
/// # Errors
///
/// Returns the [`JoinError`] [`validate_query`] would, before any tuple is
/// emitted.
fn run_join<'a, R, F, JA, S>(
    relations: &'a BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    mut emit: S,
) -> Result<(), JoinError>
where
    R: Relation + Cardinality + 'a,
    F: JoinFamily<R>,
    JA: JoinAlgo<F::Wrapper<'a>>,
    S: FnMut(&[usize]),
{
    let head_len = query.head.terms.len();
    let Prepared {
        query: rewritten,
        const_specs,
        selection_specs,
    } = prepare(&query, relations)?;

    // `prepare` has checked that every relation the body names is present.
    let lookup = |name: &str| -> &'a R { &relations[name] };

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
        relations
            .get(base)
            .map(|r| RelationStats::new(r.tuple_count(), r.header().arity()))
    });
    let plan = optimiser.plan(&rewritten, &stats);

    // Projection to the head (#71). The executors emit every variable of
    // the rewritten query in canonical order, and `analyse` numbers the
    // head's variables `0..head_len` in head order — which holds because
    // `prepare` has checked that the head is distinct variables — so the
    // head's columns are each row's prefix. Lending that prefix allocates
    // nothing; duplicates stay (bag semantics), so counts are unchanged.
    JA::join_for_each(&plan, rewritten, ds_map, |row| emit(&row[..head_len]));
    Ok(())
}

/// Sorted-family join entry point: runs `query` over `relations` with the
/// [`TrieIterable`]-family algorithm `JA` (normally
/// [`LeapfrogTriejoin`](kermit_algos::LeapfrogTriejoin)), planned by
/// `optimiser`, and passes each result tuple to `emit` without
/// materialising the result. Mirror of [`hash_join_for_each`].
///
/// # Errors
///
/// Returns a [`JoinError`], before emitting anything, if the query cannot
/// run over `relations` (see [`validate_query`]).
pub fn lftj_join_for_each<R, JA>(
    relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    emit: impl FnMut(&[usize]),
) -> Result<(), JoinError>
where
    R: TrieIterable + Relation + Cardinality,
    JA: for<'a> JoinAlgo<TrieIterKind<'a, R>>,
{
    run_join::<R, SortedFamily, JA, _>(relations, query, optimiser, emit)
}

/// [`lftj_join_for_each`], collected: returns every result tuple. For
/// callers that need the rows themselves; allocates one `Vec` per tuple.
///
/// # Errors
///
/// As [`lftj_join_for_each`].
pub fn lftj_join<R, JA>(
    relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
) -> Result<Vec<Vec<usize>>, JoinError>
where
    R: TrieIterable + Relation + Cardinality,
    JA: for<'a> JoinAlgo<TrieIterKind<'a, R>>,
{
    let mut tuples = Vec::new();
    lftj_join_for_each::<R, JA>(relations, query, optimiser, |tuple| {
        tuples.push(tuple.to_vec())
    })?;
    Ok(tuples)
}

/// Hash-family join entry point: runs `query` over `relations` with
/// [`HashTriejoin`], planned by `optimiser`, and passes each result tuple
/// to `emit` without materialising the result. Mirror of
/// [`lftj_join_for_each`].
///
/// `H` selects the hash function used for any constant-atom singletons;
/// callers must thread the same `H` used when constructing the
/// `HashTrie<H>` relations so the algorithm sees matching hashes on both
/// sides of the intersection. Rust forbids defaults on free-function type
/// parameters (see issue #36887), so callers specify it via turbofish.
///
/// # Errors
///
/// Returns a [`JoinError`], before emitting anything, if the query cannot
/// run over `relations` (see [`validate_query`]).
pub fn hash_join_for_each<R, H>(
    relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
    emit: impl FnMut(&[usize]),
) -> Result<(), JoinError>
where
    R: HashTrieIterable + Relation + Cardinality,
    H: HashStrategy,
{
    run_join::<R, HashFamily<H>, HashTriejoin, _>(relations, query, optimiser, emit)
}

/// [`hash_join_for_each`], collected: returns every result tuple. For
/// callers that need the rows themselves; allocates one `Vec` per tuple.
///
/// # Errors
///
/// As [`hash_join_for_each`].
pub fn hash_join<R, H>(
    relations: &BTreeMap<String, R>, query: JoinQuery, optimiser: &dyn QueryOptimiser,
) -> Result<Vec<Vec<usize>>, JoinError>
where
    R: HashTrieIterable + Relation + Cardinality,
    H: HashStrategy,
{
    let mut tuples = Vec::new();
    hash_join_for_each::<R, H>(relations, query, optimiser, |tuple| {
        tuples.push(tuple.to_vec())
    })?;
    Ok(tuples)
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
        let mut got =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser)
                .unwrap();
        got.sort();
        assert_eq!(got, vec![vec![2], vec![3]]);
    }

    #[test]
    fn test_join_with_constant_filter() {
        let relations = rels(vec![("p", 2, vec![vec![1, 10], vec![2, 20], vec![3, 30]])]);
        let query: JoinQuery = "Q(X) :- p(X, c10).".parse().unwrap();
        let result =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser)
                .unwrap();
        let mut got = result;
        got.sort();
        assert_eq!(
            got,
            vec![vec![1]],
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
        let mut got =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser)
                .unwrap();
        got.sort();
        assert_eq!(got, vec![vec![1], vec![3]]);
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
        let mut got =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser)
                .unwrap();
        got.sort();
        assert_eq!(got, vec![vec![1], vec![3]]);
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
        let mut got =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser)
                .unwrap();
        got.sort();
        assert_eq!(got, vec![vec![1, 1], vec![1, 2], vec![3, 3]]);
    }

    /// `Q(X) :- r(X, _).` — a trailing placeholder is a fresh unused
    /// variable, so `X` repeats once per matching tuple (bag semantics).
    /// Before the placeholder rewrite the trailing level was never
    /// opened and each `X` appeared once.
    #[test]
    fn test_join_trailing_placeholder() {
        let relations = rels(vec![("r", 2, vec![vec![1, 2], vec![1, 3], vec![2, 4]])]);
        let query: JoinQuery = "Q(X) :- r(X, _).".parse().unwrap();
        let mut got =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser)
                .unwrap();
        got.sort();
        assert_eq!(got, vec![vec![1], vec![1], vec![2]]);
    }

    /// `Q(X, Y) :- r(X, _, Y).` — `Y` names the third column. Before the
    /// placeholder rewrite it bound the second (issue #73).
    #[test]
    fn test_join_middle_placeholder() {
        let relations = rels(vec![("r", 3, vec![vec![1, 2, 3], vec![1, 4, 5]])]);
        let query: JoinQuery = "Q(X, Y) :- r(X, _, Y).".parse().unwrap();
        let mut got =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser)
                .unwrap();
        got.sort();
        assert_eq!(got, vec![vec![1, 3], vec![1, 5]]);
    }

    /// Rows carry exactly the head's columns, in head order: body-only
    /// variables — written, or introduced by the const, placeholder and
    /// selection rewrites — are dropped, and duplicates are kept (bag
    /// semantics), so the row count is unchanged by projection (#71).
    #[test]
    fn results_are_projected_to_the_head() {
        let relations = rels(vec![
            ("r", 2, vec![vec![1, 1], vec![1, 2], vec![2, 3], vec![3, 3]]),
            ("p", 2, vec![vec![1, 10], vec![2, 20]]),
        ]);
        for (q, want) in [
            ("Q(X) :- r(X, Y).", vec![vec![1], vec![1], vec![2], vec![3]]),
            ("Q(Y) :- r(X, Y).", vec![vec![1], vec![2], vec![3], vec![3]]),
            ("Q(X) :- r(X, _).", vec![vec![1], vec![1], vec![2], vec![3]]),
            ("Q(X) :- p(X, c10).", vec![vec![1]]),
            ("Q(X) :- r(X, X).", vec![vec![1], vec![3]]),
            ("Q(Y, X) :- p(X, Y).", vec![vec![10, 1], vec![20, 2]]),
        ] {
            let query: JoinQuery = q.parse().unwrap();
            let mut got =
                lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser)
                    .unwrap();
            got.sort();
            assert_eq!(got, want, "{q}");
        }
    }

    /// The streaming entry point visits every row without collecting;
    /// `lftj_join` is the same traversal, collected.
    #[test]
    fn lftj_join_for_each_visits_every_row() {
        let relations = rels(vec![
            ("first", 1, vec![vec![1], vec![2], vec![3]]),
            ("second", 1, vec![vec![2], vec![3], vec![4]]),
        ]);
        let query: JoinQuery = "Q(X) :- first(X), second(X).".parse().unwrap();
        let mut got = Vec::new();
        lftj_join_for_each::<TreeTrie, LeapfrogTriejoin>(
            &relations,
            query,
            &LexicographicOptimiser,
            |row| got.push(row.to_vec()),
        )
        .unwrap();
        got.sort();
        assert_eq!(got, vec![vec![2], vec![3]]);
    }

    /// `missing` was never added. Silently dropping the atom would mask
    /// typos and load failures, and panicking exited 101; the entry point
    /// returns the error instead.
    #[test]
    fn test_join_errors_on_missing_relation() {
        let relations = rels(vec![("edge", 2, vec![vec![1, 2]])]);
        let query: JoinQuery = "Q(X) :- missing(X).".parse().unwrap();
        assert_eq!(
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&relations, query, &LexicographicOptimiser),
            Err(JoinError::UnknownRelation {
                relation: "missing".into(),
                known: vec!["edge".into()],
            })
        );
    }

    /// A rejected query emits nothing: validation runs before the join.
    #[test]
    fn a_rejected_query_emits_no_rows() {
        let relations = rels(vec![("edge", 2, vec![vec![1, 2], vec![3, 4]])]);
        let query: JoinQuery = "Q(X, Y) :- edge(X, Z).".parse().unwrap();
        let mut rows = 0;
        let result = lftj_join_for_each::<TreeTrie, LeapfrogTriejoin>(
            &relations,
            query,
            &LexicographicOptimiser,
            |_| rows += 1,
        );
        assert!(matches!(result, Err(JoinError::UnboundHeadVariable { .. })));
        assert_eq!(rows, 0);
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
        )
        .unwrap();
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
    /// The entry point projects each row to the head, so the rewrite's
    /// fresh `K0` column never reaches the caller — mirroring
    /// [`tests::test_join_with_constant_filter`] for LFTJ above.
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
        )
        .unwrap();
        let mut got = result;
        got.sort();
        assert_eq!(
            got,
            vec![vec![1], vec![2]],
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
        )
        .unwrap();
        let mut got = result;
        got.sort();
        assert_eq!(got, vec![vec![1], vec![3]]);
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
        )
        .unwrap();
        let mut got = result;
        got.sort();
        assert_eq!(got, vec![vec![1], vec![3]]);
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
        )
        .unwrap();
        let mut got = result;
        got.sort();
        assert_eq!(got, vec![vec![1, 1], vec![1, 2], vec![3, 3]]);
    }

    /// Mirror of [`tests::test_join_trailing_placeholder`]. Before the
    /// placeholder rewrite this panicked in `emit_leaf` ("at leaf level
    /// for every participating iter").
    #[test]
    fn hash_join_trailing_placeholder() {
        let mut relations: BTreeMap<String, HashTrie> = BTreeMap::new();
        relations.insert(
            "r".to_string(),
            HashTrie::from_tuples(2.into(), vec![vec![1, 2], vec![1, 3], vec![2, 4]]),
        );
        let q: JoinQuery = "Q(X) :- r(X, _).".parse().unwrap();
        let result = hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(
            &relations,
            q,
            &LexicographicOptimiser,
        )
        .unwrap();
        let mut got = result;
        got.sort();
        assert_eq!(got, vec![vec![1], vec![1], vec![2]]);
    }

    /// Mirror of [`tests::test_join_middle_placeholder`]; panicked at the
    /// same site as the trailing case.
    #[test]
    fn hash_join_middle_placeholder() {
        let mut relations: BTreeMap<String, HashTrie> = BTreeMap::new();
        relations.insert(
            "r".to_string(),
            HashTrie::from_tuples(3.into(), vec![vec![1, 2, 3], vec![1, 4, 5]]),
        );
        let q: JoinQuery = "Q(X, Y) :- r(X, _, Y).".parse().unwrap();
        let result = hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(
            &relations,
            q,
            &LexicographicOptimiser,
        )
        .unwrap();
        let mut got = result;
        got.sort();
        assert_eq!(got, vec![vec![1, 3], vec![1, 5]]);
    }

    /// Every #78 shape is rejected by the hash family with exactly the
    /// error the sorted family gives: validation runs before either
    /// family's code.
    #[test]
    fn both_families_reject_malformed_queries_identically() {
        use {crate::db::lftj_join, kermit_algos::LeapfrogTriejoin, kermit_ds::TreeTrie};
        let edges = vec![vec![1, 2], vec![2, 1], vec![2, 3]];
        let hash: BTreeMap<String, HashTrie> = BTreeMap::from([(
            "edge".into(),
            HashTrie::from_tuples(2.into(), edges.clone()),
        )]);
        let sorted: BTreeMap<String, TreeTrie> =
            BTreeMap::from([("edge".into(), TreeTrie::from_tuples(2.into(), edges))]);
        for q in [
            "Q(X, Y) :- edge(X, Z).",
            "Q(X, X) :- edge(X, Y).",
            "Q(X, Y, Z) :- edge(X, Y, Z).",
            "Q(X) :- edge(X).",
            "Q(X, Y) :- nope(X, Y).",
            "Q(_) :- edge(X, Y).",
            "Q(X) :- edge(X, cfoo).",
            "Q(X) :- Const_c5(X).",
            "Q(X, Y) :- edge(X, Y), edge(Y, X).",
        ] {
            let query: JoinQuery = q.parse().unwrap();
            let from_hash = hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(
                &hash,
                query.clone(),
                &LexicographicOptimiser,
            );
            let from_sorted =
                lftj_join::<TreeTrie, LeapfrogTriejoin>(&sorted, query, &LexicographicOptimiser);
            assert!(from_hash.is_err(), "{q} was accepted");
            assert_eq!(from_hash, from_sorted, "{q}");
        }
    }

    /// Mirror of [`tests::results_are_projected_to_the_head`].
    #[test]
    fn hash_join_results_are_projected_to_the_head() {
        let mut relations: BTreeMap<String, HashTrie> = BTreeMap::new();
        relations.insert(
            "r".to_string(),
            HashTrie::from_tuples(2.into(), vec![vec![1, 1], vec![1, 2], vec![2, 3], vec![
                3, 3,
            ]]),
        );
        relations.insert(
            "p".to_string(),
            HashTrie::from_tuples(2.into(), vec![vec![1, 10], vec![2, 20]]),
        );
        for (q, want) in [
            ("Q(X) :- r(X, Y).", vec![vec![1], vec![1], vec![2], vec![3]]),
            ("Q(Y) :- r(X, Y).", vec![vec![1], vec![2], vec![3], vec![3]]),
            ("Q(X) :- r(X, _).", vec![vec![1], vec![1], vec![2], vec![3]]),
            ("Q(X) :- p(X, c10).", vec![vec![1]]),
            ("Q(X) :- r(X, X).", vec![vec![1], vec![3]]),
            ("Q(Y, X) :- p(X, Y).", vec![vec![10, 1], vec![20, 2]]),
        ] {
            let query: JoinQuery = q.parse().unwrap();
            let mut got = hash_join::<HashTrie<SipHashStrategy>, SipHashStrategy>(
                &relations,
                query,
                &LexicographicOptimiser,
            )
            .unwrap();
            got.sort();
            assert_eq!(got, want, "{q}");
        }
    }

    #[test]
    fn hash_join_for_each_visits_every_row() {
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
        let mut got = Vec::new();
        hash_join_for_each::<HashTrie<SipHashStrategy>, SipHashStrategy>(
            &relations,
            q,
            &LexicographicOptimiser,
            |row| got.push(row.to_vec()),
        )
        .unwrap();
        got.sort();
        assert_eq!(got, vec![vec![2], vec![3]]);
    }
}

#[cfg(test)]
mod family_walk_tests {
    use {
        super::*,
        kermit_ds::{ColumnTrie, HashTrie, NoPruning, PruningPolicy, SingletonPruning, TreeTrie},
        kermit_iters::SipHashStrategy,
    };

    fn walked<R, F: JoinFamily<R>>(relation: &R) -> Vec<Vec<usize>> {
        let mut tuples = Vec::new();
        F::for_each_tuple(relation, |tuple| tuples.push(tuple.to_vec()));
        tuples
    }

    /// Shared prefixes and single-tuple subtries, so the pruned hash trie
    /// stores some subtries as `Singleton`s.
    fn tuples() -> Vec<Vec<usize>> {
        vec![
            vec![1, 10, 100],
            vec![1, 10, 101],
            vec![1, 20, 100],
            vec![2, 30, 300],
            vec![3, 40, 400],
        ]
    }

    #[test]
    fn sorted_walk_visits_every_tuple_in_order() {
        let tree: TreeTrie = TreeTrie::from_tuples(3.into(), tuples());
        assert_eq!(walked::<_, SortedFamily>(&tree), tuples());
        let column: ColumnTrie = ColumnTrie::from_tuples(3.into(), tuples());
        assert_eq!(walked::<_, SortedFamily>(&column), tuples());
    }

    /// The family walk goes through `HashTrieIterator` and must visit
    /// exactly what the trie's own `for_each_tuple` visits.
    fn assert_hash_walk_matches_the_trie<P: PruningPolicy>(tuples: Vec<Vec<usize>>) {
        let trie: HashTrie<SipHashStrategy, P> = HashTrie::from_tuples(3.into(), tuples);
        let mut expected = Vec::new();
        trie.for_each_tuple(|tuple| expected.push(tuple.to_vec()));
        let mut got = walked::<_, HashFamily<SipHashStrategy>>(&trie);
        expected.sort();
        got.sort();
        assert_eq!(got, expected);
    }

    #[test]
    fn hash_walk_visits_what_the_trie_stores() {
        assert_hash_walk_matches_the_trie::<NoPruning>(tuples());
        assert_hash_walk_matches_the_trie::<SingletonPruning>(tuples());
        // The hash trie is a multiset: a duplicate is visited twice.
        let mut duplicated = tuples();
        duplicated.push(vec![1, 10, 100]);
        assert_hash_walk_matches_the_trie::<NoPruning>(duplicated.clone());
        assert_hash_walk_matches_the_trie::<SingletonPruning>(duplicated);
        assert_hash_walk_matches_the_trie::<NoPruning>(Vec::new());
        assert_hash_walk_matches_the_trie::<SingletonPruning>(Vec::new());
    }
}
